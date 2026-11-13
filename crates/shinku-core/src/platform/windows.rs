//! La machine, sous Windows.
//!
//! Chaque question posée par le moteur correspond à un appel documenté de
//! l'API Windows, sans droits particuliers. Rien ici ne modifie la machine,
//! à part la priorité du fil d'exécution courant.
#![allow(unsafe_code)]

use std::ffi::{OsString, c_void};
use std::fs::OpenOptions;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery, STORAGE_PROPERTY_QUERY,
    StorageDeviceSeekPenaltyProperty,
};
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN, THREAD_MODE_BACKGROUND_END,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_LocalAppData,
    FOLDERID_LocalAppDataLow, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Profile,
    FOLDERID_ProgramData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86,
    FOLDERID_RoamingAppData, FOLDERID_SkyDrive, FOLDERID_Videos, FOLDERID_Windows,
    KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath, SHQUERYRBINFO, SHQueryRecycleBinW,
};
use windows::core::{GUID, HSTRING};

use super::{Environment, KnownFolder, RecycleBin, Volume};
use crate::fs::{fold_path, to_internal_path};
use crate::model::Timestamp;

/// Valeur de `GetDriveTypeW` pour un disque interne.
const DRIVE_FIXED: u32 = 3;
const FILE_SHARE_ALL: u32 = 0x7;

#[derive(Debug)]
pub(super) struct SystemEnvironment;

impl SystemEnvironment {
    pub(super) fn new() -> Self {
        Self
    }
}

/// Demande à Windows le chemin d'un dossier connu.
fn known_folder_path(id: &GUID) -> Option<PathBuf> {
    // SAFETY: `id` pointe vers un GUID valide pendant l'appel. Sans jeton
    // (`None`), Windows répond pour l'utilisateur courant.
    let raw = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DONT_VERIFY, None) }.ok()?;
    // SAFETY: en cas de succès, `raw` est une chaîne UTF-16 terminée par zéro,
    // allouée par Windows et valide jusqu'à sa libération ci-dessous.
    let text = OsString::from_wide(unsafe { raw.as_wide() });
    // SAFETY: la chaîne vient de `SHGetKnownFolderPath`, qui demande de la
    // libérer avec `CoTaskMemFree`. Elle n'est plus utilisée ensuite.
    unsafe { CoTaskMemFree(Some(raw.0.cast::<c_void>().cast_const())) };
    to_internal_path(Path::new(&text)).ok()
}

/// Lit une chaîne UTF-16 terminée par zéro dans un tampon.
fn text_from(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// Demande au disque s'il a un temps de positionnement (disque dur à plateaux).
fn seek_penalty(letter: char) -> Option<bool> {
    // Sans aucun droit d'accès demandé, cette ouverture est permise à tout
    // utilisateur : elle ne sert qu'à interroger le périphérique.
    let device = OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_ALL)
        .open(format!(r"\\.\{letter}:"))
        .ok()?;
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    // Réponse attendue : DEVICE_SEEK_PENALTY_DESCRIPTOR, soit deux entiers de
    // 32 bits (version, taille) puis un octet booléen. On la reçoit dans un
    // tampon brut pour ne rien supposer de la valeur de cet octet.
    let mut answer = [0u32; 3];
    let mut returned = 0u32;
    // SAFETY: le handle appartient à `device`, vivant pendant l'appel. Les
    // tampons d'entrée et de sortie sont des variables locales dont on
    // annonce la taille exacte ; l'appel est synchrone (pas d'OVERLAPPED).
    unsafe {
        DeviceIoControl(
            HANDLE(device.as_raw_handle()),
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&raw const query).cast::<c_void>()),
            size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some((&raw mut answer).cast::<c_void>()),
            size_of_val(&answer) as u32,
            Some(&raw mut returned),
            None,
        )
    }
    .ok()?;
    (returned >= 9).then(|| answer[2] & 0xFF != 0)
}

fn volume_for(letter: char, system_drive: Option<char>) -> Option<Volume> {
    let root_text = format!(r"{letter}:\");
    let root_wide = HSTRING::from(root_text.as_str());

    // SAFETY: `root_wide` est une chaîne UTF-16 terminée par zéro, vivante
    // pendant l'appel.
    let kind = unsafe { GetDriveTypeW(&root_wide) };
    if kind != DRIVE_FIXED {
        // Amovible, réseau, optique : hors du champ de Shinku en version 1.
        return None;
    }

    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: chaîne valide comme ci-dessus ; les deux pointeurs désignent des
    // variables locales vivantes.
    unsafe { GetDiskFreeSpaceExW(&root_wide, Some(&raw mut free), Some(&raw mut total), None) }
        .ok()?;

    let mut label = [0u16; 261];
    let mut file_system = [0u16; 261];
    // SAFETY: chaîne valide comme ci-dessus ; les tampons sont des tableaux
    // locaux dont la crate transmet la longueur exacte.
    let named = unsafe {
        GetVolumeInformationW(
            &root_wide,
            Some(&mut label),
            None,
            None,
            None,
            Some(&mut file_system),
        )
    };

    Some(Volume {
        root: to_internal_path(Path::new(&root_text)).ok()?,
        label: if named.is_ok() { text_from(&label) } else { String::new() },
        file_system: if named.is_ok() { text_from(&file_system) } else { String::new() },
        total,
        free: free.min(total),
        fixed: true,
        seek_penalty: seek_penalty(letter),
        system: system_drive == Some(letter),
    })
}

/// Lettre du lecteur d'un chemin interne (`\\?\C:\…`), en majuscule.
fn drive_letter(path: &Path) -> Option<char> {
    let drive = fold_path(path)?.into_iter().next()?;
    let mut characters = drive.chars();
    match (characters.next(), characters.next(), characters.next()) {
        (Some(letter), Some(':'), None) if letter.is_ascii_alphabetic() => {
            Some(letter.to_ascii_uppercase())
        }
        _ => None,
    }
}

fn fixed_drive_letters() -> impl Iterator<Item = char> {
    // SAFETY: appel sans paramètre, qui rend un masque de bits.
    let mask = unsafe { GetLogicalDrives() };
    ('A'..='Z').enumerate().filter(move |(index, _)| mask & (1 << index) != 0).map(|(_, l)| l)
}

impl Environment for SystemEnvironment {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        let id = match folder {
            KnownFolder::Profile => &FOLDERID_Profile,
            KnownFolder::Desktop => &FOLDERID_Desktop,
            KnownFolder::Documents => &FOLDERID_Documents,
            KnownFolder::Downloads => &FOLDERID_Downloads,
            KnownFolder::Music => &FOLDERID_Music,
            KnownFolder::Pictures => &FOLDERID_Pictures,
            KnownFolder::Videos => &FOLDERID_Videos,
            KnownFolder::LocalAppData => &FOLDERID_LocalAppData,
            KnownFolder::LocalAppDataLow => &FOLDERID_LocalAppDataLow,
            KnownFolder::RoamingAppData => &FOLDERID_RoamingAppData,
            KnownFolder::Windows => &FOLDERID_Windows,
            KnownFolder::ProgramFiles => &FOLDERID_ProgramFiles,
            KnownFolder::ProgramFilesX86 => &FOLDERID_ProgramFilesX86,
            KnownFolder::ProgramData => &FOLDERID_ProgramData,
            // `%TEMP%` n'est pas un dossier connu du shell : c'est une
            // variable d'environnement, que la bibliothèque standard lit
            // comme le fait Windows.
            KnownFolder::Temp => return to_internal_path(&std::env::temp_dir()).ok(),
        };
        known_folder_path(id)
    }

    fn volumes(&self) -> Vec<Volume> {
        let system_drive =
            self.known_folder(KnownFolder::Windows).as_deref().and_then(drive_letter);
        fixed_drive_letters().filter_map(|letter| volume_for(letter, system_drive)).collect()
    }

    fn sync_roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = Vec::new();
        let mut add = |path: Option<PathBuf>| {
            if let Some(path) = path {
                let known = fold_path(&path);
                if known.is_some() && !roots.iter().any(|root| fold_path(root) == known) {
                    roots.push(path);
                }
            }
        };
        add(known_folder_path(&FOLDERID_SkyDrive));
        // OneDrive publie aussi ses dossiers par ces variables : la seconde
        // et la troisième distinguent le compte personnel du compte d'entreprise.
        for variable in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
            add(std::env::var_os(variable)
                .and_then(|value| to_internal_path(Path::new(&value)).ok()));
        }
        roots
    }

    fn recycle_bin(&self) -> Option<RecycleBin> {
        let mut total = RecycleBin::default();
        let mut answered = false;
        for letter in fixed_drive_letters() {
            let root = HSTRING::from(format!(r"{letter}:\"));
            let mut info =
                SHQUERYRBINFO { cbSize: size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
            // SAFETY: `root` est une chaîne UTF-16 terminée par zéro et `info`
            // une structure locale dont le champ `cbSize` annonce la taille.
            if unsafe { SHQueryRecycleBinW(&root, &raw mut info) }.is_ok() {
                answered = true;
                // Copies par valeur : la structure peut être compactée.
                let (bytes, items) = (info.i64Size, info.i64NumItems);
                total.bytes = total.bytes.saturating_add(bytes.max(0) as u64);
                total.items = total.items.saturating_add(items.max(0) as u64);
            }
        }
        answered.then_some(total)
    }
}

pub(super) fn set_background_priority(on: bool) -> bool {
    let mode = if on { THREAD_MODE_BACKGROUND_BEGIN } else { THREAD_MODE_BACKGROUND_END };
    // SAFETY: `GetCurrentThread` rend un pseudo-handle toujours valide pour le
    // fil courant ; l'appel ne touche à aucune mémoire de ce programme.
    unsafe { SetThreadPriority(GetCurrentThread(), mode) }.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_letters_are_read_from_internal_paths() {
        assert_eq!(drive_letter(Path::new(r"\\?\c:\Windows")), Some('C'));
        assert_eq!(drive_letter(Path::new(r"D:\")), Some('D'));
        assert_eq!(drive_letter(Path::new(r"\\?\UNC\serveur\partage")), None);
    }

    #[test]
    fn buffers_stop_at_the_first_zero() {
        let buffer: Vec<u16> = "NTFS\0reste".encode_utf16().collect();
        assert_eq!(text_from(&buffer), "NTFS");
        assert_eq!(text_from(&[]), "");
    }

    #[test]
    fn the_system_volume_is_among_the_volumes() {
        let machine = SystemEnvironment::new();
        let volumes = machine.volumes();
        assert!(volumes.iter().filter(|volume| volume.system).count() <= 1);
        if let Some(windows) = machine.known_folder(KnownFolder::Windows) {
            let system = volumes.iter().find(|volume| volume.system);
            if let Some(system) = system {
                assert!(windows.starts_with(&system.root), "{windows:?} sous {:?}", system.root);
            }
        }
    }
}
