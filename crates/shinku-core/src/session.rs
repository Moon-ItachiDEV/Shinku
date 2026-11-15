//! La session : ce que la fenêtre de Shinku suit du début à la fin.
//!
//! Une session lance une analyse, la suit, l'arrête, et garde son rapport.
//! L'interface n'en reçoit qu'une vue ([`SessionView`]) : un état, un
//! avancement et des totaux. Les listes se demandent ensuite page par page,
//! au rapport gardé ici.
//!
//! Une seule analyse tourne à la fois. En lancer une nouvelle arrête la
//! précédente : ses résultats partiels sont jetés, ses empreintes restent
//! dans le cache.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

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
    /// L'analyse s'est arrêtée sur une erreur inattendue.
    Failed,
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
}

/// Une session de Shinku.
#[derive(Debug)]
pub struct Session {
    environment: Arc<dyn Environment>,
    cache: Arc<dyn DigestCache>,
    scan: Mutex<Option<ScanHandle>>,
}

impl Session {
    /// Une session sur cette machine, avec ce cache d'empreintes.
    pub fn new(environment: Arc<dyn Environment>, cache: Arc<dyn DigestCache>) -> Self {
        Self { environment, cache, scan: Mutex::new(None) }
    }

    /// La machine de cette session.
    pub fn environment(&self) -> &Arc<dyn Environment> {
        &self.environment
    }

    /// Lance une analyse. Celle qui tournait, s'il y en avait une, est arrêtée.
    pub fn start_scan(&self, options: ScanOptions) -> io::Result<()> {
        let mut scan = self.scan.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = scan.take() {
            previous.control().cancel();
        }
        *scan = Some(start(Arc::clone(&self.environment), Arc::clone(&self.cache), options)?);
        Ok(())
    }

    /// Arrête l'analyse en cours et oublie ses résultats. Sans effet s'il n'y
    /// en a pas.
    pub fn cancel_scan(&self) {
        let mut scan = self.scan.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = scan.take() {
            previous.control().cancel();
        }
    }

    /// Le dernier rapport publié : partiel tant que les doublons se cherchent.
    pub fn report(&self) -> Option<Arc<Report>> {
        let scan = self.scan.lock().unwrap_or_else(PoisonError::into_inner);
        scan.as_ref().and_then(|scan| scan.control().latest_report())
    }

    /// Ce que l'interface doit afficher maintenant.
    pub fn view(&self) -> SessionView {
        let scan = self.scan.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(scan) = scan.as_ref() else {
            return SessionView {
                state: SessionState::Idle,
                progress: idle_progress(),
                summary: None,
                skipped: SkippedCounts::default(),
                stale: false,
            };
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
        }
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
    use crate::fs::to_internal_path;
    use crate::hash::MemoryCache;
    use crate::model::{ModuleId, Timestamp, Trigger};
    use crate::platform::{KnownFolder, StaticEnvironment, Volume};
    use crate::scan::report::CategoryState;

    /// Un petit PC : un fichier temporaire ancien et deux photos identiques.
    fn pc() -> (tempfile::TempDir, StaticEnvironment, PathBuf) {
        let workspace = tempfile::tempdir().unwrap();
        let root = to_internal_path(&fs::canonicalize(workspace.path()).unwrap()).unwrap();
        let profile = root.join("Users").join("a");
        let local = profile.join("AppData").join("Local");
        fs::create_dir_all(local.join("Temp")).unwrap();
        fs::create_dir_all(profile.join("Documents")).unwrap();
        fs::create_dir_all(profile.join("Pictures")).unwrap();
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
            .with_folder(KnownFolder::Temp, local.join("Temp"));
        (workspace, environment, profile)
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
    }
}
