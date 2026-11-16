//! Ce que le moteur demande à la machine : dossiers connus, disques,
//! corbeille, programmes en cours.
//!
//! Tout passe par le trait [`Environment`]. Le moteur ne lit jamais une
//! variable d'environnement ni une API de Windows directement : les tests lui
//! donnent ainsi un PC fabriqué de toutes pièces ([`StaticEnvironment`]), et
//! la vraie implémentation reste petite et isolée.
//!
//! Deux demandes modifient la machine : vider la corbeille et y envoyer un
//! fichier ou un dossier entier. Le grand vide fait les deux ; le module
//! « Désinstallation » n'envoie à la corbeille que des restes d'apps.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

use crate::apps::host::AppsHost;
use crate::game::host::GameHost;
use crate::model::Timestamp;
use crate::startup::host::StartupHost;

pub mod agent;
mod bin_rule;

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

/// Pourquoi un fichier ou un dossier n'est pas parti à la corbeille. Dans tous
/// les cas, il est resté à sa place, **en entier** : un dossier ne part jamais à
/// moitié.
#[derive(Debug)]
pub enum RecycleError {
    /// La corbeille ne peut pas le recevoir, **d'après ses réglages lus avant
    /// l'envoi** (la garde : désactivée, stratégie `NoRecycleFiles`, trop petite
    /// pour lui avec une marge de 10 %, ou sans capacité connue et plus grand que
    /// 5 % du volume), ou, à défaut, d'après ce que le shell a dit en route.
    /// Windows l'aurait supprimé définitivement : Shinku a refusé, et l'élément
    /// est resté en entier. La garde ne voit que des réglages : si Windows
    /// supprime quand même pour de bon, on ne le sait qu'après ([`Sent::Destroyed`]).
    Refused,
    /// Cette machine n'a pas de corbeille que Shinku sache utiliser.
    Unavailable,
    /// Un programme utilise le fichier.
    Locked,
    /// Windows refuse l'accès au fichier.
    AccessDenied,
    /// La corbeille a échoué pour une autre raison.
    Failed(String),
}

impl fmt::Display for RecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused => f.write_str("la corbeille ne peut pas recevoir ce fichier"),
            Self::Unavailable => f.write_str("la corbeille n'est pas disponible"),
            Self::Locked => f.write_str("un programme utilise ce fichier"),
            Self::AccessDenied => f.write_str("accès refusé"),
            Self::Failed(reason) => write!(f, "la corbeille a échoué : {reason}"),
        }
    }
}

impl std::error::Error for RecycleError {}

/// Où est parti un élément que Windows a accepté de supprimer. Le code de
/// réussite du shell ne le dit pas (il est le même dans les deux cas) : c'est
/// l'élément de corbeille rendu par Windows, puis le disque, qui tranchent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sent {
    /// Dans la corbeille : l'élément y est, restaurable.
    InBin,
    /// Windows l'a supprimé pour de bon : il n'est pas dans la corbeille (le
    /// shell n'a rendu aucun élément de corbeille). C'est un **constat après
    /// coup**, pas une barrière : la barrière est la garde, avant l'envoi.
    Destroyed,
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

    /// Noms des programmes en cours d'exécution (`chrome.exe`…), tous
    /// comptes confondus. `None` si la machine ne sait pas le dire : le
    /// moteur considère alors que n'importe quelle app peut être ouverte.
    fn running_processes(&self) -> Option<Vec<String>> {
        None
    }

    /// Vide la corbeille des disques que [`Environment::recycle_bin`] mesure.
    /// Sans retour possible.
    fn empty_recycle_bin(&self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "pas de corbeille sur cette machine"))
    }

    /// Envoie un fichier, ou un dossier **entier**, à la corbeille, d'où
    /// l'utilisateur peut le restaurer. `path` est sous forme interne.
    ///
    /// Un dossier part en **un seul élément**, avec tout ce qu'il contient : si
    /// une partie ne peut pas partir (un fichier tenu ouvert, une corbeille
    /// trop petite), l'envoi échoue et le dossier reste en entier, sans que
    /// rien n'en soit parti. Un lien (jonction, lien symbolique) n'est jamais
    /// envoyé, ni un dossier dont le chemin en traverse un.
    ///
    /// Ce qui est garanti : la garde de la corbeille (réglages lus avant
    /// l'envoi, voir [`RecycleError::Refused`]) refuse ce que la corbeille ne
    /// recevrait pas, et l'élément reste alors à sa place. **Ce qui ne l'est
    /// pas** : si Windows supprime quand même pour de bon (réglage que la garde
    /// ne voit pas), `Ok(())` est rendu ici comme pour un envoi réussi, car ce
    /// trait ne distingue pas [`Sent::Destroyed`] de [`Sent::InBin`]. Limite
    /// connue, à traiter dans le chantier du nettoyage ; l'appelant qui doit le
    /// savoir (le module « Désinstallation ») passe par l'hôte, qui le dit. Les
    /// essais de la vraie corbeille (`SHINKU_ESSAI_CORBEILLE`, machine jetable)
    /// prouvent la garde, et ce que Windows fait seul sans elle.
    fn recycle(&self, path: &Path) -> Result<(), RecycleError> {
        let _ = path;
        Err(RecycleError::Unavailable)
    }
}

/// La vraie machine.
pub fn system() -> impl Environment {
    imp::SystemEnvironment::new()
}

/// La machine du mode jeu : la vraie sous Windows, une machine qui ne voit
/// rien ailleurs.
pub fn game_host() -> Arc<dyn GameHost> {
    imp::game_host()
}

/// La machine du module « Démarrage » : la vraie sous Windows, une machine qui
/// ne voit rien ailleurs.
pub fn startup_host() -> Arc<dyn StartupHost> {
    imp::startup_host()
}

/// La machine du module « Désinstallation » : la vraie sous Windows, une machine
/// qui ne voit rien ailleurs.
pub fn apps_host() -> Arc<dyn AppsHost> {
    imp::apps_host()
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
///
/// Sa corbeille est un simple dossier ([`StaticEnvironment::with_recycle_folder`]) :
/// y « envoyer » un fichier l'y déplace, la « vider » supprime ce qu'elle a
/// elle-même reçu. Les copies d'une même machine partagent cette corbeille.
#[derive(Clone, Debug, Default)]
pub struct StaticEnvironment {
    now: Option<Timestamp>,
    folders: Vec<(KnownFolder, PathBuf)>,
    volumes: Vec<Volume>,
    sync_roots: Vec<PathBuf>,
    recycle_bin: Option<RecycleBin>,
    parallelism: Option<usize>,
    processes: Vec<String>,
    processes_unknown: bool,
    recycle_folder: Option<PathBuf>,
    recycle_limit: Option<u64>,
    bin: Arc<Mutex<StaticBin>>,
}

/// Ce qui est arrivé à la corbeille d'une machine décrite à la main.
#[derive(Debug, Default)]
struct StaticBin {
    /// La corbeille a été vidée : son contenu de départ n'existe plus.
    emptied: bool,
    /// Fichiers reçus depuis, et leur taille.
    held: Vec<(PathBuf, u64)>,
    /// Numérote les fichiers reçus : deux fichiers de même nom ne se gênent pas.
    received: u64,
}

/// La taille des fichiers d'un dossier, sans suivre aucun lien.
fn folder_bytes(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => folder_bytes(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |metadata| metadata.len()),
            _ => 0,
        })
        .sum()
}

/// Supprime pour de bon ce que la corbeille fabriquée a reçu : un fichier, ou
/// un dossier entier.
fn remove_held(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path),
        _ => std::fs::remove_file(path),
    }
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

    /// Déclare un programme en cours d'exécution (`chrome.exe`).
    pub fn with_process(mut self, name: impl Into<String>) -> Self {
        self.processes.push(name.into());
        self
    }

    /// La machine ne sait pas dire quels programmes tournent.
    pub fn without_process_list(mut self) -> Self {
        self.processes_unknown = true;
        self
    }

    /// Donne à la machine une corbeille : un dossier existant, sur le même
    /// disque que les fichiers qu'on y enverra.
    pub fn with_recycle_folder(mut self, folder: impl Into<PathBuf>) -> Self {
        self.recycle_folder = Some(folder.into());
        self
    }

    /// La corbeille refuse les fichiers plus gros que `bytes`, comme celle de
    /// Windows refuse un fichier plus gros qu'elle.
    pub fn with_recycle_limit(mut self, bytes: u64) -> Self {
        self.recycle_limit = Some(bytes);
        self
    }

    fn bin(&self) -> std::sync::MutexGuard<'_, StaticBin> {
        self.bin.lock().unwrap_or_else(PoisonError::into_inner)
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
        if self.recycle_bin.is_none() && self.recycle_folder.is_none() {
            return None;
        }
        let bin = self.bin();
        let mut total = match self.recycle_bin {
            Some(initial) if !bin.emptied => initial,
            _ => RecycleBin::default(),
        };
        for (_, bytes) in &bin.held {
            total.bytes += bytes;
            total.items += 1;
        }
        Some(total)
    }

    fn parallelism(&self) -> usize {
        self.parallelism
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from))
    }

    fn running_processes(&self) -> Option<Vec<String>> {
        (!self.processes_unknown).then(|| self.processes.clone())
    }

    fn empty_recycle_bin(&self) -> io::Result<()> {
        if self.recycle_bin.is_none() && self.recycle_folder.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "cette machine n'a pas de corbeille",
            ));
        }
        let mut bin = self.bin();
        bin.emptied = true;
        // Seulement ce que cette corbeille a reçu : rien d'autre ne part.
        let mut failure = None;
        bin.held.retain(|(path, _)| match remove_held(path) {
            Ok(()) => false,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => {
                failure = Some(error);
                true
            }
        });
        failure.map_or(Ok(()), Err)
    }

    fn recycle(&self, path: &Path) -> Result<(), RecycleError> {
        let Some(folder) = &self.recycle_folder else { return Err(RecycleError::Unavailable) };
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|error| RecycleError::Failed(error.to_string()))?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(RecycleError::Failed("ni un fichier ni un dossier".to_owned()));
        }
        // Un dossier pèse la somme de ses fichiers ; un lien n'est pas suivi.
        let bytes = if metadata.is_dir() { folder_bytes(path) } else { metadata.len() };
        if self.recycle_limit.is_some_and(|limit| bytes > limit) {
            return Err(RecycleError::Refused);
        }
        let mut bin = self.bin();
        bin.received += 1;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let kept = folder.join(format!("{:06}-{name}", bin.received));
        std::fs::rename(path, &kept).map_err(|error| RecycleError::Failed(error.to_string()))?;
        bin.held.push((kept, bytes));
        Ok(())
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
        assert_eq!(environment.running_processes(), Some(Vec::new()));
        let busy = environment.clone().with_process("chrome.exe");
        assert_eq!(busy.running_processes(), Some(vec!["chrome.exe".to_owned()]));
        assert_eq!(busy.without_process_list().running_processes(), None);
    }

    #[test]
    fn a_machine_without_a_recycle_bin_refuses_politely() {
        let environment = StaticEnvironment::new();
        assert_eq!(environment.recycle_bin(), None);
        assert!(environment.empty_recycle_bin().is_err());
        assert!(matches!(
            environment.recycle(Path::new("/nulle/part")),
            Err(RecycleError::Unavailable)
        ));
    }

    #[test]
    fn the_hand_made_recycle_bin_receives_and_empties() {
        let workspace = tempfile::tempdir().unwrap();
        let bin = workspace.path().join("corbeille");
        std::fs::create_dir(&bin).unwrap();
        let photo = workspace.path().join("photo.jpg");
        let film = workspace.path().join("film.mkv");
        let stranger = bin.join("pas-à-moi.txt");
        std::fs::write(&photo, [1u8; 300]).unwrap();
        std::fs::write(&film, [2u8; 5000]).unwrap();
        std::fs::write(&stranger, b"x").unwrap();

        let environment = StaticEnvironment::new()
            .with_recycle_bin(RecycleBin { bytes: 1000, items: 2 })
            .with_recycle_folder(&bin)
            .with_recycle_limit(4096);
        // Une copie de la machine voit la même corbeille.
        let twin = environment.clone();

        environment.recycle(&photo).unwrap();
        assert!(!photo.exists(), "le fichier a quitté sa place");
        assert_eq!(twin.recycle_bin(), Some(RecycleBin { bytes: 1300, items: 3 }));

        // Trop gros pour la corbeille : refusé, et laissé à sa place.
        assert!(matches!(environment.recycle(&film), Err(RecycleError::Refused)));
        assert!(film.exists());
        // Un dossier s'y envoie en un seul élément, avec tout ce qu'il contient.
        let folder = workspace.path().join("dossier");
        std::fs::create_dir_all(folder.join("a").join("b")).unwrap();
        std::fs::write(folder.join("a").join("b").join("c.txt"), [4u8; 700]).unwrap();
        std::fs::write(folder.join("d.txt"), [5u8; 100]).unwrap();
        environment.recycle(&folder).unwrap();
        assert!(!folder.exists(), "le dossier entier a quitté sa place");
        assert_eq!(twin.recycle_bin(), Some(RecycleBin { bytes: 2100, items: 4 }));
        // Trop gros pour la corbeille : il reste en entier.
        let big = workspace.path().join("gros");
        std::fs::create_dir(&big).unwrap();
        std::fs::write(big.join("x.bin"), [1u8; 3000]).unwrap();
        std::fs::write(big.join("y.bin"), [1u8; 3000]).unwrap();
        assert!(matches!(environment.recycle(&big), Err(RecycleError::Refused)));
        assert!(big.join("x.bin").exists() && big.join("y.bin").exists());

        twin.empty_recycle_bin().unwrap();
        assert_eq!(environment.recycle_bin(), Some(RecycleBin::default()));
        assert!(stranger.exists(), "vider ne supprime que ce que la corbeille a reçu");
        assert_eq!(std::fs::read_dir(&bin).unwrap().count(), 1);
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
