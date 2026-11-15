//! La machine, hors de Windows : juste assez pour développer et tester.
//!
//! Shinku ne vise que Windows. Ici, les dossiers connus sont déduits du
//! dossier personnel quand ils existent, et il n'y a ni corbeille ni mesure
//! des disques.

use std::path::PathBuf;
use std::sync::Arc;

use super::{Desktop, Environment, KnownFolder, RecycleBin, Rights, Volume};
use crate::fs::to_internal_path;
use crate::game::host::{GameHost, InertHost};
use crate::model::Timestamp;

#[derive(Debug)]
pub(super) struct SystemEnvironment {
    home: Option<PathBuf>,
}

impl SystemEnvironment {
    pub(super) fn new() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .and_then(|home| to_internal_path(&home).ok());
        Self { home }
    }
}

impl Environment for SystemEnvironment {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        let home = self.home.as_ref()?;
        let path = match folder {
            KnownFolder::Profile => home.clone(),
            KnownFolder::Desktop => home.join("Desktop"),
            KnownFolder::Documents => home.join("Documents"),
            KnownFolder::Downloads => home.join("Downloads"),
            KnownFolder::Music => home.join("Music"),
            KnownFolder::Pictures => home.join("Pictures"),
            KnownFolder::Videos => home.join("Videos"),
            KnownFolder::Temp => to_internal_path(&std::env::temp_dir()).ok()?,
            // Pas d'équivalent : les règles de Windows ne s'appliquent pas ici.
            KnownFolder::LocalAppData
            | KnownFolder::LocalAppDataLow
            | KnownFolder::RoamingAppData
            | KnownFolder::Windows
            | KnownFolder::ProgramFiles
            | KnownFolder::ProgramFilesX86
            | KnownFolder::ProgramData => return None,
        };
        Some(path)
    }

    fn volumes(&self) -> Vec<Volume> {
        vec![Volume {
            root: PathBuf::from("/"),
            label: String::new(),
            file_system: String::new(),
            total: 0,
            free: 0,
            fixed: true,
            seek_penalty: None,
            system: true,
        }]
    }

    fn sync_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn recycle_bin(&self) -> Option<RecycleBin> {
        None
    }
}

pub(super) fn set_background_priority(_on: bool) -> bool {
    false
}

/// L'élévation est une notion de Windows : ici, un processus est toujours ordinaire.
pub(super) fn own_rights() -> Rights {
    Rights::Standard
}

pub(super) fn desktop_rights() -> Desktop {
    Desktop::Absent
}

/// Hors de Windows, le mode jeu ne voit rien : pas de premier plan, pas de
/// mode d'alimentation.
pub(super) fn game_host() -> Arc<dyn GameHost> {
    Arc::new(InertHost)
}

/// L'agent hors de Windows : il n'existe pas.
pub(super) mod agent {
    use std::io;

    use crate::game::agent::{AgentEvent, Flow};
    use crate::platform::agent::{AgentState, Autostart, OpenProgram, Order, WindowsPage};

    /// Rien à garder : il n'y a pas d'instance unique hors de Windows.
    #[derive(Debug)]
    pub struct Guard;

    pub fn acquire_instance() -> Option<Guard> {
        None
    }

    pub fn agent_running() -> bool {
        false
    }

    pub fn spawn_agent(_release_first: bool) -> AgentState {
        AgentState::Missing
    }

    pub fn post_agent(_order: Order) -> bool {
        false
    }

    pub fn run_agent(_handler: &mut dyn FnMut(AgentEvent) -> Flow) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "l'agent ne tourne que sous Windows"))
    }

    pub fn set_autostart(_on: bool) -> io::Result<()> {
        Ok(())
    }

    pub fn autostart() -> Autostart {
        Autostart::Absent
    }

    pub fn open_apps() -> Vec<OpenProgram> {
        Vec::new()
    }

    pub fn open_windows_settings(_page: WindowsPage) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "les réglages de Windows n'existent que sous Windows",
        ))
    }
}
