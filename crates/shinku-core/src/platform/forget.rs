//! La vraie machine de `shinku.exe --oublier`.
//!
//! Aucun appel de Windows ici : l'élévation est la décision de
//! [`elevated_or_unknown`] (celle de la fenêtre, qui refuse en plus quand les
//! droits ne se lisent pas), les dossiers viennent de [`system`], les suppressions de
//! [`crate::fs`] et le temps de la bibliothèque standard. Le code `unsafe`, il y
//! en a, est dans ces modules-là.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{Environment, KnownFolder, elevated_or_unknown, system};
use crate::forget::ForgetHost;
use crate::fs::{self, Removed, TreeLimits, TreeReport};

/// La machine qui efface pour de bon.
#[derive(Debug)]
pub struct SystemForget {
    started: Instant,
}

impl SystemForget {
    /// L'horloge part d'ici.
    pub fn new() -> Self {
        Self { started: Instant::now() }
    }
}

impl Default for SystemForget {
    fn default() -> Self {
        Self::new()
    }
}

impl ForgetHost for SystemForget {
    fn elevated(&self) -> bool {
        elevated_or_unknown()
    }

    fn folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        system().known_folder(folder)
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn remove_file(&self, dir: &Path, name: &OsStr) -> Removed {
        fs::remove_plain_file(dir, name)
    }

    fn remove_empty_dir(&self, dir: &Path) -> Removed {
        fs::remove_empty_dir(dir)
    }

    fn remove_tree(&self, root: &Path, deadline: Duration) -> TreeReport {
        fs::remove_tree(root, TreeLimits::default(), &|| self.elapsed() >= deadline)
    }
}
