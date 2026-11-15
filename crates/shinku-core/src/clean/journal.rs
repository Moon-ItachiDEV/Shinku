//! Le journal des grands vides, gardé dans `%APPDATA%\Shinku\journal.json`.
//!
//! Il sert à une chose : savoir, au lancement suivant, qu'un grand vide a été
//! coupé en route (panne de courant, PC éteint) et ce qu'il avait déjà fait.
//! Rien n'est rejoué : Shinku le dit, et une nouvelle analyse repart du disque
//! tel qu'il est.
//!
//! Le journal ne contient que des dates et des totaux, jamais un chemin ni un
//! nom de fichier. Il ne quitte pas le PC.
//!
//! L'écrire ne doit jamais empêcher un nettoyage : sur un disque plein, c'est
//! justement le nettoyage qui rendra de la place. Les erreurs d'écriture sont
//! donc rendues à l'appelant, qui les ignore.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::Timestamp;
use crate::platform::Environment;
use crate::settings::{app_dir, write_replacing};

/// Version du format de `journal.json`.
pub const JOURNAL_VERSION: u32 = 1;
/// Nom du fichier.
const FILE_NAME: &str = "journal.json";
/// Nombre de grands vides gardés : les plus anciens sont oubliés.
const RUNS_KEPT: usize = 100;

/// Où en est un grand vide dans le journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    /// En cours. S'il l'est encore au lancement de Shinku, c'est qu'il a été coupé.
    Running,
    /// Mené jusqu'au bout.
    Completed,
    /// Arrêté par l'utilisateur.
    Cancelled,
    /// Coupé en route : Shinku s'est arrêté sans pouvoir conclure.
    Interrupted,
}

/// Un grand vide dans le journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    /// Début, en millisecondes depuis 1970.
    pub started_ms: i64,
    /// Fin, si elle a pu être notée.
    pub finished_ms: Option<i64>,
    /// Où il en est.
    pub state: RunState,
    /// Place libérée pour de bon, au dernier pointage.
    pub freed_bytes: u64,
    /// Place occupée par ce qui est parti à la corbeille, au dernier pointage.
    pub recoverable_bytes: u64,
    /// Fichiers laissés en place, au dernier pointage.
    pub skipped_files: u64,
}

impl Run {
    /// Un grand vide qui commence.
    pub fn starting(started: Timestamp) -> Self {
        Self {
            started_ms: started.unix_millis(),
            finished_ms: None,
            state: RunState::Running,
            freed_bytes: 0,
            recoverable_bytes: 0,
            skipped_files: 0,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    version: u32,
    runs: Vec<Run>,
}

/// Le fichier du journal.
#[derive(Clone, Debug)]
pub struct JournalStore {
    path: PathBuf,
}

impl JournalStore {
    /// Le journal de cette machine : `%APPDATA%\Shinku\journal.json`. `None`
    /// si Windows ne donne pas le dossier `AppData\Roaming`.
    pub fn for_machine(environment: &dyn Environment) -> Option<Self> {
        Some(Self::at(app_dir(environment)?.join(FILE_NAME)))
    }

    /// Un journal à cet emplacement.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Emplacement du fichier.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Les grands vides notés, du plus ancien au plus récent. Un journal
    /// absent ou illisible est un journal vide.
    pub fn runs(&self) -> Vec<Run> {
        let Ok(text) = fs::read_to_string(&self.path) else { return Vec::new() };
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        serde_json::from_str::<Document>(text).map(|document| document.runs).unwrap_or_default()
    }

    fn save(&self, mut runs: Vec<Run>) -> io::Result<()> {
        if runs.len() > RUNS_KEPT {
            runs.drain(..runs.len() - RUNS_KEPT);
        }
        let document = Document { version: JOURNAL_VERSION, runs };
        let json = serde_json::to_string_pretty(&document).map_err(io::Error::other)?;
        let dir = self
            .path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "journal sans dossier"))?;
        fs::create_dir_all(dir)?;
        write_replacing(&self.path, json.as_bytes())
    }

    /// Note un grand vide : son début, un pointage en route, ou sa fin. Deux
    /// notes qui portent le même début décrivent le même grand vide.
    pub fn record(&self, run: &Run) -> io::Result<()> {
        let mut runs = self.runs();
        match runs.iter_mut().rev().find(|known| known.started_ms == run.started_ms) {
            Some(known) => *known = *run,
            None => runs.push(*run),
        }
        self.save(runs)
    }

    /// À appeler au lancement de Shinku : un grand vide resté « en cours » a
    /// été coupé. Il est noté comme tel, et rendu pour que Shinku le dise.
    ///
    /// Un seul grand vide tourne à la fois, et une seule fenêtre de Shinku :
    /// rien d'autre n'écrit dans le journal à ce moment-là.
    pub fn close_interrupted(&self) -> Option<Run> {
        let mut runs = self.runs();
        let mut last = None;
        for run in runs.iter_mut().filter(|run| run.state == RunState::Running) {
            run.state = RunState::Interrupted;
            last = Some(*run);
        }
        if last.is_some() {
            // Si l'écriture échoue, il sera rendu à nouveau au prochain
            // lancement : c'est sans gravité.
            let _ = self.save(runs);
        }
        last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, JournalStore) {
        let temp = tempfile::tempdir().unwrap();
        let store = JournalStore::at(temp.path().join("Shinku").join("journal.json"));
        (temp, store)
    }

    fn run(started_ms: i64) -> Run {
        Run::starting(Timestamp::from_ticks(started_ms * 10_000))
    }

    #[test]
    fn a_missing_or_damaged_journal_is_empty() {
        let (_temp, store) = store();
        assert!(store.runs().is_empty());
        assert_eq!(store.close_interrupted(), None);
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), "{ \"runs\": [").unwrap();
        assert!(store.runs().is_empty());
        // Un journal illisible est simplement recommencé.
        store.record(&run(5)).unwrap();
        assert_eq!(store.runs(), [run(5)]);
    }

    #[test]
    fn a_run_is_updated_in_place() {
        let (_temp, store) = store();
        store.record(&run(1000)).unwrap();
        store.record(&run(2000)).unwrap();
        let mut second = run(2000);
        second.freed_bytes = 4096;
        second.skipped_files = 2;
        store.record(&second).unwrap();
        second.state = RunState::Completed;
        second.finished_ms = Some(2500);
        store.record(&second).unwrap();
        assert_eq!(store.runs(), [run(1000), second]);

        let text = fs::read_to_string(store.path()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["version"], JOURNAL_VERSION);
        assert_eq!(json["runs"][1]["state"], "completed");
        assert_eq!(json["runs"][1]["freedBytes"], 4096);
        assert_eq!(json["runs"][0]["finishedMs"], serde_json::Value::Null);
    }

    #[test]
    fn a_run_left_running_was_interrupted() {
        let (_temp, store) = store();
        let mut done = run(1000);
        done.state = RunState::Cancelled;
        store.record(&done).unwrap();
        let mut cut = run(2000);
        cut.freed_bytes = 7 * 1024 * 1024;
        store.record(&cut).unwrap();

        let found = store.close_interrupted().expect("un grand vide coupé");
        assert_eq!(found.state, RunState::Interrupted);
        assert_eq!((found.started_ms, found.freed_bytes), (2000, 7 * 1024 * 1024));
        // Il n'est annoncé qu'une fois.
        assert_eq!(store.close_interrupted(), None);
        assert_eq!(store.runs()[1].state, RunState::Interrupted);
        assert_eq!(store.runs()[0].state, RunState::Cancelled);
    }

    #[test]
    fn only_the_most_recent_runs_are_kept() {
        let (_temp, store) = store();
        let runs: Vec<Run> = (0..RUNS_KEPT as i64 + 5).map(run).collect();
        store.save(runs).unwrap();
        let kept = store.runs();
        assert_eq!(kept.len(), RUNS_KEPT);
        assert_eq!(kept[0].started_ms, 5);
    }

    #[test]
    fn a_journal_that_cannot_be_written_says_so() {
        let temp = tempfile::tempdir().unwrap();
        let blocked = temp.path().join("bloque");
        fs::write(&blocked, b"x").unwrap();
        let store = JournalStore::at(blocked.join("Shinku").join("journal.json"));
        assert!(store.record(&run(1)).is_err());
        assert!(store.runs().is_empty());
    }
}
