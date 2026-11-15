//! Le guichet : tout ce que la fenêtre de Shinku peut demander au cœur.
//!
//! La fenêtre affiche une page web. On ne lui confie donc que des demandes
//! précises et bornées : lancer une analyse, lire une page de résultats,
//! changer un réglage, faire le vide. Aucune demande ne prend un chemin pour
//! le lire, l'écrire ou le supprimer. Le seul chemin accepté désigne un
//! dossier à ne jamais toucher : il ne peut que réduire ce que Shinku regarde.
//!
//! Le grand vide lui-même se demande avec trois cases cochées et la date de
//! l'analyse affichée : ce qui part est décidé par le cœur, à partir du
//! rapport qu'il a gardé et du disque tel qu'il est.
//!
//! Ce module ne dépend pas de Tauri : il se teste comme le reste du cœur, et
//! les commandes de l'application ne font que l'appeler.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Serialize;

use crate::clean::journal::{JournalStore, Run};
use crate::clean::{CleanPreview, CleanRefusal, CleanRequest};
use crate::fs::{fold_path, to_display_path, to_internal_path};
use crate::hash::{DigestCache, MemoryCache};
use crate::model::Trigger;
use crate::platform::{self, Environment, RecycleBin, Volume};
use crate::scan::ScanOptions;
use crate::scan::report::{DuplicateGroup, DuplicatesState, JunkReport, KeepChoice, LargeFile};
use crate::session::{Session, SessionError, SessionView};
use crate::settings::{LoadIssue, Loaded, Settings, SettingsStore, Theme};

/// Nombre maximal d'éléments rendus par page, quelle que soit la demande.
pub const PAGE_LIMIT: usize = 200;

/// Nom du réglage des « dossiers jamais touchés » dans `settings.json`.
const EXCLUDED_FIELD: &str = "excludedFolders";

/// Temps laissé à un grand vide pour finir son fichier ou son élément en
/// cours et noter son bilan quand Shinku se ferme. Un élément jugé en bloc
/// est borné en taille pour tenir dans ce délai.
const SHUTDOWN_PATIENCE: Duration = Duration::from_secs(20);

/// Ce que la fenêtre affiche dès son ouverture.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// Version de Shinku.
    pub version: &'static str,
    /// Les disques, pour la jauge du menu.
    pub volumes: Vec<Volume>,
    /// La corbeille, si elle a pu être mesurée.
    pub recycle_bin: Option<RecycleBin>,
    /// Les réglages en vigueur.
    pub settings: Settings,
    /// Ce qui n'a pas pu être relu dans le fichier des réglages, tant que
    /// l'utilisateur n'en a pas pris connaissance.
    pub settings_issue: Option<LoadIssue>,
    /// Vrai si ce problème empêche d'analyser : des « dossiers jamais
    /// touchés » ont pu être oubliés.
    pub scan_blocked: bool,
    /// Le grand vide que Shinku n'a pas pu conclure la dernière fois (PC
    /// éteint, Shinku fermé de force), avec ce qu'il avait déjà fait. Rien
    /// n'est rejoué. Annoncé jusqu'à la prochaine analyse.
    pub interrupted_clean: Option<Run>,
}

/// Une page de la liste des gros fichiers.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFilesPage {
    /// Seuil appliqué, en octets sur le disque.
    pub threshold: u64,
    /// Nombre de fichiers au-dessus du seuil.
    pub count: u64,
    /// Place qu'ils occupent.
    pub bytes: u64,
    /// Nombre de fichiers gardés dans la liste (elle est bornée).
    pub listed: usize,
    /// Position du premier fichier de cette page.
    pub offset: usize,
    /// Les fichiers de cette page.
    pub files: Vec<LargeFile>,
}

/// Une page de la liste des groupes de doublons.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatesPage {
    /// Où en est la recherche.
    pub state: DuplicatesState,
    /// Nombre de fichiers examinés.
    pub candidates: u64,
    /// Nombre de copies en trop.
    pub copies: u64,
    /// Place que ces copies occupent.
    pub bytes: u64,
    /// Fichiers écartés en cours de route.
    pub skipped_files: u64,
    /// Liens physiques rencontrés.
    pub hard_links: u64,
    /// Nombre total de groupes.
    pub group_count: usize,
    /// Position du premier groupe de cette page.
    pub offset: usize,
    /// Les groupes de cette page.
    pub groups: Vec<DuplicateGroup>,
}

/// Pourquoi un dossier ne peut pas rejoindre les « dossiers jamais touchés ».
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderRefusal {
    /// Le chemin n'est pas absolu, ou contient `..`.
    NotAbsolute,
    /// Le chemin ne désigne pas un dossier lisible.
    NotAFolder,
    /// Le chemin contient des caractères que le fichier des réglages ne sait
    /// pas garder.
    NotUnicode,
}

/// Une demande refusée ou qui a échoué. L'interface en tire un message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum BackendError {
    /// Les réglages n'ont pas pu être relus en entier : l'utilisateur doit en
    /// prendre connaissance avant d'analyser.
    SettingsUnchecked,
    /// L'analyse n'a pas pu démarrer.
    ScanNotStarted {
        /// Ce que le système en dit.
        reason: String,
    },
    /// Les réglages n'ont pas pu être enregistrés : rien n'a changé.
    SettingsNotSaved {
        /// Ce que le système en dit.
        reason: String,
    },
    /// Ce dossier ne peut pas être ajouté aux « dossiers jamais touchés ».
    FolderRefused {
        /// La raison.
        reason: FolderRefusal,
    },
    /// Un grand vide tourne : cette demande attendra qu'il ait fini.
    CleanRunning,
    /// Le grand vide est refusé : rien n'a été touché.
    CleanRefused {
        /// La raison.
        reason: CleanRefusal,
    },
    /// Le grand vide n'a pas pu démarrer : rien n'a été touché.
    CleanNotStarted {
        /// Ce que le système en dit.
        reason: String,
    },
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SettingsUnchecked => {
                f.write_str("les réglages n'ont pas pu être relus en entier")
            }
            Self::ScanNotStarted { reason } => {
                write!(f, "l'analyse n'a pas pu démarrer : {reason}")
            }
            Self::SettingsNotSaved { reason } => {
                write!(f, "les réglages n'ont pas pu être enregistrés : {reason}")
            }
            Self::FolderRefused { reason } => write!(f, "dossier refusé : {reason:?}"),
            Self::CleanRunning => f.write_str("un grand vide est en cours"),
            Self::CleanRefused { reason } => write!(f, "grand vide refusé : {reason}"),
            Self::CleanNotStarted { reason } => {
                write!(f, "le grand vide n'a pas pu démarrer : {reason}")
            }
        }
    }
}

impl std::error::Error for BackendError {}

/// Les réglages en mémoire, tels qu'ils ont été lus puis modifiés.
#[derive(Debug)]
struct Desk {
    settings: Settings,
    /// Ce qui n'a pas pu être relu au démarrage, tant que l'utilisateur n'en
    /// a pas pris connaissance.
    issue: Option<LoadIssue>,
    /// Le grand vide coupé la dernière fois, tant qu'il reste à annoncer.
    interrupted: Option<Run>,
}

/// Le guichet de Shinku.
///
/// Lancer une analyse, lancer un grand vide et changer les « dossiers jamais
/// touchés » se font chacun d'un seul tenant, sous le verrou des réglages :
/// une analyse ou un grand vide ne part jamais avec une liste de dossiers
/// exclus qui vient de changer.
#[derive(Debug)]
pub struct Backend {
    environment: Arc<dyn Environment>,
    session: Session,
    store: Option<SettingsStore>,
    journal: Option<JournalStore>,
    desk: Mutex<Desk>,
    scan_defaults: ScanOptions,
}

impl Backend {
    /// Le guichet de cette machine : la vraie machine, ses réglages, son
    /// journal des grands vides, et un cache d'empreintes qui vit le temps du
    /// processus.
    pub fn for_machine() -> Self {
        let environment: Arc<dyn Environment> = Arc::new(platform::system());
        let store = SettingsStore::for_machine(environment.as_ref());
        let journal = JournalStore::for_machine(environment.as_ref());
        let backend = Self::new(environment, Arc::new(MemoryCache::new()), store);
        match journal {
            Some(journal) => backend.with_journal(journal),
            None => backend,
        }
    }

    /// Un guichet sur cette machine, ce cache et ce fichier de réglages.
    /// Sans fichier, les réglages par défaut s'appliquent et ne peuvent pas
    /// être changés. Sans journal ([`Backend::with_journal`]), les grands
    /// vides ne laissent pas de trace.
    pub fn new(
        environment: Arc<dyn Environment>,
        cache: Arc<dyn DigestCache>,
        store: Option<SettingsStore>,
    ) -> Self {
        let Loaded { settings, issue } = store.as_ref().map_or_else(
            || Loaded { settings: Settings::default(), issue: None },
            SettingsStore::load,
        );
        Self {
            session: Session::new(Arc::clone(&environment), cache),
            environment,
            store,
            journal: None,
            desk: Mutex::new(Desk { settings, issue, interrupted: None }),
            scan_defaults: ScanOptions::new(Trigger::Button),
        }
    }

    /// Branche le journal des grands vides.
    ///
    /// Un grand vide resté « en cours » dans ce journal n'a pas pu être
    /// conclu la dernière fois : il y est noté comme coupé, et annoncé dans
    /// [`Overview::interrupted_clean`]. Rien n'est rejoué.
    pub fn with_journal(mut self, journal: JournalStore) -> Self {
        let interrupted = journal.close_interrupted();
        self.desk.get_mut().unwrap_or_else(PoisonError::into_inner).interrupted = interrupted;
        self.journal = Some(journal);
        self
    }

    /// Remplace les réglages de base des analyses (seuils, modules). Les
    /// « dossiers jamais touchés » viennent toujours des réglages de
    /// l'utilisateur.
    pub fn with_scan_defaults(mut self, options: ScanOptions) -> Self {
        self.scan_defaults = options;
        self
    }

    fn desk(&self) -> MutexGuard<'_, Desk> {
        self.desk.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ce que la fenêtre affiche dès son ouverture, et rafraîchit ensuite.
    pub fn overview(&self) -> Overview {
        let (settings, settings_issue, interrupted_clean) = {
            let desk = self.desk();
            (desk.settings.clone(), desk.issue.clone(), desk.interrupted)
        };
        Overview {
            version: env!("CARGO_PKG_VERSION"),
            volumes: self.environment.volumes(),
            recycle_bin: self.environment.recycle_bin(),
            scan_blocked: settings_issue.as_ref().is_some_and(blocks_scan),
            settings,
            settings_issue,
            interrupted_clean,
        }
    }

    /// L'utilisateur a lu l'avertissement sur ses réglages : on ne le lui
    /// montre plus, et l'analyse redevient possible.
    pub fn acknowledge_settings_issue(&self) {
        self.desk().issue = None;
    }

    /// Lance une analyse avec les réglages en vigueur. Celle qui tournait,
    /// s'il y en avait une, est arrêtée. Refusé tant qu'un grand vide tourne.
    pub fn start_scan(&self) -> Result<SessionView, BackendError> {
        {
            let mut desk = self.desk();
            // Des dossiers à ne jamais toucher ont pu être oubliés : on
            // n'analyse pas dans le dos de l'utilisateur.
            if desk.issue.as_ref().is_some_and(blocks_scan) {
                return Err(BackendError::SettingsUnchecked);
            }
            let options = self.scan_defaults.clone().with_settings(&desk.settings);
            self.session.start_scan(options).map_err(|error| match error {
                SessionError::CleanRunning => BackendError::CleanRunning,
                SessionError::Thread(error) => {
                    BackendError::ScanNotStarted { reason: error.to_string() }
                }
                // Une analyse n'est jamais refusée comme un grand vide.
                SessionError::CleanRefused(refusal) => {
                    BackendError::ScanNotStarted { reason: refusal.to_string() }
                }
            })?;
            // Cette analyse repart du disque tel qu'il est : le grand vide
            // coupé la dernière fois n'a plus à être annoncé.
            desk.interrupted = None;
        }
        Ok(self.session.view())
    }

    /// Arrête l'analyse en cours et oublie ses résultats, ou referme le bilan
    /// d'un grand vide terminé. Sans effet sur un grand vide qui tourne.
    pub fn cancel_scan(&self) -> SessionView {
        self.session.cancel_scan();
        self.session.view()
    }

    /// Ce que l'écran de confirmation doit dire avant un grand vide : la
    /// corbeille telle qu'elle est, les apps ouvertes, les copies rangées
    /// dans un dossier synchronisé ou sur un autre disque.
    pub fn clean_preview(&self) -> Result<CleanPreview, BackendError> {
        self.session.clean_preview().map_err(|reason| BackendError::CleanRefused { reason })
    }

    /// Lance le grand vide que l'utilisateur vient de confirmer.
    ///
    /// La demande ne porte que trois cases et la date de l'analyse affichée.
    /// Les « dossiers jamais touchés » sont ceux des réglages en vigueur.
    /// Refusée, elle ne touche à rien et les résultats restent affichés.
    pub fn start_clean(&self, request: CleanRequest) -> Result<SessionView, BackendError> {
        {
            let desk = self.desk();
            // Même règle que pour analyser : avec une liste de dossiers à ne
            // jamais toucher peut-être incomplète, rien ne se supprime.
            if desk.issue.as_ref().is_some_and(blocks_scan) {
                return Err(BackendError::SettingsUnchecked);
            }
            let excluded = desk.settings.excluded_folders.clone();
            self.session.start_clean(request, excluded, self.journal.clone()).map_err(|error| {
                match error {
                    SessionError::CleanRunning => BackendError::CleanRunning,
                    SessionError::CleanRefused(reason) => BackendError::CleanRefused { reason },
                    SessionError::Thread(error) => {
                        BackendError::CleanNotStarted { reason: error.to_string() }
                    }
                }
            })?;
        }
        Ok(self.session.view())
    }

    /// Demande l'arrêt du grand vide en cours. Le fichier en cours est mené à
    /// son terme, puis le bilan partiel arrive : ce qui est parti reste parti.
    /// Sans effet s'il n'y en a pas, ou s'il vient de finir.
    pub fn cancel_clean(&self) -> SessionView {
        self.session.cancel_clean();
        self.session.view()
    }

    /// À appeler quand Shinku se ferme. Ce qui tourne est arrêté ; un grand
    /// vide a quelques secondes pour finir son fichier en cours et noter son
    /// bilan au journal. S'il n'y arrive pas, Shinku se ferme quand même, et
    /// l'annoncera au prochain lancement.
    pub fn shutdown(&self) {
        self.session.shutdown(SHUTDOWN_PATIENCE);
    }

    /// Où en est la session : état, avancement, totaux.
    pub fn view(&self) -> SessionView {
        self.session.view()
    }

    /// Le détail des fichiers inutiles, s'il y a des résultats.
    pub fn junk(&self) -> Option<JunkReport> {
        self.session.report().map(|report| report.junk.clone())
    }

    /// Une page de gros fichiers, s'il y a des résultats.
    pub fn large_files(&self, offset: usize, limit: usize) -> Option<LargeFilesPage> {
        let report = self.session.report()?;
        let large = &report.large_files;
        Some(LargeFilesPage {
            threshold: large.threshold,
            count: large.count,
            bytes: large.bytes,
            listed: large.files.len(),
            offset: offset.min(large.files.len()),
            files: report.large_files_page(offset, limit.min(PAGE_LIMIT)).to_vec(),
        })
    }

    /// Une page de groupes de doublons, s'il y a des résultats.
    pub fn duplicates(&self, offset: usize, limit: usize) -> Option<DuplicatesPage> {
        let report = self.session.report()?;
        let duplicates = &report.duplicates;
        Some(DuplicatesPage {
            state: duplicates.state,
            candidates: duplicates.candidates,
            copies: duplicates.copies,
            bytes: duplicates.bytes,
            skipped_files: duplicates.skipped_files,
            hard_links: duplicates.hard_links,
            group_count: duplicates.groups.len(),
            offset: offset.min(duplicates.groups.len()),
            groups: report.duplicates_page(offset, limit.min(PAGE_LIMIT)).to_vec(),
        })
    }

    /// Enregistre l'exemplaire que l'utilisateur veut garder d'un doublon,
    /// pour le prochain grand vide. Le choix désigne un exemplaire déjà
    /// connu du cœur, par son groupe et son rang : aucun chemin n'est reçu.
    /// Faux si le choix ne désigne rien dans les résultats en cours.
    pub fn keep_duplicate(&self, choice: &KeepChoice) -> bool {
        self.session.keep_duplicate(choice)
    }

    /// Le mode d'affichage en vigueur. Réponse immédiate : la fenêtre s'en
    /// sert pour prendre la bonne couleur avant même d'afficher sa page.
    pub fn theme(&self) -> Theme {
        self.desk().settings.theme
    }

    /// Change le mode d'affichage.
    pub fn set_theme(&self, theme: Theme) -> Result<Settings, BackendError> {
        self.change_settings(|settings| settings.theme = theme)
    }

    /// Ajoute un dossier aux « dossiers jamais touchés ».
    ///
    /// Les résultats en cours sont oubliés : ils ont été établis avec
    /// l'ancienne liste. Refusé tant qu'un grand vide tourne.
    pub fn exclude_folder(&self, path: &Path) -> Result<Settings, BackendError> {
        let folder = checked_folder(path)?;
        let wanted = fold_path(&folder);
        self.change_exclusions(|settings| {
            let known = settings.excluded_folders.iter().any(|other| fold_path(other) == wanted);
            if !known {
                settings.excluded_folders.push(folder);
            }
        })
    }

    /// Retire un dossier des « dossiers jamais touchés ». Sans effet s'il n'y
    /// est pas.
    ///
    /// Les résultats en cours sont oubliés : ils ont été établis avec
    /// l'ancienne liste. Refusé tant qu'un grand vide tourne.
    pub fn include_folder(&self, path: &Path) -> Result<Settings, BackendError> {
        self.change_exclusions(|settings| settings.excluded_folders.retain(|other| other != path))
    }

    /// Change la liste des « dossiers jamais touchés » et oublie les résultats
    /// établis avec l'ancienne.
    ///
    /// Un grand vide qui tourne a démarré avec la liste d'avant : lui en
    /// donner une autre en route ne protégerait rien. La demande est refusée
    /// jusqu'à ce qu'il ait fini.
    fn change_exclusions(
        &self,
        edit: impl FnOnce(&mut Settings),
    ) -> Result<Settings, BackendError> {
        let mut desk = self.desk();
        if self.session.is_cleaning() {
            return Err(BackendError::CleanRunning);
        }
        let settings = self.save_settings(&mut desk, edit)?;
        // Le bilan d'un grand vide terminé n'a rien à voir avec la liste : il reste.
        self.session.forget_scan();
        Ok(settings)
    }

    /// Applique un changement aux réglages et l'enregistre.
    fn change_settings(&self, edit: impl FnOnce(&mut Settings)) -> Result<Settings, BackendError> {
        self.save_settings(&mut self.desk(), edit)
    }

    /// Applique un changement aux réglages et l'enregistre. Si
    /// l'enregistrement échoue, rien ne change, ni en mémoire ni sur le disque.
    fn save_settings(
        &self,
        desk: &mut Desk,
        edit: impl FnOnce(&mut Settings),
    ) -> Result<Settings, BackendError> {
        let mut next = desk.settings.clone();
        edit(&mut next);
        next.sanitize();
        if next == desk.settings {
            return Ok(next);
        }
        let store = self.store.as_ref().ok_or_else(|| BackendError::SettingsNotSaved {
            reason: "le dossier AppData de ce compte est introuvable".to_owned(),
        })?;
        store
            .save(&next)
            .map_err(|error| BackendError::SettingsNotSaved { reason: error.to_string() })?;
        desk.settings = next.clone();
        Ok(next)
    }
}

/// Vrai si ce problème de lecture a pu faire oublier des dossiers exclus.
fn blocks_scan(issue: &LoadIssue) -> bool {
    match issue {
        LoadIssue::Unreadable { .. } => true,
        LoadIssue::Partial { fields } => fields.contains(&EXCLUDED_FIELD),
    }
}

/// Contrôle un dossier à exclure et le rend sous sa forme habituelle (`C:\…`).
fn checked_folder(path: &Path) -> Result<PathBuf, BackendError> {
    let refuse = |reason| BackendError::FolderRefused { reason };
    if path.to_str().is_none() {
        return Err(refuse(FolderRefusal::NotUnicode));
    }
    let internal = to_internal_path(path).map_err(|_| refuse(FolderRefusal::NotAbsolute))?;
    if fold_path(&internal).is_none() {
        return Err(refuse(FolderRefusal::NotAbsolute));
    }
    // Un lien vers un dossier est accepté : le plan d'analyse exclut aussi le
    // dossier réel vers lequel il mène.
    match std::fs::metadata(&internal) {
        Ok(metadata) if metadata.is_dir() => Ok(to_display_path(&internal)),
        _ => Err(refuse(FolderRefusal::NotAFolder)),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, Instant, SystemTime};

    use super::*;
    use crate::clean::journal::RunState;
    use crate::clean::report::{CleanOutcome, CleanReport, StepState};
    use crate::model::{ModuleId, Timestamp};
    use crate::platform::{KnownFolder, StaticEnvironment};
    use crate::session::SessionState;
    use crate::testing::SlowBin;

    /// Un petit PC : un fichier temporaire ancien, deux photos identiques,
    /// une vidéo, un dossier privé qui contient une copie de la photo, et une
    /// corbeille qui contient déjà trois éléments.
    struct Pc {
        _workspace: tempfile::TempDir,
        environment: StaticEnvironment,
        profile: PathBuf,
        store: SettingsStore,
        journal: JournalStore,
    }

    impl Pc {
        fn temp(&self) -> PathBuf {
            self.profile.join("AppData").join("Local").join("Temp")
        }

        /// Les trois exemplaires de la photo.
        fn photos(&self) -> [PathBuf; 3] {
            let documents = self.profile.join("Documents");
            [
                documents.join("photo.jpg"),
                self.profile.join("Pictures").join("photo.jpg"),
                documents.join("Privé").join("photo.jpg"),
            ]
        }
    }

    fn pc() -> Pc {
        let workspace = tempfile::tempdir().unwrap();
        let root = to_internal_path(&fs::canonicalize(workspace.path()).unwrap()).unwrap();
        let profile = root.join("Users").join("a");
        let local = profile.join("AppData").join("Local");
        let roaming = profile.join("AppData").join("Roaming");
        fs::create_dir_all(local.join("Temp")).unwrap();
        fs::create_dir_all(&roaming).unwrap();
        fs::create_dir_all(profile.join("Documents").join("Privé")).unwrap();
        fs::create_dir_all(profile.join("Pictures")).unwrap();
        fs::create_dir_all(profile.join("Videos")).unwrap();
        fs::create_dir_all(root.join("$Recycle.Bin")).unwrap();
        fs::write(local.join("Temp").join("vieux.tmp"), [7u8; 9000]).unwrap();
        fs::write(profile.join("Documents").join("photo.jpg"), [3u8; 8000]).unwrap();
        fs::write(profile.join("Pictures").join("photo.jpg"), [3u8; 8000]).unwrap();
        fs::write(profile.join("Documents").join("Privé").join("photo.jpg"), [3u8; 8000]).unwrap();
        fs::write(profile.join("Videos").join("film.mkv"), [9u8; 70_000]).unwrap();
        let month = Duration::from_secs(30 * 24 * 3600);
        let environment = StaticEnvironment::new()
            .with_now(Timestamp::from_system_time(SystemTime::now() + month))
            .with_parallelism(2)
            .with_volume(Volume {
                root: root.clone(),
                label: "Système".to_owned(),
                file_system: "NTFS".to_owned(),
                total: 1_000_000,
                free: 400_000,
                fixed: true,
                seek_penalty: Some(false),
                system: true,
            })
            .with_recycle_bin(RecycleBin { bytes: 4096, items: 3 })
            .with_recycle_folder(root.join("$Recycle.Bin"))
            .with_folder(KnownFolder::Profile, profile.clone())
            .with_folder(KnownFolder::Documents, profile.join("Documents"))
            .with_folder(KnownFolder::Pictures, profile.join("Pictures"))
            .with_folder(KnownFolder::Videos, profile.join("Videos"))
            .with_folder(KnownFolder::LocalAppData, local.clone())
            .with_folder(KnownFolder::RoamingAppData, roaming)
            .with_folder(KnownFolder::Temp, local.join("Temp"));
        let store = SettingsStore::for_machine(&environment).unwrap();
        let journal = JournalStore::for_machine(&environment).unwrap();
        Pc { _workspace: workspace, environment, profile, store, journal }
    }

    fn backend(pc: &Pc) -> Backend {
        backend_on(pc.environment.clone(), pc)
    }

    /// Le guichet de ce PC, vu à travers une autre machine que la sienne.
    fn backend_on(environment: impl Environment + 'static, pc: &Pc) -> Backend {
        let mut options = ScanOptions::new(Trigger::Button);
        options.duplicate_min_size = 4096;
        options.large_threshold = 32 * 1024;
        Backend::new(Arc::new(environment), Arc::new(MemoryCache::new()), Some(pc.store.clone()))
            .with_scan_defaults(options)
            .with_journal(pc.journal.clone())
    }

    fn wait_for(backend: &Backend, wanted: SessionState) -> SessionView {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let view = backend.view();
            if view.state == wanted {
                return view;
            }
            assert!(Instant::now() < deadline, "état attendu : {wanted:?}, vu : {:?}", view.state);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_results(backend: &Backend) -> SessionView {
        wait_for(backend, SessionState::Results)
    }

    /// Attend la fin du grand vide et rend son bilan.
    fn wait_for_the_report(backend: &Backend) -> CleanReport {
        let view = wait_for(backend, SessionState::Cleaned);
        view.clean.and_then(|clean| clean.report).expect("le bilan")
    }

    /// La demande que l'écran de confirmation enverrait, toutes cases cochées.
    fn everything(backend: &Backend) -> CleanRequest {
        let preview = backend.clean_preview().expect("l'écran de confirmation");
        CleanRequest {
            analysis: preview.analysis,
            recycle_bin: true,
            junk_files: true,
            duplicates: true,
        }
    }

    fn items(view: &SessionView, module: ModuleId) -> u64 {
        let summary = view.summary.as_ref().expect("un bilan");
        summary.categories.iter().find(|category| category.module == module).unwrap().items
    }

    #[test]
    fn the_overview_describes_the_machine() {
        let pc = pc();
        let backend = backend(&pc);
        let overview = backend.overview();
        assert_eq!(overview.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(overview.volumes.len(), 1);
        assert_eq!(overview.recycle_bin, Some(RecycleBin { bytes: 4096, items: 3 }));
        assert_eq!(overview.settings, Settings::default());
        assert_eq!(overview.settings_issue, None);
        assert!(!overview.scan_blocked);

        let json = serde_json::to_value(&overview).unwrap();
        assert_eq!(json["volumes"][0]["label"], "Système");
        assert_eq!(json["recycleBin"]["items"], 3);
        assert_eq!(json["settings"]["theme"], "system");
        assert_eq!(json["settingsIssue"], serde_json::Value::Null);
        assert_eq!(json["scanBlocked"], false);
        assert_eq!(json["interruptedClean"], serde_json::Value::Null);
    }

    #[test]
    fn a_scan_gives_pages_of_results() {
        let pc = pc();
        let backend = backend(&pc);
        assert!(backend.junk().is_none());
        assert!(backend.large_files(0, 10).is_none());
        assert!(backend.duplicates(0, 10).is_none());

        let started = backend.start_scan().unwrap();
        assert_ne!(started.state, SessionState::Failed);
        let view = wait_for_results(&backend);
        assert_eq!(items(&view, ModuleId::JunkFiles), 1);
        assert_eq!(items(&view, ModuleId::Duplicates), 2);
        assert_eq!(items(&view, ModuleId::LargeFiles), 1);
        assert_eq!(items(&view, ModuleId::RecycleBin), 3);

        let junk = backend.junk().unwrap();
        assert_eq!(junk.files, 1);

        let large = backend.large_files(0, 10).unwrap();
        assert_eq!((large.count, large.listed, large.offset), (1, 1, 0));
        assert_eq!(large.files.len(), 1);
        assert!(large.files[0].path.ends_with("film.mkv"));
        // Au-delà de la fin, la page est vide et le dit.
        let beyond = backend.large_files(50, 10).unwrap();
        assert_eq!((beyond.offset, beyond.files.len()), (1, 0));

        let duplicates = backend.duplicates(0, usize::MAX).unwrap();
        assert_eq!(duplicates.state, DuplicatesState::Done);
        assert_eq!((duplicates.group_count, duplicates.copies), (1, 2));
        assert_eq!(duplicates.groups[0].files.len(), 3);

        // Les chemins voyagent sous leur forme habituelle.
        let json = serde_json::to_value(&large).unwrap();
        let path = json["files"][0]["path"].as_str().unwrap();
        assert!(!path.starts_with(r"\\?\"), "{path}");
    }

    #[test]
    fn excluding_a_folder_is_saved_and_forgets_the_results() {
        let pc = pc();
        let backend = backend(&pc);
        backend.start_scan().unwrap();
        let view = wait_for_results(&backend);
        assert_eq!(items(&view, ModuleId::Duplicates), 2);

        let private = pc.profile.join("Documents").join("Privé");
        let settings = backend.exclude_folder(&private).unwrap();
        assert_eq!(settings.excluded_folders, [to_display_path(&private)]);
        // Les anciens résultats montraient le contenu du dossier : ils partent.
        assert_eq!(backend.view().state, SessionState::Idle);
        assert!(backend.duplicates(0, 10).is_none());
        // Le fichier des réglages le sait déjà.
        assert_eq!(pc.store.load().settings.excluded_folders, settings.excluded_folders);

        // L'ajouter deux fois ne le range qu'une fois.
        let again = backend.exclude_folder(&private).unwrap();
        assert_eq!(again.excluded_folders.len(), 1);

        backend.start_scan().unwrap();
        let view = wait_for_results(&backend);
        assert_eq!(items(&view, ModuleId::Duplicates), 1, "la copie privée n'est plus vue");
        let duplicates = backend.duplicates(0, 10).unwrap();
        assert!(duplicates.groups[0].files.iter().all(|file| !file.path.starts_with(&private)));

        // Un autre guichet, plus tard, retrouve le dossier exclu.
        let later = self::backend(&pc);
        assert_eq!(later.overview().settings.excluded_folders, settings.excluded_folders);

        let settings = backend.include_folder(&settings.excluded_folders[0]).unwrap();
        assert!(settings.excluded_folders.is_empty());
        assert_eq!(backend.view().state, SessionState::Idle);
        assert!(pc.store.load().settings.excluded_folders.is_empty());
    }

    #[test]
    fn only_real_folders_can_be_excluded() {
        let pc = pc();
        let backend = backend(&pc);
        let refused = |path: &Path| match backend.exclude_folder(path) {
            Err(BackendError::FolderRefused { reason }) => reason,
            other => panic!("refus attendu pour {path:?} : {other:?}"),
        };
        assert_eq!(refused(Path::new("Documents")), FolderRefusal::NotAbsolute);
        // Sous sa forme habituelle (`C:\…`), un chemin peut contenir « .. ».
        let dotted = to_display_path(&pc.profile).join("Documents").join("..").join("Pictures");
        assert_eq!(refused(&dotted), FolderRefusal::NotAbsolute);
        assert_eq!(
            refused(&pc.profile.join("Documents").join("photo.jpg")),
            FolderRefusal::NotAFolder
        );
        assert_eq!(refused(&pc.profile.join("Absent")), FolderRefusal::NotAFolder);
        assert!(backend.overview().settings.excluded_folders.is_empty());
        assert_eq!(pc.store.load().settings, Settings::default());
    }

    #[test]
    fn the_theme_is_saved() {
        let pc = pc();
        let backend = backend(&pc);
        backend.start_scan().unwrap();
        wait_for_results(&backend);
        assert_eq!(backend.theme(), Theme::System);
        let settings = backend.set_theme(Theme::Light).unwrap();
        assert_eq!(settings.theme, Theme::Light);
        assert_eq!(backend.theme(), Theme::Light);
        assert_eq!(pc.store.load().settings.theme, Theme::Light);
        // Changer d'apparence ne touche pas aux résultats.
        assert_eq!(backend.view().state, SessionState::Results);
    }

    #[test]
    fn a_failed_save_changes_nothing() {
        let pc = pc();
        // Le « dossier » des réglages est un fichier : rien ne peut s'y écrire.
        let blocked = pc.profile.join("bloque");
        fs::write(&blocked, b"x").unwrap();
        let store = SettingsStore::at(blocked.join("Shinku").join("settings.json"));
        let backend = Backend::new(
            Arc::new(pc.environment.clone()),
            Arc::new(MemoryCache::new()),
            Some(store),
        );
        let error = backend.set_theme(Theme::Dark).unwrap_err();
        assert!(matches!(error, BackendError::SettingsNotSaved { .. }), "{error:?}");
        assert_eq!(backend.overview().settings.theme, Theme::System);
        assert_eq!(backend.theme(), Theme::System);

        // Sans fichier de réglages du tout, même réponse.
        let backend =
            Backend::new(Arc::new(pc.environment.clone()), Arc::new(MemoryCache::new()), None);
        assert!(matches!(
            backend.set_theme(Theme::Dark),
            Err(BackendError::SettingsNotSaved { .. })
        ));
        // Redemander la valeur en vigueur n'écrit rien : ce n'est pas une erreur.
        assert!(backend.set_theme(Theme::System).is_ok());
    }

    #[test]
    fn forgotten_exclusions_block_the_scan_until_acknowledged() {
        let pc = pc();
        fs::create_dir_all(pc.store.path().parent().unwrap()).unwrap();
        fs::write(pc.store.path(), "{ \"excludedFolders\": [\"C:\\\\Privé\"").unwrap();
        let backend = backend(&pc);
        let overview = backend.overview();
        assert!(matches!(overview.settings_issue, Some(LoadIssue::Unreadable { .. })));
        assert!(overview.scan_blocked);
        assert_eq!(backend.start_scan().unwrap_err(), BackendError::SettingsUnchecked);
        assert_eq!(backend.view().state, SessionState::Idle);

        backend.acknowledge_settings_issue();
        let overview = backend.overview();
        assert_eq!(overview.settings_issue, None);
        assert!(!overview.scan_blocked);
        backend.start_scan().unwrap();
        wait_for_results(&backend);
    }

    #[test]
    fn a_damaged_exclusion_blocks_the_scan_but_a_bad_theme_does_not() {
        let pc = pc();
        fs::create_dir_all(pc.store.path().parent().unwrap()).unwrap();
        fs::write(pc.store.path(), r#"{ "theme": "fluo" }"#).unwrap();
        let overview = backend(&pc).overview();
        assert!(matches!(overview.settings_issue, Some(LoadIssue::Partial { .. })));
        assert!(!overview.scan_blocked);

        fs::write(pc.store.path(), r#"{ "excludedFolders": ["D:\\Photos", 12] }"#).unwrap();
        let backend = backend(&pc);
        assert!(backend.overview().scan_blocked);
        assert_eq!(backend.start_scan().unwrap_err(), BackendError::SettingsUnchecked);
    }

    #[test]
    fn errors_travel_as_plain_json() {
        let json =
            serde_json::to_value(BackendError::FolderRefused { reason: FolderRefusal::NotAFolder })
                .unwrap();
        assert_eq!(json["kind"], "folderRefused");
        assert_eq!(json["reason"], "notAFolder");
        let json = serde_json::to_value(BackendError::SettingsUnchecked).unwrap();
        assert_eq!(json["kind"], "settingsUnchecked");
        let json =
            serde_json::to_value(BackendError::SettingsNotSaved { reason: "disque plein".into() })
                .unwrap();
        assert_eq!(json["kind"], "settingsNotSaved");
        assert_eq!(json["reason"], "disque plein");
        let json = serde_json::to_value(BackendError::CleanRunning).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "cleanRunning" }));
        let json =
            serde_json::to_value(BackendError::CleanRefused { reason: CleanRefusal::StaleResults })
                .unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "cleanRefused", "reason": "staleResults" }));
        let json = serde_json::to_value(BackendError::CleanNotStarted {
            reason: "plus de mémoire".into(),
        })
        .unwrap();
        assert_eq!(json["kind"], "cleanNotStarted");
        assert_eq!(json["reason"], "plus de mémoire");
    }

    #[test]
    fn a_clean_goes_from_the_confirmation_to_its_report() {
        let pc = pc();
        let backend = backend(&pc);
        // Sans résultats, il n'y a rien à annoncer ni à lancer.
        let no_results = BackendError::CleanRefused { reason: CleanRefusal::NoResults };
        assert_eq!(backend.clean_preview().unwrap_err(), no_results);
        let blind =
            CleanRequest { analysis: 0, recycle_bin: true, junk_files: true, duplicates: true };
        assert_eq!(backend.start_clean(blind).unwrap_err(), no_results);

        backend.start_scan().unwrap();
        wait_for_results(&backend);
        let preview = backend.clean_preview().unwrap();
        assert_eq!(preview.recycle_bin, Some(RecycleBin { bytes: 4096, items: 3 }));
        assert_eq!((preview.junk.files, preview.duplicates.copies), (1, 2));

        let started = backend.start_clean(everything(&backend)).unwrap();
        assert!(matches!(started.state, SessionState::Cleaning | SessionState::Cleaned));
        let report = wait_for_the_report(&backend);
        assert_eq!(report.outcome, CleanOutcome::Completed);
        assert_eq!((report.recycle_bin.state, report.recycle_bin.items), (StepState::Done, 3));
        assert_eq!((report.junk.state, report.junk.files), (StepState::Done, 1));
        assert_eq!((report.duplicates.state, report.duplicates.copies), (StepState::Done, 2));
        assert_eq!(report.skipped.total(), 0);

        // Sur le disque : le fichier temporaire est parti, un exemplaire de la
        // photo est resté, la vidéo n'a pas bougé.
        assert!(!pc.temp().join("vieux.tmp").exists());
        assert_eq!(pc.photos().iter().filter(|photo| photo.exists()).count(), 1);
        assert!(pc.profile.join("Videos").join("film.mkv").exists());
        // La corbeille a d'abord été vidée, puis a reçu les deux copies : elles
        // sont récupérables.
        assert_eq!(backend.overview().recycle_bin, Some(RecycleBin { bytes: 16_000, items: 2 }));

        // Les listes de l'analyse ne sont plus proposées : le disque a changé.
        assert!(backend.junk().is_none());
        assert!(backend.duplicates(0, 10).is_none());
        assert!(backend.large_files(0, 10).is_none());
        assert_eq!(backend.clean_preview().unwrap_err(), no_results);

        // Le journal garde les totaux de ce grand vide.
        let noted = pc.journal.runs();
        assert_eq!(noted.len(), 1);
        assert_eq!(noted[0].state, RunState::Completed);
        assert_eq!(noted[0].freed_bytes, report.freed_bytes);
        assert_eq!(noted[0].recoverable_bytes, report.recoverable_bytes);
        assert_eq!(backend.overview().interrupted_clean, None);

        // Refermer le bilan ramène à l'accueil.
        assert_eq!(backend.cancel_clean().state, SessionState::Cleaned);
        assert_eq!(backend.cancel_scan().state, SessionState::Idle);
        backend.shutdown();
        assert_eq!(backend.view().state, SessionState::Idle);
    }

    #[test]
    fn a_clean_never_enters_a_folder_never_to_touch() {
        let pc = pc();
        let backend = backend(&pc);
        // Un dossier à ne jamais toucher, rangé au milieu des fichiers temporaires.
        let kept = pc.temp().join("garde");
        fs::create_dir_all(&kept).unwrap();
        fs::write(kept.join("important.tmp"), [5u8; 5000]).unwrap();
        backend.exclude_folder(&kept).unwrap();

        backend.start_scan().unwrap();
        let view = wait_for_results(&backend);
        assert_eq!(items(&view, ModuleId::JunkFiles), 1);
        let request =
            CleanRequest { recycle_bin: false, duplicates: false, ..everything(&backend) };
        backend.start_clean(request).unwrap();
        let report = wait_for_the_report(&backend);
        assert_eq!((report.junk.state, report.junk.files), (StepState::Done, 1));
        assert!(!pc.temp().join("vieux.tmp").exists());
        assert!(kept.join("important.tmp").exists(), "le dossier exclu a été nettoyé");
    }

    #[test]
    fn while_a_clean_runs_everything_else_waits() {
        let pc = pc();
        let (slow, gate) = SlowBin::new(pc.environment.clone());
        let backend = backend_on(slow, &pc);
        backend.start_scan().unwrap();
        wait_for_results(&backend);
        let bin_only =
            CleanRequest { junk_files: false, duplicates: false, ..everything(&backend) };
        assert_eq!(backend.start_clean(bin_only).unwrap().state, SessionState::Cleaning);
        gate.wait_until_reached();

        assert_eq!(backend.start_scan().unwrap_err(), BackendError::CleanRunning);
        assert_eq!(backend.start_clean(bin_only).unwrap_err(), BackendError::CleanRunning);
        // Changer les dossiers à ne jamais toucher en plein grand vide ne
        // protégerait rien : il a démarré avec la liste d'avant.
        let private = pc.profile.join("Documents").join("Privé");
        assert_eq!(backend.exclude_folder(&private).unwrap_err(), BackendError::CleanRunning);
        assert_eq!(backend.include_folder(&private).unwrap_err(), BackendError::CleanRunning);
        assert!(backend.overview().settings.excluded_folders.is_empty());
        assert!(pc.store.load().settings.excluded_folders.is_empty());
        // L'apparence, elle, ne change rien à ce qui est nettoyé.
        backend.set_theme(Theme::Dark).unwrap();
        assert_eq!(backend.cancel_scan().state, SessionState::Cleaning);

        // Demander l'arrêt ne coupe pas l'étape en cours : elle se termine,
        // puis le bilan arrive.
        assert_eq!(backend.cancel_clean().state, SessionState::Cleaning);
        gate.open();
        let report = wait_for_the_report(&backend);
        assert_eq!((report.recycle_bin.state, report.recycle_bin.items), (StepState::Done, 3));

        // Le grand vide fini, les réglages se changent à nouveau. Le bilan,
        // lui, reste affiché : il n'a rien à voir avec cette liste.
        let settings = backend.exclude_folder(&private).unwrap();
        assert_eq!(settings.excluded_folders, [to_display_path(&private)]);
        assert_eq!(backend.view().state, SessionState::Cleaned);
    }

    #[test]
    fn a_clean_cut_short_is_announced_until_the_next_scan() {
        let pc = pc();
        // La dernière fois, le PC s'est éteint en plein grand vide.
        let mut cut = Run::starting(pc.environment.now());
        cut.freed_bytes = 7_200_000_000;
        cut.skipped_files = 2;
        pc.journal.record(&cut).unwrap();

        let backend = backend(&pc);
        let announced = backend.overview().interrupted_clean.expect("le grand vide coupé");
        assert_eq!(announced, Run { state: RunState::Interrupted, ..cut });
        let json = serde_json::to_value(backend.overview()).unwrap();
        assert_eq!(json["interruptedClean"]["state"], "interrupted");
        assert_eq!(json["interruptedClean"]["freedBytes"], 7_200_000_000_u64);
        assert_eq!(json["interruptedClean"]["finishedMs"], serde_json::Value::Null);

        // Une nouvelle analyse repart du disque tel qu'il est : l'annonce a servi.
        backend.start_scan().unwrap();
        assert_eq!(backend.overview().interrupted_clean, None);
        // Rien n'est rejoué, et il n'est annoncé qu'au lancement qui suit.
        assert_eq!(pc.journal.runs(), [Run { state: RunState::Interrupted, ..cut }]);
        assert_eq!(self::backend(&pc).overview().interrupted_clean, None);
    }

    #[test]
    fn forgotten_exclusions_block_the_clean_too() {
        let pc = pc();
        let backend = backend(&pc);
        backend.start_scan().unwrap();
        wait_for_results(&backend);
        let request = everything(&backend);
        // Cas d'école : le doute sur les dossiers exclus n'apparaît qu'une
        // fois l'analyse faite.
        backend.desk().issue = Some(LoadIssue::Partial { fields: vec![EXCLUDED_FIELD] });
        assert_eq!(backend.start_clean(request).unwrap_err(), BackendError::SettingsUnchecked);
        assert_eq!(backend.view().state, SessionState::Results);
        assert!(pc.temp().join("vieux.tmp").exists());
        assert!(pc.photos().iter().all(|photo| photo.exists()));

        backend.acknowledge_settings_issue();
        backend.start_clean(request).unwrap();
        wait_for_the_report(&backend);
        assert!(!pc.temp().join("vieux.tmp").exists());
    }
}
