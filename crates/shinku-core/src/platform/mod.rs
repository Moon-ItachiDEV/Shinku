//! Ce que le moteur demande à la machine : dossiers connus, disques, corbeille.
//!
//! Tout passe par le trait [`Environment`]. Le moteur ne lit jamais une
//! variable d'environnement ni une API de Windows directement : les tests lui
//! donnent ainsi un PC fabriqué de toutes pièces ([`StaticEnvironment`]), et
//! la vraie implémentation reste petite et isolée.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::model::Timestamp;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

/// Les dossiers que Windows connaît par leur rôle plutôt que par leur chemin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KnownFolder {
    /// Le profil de l'utilisateur (`C:\Users\Léa`).
    Profile,
    /// Bureau.
    Desktop,
    /// Documents.
    Documents,
    /// Téléchargements.
    Downloads,
    /// Musique.
    Music,
    /// Images.
    Pictures,
    /// Vidéos.
    Videos,
    /// `AppData\Local`.
    LocalAppData,
    /// `AppData\LocalLow`.
    LocalAppDataLow,
    /// `AppData\Roaming`.
    RoamingAppData,
    /// Le dossier temporaire de l'utilisateur (`%TEMP%`).
    Temp,
    /// `C:\Windows`.
    Windows,
    /// `C:\Program Files`.
    ProgramFiles,
    /// `C:\Program Files (x86)`.
    ProgramFilesX86,
    /// `C:\ProgramData`.
    ProgramData,
}

impl KnownFolder {
    /// Tous les dossiers connus.
    pub const ALL: [Self; 15] = [
        Self::Profile,
        Self::Desktop,
        Self::Documents,
        Self::Downloads,
        Self::Music,
        Self::Pictures,
        Self::Videos,
        Self::LocalAppData,
        Self::LocalAppDataLow,
        Self::RoamingAppData,
        Self::Temp,
        Self::Windows,
        Self::ProgramFiles,
        Self::ProgramFilesX86,
        Self::ProgramData,
    ];

    /// Les bibliothèques de l'utilisateur : là où vivent ses fichiers personnels.
    pub const LIBRARIES: [Self; 6] = [
        Self::Desktop,
        Self::Documents,
        Self::Downloads,
        Self::Music,
        Self::Pictures,
        Self::Videos,
    ];
}

/// Un disque (volume) vu par Windows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    /// Racine du volume, sous forme interne (`\\?\C:\`).
    #[serde(serialize_with = "serialize_display_path")]
    pub root: PathBuf,
    /// Nom donné au disque, s'il en a un.
    pub label: String,
    /// Système de fichiers (`NTFS`, `ReFS`, `exFAT`…).
    pub file_system: String,
    /// Capacité, en octets.
    pub total: u64,
    /// Espace libre pour l'utilisateur, en octets.
    pub free: u64,
    /// Disque interne (ni amovible, ni réseau, ni optique).
    pub fixed: bool,
    /// Vrai pour un disque dur à plateaux, faux pour un SSD, inconnu sinon.
    /// Sur un disque dur, lire en parallèle ralentit tout.
    pub seek_penalty: Option<bool>,
    /// Le disque où Windows est installé.
    pub system: bool,
}

impl Volume {
    /// Nombre de lectures en parallèle adapté à ce disque.
    pub fn read_threads(&self, available: usize) -> usize {
        let limit = match self.seek_penalty {
            Some(true) => 1,
            Some(false) => 8,
            None => 4,
        };
        available.clamp(1, limit)
    }
}

/// Côté interface, un chemin voyage sous sa forme habituelle (`C:\…`).
pub(crate) fn serialize_display_path<S: serde::Serializer>(
    path: &Path,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&crate::fs::to_display_path(path).to_string_lossy())
}

/// État de la corbeille, tous disques confondus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RecycleBin {
    /// Place occupée, en octets.
    pub bytes: u64,
    /// Nombre d'éléments.
    pub items: u64,
}

/// La machine, vue par le moteur.
///
/// Tous les chemins rendus sont absolus et sous forme interne
/// (voir [`crate::fs::to_internal_path`]).
pub trait Environment: Send + Sync + fmt::Debug {
    /// L'instant présent.
    fn now(&self) -> Timestamp;

    /// Chemin d'un dossier connu, s'il est défini sur cette machine.
    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf>;

    /// Les disques visibles.
    fn volumes(&self) -> Vec<Volume>;

    /// Racines des dossiers synchronisés avec un service en ligne (OneDrive…).
    /// Supprimer un fichier à l'intérieur le supprime aussi dans le cloud.
    fn sync_roots(&self) -> Vec<PathBuf>;

    /// État de la corbeille, si on peut le connaître.
    fn recycle_bin(&self) -> Option<RecycleBin>;

    /// Nombre de fils d'exécution utilisables.
    fn parallelism(&self) -> usize {
        std::thread::available_parallelism().map_or(1, usize::from)
    }
}

/// La vraie machine.
pub fn system() -> impl Environment {
    imp::SystemEnvironment::new()
}

/// Vrai si ce processus a reçu les droits d'un administrateur par une
/// élévation : « Exécuter en tant qu'administrateur », ou un lancement depuis
/// un programme lui-même élevé. Toujours faux hors de Windows.
///
/// La fenêtre de Shinku et son agent s'en servent pour refuser de démarrer :
/// ils affichent une page web et travaillent sur le profil de l'utilisateur,
/// deux choses qui n'ont rien à faire avec des droits d'administrateur. Élevé,
/// le processus peut même tourner sous un autre compte, donc regarder un autre
/// profil que celui de la personne assise devant l'écran.
///
/// Faux, en revanche, sur un PC où tout tourne déjà avec tous les droits
/// (contrôle de compte désactivé, session du compte Administrateur intégré) :
/// il n'y existe aucune autre façon de lancer Shinku.
pub fn runs_elevated() -> bool {
    elevated_launch(imp::own_rights(), imp::desktop_rights)
}

/// Les droits d'un processus, tels que Windows les décrit dans son jeton.
// Hors de Windows, un processus est toujours ordinaire : seuls les tests
// fabriquent les autres cas.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rights {
    /// Les droits ordinaires d'un programme lancé normalement.
    Standard,
    /// Tous les droits, obtenus par une élévation : ce jeton est la moitié
    /// « administrateur » d'une paire, l'autre moitié sert au quotidien.
    Elevated,
    /// Tous les droits, sans paire : le jeton ne dit pas, à lui seul, si le
    /// reste de la session tourne avec les mêmes droits.
    Unrestricted,
}

/// Ce qu'on sait du bureau de la session (l'Explorateur de fichiers).
// Hors de Windows, il est toujours absent.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Desktop {
    /// Il tourne avec ces droits.
    Running(Rights),
    /// Il existe, mais Windows refuse qu'on le regarde : il appartient à un
    /// autre compte.
    Unreachable,
    /// Il n'y a pas de bureau (session sans interface, bureau remplacé).
    Absent,
}

/// La décision de [`runs_elevated`], séparée des appels à Windows pour être
/// testée partout.
fn elevated_launch(own: Rights, desktop: impl FnOnce() -> Desktop) -> bool {
    match own {
        Rights::Standard => false,
        Rights::Elevated => true,
        // Tous les droits sans paire de jetons : c'est normal si toute la
        // session est dans ce cas, et c'est une élévation si le bureau, lui,
        // tourne avec des droits ordinaires ou sous un autre compte (un
        // compte standard a saisi le mot de passe d'un administrateur).
        Rights::Unrestricted => match desktop() {
            Desktop::Running(Rights::Standard) | Desktop::Unreachable => true,
            Desktop::Running(Rights::Elevated | Rights::Unrestricted) | Desktop::Absent => false,
        },
    }
}

/// Passe le fil d'exécution courant en priorité basse (processeur et disque)
/// tant que la valeur rendue vit. Sans effet hors de Windows.
pub fn background_priority() -> BackgroundPriority {
    BackgroundPriority { active: imp::set_background_priority(true) }
}

/// Garde de la priorité basse : la priorité normale revient quand elle disparaît.
#[derive(Debug)]
pub struct BackgroundPriority {
    active: bool,
}

impl Drop for BackgroundPriority {
    fn drop(&mut self) {
        if self.active {
            imp::set_background_priority(false);
        }
    }
}

/// Une machine décrite à la main : pour les tests, et pour essayer le moteur
/// sur un dossier quelconque.
#[derive(Clone, Debug, Default)]
pub struct StaticEnvironment {
    now: Option<Timestamp>,
    folders: Vec<(KnownFolder, PathBuf)>,
    volumes: Vec<Volume>,
    sync_roots: Vec<PathBuf>,
    recycle_bin: Option<RecycleBin>,
    parallelism: Option<usize>,
}

impl StaticEnvironment {
    /// Une machine vide : aucun dossier connu, aucun disque.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fige l'instant présent.
    pub fn with_now(mut self, now: Timestamp) -> Self {
        self.now = Some(now);
        self
    }

    /// Déclare un dossier connu. `path` doit être sous forme interne.
    pub fn with_folder(mut self, folder: KnownFolder, path: impl Into<PathBuf>) -> Self {
        self.folders.retain(|(known, _)| *known != folder);
        self.folders.push((folder, path.into()));
        self
    }

    /// Déclare un disque.
    pub fn with_volume(mut self, volume: Volume) -> Self {
        self.volumes.push(volume);
        self
    }

    /// Déclare un dossier synchronisé.
    pub fn with_sync_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.sync_roots.push(path.into());
        self
    }

    /// Fixe l'état de la corbeille.
    pub fn with_recycle_bin(mut self, recycle_bin: RecycleBin) -> Self {
        self.recycle_bin = Some(recycle_bin);
        self
    }

    /// Fixe le nombre de fils d'exécution.
    pub fn with_parallelism(mut self, threads: usize) -> Self {
        self.parallelism = Some(threads.max(1));
        self
    }
}

impl Environment for StaticEnvironment {
    fn now(&self) -> Timestamp {
        self.now.unwrap_or_else(Timestamp::now)
    }

    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        self.folders.iter().find(|(known, _)| *known == folder).map(|(_, path)| path.clone())
    }

    fn volumes(&self) -> Vec<Volume> {
        self.volumes.clone()
    }

    fn sync_roots(&self) -> Vec<PathBuf> {
        self.sync_roots.clone()
    }

    fn recycle_bin(&self) -> Option<RecycleBin> {
        self.recycle_bin
    }

    fn parallelism(&self) -> usize {
        self.parallelism
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(seek_penalty: Option<bool>) -> Volume {
        Volume {
            root: PathBuf::from("/"),
            label: String::new(),
            file_system: "NTFS".to_owned(),
            total: 0,
            free: 0,
            fixed: true,
            seek_penalty,
            system: true,
        }
    }

    #[test]
    fn a_hard_disk_is_read_by_one_thread() {
        assert_eq!(volume(Some(true)).read_threads(16), 1);
        assert_eq!(volume(Some(false)).read_threads(16), 8);
        assert_eq!(volume(Some(false)).read_threads(4), 4);
        assert_eq!(volume(None).read_threads(16), 4);
        assert_eq!(volume(None).read_threads(0), 1);
    }

    #[test]
    fn only_a_real_elevation_counts_as_an_elevated_launch() {
        let never = || -> Desktop { panic!("le bureau n'a pas à être consulté") };
        assert!(!elevated_launch(Rights::Standard, never));
        assert!(elevated_launch(Rights::Elevated, never));

        // Tous les droits sans paire de jetons : tout dépend du bureau.
        let with = |desktop| elevated_launch(Rights::Unrestricted, || desktop);
        assert!(with(Desktop::Running(Rights::Standard)), "bureau ordinaire : élévation");
        assert!(with(Desktop::Unreachable), "bureau d'un autre compte : élévation");
        assert!(!with(Desktop::Running(Rights::Unrestricted)), "tout le PC a tous les droits");
        assert!(!with(Desktop::Running(Rights::Elevated)), "bureau lui-même élevé");
        assert!(!with(Desktop::Absent), "pas de bureau : rien à comparer");
    }

    #[test]
    fn a_static_environment_answers_what_it_was_given() {
        let environment = StaticEnvironment::new()
            .with_now(Timestamp::from_unix(1_700_000_000, 0))
            .with_folder(KnownFolder::Documents, "/home/léa/Documents")
            .with_folder(KnownFolder::Documents, "/data/Documents")
            .with_recycle_bin(RecycleBin { bytes: 5, items: 1 })
            .with_parallelism(0);
        assert_eq!(environment.now(), Timestamp::from_unix(1_700_000_000, 0));
        assert_eq!(
            environment.known_folder(KnownFolder::Documents),
            Some(PathBuf::from("/data/Documents"))
        );
        assert_eq!(environment.known_folder(KnownFolder::Pictures), None);
        assert_eq!(environment.recycle_bin(), Some(RecycleBin { bytes: 5, items: 1 }));
        assert_eq!(environment.parallelism(), 1);
    }

    #[test]
    fn the_real_machine_answers_without_failing() {
        let machine = system();
        // Rien de précis à vérifier ici : la machine de test est quelconque.
        // On s'assure seulement que chaque question reçoit une réponse cohérente.
        for folder in KnownFolder::ALL {
            if let Some(path) = machine.known_folder(folder) {
                assert!(path.is_absolute(), "{folder:?} : {path:?}");
                assert_eq!(crate::fs::to_internal_path(&path).ok().as_ref(), Some(&path));
            }
        }
        for volume in machine.volumes() {
            assert!(volume.root.is_absolute());
            assert!(volume.free <= volume.total);
        }
        for root in machine.sync_roots() {
            assert!(root.is_absolute());
        }
        let _ = machine.recycle_bin();
        assert!(machine.parallelism() >= 1);
        let _priority = background_priority();
        // Les tests peuvent tourner avec ou sans droits d'administrateur.
        let _ = runs_elevated();
    }
}
