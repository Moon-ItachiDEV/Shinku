//! L'étape « Fichiers inutiles » du grand vide : suppression directe.
//!
//! Rien ne vient de l'interface ni du rapport affiché : l'étape refait le
//! parcours de l'analyse, avec les mêmes règles et les mêmes âges, et supprime
//! ce que ce parcours compte « à nettoyer » ([`crate::scan::index`]). Ce qui
//! a changé depuis l'analyse est donc jugé tel qu'il est maintenant, et rien
//! de plus récent qu'elle n'est touché.
//!
//! Chaque suppression passe par [`Removal`] : l'entrée est ouverte sans suivre
//! de lien, revérifiée, puis supprimée par ce même accès. Un fichier qu'une
//! app tient est laissé. Rien n'est forcé, rien n'est remis au prochain
//! démarrage de Windows.
//!
//! Un élément jugé en bloc (un sous-dossier de `%TEMP%`) part en entier ou
//! reste en entier : si un seul de ses fichiers résiste, c'est qu'un
//! programme s'en sert encore, et lui retirer le reste le casserait.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::report::{JunkStep, Left, LeftOpen, SkippedFiles, StepState};
use super::{CleanControl, CleanPhase, is_running};
use crate::fs::{Removal, RemoveError};
use crate::guard::Guard;
use crate::model::{Timestamp, Trigger};
use crate::platform::Environment;
use crate::scan::index::{SweptDir, SweptFile};
use crate::scan::plan::Plan;
use crate::scan::{Modules, Progress, ScanOptions, index_disks};

/// Ce que l'étape a fait.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    pub step: JunkStep,
    pub skipped: SkippedFiles,
}

/// Un élément jugé en bloc, avec tout ce que le parcours y a relevé.
#[derive(Debug, Default)]
struct Unit {
    files: Vec<SweptFile>,
    dirs: Vec<SweptDir>,
}

/// Ce qu'une règle a à nettoyer.
#[derive(Debug, Default)]
struct Batch {
    /// Fichiers jugés un par un.
    loose: Vec<SweptFile>,
    /// Éléments jugés en bloc, par identifiant.
    units: BTreeMap<u32, Unit>,
    /// Place occupée par l'ensemble.
    bytes: u64,
}

impl Batch {
    fn is_empty(&self) -> bool {
        self.loose.is_empty() && self.units.is_empty()
    }
}

/// Ce qu'il faut savoir faire d'une entrée ouverte pour la supprimer en lot.
/// [`Removal`] le fait pour de vrai ; les tests en fabriquent de fausses.
pub(crate) trait Staged {
    /// Demande la suppression, sans l'accomplir. Lâcher la valeur retire la demande.
    fn mark(&mut self) -> Result<(), RemoveError>;
    /// Accomplit la suppression.
    fn commit(self) -> Result<(), RemoveError>;
}

impl Staged for Removal {
    fn mark(&mut self) -> Result<(), RemoveError> {
        Removal::mark(self)
    }

    fn commit(self) -> Result<(), RemoveError> {
        Removal::commit(self)
    }
}

/// Ce qu'un lot « tout ou rien » a supprimé.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Removed {
    pub files: u64,
    pub bytes: u64,
    /// Entrées qui ont résisté après que toutes ont accepté de partir. Cela
    /// n'arrive pas sous Windows, où la demande acceptée vaut suppression.
    pub failed: u64,
}

/// Supprime toutes ces entrées (avec la place que chacune occupe), ou aucune.
///
/// La suppression est d'abord demandée pour chacune. Au premier refus, tout
/// est lâché : les demandes déjà posées sont retirées et rien ne part.
pub(crate) fn all_or_nothing<S: Staged>(
    mut entries: Vec<(S, u64)>,
) -> Result<Removed, RemoveError> {
    for (entry, _) in &mut entries {
        entry.mark()?;
    }
    let mut removed = Removed::default();
    for (entry, bytes) in entries {
        match entry.commit() {
            Ok(()) => {
                removed.files += 1;
                removed.bytes += bytes;
            }
            Err(_) => removed.failed += 1,
        }
    }
    Ok(removed)
}

/// Range un refus de suppression. `None` : l'entrée avait déjà disparu, il
/// n'y avait plus rien à faire.
fn reason(error: &RemoveError) -> Option<Left> {
    match error {
        RemoveError::NotFound => None,
        RemoveError::Locked => Some(Left::InUse),
        RemoveError::Changed => Some(Left::Changed),
        RemoveError::AccessDenied => Some(Left::Denied),
        RemoveError::NotEmpty | RemoveError::Io(_) => Some(Left::Other),
    }
}

/// Supprime un fichier jugé seul.
fn remove_loose(guard: &Guard, file: &SweptFile) -> Result<(), Option<Left>> {
    // Dernier contrôle avant d'écrire : la liste d'interdits, relue pour ce chemin.
    if guard.forbids(&file.path) {
        return Err(Some(Left::Other));
    }
    Removal::open(&file.path, &file.expected)
        .and_then(Removal::commit)
        .map_err(|error| reason(&error))
}

/// Au-delà de ce nombre d'entrées, un élément jugé en bloc est laissé. Toutes
/// ses entrées sont tenues ouvertes en même temps, et la suppression d'un
/// élément ne s'interrompt pas : elle doit rester courte, pour que fermer
/// Shinku n'en laisse jamais un à moitié supprimé.
const MAX_UNIT_ENTRIES: usize = 10_000;

/// Supprime un élément jugé en bloc : tous ses fichiers, puis ses dossiers.
fn remove_unit(guard: &Guard, unit: &Unit, cutoff: Timestamp) -> Result<Removed, Left> {
    if unit.files.len() + unit.dirs.len() > MAX_UNIT_ENTRIES {
        return Err(Left::Other);
    }
    let paths = unit.files.iter().map(|file| &file.path).chain(unit.dirs.iter().map(|d| &d.path));
    if paths.into_iter().any(|path| guard.forbids(path)) {
        return Err(Left::Other);
    }
    // Dans un lot, une entrée disparue n'est pas « déjà faite » : c'est que
    // quelqu'un d'autre y travaille. L'élément est laissé.
    let left = |error: RemoveError| reason(&error).unwrap_or(Left::Changed);

    // Tout est ouvert avant que quoi que ce soit ne parte. Les dossiers
    // d'abord : leur date, relue sur le dossier ouvert, dit si quelque chose y
    // est entré ou en est sorti depuis l'âge minimal de la règle.
    let mut dirs: Vec<(u32, Removal)> = Vec::with_capacity(unit.dirs.len());
    for dir in &unit.dirs {
        let opened = Removal::open(&dir.path, &dir.expected).map_err(left)?;
        if opened.modified().map_err(left)? > cutoff {
            return Err(Left::Changed);
        }
        dirs.push((dir.depth, opened));
    }
    let mut files: Vec<(Removal, u64)> = Vec::with_capacity(unit.files.len());
    for file in &unit.files {
        // Une entrée qu'une app tient sans permettre sa suppression est
        // refusée ici, et l'élément entier reste.
        files.push((Removal::open(&file.path, &file.expected).map_err(left)?, file.bytes));
    }

    let removed = all_or_nothing(files).map_err(left)?;

    // Les dossiers, du plus profond au moins profond. Un dossier où un
    // fichier est apparu entre-temps n'est pas vide : il reste, avec ce
    // fichier, et ses parents aussi.
    dirs.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
    for (_, dir) in dirs {
        let _ = dir.commit();
    }
    Ok(removed)
}

/// Vrai si l'une de ces apps tourne, ou si la machine ne sait pas le dire.
fn app_is_open(environment: &dyn Environment, processes: &[&str]) -> bool {
    is_running(environment.running_processes().as_deref(), processes)
}

/// Le plan de l'étape : les règles de fichiers inutiles et rien d'autre, aux
/// âges de l'analyse commencée à `analysis_started`.
pub(crate) fn plan(
    environment: &dyn Environment,
    analysis_started: Timestamp,
    excluded: &[PathBuf],
) -> Plan {
    let options = ScanOptions {
        excluded: excluded.to_vec(),
        modules: Modules { junk: true, duplicates: false, large_files: false, recycle_bin: false },
        background: false,
        ..ScanOptions::new(Trigger::Button)
    };
    Plan::build(environment, &options, analysis_started)
}

/// Mène l'étape. `tick` est appelé régulièrement : il sert au journal.
pub(crate) fn clean(
    environment: &dyn Environment,
    analysis_started: Timestamp,
    excluded: &[PathBuf],
    control: &CleanControl,
    tick: &mut dyn FnMut(),
) -> Outcome {
    let mut outcome = Outcome::default();
    let plan = plan(environment, analysis_started, excluded);
    // Un « dossier jamais touché » que Shinku ne sait pas appliquer ne
    // protège rien : dans le doute, on ne supprime rien.
    if plan.misses_an_exclusion() {
        outcome.step.state = StepState::Failed;
        return outcome;
    }
    let Some(indexed) =
        index_disks(&plan, false, &Progress::default(), control.cancel_flag(), true)
    else {
        outcome.step.state = StepState::Stopped;
        return outcome;
    };

    let mut batches: Vec<Batch> = plan.rules.iter().map(|_| Batch::default()).collect();
    for file in indexed.swept_files {
        let batch = &mut batches[file.rule as usize];
        if file.unit == 0 {
            batch.bytes += file.bytes;
            batch.loose.push(file);
        } else if indexed.clean_units.contains(&file.unit) {
            batch.bytes += file.bytes;
            batch.units.entry(file.unit).or_default().files.push(file);
        }
    }
    for dir in indexed.swept_dirs {
        if indexed.clean_units.contains(&dir.unit) {
            batches[dir.rule as usize].units.entry(dir.unit).or_default().dirs.push(dir);
        }
    }

    let guard = Guard::for_machine(environment);
    let progress = control.progress_counters();
    progress.begin(CleanPhase::JunkFiles, batches.iter().map(|batch| batch.bytes).sum());
    let step = &mut outcome.step;
    let skipped = &mut outcome.skipped;
    step.state = StepState::Done;

    'rules: for (planned, batch) in plan.rules.iter().zip(batches) {
        if batch.is_empty() {
            continue;
        }
        if control.is_cancelled() {
            step.state = StepState::Stopped;
            break;
        }
        let rule = planned.rule;
        // Nettoyer le cache d'une app ouverte peut lui faire perdre ce
        // qu'elle est en train d'écrire : on la laisse, et on le dit.
        if !rule.close_first.is_empty() && app_is_open(environment, rule.close_first) {
            match step.left_open.iter_mut().find(|app| app.label == rule.label) {
                Some(app) => app.bytes += batch.bytes,
                None => step.left_open.push(LeftOpen { label: rule.label, bytes: batch.bytes }),
            }
            progress.advance(batch.bytes);
            continue;
        }

        for file in &batch.loose {
            if control.is_cancelled() {
                step.state = StepState::Stopped;
                break 'rules;
            }
            match remove_loose(&guard, file) {
                Ok(()) => {
                    step.files += 1;
                    step.bytes += file.bytes;
                    progress.freed(file.bytes, 1);
                }
                Err(None) => {}
                Err(Some(reason)) => {
                    skipped.add(reason, 1);
                    progress.skipped(1);
                }
            }
            progress.advance(file.bytes);
            tick();
        }

        for unit in batch.units.values() {
            if control.is_cancelled() {
                step.state = StepState::Stopped;
                break 'rules;
            }
            let (files, bytes) =
                (unit.files.len() as u64, unit.files.iter().map(|file| file.bytes).sum::<u64>());
            match remove_unit(&guard, unit, planned.cutoff) {
                Ok(removed) => {
                    step.files += removed.files;
                    step.bytes += removed.bytes;
                    progress.freed(removed.bytes, removed.files);
                    if removed.failed > 0 {
                        skipped.add(Left::Other, removed.failed);
                        progress.skipped(removed.failed);
                    }
                }
                Err(reason) => {
                    skipped.add(reason, files);
                    progress.skipped(files);
                }
            }
            progress.advance(bytes);
            tick();
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    /// Une fausse entrée, qui note ce qu'on lui fait.
    struct Fake {
        name: &'static str,
        refuses: bool,
        marked: bool,
        log: Rc<RefCell<Vec<String>>>,
    }

    impl Staged for Fake {
        fn mark(&mut self) -> Result<(), RemoveError> {
            if self.refuses {
                self.log.borrow_mut().push(format!("{} refuse", self.name));
                return Err(RemoveError::AccessDenied);
            }
            self.marked = true;
            self.log.borrow_mut().push(format!("{} marqué", self.name));
            Ok(())
        }

        fn commit(mut self) -> Result<(), RemoveError> {
            self.marked = false;
            self.log.borrow_mut().push(format!("{} supprimé", self.name));
            Ok(())
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            if self.marked {
                self.log.borrow_mut().push(format!("{} rendu", self.name));
            }
        }
    }

    fn fakes(names: &[(&'static str, bool)], log: &Rc<RefCell<Vec<String>>>) -> Vec<(Fake, u64)> {
        names
            .iter()
            .map(|&(name, refuses)| {
                (Fake { name, refuses, marked: false, log: Rc::clone(log) }, 100)
            })
            .collect()
    }

    #[test]
    fn everything_goes_when_nothing_resists() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let removed =
            all_or_nothing(fakes(&[("a", false), ("b", false), ("c", false)], &log)).unwrap();
        assert_eq!(removed, Removed { files: 3, bytes: 300, failed: 0 });
        assert_eq!(
            *log.borrow(),
            ["a marqué", "b marqué", "c marqué", "a supprimé", "b supprimé", "c supprimé"]
        );
    }

    #[test]
    fn one_refusal_gives_everything_back() {
        // « c » est le programme encore ouvert d'un dossier temporaire : les
        // fichiers déjà réservés sont rendus, et rien n'est supprimé.
        let log = Rc::new(RefCell::new(Vec::new()));
        let outcome = all_or_nothing(fakes(&[("a", false), ("b", false), ("c", true)], &log));
        assert!(matches!(outcome, Err(RemoveError::AccessDenied)));
        let log = log.borrow();
        assert!(!log.iter().any(|line| line.ends_with("supprimé")), "{log:?}");
        assert!(log.contains(&"a rendu".to_owned()) && log.contains(&"b rendu".to_owned()));
    }

    #[test]
    fn an_empty_batch_is_a_success() {
        assert_eq!(all_or_nothing(Vec::<(Fake, u64)>::new()).unwrap(), Removed::default());
    }

    #[test]
    fn refusals_are_sorted_by_reason() {
        assert_eq!(reason(&RemoveError::NotFound), None);
        assert_eq!(reason(&RemoveError::Locked), Some(Left::InUse));
        assert_eq!(reason(&RemoveError::Changed), Some(Left::Changed));
        assert_eq!(reason(&RemoveError::AccessDenied), Some(Left::Denied));
        assert_eq!(reason(&RemoveError::NotEmpty), Some(Left::Other));
    }
}
