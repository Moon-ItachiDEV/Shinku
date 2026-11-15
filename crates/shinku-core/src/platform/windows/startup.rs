//! Le module « Démarrage », sous Windows : [`StartupHost`] sur de vrais appels
//! documentés de l'API Windows.
//!
//! Ce module lit six places (valeurs `Run` du compte et du PC, dossiers
//! Démarrage du compte et commun) et leurs valeurs d'approbation. Il ne
//! **modifie qu'une chose** : une valeur d'approbation du compte, sous
//! `HKCU\…\Explorer\StartupApproved`. Il n'ouvre jamais `HKLM` en écriture :
//! [`StartupHost::set_approval`] refuse toute place du PC avant même de
//! toucher au registre.
//!
//! Les clés sont écrites dans un [`Layout`] : en production celui de Windows,
//! dans les tests une racine d'essai sous `HKCU`, jamais les vraies clés.
//!
//! Rien ici ne « répare » ni ne résout : un raccourci cassé reste cassé
//! (`SLGP_RAWPATH`, pas de `Resolve`). Rien n'est lu hors d'un lecteur fixe
//! (`GetDriveTypeW`) : ni chemin réseau, ni lecteur réseau ou amovible, ni lien
//! suivi. Une borne atteinte est dite (`RawPlace::complete`), jamais tue.
#![allow(unsafe_code)]

use std::ffi::{OsStr, OsString, c_void};
use std::fs;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND,
    ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::Storage::FileSystem::GetDriveTypeW;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize, IPersistFile, STGM_READ,
};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, KEY_WOW64_32KEY,
    KEY_WOW64_64KEY, REG_BINARY, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ,
    REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW,
    RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::SystemInformation::{
    GetSystemTimeAsFileTime, GetSystemWindowsDirectoryW,
};
use windows::Win32::UI::Shell::{
    FOLDERID_CommonStartup, FOLDERID_Startup, IShellLinkW, KF_FLAG_DONT_VERIFY,
    SHGetKnownFolderPath, SLGP_RAWPATH, ShellLink,
};
use windows::core::{GUID, HSTRING, PCWSTR, PWSTR};

use super::game::version_string;
use crate::model::Timestamp;
use crate::startup::host::{Existence, FileIdentity, RawApproval, RawEntry, RawPlace, StartupHost};
use crate::startup::{Place, Scope};

/// Valeur de `GetDriveTypeW` pour un disque interne.
const DRIVE_FIXED: u32 = 3;
/// Nombre maximal d'entrées lues par place.
const MAX_ENTRIES: usize = crate::startup::MAX_ENTRIES;
/// Longueur maximale d'un nom d'entrée, en caractères.
const MAX_NAME: usize = 255;
/// Longueur maximale d'une commande, en caractères.
const MAX_COMMAND: usize = 2048;
/// Nombre de valeurs examinées au plus dans une clé `Run`.
const MAX_SCANNED: u32 = 4096;
/// Taille maximale d'une valeur lue dans une clé `Run`, en octets.
const MAX_VALUE_BYTES: usize = 1 << 20;
/// Longueur maximale d'un nom de valeur de registre, en caractères.
const MAX_VALUE_NAME: usize = 16_384;

/// Une ruche du registre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hive {
    /// `HKEY_CURRENT_USER`.
    Current,
    /// `HKEY_LOCAL_MACHINE`.
    Local,
}

impl Hive {
    fn root(self) -> HKEY {
        match self {
            Self::Current => HKEY_CURRENT_USER,
            Self::Local => HKEY_LOCAL_MACHINE,
        }
    }
}

/// Où sont les clés : la ruche et le chemin de chaque clé `Run` et de chaque
/// clé d'approbation.
#[derive(Clone, Debug)]
struct Layout {
    user: Hive,
    machine: Hive,
    user_run: String,
    user_run32: String,
    user_approved: String,
    machine_run: String,
    machine_run32: String,
    machine_approved: String,
    /// La vue dans laquelle s'ouvre la clé `Run` 32 bits du PC.
    machine_run32_view: u32,
    /// Les dossiers Démarrage, si on ne les demande pas à Windows.
    user_folder: Option<PathBuf>,
    machine_folder: Option<PathBuf>,
}

/// Où se trouve une place.
struct Where {
    hive: Hive,
    /// La clé `Run` et la vue où l'ouvrir ; `None` pour un dossier.
    run: Option<(String, u32)>,
    /// La clé d'approbation de la place.
    approved: String,
}

impl Layout {
    /// Les vraies clés de Windows.
    fn real() -> Self {
        let run = r"Software\Microsoft\Windows\CurrentVersion\Run".to_owned();
        let approved =
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved".to_owned();
        Self {
            user: Hive::Current,
            machine: Hive::Local,
            user_run: run.clone(),
            // `HKCU\Software` n'est pas redirigé : le chemin est écrit en toutes lettres.
            user_run32: r"Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Run".to_owned(),
            user_approved: approved.clone(),
            machine_run: run,
            machine_run32: r"Software\Microsoft\Windows\CurrentVersion\Run".to_owned(),
            machine_approved: approved,
            machine_run32_view: KEY_WOW64_32KEY.0,
            user_folder: None,
            machine_folder: None,
        }
    }

    fn place(&self, place: Place) -> Where {
        let sub = match place {
            Place::UserRun | Place::MachineRun => "Run",
            Place::UserRun32 | Place::MachineRun32 => "Run32",
            Place::UserFolder | Place::MachineFolder => "StartupFolder",
        };
        let (hive, approved) = match place.scope() {
            Scope::User => (self.user, &self.user_approved),
            Scope::Machine => (self.machine, &self.machine_approved),
        };
        let run = match place {
            Place::UserRun => Some((self.user_run.clone(), KEY_WOW64_64KEY.0)),
            Place::UserRun32 => Some((self.user_run32.clone(), KEY_WOW64_64KEY.0)),
            Place::MachineRun => Some((self.machine_run.clone(), KEY_WOW64_64KEY.0)),
            Place::MachineRun32 => Some((self.machine_run32.clone(), self.machine_run32_view)),
            Place::UserFolder | Place::MachineFolder => None,
        };
        Where { hive, run, approved: format!(r"{approved}\{sub}") }
    }
}

/// La machine du module « Démarrage ».
#[derive(Debug)]
pub(in crate::platform) struct SystemStartupHost {
    layout: Layout,
}

impl SystemStartupHost {
    pub(in crate::platform) fn new() -> Self {
        Self { layout: Layout::real() }
    }
}

/// Une clé ouverte : fermée à la fin de sa vie, quoi qu'il arrive.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: la clé a été ouverte ou créée par ce module et n'est fermée qu'ici.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn os_error(code: WIN32_ERROR) -> io::Error {
    io::Error::from_raw_os_error(code.0 as i32)
}

/// Ouvre une clé. `None` si elle n'existe pas : une place absente est une
/// place vide, pas une erreur.
fn open_key(hive: Hive, path: &str, access: REG_SAM_FLAGS) -> io::Result<Option<Key>> {
    let mut key = HKEY::default();
    // SAFETY: `path` est une chaîne terminée par zéro, valide pendant l'appel ;
    // `key` est une variable locale où Windows écrit le handle.
    let result =
        unsafe { RegOpenKeyExW(hive.root(), &HSTRING::from(path), None, access, &raw mut key) };
    if result == ERROR_SUCCESS {
        Ok(Some(Key(key)))
    } else if result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND {
        Ok(None)
    } else {
        Err(os_error(result))
    }
}

/// Un nom avec un zéro au milieu ne peut pas être passé à Windows tel quel.
fn has_nul(name: &str) -> bool {
    name.contains('\0')
}

/// Les unités UTF-16 d'un texte, terminées par zéro.
fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

/// Le texte d'une valeur `REG_SZ` : jusqu'au premier zéro. Un texte qui n'est
/// pas de l'UTF-16 valide donne une commande **vide** (cible inconnue) : une
/// conversion approximative ferait chercher sur le disque un chemin qui n'est
/// pas le vrai, et conclure « désinstallée » pour un programme bien présent.
fn string_of(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16(&units).unwrap_or_default()
}

/// Un `FILETIME` sous forme d'entier de 64 bits.
fn file_time(time: windows::Win32::Foundation::FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

impl SystemStartupHost {
    /// Les valeurs texte d'une clé `Run`.
    fn registry_entries(&self, place: Place) -> io::Result<RawPlace> {
        let location = self.layout.place(place);
        let Some((path, view)) = location.run else { return Ok(RawPlace::complete(Vec::new())) };
        let Some(key) = open_key(location.hive, &path, KEY_READ | REG_SAM_FLAGS(view))? else {
            return Ok(RawPlace::complete(Vec::new()));
        };

        let (mut values, mut longest_name, mut longest_value) = (0u32, 0u32, 0u32);
        // SAFETY: la clé est ouverte pour `key` ; les trois pointeurs sont des
        // variables locales où Windows écrit un nombre.
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
                Some(&raw mut longest_name),
                Some(&raw mut longest_value),
                None,
                None,
            )
        };
        if result != ERROR_SUCCESS {
            return Err(os_error(result));
        }
        // Les tampons sont à la taille des plus grandes valeurs de la clé,
        // bornée : une valeur plus grande est ignorée (de toute façon trop longue).
        let mut name = vec![0u16; (longest_name as usize + 1).min(MAX_VALUE_NAME + 1)];
        let mut data = vec![0u8; (longest_value as usize).clamp(2, MAX_VALUE_BYTES) + 2];

        let mut entries = Vec::new();
        // Une borne atteinte est dite : jamais une liste présentée complète
        // alors qu'elle ne l'est pas.
        let mut complete = values <= MAX_SCANNED;
        for index in 0..values.min(MAX_SCANNED) {
            let mut name_len = name.len() as u32;
            let mut data_len = data.len() as u32;
            let mut kind = 0u32;
            // SAFETY: la clé est ouverte pour `key` ; les tampons sont locaux et
            // `name_len` et `data_len` annoncent leurs tailles (en caractères
            // pour le nom, en octets pour les données).
            let result = unsafe {
                RegEnumValueW(
                    key.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &raw mut name_len,
                    None,
                    Some(&raw mut kind),
                    Some(data.as_mut_ptr()),
                    Some(&raw mut data_len),
                )
            };
            if result == ERROR_NO_MORE_ITEMS {
                break;
            }
            if result == ERROR_MORE_DATA {
                // Une valeur démesurée (plus d'un mébioctet) : on ne l'a pas lue.
                complete = false;
                continue;
            }
            if result != ERROR_SUCCESS {
                return Err(os_error(result));
            }
            // Seules les valeurs texte sont des entrées.
            if kind != REG_SZ.0 && kind != REG_EXPAND_SZ.0 {
                continue;
            }
            let units = &name[..(name_len as usize).min(name.len())];
            // Le nom vide est la valeur par défaut de la clé : pas une entrée.
            if units.is_empty() {
                continue;
            }
            if entries.len() >= MAX_ENTRIES {
                complete = false;
                break;
            }
            if units.len() > MAX_NAME {
                complete = false;
                continue;
            }
            // Un nom qui n'est pas de l'UTF-16 valide, ou qui contient un zéro,
            // est gardé comme approximatif : le cœur ne lit pas son approbation
            // et ne le règle pas, mais il le montre.
            let (text, name_exact) = decode_name(units);
            let mut command = string_of(&data[..(data_len as usize).min(data.len())]);
            // Une commande trop longue n'écarte pas l'entrée : elle est gardée,
            // sans commande, donc de cible inconnue.
            if command.chars().count() > MAX_COMMAND {
                command.clear();
            }
            entries.push(RawEntry { name: text, name_exact, command });
        }
        Ok(RawPlace { entries, complete })
    }

    /// Le dossier Démarrage de cette place.
    fn folder(&self, place: Place) -> io::Result<PathBuf> {
        let given = match place {
            Place::UserFolder => &self.layout.user_folder,
            _ => &self.layout.machine_folder,
        };
        match given {
            Some(folder) => Ok(folder.clone()),
            None => known_folder(folder_id(place)).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "Windows ne donne pas ce dossier")
            }),
        }
    }

    /// Les fichiers d'un dossier Démarrage, cachés compris.
    fn folder_entries(&self, place: Place) -> io::Result<RawPlace> {
        let folder = self.folder(place)?;
        // Un dossier redirigé hors d'un lecteur fixe (partage réseau d'un
        // profil d'entreprise, par exemple) n'est pas lu : cela pourrait
        // déclencher une connexion. La place est dite illisible.
        if !self.is_local_fixed(&folder.to_string_lossy()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "le dossier Démarrage n'est pas sur un lecteur fixe",
            ));
        }
        let listing = match fs::read_dir(&folder) {
            Ok(listing) => listing,
            // Un dossier qui n'existe pas est un dossier vide.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(RawPlace::complete(Vec::new()));
            }
            Err(error) => return Err(error),
        };
        // Les raccourcis se lisent avec COM, sur le fil appelant.
        let _com = ComScope::enter();
        let mut entries = Vec::new();
        let mut complete = true;
        let mut examined = 0usize;
        for item in listing {
            // Tout élément examiné compte, gardé ou non : un dossier rempli de
            // sous-dossiers ne doit pas faire parcourir des millions d'éléments.
            examined += 1;
            if examined > MAX_SCANNED as usize {
                complete = false;
                break;
            }
            let item = item?;
            // `file_type` de l'entrée de dossier ne suit pas les liens.
            let Ok(kind) = item.file_type() else { continue };
            let is_link = match classify(kind.is_file(), kind.is_symlink(), kind.is_symlink_dir()) {
                Kind::Skip => continue,
                Kind::File => false,
                Kind::Link => true,
            };
            let file_name = item.file_name();
            let units: Vec<u16> = file_name.encode_wide().collect();
            let lossy = String::from_utf16_lossy(&units);
            if units.is_empty() || lossy.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            if entries.len() >= MAX_ENTRIES {
                complete = false;
                break;
            }
            if units.len() > MAX_NAME {
                complete = false;
                continue;
            }
            let name_exact = String::from_utf16(&units).is_ok();
            let command = folder_command(&item.path(), is_link);
            entries.push(RawEntry { name: lossy, name_exact, command });
        }
        Ok(RawPlace { entries, complete })
    }
}

/// Ce qu'un élément du dossier Démarrage est pour Shinku.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Pas une app : un sous-dossier, un lien vers un dossier (jonction ou lien
    /// symbolique de dossier).
    Skip,
    /// Un fichier.
    File,
    /// Un lien vers un fichier : listé (l'Explorateur le lance), jamais suivi.
    Link,
}

/// Le genre d'un élément, d'après ce que l'entrée du dossier dit sans suivre
/// aucun lien.
fn classify(is_file: bool, is_symlink: bool, is_symlink_dir: bool) -> Kind {
    if is_symlink_dir {
        Kind::Skip
    } else if is_symlink {
        Kind::Link
    } else if is_file {
        Kind::File
    } else {
        Kind::Skip
    }
}

/// La commande d'un fichier du dossier Démarrage : la cible du raccourci si elle
/// se lit, sinon le fichier lui-même, entre guillemets (un chemin, pas une
/// ligne de commande : ses espaces ne le coupent pas).
///
/// Un **lien** est listé sous son nom (l'Explorateur le lance) mais jamais suivi :
/// ni sa cible lue, ni ses métadonnées demandées. Sans commande, sa cible est
/// inconnue. Une cible trop longue ne donne jamais un chemin tronqué : elle
/// reste inconnue elle aussi, et une commande trop longue est gardée sans
/// commande plutôt que d'écarter l'entrée.
fn folder_command(path: &Path, is_link: bool) -> String {
    if is_link {
        return String::new();
    }
    let is_shortcut =
        path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"));
    let target = match is_shortcut.then(|| shortcut_target(path)) {
        Some(Shortcut::Path(target)) => target,
        // Trop longue ou inexacte : jamais un chemin tronqué ou approximatif.
        Some(Shortcut::TooLong | Shortcut::Inexact) => return String::new(),
        Some(Shortcut::Unreadable) | None => match path.to_str() {
            Some(text) => text.to_owned(),
            // Un nom de fichier qui n'est pas de l'UTF-16 valide : cible inconnue.
            None => return String::new(),
        },
    };
    let command = format!("\"{target}\"");
    if command.chars().count() > MAX_COMMAND { String::new() } else { command }
}

/// Un nom de valeur, et s'il est exact : de l'UTF-16 valide, sans zéro au milieu.
/// Sinon il est approximatif (caractères de remplacement) : le cœur le montre,
/// mais ne lit pas son approbation et ne le règle pas.
fn decode_name(units: &[u16]) -> (String, bool) {
    match String::from_utf16(units) {
        Ok(text) => (text, !units.contains(&0)),
        Err(_) => (String::from_utf16_lossy(units), false),
    }
}

/// Le dossier connu de Windows qui est le dossier Démarrage de cette place.
fn folder_id(place: Place) -> &'static GUID {
    match place {
        Place::UserFolder => &FOLDERID_Startup,
        _ => &FOLDERID_CommonStartup,
    }
}

/// L'appartement COM du fil appelant, pour la durée d'une lecture.
struct ComScope {
    initialized: bool,
}

impl ComScope {
    fn enter() -> Self {
        // SAFETY: appel sans pointeur. Si le fil a déjà un appartement d'un
        // autre type, l'appel échoue et COM reste utilisable tel qu'il est.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Self { initialized: result.is_ok() }
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: appariée à l'initialisation réussie de ce même fil.
            unsafe { CoUninitialize() };
        }
    }
}

/// Ce qu'on tire d'un fichier `.lnk`.
enum Shortcut {
    /// La cible, telle que le raccourci l'écrit.
    Path(String),
    /// Une cible qui ne tient pas dans le tampon.
    TooLong,
    /// Une cible qui n'est pas de l'UTF-16 valide : la cible reste inconnue.
    Inexact,
    /// Le fichier ne se lit pas ou n'a pas de cible.
    Unreadable,
}

/// Taille du tampon de `GetPath`, en caractères : un chemin NTFS ne va pas
/// au-delà de 32 767.
const TARGET_BUFFER: usize = 32_768;

/// La cible d'un raccourci, telle qu'il l'écrit (`SLGP_RAWPATH`, sans la résoudre
/// ni la réparer).
fn shortcut_target(path: &Path) -> Shortcut {
    // SAFETY: `ShellLink` est la classe des raccourcis ; l'interface demandée
    // est celle que cette classe porte.
    let Ok(link) =
        (unsafe { CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) })
    else {
        return Shortcut::Unreadable;
    };
    let Ok(file) = windows::core::Interface::cast::<IPersistFile>(&link) else {
        return Shortcut::Unreadable;
    };
    let name = wide(path.as_os_str());
    // SAFETY: `name` est une chaîne terminée par zéro, valide pendant l'appel.
    if unsafe { file.Load(PCWSTR(name.as_ptr()), STGM_READ) }.is_err() {
        return Shortcut::Unreadable;
    }
    let mut buffer = vec![0u16; TARGET_BUFFER];
    // SAFETY: `buffer` est local et Windows n'y écrit pas plus que sa taille ;
    // aucune structure `WIN32_FIND_DATAW` n'est demandée.
    if unsafe { link.GetPath(&mut buffer, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32) }.is_err() {
        return Shortcut::Unreadable;
    }
    target_from_buffer(&buffer)
}

/// Le texte que `GetPath` a laissé dans le tampon. Pas de zéro final, ou un
/// tampon plein : le chemin est peut-être tronqué, on ne le garde pas.
fn target_from_buffer(buffer: &[u16]) -> Shortcut {
    match buffer.iter().position(|&unit| unit == 0) {
        None => Shortcut::TooLong,
        Some(length) if length + 1 >= buffer.len() => Shortcut::TooLong,
        Some(0) => Shortcut::Unreadable,
        Some(length) => match String::from_utf16(&buffer[..length]) {
            Ok(target) => Shortcut::Path(target),
            Err(_) => Shortcut::Inexact,
        },
    }
}

/// Demande à Windows le chemin d'un dossier connu, sans le créer.
fn known_folder(id: &GUID) -> Option<PathBuf> {
    // SAFETY: `id` pointe vers un GUID valide pendant l'appel. Sans jeton
    // (`None`), Windows répond pour l'utilisateur courant.
    let raw = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DONT_VERIFY, None) }.ok()?;
    // SAFETY: en cas de succès, `raw` est une chaîne UTF-16 terminée par zéro,
    // allouée par Windows et valide jusqu'à sa libération ci-dessous.
    let text = OsString::from_wide(unsafe { raw.as_wide() });
    // SAFETY: la chaîne vient de `SHGetKnownFolderPath`, qui demande de la
    // libérer avec `CoTaskMemFree`. Elle n'est plus utilisée ensuite.
    unsafe { CoTaskMemFree(Some(raw.0.cast::<c_void>().cast_const())) };
    Some(PathBuf::from(text))
}

/// `ERROR_FILE_NOT_FOUND` (2) et `ERROR_PATH_NOT_FOUND` (3), lus par le code
/// brut : seuls « introuvable » disent « absent ». Un lecteur absent, un
/// partage éteint ou un nom invalide sont des incertitudes.
fn is_not_found(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(2 | 3))
}

/// Les attributs d'un fichier « en ligne seulement » (service de stockage en
/// ligne) : `OFFLINE`, `RECALL_ON_OPEN`, `RECALL_ON_DATA_ACCESS`.
const ATTRIBUTES_ONLINE_ONLY: u32 = 0x0000_1000 | 0x0004_0000 | 0x0040_0000;

/// Vrai si on peut lire ce fichier sans qu'il soit téléchargé.
fn reads_without_download(attributes: u32) -> bool {
    attributes & ATTRIBUTES_ONLINE_ONLY == 0
}

/// La racine `X:\` d'un chemin absolu à lettre de lecteur, y compris sous la
/// forme `\\?\X:\`. `None` pour tout le reste (chemin réseau, relatif…).
fn drive_root(path: &str) -> Option<String> {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    let bytes = path.as_bytes();
    let rooted = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    rooted.then(|| format!("{}:\\", char::from(bytes[0].to_ascii_uppercase())))
}

impl StartupHost for SystemStartupHost {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn file_time_now(&self) -> u64 {
        // SAFETY: appel sans argument.
        file_time(unsafe { GetSystemTimeAsFileTime() })
    }

    fn entries(&self, place: Place) -> io::Result<RawPlace> {
        match place {
            Place::UserFolder | Place::MachineFolder => self.folder_entries(place),
            _ => self.registry_entries(place),
        }
    }

    fn approval(&self, place: Place, name: &str) -> io::Result<Option<RawApproval>> {
        if has_nul(name) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "nom invalide"));
        }
        let location = self.layout.place(place);
        let Some(key) = open_key(location.hive, &location.approved, KEY_READ | KEY_WOW64_64KEY)?
        else {
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
            // Plus long que 256 octets : ce n'est pas la valeur de Windows.
            return Err(io::Error::new(io::ErrorKind::InvalidData, "valeur trop longue"));
        }
        if result != ERROR_SUCCESS {
            return Err(os_error(result));
        }
        let bytes = buffer[..(size as usize).min(buffer.len())].to_vec();
        Ok(Some(RawApproval { is_binary: kind == REG_BINARY, bytes }))
    }

    fn set_approval(&self, place: Place, name: &str, bytes: [u8; 12]) -> io::Result<()> {
        // Ce lot n'écrit que pour le compte : aucune clé du PC n'est ouverte.
        if place.scope() != Scope::User {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "les entrées du PC ne se règlent pas ici",
            ));
        }
        if has_nul(name) || name.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "nom invalide"));
        }
        let location = self.layout.place(place);
        let mut key = HKEY::default();
        // SAFETY: `location.approved` est une chaîne terminée par zéro, valide
        // pendant l'appel ; `key` est une variable locale où Windows écrit le
        // handle. La ruche est celle du compte (la portée a été vérifiée) ; la
        // clé et ses parents sont créés si elles manquent.
        let result = unsafe {
            RegCreateKeyExW(
                location.hive.root(),
                &HSTRING::from(location.approved.as_str()),
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
        // SAFETY: la clé est ouverte pour `key` ; `bytes` est un tampon local
        // de douze octets.
        let result =
            unsafe { RegSetValueExW(key.0, &HSTRING::from(name), None, REG_BINARY, Some(&bytes)) };
        if result == ERROR_SUCCESS { Ok(()) } else { Err(os_error(result)) }
    }

    fn expand(&self, text: &str) -> String {
        let source = HSTRING::from(text);
        let mut buffer = vec![0u16; 4096];
        for _ in 0..2 {
            // SAFETY: `source` est une chaîne terminée par zéro, valide pendant
            // l'appel ; `buffer` est local, Windows n'y écrit pas plus que sa taille.
            let needed = unsafe { ExpandEnvironmentStringsW(&source, Some(&mut buffer)) } as usize;
            if needed == 0 {
                break;
            }
            if needed <= buffer.len() {
                // Conversion stricte : une variable qui n'est pas de l'UTF-16
                // valide donnerait un chemin approximatif, donc un fichier
                // « absent » à tort. Une ligne vide dit « impossible à dire ».
                let end = buffer[..needed].iter().position(|&unit| unit == 0).unwrap_or(needed);
                return String::from_utf16(&buffer[..end]).unwrap_or_default();
            }
            // Trop petit : Windows dit la taille qu'il faut (zéro final compris).
            if needed > 32_768 {
                break;
            }
            buffer = vec![0u16; needed];
        }
        text.to_owned()
    }

    fn is_local_fixed(&self, path: &str) -> bool {
        let Some(root) = drive_root(path) else { return false };
        let root = HSTRING::from(root.as_str());
        // SAFETY: `root` est une chaîne terminée par zéro, valide pendant l'appel.
        unsafe { GetDriveTypeW(&root) == DRIVE_FIXED }
    }

    fn existence(&self, path: &str) -> Existence {
        // Hors d'un lecteur fixe, aucun accès : la réponse est « incertain ».
        if !self.is_local_fixed(path) {
            return Existence::Unsure;
        }
        // `symlink_metadata` ne suit pas les liens : un lien n'est ni suivi ni
        // pris pour un fichier. Seul « introuvable » dit « absent ».
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => Existence::File,
            Ok(metadata) if metadata.is_dir() => Existence::Folder,
            Ok(_) => Existence::Unsure,
            Err(error) if is_not_found(&error) => Existence::Absent,
            Err(_) => Existence::Unsure,
        }
    }

    fn identity(&self, path: &str) -> FileIdentity {
        // La lecture des ressources de version charge le fichier : jamais hors
        // d'un lecteur fixe.
        if !self.is_local_fixed(path) {
            return FileIdentity::default();
        }
        // Un fichier « en ligne seulement » ne se lit pas : le lire le ferait
        // télécharger. Il existe, mais n'a pas de description.
        let readable = fs::symlink_metadata(path)
            .is_ok_and(|metadata| reads_without_download(metadata.file_attributes()));
        if !readable {
            return FileIdentity::default();
        }
        FileIdentity {
            description: version_string(path, "FileDescription"),
            company: version_string(path, "CompanyName"),
        }
    }

    fn windows_dir(&self) -> String {
        let mut buffer = [0u16; 512];
        // SAFETY: `buffer` est local et Windows n'y écrit pas plus que sa taille.
        let length = unsafe { GetSystemWindowsDirectoryW(Some(&mut buffer)) } as usize;
        if length == 0 || length > buffer.len() {
            return String::new();
        }
        String::from_utf16_lossy(&buffer[..length]).trim_end_matches(['\\', '/']).to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::approval::{cut_bytes, restore_bytes};
    use crate::startup::{self, State, ToggleError};
    use windows::Win32::System::Registry::{REG_DWORD, RegDeleteTreeW};

    /// Une racine d'essai sous `HKCU`, supprimée à la fin : les tests n'écrivent
    /// jamais sous les vraies clés `Run` ou `StartupApproved`.
    struct Sandbox {
        root: String,
        user_folder: tempfile::TempDir,
        machine_folder: tempfile::TempDir,
        host: SystemStartupHost,
    }

    impl Sandbox {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |time| time.as_nanos());
            let root = format!(r"Software\Shinku-Essai-{}-{unique}", std::process::id());
            let user_folder = tempfile::tempdir().unwrap();
            let machine_folder = tempfile::tempdir().unwrap();
            let layout = Layout {
                user: Hive::Current,
                // Le « PC » de l'essai est aussi sous `HKCU` : on ne touche pas à `HKLM`.
                machine: Hive::Current,
                user_run: format!(r"{root}\UserRun"),
                user_run32: format!(r"{root}\UserRun32"),
                user_approved: format!(r"{root}\UserApproved"),
                machine_run: format!(r"{root}\MachineRun"),
                machine_run32: format!(r"{root}\MachineRun32"),
                machine_approved: format!(r"{root}\MachineApproved"),
                // Sous `HKCU\Software`, il n'y a pas de vue 32 bits à part.
                machine_run32_view: KEY_WOW64_64KEY.0,
                user_folder: Some(user_folder.path().to_path_buf()),
                machine_folder: Some(machine_folder.path().to_path_buf()),
            };
            Self { root, user_folder, machine_folder, host: SystemStartupHost { layout } }
        }

        fn create(&self, path: &str) -> Key {
            let mut key = HKEY::default();
            // SAFETY: chaîne terminée par zéro ; `key` reçoit le handle.
            let result = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(path),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE | KEY_READ | KEY_WOW64_64KEY,
                    None,
                    &raw mut key,
                    None,
                )
            };
            assert_eq!(result, ERROR_SUCCESS, "clé d'essai");
            Key(key)
        }

        /// Écrit une valeur dans la clé `Run` de cette place.
        fn put(&self, place: Place, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) {
            let path = self.host.layout.place(place).run.unwrap().0;
            let key = self.create(&path);
            // SAFETY: clé ouverte ; `data` est un tampon local.
            let result =
                unsafe { RegSetValueExW(key.0, &HSTRING::from(name), None, kind, Some(data)) };
            assert_eq!(result, ERROR_SUCCESS);
        }

        fn put_text(&self, place: Place, name: &str, kind: REG_VALUE_TYPE, text: &str) {
            let data: Vec<u8> = text
                .encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(|unit| unit.to_le_bytes())
                .collect();
            self.put(place, name, kind, &data);
        }

        /// Écrit une valeur d'approbation, sans passer par Shinku.
        fn approve(&self, place: Place, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) {
            let key = self.create(&self.host.layout.place(place).approved);
            // SAFETY: clé ouverte ; `data` est un tampon local.
            let result =
                unsafe { RegSetValueExW(key.0, &HSTRING::from(name), None, kind, Some(data)) };
            assert_eq!(result, ERROR_SUCCESS);
        }

        fn exists(&self, path: &str) -> bool {
            open_key(Hive::Current, path, KEY_READ | KEY_WOW64_64KEY).unwrap().is_some()
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            // SAFETY: chaîne terminée par zéro ; ne supprime que la racine d'essai.
            let _ =
                unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(self.root.as_str())) };
            let _ = (&self.user_folder, &self.machine_folder);
        }
    }

    /// Pose l'attribut « caché » et vérifie qu'il y est.
    fn hide(path: &Path) {
        use windows::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW, SetFileAttributesW,
        };
        let name = HSTRING::from(path.as_os_str());
        // SAFETY: `name` est une chaîne terminée par zéro, valide pendant l'appel.
        let set = unsafe { SetFileAttributesW(&name, FILE_ATTRIBUTE_HIDDEN) };
        assert!(set.is_ok(), "impossible de cacher le fichier d'essai : {set:?}");
        // SAFETY: même chaîne, valide pendant l'appel.
        let attributes = unsafe { GetFileAttributesW(&name) };
        assert!(
            attributes != u32::MAX && attributes & FILE_ATTRIBUTE_HIDDEN.0 != 0,
            "le fichier d'essai n'est pas caché ({attributes:#x}) : le test ne prouverait rien"
        );
    }

    fn sorted(mut entries: Vec<RawEntry>) -> Vec<RawEntry> {
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    #[test]
    fn a_missing_place_is_an_empty_place() {
        let sandbox = Sandbox::new();
        for place in Place::ALL {
            assert_eq!(sandbox.host.entries(place).unwrap().entries, Vec::new(), "{place:?}");
        }
        assert_eq!(sandbox.host.approval(Place::UserRun, "Rien").unwrap(), None);
    }

    #[test]
    fn only_text_values_of_run_are_entries() {
        let sandbox = Sandbox::new();
        sandbox.put_text(Place::UserRun, "Texte", REG_SZ, r"C:\a\a.exe /x");
        sandbox.put_text(Place::UserRun, "Variable", REG_EXPAND_SZ, r"%ProgramFiles%\b\b.exe");
        sandbox.put(Place::UserRun, "Nombre", REG_DWORD, &[1, 0, 0, 0]);
        sandbox.put(Place::UserRun, "Binaire", REG_BINARY, &[1, 2, 3]);
        // Le nom vide est la valeur par défaut de la clé.
        sandbox.put_text(Place::UserRun, "", REG_SZ, r"C:\defaut.exe");
        let entries = sorted(sandbox.host.entries(Place::UserRun).unwrap().entries);
        assert_eq!(
            entries,
            vec![
                RawEntry::new("Texte", r"C:\a\a.exe /x"),
                // La commande est donnée telle qu'écrite : le développement est un autre geste.
                RawEntry::new("Variable", r"%ProgramFiles%\b\b.exe"),
            ]
        );
    }

    #[test]
    fn a_name_that_is_too_long_is_left_out_and_the_place_says_so() {
        let sandbox = Sandbox::new();
        sandbox.put_text(Place::UserRun, &"n".repeat(255), REG_SZ, "ok");
        // Au plafond : lu, et la place est complète.
        let place = sandbox.host.entries(Place::UserRun).unwrap();
        assert!(place.complete);
        assert_eq!(place.entries.len(), 1);
        sandbox.put_text(Place::UserRun, &"n".repeat(256), REG_SZ, "trop long");
        let place = sandbox.host.entries(Place::UserRun).unwrap();
        assert!(!place.complete, "un nom écarté se dit");
        let names: Vec<String> = place.entries.into_iter().map(|entry| entry.name).collect();
        assert_eq!(names, ["n".repeat(255)]);
    }

    #[test]
    fn a_command_that_is_too_long_keeps_its_entry_without_a_command() {
        let sandbox = Sandbox::new();
        sandbox.put_text(Place::UserRun, "court", REG_SZ, &"c".repeat(2048));
        sandbox.put_text(Place::UserRun, "long", REG_SZ, &"c".repeat(2049));
        let place = sandbox.host.entries(Place::UserRun).unwrap();
        assert!(place.complete, "l'entrée est gardée : rien n'est caché");
        let entries = sorted(place.entries);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], RawEntry::new("court", &"c".repeat(2048)));
        assert_eq!(entries[1], RawEntry::new("long", ""));
    }

    #[test]
    fn at_most_three_hundred_entries_per_place_and_the_place_says_so() {
        let sandbox = Sandbox::new();
        for n in 0..MAX_ENTRIES {
            sandbox.put_text(Place::UserRun32, &format!("app{n}"), REG_SZ, r"C:\a.exe");
        }
        let place = sandbox.host.entries(Place::UserRun32).unwrap();
        assert_eq!(place.entries.len(), MAX_ENTRIES);
        assert!(place.complete, "300 entrées : tout est lu");
        sandbox.put_text(Place::UserRun32, "une de trop", REG_SZ, r"C:\a.exe");
        let place = sandbox.host.entries(Place::UserRun32).unwrap();
        assert_eq!(place.entries.len(), MAX_ENTRIES);
        assert!(!place.complete, "la 301e entrée n'est pas lue : la liste est incomplète");
    }

    #[test]
    fn a_value_that_is_too_big_makes_the_place_incomplete() {
        let sandbox = Sandbox::new();
        sandbox.put_text(Place::UserRun, "normale", REG_SZ, r"C:\a.exe");
        // Plus d'un mébioctet : la valeur ne se lit pas en entier.
        sandbox.put_text(Place::UserRun, "enorme", REG_SZ, &"c".repeat(MAX_VALUE_BYTES));
        let place = sandbox.host.entries(Place::UserRun).unwrap();
        assert!(!place.complete);
        assert!(place.entries.iter().any(|entry| entry.name == "normale"));
    }

    #[test]
    fn the_six_places_are_separate_keys() {
        let sandbox = Sandbox::new();
        sandbox.put_text(Place::UserRun, "ur", REG_SZ, "1");
        sandbox.put_text(Place::UserRun32, "uw", REG_SZ, "2");
        sandbox.put_text(Place::MachineRun, "mr", REG_SZ, "3");
        sandbox.put_text(Place::MachineRun32, "mw", REG_SZ, "4");
        for (place, name) in [
            (Place::UserRun, "ur"),
            (Place::UserRun32, "uw"),
            (Place::MachineRun, "mr"),
            (Place::MachineRun32, "mw"),
        ] {
            let entries = sandbox.host.entries(place).unwrap().entries;
            assert_eq!(entries.len(), 1, "{place:?}");
            assert_eq!(entries[0].name, name);
        }
    }

    #[test]
    fn a_value_name_is_read_with_its_exact_case() {
        let sandbox = Sandbox::new();
        sandbox.put_text(Place::UserRun, "OneDrive", REG_SZ, r"C:\a.exe");
        sandbox.put_text(Place::UserRun, "ÉTÉ ünï", REG_SZ, r"C:\a.exe");
        let names: Vec<String> = sorted(sandbox.host.entries(Place::UserRun).unwrap().entries)
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["OneDrive", "ÉTÉ ünï"]);
        assert!(
            sandbox
                .host
                .entries(Place::UserRun)
                .unwrap()
                .entries
                .iter()
                .all(|entry| entry.name_exact)
        );
    }

    #[test]
    fn startup_folders_list_every_file_even_hidden_ones_but_not_desktop_ini() {
        let sandbox = Sandbox::new();
        let folder = sandbox.user_folder.path();
        fs::write(folder.join("script.bat"), "echo").unwrap();
        fs::write(folder.join("desktop.ini"), "[x]").unwrap();
        fs::write(folder.join("DESKTOP.INI"), "[x]").unwrap();
        fs::write(folder.join("Caché.cmd"), "echo").unwrap();
        fs::create_dir(folder.join("sous-dossier")).unwrap();
        fs::write(folder.join("sous-dossier").join("dedans.bat"), "echo").unwrap();
        // « Caché » : l'attribut est posé, puis relu. S'il ne se pose pas, le
        // test le dit et échoue : il ne passe pas sans avoir rien vérifié.
        hide(&folder.join("Caché.cmd"));
        let names: Vec<String> = sorted(sandbox.host.entries(Place::UserFolder).unwrap().entries)
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["Caché.cmd", "script.bat"]);
        // Le dossier du PC est un autre dossier.
        assert_eq!(sandbox.host.entries(Place::MachineFolder).unwrap().entries, Vec::new());
    }

    #[test]
    fn a_file_without_a_shortcut_target_is_its_own_quoted_path() {
        let sandbox = Sandbox::new();
        let folder = sandbox.user_folder.path();
        fs::write(folder.join("Mon script.bat"), "echo").unwrap();
        // Un « raccourci » qui n'en est pas un : sa cible ne se lit pas.
        fs::write(folder.join("cassé.lnk"), "pas un raccourci").unwrap();
        let entries = sorted(sandbox.host.entries(Place::UserFolder).unwrap().entries);
        let quoted = |name: &str| format!("\"{}\"", folder.join(name).display());
        assert_eq!(entries[0], RawEntry::new("Mon script.bat", &quoted("Mon script.bat")));
        assert_eq!(entries[1], RawEntry::new("cassé.lnk", &quoted("cassé.lnk")));
    }

    #[test]
    fn a_shortcut_gives_its_target_without_resolving_it() {
        let sandbox = Sandbox::new();
        let folder = sandbox.user_folder.path();
        let _com = ComScope::enter();
        // SAFETY: création d'un raccourci de test : classe et interface ci-dessous.
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.unwrap();
        let target = wide(OsStr::new(r"C:\Dossier qui n'existe pas\prog.exe"));
        // SAFETY: `target` est une chaîne terminée par zéro, valide pendant l'appel.
        unsafe { link.SetPath(PCWSTR(target.as_ptr())) }.unwrap();
        let file: IPersistFile = windows::core::Interface::cast(&link).unwrap();
        let path = wide(folder.join("Essai.LNK").as_os_str());
        // SAFETY: `path` est une chaîne terminée par zéro, valide pendant l'appel.
        unsafe { file.Save(PCWSTR(path.as_ptr()), true) }.unwrap();

        let entries = sandbox.host.entries(Place::UserFolder).unwrap().entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Essai.LNK");
        // La cible est donnée telle quelle, entre guillemets, même si elle n'existe pas.
        assert_eq!(entries[0].command, "\"C:\\Dossier qui n'existe pas\\prog.exe\"");
    }

    #[test]
    fn approvals_are_read_with_their_type_and_exact_bytes() {
        let sandbox = Sandbox::new();
        let cut = cut_bytes(133_760_000_000_000_000);
        sandbox.approve(Place::UserRun, "Coupée", REG_BINARY, &cut);
        sandbox.approve(Place::UserRun, "Nombre", REG_DWORD, &[3, 0, 0, 0]);
        sandbox.approve(Place::UserRun, "Long", REG_BINARY, &[7u8; 300]);
        let read = sandbox.host.approval(Place::UserRun, "Coupée").unwrap().unwrap();
        assert!(read.is_binary);
        assert_eq!(read.bytes, cut);
        // Le nom d'une valeur se compare sans la casse.
        assert!(sandbox.host.approval(Place::UserRun, "COUPÉE").unwrap().is_some());
        let number = sandbox.host.approval(Place::UserRun, "Nombre").unwrap().unwrap();
        assert!(!number.is_binary);
        assert!(
            sandbox.host.approval(Place::UserRun, "Long").is_err(),
            "plus long que la valeur de Windows"
        );
        assert_eq!(sandbox.host.approval(Place::UserRun, "Absente").unwrap(), None);
        // L'approbation d'une autre place n'est pas celle-ci.
        assert_eq!(sandbox.host.approval(Place::UserRun32, "Coupée").unwrap(), None);
    }

    #[test]
    fn an_approval_is_written_under_the_right_key_of_the_account() {
        let sandbox = Sandbox::new();
        let stamp = sandbox.host.file_time_now();
        for (place, sub) in [
            (Place::UserRun, "Run"),
            (Place::UserRun32, "Run32"),
            (Place::UserFolder, "StartupFolder"),
        ] {
            // La clé d'approbation n'existe pas encore : elle est créée.
            assert!(!sandbox.exists(&format!(r"{}\UserApproved\{sub}", sandbox.root)));
            sandbox.host.set_approval(place, "Essai Shinku.lnk", cut_bytes(stamp)).unwrap();
            let read = sandbox.host.approval(place, "Essai Shinku.lnk").unwrap().unwrap();
            assert_eq!(read.bytes, cut_bytes(stamp), "{place:?}");
            assert!(sandbox.exists(&format!(r"{}\UserApproved\{sub}", sandbox.root)));
            sandbox.host.set_approval(place, "Essai Shinku.lnk", restore_bytes()).unwrap();
            let read = sandbox.host.approval(place, "Essai Shinku.lnk").unwrap().unwrap();
            assert_eq!(read.bytes, restore_bytes(), "{place:?}");
        }
    }

    #[test]
    fn nothing_is_ever_written_for_the_machine() {
        let sandbox = Sandbox::new();
        for place in [Place::MachineRun, Place::MachineRun32, Place::MachineFolder] {
            let error = sandbox
                .host
                .set_approval(place, "Essai", cut_bytes(1))
                .expect_err("une place du PC ne s'écrit pas");
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{place:?}");
        }
        assert!(!sandbox.exists(&format!(r"{}\MachineApproved", sandbox.root)));
        // Un nom que Windows ne saurait pas passer est refusé avant le registre.
        assert!(sandbox.host.set_approval(Place::UserRun, "a\0b", restore_bytes()).is_err());
        assert!(sandbox.host.set_approval(Place::UserRun, "", restore_bytes()).is_err());
        assert!(!sandbox.exists(&format!(r"{}\UserApproved", sandbox.root)));
    }

    #[test]
    fn variables_and_files_and_windows_are_answered_by_windows() {
        let sandbox = Sandbox::new();
        let host = &sandbox.host;
        // Une variable que Windows définit toujours : aucune variable
        // d'environnement n'est posée par le test (il tourne avec d'autres).
        let root = host.expand("%SystemRoot%");
        assert!(root.eq_ignore_ascii_case(&host.windows_dir()), "{root}");
        assert_eq!(host.expand(r"%SystemRoot%\a.exe /x"), format!(r"{root}\a.exe /x"));
        assert_eq!(host.expand("%SHINKU_VARIABLE_INCONNUE%\\a"), "%SHINKU_VARIABLE_INCONNUE%\\a");
        assert_eq!(host.expand(""), "");
        let long = "x".repeat(9000);
        assert_eq!(host.expand(&long), long);

        let windows = host.windows_dir();
        assert!(!windows.is_empty() && !windows.ends_with('\\'), "{windows}");
        assert!(Path::new(&windows).is_dir(), "{windows}");
        assert!(host.file_time_now() > 133_000_000_000_000_000);
    }

    #[test]
    fn this_program_has_an_identity_or_none_but_never_fails() {
        let sandbox = Sandbox::new();
        let own = std::env::current_exe().unwrap();
        let identity = sandbox.host.identity(&own.to_string_lossy());
        // Un programme de test n'a pas forcément de ressource de version.
        if let Some(text) = identity.description {
            assert!(!text.is_empty());
        }
        assert_eq!(
            sandbox.host.identity(r"C:\Ce\Fichier\n'existe\pas.exe"),
            FileIdentity::default()
        );
    }

    // --- Fichiers, lecteurs, liens ---

    #[test]
    fn a_path_is_a_file_a_folder_absent_or_unsure() {
        let sandbox = Sandbox::new();
        let host = &sandbox.host;
        let folder = sandbox.user_folder.path();
        let file = folder.join("a.txt");
        fs::write(&file, "x").unwrap();
        assert_eq!(host.existence(&file.to_string_lossy()), Existence::File);
        assert_eq!(host.existence(&folder.to_string_lossy()), Existence::Folder);
        // Seul « introuvable » dit « absent ».
        assert_eq!(host.existence(&folder.join("rien.exe").to_string_lossy()), Existence::Absent);
        assert_eq!(host.existence(r"C:\Ce\Dossier\n'existe\pas\a.exe"), Existence::Absent);
        // Un nom que Windows refuse : on ne sait pas, ce n'est pas « absent ».
        assert_eq!(host.existence(&format!("{}\\a\0b.exe", folder.display())), Existence::Unsure);
        assert_eq!(host.existence(&format!("{}\\a<b>.exe", folder.display())), Existence::Unsure);
    }

    #[test]
    fn only_a_fixed_drive_is_local() {
        let sandbox = Sandbox::new();
        let host = &sandbox.host;
        assert!(host.is_local_fixed(&sandbox.user_folder.path().to_string_lossy()));
        assert!(host.is_local_fixed(r"C:\Windows\a.exe"));
        assert!(host.is_local_fixed("c:/windows/a.exe"));
        assert!(host.is_local_fixed(r"\\?\C:\Windows\a.exe"));
        // Ni réseau, ni relatif, ni forme inconnue.
        assert!(!host.is_local_fixed(r"\\serveur-qui-n-existe-pas\partage\a.exe"));
        assert!(!host.is_local_fixed("//serveur-qui-n-existe-pas/partage/a.exe"));
        assert!(!host.is_local_fixed(r"\\?\UNC\serveur\partage\a.exe"));
        assert!(!host.is_local_fixed(r"dossier\a.exe"));
        assert!(!host.is_local_fixed(""));
        // `A:` n'est pas un disque fixe (lecteur absent ou amovible).
        assert!(!host.is_local_fixed(r"A:\a.exe"));
    }

    #[test]
    fn nothing_is_read_outside_a_fixed_drive() {
        let sandbox = Sandbox::new();
        let host = &sandbox.host;
        // Aucun accès : la réponse est « incertain », jamais « absent ».
        assert_eq!(host.existence(r"A:\a.exe"), Existence::Unsure);
        assert_eq!(host.existence(r"\\serveur-qui-n-existe-pas\partage\a.exe"), Existence::Unsure);
        assert_eq!(host.existence("//serveur-qui-n-existe-pas/partage/a.exe"), Existence::Unsure);
        // Même un fichier qui existe, vu par un autre chemin que celui d'un lecteur fixe.
        assert_eq!(host.identity(r"A:\a.exe"), FileIdentity::default());
        assert_eq!(
            host.identity(r"\\serveur-qui-n-existe-pas\partage\a.exe"),
            FileIdentity::default()
        );
    }

    #[test]
    fn a_startup_folder_that_is_not_on_a_fixed_drive_is_not_read() {
        let sandbox = Sandbox::new();
        let mut layout = sandbox.host.layout.clone();
        layout.user_folder = Some(PathBuf::from(r"\\serveur-qui-n-existe-pas\profil\Démarrage"));
        layout.machine_folder = Some(PathBuf::from(r"A:\Démarrage"));
        let host = SystemStartupHost { layout };
        for place in [Place::UserFolder, Place::MachineFolder] {
            let error =
                host.entries(place).expect_err("une place hors d'un lecteur fixe ne se lit pas");
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{place:?}");
        }
    }

    #[test]
    fn a_link_in_the_startup_folder_is_listed_but_never_followed() {
        let sandbox = Sandbox::new();
        let folder = sandbox.user_folder.path();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("cible.exe");
        fs::write(&target, "x").unwrap();
        let made = std::os::windows::fs::symlink_file(&target, folder.join("Lien.exe"));
        let listed = made.is_ok()
            && fs::read_dir(folder).unwrap().any(|item| item.unwrap().file_name() == "Lien.exe");
        if !listed {
            // Windows refuse les liens sans privilège, et Wine ne les énumère
            // pas : l'essai de bout en bout est impossible ici. La règle du
            // lien est vérifiée à part (`a_link_is_never_followed_by_the_command`).
            eprintln!(
                "ESSAI DE BOUT EN BOUT NON FAIT : le lien symbolique ne se crée pas ou ne \
                 s'énumère pas ici ({made:?}). Il doit tourner sur le Windows de l'intégration \
                 continue (administrateur)."
            );
            return;
        }
        fs::write(folder.join("Normal.bat"), "echo").unwrap();
        let entries = sorted(sandbox.host.entries(Place::UserFolder).unwrap().entries);
        assert_eq!(entries.len(), 2, "le lien est listé : l'Explorateur le lance");
        assert_eq!(entries[0], RawEntry::new("Lien.exe", ""), "sans commande : cible inconnue");
        assert_eq!(entries[1].name, "Normal.bat");
        // Un lien n'est jamais pris pour un fichier ni suivi.
        assert_eq!(
            sandbox.host.existence(&folder.join("Lien.exe").to_string_lossy()),
            Existence::Unsure
        );
    }

    #[test]
    fn a_link_is_never_followed_by_the_command() {
        let sandbox = Sandbox::new();
        let folder = sandbox.user_folder.path();
        let _com = ComScope::enter();
        // Un vrai raccourci, dont la cible se lit sans peine.
        // SAFETY: création d'un raccourci de test : classe et interface ci-dessous.
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.unwrap();
        let target = wide(OsStr::new(r"C:\Cible\prog.exe"));
        // SAFETY: `target` est une chaîne terminée par zéro, valide pendant l'appel.
        unsafe { link.SetPath(PCWSTR(target.as_ptr())) }.unwrap();
        let file: IPersistFile = windows::core::Interface::cast(&link).unwrap();
        let path = folder.join("Raccourci.lnk");
        let name = wide(path.as_os_str());
        // SAFETY: `name` est une chaîne terminée par zéro, valide pendant l'appel.
        unsafe { file.Save(PCWSTR(name.as_ptr()), true) }.unwrap();

        assert_eq!(folder_command(&path, false), "\"C:\\Cible\\prog.exe\"");
        // Le même fichier, s'il est un lien : rien n'est lu, la cible est inconnue.
        assert_eq!(folder_command(&path, true), "");
        // Un fichier ordinaire est son propre chemin.
        let plain = folder.join("a b.bat");
        assert_eq!(folder_command(&plain, false), format!("\"{}\"", plain.display()));
        // Une commande trop longue est gardée, sans commande.
        let long = folder.join("x".repeat(MAX_COMMAND));
        assert_eq!(folder_command(&long, false), "");
    }

    #[test]
    fn a_target_buffer_that_is_full_is_too_long_never_a_cut_path() {
        let full = vec![b'a' as u16; 64];
        assert!(matches!(target_from_buffer(&full), Shortcut::TooLong), "pas de zéro");
        let mut almost = vec![b'a' as u16; 64];
        almost[63] = 0;
        assert!(matches!(target_from_buffer(&almost), Shortcut::TooLong), "tampon plein");
        let mut short = vec![0u16; 64];
        short[..3].copy_from_slice(&[b'C' as u16, b':' as u16, b'\\' as u16]);
        assert!(matches!(target_from_buffer(&short), Shortcut::Path(path) if path == r"C:\"));
        assert!(matches!(target_from_buffer(&[0u16; 64]), Shortcut::Unreadable));
    }

    // --- Chemins inexacts, bornes du dossier, liens, fichiers en ligne ---

    /// Un nom ou un chemin avec un surrogat isolé : de l'UTF-16 invalide.
    fn broken(prefix: &str, suffix: &str) -> OsString {
        let mut units: Vec<u16> = prefix.encode_utf16().collect();
        units.push(0xD800);
        units.extend(suffix.encode_utf16());
        OsString::from_wide(&units)
    }

    #[test]
    fn a_command_that_is_not_valid_utf16_is_empty_never_approximate() {
        let bytes = |units: &[u16]| -> Vec<u8> {
            units.iter().chain(&[0u16]).flat_map(|unit| unit.to_le_bytes()).collect()
        };
        let valid: Vec<u16> = r"C:\a\é.exe".encode_utf16().collect();
        assert_eq!(string_of(&bytes(&valid)), r"C:\a\é.exe");
        let mut invalid: Vec<u16> = r"C:\a\".encode_utf16().collect();
        invalid.push(0xD800);
        invalid.extend(".exe".encode_utf16());
        // Pas de U+FFFD : la cible ne se cherchera pas sous un autre nom.
        assert_eq!(string_of(&bytes(&invalid)), "");

        let mut buffer = vec![0u16; 64];
        buffer[..invalid.len()].copy_from_slice(&invalid);
        assert!(matches!(target_from_buffer(&buffer), Shortcut::Inexact));

        let path = PathBuf::from(broken(r"C:\Users\a\", ".exe"));
        assert_eq!(folder_command(&path, false), "", "un fichier au nom inexact : cible inconnue");
        let shortcut = PathBuf::from(broken(r"C:\Users\a\", ".lnk"));
        assert_eq!(folder_command(&shortcut, false), "");
    }

    #[test]
    fn a_file_whose_name_is_not_valid_utf16_is_listed_without_a_target() {
        let sandbox = Sandbox::new();
        let name = broken("", ".exe");
        if fs::write(sandbox.user_folder.path().join(&name), "x").is_err() {
            eprintln!(
                "NOM INEXACT NON CRÉABLE ICI : essai de bout en bout non fait (voir le test pur)"
            );
            return;
        }
        let place = sandbox.host.entries(Place::UserFolder).unwrap();
        assert_eq!(place.entries.len(), 1);
        assert!(!place.entries[0].name_exact);
        assert_eq!(place.entries[0].command, "", "jamais un chemin approximatif");
    }

    #[test]
    fn the_startup_folder_loop_is_bounded_even_by_what_it_skips() {
        let sandbox = Sandbox::new();
        let folder = sandbox.user_folder.path();
        for n in 0..(MAX_SCANNED as usize + 5) {
            fs::create_dir(folder.join(format!("d{n}"))).unwrap();
        }
        let place = sandbox.host.entries(Place::UserFolder).unwrap();
        assert!(place.entries.is_empty());
        assert!(!place.complete, "la borne atteinte se dit");
    }

    #[test]
    fn a_folder_link_is_not_an_app_but_a_file_link_is_listed() {
        // (est un fichier, est un lien, est un lien de dossier)
        assert_eq!(classify(true, false, false), Kind::File);
        assert_eq!(classify(false, true, false), Kind::Link, "lien vers un fichier : listé");
        assert_eq!(classify(false, false, false), Kind::Skip, "sous-dossier");
        assert_eq!(classify(false, true, true), Kind::Skip, "jonction ou lien de dossier");
        assert_eq!(classify(true, true, true), Kind::Skip);
    }

    #[test]
    fn only_not_found_errors_say_absent() {
        for code in [2, 3] {
            assert!(is_not_found(&io::Error::from_raw_os_error(code)), "{code}");
        }
        // 15 : lecteur invalide, 53 : chemin réseau introuvable, 67 : nom de
        // partage introuvable, 5 : accès refusé, 123 : nom invalide.
        for code in [5, 15, 53, 67, 123] {
            assert!(!is_not_found(&io::Error::from_raw_os_error(code)), "{code}");
        }
        assert!(!is_not_found(&io::Error::from(io::ErrorKind::NotFound)), "sans code brut");
    }

    #[test]
    fn an_online_only_file_is_never_read_for_its_identity() {
        assert!(reads_without_download(0x20), "ARCHIVE seul");
        assert!(reads_without_download(0x1 | 0x20 | 0x400), "lecture seule, point d'analyse");
        assert!(!reads_without_download(0x1000), "OFFLINE");
        assert!(!reads_without_download(0x0004_0000), "RECALL_ON_OPEN");
        assert!(!reads_without_download(0x0040_0000 | 0x400), "RECALL_ON_DATA_ACCESS");
        // Un fichier ordinaire se lit : l'identité de ce programme d'essai ne plante pas.
        let sandbox = Sandbox::new();
        let own = std::env::current_exe().unwrap();
        let attributes = fs::symlink_metadata(&own).unwrap().file_attributes();
        assert!(reads_without_download(attributes));
        let _ = sandbox.host.identity(&own.to_string_lossy());
    }

    // --- Les vraies clés ---

    #[test]
    fn the_real_layout_matches_the_six_places_of_the_spec() {
        let layout = Layout::real();
        let run = r"Software\Microsoft\Windows\CurrentVersion\Run".to_owned();
        let user32 = r"Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Run".to_owned();
        let approved = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";
        let wanted = [
            (Place::UserRun, Hive::Current, Some((run.clone(), KEY_WOW64_64KEY.0)), "Run"),
            (Place::UserRun32, Hive::Current, Some((user32, KEY_WOW64_64KEY.0)), "Run32"),
            (Place::UserFolder, Hive::Current, None, "StartupFolder"),
            (Place::MachineRun, Hive::Local, Some((run.clone(), KEY_WOW64_64KEY.0)), "Run"),
            (Place::MachineRun32, Hive::Local, Some((run, KEY_WOW64_32KEY.0)), "Run32"),
            (Place::MachineFolder, Hive::Local, None, "StartupFolder"),
        ];
        for (place, hive, run, sub) in wanted {
            let found = layout.place(place);
            assert_eq!(found.hive, hive, "{place:?} : ruche");
            assert_eq!(found.run, run, "{place:?} : clé Run et vue");
            assert_eq!(found.approved, format!(r"{approved}\{sub}"), "{place:?} : approbation");
        }
        // Les dossiers viennent de Windows, pas d'un chemin écrit en dur.
        assert!(layout.user_folder.is_none() && layout.machine_folder.is_none());
        assert!(*folder_id(Place::UserFolder) == FOLDERID_Startup);
        assert!(*folder_id(Place::MachineFolder) == FOLDERID_CommonStartup);
    }

    #[test]
    fn reading_the_six_real_places_neither_panics_nor_writes() {
        let host = SystemStartupHost::new();
        let keys: Vec<String> = ["Run", "Run32", "StartupFolder"]
            .iter()
            .map(|sub| {
                format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\{sub}")
            })
            .collect();
        let existing = |keys: &[String]| -> Vec<bool> {
            keys.iter()
                .map(|key| {
                    open_key(Hive::Current, key, KEY_READ | KEY_WOW64_64KEY).unwrap().is_some()
                })
                .collect()
        };
        let before = existing(&keys);
        for place in Place::ALL {
            let read = host.entries(place);
            if matches!(
                place,
                Place::UserRun | Place::UserRun32 | Place::MachineRun | Place::MachineRun32
            ) {
                assert!(read.is_ok(), "{place:?} : {read:?}");
            }
            if let Ok(read) = read {
                for entry in read.entries.iter().take(5) {
                    // Lire une approbation ne crée rien.
                    let _ = host.approval(place, &entry.name);
                }
            }
        }
        assert_eq!(existing(&keys), before, "lire ne crée aucune clé d'approbation");
    }

    #[test]
    fn a_name_that_is_not_valid_utf16_or_has_a_nul_is_not_exact() {
        let units = |text: &str| -> Vec<u16> { text.encode_utf16().collect() };
        assert_eq!(decode_name(&units("Discord")), ("Discord".to_owned(), true));
        assert_eq!(decode_name(&units("ÉTÉ")), ("ÉTÉ".to_owned(), true));
        // Un surrogat isolé : le nom est approximatif.
        let (text, exact) = decode_name(&[0x56, 0x72, 0xD800, 0x61]);
        assert!(!exact && text.contains('\u{fffd}'));
        // Un zéro au milieu : gardé, approximatif.
        let (text, exact) = decode_name(&units("a\0b"));
        assert!(!exact);
        assert_eq!(text, "a\0b");
    }

    // --- Le cœur sur la vraie machine d'essai ---

    #[test]
    fn the_core_cuts_and_restores_an_account_entry_on_the_real_registry() {
        let sandbox = Sandbox::new();
        let host = &sandbox.host;
        sandbox.put_text(Place::UserRun, "Essai Shinku Compte", REG_SZ, r"C:\Essai\essai.exe");
        sandbox.put_text(Place::MachineRun, "Essai Shinku PC", REG_SZ, r"C:\Essai\pc.exe");
        // Le PC aussi est lisible, sans interrupteur.
        let listing = startup::enumerate(host, None, None);
        assert!(listing.unreadable.is_empty());
        let user =
            listing.entries.iter().find(|entry| entry.raw_name == "Essai Shinku Compte").unwrap();
        let machine =
            listing.entries.iter().find(|entry| entry.raw_name == "Essai Shinku PC").unwrap();
        assert!(user.adjustable && user.state == State::Enabled);
        assert!(!machine.adjustable);

        let change = startup::toggle(host, None, user.id, false).unwrap();
        assert!(change.changed);
        let read = host.approval(Place::UserRun, "Essai Shinku Compte").unwrap().unwrap();
        assert_eq!(read.bytes, cut_bytes(change.file_time));
        let listing = startup::enumerate(host, None, None);
        let user =
            listing.entries.iter().find(|entry| entry.raw_name == "Essai Shinku Compte").unwrap();
        assert_eq!(user.state, State::Disabled);
        assert!(user.disabled_ms.is_some());

        startup::toggle(host, None, user.id, true).unwrap();
        let read = host.approval(Place::UserRun, "Essai Shinku Compte").unwrap().unwrap();
        assert_eq!(read.bytes, restore_bytes());

        // Le PC : refusé par le cœur, et rien n'est écrit.
        assert_eq!(startup::toggle(host, None, machine.id, false), Err(ToggleError::NotAdjustable));
        assert!(!sandbox.exists(&format!(r"{}\MachineApproved", sandbox.root)));
    }

    #[test]
    fn an_approval_in_a_form_shinku_does_not_know_is_never_overwritten() {
        let sandbox = Sandbox::new();
        let host = &sandbox.host;
        sandbox.put_text(Place::UserRun, "Bizarre", REG_SZ, r"C:\Essai\essai.exe");
        let strange = [6u8, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];
        sandbox.approve(Place::UserRun, "Bizarre", REG_BINARY, &strange);
        let listing = startup::enumerate(host, None, None);
        let entry = &listing.entries[0];
        assert_eq!(entry.state, State::Unknown);
        assert_eq!(startup::toggle(host, None, entry.id, false), Err(ToggleError::NotAdjustable));
        assert_eq!(startup::toggle(host, None, entry.id, true), Err(ToggleError::NotAdjustable));
        let read = host.approval(Place::UserRun, "Bizarre").unwrap().unwrap();
        assert_eq!(read.bytes, strange, "la valeur n'a pas bougé");
    }
}
