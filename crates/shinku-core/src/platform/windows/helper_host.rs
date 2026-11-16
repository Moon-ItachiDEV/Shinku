//! L'assistant élevé sous Windows : la machine vue par `shinku-helper.exe`
//! ([`MachineStartup`]).
//!
//! Ce fichier **n'emprunte rien à `StartupHost`** (qui développe des variables
//! et sonde des fichiers) et **ne touche jamais à `HKCU`** : dans les modes
//! « mot de passe d'un administrateur », ce serait la ruche d'un autre compte.
//! Il ne lit que des **noms** (noms de valeurs, noms de fichiers) et la valeur
//! d'approbation ; il n'ouvre aucun fichier pour en lire le contenu et ne
//! développe aucune variable.
//!
//! **Liste fermée des fonctions de Windows** : c'est **cette liste** qui fait foi
//! (la conception et la spec y renvoient). Ce que le processus élevé appelle,
//! ici et dans [`super::security`] et `super::startup::known_folder` qui lui
//! servent :
//!
//! - durcissement : `SetDefaultDllDirectories` ;
//! - jeton : `CheckTokenMembership`, `CreateWellKnownSid` ;
//! - son propre chemin : `GetModuleFileNameW` (par `current_exe`) ;
//! - fichiers et dossiers, **sans jamais lire un contenu** : `CreateFileW`
//!   (`READ_CONTROL`, attributs ; `FILE_READ_DATA` pour le seul fichier de
//!   l'assistant, sans `ReadFile`), `GetFileInformationByHandle`,
//!   `GetFinalPathNameByHandleW`, `FindFirstFileExW`, `FindNextFileW` ;
//! - droits : `GetSecurityInfo`, `GetAclInformation`, `GetAce`, `IsValidSid`,
//!   `GetSidIdentifierAuthority`, `GetSidSubAuthorityCount`, `GetSidSubAuthority`,
//!   `LocalFree` ;
//! - dossiers connus : `SHGetKnownFolderPath`, `CoTaskMemFree` ;
//! - registre, **`HKEY_LOCAL_MACHINE` seulement** : `RegOpenKeyExW`,
//!   `RegQueryInfoKeyW`, `RegEnumValueW` (sans données), `RegQueryValueExW` (la
//!   seule approbation), `RegCreateKeyExW` et `RegSetValueExW` (la seule
//!   approbation) ;
//! - heure : `GetSystemTimeAsFileTime` ;
//! - la boîte : `MessageBoxW` ; l'échéance : `CreateThread` et `Sleep` (par
//!   `std::thread`), `ExitProcess` (par `process::exit`) ;
//! - fermeture : `CloseHandle`, `RegCloseKey`, `FindClose`.
//!
//! Rien d'autre. Côté `shinku.exe` seulement (jamais dans l'assistant) :
//! `ShellExecuteExW`, `WaitForSingleObject`, `GetExitCodeProcess`,
//! `GetSystemDirectoryW`, `GetCurrentProcess`, `OpenProcessToken`,
//! `GetTokenInformation`, `CoInitializeEx` et `CoUninitialize`.
//!
//! Le processus élevé ne lit pas `HKCU` **par son propre code** ; `user32` et le
//! cadre de saisie, eux, peuvent y lire ce qu'ils veulent (c'est une limite
//! connue, voir la conception).
#![allow(unsafe_code)]

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::sync::Arc;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FIND_FIRST_EX_LARGE_FETCH, FindClose, FindExInfoBasic, FindExSearchNameMatch,
    FindFirstFileExW, FindNextFileW, WIN32_FIND_DATAW,
};
use windows::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_BINARY, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
    RegCreateKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegQueryValueExW,
    RegSetValueExW,
};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::UI::Shell::{
    FOLDERID_CommonStartup, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX64,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IDOK, MB_DEFBUTTON2, MB_ICONWARNING, MB_OKCANCEL, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::security::{self, Opened};
use super::startup::{Key, known_folder, os_error};
use crate::helper::location::{self, Placed};
use crate::helper::request::StartupSlot;
use crate::helper::startup::{
    Deadline, MachineStartup, Names, NamesError, QUESTION_DEADLINE, settle,
};
use crate::startup::MAX_ENTRIES;
use crate::startup::MAX_NAME;
use crate::startup::host::RawApproval;

/// `HKLM\…\Run`.
const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
/// `HKLM\…\Explorer\StartupApproved`.
const APPROVED_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";
/// Nombre de valeurs examinées au plus dans une place (comme au lot 1).
const MAX_SCANNED: u32 = 4096;
/// Un nom de valeur de registre a au plus 16 383 caractères.
const NAME_BUFFER: usize = 16_384;

const SHARE_ALL: FILE_SHARE_MODE =
    FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0);

/// Durcit le processus : ne charger de bibliothèque que depuis `System32`. À
/// appeler **avant tout** autre chose. Faux si Windows refuse : l'assistant
/// sort alors avec le code 8.
pub(crate) fn harden() -> bool {
    // SAFETY: l'appel ne prend qu'un drapeau ; il restreint les dossiers où ce
    // processus cherchera les bibliothèques qu'il chargera ensuite.
    unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) }.is_ok()
}

/// Le dossier Démarrage commun de Windows, **s'il est sûr** (propriétaire et
/// droits) : la poignée est rendue ; son chemin final est celui qu'on énumère.
pub(super) fn common_folder() -> Option<Opened> {
    checked_common_folder(&known_folder(&FOLDERID_CommonStartup)?)
}

/// Ce dossier est-il sûr pour y lire des noms ? Ouvert sans suivre de lien,
/// propriétaire et droits relus par la même décision que l'emplacement de
/// l'assistant. Le chemin est donné : on l'essaie sur un dossier temporaire.
fn checked_common_folder(folder: &Path) -> Option<Opened> {
    let opened = security::open_object(folder, SHARE_ALL, false)?;
    (!opened.placed.reparse && location::is_safe(&opened.placed.security)).then_some(opened)
}

/// Les dossiers `Program Files` que Windows donne, ouverts.
fn program_files() -> Vec<Placed> {
    [&FOLDERID_ProgramFiles, &FOLDERID_ProgramFilesX64]
        .into_iter()
        .filter_map(known_folder)
        .filter_map(|folder| security::open_object(&folder, SHARE_ALL, false))
        .map(|opened| opened.placed)
        .collect()
}

/// Ouvre ce fichier comme assistant, **si** son emplacement est sûr (D5) : le
/// fichier s'appelle `shinku-helper.exe`, n'est pas un lien, est sûr ; son
/// dossier est l'enfant direct de `Program Files` ; ce dossier et `Program
/// Files` sont sûrs. La poignée du fichier est rendue (accès aux données et
/// partage en lecture seule) : celui qui la garde ouverte empêche qu'on réécrive
/// ou supprime le fichier, un garde-fou contre nos erreurs, pas une barrière.
pub(super) fn open_helper(path: &Path) -> Option<Opened> {
    let file = security::open_object(path, FILE_SHARE_READ, true)?;
    let folder_path = Path::new(&file.placed.path).parent()?.to_path_buf();
    let folder = security::open_object(&folder_path, SHARE_ALL, false)?;
    location::helper_is_well_placed(&file.placed, &folder.placed, &program_files()).then_some(file)
}

/// Ouvre une clé du PC (`HKEY_LOCAL_MACHINE`, et nulle autre ruche). `None` si
/// elle n'existe pas.
fn open_machine_key(path: &str, access: REG_SAM_FLAGS) -> io::Result<Option<Key>> {
    let mut key = HKEY::default();
    // SAFETY: `path` est une chaîne terminée par zéro, valide pendant l'appel ;
    // `key` est une variable locale où Windows écrit la poignée.
    let result = unsafe {
        RegOpenKeyExW(HKEY_LOCAL_MACHINE, &HSTRING::from(path), None, access, &raw mut key)
    };
    if result == ERROR_SUCCESS {
        Ok(Some(Key(key)))
    } else if result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND {
        Ok(None)
    } else {
        Err(os_error(result))
    }
}

/// La machine de l'assistant.
#[derive(Debug, Default)]
pub(crate) struct SystemMachineStartup;

fn approved_key(place: StartupSlot) -> String {
    let sub = match place {
        StartupSlot::Run => "Run",
        StartupSlot::Run32 => "Run32",
        StartupSlot::Folder => "StartupFolder",
    };
    format!(r"{APPROVED_KEY}\{sub}")
}

/// Les noms des valeurs texte d'une clé `Run` du PC, dans la vue demandée.
fn run_names(view: u32) -> io::Result<Names> {
    match open_machine_key(RUN_KEY, KEY_READ | REG_SAM_FLAGS(view))? {
        Some(key) => names_in(&key),
        None => Ok(Names { names: Vec::new(), complete: true }),
    }
}

/// Les noms des valeurs texte non vides d'une clé déjà ouverte. Les valeurs
/// d'un autre type (nombre, binaire) ne sont pas des entrées.
fn names_in(key: &Key) -> io::Result<Names> {
    let mut values = 0u32;
    // SAFETY: la clé est ouverte pour `key` ; le seul pointeur donné est une
    // variable locale où Windows écrit un nombre.
    let result = unsafe {
        RegQueryInfoKeyW(
            key.0,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&raw mut values),
            None,
            None,
            None,
            None,
        )
    };
    if result != ERROR_SUCCESS {
        return Err(os_error(result));
    }
    let mut buffer = vec![0u16; NAME_BUFFER];
    let mut names = Vec::new();
    let mut complete = values <= MAX_SCANNED;
    for index in 0..values.min(MAX_SCANNED) {
        let mut length = buffer.len() as u32;
        let mut kind = 0u32;
        // SAFETY: la clé est ouverte ; `buffer` est local et `length` annonce sa
        // taille en caractères. Aucune donnée n'est demandée (pointeurs nuls) :
        // seul le type l'est.
        let result = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(buffer.as_mut_ptr())),
                &raw mut length,
                None,
                Some(&raw mut kind),
                None,
                None,
            )
        };
        if result == ERROR_NO_MORE_ITEMS {
            break;
        }
        if result == ERROR_MORE_DATA {
            complete = false;
            continue;
        }
        if result != ERROR_SUCCESS {
            return Err(os_error(result));
        }
        if kind != REG_SZ.0 && kind != REG_EXPAND_SZ.0 {
            continue;
        }
        let units = &buffer[..(length as usize).min(buffer.len())];
        if units.is_empty() {
            continue;
        }
        if names.len() >= MAX_ENTRIES {
            complete = false;
            break;
        }
        if units.len() > MAX_NAME {
            complete = false;
            continue;
        }
        // Un nom qui n'est pas de l'UTF-16 valide ne peut égaler aucun nom d'une
        // demande : il est laissé de côté.
        if let Ok(name) = String::from_utf16(units) {
            names.push(name);
        }
    }
    Ok(Names { names, complete })
}

/// Les noms des fichiers du dossier Démarrage commun, d'après les seuls
/// attributs rendus par l'énumération (aucun fichier n'est ouvert).
fn folder_names(folder: &Path) -> io::Result<Names> {
    let mut pattern: Vec<u16> = folder.as_os_str().to_string_lossy().encode_utf16().collect();
    pattern.extend(r"\*".encode_utf16());
    pattern.push(0);
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: `pattern` est une chaîne terminée par zéro ; `data` est une
    // structure locale que Windows remplit.
    let handle = unsafe {
        FindFirstFileExW(
            PCWSTR(pattern.as_ptr()),
            FindExInfoBasic,
            (&raw mut data).cast(),
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        )
    }
    .map_err(|error| io::Error::other(error.to_string()))?;
    let mut names = Vec::new();
    let mut complete = true;
    let mut examined = 0u32;
    loop {
        examined += 1;
        if examined > MAX_SCANNED {
            complete = false;
            break;
        }
        let length =
            data.cFileName.iter().position(|&unit| unit == 0).unwrap_or(data.cFileName.len());
        let units = &data.cFileName[..length];
        let is_folder = data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
        let desktop_ini = String::from_utf16_lossy(units).eq_ignore_ascii_case("desktop.ini");
        if !is_folder && !units.is_empty() && !desktop_ini {
            if names.len() >= MAX_ENTRIES {
                complete = false;
                break;
            }
            if units.len() > MAX_NAME {
                complete = false;
            } else if let Ok(name) = String::from_utf16(units) {
                names.push(name);
            }
        }
        // SAFETY: `handle` est une recherche ouverte ; `data` est locale.
        if unsafe { FindNextFileW(handle, &raw mut data) }.is_err() {
            break;
        }
    }
    // SAFETY: `handle` a été ouverte par `FindFirstFileExW` et n'est plus utilisée.
    let _ = unsafe { FindClose(handle) };
    Ok(Names { names, complete })
}

impl MachineStartup for SystemMachineStartup {
    fn has_rights(&self) -> bool {
        security::administrators_enabled()
    }

    fn location_safe(&self) -> bool {
        std::env::current_exe().ok().and_then(|path| open_helper(&path)).is_some()
    }

    fn names(&self, place: StartupSlot) -> Result<Names, NamesError> {
        match place {
            StartupSlot::Run => run_names(KEY_WOW64_64KEY.0),
            StartupSlot::Run32 => run_names(KEY_WOW64_32KEY.0),
            StartupSlot::Folder => {
                // La poignée reste ouverte jusqu'à la fin de l'énumération, et c'est
                // le chemin final vérifié qu'on énumère.
                let opened = common_folder().ok_or(NamesError::UnsafeFolder)?;
                let names = folder_names(Path::new(&opened.placed.path));
                drop(opened);
                names
            }
        }
        .map_err(|_| NamesError::Unreadable)
    }

    fn approval(&self, place: StartupSlot, name: &str) -> io::Result<Option<RawApproval>> {
        let Some(key) = open_machine_key(&approved_key(place), KEY_READ | KEY_WOW64_64KEY)? else {
            return Ok(None);
        };
        let mut buffer = [0u8; 256];
        let mut size = buffer.len() as u32;
        let mut kind = REG_VALUE_TYPE(0);
        // SAFETY: la clé est ouverte pour `key` ; le tampon est local et `size`
        // annonce sa taille en octets ; `kind` est une variable locale.
        let result = unsafe {
            RegQueryValueExW(
                key.0,
                &HSTRING::from(name),
                None,
                Some(&raw mut kind),
                Some(buffer.as_mut_ptr()),
                Some(&raw mut size),
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if result == ERROR_MORE_DATA {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "valeur trop longue"));
        }
        if result != ERROR_SUCCESS {
            return Err(os_error(result));
        }
        let bytes = buffer[..(size as usize).min(buffer.len())].to_vec();
        Ok(Some(RawApproval { is_binary: kind == REG_BINARY, bytes }))
    }

    fn set_approval(&self, place: StartupSlot, name: &str, bytes: [u8; 12]) -> io::Result<()> {
        if name.is_empty() || name.contains('\0') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "nom invalide"));
        }
        let mut key = HKEY::default();
        // SAFETY: le chemin est une chaîne terminée par zéro, valide pendant
        // l'appel ; `key` est une variable locale où Windows écrit la poignée. La
        // ruche est `HKLM` (jamais `HKCU`) ; la clé et ses parents sont créés si
        // elles manquent.
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                &HSTRING::from(approved_key(place).as_str()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_WOW64_64KEY,
                None,
                &raw mut key,
                None,
            )
        };
        if result != ERROR_SUCCESS {
            return Err(os_error(result));
        }
        let key = Key(key);
        // SAFETY: la clé est ouverte pour `key` ; `bytes` est un tampon local de
        // douze octets : la valeur d'approbation, rien d'autre.
        let result =
            unsafe { RegSetValueExW(key.0, &HSTRING::from(name), None, REG_BINARY, Some(&bytes)) };
        if result == ERROR_SUCCESS { Ok(()) } else { Err(os_error(result)) }
    }

    fn file_time_now(&self) -> u64 {
        // SAFETY: appel sans argument.
        let time = unsafe { GetSystemTimeAsFileTime() };
        (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
    }

    fn ask(&self, text: &str) -> bool {
        let deadline = Arc::new(Deadline::new());
        let timer = Arc::clone(&deadline);
        // L'échéance : de la réponse et de la minuterie, la première qui change
        // l'état gagne. Si c'est la minuterie, le processus sort avec le code 3,
        // sans rien écrire.
        std::thread::spawn(move || {
            std::thread::sleep(QUESTION_DEADLINE);
            if timer.expire() {
                std::process::exit(3);
            }
        });
        // SAFETY: `text` et le titre sont des chaînes UTF-16 terminées par zéro,
        // vivantes pendant l'appel ; aucune fenêtre propriétaire.
        let answer = unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(text),
                &HSTRING::from("Shinku"),
                MB_OKCANCEL | MB_DEFBUTTON2 | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST,
            )
        };
        // La décision est dans `settle` (essayée) : la minuterie a-t-elle gagné ?
        let Some(accepted) = settle(&deadline, answer == IDOK) else {
            // La minuterie a gagné : le processus est en train de sortir.
            loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        };
        accepted
    }
}

/// Les arguments de la ligne de commande, sans le nom du programme.
pub(crate) fn arguments() -> Vec<OsString> {
    std::env::args_os().skip(1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_approval_keys_are_the_three_of_the_pc() {
        assert!(approved_key(StartupSlot::Run).ends_with(r"StartupApproved\Run"));
        assert!(approved_key(StartupSlot::Run32).ends_with(r"StartupApproved\Run32"));
        assert!(approved_key(StartupSlot::Folder).ends_with(r"StartupApproved\StartupFolder"));
    }

    #[test]
    fn a_name_that_does_not_exist_is_never_found_and_nothing_fails() {
        let host = SystemMachineStartup;
        for slot in [StartupSlot::Run, StartupSlot::Run32] {
            let names = host.names(slot).expect("la place se lit");
            assert!(!names.names.iter().any(|name| name == "Nom qui n'existe pas -- Shinku essai"));
            assert!(!names.names.iter().any(|name| name.is_empty()));
        }
    }

    #[test]
    fn the_approval_of_a_missing_name_is_absent() {
        let host = SystemMachineStartup;
        assert!(
            host.approval(StartupSlot::Run, "Nom qui n'existe pas -- Shinku essai")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_folder_listing_gives_files_only_without_desktop_ini() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("a.lnk"), b"x").unwrap();
        std::fs::write(temp.path().join("Desktop.INI"), b"x").unwrap();
        std::fs::create_dir(temp.path().join("sous-dossier")).unwrap();
        let names = folder_names(temp.path()).unwrap();
        assert_eq!(names.names, ["a.lnk"]);
        assert!(names.complete);
        let long = temp.path().join("n".repeat(MAX_NAME + 1));
        if std::fs::write(long, b"x").is_err() {
            // Sous Wine, le système de fichiers Linux refuse 256 octets.
            return;
        }
        let names = folder_names(temp.path()).unwrap();
        assert_eq!(names.names, ["a.lnk"]);
        assert!(!names.complete, "un nom trop long rend la place incomplète");
    }

    #[test]
    fn the_file_time_is_plausible() {
        let time = SystemMachineStartup.file_time_now();
        // Après le 1er janvier 2020.
        assert!(time > 132_223_104_000_000_000);
    }

    /// Le code d'un fichier, sans ses commentaires ni ses essais.
    fn code_of(source: &str) -> String {
        let end = source.find("#[cfg(test)]").unwrap_or(source.len());
        source[..end]
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_helper_never_names_the_account_hive() {
        let files = [
            ("helper_host.rs", include_str!("helper_host.rs")),
            ("helper/mod.rs", include_str!("../../helper/mod.rs")),
            ("helper/request.rs", include_str!("../../helper/request.rs")),
            ("helper/location.rs", include_str!("../../helper/location.rs")),
            ("helper/startup.rs", include_str!("../../helper/startup.rs")),
        ];
        for (name, source) in files {
            let code = code_of(source);
            for banned in ["Hive", "HKEY_CURRENT_USER", "HKCU", "HKEY_USERS", "StartupHost"] {
                assert!(!code.contains(banned), "{name} nomme {banned}");
            }
        }
        // Et la ruche du PC y est bien nommée : l'essai ne passe pas à vide.
        assert!(code_of(include_str!("helper_host.rs")).contains("HKEY_LOCAL_MACHINE"));
    }

    #[test]
    fn the_common_folder_goes_through_the_safety_decision_with_its_path() {
        // Un dossier temporaire du compte n'est pas celui d'un administrateur :
        // refusé (propriétaire ou droits). Sous Wine, le propriétaire est le
        // compte Linux : refusé aussi.
        let temp = tempfile::tempdir().unwrap();
        assert!(checked_common_folder(temp.path()).is_none());
        assert!(checked_common_folder(&temp.path().join("absent")).is_none());
        // Et l'énumération du dossier refusé ne se fait pas : le guichet dit
        // « dossier non sûr » avant de lire le moindre nom.
    }

    /// Une clé temporaire du compte (dans l'essai seulement), effacée à la fin.
    struct TempKey {
        root: String,
        key: Key,
    }

    impl TempKey {
        fn new(tag: &str) -> Self {
            use windows::Win32::System::Registry::HKEY_CURRENT_USER;
            let root = format!(r"Software\Shinku\EssaiAssistant\{tag}-{}", std::process::id());
            let mut key = HKEY::default();
            // SAFETY: chemin terminé par zéro, valide pendant l'appel ; `key` locale.
            let result = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(root.as_str()),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_READ | KEY_SET_VALUE,
                    None,
                    &raw mut key,
                    None,
                )
            };
            assert_eq!(result, ERROR_SUCCESS);
            Self { root, key: Key(key) }
        }

        fn set(&self, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) {
            // SAFETY: clé ouverte ; `data` est un tampon local.
            let result =
                unsafe { RegSetValueExW(self.key.0, &HSTRING::from(name), None, kind, Some(data)) };
            assert_eq!(result, ERROR_SUCCESS);
        }
    }

    impl Drop for TempKey {
        fn drop(&mut self) {
            use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};
            // SAFETY: chemin terminé par zéro ; la clé est celle de cet essai.
            let _ =
                unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(self.root.as_str())) };
        }
    }

    fn text(value: &str) -> Vec<u8> {
        value.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn only_non_empty_text_values_are_entries() {
        use windows::Win32::System::Registry::REG_DWORD;
        let temp = TempKey::new("noms");
        temp.set("texte", REG_SZ, &text(r"C:\a.exe"));
        temp.set("developpe", REG_EXPAND_SZ, &text(r"%X%\b.exe"));
        temp.set("", REG_SZ, &text(r"C:\defaut.exe")); // la valeur par défaut : nom vide
        temp.set("nombre", REG_DWORD, &1u32.to_le_bytes());
        temp.set("binaire", REG_BINARY, &[1, 2, 3]);
        let names = names_in(&temp.key).expect("lue");
        let mut found = names.names.clone();
        found.sort();
        assert_eq!(found, ["developpe", "texte"], "ni nombre, ni binaire, ni nom vide");
        assert!(names.complete);
    }
}
