//! Le grand vide : la phase qui supprime.
//!
//! Rien de ce qui part ne vient de l'interface. Elle envoie trois cases
//! cochées et la date de l'analyse affichée ([`CleanRequest`]) ; la liste de
//! ce qui est à supprimer vient du rapport que le cœur a gardé, et du disque
//! tel qu'il est au moment d'écrire.
//!
//! L'ordre des étapes est fixe :
//!
//! 1. la corbeille est vidée. C'est la seule étape sans retour : l'interface
//!    la fait toujours confirmer ;
//! 2. les fichiers inutiles sont supprimés directement (module `junk`) : ils
//!    se recréent seuls ;
//! 3. les copies en double partent à la corbeille (module `duplicates`).
//!    Vidée juste avant, elle les garde : elles restent récupérables.
//!
//! Les garde-fous sont les mêmes partout. Des résultats de plus de dix minutes
//! ne servent pas à nettoyer. Chaque fichier est revérifié sur le disque juste
//! avant de partir : s'il a changé, il reste. Un fichier qu'une app utilise
//! reste aussi : rien n'est forcé, rien n'est remis au prochain démarrage de
//! Windows. Arrêter le grand vide laisse finir le fichier en cours, puis rend
//! un bilan partiel : ce qui est parti reste parti.
//!
//! Le bilan ([`report::CleanReport`]) ne mélange jamais ce qui est libéré pour
//! de bon et ce qui attend dans la corbeille.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::model::Timestamp;
use crate::platform::{Environment, RecycleBin};
use crate::scan::report::{DuplicatesState, Report};

mod duplicates;
pub mod journal;
mod junk;
pub mod report;

use journal::{JournalStore, Run, RunState};
use report::{BinStep, CleanOutcome, CleanReport, LeftOpen, StepState};

/// Ce que l'utilisateur a demandé en confirmant « Faire le vide ».
///
/// Trois cases et une date : aucune liste, aucun chemin. Ce qui part est
/// décidé par le cœur.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanRequest {
    /// Début de l'analyse que l'utilisateur a sous les yeux, en millisecondes
    /// depuis 1970. Si le cœur en garde une autre, la demande est refusée.
    pub analysis: i64,
    /// Vider la corbeille.
    pub recycle_bin: bool,
    /// Supprimer les fichiers inutiles.
    pub junk_files: bool,
    /// Envoyer les copies en double à la corbeille. Ce sont des fichiers
    /// personnels : l'interface ne coche jamais cette case d'office.
    pub duplicates: bool,
}

/// Pourquoi un grand vide est refusé avant d'avoir commencé.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanRefusal {
    /// Il n'y a pas de résultats complets : pas d'analyse, ou elle tourne encore.
    NoResults,
    /// Les résultats ont plus de dix minutes : le disque a pu changer.
    StaleResults,
    /// La demande porte sur une autre analyse que celle que le cœur garde.
    OtherAnalysis,
    /// Rien n'est coché, ou ce qui est coché n'a rien à nettoyer.
    NothingToClean,
}

impl fmt::Display for CleanRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoResults => "il n'y a pas de résultats complets",
            Self::StaleResults => "les résultats sont trop anciens",
            Self::OtherAnalysis => "la demande porte sur une autre analyse",
            Self::NothingToClean => "il n'y a rien à nettoyer",
        })
    }
}

impl std::error::Error for CleanRefusal {}

/// Le rapport, s'il est complet : l'analyse est finie, doublons compris.
fn complete(report: Option<&Report>) -> Result<&Report, CleanRefusal> {
    report.filter(|report| report.complete).ok_or(CleanRefusal::NoResults)
}

/// Refuse des résultats trop vieux pour nettoyer sans réanalyser.
fn fresh(environment: &dyn Environment, report: &Report) -> Result<(), CleanRefusal> {
    if report.is_stale(environment.now()) {
        return Err(CleanRefusal::StaleResults);
    }
    Ok(())
}

/// Vrai si l'analyse a trouvé des copies en double à envoyer à la corbeille.
fn has_duplicates(report: &Report) -> bool {
    report.duplicates.state == DuplicatesState::Done && report.duplicates.copies > 0
}

/// Dit si cette demande peut être menée avec ce rapport.
pub fn check(
    environment: &dyn Environment,
    report: Option<&Report>,
    request: &CleanRequest,
) -> Result<(), CleanRefusal> {
    let report = complete(report)?;
    if report.started.unix_millis() != request.analysis {
        return Err(CleanRefusal::OtherAnalysis);
    }
    fresh(environment, report)?;
    let something = (request.junk_files && report.junk.files > 0)
        || (request.duplicates && has_duplicates(report))
        // La corbeille est regardée telle qu'elle est maintenant.
        || (request.recycle_bin && environment.recycle_bin().is_some_and(|bin| bin.items > 0));
    if something { Ok(()) } else { Err(CleanRefusal::NothingToClean) }
}

/// Vrai si l'un de ces programmes tourne. Une machine qui ne sait pas dire ce
/// qui tourne (`running` absent) est traitée comme si tout était ouvert.
fn is_running(running: Option<&[String]>, processes: &[&str]) -> bool {
    match running {
        None => true,
        Some(running) => running
            .iter()
            .any(|name| processes.iter().any(|wanted| name.eq_ignore_ascii_case(wanted))),
    }
}

/// Ce que l'écran de confirmation doit dire avant un grand vide.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanPreview {
    /// Début de l'analyse décrite, à renvoyer dans la demande.
    pub analysis: i64,
    /// La corbeille telle qu'elle est maintenant, si elle peut être mesurée.
    /// La vider est sans retour.
    pub recycle_bin: Option<RecycleBin>,
    /// Les fichiers inutiles.
    pub junk: JunkPreview,
    /// Les copies en double.
    pub duplicates: DuplicatesPreview,
}

/// Ce que le grand vide ferait des fichiers inutiles.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkPreview {
    /// Place que l'analyse a comptée.
    pub bytes: u64,
    /// Nombre de fichiers.
    pub files: u64,
    /// Apps ouvertes en ce moment : leurs fichiers ne seront pas touchés tant
    /// qu'elles le restent.
    pub open_apps: Vec<LeftOpen>,
}

/// Ce que le grand vide ferait des copies en double.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatesPreview {
    /// Place que les copies occupent.
    pub bytes: u64,
    /// Nombre de copies : un exemplaire de chaque groupe reste toujours.
    pub copies: u64,
    /// Copies rangées dans un dossier synchronisé : les envoyer à la
    /// corbeille les retire aussi du service en ligne.
    pub synced_copies: u64,
    /// Copies rangées sur un autre disque que l'exemplaire gardé : c'est
    /// peut-être une sauvegarde voulue.
    pub other_disk_copies: u64,
}

/// Prépare l'écran de confirmation à partir du rapport gardé.
pub fn preview(
    environment: &dyn Environment,
    report: Option<&Report>,
) -> Result<CleanPreview, CleanRefusal> {
    let report = complete(report)?;
    fresh(environment, report)?;

    let running = environment.running_processes();
    let mut open_apps: Vec<LeftOpen> = Vec::new();
    for line in report.junk.categories.iter().flat_map(|category| &category.lines) {
        if line.close_first.is_empty() || !is_running(running.as_deref(), &line.close_first) {
            continue;
        }
        match open_apps.iter_mut().find(|app| app.label == line.label) {
            Some(app) => app.bytes += line.bytes,
            None => open_apps.push(LeftOpen { label: line.label, bytes: line.bytes }),
        }
    }

    let mut duplicates = DuplicatesPreview::default();
    if has_duplicates(report) {
        duplicates.bytes = report.duplicates.bytes;
        duplicates.copies = report.duplicates.copies;
        for group in &report.duplicates.groups {
            let kept = group.files.iter().find(|file| file.keep);
            for copy in group.files.iter().filter(|file| !file.keep) {
                duplicates.synced_copies += u64::from(copy.synced);
                duplicates.other_disk_copies +=
                    u64::from(kept.is_some_and(|kept| kept.volume != copy.volume));
            }
        }
    }

    Ok(CleanPreview {
        analysis: report.started.unix_millis(),
        recycle_bin: environment.recycle_bin(),
        junk: JunkPreview { bytes: report.junk.bytes, files: report.junk.files, open_apps },
        duplicates,
    })
}

/// Étape d'un grand vide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanPhase {
    /// Préparation.
    Preparing,
    /// La corbeille se vide.
    RecycleBin,
    /// Les fichiers inutiles partent.
    JunkFiles,
    /// Les copies en double sont comparées, puis envoyées à la corbeille.
    Duplicates,
    /// Terminé : le bilan est prêt.
    Done,
}

impl CleanPhase {
    const ALL: [Self; 5] =
        [Self::Preparing, Self::RecycleBin, Self::JunkFiles, Self::Duplicates, Self::Done];
}

/// Avancement d'un grand vide, mis à jour sans verrou.
#[derive(Debug, Default)]
pub struct CleanProgress {
    phase: AtomicU8,
    freed_bytes: AtomicU64,
    recycled_bytes: AtomicU64,
    files: AtomicU64,
    skipped_files: AtomicU64,
    work_done: AtomicU64,
    work_total: AtomicU64,
}

impl CleanProgress {
    /// Commence une étape. `total` est son volume de travail, en octets ;
    /// 0 quand on ne le connaît pas.
    pub(crate) fn begin(&self, phase: CleanPhase, total: u64) {
        self.work_done.store(0, Ordering::Relaxed);
        self.work_total.store(total, Ordering::Relaxed);
        self.phase.store(phase as u8, Ordering::Relaxed);
    }

    /// Avance dans l'étape en cours.
    pub(crate) fn advance(&self, bytes: u64) {
        self.work_done.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Des fichiers viennent d'être supprimés pour de bon.
    pub(crate) fn freed(&self, bytes: u64, files: u64) {
        self.freed_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.files.fetch_add(files, Ordering::Relaxed);
    }

    /// Des fichiers viennent de partir à la corbeille.
    pub(crate) fn recycled(&self, bytes: u64, files: u64) {
        self.recycled_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.files.fetch_add(files, Ordering::Relaxed);
    }

    /// Des fichiers viennent d'être laissés en place.
    pub(crate) fn skipped(&self, files: u64) {
        self.skipped_files.fetch_add(files, Ordering::Relaxed);
    }

    /// Photographie de l'avancement.
    pub fn snapshot(&self) -> CleanProgressSnapshot {
        let phase = CleanPhase::ALL
            .get(self.phase.load(Ordering::Relaxed) as usize)
            .copied()
            .unwrap_or(CleanPhase::Preparing);
        let total = self.work_total.load(Ordering::Relaxed);
        CleanProgressSnapshot {
            phase,
            freed_bytes: self.freed_bytes.load(Ordering::Relaxed),
            recycled_bytes: self.recycled_bytes.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
            skipped_files: self.skipped_files.load(Ordering::Relaxed),
            work_done: self.work_done.load(Ordering::Relaxed).min(total),
            work_total: total,
        }
    }
}

/// Avancement d'un grand vide à un instant donné.
///
/// Les chiffres sont ceux du comptage en cours de route : le bilan, à la fin,
/// les rapproche de ce que les disques et la corbeille ont réellement gagné.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanProgressSnapshot {
    /// Étape en cours.
    pub phase: CleanPhase,
    /// Place libérée pour de bon jusqu'ici.
    pub freed_bytes: u64,
    /// Place occupée par ce qui est parti à la corbeille jusqu'ici.
    pub recycled_bytes: u64,
    /// Fichiers supprimés ou envoyés à la corbeille.
    pub files: u64,
    /// Fichiers laissés en place.
    pub skipped_files: u64,
    /// Travail accompli dans l'étape en cours, en octets.
    pub work_done: u64,
    /// Travail de l'étape en cours, en octets. 0 : on ne le connaît pas.
    pub work_total: u64,
}

/// Ce qui relie un grand vide en cours à celui qui l'a lancé : avancement,
/// arrêt, bilan.
#[derive(Debug, Default)]
pub struct CleanControl {
    progress: CleanProgress,
    cancel: AtomicBool,
    report: Mutex<Option<Arc<CleanReport>>>,
}

impl CleanControl {
    /// Crée le contrôle d'un nouveau grand vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Demande l'arrêt. Le fichier en cours est mené à son terme, puis le
    /// grand vide s'arrête et rend un bilan partiel. Ce qui est déjà parti
    /// reste parti.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Vrai si l'arrêt a été demandé.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Avancement actuel.
    pub fn progress(&self) -> CleanProgressSnapshot {
        self.progress.snapshot()
    }

    /// Le bilan, une fois le grand vide terminé ou arrêté.
    pub fn report(&self) -> Option<Arc<CleanReport>> {
        self.report.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub(crate) fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }

    pub(crate) fn progress_counters(&self) -> &CleanProgress {
        &self.progress
    }

    fn publish(&self, report: Arc<CleanReport>) {
        *self.report.lock().unwrap_or_else(PoisonError::into_inner) = Some(report);
    }
}

/// Mène un grand vide jusqu'au bout, dans le fil d'exécution appelant.
///
/// `report` est le rapport de l'analyse affichée ; `excluded`, les « dossiers
/// jamais touchés » en vigueur ; `journal`, où noter l'avancement.
pub fn run(
    environment: &dyn Environment,
    report: &Report,
    request: &CleanRequest,
    excluded: &[PathBuf],
    journal: Option<&JournalStore>,
    control: &CleanControl,
) -> Result<Arc<CleanReport>, CleanRefusal> {
    check(environment, Some(report), request)?;
    Ok(execute(environment, report, request, excluded, journal, control))
}

fn execute(
    environment: &dyn Environment,
    report: &Report,
    request: &CleanRequest,
    excluded: &[PathBuf],
    journal: Option<&JournalStore>,
    control: &CleanControl,
) -> Arc<CleanReport> {
    let started = environment.now();
    let clock = Instant::now();
    let progress = &control.progress;
    progress.begin(CleanPhase::Preparing, 0);
    let mut pacer = Pacer::begin(journal, started);

    // 1. La corbeille, d'abord : les copies envoyées ensuite y resteront.
    let free_before = free_space(environment);
    let mut recycle_bin = BinStep::default();
    if request.recycle_bin {
        if control.is_cancelled() {
            recycle_bin.state = StepState::Stopped;
        } else {
            recycle_bin = empty_bin(environment, progress);
        }
        pacer.note(progress);
    }

    // 2. Les fichiers inutiles : suppression directe.
    let mut junk = junk::Outcome::default();
    if request.junk_files {
        if control.is_cancelled() {
            junk.step.state = StepState::Stopped;
        } else if report.junk.files == 0 {
            junk.step.state = StepState::Done;
        } else {
            progress.begin(CleanPhase::JunkFiles, 0);
            let mut tick = || pacer.tick(progress);
            junk = junk::clean(environment, report.started, excluded, control, &mut tick);
        }
        pacer.note(progress);
    }
    let free_after = free_space(environment);

    // 3. Les copies en double : à la corbeille, jamais supprimées.
    let mut duplicates = duplicates::Outcome::default();
    let bin_before = environment.recycle_bin();
    if request.duplicates {
        if control.is_cancelled() {
            duplicates.step.state = StepState::Stopped;
        } else if !has_duplicates(report) {
            duplicates.step.state = StepState::Done;
        } else {
            progress.begin(CleanPhase::Duplicates, 0);
            let mut tick = || pacer.tick(progress);
            duplicates = duplicates::clean(environment, report, excluded, control, &mut tick);
        }
    }
    let bin_after = environment.recycle_bin();

    // Arrêté par l'utilisateur : une étape s'est arrêtée sans raison à elle.
    let cancelled = recycle_bin.state == StepState::Stopped
        || junk.step.state == StepState::Stopped
        || (duplicates.step.state == StepState::Stopped && duplicates.step.stop.is_none());
    let mut skipped = junk.skipped;
    skipped.merge(duplicates.skipped);
    let report = Arc::new(CleanReport {
        started,
        finished: environment.now(),
        outcome: if cancelled { CleanOutcome::Cancelled } else { CleanOutcome::Completed },
        freed_bytes: freed_for_good(recycle_bin.bytes + junk.step.bytes, free_before, free_after),
        recoverable_bytes: recoverable(
            duplicates.step.bytes,
            duplicates.sent_logical,
            bin_before,
            bin_after,
        ),
        recycle_bin,
        junk: junk.step,
        duplicates: duplicates.step,
        skipped,
        duration_ms: clock.elapsed().as_millis() as u64,
    });
    // Le journal d'abord : si Shinku est fermé maintenant, il saura au
    // prochain lancement que ce grand vide a été conclu. Le bilan en dernier :
    // qui le lit trouve un avancement déjà arrivé au bout.
    pacer.finish(&report);
    progress.begin(CleanPhase::Done, 0);
    control.publish(Arc::clone(&report));
    report
}

/// Vide la corbeille, et dit ce qui en est réellement sorti.
fn empty_bin(environment: &dyn Environment, progress: &CleanProgress) -> BinStep {
    progress.begin(CleanPhase::RecycleBin, 0);
    let Some(before) = environment.recycle_bin() else {
        return BinStep { state: StepState::Failed, bytes: 0, items: 0 };
    };
    if before.items == 0 {
        return BinStep { state: StepState::Done, bytes: 0, items: 0 };
    }
    let emptied = environment.empty_recycle_bin();
    // Ce qui compte n'est pas la réponse de Windows mais ce que la corbeille
    // contient maintenant : elle peut s'être vidée en partie seulement.
    let step = match (emptied, environment.recycle_bin()) {
        (emptied, Some(after)) => BinStep {
            state: if emptied.is_ok() { StepState::Done } else { StepState::Failed },
            bytes: before.bytes.saturating_sub(after.bytes),
            items: before.items.saturating_sub(after.items),
        },
        (Ok(()), None) => {
            BinStep { state: StepState::Done, bytes: before.bytes, items: before.items }
        }
        (Err(_), None) => BinStep { state: StepState::Failed, bytes: 0, items: 0 },
    };
    progress.freed(step.bytes, 0);
    step
}

/// Espace libre sur les disques internes, tous confondus. `None` si la
/// machine n'en montre aucun.
fn free_space(environment: &dyn Environment) -> Option<u64> {
    let volumes = environment.volumes();
    let mut fixed = volumes.iter().filter(|volume| volume.fixed).peekable();
    fixed.peek()?;
    Some(fixed.map(|volume| volume.free).sum())
}

/// Place libérée pour de bon.
///
/// `counted` est ce que Shinku a compté en supprimant. Quand les disques ont
/// pu être mesurés avant et après, et qu'ils ont gagné de la place, le chiffre
/// rendu ne dépasse pas ce gain : un fichier qui existait sous deux noms, ou
/// qu'une app tient encore, ne rend pas sa place en partant.
///
/// Des disques qui n'ont rien gagné du tout ne disent plus rien du nettoyage :
/// un autre programme les remplissait en même temps. Il reste alors le compte.
fn freed_for_good(counted: u64, free_before: Option<u64>, free_after: Option<u64>) -> u64 {
    match (free_before, free_after) {
        (Some(before), Some(after)) if after > before => counted.min(after - before),
        _ => counted,
    }
}

/// Place occupée par ce qui attend réellement dans la corbeille.
///
/// Windows mesure sa corbeille en tailles de fichiers, pas en place occupée :
/// c'est dans cette unité (`sent_logical`) qu'on vérifie qu'elle a grossi de
/// tout ce qu'elle a reçu. Si elle a grossi de moins, c'est qu'elle était
/// pleine et qu'elle a déjà abandonné une partie de son contenu.
fn recoverable(
    sent_on_disk: u64,
    sent_logical: u64,
    before: Option<RecycleBin>,
    after: Option<RecycleBin>,
) -> u64 {
    match (before, after) {
        (Some(before), Some(after)) => {
            let growth = after.bytes.saturating_sub(before.bytes);
            if growth >= sent_logical { sent_on_disk } else { sent_on_disk.min(growth) }
        }
        // Sans mesure, il reste que chaque envoi a été vérifié un par un.
        _ => sent_on_disk,
    }
}

/// Tient le journal pendant un grand vide : au début, de temps en temps, à
/// la fin. Une écriture qui échoue est ignorée : sur un disque plein, c'est
/// le nettoyage qui compte.
struct Pacer<'a> {
    store: Option<&'a JournalStore>,
    run: Run,
    written: Instant,
}

impl<'a> Pacer<'a> {
    /// Délai entre deux notes en cours de route.
    const EVERY: Duration = Duration::from_secs(2);

    fn begin(store: Option<&'a JournalStore>, started: Timestamp) -> Self {
        let mut pacer = Self { store, run: Run::starting(started), written: Instant::now() };
        pacer.write();
        pacer
    }

    fn write(&mut self) {
        if let Some(store) = self.store {
            let _ = store.record(&self.run);
        }
        self.written = Instant::now();
    }

    /// Note où en est le grand vide.
    fn note(&mut self, progress: &CleanProgress) {
        let now = progress.snapshot();
        self.run.freed_bytes = now.freed_bytes;
        self.run.recoverable_bytes = now.recycled_bytes;
        self.run.skipped_files = now.skipped_files;
        self.write();
    }

    /// Le note seulement si la dernière note commence à dater.
    fn tick(&mut self, progress: &CleanProgress) {
        if self.written.elapsed() >= Self::EVERY {
            self.note(progress);
        }
    }

    fn finish(&mut self, report: &CleanReport) {
        self.run.finished_ms = Some(report.finished.unix_millis());
        self.run.state = match report.outcome {
            CleanOutcome::Completed => RunState::Completed,
            CleanOutcome::Cancelled => RunState::Cancelled,
        };
        self.run.freed_bytes = report.freed_bytes;
        self.run.recoverable_bytes = report.recoverable_bytes;
        self.run.skipped_files = report.skipped.total();
        self.write();
    }
}

/// Pourquoi un grand vide n'a pas pu être lancé.
#[derive(Debug)]
pub enum CleanStartError {
    /// La demande est refusée : rien n'a été touché.
    Refused(CleanRefusal),
    /// Le système n'a pas pu créer le fil d'exécution.
    Thread(io::Error),
}

impl fmt::Display for CleanStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => write!(f, "grand vide refusé : {refusal}"),
            Self::Thread(error) => write!(f, "le grand vide n'a pas pu démarrer : {error}"),
        }
    }
}

impl std::error::Error for CleanStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Refused(refusal) => Some(refusal),
            Self::Thread(error) => Some(error),
        }
    }
}

/// Demande l'arrêt du grand vide quand plus personne ne le suit.
#[derive(Debug)]
struct Watch {
    control: Arc<CleanControl>,
    armed: bool,
}

impl Drop for Watch {
    fn drop(&mut self) {
        if self.armed {
            self.control.cancel();
        }
    }
}

/// Un grand vide lancé dans son propre fil d'exécution.
///
/// Lâcher cette valeur demande l'arrêt : le fichier en cours est mené à son
/// terme, le bilan partiel est noté au journal, et le fil s'arrête.
#[derive(Debug)]
pub struct CleanHandle {
    request: CleanRequest,
    watch: Watch,
    thread: JoinHandle<Arc<CleanReport>>,
}

/// Lance un grand vide sans bloquer l'appelant. La demande est contrôlée
/// avant : refusée, elle ne touche à rien.
pub fn start(
    environment: Arc<dyn Environment>,
    report: Arc<Report>,
    request: CleanRequest,
    excluded: Vec<PathBuf>,
    journal: Option<JournalStore>,
) -> Result<CleanHandle, CleanStartError> {
    check(environment.as_ref(), Some(&report), &request).map_err(CleanStartError::Refused)?;
    let control = Arc::new(CleanControl::new());
    let shared = Arc::clone(&control);
    let thread = std::thread::Builder::new()
        .name("shinku-vide".to_owned())
        .spawn(move || {
            execute(environment.as_ref(), &report, &request, &excluded, journal.as_ref(), &shared)
        })
        .map_err(CleanStartError::Thread)?;
    Ok(CleanHandle { request, watch: Watch { control, armed: true }, thread })
}

impl CleanHandle {
    /// Ce qui a été demandé à ce grand vide.
    pub fn request(&self) -> CleanRequest {
        self.request
    }

    /// Avancement, arrêt et bilan de ce grand vide.
    pub fn control(&self) -> &Arc<CleanControl> {
        &self.watch.control
    }

    /// Vrai quand le grand vide est terminé ou arrêté.
    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    /// Attend la fin du grand vide et rend son bilan.
    pub fn wait(self) -> Arc<CleanReport> {
        let Self { mut watch, thread, .. } = self;
        let outcome = thread.join();
        // Il est fini : il n'y a plus rien à arrêter.
        watch.armed = false;
        match outcome {
            Ok(report) => report,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bin(bytes: u64) -> Option<RecycleBin> {
        Some(RecycleBin { bytes, items: 1 })
    }

    #[test]
    fn the_freed_figure_never_exceeds_what_the_disks_gained() {
        // Pas de mesure : il reste le compte.
        assert_eq!(freed_for_good(500, None, None), 500);
        assert_eq!(freed_for_good(500, Some(1000), None), 500);
        // Les disques ont gagné plus que le compte : un autre programme a
        // libéré de la place en même temps. Ce n'est pas à Shinku.
        assert_eq!(freed_for_good(500, Some(1000), Some(1800)), 500);
        // Ils ont gagné moins : c'est ce gain qui est annoncé.
        assert_eq!(freed_for_good(500, Some(1000), Some(1300)), 300);
        // Ils n'ont rien gagné : la mesure ne dit plus rien du nettoyage.
        assert_eq!(freed_for_good(500, Some(1000), Some(1000)), 500);
        assert_eq!(freed_for_good(500, Some(1000), Some(200)), 500);
        assert_eq!(freed_for_good(0, Some(1000), Some(5000)), 0);
    }

    #[test]
    fn what_the_recycle_bin_dropped_is_not_called_recoverable() {
        // La corbeille a grossi de tout ce qu'elle a reçu.
        assert_eq!(recoverable(4096, 3000, bin(100), bin(3100)), 4096);
        assert_eq!(recoverable(4096, 3000, bin(100), bin(9000)), 4096);
        // Pleine, elle a déjà abandonné une partie de ce qu'elle a reçu.
        assert_eq!(recoverable(4096, 3000, bin(100), bin(1100)), 1000);
        assert_eq!(recoverable(4096, 3000, bin(100), bin(100)), 0);
        // Vidée par quelqu'un d'autre entre-temps.
        assert_eq!(recoverable(4096, 3000, bin(5000), bin(0)), 0);
        // Sans mesure, les envois vérifiés un par un font foi.
        assert_eq!(recoverable(4096, 3000, None, bin(3100)), 4096);
        assert_eq!(recoverable(4096, 3000, bin(100), None), 4096);
        assert_eq!(recoverable(0, 0, bin(100), bin(100)), 0);
    }

    #[test]
    fn an_unknown_process_list_counts_as_everything_open() {
        let running = ["Explorer.EXE".to_owned(), "CHROME.exe".to_owned()];
        assert!(is_running(Some(&running), &["chrome.exe"]));
        assert!(!is_running(Some(&running), &["msedge.exe"]));
        assert!(!is_running(Some(&[]), &["chrome.exe"]));
        assert!(is_running(None, &["chrome.exe"]));
    }

    #[test]
    fn progress_is_counted_step_by_step() {
        let progress = CleanProgress::default();
        assert_eq!(progress.snapshot().phase, CleanPhase::Preparing);
        progress.begin(CleanPhase::JunkFiles, 1000);
        progress.advance(400);
        progress.freed(300, 2);
        progress.skipped(1);
        let now = progress.snapshot();
        assert_eq!((now.phase, now.work_done, now.work_total), (CleanPhase::JunkFiles, 400, 1000));
        assert_eq!((now.freed_bytes, now.files, now.skipped_files), (300, 2, 1));

        // Une nouvelle étape repart de zéro, les totaux restent.
        progress.begin(CleanPhase::Duplicates, 50);
        progress.advance(80);
        progress.recycled(64, 1);
        let now = progress.snapshot();
        assert_eq!((now.work_done, now.work_total), (50, 50), "jamais plus que le total");
        assert_eq!((now.freed_bytes, now.recycled_bytes, now.files), (300, 64, 3));
    }

    #[test]
    fn a_request_is_three_boxes_and_a_date() {
        let request: CleanRequest = serde_json::from_str(
            r#"{ "analysis": 1700000000123, "recycleBin": true, "junkFiles": true, "duplicates": false }"#,
        )
        .unwrap();
        assert_eq!(
            request,
            CleanRequest {
                analysis: 1_700_000_000_123,
                recycle_bin: true,
                junk_files: true,
                duplicates: false
            }
        );
        // Rien d'autre ne passe : ni chemin, ni liste, ni case oubliée.
        let with_a_path = r#"{ "analysis": 1, "recycleBin": true, "junkFiles": true,
            "duplicates": false, "paths": ["C:\\Users"] }"#;
        assert!(serde_json::from_str::<CleanRequest>(with_a_path).is_err());
        let incomplete = r#"{ "analysis": 1, "recycleBin": true, "junkFiles": true }"#;
        assert!(serde_json::from_str::<CleanRequest>(incomplete).is_err());
    }

    #[test]
    fn phases_and_refusals_travel_as_plain_json() {
        assert_eq!(serde_json::to_value(CleanPhase::RecycleBin).unwrap(), "recycleBin");
        assert_eq!(serde_json::to_value(CleanPhase::JunkFiles).unwrap(), "junkFiles");
        assert_eq!(serde_json::to_value(CleanRefusal::StaleResults).unwrap(), "staleResults");
        assert_eq!(serde_json::to_value(CleanRefusal::NothingToClean).unwrap(), "nothingToClean");
    }
}
