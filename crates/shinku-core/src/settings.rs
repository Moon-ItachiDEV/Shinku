//! Les réglages de l'utilisateur, gardés dans `%APPDATA%\Shinku\settings.json`.
//!
//! Deux précautions guident ce module :
//!
//! - un fichier abîmé ne doit pas faire oublier les « dossiers jamais
//!   touchés ». La lecture est donc tolérante : chaque réglage est lu pour
//!   lui-même, et un réglage illisible reprend sa valeur par défaut sans
//!   emporter les autres ;
//! - l'écriture ne laisse jamais un fichier à moitié écrit : on écrit à côté,
//!   puis on remplace.
//!
//! Si le fichier entier est illisible, on le dit ([`LoadIssue`]) : c'est à
//! l'interface de prévenir que les réglages sont revenus à leurs valeurs par
//! défaut.
//!
//! Ce fichier est lu par deux programmes : la fenêtre, qui l'écrit, et l'agent
//! du mode jeu, qui le relit quand sa date change. N'importe quel programme du
//! compte peut aussi l'écrire : tout ce qui vient de lui est donc contrôlé.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::platform::{Environment, KnownFolder};

/// Version du format de `settings.json`.
pub const SETTINGS_VERSION: u32 = 1;

/// Dossier de Shinku dans `AppData\Roaming`.
const APP_DIR: &str = "Shinku";
/// Nom du fichier des réglages.
const FILE_NAME: &str = "settings.json";
/// Nom sous lequel un fichier illisible est mis de côté avant d'être remplacé.
const UNREADABLE_NAME: &str = "settings.illisible.json";

/// Seuils proposés, en Go : « Me prévenir à partir de » et l'alerte corbeille.
pub const THRESHOLDS_GB: [u32; 4] = [2, 5, 10, 20];
/// Limites proposées pour le cache de shaders, en Go.
pub const SHADER_CACHE_LIMITS_GB: [u32; 2] = [5, 10];
/// Délais proposés pour le vidage automatique de la corbeille, en jours.
pub const AUTO_EMPTY_DAYS: [u32; 3] = [30, 60, 90];
/// Nombre maximal de noms d'exécutables dans une liste du mode jeu.
pub const MAX_EXE_NAMES: usize = 64;
/// Longueur maximale d'un nom d'exécutable, en caractères.
pub const MAX_EXE_NAME_LEN: usize = 64;
/// Nombre maximal de chemins dans une liste du mode jeu.
pub const MAX_GAME_PATHS: usize = 256;
/// Longueur maximale d'un chemin d'une liste du mode jeu, en caractères.
pub const MAX_GAME_PATH_LEN: usize = 1024;
/// Taille maximale du fichier des réglages : au-delà, il est traité comme
/// illisible (n'importe quel programme du compte peut l'écrire).
pub const MAX_FILE_BYTES: u64 = 1 << 20;

/// Langue de l'interface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    /// Français.
    #[default]
    Fr,
    /// Anglais.
    En,
}

/// Mode d'affichage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Celui de Windows.
    #[default]
    System,
    /// Clair (« Brume »).
    Light,
    /// Sombre (« Crépuscule »).
    Dark,
}

/// Rythme de l'analyse automatique.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoScan {
    /// Jamais.
    Never,
    /// Chaque semaine.
    #[default]
    Weekly,
    /// Chaque mois.
    Monthly,
}

/// Les réglages de Shinku.
///
/// Les noms des champs sont ceux du fichier, en `camelCase`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Version du format.
    pub version: u32,
    /// Lancer Shinku avec Windows, en arrière-plan et sans fenêtre.
    pub launch_with_windows: bool,
    /// Langue de l'interface.
    pub language: Language,
    /// Mode d'affichage.
    pub theme: Theme,
    /// Rythme de l'analyse automatique.
    pub auto_scan: AutoScan,
    /// Prévenir quand un grand vide peut libérer au moins cet espace, en Go.
    pub notify_from_gb: u32,
    /// Mode jeu automatique : quand un jeu passe au premier plan, l'agent de
    /// Shinku applique les réglages ci-dessous, puis remet tout en place.
    pub game_mode_auto: bool,
    /// Pendant la partie, passer l'alimentation en « Performances élevées ».
    pub game_mode_power: bool,
    /// Pendant la partie, mettre des apps d'arrière-plan en veilleuse.
    pub game_mode_quiet_apps: bool,
    /// Apps cochées par la personne, en plus de celles qui le sont d'office
    /// (noms d'exécutables, en minuscules).
    pub game_mode_quiet_added: Vec<String>,
    /// Apps cochées d'office que la personne a décochées (noms d'exécutables,
    /// en minuscules).
    pub game_mode_quiet_removed: Vec<String>,
    /// Jeux ajoutés par la personne (chemins d'exécutables).
    pub game_mode_games: Vec<PathBuf>,
    /// « Ce n'est pas un jeu » : exécutables que Shinku ne traite jamais comme des jeux.
    pub game_mode_not_games: Vec<PathBuf>,
    /// Vider le cache de shaders au-delà de cette taille, en Go. `None` : jamais.
    pub shader_cache_limit_gb: Option<u32>,
    /// Alerter quand la corbeille est trop pleine.
    pub recycle_bin_alert: bool,
    /// Seuil de cette alerte, en Go.
    pub recycle_bin_threshold_gb: u32,
    /// Vider de la corbeille ce qui y est depuis ce nombre de jours. `None` : jamais.
    pub recycle_bin_auto_empty_days: Option<u32>,
    /// Toujours demander avant de supprimer.
    pub always_ask: bool,
    /// Passer par la corbeille, pour pouvoir récupérer un fichier.
    pub use_recycle_bin: bool,
    /// « Dossiers jamais touchés » : Shinku ne les analyse pas.
    pub excluded_folders: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            launch_with_windows: true,
            language: Language::Fr,
            theme: Theme::System,
            auto_scan: AutoScan::Weekly,
            notify_from_gb: 5,
            game_mode_auto: false,
            game_mode_power: true,
            game_mode_quiet_apps: true,
            game_mode_quiet_added: Vec::new(),
            game_mode_quiet_removed: Vec::new(),
            game_mode_games: Vec::new(),
            game_mode_not_games: Vec::new(),
            shader_cache_limit_gb: Some(5),
            recycle_bin_alert: true,
            recycle_bin_threshold_gb: 5,
            recycle_bin_auto_empty_days: None,
            always_ask: true,
            use_recycle_bin: true,
            excluded_folders: Vec::new(),
        }
    }
}

impl Settings {
    /// Ramène chaque réglage à une valeur que Shinku sait appliquer. Rend le
    /// nom des réglages corrigés.
    ///
    /// Les dossiers exclus sont gardés tels quels, même s'ils n'existent pas
    /// (un disque débranché reviendra) : seuls les doublons sont retirés.
    ///
    /// Les listes du mode jeu ne gardent que des noms d'exécutables valides
    /// (voir [`clean_exe_name`], et jamais un lanceur de jeux) et des chemins
    /// non vides, de longueur bornée, sans doublon, en nombre borné.
    pub fn sanitize(&mut self) -> Vec<&'static str> {
        let defaults = Self::default();
        let mut fixed = Vec::new();
        self.version = SETTINGS_VERSION;
        if !THRESHOLDS_GB.contains(&self.notify_from_gb) {
            self.notify_from_gb = defaults.notify_from_gb;
            fixed.push("notifyFromGb");
        }
        if !THRESHOLDS_GB.contains(&self.recycle_bin_threshold_gb) {
            self.recycle_bin_threshold_gb = defaults.recycle_bin_threshold_gb;
            fixed.push("recycleBinThresholdGb");
        }
        if self.shader_cache_limit_gb.is_some_and(|limit| !SHADER_CACHE_LIMITS_GB.contains(&limit))
        {
            // Dans le doute, on ne vide pas.
            self.shader_cache_limit_gb = None;
            fixed.push("shaderCacheLimitGb");
        }
        if self.recycle_bin_auto_empty_days.is_some_and(|days| !AUTO_EMPTY_DAYS.contains(&days)) {
            // Dans le doute, on ne vide pas.
            self.recycle_bin_auto_empty_days = None;
            fixed.push("recycleBinAutoEmptyDays");
        }
        let mut seen: Vec<PathBuf> = Vec::with_capacity(self.excluded_folders.len());
        for folder in self.excluded_folders.drain(..) {
            if !folder.as_os_str().is_empty() && !seen.contains(&folder) {
                seen.push(folder);
            }
        }
        self.excluded_folders = seen;
        for (list, name) in [
            (&mut self.game_mode_quiet_added, "gameModeQuietAdded"),
            (&mut self.game_mode_quiet_removed, "gameModeQuietRemoved"),
        ] {
            if clean_exe_names(list) {
                fixed.push(name);
            }
        }
        // Un lanceur de jeux n'est jamais mis en veilleuse : il en est le parent.
        let before = self.game_mode_quiet_added.len();
        self.game_mode_quiet_added.retain(|name| !crate::game::detect::is_launcher(name));
        if self.game_mode_quiet_added.len() != before && !fixed.contains(&"gameModeQuietAdded") {
            fixed.push("gameModeQuietAdded");
        }
        for (list, name) in [
            (&mut self.game_mode_games, "gameModeGames"),
            (&mut self.game_mode_not_games, "gameModeNotGames"),
        ] {
            if clean_game_paths(list) {
                fixed.push(name);
            }
        }
        fixed
    }

    /// Lit des réglages à partir d'un document JSON, réglage par réglage.
    /// Rend aussi le nom de ceux qui n'ont pas pu être lus ou appliqués.
    fn from_document(document: &Map<String, Value>) -> (Self, Vec<&'static str>) {
        let mut settings = Self::default();
        let mut rejected = Vec::new();
        read(document, "launchWithWindows", &mut settings.launch_with_windows, &mut rejected);
        read(document, "language", &mut settings.language, &mut rejected);
        read(document, "theme", &mut settings.theme, &mut rejected);
        read(document, "autoScan", &mut settings.auto_scan, &mut rejected);
        read(document, "notifyFromGb", &mut settings.notify_from_gb, &mut rejected);
        // L'ancien réglage `gameModeOnLaunch` n'existe plus : s'il traîne dans
        // un fichier, il est ignoré.
        read(document, "gameModeAuto", &mut settings.game_mode_auto, &mut rejected);
        read(document, "gameModePower", &mut settings.game_mode_power, &mut rejected);
        read(document, "gameModeQuietApps", &mut settings.game_mode_quiet_apps, &mut rejected);
        read(document, "shaderCacheLimitGb", &mut settings.shader_cache_limit_gb, &mut rejected);
        read(document, "recycleBinAlert", &mut settings.recycle_bin_alert, &mut rejected);
        read(
            document,
            "recycleBinThresholdGb",
            &mut settings.recycle_bin_threshold_gb,
            &mut rejected,
        );
        read(
            document,
            "recycleBinAutoEmptyDays",
            &mut settings.recycle_bin_auto_empty_days,
            &mut rejected,
        );
        read(document, "alwaysAsk", &mut settings.always_ask, &mut rejected);
        read(document, "useRecycleBin", &mut settings.use_recycle_bin, &mut rejected);
        // Les dossiers exclus se lisent un par un : une entrée abîmée ne doit
        // pas faire perdre les autres.
        match document.get("excludedFolders") {
            None => {}
            Some(Value::Array(entries)) => {
                for entry in entries {
                    match entry.as_str() {
                        Some(path) => settings.excluded_folders.push(PathBuf::from(path)),
                        None if !rejected.contains(&"excludedFolders") => {
                            rejected.push("excludedFolders");
                        }
                        None => {}
                    }
                }
            }
            Some(_) => rejected.push("excludedFolders"),
        }
        // Les listes du mode jeu, elles aussi, se lisent entrée par entrée.
        settings.game_mode_quiet_added =
            read_strings(document, "gameModeQuietAdded", &mut rejected);
        settings.game_mode_quiet_removed =
            read_strings(document, "gameModeQuietRemoved", &mut rejected);
        settings.game_mode_games = read_strings(document, "gameModeGames", &mut rejected)
            .into_iter()
            .map(PathBuf::from)
            .collect();
        settings.game_mode_not_games = read_strings(document, "gameModeNotGames", &mut rejected)
            .into_iter()
            .map(PathBuf::from)
            .collect();
        for name in settings.sanitize() {
            if !rejected.contains(&name) {
                rejected.push(name);
            }
        }
        (settings, rejected)
    }
}

/// Lit un réglage s'il est présent et valide ; sinon le laisse à sa valeur
/// par défaut et note son nom.
fn read<T: DeserializeOwned>(
    document: &Map<String, Value>,
    name: &'static str,
    slot: &mut T,
    rejected: &mut Vec<&'static str>,
) {
    if let Some(value) = document.get(name) {
        match T::deserialize(value) {
            Ok(read) => *slot = read,
            Err(_) => rejected.push(name),
        }
    }
}

/// Lit une liste de textes, entrée par entrée : une entrée qui n'est pas un
/// texte est écartée et fait noter le réglage, sans emporter les autres.
fn read_strings(
    document: &Map<String, Value>,
    name: &'static str,
    rejected: &mut Vec<&'static str>,
) -> Vec<String> {
    match document.get(name) {
        None => Vec::new(),
        Some(Value::Array(entries)) => {
            let mut strings = Vec::with_capacity(entries.len());
            for entry in entries {
                match entry.as_str() {
                    Some(text) => strings.push(text.to_owned()),
                    None if !rejected.contains(&name) => rejected.push(name),
                    None => {}
                }
            }
            strings
        }
        Some(_) => {
            rejected.push(name);
            Vec::new()
        }
    }
}

/// Un nom d'exécutable que Shinku sait comparer : en minuscules, terminé par
/// `.exe`, sans séparateur de dossier, ni `:`, ni caractère de contrôle, et
/// d'au plus [`MAX_EXE_NAME_LEN`] caractères. `None` sinon.
pub fn clean_exe_name(name: &str) -> Option<String> {
    let name = name.to_lowercase();
    let valid = name.ends_with(".exe")
        && name.chars().count() <= MAX_EXE_NAME_LEN
        && !name.chars().any(|c| c == '\\' || c == '/' || c == ':' || c.is_control());
    valid.then_some(name)
}

/// Nettoie une liste de noms d'exécutables : noms valides seulement, sans
/// doublon, [`MAX_EXE_NAMES`] au plus. Vrai si une entrée valide ou non a dû
/// être écartée pour une autre raison qu'un doublon.
fn clean_exe_names(list: &mut Vec<String>) -> bool {
    let mut kept: Vec<String> = Vec::with_capacity(list.len());
    let mut dropped = false;
    for name in list.drain(..) {
        match clean_exe_name(&name) {
            Some(clean) if kept.contains(&clean) => {}
            Some(clean) if kept.len() < MAX_EXE_NAMES => kept.push(clean),
            _ => dropped = true,
        }
    }
    *list = kept;
    dropped
}

/// Nettoie une liste de chemins : non vides, d'au plus [`MAX_GAME_PATH_LEN`]
/// caractères, sans doublon (la casse ne compte pas), [`MAX_GAME_PATHS`] au
/// plus. Vrai si une entrée a dû être écartée pour une autre raison qu'un
/// doublon.
fn clean_game_paths(list: &mut Vec<PathBuf>) -> bool {
    let mut kept: Vec<PathBuf> = Vec::with_capacity(list.len());
    let mut keys: Vec<String> = Vec::with_capacity(list.len());
    let mut dropped = false;
    for path in list.drain(..) {
        let text = path.to_string_lossy();
        // Mesuré avant de mettre en minuscules : un chemin énorme n'est pas copié.
        if text.chars().take(MAX_GAME_PATH_LEN + 1).count() > MAX_GAME_PATH_LEN {
            dropped = true;
            continue;
        }
        let key = crate::game::detect::key(&text);
        if key.is_empty() {
            dropped = true;
        } else if keys.contains(&key) {
        } else if kept.len() < MAX_GAME_PATHS {
            keys.push(key);
            kept.push(path);
        } else {
            dropped = true;
        }
    }
    *list = kept;
    dropped
}

/// Ce qui n'a pas pu être lu.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LoadIssue {
    /// Le fichier existe mais n'est pas lisible : tous les réglages sont
    /// revenus à leurs valeurs par défaut, y compris les dossiers exclus.
    Unreadable {
        /// Ce que le système ou l'analyseur JSON en dit.
        reason: String,
    },
    /// Certains réglages étaient invalides : eux seuls sont revenus à leurs
    /// valeurs par défaut.
    Partial {
        /// Noms des réglages concernés, tels qu'ils sont écrits dans le fichier.
        fields: Vec<&'static str>,
    },
}

/// Résultat d'une lecture des réglages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Loaded {
    /// Les réglages à appliquer.
    pub settings: Settings,
    /// Ce qui n'a pas pu être lu, s'il y a lieu.
    pub issue: Option<LoadIssue>,
}

/// Le fichier des réglages.
#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    /// Le fichier des réglages de cette machine : `%APPDATA%\Shinku\settings.json`.
    /// `None` si Windows ne donne pas le dossier `AppData\Roaming`.
    pub fn for_machine(environment: &dyn Environment) -> Option<Self> {
        Some(Self::at(app_dir(environment)?.join(FILE_NAME)))
    }

    /// Un fichier de réglages à cet emplacement.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Emplacement du fichier.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Lit les réglages. Un fichier absent donne les réglages par défaut.
    pub fn load(&self) -> Loaded {
        let text = match read_bounded(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Loaded { settings: Settings::default(), issue: None };
            }
            Err(error) => return unreadable(error.to_string()),
        };
        // Certains éditeurs ajoutent une marque d'ordre des octets en tête.
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(document)) => {
                let (settings, rejected) = Settings::from_document(&document);
                let issue =
                    (!rejected.is_empty()).then_some(LoadIssue::Partial { fields: rejected });
                Loaded { settings, issue }
            }
            Ok(_) => unreadable("le fichier ne contient pas un objet JSON".to_owned()),
            Err(error) => unreadable(error.to_string()),
        }
    }

    /// Enregistre les réglages, après les avoir ramenés à des valeurs sûres.
    ///
    /// Le fichier est écrit à côté puis mis en place d'un seul geste : une
    /// coupure de courant laisse l'ancien ou le nouveau, jamais un mélange.
    /// Un fichier existant mais illisible est gardé sous un autre nom.
    pub fn save(&self, settings: &Settings) -> io::Result<()> {
        let mut settings = settings.clone();
        settings.sanitize();
        let json = serde_json::to_string_pretty(&settings).map_err(io::Error::other)?;

        let dir = self.path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "le fichier des réglages n'a pas de dossier",
            )
        })?;
        fs::create_dir_all(dir)?;
        if matches!(self.load().issue, Some(LoadIssue::Unreadable { .. })) {
            // On ne détruit pas ce qu'on n'a pas su lire.
            let _ = fs::rename(&self.path, dir.join(UNREADABLE_NAME));
        }

        write_replacing(&self.path, json.as_bytes())
    }
}

/// Le dossier de Shinku dans `AppData\Roaming`, où il garde ses réglages et
/// son journal. `None` si Windows ne donne pas ce dossier.
pub(crate) fn app_dir(environment: &dyn Environment) -> Option<PathBuf> {
    Some(environment.known_folder(KnownFolder::RoamingAppData)?.join(APP_DIR))
}

/// Écrit un petit fichier de Shinku d'un seul geste : le contenu est écrit à
/// côté, puis mis en place. Une coupure de courant laisse l'ancien fichier ou
/// le nouveau, jamais un mélange.
pub(crate) fn write_replacing(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let temporary = path.with_file_name(name);
    let written = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(content)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Lit le fichier en entier, à condition qu'il tienne dans [`MAX_FILE_BYTES`].
fn read_bounded(path: &Path) -> io::Result<String> {
    let file = fs::File::open(path)?;
    let mut text = String::new();
    file.take(MAX_FILE_BYTES + 1).read_to_string(&mut text)?;
    if u64::try_from(text.len()).map_or(true, |len| len > MAX_FILE_BYTES) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "le fichier fait plus de 1 Mo"));
    }
    Ok(text)
}

fn unreadable(reason: String) -> Loaded {
    Loaded { settings: Settings::default(), issue: Some(LoadIssue::Unreadable { reason }) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::StaticEnvironment;

    fn store() -> (tempfile::TempDir, SettingsStore) {
        let temp = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(temp.path().join("Shinku").join("settings.json"));
        (temp, store)
    }

    #[test]
    fn defaults_follow_the_decisions_taken() {
        let settings = Settings::default();
        // « Lancer Shinku avec Windows » est actif par défaut : décision du 13 novembre 2026.
        assert!(settings.launch_with_windows);
        assert!(settings.always_ask);
        assert!(settings.use_recycle_bin);
        assert_eq!(settings.recycle_bin_auto_empty_days, None);
        assert_eq!(settings.recycle_bin_threshold_gb, 5);
        assert!(settings.excluded_folders.is_empty());
        // Le mode jeu automatique est éteint : rien ne change sans l'accord de la personne.
        assert!(!settings.game_mode_auto);
        assert!(settings.game_mode_power);
        assert!(settings.game_mode_quiet_apps);
        assert!(settings.game_mode_quiet_added.is_empty());
        assert!(settings.game_mode_quiet_removed.is_empty());
        assert!(settings.game_mode_games.is_empty());
        assert!(settings.game_mode_not_games.is_empty());
        let mut copy = settings.clone();
        assert!(copy.sanitize().is_empty(), "les valeurs par défaut sont valides");
        assert_eq!(copy, settings);
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let (_temp, store) = store();
        assert_eq!(store.load(), Loaded { settings: Settings::default(), issue: None });
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let (_temp, store) = store();
        let settings = Settings {
            launch_with_windows: false,
            language: Language::En,
            theme: Theme::Dark,
            auto_scan: AutoScan::Monthly,
            notify_from_gb: 20,
            game_mode_auto: true,
            game_mode_power: false,
            game_mode_quiet_apps: false,
            game_mode_quiet_added: vec!["chrome.exe".to_owned(), "qbittorrent.exe".to_owned()],
            game_mode_quiet_removed: vec!["dropbox.exe".to_owned()],
            game_mode_games: vec![PathBuf::from(r"C:\Program Files\Java\bin\javaw.exe")],
            game_mode_not_games: vec![PathBuf::from(r"D:\Games\Hades\Hades.exe")],
            shader_cache_limit_gb: None,
            recycle_bin_alert: false,
            recycle_bin_threshold_gb: 10,
            recycle_bin_auto_empty_days: Some(60),
            always_ask: false,
            use_recycle_bin: false,
            excluded_folders: vec![PathBuf::from("D:\\Sauvegardes"), PathBuf::from("C:\\Privé")],
            ..Settings::default()
        };
        store.save(&settings).unwrap();
        assert_eq!(store.load(), Loaded { settings, issue: None });
        // Rien ne traîne à côté du fichier.
        let names: Vec<_> = fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["settings.json"]);
    }

    #[test]
    fn the_file_uses_the_documented_names() {
        let (_temp, store) = store();
        store.save(&Settings::default()).unwrap();
        let text = fs::read_to_string(store.path()).unwrap();
        let document: Value = serde_json::from_str(&text).unwrap();
        let mut names: Vec<&str> =
            document.as_object().unwrap().keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "alwaysAsk",
                "autoScan",
                "excludedFolders",
                "gameModeAuto",
                "gameModeGames",
                "gameModeNotGames",
                "gameModePower",
                "gameModeQuietAdded",
                "gameModeQuietApps",
                "gameModeQuietRemoved",
                "language",
                "launchWithWindows",
                "notifyFromGb",
                "recycleBinAlert",
                "recycleBinAutoEmptyDays",
                "recycleBinThresholdGb",
                "shaderCacheLimitGb",
                "theme",
                "useRecycleBin",
                "version",
            ]
        );
        assert_eq!(document["theme"], "system");
        assert_eq!(document["autoScan"], "weekly");
        assert_eq!(document["language"], "fr");
        assert_eq!(document["recycleBinAutoEmptyDays"], Value::Null);
    }

    #[test]
    fn one_bad_setting_does_not_lose_the_others() {
        let (_temp, store) = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        // Un thème inconnu, un seuil hors liste, une entrée abîmée, un réglage
        // d'une version future : le reste doit être gardé.
        let text = r#"{
            "version": 7,
            "theme": "fluo",
            "notifyFromGb": 3,
            "useRecycleBin": false,
            "recycleBinAutoEmptyDays": 45,
            "excludedFolders": ["C:\\Privé", 12, "D:\\Photos", "C:\\Privé"],
            "reglageDuFutur": true
        }"#;
        fs::write(store.path(), text).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.settings.theme, Theme::System);
        assert_eq!(loaded.settings.notify_from_gb, 5);
        assert!(!loaded.settings.use_recycle_bin, "un réglage valide est gardé");
        assert_eq!(
            loaded.settings.recycle_bin_auto_empty_days, None,
            "dans le doute, ne pas vider"
        );
        assert_eq!(
            loaded.settings.excluded_folders,
            [PathBuf::from("C:\\Privé"), PathBuf::from("D:\\Photos")],
            "les dossiers lisibles restent exclus"
        );
        assert_eq!(loaded.settings.version, SETTINGS_VERSION);
        let Some(LoadIssue::Partial { fields }) = loaded.issue else {
            panic!("un problème partiel était attendu : {:?}", loaded.issue);
        };
        assert_eq!(fields, ["theme", "excludedFolders", "notifyFromGb", "recycleBinAutoEmptyDays"]);
    }

    #[test]
    fn an_unreadable_file_is_reported_and_kept_aside() {
        let (_temp, store) = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), "{ \"excludedFolders\": [\"C:\\\\Privé\"").unwrap();
        let loaded = store.load();
        assert_eq!(loaded.settings, Settings::default());
        assert!(matches!(loaded.issue, Some(LoadIssue::Unreadable { .. })), "{:?}", loaded.issue);

        // Enregistrer par-dessus ne détruit pas le fichier illisible.
        store.save(&Settings::default()).unwrap();
        assert_eq!(store.load().issue, None);
        let aside = store.path().with_file_name(UNREADABLE_NAME);
        assert!(fs::read_to_string(aside).unwrap().contains("Privé"));

        // Un fichier qui n'est pas un objet est tout aussi illisible.
        fs::write(store.path(), "[1, 2, 3]").unwrap();
        assert!(matches!(store.load().issue, Some(LoadIssue::Unreadable { .. })));
    }

    #[test]
    fn a_byte_order_mark_is_accepted() {
        let (_temp, store) = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), "\u{feff}{ \"theme\": \"light\" }").unwrap();
        let loaded = store.load();
        assert_eq!(loaded.settings.theme, Theme::Light);
        assert_eq!(loaded.issue, None);
    }

    #[test]
    fn saving_brings_values_back_to_safe_ones() {
        let (_temp, store) = store();
        let settings = Settings {
            recycle_bin_threshold_gb: 0,
            shader_cache_limit_gb: Some(1),
            excluded_folders: vec![PathBuf::new(), PathBuf::from("D:\\A"), PathBuf::from("D:\\A")],
            ..Settings::default()
        };
        store.save(&settings).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.issue, None);
        assert_eq!(loaded.settings.recycle_bin_threshold_gb, 5);
        assert_eq!(loaded.settings.shader_cache_limit_gb, None);
        assert_eq!(loaded.settings.excluded_folders, [PathBuf::from("D:\\A")]);
    }

    #[test]
    fn the_file_lives_in_roaming_app_data() {
        let roaming =
            Path::new(if cfg!(windows) { r"C:\Users\a\AppData\Roaming" } else { "/a/Roaming" });
        let environment =
            StaticEnvironment::new().with_folder(KnownFolder::RoamingAppData, roaming);
        let store = SettingsStore::for_machine(&environment).unwrap();
        assert_eq!(store.path(), roaming.join("Shinku").join("settings.json"));
        assert!(SettingsStore::for_machine(&StaticEnvironment::new()).is_none());
    }

    #[test]
    fn the_old_game_mode_setting_is_ignored() {
        let (_temp, store) = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), r#"{ "gameModeOnLaunch": true, "gameModeAuto": true }"#).unwrap();
        let loaded = store.load();
        assert!(loaded.settings.game_mode_auto);
        assert_eq!(loaded.issue, None, "un ancien réglage n'est pas une erreur");
        // Il ne revient pas à la prochaine écriture.
        store.save(&loaded.settings).unwrap();
        assert!(!fs::read_to_string(store.path()).unwrap().contains("gameModeOnLaunch"));
    }

    #[test]
    fn game_mode_lists_are_cleaned_entry_by_entry() {
        let (_temp, store) = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let long = format!("{}.exe", "x".repeat(70));
        let text = serde_json::json!({
            "gameModeQuietAdded": [
                "Chrome.EXE", "chrome.exe", "notepad", "a\\b.exe", "a/b.exe", "c:d.exe",
                "tab\t.exe", long, 12, "qBittorrent.exe"
            ],
            "gameModeQuietRemoved": "pas une liste",
            "gameModeGames": [r"C:\Jeux\A.exe", r"c:\jeux\a.EXE", "", 5, r"D:\B.exe"],
            "gameModeNotGames": [r"E:\X.exe"],
            "gameModeAuto": "oui",
        });
        fs::write(store.path(), text.to_string()).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.settings.game_mode_quiet_added, ["chrome.exe", "qbittorrent.exe"]);
        assert!(loaded.settings.game_mode_quiet_removed.is_empty());
        assert_eq!(
            loaded.settings.game_mode_games,
            [PathBuf::from(r"C:\Jeux\A.exe"), PathBuf::from(r"D:\B.exe")]
        );
        assert_eq!(loaded.settings.game_mode_not_games, [PathBuf::from(r"E:\X.exe")]);
        assert!(
            !loaded.settings.game_mode_auto,
            "un réglage illisible revient à sa valeur par défaut"
        );
        let Some(LoadIssue::Partial { fields }) = loaded.issue else {
            panic!("un problème partiel était attendu");
        };
        for field in ["gameModeAuto", "gameModeQuietAdded", "gameModeQuietRemoved", "gameModeGames"]
        {
            assert!(fields.contains(&field), "{field} dans {fields:?}");
        }
        // Ces réglages ne bloquent jamais l'analyse.
        assert!(fields.iter().all(|field| *field != EXCLUDED_NAME));
    }

    const EXCLUDED_NAME: &str = "excludedFolders";

    #[test]
    fn game_mode_lists_are_bounded() {
        let mut settings = Settings {
            game_mode_quiet_added: (0..100).map(|n| format!("app{n}.exe")).collect(),
            game_mode_games: (0..300).map(|n| PathBuf::from(format!(r"D:\g{n}.exe"))).collect(),
            ..Settings::default()
        };
        let fixed = settings.sanitize();
        assert_eq!(settings.game_mode_quiet_added.len(), MAX_EXE_NAMES);
        assert_eq!(settings.game_mode_games.len(), MAX_GAME_PATHS);
        assert_eq!(fixed, ["gameModeQuietAdded", "gameModeGames"]);
        assert_eq!(clean_exe_name("Ünïcode.EXE").as_deref(), Some("ünïcode.exe"));
        assert_eq!(clean_exe_name(&format!("{}.exe", "x".repeat(60))).map(|n| n.len()), Some(64));
        assert_eq!(clean_exe_name(&format!("{}.exe", "x".repeat(61))), None);
        assert_eq!(clean_exe_name(""), None);
        assert_eq!(clean_exe_name(".exe").as_deref(), Some(".exe"));
    }

    #[test]
    fn unreadable_game_mode_fields_are_reported_as_partial_and_not_as_excluded_folders() {
        // Ce test ne prouve que le rapport des champs ; que ces champs ne bloquent
        // pas l'analyse est prouvé par `only_the_excluded_folders_block_a_scan` (backend.rs).
        let (_temp, store) = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), r#"{ "gameModeGames": 3, "gameModeQuietAdded": [1] }"#).unwrap();
        let Some(LoadIssue::Partial { fields }) = store.load().issue else { panic!() };
        assert_eq!(fields, ["gameModeQuietAdded", "gameModeGames"]);
    }

    #[test]
    fn a_settings_file_over_one_mebibyte_is_unreadable() {
        let (_temp, store) = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), " ".repeat(usize::try_from(MAX_FILE_BYTES).unwrap() + 10)).unwrap();
        let loaded = store.load();
        assert!(matches!(loaded.issue, Some(LoadIssue::Unreadable { .. })));
        assert_eq!(loaded.settings, Settings::default());
    }

    #[test]
    fn a_hostile_game_path_is_dropped_and_a_long_one_is_kept_up_to_the_limit() {
        let mut settings = Settings {
            game_mode_games: vec![
                PathBuf::from("x".repeat(5_000_000)),
                PathBuf::from(format!(r"D:\{}", "y".repeat(MAX_GAME_PATH_LEN - 3))),
                PathBuf::from(format!(r"D:\{}", "z".repeat(MAX_GAME_PATH_LEN - 2))),
            ],
            ..Settings::default()
        };
        let fixed = settings.sanitize();
        assert_eq!(settings.game_mode_games.len(), 1, "seul le chemin de 1024 signes reste");
        assert_eq!(settings.game_mode_games[0].as_os_str().len(), MAX_GAME_PATH_LEN);
        assert!(fixed.contains(&"gameModeGames"));
    }

    #[test]
    fn game_launchers_are_removed_from_the_quiet_list() {
        let mut settings = Settings {
            game_mode_quiet_added: vec!["steam.exe".to_owned(), "itch.exe".to_owned()],
            ..Settings::default()
        };
        let fixed = settings.sanitize();
        assert_eq!(settings.game_mode_quiet_added, ["itch.exe"]);
        assert!(fixed.contains(&"gameModeQuietAdded"));
    }
}
