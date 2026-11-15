//! L'analyse : la première phase du grand vide, en lecture seule.
//!
//! Une analyse enchaîne quatre étapes :
//!
//! 1. le plan : règles, exclusions et interdits rangés dans un arbre ;
//! 2. le parcours : une seule lecture du disque sert les fichiers inutiles,
//!    les gros fichiers et les candidats aux doublons ;
//! 3. un premier rapport, publié tout de suite ;
//! 4. les doublons, dont la durée dépend du volume à lire : le rapport est
//!    complété quand ils sont prêts.
//!
//! Rien n'est écrit sur le disque de l'utilisateur. Annuler jette les
//! résultats partiels ; les empreintes déjà calculées restent dans le cache.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Instant;

use serde::Serialize;

use crate::hash::DigestCache;
use crate::model::Trigger;
use crate::platform::Environment;
use crate::rules::JunkCategory;
use crate::settings::Settings;
use crate::walk::{Root, WalkOptions, walk};

mod duplicates;
mod index;
mod plan;
pub mod report;

use duplicates::Finder;
use index::{Index, Indexed, RuleTotals};
use plan::Plan;
use report::{
    DuplicatesReport, DuplicatesState, JunkCategoryReport, JunkLine, JunkReport, LargeFilesReport,
    Report, Skipped, Stats,
};

/// Seuil des gros fichiers : 3 Go, comptés comme l'Explorateur (1 Go = 1024 Mo).
pub const DEFAULT_LARGE_THRESHOLD: u64 = 3 * 1024 * 1024 * 1024;
/// En dessous de cette taille, un fichier n'est pas comparé : le gain ne vaut
/// pas le risque de toucher à un petit fichier qu'une app duplique exprès.
pub const DEFAULT_DUPLICATE_MIN_SIZE: u64 = 1024 * 1024;

/// Ce qu'une analyse mesure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modules {
    /// Fichiers inutiles.
    pub junk: bool,
    /// Doublons.
    pub duplicates: bool,
    /// Gros fichiers.
    pub large_files: bool,
    /// Corbeille.
    pub recycle_bin: bool,
}

impl Modules {
    /// Tout ce que le grand vide mesure.
    pub const ALL: Self =
        Self { junk: true, duplicates: true, large_files: true, recycle_bin: true };
}

/// Réglages d'une analyse.
#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Ce qui déclenche l'analyse.
    pub trigger: Trigger,
    /// « Dossiers jamais touchés » : chemins absolus que l'analyse ignore.
    pub excluded: Vec<PathBuf>,
    /// Place sur le disque à partir de laquelle un fichier est « gros ».
    pub large_threshold: u64,
    /// Taille minimale d'un fichier pour chercher ses doublons.
    pub duplicate_min_size: u64,
    /// Ce qu'on mesure.
    pub modules: Modules,
    /// Lire en priorité basse : pour les analyses lancées sans l'utilisateur.
    pub background: bool,
}

impl ScanOptions {
    /// Réglages par défaut pour ce déclencheur. Une analyse automatique
    /// passe après l'utilisateur.
    pub fn new(trigger: Trigger) -> Self {
        Self {
            trigger,
            excluded: Vec::new(),
            large_threshold: DEFAULT_LARGE_THRESHOLD,
            duplicate_min_size: DEFAULT_DUPLICATE_MIN_SIZE,
            modules: Modules::ALL,
            background: trigger == Trigger::Automatic,
        }
    }

    /// Applique les réglages de l'utilisateur : ses « dossiers jamais touchés ».
    pub fn with_settings(mut self, settings: &Settings) -> Self {
        self.excluded = settings.excluded_folders.clone();
        self
    }
}

/// Étape d'une analyse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Préparation du plan.
    Preparing,
    /// Parcours du disque.
    Walking,
    /// Doublons : comparaison des extraits.
    DuplicatesQuick,
    /// Doublons : lecture complète des fichiers qui se ressemblent.
    DuplicatesFull,
    /// Terminée.
    Done,
    /// Annulée : il n'y a pas de résultats.
    Cancelled,
}

impl Phase {
    const ALL: [Self; 6] = [
        Self::Preparing,
        Self::Walking,
        Self::DuplicatesQuick,
        Self::DuplicatesFull,
        Self::Done,
        Self::Cancelled,
    ];
}

/// Avancement d'une analyse, mis à jour sans verrou par les fils de lecture.
#[derive(Debug, Default)]
pub struct Progress {
    phase: AtomicU8,
    pub(crate) folders: AtomicU64,
    pub(crate) files: AtomicU64,
    pub(crate) found_bytes: AtomicU64,
    work_done: AtomicU64,
    work_total: AtomicU64,
}

impl Progress {
    fn set_phase(&self, phase: Phase) {
        self.phase.store(phase as u8, Ordering::Relaxed);
    }

    /// Commence une étape dont on connaît le volume de travail, en octets.
    pub(crate) fn begin(&self, phase: Phase, total: u64) {
        self.work_done.store(0, Ordering::Relaxed);
        self.work_total.store(total, Ordering::Relaxed);
        self.set_phase(phase);
    }

    pub(crate) fn advance(&self, bytes: u64) {
        self.work_done.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Photographie de l'avancement.
    pub fn snapshot(&self) -> ProgressSnapshot {
        let phase = Phase::ALL
            .get(self.phase.load(Ordering::Relaxed) as usize)
            .copied()
            .unwrap_or(Phase::Preparing);
        let total = self.work_total.load(Ordering::Relaxed);
        ProgressSnapshot {
            phase,
            folders: self.folders.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
            found_bytes: self.found_bytes.load(Ordering::Relaxed),
            work_done: self.work_done.load(Ordering::Relaxed).min(total),
            work_total: total,
        }
    }
}

/// Avancement d'une analyse à un instant donné.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressSnapshot {
    /// Étape en cours.
    pub phase: Phase,
    /// Dossiers lus.
    pub folders: u64,
    /// Fichiers vus.
    pub files: u64,
    /// Fichiers inutiles déjà trouvés, en octets (chiffre provisoire).
    pub found_bytes: u64,
    /// Doublons : octets déjà comparés dans l'étape en cours.
    pub work_done: u64,
    /// Doublons : octets à comparer dans l'étape en cours.
    pub work_total: u64,
}

/// Ce qui relie une analyse en cours à celui qui l'a lancée : avancement,
/// annulation, dernier rapport publié.
#[derive(Debug, Default)]
pub struct Control {
    progress: Progress,
    cancel: AtomicBool,
    latest: Mutex<Option<Arc<Report>>>,
}

impl Control {
    /// Crée le contrôle d'une nouvelle analyse.
    pub fn new() -> Self {
        Self::default()
    }

    /// Demande l'arrêt. L'analyse s'arrête dès que les lectures en cours se
    /// terminent, et ses résultats partiels sont jetés.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Vrai si l'arrêt a été demandé.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Avancement actuel.
    pub fn progress(&self) -> ProgressSnapshot {
        self.progress.snapshot()
    }

    /// Dernier rapport publié : d'abord sans les doublons, puis complet.
    pub fn latest_report(&self) -> Option<Arc<Report>> {
        self.latest.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn publish(&self, report: Option<Arc<Report>>) {
        *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = report;
    }
}

/// Mène une analyse jusqu'au bout, dans le fil d'exécution appelant.
///
/// Rend le rapport complet, ou `None` si l'analyse a été annulée.
pub fn run(
    environment: &dyn Environment,
    cache: &dyn DigestCache,
    options: &ScanOptions,
    control: &Control,
) -> Option<Arc<Report>> {
    let outcome = analyse(environment, cache, options, control);
    if outcome.is_none() {
        // Annulée : rien de partiel ne doit rester visible.
        control.publish(None);
        control.progress.set_phase(Phase::Cancelled);
    } else {
        control.progress.set_phase(Phase::Done);
    }
    outcome
}

fn analyse(
    environment: &dyn Environment,
    cache: &dyn DigestCache,
    options: &ScanOptions,
    control: &Control,
) -> Option<Arc<Report>> {
    let started = environment.now();
    let clock = Instant::now();
    let cancelled = || control.is_cancelled();

    control.progress.set_phase(Phase::Preparing);
    let plan = Plan::build(environment, options);
    if cancelled() {
        return None;
    }

    control.progress.set_phase(Phase::Walking);
    let mut indexed = index_disks(&plan, options.background, control)?;
    let walk_ms = clock.elapsed().as_millis() as u64;

    let recycle_bin = if options.modules.recycle_bin { environment.recycle_bin() } else { None };
    let candidates = std::mem::take(&mut indexed.candidates);
    let large = std::mem::take(&mut indexed.large);
    let mut report = Report {
        trigger: options.trigger,
        started,
        finished: environment.now(),
        complete: !options.modules.duplicates,
        volumes: plan.volumes.clone(),
        junk: junk_report(&plan, &indexed.junk),
        large_files: LargeFilesReport {
            threshold: plan.large_threshold,
            count: large.count,
            bytes: large.bytes,
            files: large.into_sorted(),
        },
        duplicates: DuplicatesReport::empty(if options.modules.duplicates {
            DuplicatesState::Pending
        } else {
            DuplicatesState::Off
        }),
        recycle_bin,
        skipped: Skipped {
            denied_folders: indexed.denied,
            denied_examples: std::mem::take(&mut indexed.denied_examples),
            unreadable_folders: indexed.unreadable,
            replaced_folders: indexed.replaced,
            refused_roots: plan.refused.clone(),
        },
        stats: Stats { folders: indexed.folders, files: indexed.files, walk_ms, total_ms: walk_ms },
    };

    if options.modules.duplicates {
        // Le reste est prêt : on le montre sans attendre les doublons.
        control.publish(Some(Arc::new(report.clone())));
        let threads_for = |volume: u64| plan.threads_for(volume);
        let finder = Finder {
            cache,
            progress: &control.progress,
            cancel: &control.cancel,
            threads_for: &threads_for,
        };
        report.duplicates = finder.find(candidates)?;
        report.complete = true;
        report.finished = environment.now();
        report.stats.total_ms = clock.elapsed().as_millis() as u64;
    }
    if cancelled() {
        return None;
    }
    let report = Arc::new(report);
    control.publish(Some(Arc::clone(&report)));
    Some(report)
}

/// Parcourt les disques du plan, l'un après l'autre : chacun avec le nombre
/// de lectures en parallèle qui lui convient. `None` si l'analyse est annulée.
fn index_disks(plan: &Plan, background: bool, control: &Control) -> Option<Indexed> {
    // Numérote les éléments jugés en bloc, tous disques confondus.
    let units = AtomicU32::new(0);
    let mut indexed = Indexed::new(plan);
    let mut volume_keys: Vec<u64> = plan.roots.iter().map(|root| root.volume_key).collect();
    volume_keys.sort_unstable();
    volume_keys.dedup();
    for volume_key in volume_keys {
        let index = Index::new(plan, &control.progress, volume_key, &units);
        let on_volume = || plan.roots.iter().filter(|root| root.volume_key == volume_key);
        let roots: Vec<Root<_>> = on_volume()
            .filter_map(|root| {
                Some(Root {
                    path: root.path.clone(),
                    file_id: root.identity.map(|identity| identity.file_id),
                    ctx: index.root_ctx(&root.components)?,
                })
            })
            .collect();
        let walk_options = WalkOptions {
            threads: on_volume().map(|root| root.threads).min().unwrap_or(1),
            background,
        };
        let sinks = walk(roots, &index, walk_options, &control.cancel);
        if control.is_cancelled() {
            return None;
        }
        indexed.absorb(plan, sinks);
    }
    Some(indexed)
}

/// Range les totaux des règles par catégorie, puis par app.
fn junk_report(plan: &Plan, totals: &[RuleTotals]) -> JunkReport {
    let mut report = JunkReport {
        categories: JunkCategory::ALL
            .into_iter()
            .map(|category| JunkCategoryReport {
                category,
                label: category.label(),
                bytes: 0,
                files: 0,
                lines: Vec::new(),
            })
            .collect(),
        ..JunkReport::default()
    };
    for (planned, total) in plan.rules.iter().zip(totals) {
        let rule = planned.rule;
        report.spared_bytes += total.spared_bytes;
        report.spared_files += total.spared_files;
        if total.files == 0 {
            continue;
        }
        report.bytes += total.bytes;
        report.files += total.files;
        let Some(category) =
            report.categories.iter_mut().find(|category| category.category == rule.category)
        else {
            continue;
        };
        category.bytes += total.bytes;
        category.files += total.files;
        // Plusieurs règles d'une même app (ses trois caches) font une seule ligne.
        let position =
            category.lines.iter().position(|line| line.label == rule.label).unwrap_or_else(|| {
                category.lines.push(JunkLine {
                    label: rule.label,
                    bytes: 0,
                    files: 0,
                    rules: Vec::new(),
                    close_first: Vec::new(),
                });
                category.lines.len() - 1
            });
        let line = &mut category.lines[position];
        line.bytes += total.bytes;
        line.files += total.files;
        line.rules.push(rule.id);
        for process in rule.close_first {
            if !line.close_first.contains(process) {
                line.close_first.push(process);
            }
        }
    }
    for category in &mut report.categories {
        category.lines.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.label.cmp(b.label)));
    }
    report
}

/// Une analyse lancée dans son propre fil d'exécution.
#[derive(Debug)]
pub struct ScanHandle {
    control: Arc<Control>,
    thread: Option<JoinHandle<Option<Arc<Report>>>>,
}

/// Lance une analyse sans bloquer l'appelant.
pub fn start(
    environment: Arc<dyn Environment>,
    cache: Arc<dyn DigestCache>,
    options: ScanOptions,
) -> std::io::Result<ScanHandle> {
    let control = Arc::new(Control::new());
    let shared = Arc::clone(&control);
    let thread = std::thread::Builder::new()
        .name("shinku-analyse".to_owned())
        .spawn(move || run(environment.as_ref(), cache.as_ref(), &options, &shared))?;
    Ok(ScanHandle { control, thread: Some(thread) })
}

impl ScanHandle {
    /// Avancement, annulation et dernier rapport de cette analyse.
    pub fn control(&self) -> &Arc<Control> {
        &self.control
    }

    /// Vrai quand l'analyse est terminée ou annulée.
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Attend la fin de l'analyse. `None` si elle a été annulée.
    pub fn wait(mut self) -> Option<Arc<Report>> {
        let thread = self.thread.take()?;
        match thread.join() {
            Ok(report) => report,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}

impl Drop for ScanHandle {
    fn drop(&mut self) {
        // Une analyse que plus personne ne suit n'a pas de raison de continuer.
        if self.thread.is_some() {
            self.control.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::fs::to_internal_path;
    use crate::model::Timestamp;
    use crate::platform::{KnownFolder, StaticEnvironment, Volume};

    #[test]
    fn an_excluded_folder_moved_during_the_scan_stays_protected() {
        let workspace = tempfile::tempdir().unwrap();
        let root = to_internal_path(&fs::canonicalize(workspace.path()).unwrap()).unwrap();
        let profile = root.join("Users").join("a");
        let local = profile.join("AppData").join("Local");
        let temp = local.join("Temp");
        let secret = temp.join("lot").join("secret");
        fs::create_dir_all(&secret).unwrap();
        fs::create_dir_all(temp.join("autre")).unwrap();
        fs::write(secret.join("cle.bin"), [1u8; 5000]).unwrap();
        fs::write(temp.join("lot").join("a.tmp"), [2u8; 5000]).unwrap();
        fs::write(temp.join("autre").join("b.tmp"), [3u8; 5000]).unwrap();
        fs::write(temp.join("c.tmp"), [4u8; 5000]).unwrap();

        // Dans un mois, tous ces fichiers seront vieux.
        let month = Duration::from_secs(30 * 24 * 3600);
        let environment = StaticEnvironment::new()
            .with_now(Timestamp::from_system_time(SystemTime::now() + month))
            .with_volume(Volume {
                root: root.clone(),
                label: String::new(),
                file_system: "NTFS".to_owned(),
                total: 1_000_000,
                free: 500_000,
                fixed: true,
                seek_penalty: Some(false),
                system: true,
            })
            .with_folder(KnownFolder::Profile, profile)
            .with_folder(KnownFolder::LocalAppData, local)
            .with_folder(KnownFolder::Temp, temp.clone());
        let mut options = ScanOptions::new(Trigger::Button);
        options.excluded = vec![secret.clone()];
        let plan = Plan::build(&environment, &options);
        assert!(plan.refused.is_empty(), "{:?}", plan.refused);

        // Entre le plan et le parcours, le dossier exclu change de place : le
        // chemin donné par l'utilisateur ne le désigne plus, son identité si.
        fs::rename(&secret, temp.join("autre").join("secret")).unwrap();

        let indexed = index_disks(&plan, false, &Control::new()).expect("pas d'annulation");
        let rule = plan
            .rules
            .iter()
            .position(|planned| planned.rule.id == "windows.temp")
            .expect("la règle des fichiers temporaires");
        let totals = indexed.junk[rule];
        // « c.tmp » est à nettoyer. « lot » abritait le dossier exclu quand le
        // plan a été fait : on n'y touche pas. « autre » l'abrite maintenant :
        // tout l'élément est épargné, « b.tmp » compris.
        assert_eq!((totals.files, totals.spared_files), (1, 1), "{totals:?}");
    }
}
