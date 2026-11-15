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

use std::fs;
use std::io::{self, Write};
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
    /// Proposer le mode jeu au lancement d'un jeu.
    pub game_mode_on_launch: bool,
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
            game_mode_on_launch: true,
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
        read(document, "gameModeOnLaunch", &mut settings.game_mode_on_launch, &mut rejected);
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
        let roaming = environment.known_folder(KnownFolder::RoamingAppData)?;
        Some(Self::at(roaming.join(APP_DIR).join(FILE_NAME)))
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
        let text = match fs::read_to_string(&self.path) {
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

        let temporary = self.path.with_extension("json.tmp");
        let written = (|| {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(json.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &self.path)
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        written
    }
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
            game_mode_on_launch: false,
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
                "gameModeOnLaunch",
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
}
