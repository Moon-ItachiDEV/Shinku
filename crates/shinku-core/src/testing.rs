//! Outils des tests du cœur : une machine qui se comporte mal sur commande.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::model::Timestamp;
use crate::platform::{
    Environment, KnownFolder, RecycleBin, RecycleError, StaticEnvironment, Volume,
};

/// Temps laissé à un test pour voir arriver ce qu'il attend.
const PATIENCE: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
struct GateState {
    reached: bool,
    open: bool,
}

/// Une porte fermée, que le test ouvre quand il veut.
#[derive(Debug, Default)]
pub(crate) struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

impl Gate {
    fn state(&self) -> MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Attend que quelqu'un soit arrivé devant la porte.
    pub(crate) fn wait_until_reached(&self) {
        let deadline = Instant::now() + PATIENCE;
        let mut state = self.state();
        while !state.reached {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "personne n'est arrivé devant la porte");
            state =
                self.changed.wait_timeout(state, left).unwrap_or_else(PoisonError::into_inner).0;
        }
    }

    /// Ouvre la porte : ceux qui attendent passent, les suivants aussi.
    pub(crate) fn open(&self) {
        self.state().open = true;
        self.changed.notify_all();
    }

    /// Se présente devant la porte et attend qu'elle s'ouvre.
    fn pass(&self) {
        let mut state = self.state();
        state.reached = true;
        self.changed.notify_all();
        while !state.open {
            state = self.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Une machine dont la corbeille se fait attendre : la vider reste bloqué
/// tant que le test n'a pas ouvert la porte. C'est ce qui permet d'observer
/// un grand vide « en cours ».
///
/// Avec `breaks_down`, vider la corbeille fait tomber le fil d'exécution,
/// comme le ferait un défaut du programme.
#[derive(Debug)]
pub(crate) struct SlowBin {
    pub(crate) machine: StaticEnvironment,
    pub(crate) gate: Arc<Gate>,
    pub(crate) breaks_down: bool,
}

impl SlowBin {
    /// Cette machine, avec une corbeille bloquée, et la porte qui la débloque.
    pub(crate) fn new(machine: StaticEnvironment) -> (Self, Arc<Gate>) {
        let gate = Arc::new(Gate::default());
        (Self { machine, gate: Arc::clone(&gate), breaks_down: false }, gate)
    }
}

impl Environment for SlowBin {
    fn now(&self) -> Timestamp {
        self.machine.now()
    }

    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        self.machine.known_folder(folder)
    }

    fn volumes(&self) -> Vec<Volume> {
        self.machine.volumes()
    }

    fn sync_roots(&self) -> Vec<PathBuf> {
        self.machine.sync_roots()
    }

    fn recycle_bin(&self) -> Option<RecycleBin> {
        self.machine.recycle_bin()
    }

    fn parallelism(&self) -> usize {
        self.machine.parallelism()
    }

    fn running_processes(&self) -> Option<Vec<String>> {
        self.machine.running_processes()
    }

    fn empty_recycle_bin(&self) -> io::Result<()> {
        assert!(!self.breaks_down, "panne simulée pendant le grand vide");
        self.gate.pass();
        self.machine.empty_recycle_bin()
    }

    fn recycle(&self, path: &Path) -> Result<(), RecycleError> {
        self.machine.recycle(path)
    }
}
