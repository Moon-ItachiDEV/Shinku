//! L'agent du mode jeu vu de la machine : qui tourne, comment le lancer,
//! comment lui parler, et la valeur `Run` qui le lance avec Windows.
//!
//! Sous Windows, tout est réel (voir `windows/agent.rs`) : une fenêtre cachée,
//! un mutex nommé, des messages. Ailleurs, rien ne tourne : il n'y a pas
//! d'agent.

use std::io;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::imp;
use crate::game::agent::{AgentEvent, Flow};
use crate::startup::State;
use crate::startup::approval;
use crate::startup::host::RawApproval;

/// Nom du fichier de l'agent, à côté de `shinku.exe`.
#[cfg(windows)]
pub const AGENT_FILE: &str = "shinku-agent.exe";
/// Nom du fichier de l'agent, à côté de `shinku`.
#[cfg(not(windows))]
pub const AGENT_FILE: &str = "shinku-agent";

/// Nom de la valeur de registre qui lance l'agent avec Windows.
pub const RUN_VALUE: &str = "Shinku";

/// L'agent, vu de la fenêtre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentState {
    /// Il tourne.
    Running,
    /// Il est installé mais ne tourne pas.
    Stopped,
    /// Cette installation n'a pas son fichier.
    Missing,
}

/// Un ordre que la fenêtre ou l'installeur donne à l'agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// Tout remettre en place maintenant.
    Release,
    /// S'arrêter, après avoir tout remis en place.
    Quit,
    /// S'arrêter, ne plus se lancer avec Windows.
    Remove,
}

/// La valeur `Run`, vue de Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Autostart {
    /// Pas de valeur.
    Absent,
    /// La valeur est là, et Windows la suit.
    Enabled,
    /// La valeur est là, mais la personne l'a désactivée dans les apps de
    /// démarrage de Windows : Shinku n'y touche pas.
    DisabledInWindows,
}

/// Une page des réglages de Windows. Trois seulement, à adresses fixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowsPage {
    /// Mode Jeu de Windows.
    GameMode,
    /// Alimentation et mise en veille.
    Power,
    /// Apps de démarrage.
    StartupApps,
}

impl WindowsPage {
    /// L'adresse `ms-settings:` de cette page.
    pub const fn uri(self) -> &'static str {
        match self {
            Self::GameMode => "ms-settings:gaming-gamemode",
            Self::Power => "ms-settings:powersleep",
            Self::StartupApps => "ms-settings:startupapps",
        }
    }
}

/// Un programme qui a une fenêtre ouverte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenProgram {
    /// Le nom de son exécutable (`javaw.exe`), tel que Windows l'écrit.
    pub name: String,
    /// Le chemin complet de son exécutable.
    pub path: String,
}

/// La valeur que la clé `Run` « Shinku » doit porter : l'agent entre
/// guillemets, avec son argument. Le module Démarrage s'en sert pour
/// reconnaître l'entrée de Shinku lui-même : seule une valeur qui porte
/// exactement cette commande en est une.
pub fn run_command() -> Option<String> {
    let file = agent_file()?;
    Some(format!("\"{}\" --demarrage", file.display()))
}

/// Le fichier de l'agent, à côté de l'exécutable courant.
pub fn agent_file() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join(AGENT_FILE))
}

/// Garde de l'instance unique : tant qu'elle vit, aucun autre agent ne démarre.
#[derive(Debug)]
pub struct Instance {
    _guard: imp::agent::Guard,
}

/// Prend la place d'unique agent. `None` s'il y en a déjà un (ou si Windows
/// refuse de créer le mutex).
pub fn acquire_instance() -> Option<Instance> {
    imp::agent::acquire_instance().map(|guard| Instance { _guard: guard })
}

/// Vrai si un agent tourne en ce moment.
pub fn agent_running() -> bool {
    imp::agent::agent_running()
}

/// L'état de l'agent : il tourne, il est arrêté, ou son fichier manque.
pub fn agent_state() -> AgentState {
    if agent_running() {
        AgentState::Running
    } else if agent_file().is_some_and(|file| file.is_file()) {
        AgentState::Stopped
    } else {
        AgentState::Missing
    }
}

/// Lance l'agent situé à côté de l'exécutable courant, sans fenêtre, avec
/// `--relacher` si demandé. Rend `Running` s'il est parti, `Missing` si le
/// fichier n'existe pas, `Stopped` s'il n'a pas pu partir.
pub fn spawn_agent(release_first: bool) -> AgentState {
    imp::agent::spawn_agent(release_first)
}

/// Poste un ordre à l'agent en cours. Faux s'il n'y en a pas.
pub fn post_agent(order: Order) -> bool {
    imp::agent::post_agent(order)
}

/// Attend que l'agent ait disparu. Vrai s'il a disparu avant la fin du délai.
pub fn wait_agent_gone(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !agent_running() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// La boucle de l'agent : une fenêtre cachée qui reçoit la minuterie et les
/// messages de Windows, et les passe à `handler` jusqu'à ce qu'il réponde
/// [`Flow::Stop`].
pub fn run_agent(handler: &mut dyn FnMut(AgentEvent) -> Flow) -> io::Result<()> {
    imp::agent::run_agent(handler)
}

/// Pose (`true`) ou retire (`false`) la valeur `Run` qui lance l'agent avec
/// Windows. C'est la seule écriture de registre de Shinku. Poser la valeur
/// quand le fichier de l'agent manque est refusé.
pub fn set_autostart(on: bool) -> io::Result<()> {
    imp::agent::set_autostart(on)
}

/// Retire la valeur `Run`, sans rien dire si elle n'y est pas.
pub fn forget_autostart() {
    // Rien à faire de plus si le registre refuse : l'agent ne démarrera
    // simplement pas son successeur.
    let _ = set_autostart(false);
}

/// Ce que dit la valeur d'approbation de la valeur `Run` de Shinku.
///
/// C'est la **même lecture stricte** que celle du module « Démarrage »
/// ([`crate::startup::approval`]) : seule la forme `03 00 00 00` + date est
/// « coupée dans Windows ». Une valeur absente, `02…`, ou un réglage que Shinku
/// ne sait pas lire (`06`, `07`, une autre longueur, un autre type, une lecture
/// refusée) ne le fait **pas** dire « coupé » : on ne prétend pas savoir.
pub fn autostart_from_approval(raw: &io::Result<Option<RawApproval>>) -> Autostart {
    match approval::read(raw, u64::MAX).state {
        State::Disabled => Autostart::DisabledInWindows,
        State::Enabled | State::Unknown => Autostart::Enabled,
    }
}

/// Où en est la valeur `Run`.
pub fn autostart() -> Autostart {
    imp::agent::autostart()
}

/// Les apps qui ont une fenêtre ouverte, une par exécutable, triées par nom.
pub fn open_apps() -> Vec<OpenProgram> {
    imp::agent::open_apps()
}

/// Ouvre une page des réglages de Windows.
pub fn open_windows_settings(page: WindowsPage) -> io::Result<()> {
    imp::agent::open_windows_settings(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_pages_have_fixed_addresses() {
        assert_eq!(WindowsPage::GameMode.uri(), "ms-settings:gaming-gamemode");
        assert_eq!(WindowsPage::Power.uri(), "ms-settings:powersleep");
        assert_eq!(WindowsPage::StartupApps.uri(), "ms-settings:startupapps");
        assert_eq!(serde_json::to_value(WindowsPage::GameMode).unwrap(), "gameMode");
        assert_eq!(serde_json::to_value(WindowsPage::Power).unwrap(), "power");
        assert_eq!(serde_json::to_value(WindowsPage::StartupApps).unwrap(), "startupApps");
        assert_eq!(serde_json::to_value(AgentState::Missing).unwrap(), "missing");
    }

    #[test]
    fn the_run_command_is_the_quoted_agent_with_its_argument() {
        let command = run_command().expect("un dossier courant");
        let file = agent_file().expect("un dossier courant");
        assert_eq!(command, format!("\"{}\" --demarrage", file.display()));
        assert!(command.starts_with('"') && command.ends_with("\" --demarrage"), "{command}");
    }

    #[test]
    fn the_autostart_reads_the_approval_as_strictly_as_the_startup_module() {
        use crate::startup::approval::{cut_bytes, restore_bytes};
        let binary =
            |bytes: &[u8]| Ok(Some(RawApproval { is_binary: true, bytes: bytes.to_vec() }));
        let cut = cut_bytes(133_760_000_000_000_000);
        // Les deux seules formes comprises.
        assert_eq!(autostart_from_approval(&binary(&cut)), Autostart::DisabledInWindows);
        assert_eq!(autostart_from_approval(&binary(&restore_bytes())), Autostart::Enabled);
        assert_eq!(autostart_from_approval(&Ok(None)), Autostart::Enabled);
        // Ce que Windows écrit parfois (`06`, `07`) : un premier octet impair ne veut
        // pas dire « coupé », et un pair non plus « actif » : on ne dit pas « coupé ».
        for first in [1u8, 5, 6, 7, 9, 0x0b, 0xff] {
            let mut bytes = [0u8; 12];
            bytes[0] = first;
            assert_eq!(
                autostart_from_approval(&binary(&bytes)),
                Autostart::Enabled,
                "{first:#x} n'est pas « coupé »"
            );
        }
        // Une autre longueur, un autre type, une lecture refusée.
        assert_eq!(autostart_from_approval(&binary(&cut[..4])), Autostart::Enabled);
        let text = RawApproval { is_binary: false, bytes: cut.to_vec() };
        assert_eq!(autostart_from_approval(&Ok(Some(text))), Autostart::Enabled);
        let refused = Err(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(autostart_from_approval(&refused), Autostart::Enabled);
    }

    #[test]
    fn without_an_agent_nothing_answers() {
        // Sur la machine de test, aucun agent ne tourne : un ordre ne va nulle part.
        if !agent_running() {
            assert!(!post_agent(Order::Release));
            assert!(wait_agent_gone(Duration::from_millis(10)));
        }
        assert_ne!(agent_state(), AgentState::Running);
        let _ = autostart();
    }
}
