//! Le module « Confidentialité », sous Windows : [`PrivacyHost`] sur de vrais
//! appels documentés de l'API Windows.
//!
//! Ce que ce module **modifie** : les *valeurs* de trois clés de
//! `HKEY_CURRENT_USER` nommées par une énumération compilée
//! ([`TypedKey`]), la liste des fichiers récents (`SHAddToRecentDocs`), le
//! presse-papiers. Il ne supprime **jamais** une clé, n'ouvre jamais `HKLM`
//! en écriture, et ne reçoit jamais un chemin de registre : le chemin d'une clé
//! est lu ici, dans [`TypedKey::path`], jamais passé par le trait.
//!
//! Les clés sont ouvertes avec `REG_OPTION_OPEN_LINK` et les seuls droits de
//! lire et d'écrire des valeurs ; une clé qui porte `SymbolicLinkValue` de type
//! `REG_LINK` est refusée. Dans les essais, un préfixe place les clés sous une
//! racine jetable de `HKCU` : jamais les vraies.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use windows::ApplicationModel::DataTransfer::Clipboard;
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND,
    ERROR_SUCCESS, HWND,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, CountClipboardFormats, EmptyClipboard, OpenClipboard,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_LINK, REG_OPTION_OPEN_LINK,
    REG_VALUE_TYPE, RegDeleteValueW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW,
    RegQueryValueExW,
};
use windows::Win32::UI::Shell::{SHARD_PIDL, SHAddToRecentDocs};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::SystemEnvironment;
use super::startup::{ComScope, Key, os_error};
use crate::fs::{DirReader, EntryKind, PathState, ReadDirError, inspect_dir_chain};
use crate::platform::{Environment, KnownFolder};
use crate::privacy::host::{
    ClipboardClear, CurrentClear, HistoryClear, PrivacyHost, RegistryError, RegistryValues,
};
use crate::privacy::rules::{MAX_VALUE_NAME, MAX_VALUES, TypedKey, ValueName};

/// Options d'ouverture de `CreateFileW` : ne pas suivre un point d'analyse final.
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
/// Partage complet : on ne gêne personne.
const SHARE_ALL: u32 = 0x7;
/// Nombre d'essais d'ouverture du presse-papiers, et pause entre deux.
const CLIPBOARD_TRIES: u32 = 10;
const CLIPBOARD_PAUSE: Duration = Duration::from_millis(50);
/// Longueur maximale d'un nom de valeur de registre, en unités.
const MAX_REGISTRY_NAME: usize = 16_383;

/// La machine du module « Confidentialité ».
#[derive(Debug)]
pub(in crate::platform) struct SystemPrivacyHost {
    /// Devant le chemin de chaque clé : vide en production, une racine jetable
    /// de `HKCU` (terminée par `\`) dans les essais.
    registry_prefix: String,
    /// Le dossier « Récents », si on ne le demande pas à Windows (essais).
    recent_dir: Option<PathBuf>,
}

impl SystemPrivacyHost {
    pub(in crate::platform) fn new() -> Self {
        Self { registry_prefix: String::new(), recent_dir: None }
    }

    fn key_path(&self, key: TypedKey) -> String {
        format!("{}{}", self.registry_prefix, key.path())
    }

    fn recent_folder(&self) -> Option<PathBuf> {
        if let Some(dir) = &self.recent_dir {
            return Some(dir.clone());
        }
        let mut path = SystemEnvironment::new().known_folder(KnownFolder::RoamingAppData)?;
        for part in ["Microsoft", "Windows", "Recent"] {
            path.push(part);
        }
        Some(path)
    }

    /// Ouvre la clé sans suivre de lien, avec les seuls droits de lire et d'écrire des
    /// valeurs. `Ok(None)` : elle n'existe pas. Un lien de registre est refusé.
    fn open(&self, key: TypedKey) -> Result<Option<Key>, RegistryError> {
        let mut handle = HKEY::default();
        // SAFETY: le chemin est une chaîne terminée par zéro, valide pendant l'appel ;
        // `handle` est une variable locale où Windows écrit le handle.
        let result = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(self.key_path(key)),
                Some(REG_OPTION_OPEN_LINK.0),
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                &raw mut handle,
            )
        };
        if result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND {
            return Ok(None);
        }
        if result != ERROR_SUCCESS {
            return Err(RegistryError::Io(os_error(result)));
        }
        let key = Key(handle);
        let mut kind = REG_VALUE_TYPE(0);
        // SAFETY: la clé est ouverte pour `key` ; le nom est une chaîne terminée
        // par zéro ; `kind` est une variable locale, aucune donnée n'est demandée.
        let link = unsafe {
            RegQueryValueExW(
                key.0,
                &HSTRING::from("SymbolicLinkValue"),
                None,
                Some(&raw mut kind),
                None,
                None,
            )
        };
        // Une clé lien porte `SymbolicLinkValue` de type `REG_LINK` : on n'y touche pas.
        // (`ERROR_MORE_DATA` : la valeur existe, sa donnée n'a pas été demandée.)
        if (link == ERROR_SUCCESS || link == ERROR_MORE_DATA) && kind == REG_LINK {
            return Err(RegistryError::Link);
        }
        Ok(Some(key))
    }
}

impl PrivacyHost for SystemPrivacyHost {
    fn local_app_data(&self) -> Option<PathBuf> {
        SystemEnvironment::new().known_folder(KnownFolder::LocalAppData)
    }

    fn firefox_present(&self) -> bool {
        let Some(mut path) = SystemEnvironment::new().known_folder(KnownFolder::RoamingAppData)
        else {
            return false;
        };
        for part in ["Mozilla", "Firefox", "Profiles"] {
            path.push(part);
        }
        matches!(inspect_dir_chain(&path), Ok(PathState::RealDir))
    }

    fn running_processes(&self) -> Option<Vec<String>> {
        SystemEnvironment::new().running_processes()
    }

    fn lockfile_held(&self, user_data: &Path) -> bool {
        // On essaie d'ouvrir `lockfile` en lecture **et en écriture**, sans le
        // créer ni rien y écrire : le navigateur le tient sans partage, ou avec un
        // partage en lecture seule, et les deux refusent cette ouverture.
        // [à vérifier sur un vrai Chrome : le mode de partage exact de `lockfile`.]
        let result = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(SHARE_ALL)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(user_data.join("lockfile"));
        match result {
            Ok(_) => false,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            // Sharing violation, accès refusé : tenu (le sens prudent).
            Err(_) => true,
        }
    }

    fn recent_count(&self) -> io::Result<u32> {
        let Some(folder) = self.recent_folder() else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "pas de dossier Récents"));
        };
        match inspect_dir_chain(&folder)? {
            PathState::RealDir => {}
            PathState::Missing => return Ok(0),
            PathState::NotPlain => {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "lien sur le chemin"));
            }
        }
        // Les noms sont lus par la liste du dossier (inévitable), jamais gardés ni rendus.
        let mut count = 0u32;
        let listed = DirReader::new().read(&folder, None, &mut |name, meta| {
            if meta.kind == EntryKind::File
                && name.to_str().is_some_and(|n| n.to_ascii_lowercase().ends_with(".lnk"))
            {
                count = count.saturating_add(1);
            }
        });
        match listed {
            Ok(()) => Ok(count),
            Err(ReadDirError::NotFound) => Ok(0),
            Err(error) => Err(io::Error::other(error.to_string())),
        }
    }

    fn clear_recent(&self) -> io::Result<()> {
        let _com = ComScope::enter();
        // SAFETY: `SHARD_PIDL` avec un pointeur nul : Windows efface tout l'usage
        // récent ; aucun pointeur n'est lu. L'appel ne rend rien : la relecture
        // du nombre de raccourcis dit ce qui s'est passé.
        unsafe { SHAddToRecentDocs(SHARD_PIDL.0 as u32, None) };
        Ok(())
    }

    fn registry_values(&self, key: TypedKey) -> Result<Option<RegistryValues>, RegistryError> {
        let Some(open) = self.open(key)? else { return Ok(None) };
        let (mut values, mut longest) = (0u32, 0u32);
        // SAFETY: la clé est ouverte pour `open` ; les deux pointeurs sont des
        // variables locales où Windows écrit un nombre.
        let result = unsafe {
            RegQueryInfoKeyW(
                open.0,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(&raw mut values),
                Some(&raw mut longest),
                None,
                None,
                None,
            )
        };
        if result != ERROR_SUCCESS {
            return Err(RegistryError::Io(os_error(result)));
        }
        let mut buffer = vec![0u16; (longest as usize + 1).min(MAX_REGISTRY_NAME + 1)];
        let mut names = Vec::new();
        let mut complete = values as usize <= MAX_VALUES;
        for index in 0..values.min(MAX_VALUES as u32) {
            let mut len = buffer.len() as u32;
            // SAFETY: la clé est ouverte ; `buffer` est local et `len` annonce sa
            // taille en caractères ; ni type ni données ne sont demandés.
            let result = unsafe {
                RegEnumValueW(
                    open.0,
                    index,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &raw mut len,
                    None,
                    None,
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
                return Err(RegistryError::Io(os_error(result)));
            }
            let units = &buffer[..(len as usize).min(buffer.len())];
            if units.len() > MAX_VALUE_NAME {
                // Trop long pour être supprimé : laissé, et dit.
                complete = false;
                continue;
            }
            names.push(ValueName(units.to_vec()));
        }
        Ok(Some(RegistryValues { names, complete }))
    }

    fn registry_delete(&self, key: TypedKey, name: &ValueName) -> Result<(), RegistryError> {
        let Some(open) = self.open(key)? else { return Ok(()) };
        let mut wide = name.0.clone();
        wide.push(0);
        // SAFETY: la clé est ouverte pour `open` ; `wide` est terminée par zéro et
        // vit pendant l'appel. Seule une *valeur* est supprimée, jamais une clé.
        let result = unsafe { RegDeleteValueW(open.0, PCWSTR(wide.as_ptr())) };
        if result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(RegistryError::Io(os_error(result)))
        }
    }

    fn clipboard_filled(&self) -> io::Result<bool> {
        // SAFETY: appel sans pointeur ni état à ouvrir.
        Ok(unsafe { CountClipboardFormats() } > 0)
    }

    fn clipboard_history_enabled(&self) -> io::Result<bool> {
        let _com = ComScope::enter();
        Clipboard::IsHistoryEnabled().map_err(|error| io::Error::other(error.to_string()))
    }

    fn clear_clipboard(&self, owner: isize) -> ClipboardClear {
        let _com = ComScope::enter();
        // L'historique (Windows+V) : `ClearHistory` rend un booléen. Faux alors que
        // l'historique est actif : échec ; historique désactivé : cas normal.
        // [à vérifier sur le vrai Windows : si l'appel exige la fenêtre au premier
        // plan, et ce qu'il rend historique désactivé.]
        let enabled = Clipboard::IsHistoryEnabled().unwrap_or(true);
        let history = match Clipboard::ClearHistory() {
            Ok(true) => HistoryClear::Cleared,
            Ok(false) | Err(_) if !enabled => HistoryClear::Disabled,
            Ok(false) | Err(_) => HistoryClear::Failed,
        };
        let window = (owner != 0).then_some(HWND(owner as *mut c_void));
        let mut current = CurrentClear::Busy;
        for attempt in 0..CLIPBOARD_TRIES {
            // SAFETY: `window` est nul ou la fenêtre de Shinku, que le guichet a reçue.
            if unsafe { OpenClipboard(window) }.is_ok() {
                // SAFETY: le presse-papiers est ouvert par ce fil, juste au-dessus.
                let emptied = unsafe { EmptyClipboard() };
                // SAFETY: appariée à l'ouverture réussie, sur le même fil.
                let _ = unsafe { CloseClipboard() };
                current =
                    if emptied.is_ok() { CurrentClear::Emptied } else { CurrentClear::Failed };
                break;
            }
            if attempt + 1 < CLIPBOARD_TRIES {
                thread::sleep(CLIPBOARD_PAUSE);
            }
        }
        ClipboardClear { history, current }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::{
        KEY_CREATE_LINK, REG_DWORD, REG_OPTION_CREATE_LINK, REG_OPTION_NON_VOLATILE, REG_SZ,
        RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
    };

    use crate::privacy::erase::{KeyOutcome, erase_key, erase_typed};
    use crate::privacy::view::TraceOutcome;

    /// Une racine d'essai sous `HKCU`, supprimée à la fin : les essais n'écrivent
    /// jamais sous les vraies clés de l'Explorateur.
    struct Sandbox {
        root: String,
        host: SystemPrivacyHost,
    }

    impl Sandbox {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |time| time.as_nanos());
            let root =
                format!(r"Software\Shinku-Essai-Confidentialite-{}-{unique}", std::process::id());
            let host = SystemPrivacyHost { registry_prefix: format!(r"{root}\"), recent_dir: None };
            Self { root, host }
        }

        fn create(
            &self,
            key: TypedKey,
            options: windows::Win32::System::Registry::REG_OPEN_CREATE_OPTIONS,
            access: windows::Win32::System::Registry::REG_SAM_FLAGS,
        ) -> Option<Key> {
            let mut handle = HKEY::default();
            // SAFETY: chaîne terminée par zéro ; `handle` reçoit le handle.
            let result = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(self.host.key_path(key)),
                    None,
                    PCWSTR::null(),
                    options,
                    access,
                    None,
                    &raw mut handle,
                    None,
                )
            };
            (result == ERROR_SUCCESS).then_some(Key(handle))
        }

        fn put(&self, key: TypedKey, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) {
            let handle = self
                .create(key, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE | KEY_QUERY_VALUE)
                .expect("clé d'essai");
            // SAFETY: clé ouverte ; `data` est un tampon local.
            let result =
                unsafe { RegSetValueExW(handle.0, &HSTRING::from(name), None, kind, Some(data)) };
            assert_eq!(result, ERROR_SUCCESS);
        }

        fn text(&self, key: TypedKey, name: &str) {
            let data: Vec<u8> = "x\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
            self.put(key, name, REG_SZ, &data);
        }

        fn names(&self, key: TypedKey) -> Option<Vec<String>> {
            self.host.registry_values(key).unwrap().map(|values| {
                let mut names: Vec<String> =
                    values.names.iter().map(|n| String::from_utf16_lossy(&n.0)).collect();
                names.sort();
                names
            })
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
    fn the_values_of_a_key_are_deleted_by_name_and_the_key_stays() {
        let sandbox = Sandbox::new();
        sandbox.text(TypedKey::RunMru, "a");
        sandbox.text(TypedKey::RunMru, "b");
        sandbox.text(TypedKey::RunMru, "MRUList");
        sandbox.put(TypedKey::RunMru, "nombre", REG_DWORD, &7u32.to_le_bytes());
        sandbox.text(TypedKey::TypedPaths, "url1");
        // Une clé voisine, hors de la liste : jamais touchée.
        sandbox.text(TypedKey::WordWheelQuery, "0");
        assert_eq!(sandbox.names(TypedKey::RunMru).unwrap(), ["MRUList", "a", "b", "nombre"]);
        assert_eq!(erase_key(&sandbox.host, TypedKey::RunMru), KeyOutcome::Erased);
        // La clé reste, vide.
        assert_eq!(sandbox.names(TypedKey::RunMru), Some(Vec::new()));
        assert_eq!(sandbox.names(TypedKey::TypedPaths).unwrap(), ["url1"]);
        assert_eq!(erase_key(&sandbox.host, TypedKey::RunMru), KeyOutcome::Nothing);
        assert_eq!(erase_typed(&sandbox.host), TraceOutcome::Erased);
        assert_eq!(sandbox.names(TypedKey::TypedPaths), Some(Vec::new()));
        assert_eq!(sandbox.names(TypedKey::WordWheelQuery), Some(Vec::new()));
    }

    #[test]
    fn an_absent_key_has_nothing_to_erase_and_is_not_created() {
        let sandbox = Sandbox::new();
        assert_eq!(sandbox.names(TypedKey::RunMru), None);
        assert_eq!(erase_typed(&sandbox.host), TraceOutcome::Nothing);
        assert_eq!(sandbox.names(TypedKey::RunMru), None);
    }

    #[test]
    fn a_registry_link_is_refused_and_its_target_untouched() {
        let sandbox = Sandbox::new();
        // La cible : une clé d'essai sous la même racine, pleine.
        sandbox.text(TypedKey::TypedPaths, "cible");
        let target =
            format!(r"\Registry\User\.Default\{}", sandbox.host.key_path(TypedKey::TypedPaths));
        let Some(link) = sandbox.create(
            TypedKey::RunMru,
            REG_OPTION_CREATE_LINK,
            KEY_CREATE_LINK | KEY_SET_VALUE | KEY_QUERY_VALUE,
        ) else {
            eprintln!(
                "[à vérifier] ce Windows (ou Wine) ne sait pas créer un lien de registre : essai passé"
            );
            return;
        };
        let data: Vec<u8> = target.encode_utf16().flat_map(u16::to_le_bytes).collect();
        // SAFETY: clé de lien ouverte ; `data` est un tampon local.
        let set = unsafe {
            RegSetValueExW(link.0, &HSTRING::from("SymbolicLinkValue"), None, REG_LINK, Some(&data))
        };
        if set != ERROR_SUCCESS {
            eprintln!("[à vérifier] valeur de lien refusée : essai passé");
            return;
        }
        match sandbox.host.registry_values(TypedKey::RunMru) {
            Err(RegistryError::Link) => {}
            other => panic!("un lien devait être refusé : {other:?}"),
        }
        assert!(matches!(
            sandbox.host.registry_delete(TypedKey::RunMru, &ValueName::new("cible")),
            Err(RegistryError::Link)
        ));
        assert_eq!(erase_key(&sandbox.host, TypedKey::RunMru), KeyOutcome::Failed);
        assert_eq!(sandbox.names(TypedKey::TypedPaths).unwrap(), ["cible"]);
    }

    #[test]
    fn recent_counts_only_shortcuts_placed_directly_in_the_folder() {
        let temp = tempfile::tempdir().unwrap();
        let root =
            crate::fs::to_internal_path(&std::fs::canonicalize(temp.path()).unwrap()).unwrap();
        for name in ["a.lnk", "B.LNK", "c.txt"] {
            std::fs::write(root.join(name), "x").unwrap();
        }
        std::fs::create_dir(root.join("sous.lnk")).unwrap();
        std::fs::write(root.join("sous.lnk").join("d.lnk"), "x").unwrap();
        let host =
            SystemPrivacyHost { registry_prefix: String::new(), recent_dir: Some(root.clone()) };
        assert_eq!(host.recent_count().unwrap(), 2);
        let missing = SystemPrivacyHost {
            registry_prefix: String::new(),
            recent_dir: Some(root.join("absent")),
        };
        assert_eq!(missing.recent_count().unwrap(), 0);
    }

    #[test]
    fn a_lockfile_held_by_someone_is_seen_and_an_absent_one_is_not() {
        let temp = tempfile::tempdir().unwrap();
        let host = SystemPrivacyHost::new();
        assert!(!host.lockfile_held(temp.path()), "pas de fichier : pas tenu");
        let held = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(0)
            .open(temp.path().join("lockfile"))
            .unwrap();
        assert!(host.lockfile_held(temp.path()));
        drop(held);
        assert!(!host.lockfile_held(temp.path()), "lâché : plus tenu");
        // Shinku n'a pas créé de fichier.
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn the_real_machine_answers_its_simple_questions() {
        let host = SystemPrivacyHost::new();
        assert!(host.running_processes().is_some());
        assert!(host.clipboard_filled().is_ok());
    }
}
