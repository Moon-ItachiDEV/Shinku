//! Un faux hôte pour essayer la décision de `--oublier` sans disque ni attente :
//! l'horloge est un compteur que `sleep` avance, et chaque appel est noté.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::ForgetHost;
use crate::fs::{Removed, TreeReport};
use crate::platform::KnownFolder;

pub const ROAMING: &str = "/pc/Roaming";
pub const LOCAL: &str = "/pc/Local";

#[derive(Debug)]
pub struct FakeHost {
    pub elevated: bool,
    pub folders: Vec<(KnownFolder, PathBuf)>,
    /// Ce que chaque appel de suppression coûte en temps.
    pub cost: Duration,
    pub file_default: Removed,
    pub dir_default: Removed,
    pub tree_default: TreeReport,
    clock: Cell<Duration>,
    slept: RefCell<Vec<Duration>>,
    log: RefCell<Vec<String>>,
    files: RefCell<HashMap<String, VecDeque<Removed>>>,
    dirs: RefCell<HashMap<String, VecDeque<Removed>>>,
    trees: RefCell<HashMap<String, VecDeque<TreeReport>>>,
}

impl FakeHost {
    /// Une machine où il n'y a rien à effacer.
    pub fn new() -> Self {
        Self {
            elevated: false,
            folders: vec![
                (KnownFolder::RoamingAppData, PathBuf::from(ROAMING)),
                (KnownFolder::LocalAppData, PathBuf::from(LOCAL)),
            ],
            cost: Duration::ZERO,
            file_default: Removed::Missing,
            dir_default: Removed::Missing,
            tree_default: TreeReport::default(),
            clock: Cell::new(Duration::ZERO),
            slept: RefCell::new(Vec::new()),
            log: RefCell::new(Vec::new()),
            files: RefCell::new(HashMap::new()),
            dirs: RefCell::new(HashMap::new()),
            trees: RefCell::new(HashMap::new()),
        }
    }

    /// Ce que les appels successifs sur ce fichier répondent, puis le défaut.
    pub fn script_file(&self, path: &str, outcomes: &[Removed]) {
        self.files.borrow_mut().insert(path.to_owned(), outcomes.iter().copied().collect());
    }

    pub fn script_dir(&self, path: &str, outcomes: &[Removed]) {
        self.dirs.borrow_mut().insert(path.to_owned(), outcomes.iter().copied().collect());
    }

    pub fn script_tree(&self, path: &str, reports: &[TreeReport]) {
        self.trees.borrow_mut().insert(path.to_owned(), reports.iter().copied().collect());
    }

    /// Les appels, dans l'ordre.
    pub fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    /// Les appels qui suppriment, sans les attentes ni les dossiers demandés.
    pub fn removals(&self) -> Vec<String> {
        self.log()
            .into_iter()
            .filter(|line| {
                line.starts_with("file ") || line.starts_with("dir ") || line.starts_with("tree ")
            })
            .collect()
    }

    pub fn sleeps(&self) -> Vec<Duration> {
        self.slept.borrow().clone()
    }

    pub fn total_slept(&self) -> Duration {
        self.slept.borrow().iter().sum()
    }

    pub fn now(&self) -> Duration {
        self.clock.get()
    }

    fn note(&self, line: String) {
        self.log.borrow_mut().push(line);
        self.clock.set(self.clock.get() + self.cost);
    }
}

/// Le chemin écrit avec des `/` partout : les essais passent aussi sous Windows.
fn shown(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn next<T: Copy>(map: &RefCell<HashMap<String, VecDeque<T>>>, key: &str, default: T) -> T {
    map.borrow_mut().get_mut(key).and_then(VecDeque::pop_front).unwrap_or(default)
}

impl ForgetHost for FakeHost {
    fn elevated(&self) -> bool {
        self.log.borrow_mut().push("elevated?".to_owned());
        self.elevated
    }

    fn folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        self.log.borrow_mut().push(format!("folder {folder:?}"));
        self.folders.iter().find(|(known, _)| *known == folder).map(|(_, path)| path.clone())
    }

    fn elapsed(&self) -> Duration {
        self.clock.get()
    }

    fn sleep(&self, duration: Duration) {
        self.slept.borrow_mut().push(duration);
        self.clock.set(self.clock.get() + duration);
    }

    fn remove_file(&self, dir: &Path, name: &OsStr) -> Removed {
        let path = shown(&dir.join(name));
        self.note(format!("file {path}"));
        next(&self.files, &path, self.file_default)
    }

    fn remove_empty_dir(&self, dir: &Path) -> Removed {
        let path = shown(dir);
        self.note(format!("dir {path}"));
        next(&self.dirs, &path, self.dir_default)
    }

    fn remove_tree(&self, root: &Path) -> TreeReport {
        let path = shown(root);
        self.note(format!("tree {path}"));
        next(&self.trees, &path, self.tree_default)
    }
}
