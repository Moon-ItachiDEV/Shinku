//! Le module « Désinstallation », sous Windows : [`AppsHost`] sur de vrais
//! appels documentés de l'API Windows.
//!
//! Ce module **lit** quatre places du registre (les trois sources de
//! « Programmes et fonctionnalités », plus `HKCU\Software\Wow6432Node\…` lue
//! seulement pour protéger), lance un désinstalleur par le shell, compte et
//! liste des dossiers, lit des raccourcis et envoie des restes à la corbeille.
//! **Il n'écrit jamais au registre** : aucune fonction d'écriture ni de
//! suppression de clé n'est importée ici hors des essais, qui travaillent sous
//! une clé à eux.
//!
//! Les clés sont dans un [`Layout`] : en production celui de Windows, dans les
//! essais une racine sous `HKCU`, jamais les vraies clés `Uninstall`.
//!
//! Rien n'est lu hors d'un lecteur fixe, aucun lien n'est suivi, et une borne
//! atteinte est dite (`RawSource::complete`), jamais tue.
#![allow(unsafe_code)]

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HANDLE,
    WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{GetFullPathNameW, GetLongPathNameW};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Registry::{
    KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_DWORD, REG_EXPAND_SZ, REG_SAM_FLAGS, REG_SZ,
    REG_VALUE_TYPE, RegEnumKeyExW, RegQueryInfoKeyW, RegQueryValueExW,
};
use windows::Win32::System::SystemInformation::{GetSystemDirectoryW, GetSystemWindowsDirectoryW};
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_LocalAppData, FOLDERID_LocalAppDataLow, FOLDERID_RoamingAppData,
    FOLDERID_StartMenu, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
    SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HRESULT, HSTRING, PCWSTR, PWSTR};

use super::startup::{
    ComScope, Hive, Key, Shortcut as LinkTarget, open_key, os_error, shortcut_target,
};
use super::{Environment, SystemEnvironment, known_folder_path, shell};
use crate::apps::host::{
    AccountFolder, AppsHost, Child, Existence, Launch, Measure, RawNumber, RawRegistration,
    RawSource, Shortcut, SystemDirs, Watch,
};
use crate::apps::{MAX_KEYS, Place, Source};
use crate::fs::{
    DirReader, EntryFlags, EntryKind, PathState, fold_path, inspect_dir_chain, to_display_path,
    to_internal_path,
};
use crate::guard::Guard;
use crate::platform::RecycleError;
use crate::startup::host::StartupHost;

use super::startup::SystemStartupHost;

/// Longueur maximale d'un nom de sous-clé, en unités UTF-16.
const MAX_KEY_NAME: usize = 255;
/// Taille maximale d'une valeur texte, en octets.
const MAX_VALUE_BYTES: usize = 32 * 1024;
/// Nombre d'éléments lus au plus dans un dossier.
const MAX_CHILDREN: usize = 4096;
/// Nombre d'éléments comptés au plus dans un dossier mesuré.
const MAX_MEASURED: u64 = 200_000;
/// Temps maximal d'une mesure.
const MEASURE_LIMIT: Duration = Duration::from_secs(5);
/// Nombre de raccourcis lus au plus.
const MAX_SHORTCUTS: usize = 2000;
/// Profondeur maximale de la recherche des raccourcis, sous la racine.
const SHORTCUT_DEPTH: usize = 3;

/// Où sont les clés : la ruche, le chemin et la vue de chaque place.
#[derive(Clone, Debug)]
struct Layout {
    machine: Hive,
    account: Hive,
    machine_path: String,
    machine32_path: String,
    account_path: String,
    account_wow_path: String,
    machine64_view: u32,
    machine32_view: u32,
}

impl Layout {
    /// Les vraies clés de Windows.
    fn real() -> Self {
        let uninstall = r"Microsoft\Windows\CurrentVersion\Uninstall";
        Self {
            machine: Hive::Local,
            account: Hive::Current,
            machine_path: format!(r"SOFTWARE\{uninstall}"),
            machine32_path: format!(r"SOFTWARE\{uninstall}"),
            account_path: format!(r"Software\{uninstall}"),
            // `HKCU\Software` n'est pas redirigé : le chemin est écrit en toutes lettres.
            account_wow_path: format!(r"Software\Wow6432Node\{uninstall}"),
            machine64_view: KEY_WOW64_64KEY.0,
            machine32_view: KEY_WOW64_32KEY.0,
        }
    }

    /// La ruche, le chemin et la vue d'une place.
    fn place(&self, place: Place) -> (Hive, &str, u32) {
        match place {
            Place::Machine64 => (self.machine, &self.machine_path, self.machine64_view),
            Place::Machine32 => (self.machine, &self.machine32_path, self.machine32_view),
            Place::Account => (self.account, &self.account_path, KEY_WOW64_64KEY.0),
            Place::AccountWow => (self.account, &self.account_wow_path, KEY_WOW64_64KEY.0),
        }
    }
}

/// La machine du module « Désinstallation ».
#[derive(Debug)]
pub(in crate::platform) struct SystemAppsHost {
    layout: Layout,
    /// Ce que le module « Démarrage » sait déjà faire : variables, lecteurs
    /// fixes, existence d'un chemin.
    base: SystemStartupHost,
    start: Instant,
    guard: OnceLock<Guard>,
    sync_roots: OnceLock<Vec<Vec<String>>>,
}

impl SystemAppsHost {
    pub(in crate::platform) fn new() -> Self {
        Self::with_layout(Layout::real())
    }

    fn with_layout(layout: Layout) -> Self {
        Self {
            layout,
            base: SystemStartupHost::new(),
            start: Instant::now(),
            guard: OnceLock::new(),
            sync_roots: OnceLock::new(),
        }
    }
}

/// Une valeur de registre lue avec son type.
enum Value {
    Text(String),
    Dword(u32),
}

/// Le texte d'une valeur `REG_SZ` : jusqu'au premier zéro, en UTF-16 **valide**.
/// Sinon rien : une conversion approximative ferait chercher un autre chemin.
fn strict_text(data: &[u8]) -> Option<String> {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16(&units).ok()
}

/// Lit une valeur, son type vérifié. Un type inattendu, un texte de plus de
/// 32 Kio ou qui n'est pas de l'UTF-16 valide : absent. `trimmed` devient vrai
/// quand une borne a écarté une valeur.
fn read_value(key: &Key, name: &str, trimmed: &mut bool) -> io::Result<Option<Value>> {
    let name = HSTRING::from(name);
    let mut kind = REG_VALUE_TYPE(0);
    let mut size = 0u32;
    // SAFETY: la clé est ouverte pour `key` ; `name` est une chaîne terminée par
    // zéro ; `kind` et `size` sont des variables locales ; sans tampon, Windows
    // ne fait que dire le type et la taille.
    let result = unsafe {
        RegQueryValueExW(key.0, &name, None, Some(&raw mut kind), None, Some(&raw mut size))
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS {
        return Err(os_error(result));
    }
    if kind == REG_DWORD {
        if size != 4 {
            return Ok(None);
        }
        let mut data = [0u8; 4];
        let mut length = 4u32;
        // SAFETY: `data` est un tampon local de quatre octets, et `length` annonce sa taille.
        let result = unsafe {
            RegQueryValueExW(
                key.0,
                &name,
                None,
                None,
                Some(data.as_mut_ptr()),
                Some(&raw mut length),
            )
        };
        return match result {
            _ if result == ERROR_SUCCESS => Ok(Some(Value::Dword(u32::from_le_bytes(data)))),
            _ if result == ERROR_FILE_NOT_FOUND || result == ERROR_MORE_DATA => Ok(None),
            _ => Err(os_error(result)),
        };
    }
    if kind != REG_SZ && kind != REG_EXPAND_SZ {
        return Ok(None);
    }
    if size as usize > MAX_VALUE_BYTES {
        *trimmed = true;
        return Ok(None);
    }
    // Deux octets de plus : un zéro final, même si la valeur n'en a pas.
    let mut data = vec![0u8; size as usize + 2];
    let mut length = size;
    // SAFETY: `data` est un tampon local d'au moins `length` octets, que `length` annonce.
    let result = unsafe {
        RegQueryValueExW(key.0, &name, None, None, Some(data.as_mut_ptr()), Some(&raw mut length))
    };
    if result == ERROR_MORE_DATA {
        *trimmed = true;
        return Ok(None);
    }
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS {
        return Err(os_error(result));
    }
    Ok(strict_text(&data[..(length as usize).min(data.len())]).map(Value::Text))
}

fn text(key: &Key, name: &str, trimmed: &mut bool) -> io::Result<Option<String>> {
    Ok(match read_value(key, name, trimmed)? {
        Some(Value::Text(text)) => Some(text),
        _ => None,
    })
}

fn dword(key: &Key, name: &str, trimmed: &mut bool) -> io::Result<Option<u32>> {
    Ok(match read_value(key, name, trimmed)? {
        Some(Value::Dword(number)) => Some(number),
        _ => None,
    })
}

impl SystemAppsHost {
    /// Une inscription, relue à l'instant. `trimmed` : une borne a écarté une valeur.
    fn read_registration(
        &self,
        place: Place,
        name: &str,
        trimmed: &mut bool,
    ) -> io::Result<Option<RawRegistration>> {
        let (hive, base, view) = self.layout.place(place);
        let path = format!(r"{base}\{name}");
        let Some(key) = open_key(hive, &path, KEY_READ | REG_SAM_FLAGS(view))? else {
            return Ok(None);
        };
        let system_component = match read_value(&key, "SystemComponent", trimmed)? {
            Some(Value::Dword(number)) => Some(RawNumber::Dword(number)),
            Some(Value::Text(text)) => Some(RawNumber::Text(text)),
            None => None,
        };
        Ok(Some(RawRegistration {
            key: name.to_owned(),
            display_name: text(&key, "DisplayName", trimmed)?,
            display_version: text(&key, "DisplayVersion", trimmed)?,
            publisher: text(&key, "Publisher", trimmed)?,
            install_location: text(&key, "InstallLocation", trimmed)?,
            install_date: text(&key, "InstallDate", trimmed)?,
            estimated_size: dword(&key, "EstimatedSize", trimmed)?,
            uninstall_string: text(&key, "UninstallString", trimmed)?,
            display_icon: text(&key, "DisplayIcon", trimmed)?,
            system_component,
            windows_installer: dword(&key, "WindowsInstaller", trimmed)?,
            no_remove: dword(&key, "NoRemove", trimmed)?,
            parent_key_name: text(&key, "ParentKeyName", trimmed)?,
            release_type: text(&key, "ReleaseType", trimmed)?,
        }))
    }
}

/// Le code Windows d'un `HRESULT` de la famille Win32 (`0x8007xxxx`).
fn win32_code(code: HRESULT) -> u32 {
    let raw = code.0 as u32;
    if raw & 0xFFFF_0000 == 0x8007_0000 { raw & 0xFFFF } else { raw }
}

/// `ERROR_CANCELLED` : la personne a refusé la fenêtre de Windows.
const ERROR_CANCELLED: u32 = 1223;

/// Ce que le lancement a donné, d'après le code d'erreur de Windows.
fn launch_error(code: u32) -> Launch {
    match code {
        ERROR_CANCELLED => Launch::Cancelled,
        // Fichier ou chemin introuvable (`ERROR_FILE_NOT_FOUND`, `ERROR_PATH_NOT_FOUND`).
        2 | 3 => Launch::Missing,
        other => Launch::Failed(other),
    }
}

/// La garde sur un processus lancé.
#[derive(Debug)]
struct ProcessWatch(OwnedHandle);

impl Watch for ProcessWatch {
    fn ended(&self) -> bool {
        // SAFETY: la poignée appartient à `self` et reste ouverte tant qu'il vit ;
        // un délai de zéro ne fait que tester l'état du processus.
        unsafe { WaitForSingleObject(HANDLE(self.0.as_raw_handle()), 0) == WAIT_OBJECT_0 }
    }
}

/// Lance par le shell, sur un fil à part où COM est prêt.
fn shell_launch(
    program: String,
    parameters: String,
    system: String,
    elevated: bool,
    watch: bool,
) -> Launch {
    let work = move || {
        // SAFETY: appel sans pointeur, sur un fil neuf ; équilibré plus bas si réussi.
        let com =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        let verb = HSTRING::from("runas");
        let file = HSTRING::from(program.as_str());
        let arguments = HSTRING::from(parameters.as_str());
        let directory = HSTRING::from(system.as_str());
        let mut mask = SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
        if watch {
            mask |= SEE_MASK_NOCLOSEPROCESS;
        }
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: mask,
            // Sans fenêtre propriétaire : Windows pose la sienne.
            lpVerb: if elevated { PCWSTR(verb.as_ptr()) } else { PCWSTR::null() },
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: if parameters.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(arguments.as_ptr())
            },
            // Le dossier système, pas celui de l'app : un processus dont le
            // dossier courant est celui de l'app peut empêcher son
            // désinstalleur de le retirer.
            lpDirectory: PCWSTR(directory.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        // SAFETY: `info` est une structure locale dont `cbSize` annonce la taille ;
        // les quatre chaînes sont terminées par zéro et vivent jusqu'à la fin de
        // l'appel, qui est synchrone (`SEE_MASK_NOASYNC`).
        let started = unsafe { ShellExecuteExW(&raw mut info) };
        let launch = match started {
            Ok(()) if watch && !info.hProcess.is_invalid() => {
                // SAFETY: Windows a rendu une poignée de processus ouverte
                // (`SEE_MASK_NOCLOSEPROCESS`) dont personne d'autre ne se sert :
                // `OwnedHandle` la fermera, une seule fois.
                let handle = unsafe { OwnedHandle::from_raw_handle(info.hProcess.0) };
                Launch::Started(Some(Box::new(ProcessWatch(handle))))
            }
            Ok(()) => Launch::Started(None),
            Err(error) => {
                let code = win32_code(error.code());
                launch_error(if code == 0 { info.hInstApp.0 as usize as u32 } else { code })
            }
        };
        if com.is_ok() {
            // SAFETY: appariée à l'initialisation réussie de ce même fil.
            unsafe { CoUninitialize() };
        }
        launch
    };
    std::thread::Builder::new()
        .name("shinku-desinstallation".to_owned())
        .spawn(work)
        .ok()
        .and_then(|thread| thread.join().ok())
        .unwrap_or(Launch::Failed(0))
}

/// Le nombre de sous-clés d'une clé ouverte.
fn subkey_count(key: &Key) -> io::Result<u32> {
    let mut subkeys = 0u32;
    // SAFETY: la clé est ouverte pour `key` ; `subkeys` est une variable
    // locale où Windows écrit un nombre ; les autres sorties sont ignorées.
    let result = unsafe {
        RegQueryInfoKeyW(
            key.0,
            None,
            None,
            None,
            Some(&raw mut subkeys),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    };
    if result != ERROR_SUCCESS {
        return Err(os_error(result));
    }
    Ok(subkeys)
}

/// Remplit un tampon par un appel de Windows qui rend la longueur écrite (ou,
/// si le tampon est trop court, la longueur qu'il faudrait). `None` en cas
/// d'échec, ou pour un texte qui n'est pas de l'UTF-16 valide.
fn grow(mut call: impl FnMut(&mut [u16]) -> u32) -> Option<String> {
    let mut buffer = vec![0u16; 1024];
    for _ in 0..2 {
        let length = call(&mut buffer) as usize;
        if length == 0 {
            return None;
        }
        if length < buffer.len() {
            return String::from_utf16(&buffer[..length]).ok();
        }
        // Un chemin ne dépasse pas 32 767 unités.
        if length > 32_768 {
            return None;
        }
        buffer = vec![0u16; length + 1];
    }
    None
}

/// Un chemin du module sous la forme interne de Windows.
fn internal(path: &str) -> io::Result<PathBuf> {
    to_internal_path(Path::new(path))
}

/// Un chemin interne sous la forme que le module comprend (`C:\…`). `None` si
/// le nom n'est pas de l'Unicode valide.
fn displayed(path: &Path) -> Option<String> {
    to_display_path(path).to_str().map(str::to_owned)
}

fn read_error(error: crate::fs::ReadDirError) -> io::Error {
    use crate::fs::ReadDirError;
    match error {
        ReadDirError::NotFound => io::Error::new(io::ErrorKind::NotFound, "introuvable"),
        ReadDirError::AccessDenied => io::Error::new(io::ErrorKind::PermissionDenied, "refusé"),
        other => io::Error::other(other.to_string()),
    }
}

impl AppsHost for SystemAppsHost {
    fn registrations(&self, place: Place) -> io::Result<RawSource> {
        let (hive, base, view) = self.layout.place(place);
        let Some(key) = open_key(hive, base, KEY_READ | REG_SAM_FLAGS(view))? else {
            return Ok(RawSource::complete(Vec::new()));
        };
        let subkeys = subkey_count(&key)?;
        // Une borne atteinte est dite : jamais une liste présentée complète
        // alors qu'elle ne l'est pas.
        let mut complete = subkeys as usize <= MAX_KEYS;
        let mut entries = Vec::new();
        let mut name = vec![0u16; MAX_KEY_NAME + 1];
        let wanted = subkeys.min(MAX_KEYS as u32);
        let mut visited = 0u32;
        for index in 0..wanted {
            let mut length = name.len() as u32;
            // SAFETY: la clé est ouverte pour `key` ; `name` est un tampon local
            // et `length` annonce sa taille en caractères, zéro final compris.
            let result = unsafe {
                RegEnumKeyExW(
                    key.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &raw mut length,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if result == ERROR_NO_MORE_ITEMS {
                break;
            }
            visited += 1;
            // Un nom de plus de 255 unités, ou une énumération qui échoue : sauté, et dit.
            if result != ERROR_SUCCESS {
                complete = false;
                continue;
            }
            let Ok(text) = String::from_utf16(&name[..(length as usize).min(name.len())]) else {
                complete = false;
                continue;
            };
            let mut trimmed = false;
            match self.read_registration(place, &text, &mut trimmed) {
                Ok(Some(raw)) => {
                    complete &= !trimmed;
                    entries.push(raw);
                }
                // Disparue entre-temps : rien à dire.
                Ok(None) => {}
                Err(_) => complete = false,
            }
        }
        // Le registre a changé pendant la lecture : des indices ont glissé, une
        // clé a pu être sautée. La liste est dite incomplète.
        if visited != wanted || subkey_count(&key)? != subkeys {
            complete = false;
        }
        Ok(RawSource { entries, complete })
    }

    fn registration(&self, source: Source, key: &str) -> io::Result<Option<RawRegistration>> {
        if key.is_empty() || key.contains(['\\', '\0']) || key.encode_utf16().count() > MAX_KEY_NAME
        {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "nom de clé invalide"));
        }
        self.read_registration(source.place(), key, &mut false)
    }

    fn expand(&self, text: &str) -> String {
        self.base.expand(text)
    }

    fn is_local_fixed(&self, path: &str) -> bool {
        self.base.is_local_fixed(path)
    }

    fn existence(&self, path: &str) -> Existence {
        self.base.existence(path)
    }

    fn system_dirs(&self) -> Option<SystemDirs> {
        let read = |fetch: &dyn Fn(&mut [u16]) -> u32| {
            let mut buffer = [0u16; 512];
            let length = fetch(&mut buffer) as usize;
            (length != 0 && length < buffer.len())
                .then(|| String::from_utf16(&buffer[..length]).ok())
                .flatten()
                .map(|text| text.trim_end_matches(['\\', '/']).to_owned())
        };
        // SAFETY: `buffer` est un tampon local ; Windows n'y écrit pas plus que sa taille.
        let system = read(&|buffer| unsafe { GetSystemDirectoryW(Some(buffer)) })?;
        // SAFETY: comme ci-dessus.
        let windows = read(&|buffer| unsafe { GetSystemWindowsDirectoryW(Some(buffer)) })?;
        Some(SystemDirs { system, windows })
    }

    fn launch(&self, program: &str, parameters: &str, elevated: bool, watch: bool) -> Launch {
        let Some(dirs) = self.system_dirs() else { return Launch::Failed(0) };
        shell_launch(program.to_owned(), parameters.to_owned(), dirs.system, elevated, watch)
    }

    fn account_folder(&self, folder: AccountFolder) -> Option<String> {
        let id = match folder {
            AccountFolder::Roaming => &FOLDERID_RoamingAppData,
            AccountFolder::Local => &FOLDERID_LocalAppData,
            AccountFolder::LocalLow => &FOLDERID_LocalAppDataLow,
            AccountFolder::StartMenu => &FOLDERID_StartMenu,
            AccountFolder::Desktop => &FOLDERID_Desktop,
        };
        // La forme que le module comprend (`C:\…`), jamais `\\?\C:\…`.
        displayed(&known_folder_path(id)?)
    }

    fn plain_chain(&self, path: &str) -> bool {
        internal(path).is_ok_and(|path| matches!(inspect_dir_chain(&path), Ok(PathState::RealDir)))
    }

    fn children(&self, path: &str) -> io::Result<Vec<Child>> {
        let folder = internal(path)?;
        let mut children = Vec::new();
        let mut overflow = false;
        DirReader::new()
            .read(&folder, None, &mut |name, meta| {
                if children.len() >= MAX_CHILDREN {
                    overflow = true;
                    return;
                }
                // Un nom qui n'est pas de l'Unicode valide ne se compare à rien.
                if let Some(name) = name.to_str() {
                    children.push(Child {
                        name: name.to_owned(),
                        is_dir: meta.kind == EntryKind::Dir,
                        is_link: meta.kind == EntryKind::Link
                            || meta.flags.contains(EntryFlags::SPECIAL),
                    });
                }
            })
            .map_err(read_error)?;
        if overflow {
            return Err(io::Error::other("trop d'éléments dans ce dossier"));
        }
        Ok(children)
    }

    fn shortcuts(&self, root: &str) -> io::Result<Vec<Shortcut>> {
        let root = internal(root)?;
        let _com = ComScope::enter();
        let mut found = Vec::new();
        let mut pending = vec![(root, 1usize)];
        while let Some((folder, depth)) = pending.pop() {
            let mut files: Vec<PathBuf> = Vec::new();
            let mut folders: Vec<PathBuf> = Vec::new();
            let read = DirReader::new().read(&folder, None, &mut |name, meta| match meta.kind {
                EntryKind::Dir if depth < SHORTCUT_DEPTH && meta.is_walkable_dir() => {
                    folders.push(folder.join(name));
                }
                EntryKind::File => {
                    let is_link = Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"));
                    if is_link {
                        files.push(folder.join(name));
                    }
                }
                _ => {}
            });
            // Le dossier de départ doit se lire ; un sous-dossier qui ne se lit
            // pas est sauté.
            if let Err(error) = read {
                if depth == 1 {
                    return Err(read_error(error));
                }
                continue;
            }
            for file in files {
                if found.len() >= MAX_SHORTCUTS {
                    return Ok(found);
                }
                let Some(path) = displayed(&file) else { continue };
                // La cible brute : jamais résolue, jamais cherchée.
                let target = match shortcut_target(&file) {
                    LinkTarget::Path(target) => Some(target),
                    _ => None,
                };
                found.push(Shortcut { path, target });
            }
            pending.extend(folders.into_iter().map(|next| (next, depth + 1)));
        }
        Ok(found)
    }

    fn measure(&self, path: &str) -> Option<Measure> {
        let top = internal(path).ok()?;
        let started = Instant::now();
        let metadata = std::fs::symlink_metadata(&top).ok()?;
        if metadata.is_file() {
            return Some(Measure { bytes: metadata.len(), files: 1, has_link: false });
        }
        let mut total = Measure { bytes: 0, files: 0, has_link: false };
        let mut seen = 0u64;
        let mut pending = vec![top];
        let mut reader = DirReader::new();
        while let Some(folder) = pending.pop() {
            let mut failed = false;
            let mut inner: Vec<PathBuf> = Vec::new();
            let result = reader.read(&folder, None, &mut |name, meta| {
                seen += 1;
                match meta.kind {
                    EntryKind::File => {
                        total.bytes = total.bytes.saturating_add(meta.size);
                        total.files += 1;
                    }
                    EntryKind::Dir if meta.is_walkable_dir() => inner.push(folder.join(name)),
                    // Un lien, ou un point d'analyse inconnu : jamais suivi, jamais compté.
                    EntryKind::Link | EntryKind::Dir | EntryKind::Other => total.has_link = true,
                }
                if seen > MAX_MEASURED {
                    failed = true;
                }
            });
            if result.is_err() || failed || started.elapsed() > MEASURE_LIMIT {
                return None;
            }
            pending.extend(inner);
        }
        Some(total)
    }

    fn recycle(&self, path: &str) -> Result<(), RecycleError> {
        let path = internal(path).map_err(|error| RecycleError::Failed(error.to_string()))?;
        shell::recycle(&path)
    }

    fn uptime(&self) -> Duration {
        self.start.elapsed()
    }

    fn own_dir(&self) -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        let folder = displayed(exe.parent()?)?;
        Some(folder.trim_end_matches(['\\', '/']).to_owned())
    }

    fn root_allowed(&self, path: &str) -> bool {
        let guard = self.guard.get_or_init(|| Guard::for_machine(&SystemEnvironment::new()));
        internal(path).is_ok_and(|path| guard.check_rule_root(&path).is_ok())
    }

    fn long_path(&self, path: &str) -> Option<String> {
        let wanted = HSTRING::from(path);
        let full = grow(|buffer| {
            // SAFETY: `wanted` est une chaîne terminée par zéro ; `buffer` est un
            // tampon local dont Windows ne dépasse pas la taille.
            unsafe { GetFullPathNameW(&wanted, Some(buffer), None) }
        })?;
        let full = HSTRING::from(full);
        let long = grow(|buffer| {
            // SAFETY: comme ci-dessus ; `full` est terminée par zéro.
            unsafe { GetLongPathNameW(&full, Some(buffer)) }
        })?;
        let shown = displayed(Path::new(&long))?;
        Some(shown.trim_end_matches(['\\', '/']).to_owned()).filter(|text| !text.is_empty())
    }

    fn in_sync_root(&self, path: &str) -> bool {
        let roots = self.sync_roots.get_or_init(|| {
            SystemEnvironment::new()
                .sync_roots()
                .iter()
                .filter_map(|root| fold_path(root))
                .collect()
        });
        // Un chemin qu'on ne sait pas lire est traité comme synchronisé : on n'y touche pas.
        let Some(target) = internal(path).ok().and_then(|path| fold_path(&path)) else {
            return true;
        };
        roots.iter().any(|root| target.starts_with(root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{OsStr, OsString};
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile};
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, RegCreateKeyExW,
        RegDeleteTreeW, RegSetValueExW,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    use crate::platform::windows::startup::wide;

    /// Une racine d'essai sous `HKCU`, supprimée à la fin : les essais n'écrivent
    /// jamais sous les vraies clés `Uninstall`.
    struct Sandbox {
        root: String,
        host: SystemAppsHost,
    }

    impl Sandbox {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |time| time.as_nanos());
            let root = format!(r"Software\Shinku-Essai-Apps-{}-{unique}", std::process::id());
            let layout = Layout {
                machine: Hive::Current,
                account: Hive::Current,
                machine_path: format!(r"{root}\M64"),
                machine32_path: format!(r"{root}\M32"),
                account_path: format!(r"{root}\Ac"),
                account_wow_path: format!(r"{root}\AcWow"),
                // Sous `HKCU\Software`, il n'y a pas de vue 32 bits à part.
                machine64_view: KEY_WOW64_64KEY.0,
                machine32_view: KEY_WOW64_64KEY.0,
            };
            Self { root, host: SystemAppsHost::with_layout(layout) }
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

        fn registration_key(&self, place: Place, name: &str) -> Key {
            let (_, base, _) = self.host.layout.place(place);
            self.create(&format!(r"{base}\{name}"))
        }

        fn put(&self, place: Place, name: &str, value: &str, kind: REG_VALUE_TYPE, data: &[u8]) {
            let key = self.registration_key(place, name);
            // SAFETY: clé ouverte ; `data` est un tampon local.
            let result =
                unsafe { RegSetValueExW(key.0, &HSTRING::from(value), None, kind, Some(data)) };
            assert_eq!(result, ERROR_SUCCESS);
        }

        fn put_text(
            &self,
            place: Place,
            name: &str,
            value: &str,
            kind: REG_VALUE_TYPE,
            text: &str,
        ) {
            let data: Vec<u8> =
                text.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect();
            self.put(place, name, value, kind, &data);
        }

        fn put_dword(&self, place: Place, name: &str, value: &str, number: u32) {
            self.put(place, name, value, REG_DWORD, &number.to_le_bytes());
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            // SAFETY: chaîne terminée par zéro ; ne supprime que la racine d'essai.
            let _ =
                unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(self.root.as_str())) };
        }
    }

    #[test]
    fn a_missing_source_is_an_empty_source() {
        let sandbox = Sandbox::new();
        for place in Place::ALL {
            let source = sandbox.host.registrations(place).unwrap();
            assert_eq!((source.entries.len(), source.complete), (0, true), "{place:?}");
        }
        assert_eq!(sandbox.host.registration(Source::Account, "Rien").unwrap(), None);
    }

    #[test]
    fn values_are_read_with_their_type() {
        let sandbox = Sandbox::new();
        let place = Place::Account;
        sandbox.put_text(place, "Essai", "DisplayName", REG_SZ, "Essai Shinku App");
        sandbox.put_text(place, "Essai", "InstallLocation", REG_EXPAND_SZ, r"%LOCALAPPDATA%\Essai");
        sandbox.put_text(place, "Essai", "UninstallString", REG_SZ, r#""C:\x\retirer.cmd" /a"#);
        sandbox.put_dword(place, "Essai", "EstimatedSize", 2048);
        sandbox.put_dword(place, "Essai", "NoRemove", 1);
        sandbox.put_dword(place, "Essai", "SystemComponent", 1);
        // Un texte là où un DWORD est attendu : absent. Un DWORD là où un texte est attendu : absent.
        sandbox.put_text(place, "Essai", "WindowsInstaller", REG_SZ, "1");
        sandbox.put_dword(place, "Essai", "Publisher", 7);
        let raw = sandbox.host.registration(Source::Account, "essai").unwrap().expect("présente");
        assert_eq!(raw.key, "essai", "le nom de la clé est celui qu'on a demandé");
        assert_eq!(raw.display_name.as_deref(), Some("Essai Shinku App"));
        // Le texte est donné tel qu'écrit : le développement est un autre geste.
        assert_eq!(raw.install_location.as_deref(), Some(r"%LOCALAPPDATA%\Essai"));
        assert_eq!(raw.uninstall_string.as_deref(), Some(r#""C:\x\retirer.cmd" /a"#));
        assert_eq!(raw.estimated_size, Some(2048));
        assert_eq!(raw.no_remove, Some(1));
        assert_eq!(raw.system_component, Some(RawNumber::Dword(1)));
        assert_eq!(raw.windows_installer, None);
        assert_eq!(raw.publisher, None);
        assert_eq!(raw.display_version, None);
        // `SystemComponent` écrit en texte.
        sandbox.put_text(place, "Texte", "SystemComponent", REG_SZ, "1");
        let raw = sandbox.host.registration(Source::Account, "Texte").unwrap().unwrap();
        assert_eq!(raw.system_component, Some(RawNumber::Text("1".to_owned())));
    }

    #[test]
    fn the_four_places_are_four_separate_keys() {
        let sandbox = Sandbox::new();
        for (place, name) in [
            (Place::Machine64, "m64"),
            (Place::Machine32, "m32"),
            (Place::Account, "ac"),
            (Place::AccountWow, "wow"),
        ] {
            sandbox.put_text(place, name, "DisplayName", REG_SZ, name);
        }
        for (place, name) in [
            (Place::Machine64, "m64"),
            (Place::Machine32, "m32"),
            (Place::Account, "ac"),
            (Place::AccountWow, "wow"),
        ] {
            let source = sandbox.host.registrations(place).unwrap();
            let keys: Vec<&str> = source.entries.iter().map(|raw| raw.key.as_str()).collect();
            assert_eq!(keys, [name], "{place:?}");
        }
        assert!(sandbox.host.registration(Source::Machine64, "ac").unwrap().is_none());
    }

    #[test]
    fn a_text_that_is_not_valid_utf16_or_too_big_is_absent_and_the_source_says_so() {
        let sandbox = Sandbox::new();
        let place = Place::Account;
        // Un surrogat isolé : de l'UTF-16 invalide.
        let broken: Vec<u8> =
            [0x0041u16, 0xD800, 0x0042, 0].into_iter().flat_map(u16::to_le_bytes).collect();
        sandbox.put(place, "Cassee", "DisplayName", REG_SZ, &broken);
        sandbox.put_text(place, "Cassee", "Publisher", REG_SZ, "Éditeur");
        let raw = sandbox.host.registration(Source::Account, "Cassee").unwrap().unwrap();
        assert_eq!(raw.display_name, None, "jamais une conversion approximative");
        assert_eq!(raw.publisher.as_deref(), Some("Éditeur"));

        // 32 Kio : lu. Un octet de plus : absent, et la place le dit.
        sandbox.put_text(place, "Grosse", "DisplayName", REG_SZ, &"n".repeat(16 * 1024 - 1));
        let source = sandbox.host.registrations(place).unwrap();
        let big = source.entries.iter().find(|raw| raw.key == "Grosse").unwrap();
        assert_eq!(big.display_name.as_ref().map(String::len), Some(16 * 1024 - 1));
        sandbox.put_text(place, "Grosse", "DisplayName", REG_SZ, &"n".repeat(16 * 1024 + 1));
        let source = sandbox.host.registrations(place).unwrap();
        assert!(!source.complete, "une valeur écartée par une borne se dit");
        let big = source.entries.iter().find(|raw| raw.key == "Grosse").unwrap();
        assert_eq!(big.display_name, None);
    }

    /// Retire un dossier d'essai, quoi qu'il arrive.
    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn account_folders_are_plain_paths_and_a_real_search_finds_settings() {
        use crate::apps::leftovers::{Snapshot, search};
        use crate::apps::paths::is_plain;
        use crate::apps::{Scope, names};

        let sandbox = Sandbox::new();
        // Chaque dossier du compte est rendu sous la forme `C:\…`, jamais
        // `\\?\C:\…` : sans quoi aucun reste de réglages n'est jamais proposé.
        for folder in [
            AccountFolder::Roaming,
            AccountFolder::Local,
            AccountFolder::LocalLow,
            AccountFolder::StartMenu,
            AccountFolder::Desktop,
        ] {
            let path = sandbox.host.account_folder(folder).expect("dossier connu");
            assert!(is_plain(&path), "{folder:?} : {path}");
            assert!(is_plain(&format!(r"{path}\Enfant")), "{folder:?} : {path}");
        }
        // Les autres chemins rendus par l'hôte sont dans la même forme.
        let own = sandbox.host.own_dir().expect("dossier du programme");
        assert!(is_plain(&own), "{own}");
        let dirs = sandbox.host.system_dirs().expect("dossiers de Windows");
        assert!(is_plain(&dirs.system) && is_plain(&dirs.windows), "{dirs:?}");

        // Une recherche de bout en bout, dans un vrai dossier de réglages.
        let letters: String = std::process::id()
            .to_string()
            .chars()
            .map(|digit| char::from(b'a' + digit.to_digit(10).unwrap_or(0) as u8))
            .collect();
        let name = format!("Essai Shinku Sonde {letters}");
        let roaming = sandbox.host.account_folder(AccountFolder::Roaming).unwrap();
        let settings = PathBuf::from(&roaming).join(&name);
        std::fs::create_dir_all(&settings).unwrap();
        let _cleanup = Dir(settings.clone());
        std::fs::write(settings.join("reglages.json"), b"{}").unwrap();
        let snapshot = Snapshot {
            name: name.clone(),
            bare: names::bare_name(&name),
            publisher: None,
            scope: Scope::Account,
            location: None,
        };
        let mut next = 0;
        let found = search(&sandbox.host, &snapshot, &mut next);
        assert!(found.complete && !found.family, "{found:?}");
        let paths: Vec<&str> = found.leftovers.iter().map(|left| left.path.as_str()).collect();
        assert_eq!(paths, [format!(r"{roaming}\{name}")], "un reste de réglages est proposé");
    }

    #[test]
    fn a_path_is_put_back_in_long_form_or_not_at_all() {
        let sandbox = Sandbox::new();
        let own = sandbox.host.own_dir().expect("dossier du programme");
        assert_eq!(sandbox.host.long_path(&format!(r"{own}\.")).as_deref(), Some(own.as_str()));
        assert_eq!(
            sandbox.host.long_path(&format!(r"\\?\{own}")).as_deref(),
            Some(own.as_str()),
            "le préfixe de chemin long est retiré"
        );
        assert_eq!(sandbox.host.long_path(r"C:\ce dossier\n'existe\pas"), None);
    }

    #[test]
    fn a_key_name_that_is_too_long_is_left_out_and_the_source_says_so() {
        let sandbox = Sandbox::new();
        let place = Place::Machine32;
        sandbox.put_text(place, &"n".repeat(255), "DisplayName", REG_SZ, "au plafond");
        let source = sandbox.host.registrations(place).unwrap();
        assert!(source.complete);
        assert_eq!(source.entries.len(), 1);
        sandbox.put_text(place, &"n".repeat(256), "DisplayName", REG_SZ, "trop long");
        let source = sandbox.host.registrations(place).unwrap();
        assert!(!source.complete);
        assert_eq!(source.entries.len(), 1);
    }

    #[test]
    fn at_most_4096_keys_per_source_and_the_source_says_so() {
        let sandbox = Sandbox::new();
        let place = Place::AccountWow;
        for n in 0..MAX_KEYS {
            let _ = sandbox.registration_key(place, &format!("app{n}"));
        }
        let source = sandbox.host.registrations(place).unwrap();
        assert_eq!(source.entries.len(), MAX_KEYS);
        assert!(source.complete, "4096 clés : tout est lu");
        let _ = sandbox.registration_key(place, "une de trop");
        let source = sandbox.host.registrations(place).unwrap();
        assert_eq!(source.entries.len(), MAX_KEYS);
        assert!(!source.complete, "la 4097e clé n'est pas lue : la liste est incomplète");
    }

    #[test]
    fn a_bad_key_name_is_an_error_never_an_absence() {
        let sandbox = Sandbox::new();
        for key in ["", r"a\b", "a\0b", &"x".repeat(256)] {
            let result = sandbox.host.registration(Source::Account, key);
            assert!(result.is_err(), "{key:?}");
        }
    }

    #[test]
    fn launch_errors_are_told_apart() {
        assert!(matches!(launch_error(1223), Launch::Cancelled));
        assert!(matches!(launch_error(2), Launch::Missing));
        assert!(matches!(launch_error(3), Launch::Missing));
        assert!(matches!(launch_error(5), Launch::Failed(5)));
        assert_eq!(win32_code(HRESULT(0x8007_04C7_u32 as i32)), 1223);
        assert_eq!(win32_code(HRESULT(0x8000_4005_u32 as i32)), 0x8000_4005);
    }

    #[test]
    fn a_program_is_launched_from_the_system_folder_and_followed_when_asked() {
        let sandbox = Sandbox::new();
        let host = &sandbox.host;
        let dirs = host.system_dirs().expect("Windows donne ses dossiers");
        assert!(dirs.system.to_lowercase().ends_with("system32"), "{dirs:?}");
        let cmd = format!(r"{}\cmd.exe", dirs.system);
        // Sans suivi : aucune garde.
        let launched = host.launch(&cmd, "/c exit 0", false, false);
        assert!(matches!(launched, Launch::Started(None)), "{launched:?}");
        // Avec suivi : la garde dit quand le processus est sorti.
        let Launch::Started(Some(watch)) = host.launch(&cmd, "/c exit 0", false, true) else {
            // Wine ne rend pas toujours la poignée : l'essai ne prouve alors rien de plus.
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        while !watch.ended() {
            assert!(Instant::now() < deadline, "le processus est sorti depuis longtemps");
            std::thread::sleep(Duration::from_millis(20));
        }
        // Un fichier qui n'existe pas : introuvable, jamais un échec vague.
        let missing = host.launch(&format!(r"{}\n'existe-pas.exe", dirs.system), "", false, false);
        assert!(matches!(missing, Launch::Missing), "{missing:?}");
    }

    #[test]
    fn folders_are_listed_and_counted_without_following_anything() {
        let sandbox = Sandbox::new();
        let host = &sandbox.host;
        let workspace = tempfile::tempdir().unwrap();
        let root = workspace.path().join("Essai Shinku App");
        std::fs::create_dir_all(root.join("a").join("b")).unwrap();
        std::fs::write(root.join("un.txt"), [1u8; 100]).unwrap();
        std::fs::write(root.join("a").join("deux.txt"), [2u8; 200]).unwrap();
        std::fs::write(root.join("a").join("b").join("trois.txt"), [3u8; 300]).unwrap();
        let path = root.to_str().unwrap();

        let mut names: Vec<(String, bool)> = host
            .children(path)
            .unwrap()
            .into_iter()
            .map(|child| (child.name, child.is_dir))
            .collect();
        names.sort();
        assert_eq!(names, [("a".to_owned(), true), ("un.txt".to_owned(), false)]);
        assert_eq!(host.measure(path), Some(Measure { bytes: 600, files: 3, has_link: false }));
        assert_eq!(
            host.measure(root.join("un.txt").to_str().unwrap()),
            Some(Measure { bytes: 100, files: 1, has_link: false })
        );
        assert_eq!(host.measure(root.join("absent").to_str().unwrap()), None);
        assert!(host.children(root.join("absent").to_str().unwrap()).is_err());
        assert!(host.plain_chain(path));
        assert!(
            !host.plain_chain(root.join("un.txt").to_str().unwrap()),
            "un fichier n'est pas un dossier"
        );
        assert_eq!(host.existence(path), Existence::Folder);
    }

    #[test]
    fn shortcuts_are_read_to_their_raw_target_and_never_resolved() {
        let sandbox = Sandbox::new();
        let workspace = tempfile::tempdir().unwrap();
        let root = workspace.path().join("Start Menu");
        let deep = root.join("Programs").join("Essai").join("Trop profond");
        std::fs::create_dir_all(&deep).unwrap();
        let make = |folder: &Path, name: &str, target: &str| {
            let _com = ComScope::enter();
            // SAFETY: création d'un raccourci de test : classe et interface ci-dessous.
            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.unwrap();
            let target = wide(OsStr::new(target));
            // SAFETY: `target` est une chaîne terminée par zéro, valide pendant l'appel.
            unsafe { link.SetPath(PCWSTR(target.as_ptr())) }.unwrap();
            let file: IPersistFile = windows::core::Interface::cast(&link).unwrap();
            let path = wide(folder.join(name).as_os_str());
            // SAFETY: `path` est une chaîne terminée par zéro, valide pendant l'appel.
            unsafe { file.Save(PCWSTR(path.as_ptr()), true) }.unwrap();
        };
        make(&root, "Racine.lnk", r"C:\Dossier qui n'existe pas\app.exe");
        make(&root.join("Programs").join("Essai"), "Essai.LNK", r"C:\Autre\x.exe");
        make(&deep, "Trop profond.lnk", r"C:\Perdu\y.exe");
        std::fs::write(root.join("pas un raccourci.txt"), "x").unwrap();
        std::fs::write(root.join("cassé.lnk"), "pas un raccourci").unwrap();

        let mut found = sandbox.host.shortcuts(root.to_str().unwrap()).unwrap();
        found.sort_by(|a, b| a.path.cmp(&b.path));
        let described: Vec<(String, Option<String>)> = found
            .into_iter()
            .map(|shortcut| {
                let name = shortcut.path.rsplit('\\').next().unwrap().to_owned();
                (name, shortcut.target)
            })
            .collect();
        assert_eq!(
            described,
            [
                ("Essai.LNK".to_owned(), Some(r"C:\Autre\x.exe".to_owned())),
                ("Racine.lnk".to_owned(), Some(r"C:\Dossier qui n'existe pas\app.exe".to_owned())),
                // Illisible : présent, sans cible.
                ("cassé.lnk".to_owned(), None),
            ],
            "trois niveaux au plus sous la racine ; la cible est donnée telle quelle"
        );
        assert!(sandbox.host.shortcuts(root.join("absent").to_str().unwrap()).is_err());
    }

    #[test]
    fn the_guard_and_the_sync_roots_answer_for_this_machine() {
        let sandbox = Sandbox::new();
        let host = &sandbox.host;
        let windows = host.system_dirs().unwrap().windows;
        assert!(!host.root_allowed(&windows), "le dossier de Windows n'est jamais une racine");
        assert!(!host.root_allowed(r"C:\"), "ni la racine d'un disque");
        assert!(!host.root_allowed(&format!(r"{windows}\Fonts\Essai")), "ni dedans");
        let own = host.own_dir().expect("le dossier de l'exécutable");
        assert!(!own.ends_with('\\'));
        assert!(host.in_sync_root("relatif"), "un chemin illisible est traité comme synchronisé");
        let _ = OsString::new();
    }
}
