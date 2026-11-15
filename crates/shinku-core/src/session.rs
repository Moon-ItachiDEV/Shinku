//! La session : ce que la fenêtre de Shinku suit du début à la fin.
//!
//! Une session lance une analyse, la suit, l'arrête, et garde son rapport.
//! Elle lance ensuite le grand vide à partir de ce rapport, et garde son
//! bilan. L'interface n'en reçoit qu'une vue ([`SessionView`]) : un état, un
//! avancement et des totaux. Les listes se demandent ensuite page par page,
//! au rapport gardé ici.
//!
//! Une seule chose tourne à la fois. Lancer une analyse arrête la précédente :
//! ses résultats partiels sont jetés, ses empreintes restent dans le cache.
//! Un grand vide, lui, ne se remplace pas : tant qu'il tourne, aucune analyse
//! ne démarre. Et dès qu'il démarre, les résultats de l'analyse sont oubliés :
//! ils décrivent un disque qui n'existe plus.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::clean::journal::JournalStore;
use crate::clean::report::CleanReport;
use crate::clean::{
    self, CleanHandle, CleanPreview, CleanProgressSnapshot, CleanRefusal, CleanRequest,
    CleanStartError,
};
use crate::hash::DigestCache;
use crate::platform::Environment;
use crate::scan::report::{Report, Summary};
use crate::scan::{Phase, ProgressSnapshot, ScanHandle, ScanOptions, start};

/// Où en est la session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    /// Rien n'a été lancé, ou la dernière analyse a été arrêtée.
    Idle,
    /// Une analyse tourne. Un premier rapport peut déjà être disponible.
    Scanning,
    /// L'analyse est terminée : le rapport est complet.
    Results,
    /// L'analyse ou le grand vide s'est arrêté sur une erreur inattendue.
    Failed,
    /// Un grand vide tourne.
    Cleaning,
    /// Le grand vide est terminé ou a été arrêté : son bilan est prêt.
    Cleaned,
}

/// Ce que l'analyse n'a pas pu lire, en chiffres.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedCounts {
    /// Dossiers non analysés : interdits au compte, illisibles ou remplacés
    /// pendant l'analyse.
    pub folders: u64,
    /// Emplacements écartés avant même de commencer.
    pub refused_roots: u64,
}

/// Ce que l'interface affiche d'une session.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    /// État de la session.
    pub state: SessionState,
    /// Avancement de l'analyse.
    pub progress: ProgressSnapshot,
    /// Totaux du dernier rapport publié, même partiel.
    pub summary: Option<Summary>,
    /// Ce qui n'a pas pu être analysé.
    pub skipped: SkippedCounts,
    /// Vrai si les résultats sont trop vieux pour nettoyer sans réanalyser.
    pub stale: bool,
    /// Le grand vide en cours ou terminé, s'il y en a un.
    pub clean: Option<CleanView>,
}

/// Ce que l'interface affiche d'un grand vide.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanView {
    /// Ce qui lui a été demandé : les trois cases, telles que confirmées.
    pub request: CleanRequest,
    /// Son avancement.
    pub progress: CleanProgressSnapshot,
    /// Son bilan, une fois terminé ou arrêté.
    pub report: Option<CleanReport>,
}

/// Pourquoi la session refuse une demande.
#[derive(Debug)]
pub enum SessionError {
    /// Un grand vide tourne : la demande attendra qu'il finisse.
    CleanRunning,
    /// Le grand vide est refusé : rien n'a été touché.
    CleanRefused(CleanRefusal),
    /// Le système n'a pas pu créer le fil d'exécution.
    Thread(io::Error),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CleanRunning => f.write_str("un grand vide est en cours"),
            Self::CleanRefused(refusal) => write!(f, "grand vide refusé : {refusal}"),
            Self::Thread(error) => write!(f, "le fil d'exécution n'a pas pu démarrer : {error}"),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CleanRunning => None,
            Self::CleanRefused(refusal) => Some(refusal),
            Self::Thread(error) => Some(error),
        }
    }
}

/// Ce que la session est en train de suivre.
#[derive(Debug)]
enum Activity {
    /// Rien.
    Idle,
    /// Une analyse, en cours ou terminée, avec son rapport.
    Scan(ScanHandle),
    /// Un grand vide, en cours ou terminé, avec son bilan.
    Clean(CleanHandle),
}

/// Vrai tant que ce grand vide n'a pas rendu son bilan. Il faut regarder
/// d'abord si son fil a fini : s'il a fini, le bilan lu ensuite est définitif.
fn still_cleaning(clean: &CleanHandle) -> bool {
    !clean.is_finished() && clean.control().report().is_none()
}

/// Une session de Shinku.
#[derive(Debug)]
pub struct Session {
    environment: Arc<dyn Environment>,
    cache: Arc<dyn DigestCache>,
    activity: Mutex<Activity>,
}

impl Session {
    /// Une session sur cette machine, avec ce cache d'empreintes.
    pub fn new(environment: Arc<dyn Environment>, cache: Arc<dyn DigestCache>) -> Self {
        Self { environment, cache, activity: Mutex::new(Activity::Idle) }
    }

    /// La machine de cette session.
    pub fn environment(&self) -> &Arc<dyn Environment> {
        &self.environment
    }

    fn activity(&self) -> MutexGuard<'_, Activity> {
        self.activity.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Lance une analyse. Celle qui tournait, s'il y en avait une, est
    /// arrêtée ; le bilan d'un grand vide terminé est oublié. Refusé tant
    /// qu'un grand vide tourne.
    pub fn start_scan(&self, options: ScanOptions) -> Result<(), SessionError> {
        let mut activity = self.activity();
        match &*activity {
            Activity::Clean(clean) if still_cleaning(clean) => {
                return Err(SessionError::CleanRunning);
            }
            Activity::Scan(previous) => previous.control().cancel(),
            Activity::Idle | Activity::Clean(_) => {}
        }
        let scan = start(Arc::clone(&self.environment), Arc::clone(&self.cache), options)
            .map_err(SessionError::Thread)?;
        *activity = Activity::Scan(scan);
        Ok(())
    }

    /// Arrête l'analyse en cours et oublie ses résultats, ou oublie le bilan
    /// d'un grand vide terminé. Sans effet sur un grand vide qui tourne, et
    /// quand il n'y a rien.
    pub fn cancel_scan(&self) {
        let mut activity = self.activity();
        match &*activity {
            Activity::Clean(clean) if still_cleaning(clean) => return,
            Activity::Scan(scan) => scan.control().cancel(),
            Activity::Idle | Activity::Clean(_) => {}
        }
        *activity = Activity::Idle;
    }

    /// Le dernier rapport publié : partiel tant que les doublons se cherchent.
    /// Il n'y en a plus dès qu'un grand vide a démarré.
    pub fn report(&self) -> Option<Arc<Report>> {
        match &*self.activity() {
            Activity::Scan(scan) => scan.control().latest_report(),
            Activity::Idle | Activity::Clean(_) => None,
        }
    }

    /// Ce que l'écran de confirmation doit dire avant un grand vide.
    pub fn clean_preview(&self) -> Result<CleanPreview, CleanRefusal> {
        clean::preview(self.environment.as_ref(), self.report().as_deref())
    }

    /// Lance le grand vide demandé, à partir du rapport gardé ici.
    ///
    /// `excluded` : les « dossiers jamais touchés » en vigueur. `journal` : où
    /// noter l'avancement. Refusée, la demande ne touche à rien et les
    /// résultats restent. Acceptée, elle les fait oublier.
    pub fn start_clean(
        &self,
        request: CleanRequest,
        excluded: Vec<PathBuf>,
        journal: Option<JournalStore>,
    ) -> Result<(), SessionError> {
        let mut activity = self.activity();
        let report = match &*activity {
            Activity::Clean(clean) if still_cleaning(clean) => {
                return Err(SessionError::CleanRunning);
            }
            Activity::Scan(scan) => scan.control().latest_report(),
            Activity::Idle | Activity::Clean(_) => None,
        };
        let report = report.ok_or(SessionError::CleanRefused(CleanRefusal::NoResults))?;
        let clean = clean::start(Arc::clone(&self.environment), report, request, excluded, journal)
            .map_err(|error| match error {
                CleanStartError::Refused(refusal) => SessionError::CleanRefused(refusal),
                CleanStartError::Thread(error) => SessionError::Thread(error),
            })?;
        *activity = Activity::Clean(clean);
        Ok(())
    }

    /// Demande l'arrêt du grand vide en cours. Le fichier en cours est mené à
    /// son terme, puis le bilan partiel arrive. Sans effet s'il n'y en a pas.
    pub fn cancel_clean(&self) {
        if let Activity::Clean(clean) = &*self.activity() {
            clean.control().cancel();
        }
    }

    /// Vrai si un grand vide tourne.
    pub fn is_cleaning(&self) -> bool {
        matches!(&*self.activity(), Activity::Clean(clean) if still_cleaning(clean))
    }

    /// À appeler quand Shinku se ferme : arrête ce qui tourne. Un grand vide
    /// a `patience` pour finir son fichier en cours et noter son bilan au
    /// journal ; au-delà, Shinku se ferme quand même, et le dira au prochain
    /// lancement.
    pub fn shutdown(&self, patience: Duration) {
        let activity = self.activity();
        match &*activity {
            Activity::Idle => {}
            Activity::Scan(scan) => scan.control().cancel(),
            Activity::Clean(clean) => {
                clean.control().cancel();
                let deadline = Instant::now() + patience;
                while !clean.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    /// Ce que l'interface doit afficher maintenant.
    pub fn view(&self) -> SessionView {
        let activity = self.activity();
        let scan = match &*activity {
            Activity::Idle => return idle_view(SessionState::Idle, None),
            Activity::Clean(clean) => {
                // L'ordre compte. D'abord si le fil a fini : s'il a fini, ce
                // qu'on lit ensuite est définitif. Puis le bilan, publié en
                // dernier : s'il est là, l'avancement lu après lui est arrivé
                // au bout.
                let finished = clean.is_finished();
                let control = clean.control();
                let report = control.report();
                let progress = control.progress();
                let state = match (&report, finished) {
                    (Some(_), _) => SessionState::Cleaned,
                    (None, false) => SessionState::Cleaning,
                    // Le fil du grand vide s'est arrêté sans conclure.
                    (None, true) => SessionState::Failed,
                };
                let report = report.map(|report| (*report).clone());
                let request = clean.request();
                return idle_view(state, Some(CleanView { request, progress, report }));
            }
            Activity::Scan(scan) => scan,
        };
        let control = scan.control();
        // L'ordre compte : on regarde d'abord si le fil d'analyse a fini. S'il
        // a fini, l'avancement et le rapport lus ensuite sont définitifs.
        let finished = scan.is_finished();
        let progress = control.progress();
        let report = control.latest_report();
        let state = if !finished {
            SessionState::Scanning
        } else {
            match (progress.phase, &report) {
                (Phase::Done, Some(report)) if report.complete => SessionState::Results,
                (Phase::Cancelled, _) => SessionState::Idle,
                // Le fil d'analyse s'est arrêté sans conclure.
                _ => SessionState::Failed,
            }
        };
        let skipped = report.as_ref().map_or_else(SkippedCounts::default, |report| {
            let skipped = &report.skipped;
            SkippedCounts {
                folders: skipped.denied_folders
                    + skipped.unreadable_folders
                    + skipped.replaced_folders,
                refused_roots: skipped.refused_roots.len() as u64,
            }
        });
        SessionView {
            state,
            progress,
            summary: report.as_ref().map(|report| report.summary()),
            skipped,
            stale: report.as_ref().is_some_and(|report| report.is_stale(self.environment.now())),
            clean: None,
        }
    }
}

/// La vue d'une session sans analyse : au repos, ou occupée par un grand vide.
fn idle_view(state: SessionState, clean: Option<CleanView>) -> SessionView {
    SessionView {
        state,
        progress: idle_progress(),
        summary: None,
        skipped: SkippedCounts::default(),
        stale: false,
        clean,
    }
}

fn idle_progress() -> ProgressSnapshot {
    ProgressSnapshot {
        phase: Phase::Preparing,
        folders: 0,
        files: 0,
        found_bytes: 0,
        work_done: 0,
        work_total: 0,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant, SystemTime};

    use super::*;
    use crate::clean::CleanPhase;
    use crate::clean::journal::RunState;
    use crate::clean::report::{CleanOutcome, StepState};
    use crate::fs::to_internal_path;
    use crate::hash::MemoryCache;
    use crate::model::{ModuleId, Timestamp, Trigger};
    use crate::platform::{KnownFolder, RecycleBin, StaticEnvironment, Volume};
    use crate::scan::report::CategoryState;
    use crate::testing::SlowBin;

    /// Un petit PC : un fichier temporaire ancien, deux photos identiques, et
    /// une corbeille qui contient déjà trois éléments.
    fn pc() -> (tempfile::TempDir, StaticEnvironment, PathBuf) {
        let workspace = tempfile::tempdir().unwrap();
        let root = to_internal_path(&fs::canonicalize(workspace.path()).unwrap()).unwrap();
        let profile = root.join("Users").join("a");
        let local = profile.join("AppData").join("Local");
        fs::create_dir_all(local.join("Temp")).unwrap();
        fs::create_dir_all(profile.join("Documents")).unwrap();
        fs::create_dir_all(profile.join("Pictures")).unwrap();
        fs::create_dir_all(root.join("$Recycle.Bin")).unwrap();
        fs::write(local.join("Temp").join("vieux.tmp"), [7u8; 9000]).unwrap();
        fs::write(profile.join("Documents").join("photo.jpg"), [3u8; 8000]).unwrap();
        fs::write(profile.join("Pictures").join("photo.jpg"), [3u8; 8000]).unwrap();
        let month = Duration::from_secs(30 * 24 * 3600);
        let environment = StaticEnvironment::new()
            .with_now(Timestamp::from_system_time(SystemTime::now() + month))
            .with_parallelism(2)
            .with_volume(Volume {
                root: root.clone(),
                label: String::new(),
                file_system: "NTFS".to_owned(),
                total: 1_000_000,
                free: 400_000,
                fixed: true,
                seek_penalty: Some(false),
                system: true,
            })
            .with_folder(KnownFolder::Profile, profile.clone())
            .with_folder(KnownFolder::Documents, profile.join("Documents"))
            .with_folder(KnownFolder::Pictures, profile.join("Pictures"))
            .with_folder(KnownFolder::LocalAppData, local.clone())
            .with_folder(KnownFolder::Temp, local.join("Temp"))
            .with_recycle_bin(RecycleBin { bytes: 4096, items: 3 })
            .with_recycle_folder(root.join("$Recycle.Bin"));
        (workspace, environment, profile)
    }

    /// Le fichier temporaire du petit PC.
    fn old_temp_file(profile: &std::path::Path) -> PathBuf {
        profile.join("AppData").join("Local").join("Temp").join("vieux.tmp")
    }

    /// Les deux photos identiques du petit PC.
    fn photos(profile: &std::path::Path) -> [PathBuf; 2] {
        ["Documents", "Pictures"].map(|folder| profile.join(folder).join("photo.jpg"))
    }

    /// Une demande qui ne coche rien, pour l'analyse que la session garde.
    fn nothing(session: &Session) -> CleanRequest {
        let analysis = session.report().expect("le rapport").started.unix_millis();
        CleanRequest { analysis, recycle_bin: false, junk_files: false, duplicates: false }
    }

    fn scanned(environment: impl Environment + 'static) -> Session {
        let session = Session::new(Arc::new(environment), Arc::new(MemoryCache::new()));
        session.start_scan(options()).unwrap();
        wait_for(&session, SessionState::Results);
        session
    }

    fn options() -> ScanOptions {
        let mut options = ScanOptions::new(Trigger::Button);
        options.duplicate_min_size = 4096;
        options
    }

    fn wait_for(session: &Session, wanted: SessionState) -> SessionView {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let view = session.view();
            if view.state == wanted {
                return view;
            }
            assert!(Instant::now() < deadline, "état attendu : {wanted:?}, vu : {:?}", view.state);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_new_session_is_idle() {
        let (_workspace, environment, _) = pc();
        let session = Session::new(Arc::new(environment), Arc::new(MemoryCache::new()));
        let view = session.view();
        assert_eq!(view.state, SessionState::Idle);
        assert!(view.summary.is_none());
        assert!(session.report().is_none());
        // Arrêter quand rien ne tourne ne fait rien.
        session.cancel_scan();
        assert_eq!(session.view().state, SessionState::Idle);
    }

    #[test]
    fn a_scan_ends_with_results() {
        let (_workspace, environment, _) = pc();
        let session = Session::new(Arc::new(environment), Arc::new(MemoryCache::new()));
        session.start_scan(options()).unwrap();
        let view = wait_for(&session, SessionState::Results);
        let summary = view.summary.expect("un bilan");
        assert!(summary.complete);
        assert!(!view.stale);
        assert_eq!(view.skipped, SkippedCounts::default());
        let by_module = |module: ModuleId| {
            *summary.categories.iter().find(|category| category.module == module).unwrap()
        };
        assert_eq!(by_module(ModuleId::JunkFiles).items, 1);
        assert_eq!(by_module(ModuleId::Duplicates).items, 1);
        assert_eq!(by_module(ModuleId::Duplicates).state, CategoryState::Ready);
        assert_eq!(view.progress.phase, Phase::Done);
        let report = session.report().expect("le rapport");
        assert_eq!(report.duplicates.groups.len(), 1);
    }

    #[test]
    fn cancelling_forgets_the_results() {
        let (_workspace, environment, _) = pc();
        let session = Session::new(Arc::new(environment), Arc::new(MemoryCache::new()));
        session.start_scan(options()).unwrap();
        wait_for(&session, SessionState::Results);
        session.cancel_scan();
        let view = session.view();
        assert_eq!(view.state, SessionState::Idle);
        assert!(view.summary.is_none());
        assert!(session.report().is_none());
    }

    #[test]
    fn a_new_scan_replaces_the_previous_one() {
        let (_workspace, environment, profile) = pc();
        let session = Session::new(Arc::new(environment), Arc::new(MemoryCache::new()));
        session.start_scan(options()).unwrap();
        wait_for(&session, SessionState::Results);
        // Entre deux analyses, une copie disparaît.
        fs::remove_file(profile.join("Pictures").join("photo.jpg")).unwrap();
        session.start_scan(options()).unwrap();
        let view = wait_for(&session, SessionState::Results);
        assert!(view.summary.is_some());
        assert!(session.report().unwrap().duplicates.groups.is_empty());
    }

    #[test]
    fn old_results_are_flagged_as_stale() {
        let (_workspace, environment, _) = pc();
        let cache: Arc<dyn DigestCache> = Arc::new(MemoryCache::new());
        let session = Session::new(Arc::new(environment.clone()), Arc::clone(&cache));
        session.start_scan(options()).unwrap();
        let view = wait_for(&session, SessionState::Results);
        assert!(!view.stale);
        let finished = view.summary.unwrap().finished;

        // La même analyse, relue onze minutes plus tard.
        let later =
            environment.with_now(Timestamp::from_ticks(finished.ticks() + 11 * 60 * 10_000_000));
        let report = session.report().unwrap();
        assert!(report.is_stale(later.now()));
    }

    #[test]
    fn the_view_travels_as_plain_json() {
        let (_workspace, environment, _) = pc();
        let session = Session::new(Arc::new(environment), Arc::new(MemoryCache::new()));
        let idle = serde_json::to_value(session.view()).unwrap();
        assert_eq!(idle["state"], "idle");
        assert_eq!(idle["progress"]["phase"], "preparing");
        assert_eq!(idle["summary"], serde_json::Value::Null);
        session.start_scan(options()).unwrap();
        let view = wait_for(&session, SessionState::Results);
        let json = serde_json::to_value(view).unwrap();
        assert_eq!(json["state"], "results");
        assert_eq!(json["skipped"]["folders"], 0);
        assert_eq!(json["summary"]["categories"][1]["module"], "junkFiles");
        assert_eq!(json["summary"]["categories"][1]["lot"], "auto");
        assert_eq!(json["summary"]["categories"][2]["module"], "duplicates");
        assert_eq!(json["summary"]["categories"][2]["lot"], "optional");
        assert_eq!(json["clean"], serde_json::Value::Null);
    }

    #[test]
    fn a_clean_follows_the_scan_and_ends_with_its_report() {
        let (_workspace, environment, profile) = pc();
        let session = Session::new(Arc::new(environment.clone()), Arc::new(MemoryCache::new()));
        // Sans résultats, il n'y a rien à annoncer.
        assert_eq!(session.clean_preview().unwrap_err(), CleanRefusal::NoResults);
        session.start_scan(options()).unwrap();
        wait_for(&session, SessionState::Results);

        let preview = session.clean_preview().unwrap();
        assert_eq!((preview.junk.files, preview.duplicates.copies), (1, 1));
        assert_eq!(preview.recycle_bin, Some(RecycleBin { bytes: 4096, items: 3 }));
        let request = CleanRequest { junk_files: true, duplicates: true, ..nothing(&session) };
        assert_eq!(preview.analysis, request.analysis);

        session.start_clean(request, Vec::new(), None).unwrap();
        // Dès le départ, les résultats de l'analyse ne sont plus proposés :
        // ils décrivent un disque qui change.
        assert!(session.report().is_none());
        let view = wait_for(&session, SessionState::Cleaned);
        assert!(view.summary.is_none());
        let clean = view.clean.expect("le grand vide");
        assert_eq!(clean.progress.phase, CleanPhase::Done);
        let report = clean.report.expect("le bilan");
        assert_eq!(report.outcome, CleanOutcome::Completed);
        assert_eq!(report.recycle_bin.state, StepState::NotRequested);
        assert_eq!((report.junk.state, report.junk.files), (StepState::Done, 1));
        assert_eq!((report.duplicates.state, report.duplicates.copies), (StepState::Done, 1));

        assert!(!old_temp_file(&profile).exists());
        // Un exemplaire de la photo est resté à sa place ; l'autre attend dans
        // la corbeille, que personne n'a vidée.
        assert_eq!(photos(&profile).iter().filter(|photo| photo.exists()).count(), 1);
        assert_eq!(environment.recycle_bin(), Some(RecycleBin { bytes: 4096 + 8000, items: 4 }));

        // Le bilan reste affiché tant qu'on ne le referme pas.
        assert!(!session.is_cleaning());
        session.cancel_clean();
        assert_eq!(session.view().state, SessionState::Cleaned);
        session.cancel_scan();
        let view = session.view();
        assert_eq!(view.state, SessionState::Idle);
        assert!(view.clean.is_none());
    }

    #[test]
    fn a_refused_clean_touches_nothing_and_keeps_the_results() {
        let (_workspace, environment, profile) = pc();
        let session = Session::new(Arc::new(environment), Arc::new(MemoryCache::new()));
        let refused = |request: CleanRequest| match session.start_clean(request, Vec::new(), None) {
            Err(SessionError::CleanRefused(refusal)) => refusal,
            other => panic!("refus attendu : {other:?}"),
        };
        let everything =
            CleanRequest { analysis: 0, recycle_bin: true, junk_files: true, duplicates: true };
        // Sans analyse, il n'y a rien à nettoyer.
        assert_eq!(refused(everything), CleanRefusal::NoResults);
        assert_eq!(session.view().state, SessionState::Idle);

        session.start_scan(options()).unwrap();
        wait_for(&session, SessionState::Results);
        // La demande porte sur une autre analyse que celle qui est affichée.
        let shown = nothing(&session).analysis;
        assert_eq!(
            refused(CleanRequest { analysis: shown + 1, ..everything }),
            CleanRefusal::OtherAnalysis
        );
        // Rien n'est coché.
        assert_eq!(refused(nothing(&session)), CleanRefusal::NothingToClean);

        let view = session.view();
        assert_eq!(view.state, SessionState::Results);
        assert!(view.clean.is_none());
        assert!(session.report().is_some());
        assert!(old_temp_file(&profile).exists());
        assert!(photos(&profile).iter().all(|photo| photo.exists()));
    }

    #[test]
    fn nothing_else_starts_while_a_clean_runs() {
        let (_workspace, environment, _) = pc();
        let (slow, gate) = SlowBin::new(environment);
        let session = scanned(slow);
        let bin_only = CleanRequest { recycle_bin: true, ..nothing(&session) };
        session.start_clean(bin_only, Vec::new(), None).unwrap();
        gate.wait_until_reached();

        let view = session.view();
        assert_eq!(view.state, SessionState::Cleaning);
        let clean = view.clean.expect("le grand vide en cours");
        assert_eq!(clean.request, bin_only);
        assert_eq!(clean.progress.phase, CleanPhase::RecycleBin);
        assert!(clean.report.is_none());
        assert!(session.is_cleaning());
        assert!(matches!(session.start_scan(options()), Err(SessionError::CleanRunning)));
        assert!(matches!(
            session.start_clean(bin_only, Vec::new(), None),
            Err(SessionError::CleanRunning)
        ));
        // Arrêter « l'analyse » ne referme pas un grand vide qui tourne.
        session.cancel_scan();
        assert_eq!(session.view().state, SessionState::Cleaning);

        gate.open();
        let view = wait_for(&session, SessionState::Cleaned);
        let report = view.clean.and_then(|clean| clean.report).expect("le bilan");
        assert_eq!((report.recycle_bin.state, report.recycle_bin.items), (StepState::Done, 3));
        assert_eq!(report.freed_bytes, 4096);

        // Le grand vide fini, une nouvelle analyse repart et referme son bilan.
        session.start_scan(options()).unwrap();
        let view = wait_for(&session, SessionState::Results);
        assert!(view.clean.is_none());
    }

    #[test]
    fn closing_shinku_stops_the_clean_without_waiting_forever() {
        let (workspace, environment, profile) = pc();
        let journal = JournalStore::at(workspace.path().join("Shinku").join("journal.json"));
        let (slow, gate) = SlowBin::new(environment);
        let session = scanned(slow);
        let request = CleanRequest { recycle_bin: true, junk_files: true, ..nothing(&session) };
        session.start_clean(request, Vec::new(), Some(journal.clone())).unwrap();
        gate.wait_until_reached();

        // La corbeille ne rend pas la main : Shinku se ferme quand même. (Une
        // soupape ouvre la porte au bout de cinq secondes : si l'attente
        // n'était pas bornée, cet essai échouerait au lieu de rester bloqué.)
        let valve = Arc::clone(&gate);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            valve.open();
        });
        let patience = Duration::from_millis(40);
        let clock = Instant::now();
        session.shutdown(patience);
        let waited = clock.elapsed();
        assert!(waited >= patience, "{waited:?}");
        assert!(
            waited < Duration::from_secs(4),
            "fermer Shinku a attendu la corbeille : {waited:?}"
        );
        assert_eq!(session.view().state, SessionState::Cleaning);
        // Le journal en garde la trace : fermé maintenant, Shinku dirait au
        // prochain lancement que ce grand vide a été coupé.
        assert_eq!(journal.runs().last().map(|run| run.state), Some(RunState::Running));

        // Elle finit par la rendre : l'arrêt demandé vaut pour la suite.
        gate.open();
        session.shutdown(Duration::from_secs(30));
        let view = session.view();
        assert_eq!(view.state, SessionState::Cleaned);
        let report = view.clean.and_then(|clean| clean.report).expect("le bilan");
        assert_eq!(report.outcome, CleanOutcome::Cancelled);
        assert_eq!(report.recycle_bin.state, StepState::Done);
        assert_eq!((report.junk.state, report.junk.files), (StepState::Stopped, 0));
        assert!(old_temp_file(&profile).exists());
        let noted = journal.runs();
        assert_eq!(noted.len(), 1);
        assert_eq!((noted[0].state, noted[0].freed_bytes), (RunState::Cancelled, 4096));

        // Fermer Shinku quand rien ne tourne ne fait rien.
        session.cancel_scan();
        session.shutdown(Duration::from_secs(30));
        assert_eq!(session.view().state, SessionState::Idle);
    }

    #[test]
    fn closing_shinku_stops_a_running_scan() {
        let (_workspace, environment, _) = pc();
        let session = Session::new(Arc::new(environment), Arc::new(MemoryCache::new()));
        session.start_scan(options()).unwrap();
        session.shutdown(Duration::from_secs(30));
        // Arrêtée en route, ou déjà terminée : dans les deux cas plus rien ne tourne.
        let deadline = Instant::now() + Duration::from_secs(30);
        while session.view().state == SessionState::Scanning {
            assert!(Instant::now() < deadline, "l'analyse tourne encore");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_clean_that_breaks_down_does_not_block_the_session() {
        let (_workspace, environment, _) = pc();
        let (mut slow, _gate) = SlowBin::new(environment);
        slow.breaks_down = true;
        let session = scanned(slow);
        let bin_only = CleanRequest { recycle_bin: true, ..nothing(&session) };
        session.start_clean(bin_only, Vec::new(), None).unwrap();

        let view = wait_for(&session, SessionState::Failed);
        // Pas de bilan, mais ce que le grand vide avait compté reste lisible.
        let clean = view.clean.expect("le grand vide");
        assert!(clean.report.is_none());
        assert_eq!(clean.progress.phase, CleanPhase::RecycleBin);
        assert!(!session.is_cleaning());
        session.start_scan(options()).unwrap();
        wait_for(&session, SessionState::Results);
    }

    #[test]
    fn the_clean_view_travels_as_plain_json() {
        let (_workspace, environment, _) = pc();
        let session = scanned(environment);
        let request = CleanRequest { recycle_bin: true, junk_files: true, ..nothing(&session) };
        session.start_clean(request, Vec::new(), None).unwrap();
        let view = wait_for(&session, SessionState::Cleaned);
        let json = serde_json::to_value(view).unwrap();
        assert_eq!(json["state"], "cleaned");
        assert_eq!(json["summary"], serde_json::Value::Null);
        assert_eq!(json["clean"]["progress"]["phase"], "done");
        assert_eq!(json["clean"]["request"]["junkFiles"], true);
        assert_eq!(json["clean"]["request"]["duplicates"], false);
        let report = &json["clean"]["report"];
        let junk_bytes = report["junk"]["bytes"].as_u64().expect("la place rendue");
        assert!(junk_bytes >= 9000, "{junk_bytes}");
        assert_eq!(json["clean"]["progress"]["freedBytes"], 4096 + junk_bytes);
        assert_eq!(report["outcome"], "completed");
        assert_eq!(report["recycleBin"]["state"], "done");
        assert_eq!(report["recycleBin"]["items"], 3);
        assert_eq!(report["junk"]["state"], "done");
        assert_eq!(report["junk"]["files"], 1);
        assert_eq!(report["duplicates"]["state"], "notRequested");
        assert_eq!(report["recoverableBytes"], 0);
        assert_eq!(report["skipped"]["inUse"], 0);
    }
}
