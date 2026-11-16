//! La machine, vue par le module « Désinstallation ».
//!
//! Comme [`crate::startup::host::StartupHost`] pour le Démarrage, le trait
//! [`AppsHost`] est tout ce que le module sait de Windows : les inscriptions
//! des apps, les variables, les fichiers, le lancement d'un désinstalleur, la
//! corbeille. La vraie implémentation est dans `platform/windows/apps.rs`. Les
//! tests utilisent [`crate::apps::fake::FakeAppsHost`], une machine fabriquée de
//! toutes pièces.
//!
//! **Le module n'écrit jamais au registre** : aucune méthode ne le permet. Ses
//! deux seuls gestes sur la machine sont le lancement d'un désinstalleur (après
//! un clic et une confirmation) et l'envoi à la corbeille de restes que la
//! personne a cochés.

use std::fmt;
use std::io;
use std::time::Duration;

use crate::apps::{Place, Source};
use crate::platform::RecycleError;
pub use crate::startup::host::Existence;

/// Un nombre que le registre peut écrire en DWORD ou en texte (`SystemComponent`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawNumber {
    /// Une valeur `REG_DWORD`.
    Dword(u32),
    /// Une valeur texte.
    Text(String),
}

impl RawNumber {
    /// Vrai pour le DWORD 1 ou le texte `1`.
    pub fn is_one(&self) -> bool {
        match self {
            Self::Dword(number) => *number == 1,
            Self::Text(text) => text.trim() == "1",
        }
    }
}

/// Une inscription telle que le registre la donne, avant tout filtre. Toutes
/// les valeurs sont facultatives. Un texte qui n'était pas de l'UTF-16 valide,
/// ou de plus de 32 Kio, est absent : jamais une conversion approximative.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawRegistration {
    /// Le nom de la sous-clé.
    pub key: String,
    /// `DisplayName`.
    pub display_name: Option<String>,
    /// `DisplayVersion`.
    pub display_version: Option<String>,
    /// `Publisher`.
    pub publisher: Option<String>,
    /// `InstallLocation`, tel qu'écrit (`%VAR%` non développées).
    pub install_location: Option<String>,
    /// `InstallDate` (texte `AAAAMMJJ`).
    pub install_date: Option<String>,
    /// `EstimatedSize`, en Kio.
    pub estimated_size: Option<u32>,
    /// `UninstallString`, telle qu'écrite.
    pub uninstall_string: Option<String>,
    /// `DisplayIcon`, tel qu'écrit.
    pub display_icon: Option<String>,
    /// `SystemComponent`.
    pub system_component: Option<RawNumber>,
    /// `WindowsInstaller` (DWORD).
    pub windows_installer: Option<u32>,
    /// `NoRemove` (DWORD).
    pub no_remove: Option<u32>,
    /// `ParentKeyName`.
    pub parent_key_name: Option<String>,
    /// `ReleaseType`.
    pub release_type: Option<String>,
}

impl RawRegistration {
    /// Une inscription qui ne porte que le nom de sa sous-clé.
    pub fn new(key: &str) -> Self {
        Self { key: key.to_owned(), ..Self::default() }
    }
}

/// Ce qu'une place contient, avec ce que l'hôte n'a pas pu lire.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawSource {
    /// Les inscriptions lues.
    pub entries: Vec<RawRegistration>,
    /// Faux si l'hôte a dû laisser quelque chose de côté : une borne atteinte
    /// (trop de sous-clés, un nom ou une valeur démesurés), une sous-clé
    /// illisible. La liste est alors **incomplète**, et la page le dit.
    pub complete: bool,
}

impl RawSource {
    /// Une place lue en entier.
    pub fn complete(entries: Vec<RawRegistration>) -> Self {
        Self { entries, complete: true }
    }
}

/// Le dossier système (`System32`) et le dossier de Windows, sans barre finale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemDirs {
    /// `C:\Windows\System32`.
    pub system: String,
    /// `C:\Windows`.
    pub windows: String,
}

/// Un dossier du compte où chercher des restes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccountFolder {
    /// `%APPDATA%`.
    Roaming,
    /// `%LOCALAPPDATA%`.
    Local,
    /// `AppData\LocalLow`.
    LocalLow,
    /// Le menu Démarrer du compte (`…\Microsoft\Windows\Start Menu`).
    StartMenu,
    /// Le Bureau du compte.
    Desktop,
}

/// Un élément du contenu direct d'un dossier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Child {
    /// Son nom.
    pub name: String,
    /// Vrai pour un vrai dossier.
    pub is_dir: bool,
    /// Vrai pour un lien (jonction, lien symbolique) : jamais suivi.
    pub is_link: bool,
}

/// Un raccourci `.lnk`, avec sa cible telle qu'il l'écrit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    /// Le chemin du fichier `.lnk`.
    pub path: String,
    /// Sa cible brute (`SLGP_RAWPATH`), jamais résolue ni cherchée.
    pub target: Option<String>,
}

/// Ce qu'un dossier pèse, compté sans jamais entrer dans un lien.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measure {
    /// Taille cumulée des fichiers, en octets.
    pub bytes: u64,
    /// Nombre de fichiers.
    pub files: u64,
    /// Un lien a été croisé (il n'est pas suivi, et n'est pas compté).
    pub has_link: bool,
}

/// Une garde sur un processus lancé : de quoi savoir s'il est sorti.
pub trait Watch: Send + Sync + fmt::Debug {
    /// Vrai si le processus est sorti.
    fn ended(&self) -> bool;
}

/// Ce qui s'est passé au lancement d'un désinstalleur.
#[derive(Debug)]
pub enum Launch {
    /// Lancé. La garde existe si on l'a demandée et que Windows l'a donnée.
    Started(Option<Box<dyn Watch>>),
    /// La personne a dit non à la fenêtre de Windows.
    Cancelled,
    /// Fichier ou chemin introuvable.
    Missing,
    /// Autre échec, avec le code de Windows.
    Failed(u32),
}

/// La machine, vue par le module « Désinstallation ». Aucune méthode n'écrit
/// au registre.
pub trait AppsHost: Send + Sync + fmt::Debug {
    /// Les inscriptions d'une place, telles quelles (aucun filtre). Une place
    /// dont la clé n'existe pas est vide.
    fn registrations(&self, place: Place) -> io::Result<RawSource>;

    /// Une inscription, relue à l'instant. `Ok(None)` : absente **avec
    /// certitude** (« introuvable » et rien d'autre).
    fn registration(&self, source: Source, key: &str) -> io::Result<Option<RawRegistration>>;

    /// Développe `%VAR%`. Strict : une variable invalide donne un texte vide.
    fn expand(&self, text: &str) -> String;

    /// Vrai si ce chemin absolu est sur un lecteur **fixe** de cette machine.
    fn is_local_fixed(&self, path: &str) -> bool;

    /// Fichier, dossier, absent avec certitude, ou impossible à dire. À
    /// n'appeler que pour un chemin d'un lecteur fixe.
    fn existence(&self, path: &str) -> Existence;

    /// Le dossier système et le dossier de Windows.
    fn system_dirs(&self) -> Option<SystemDirs>;

    /// Lance un programme par le shell, depuis le dossier système. `elevated` :
    /// avec la demande de droits de Windows. `watch` : garde de quoi savoir si
    /// le processus lancé est sorti. Ne rend la main qu'après la réponse à la
    /// fenêtre de Windows, qui peut rester ouverte des minutes.
    fn launch(&self, program: &str, parameters: &str, elevated: bool, watch: bool) -> Launch;

    /// Un dossier du compte, `None` s'il est inconnu.
    fn account_folder(&self, folder: AccountFolder) -> Option<String>;

    /// Vrai si chaque dossier du chemin, depuis la racine, est un vrai dossier
    /// (aucun lien).
    fn plain_chain(&self, path: &str) -> bool;

    /// Les noms du contenu direct d'un dossier. Borné à 4096 éléments : au-delà,
    /// une erreur (on ne dit pas « stable » d'un dossier qu'on ne voit pas en
    /// entier).
    fn children(&self, path: &str) -> io::Result<Vec<Child>>;

    /// Les raccourcis `.lnk` sous un dossier (profondeur 3 au plus, 2000 au
    /// plus), avec leur cible brute.
    fn shortcuts(&self, root: &str) -> io::Result<Vec<Shortcut>>;

    /// Taille et nombre de fichiers d'un dossier, ou taille d'un fichier,
    /// sans jamais entrer dans un lien. `None` au-delà des bornes (200 000
    /// éléments, 5 s) ou si une partie est illisible.
    fn measure(&self, path: &str) -> Option<Measure>;

    /// Met un fichier ou un dossier **entier** à la corbeille. Jamais de
    /// suppression définitive.
    fn recycle(&self, path: &str) -> Result<(), RecycleError>;

    /// Horloge monotone (temps depuis le démarrage), pour tous les délais.
    fn uptime(&self) -> Duration;

    /// Le dossier de `shinku.exe`, sans barre finale.
    fn own_dir(&self) -> Option<String>;

    /// Vrai si le garde accepte ce chemin comme racine d'une règle
    /// ([`crate::guard::Guard::check_rule_root`]) : ni racine d'un disque, ni
    /// Windows, Program Files, ProgramData, profil, bibliothèques ou `AppData`
    /// eux-mêmes, ni dedans, ni un dossier qui en contient un.
    fn root_allowed(&self, path: &str) -> bool;

    /// Remet un chemin en forme **longue et complète** : noms courts (`~1`)
    /// développés, `.` et `..` résolus, préfixe `\\?\` retiré. `None` si la
    /// machine n'y arrive pas (le chemin n'existe pas, ou ne se lit pas). Sert
    /// pour ce que **les autres apps** écrivent dans leur inscription : un chemin
    /// qu'on ne sait pas remettre en forme ne protège rien, donc on le dit.
    fn long_path(&self, path: &str) -> Option<String>;

    /// Vrai si ce chemin est un dossier synchronisé avec un service en ligne
    /// (OneDrive…), ou dedans : la corbeille n'y a pas le même sens.
    fn in_sync_root(&self, path: &str) -> bool;
}

/// Une machine qui ne voit rien : aucune app. C'est celle des plateformes sans
/// Windows. Tout lancement y est refusé.
#[derive(Clone, Copy, Debug, Default)]
pub struct InertAppsHost;

/// Le code Windows « non pris en charge » (`ERROR_NOT_SUPPORTED`).
const NOT_SUPPORTED: u32 = 50;

impl AppsHost for InertAppsHost {
    fn registrations(&self, _place: Place) -> io::Result<RawSource> {
        Ok(RawSource::complete(Vec::new()))
    }

    fn registration(&self, _source: Source, _key: &str) -> io::Result<Option<RawRegistration>> {
        Ok(None)
    }

    fn expand(&self, text: &str) -> String {
        text.to_owned()
    }

    fn is_local_fixed(&self, _path: &str) -> bool {
        false
    }

    fn existence(&self, _path: &str) -> Existence {
        Existence::Unsure
    }

    fn system_dirs(&self) -> Option<SystemDirs> {
        None
    }

    fn launch(&self, _program: &str, _parameters: &str, _elevated: bool, _watch: bool) -> Launch {
        Launch::Failed(NOT_SUPPORTED)
    }

    fn account_folder(&self, _folder: AccountFolder) -> Option<String> {
        None
    }

    fn plain_chain(&self, _path: &str) -> bool {
        false
    }

    fn children(&self, _path: &str) -> io::Result<Vec<Child>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "pas de Windows ici"))
    }

    fn shortcuts(&self, _root: &str) -> io::Result<Vec<Shortcut>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "pas de Windows ici"))
    }

    fn measure(&self, _path: &str) -> Option<Measure> {
        None
    }

    fn recycle(&self, _path: &str) -> Result<(), RecycleError> {
        Err(RecycleError::Unavailable)
    }

    fn uptime(&self) -> Duration {
        // L'horloge monotone de ce processus : seuls les écarts comptent.
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed()
    }

    fn own_dir(&self) -> Option<String> {
        None
    }

    fn root_allowed(&self, _path: &str) -> bool {
        false
    }

    fn in_sync_root(&self, _path: &str) -> bool {
        true
    }

    fn long_path(&self, _path: &str) -> Option<String> {
        None
    }
}
