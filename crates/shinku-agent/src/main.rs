//! `shinku-agent.exe` : l'agent de Shinku, lancé avec Windows ou par la fenêtre.
//!
//! Il n'a ni fenêtre visible, ni console, ni droits administrateur. Son rôle
//! aujourd'hui : le mode jeu. Toutes les deux secondes, il regarde la fenêtre
//! au premier plan ; quand un jeu s'y installe, il applique les réglages de la
//! personne, puis les remet en place à la fin de la partie.
//!
//! Arguments :
//!
//! - aucun : démarrage depuis la fenêtre de Shinku ;
//! - `--demarrage` : lancé par Windows à l'ouverture de session (deux minutes
//!   de calme avant de détecter quoi que ce soit) ;
//! - `--relacher` : démarre en remettant d'abord en place une partie restée au
//!   journal ;
//! - `--quitter` : demande à l'agent qui tourne de s'arrêter (installeur) ;
//! - `--retirer` : lui demande de s'arrêter et de ne plus se lancer avec
//!   Windows (désinstallation) ;
//! - `--version`.
//!
//! Tout autre argument est refusé (code 2). Lancé avec des droits
//! d'administrateur, il s'arrête aussitôt sans rien dire (code 3) : un agent
//! élevé n'aurait pas les mêmes droits que les jeux de la personne.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;
use std::time::Duration;

use shinku_core::game::agent::{Agent, Flow};
use shinku_core::game::store::GameStore;
use shinku_core::platform::agent::{
    Order, acquire_instance, forget_autostart, post_agent, run_agent, wait_agent_gone,
};
use shinku_core::platform::{self, Environment};
use shinku_core::settings::SettingsStore;

/// Argument inconnu.
const REFUSED: u8 = 2;
/// Lancé avec des droits d'administrateur.
const ELEVATED: u8 = 3;
/// Quelque chose d'indispensable manque (dossier de données, boucle de messages).
const FAILED: u8 = 1;
/// Temps laissé à l'agent pour s'arrêter quand on le lui demande.
const STOP_WAIT: Duration = Duration::from_secs(10);

/// Ce qu'on a demandé à ce programme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    /// Veiller.
    Watch {
        /// Lancé par Windows : pas de détection pendant deux minutes.
        delayed: bool,
        /// Remettre d'abord en place la partie du journal.
        release_first: bool,
    },
    /// Donner un ordre à l'agent qui tourne.
    Order(Order),
    /// Dire sa version.
    Version,
}

/// Lit les arguments : aucun, ou un seul, parmi ceux qu'on connaît.
fn parse(mut arguments: impl Iterator<Item = String>) -> Option<Command> {
    let first = arguments.next();
    if arguments.next().is_some() {
        return None;
    }
    Some(match first.as_deref() {
        None => Command::Watch { delayed: false, release_first: false },
        Some("--demarrage") => Command::Watch { delayed: true, release_first: false },
        Some("--relacher") => Command::Watch { delayed: false, release_first: true },
        Some("--quitter") => Command::Order(Order::Quit),
        Some("--retirer") => Command::Order(Order::Remove),
        Some("--version") => Command::Version,
        Some(_) => return None,
    })
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        None => ExitCode::from(REFUSED),
        Some(Command::Version) => {
            println!("shinku-agent {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(Command::Order(order)) => give(order),
        Some(Command::Watch { delayed, release_first }) => watch(delayed, release_first),
    }
}

/// Donne un ordre à l'agent qui tourne et attend qu'il ait fini.
fn give(order: Order) -> ExitCode {
    if !post_agent(order) {
        // Personne à qui parler. Pour une désinstallation, la valeur `Run`
        // ne doit pas rester pour autant.
        if order == Order::Remove {
            forget_autostart();
        }
        return ExitCode::SUCCESS;
    }
    if wait_agent_gone(STOP_WAIT) { ExitCode::SUCCESS } else { ExitCode::from(FAILED) }
}

/// Veille sur les jeux jusqu'à ce qu'on lui dise d'arrêter.
fn watch(delayed: bool, release_first: bool) -> ExitCode {
    if platform::runs_elevated() {
        return ExitCode::from(ELEVATED);
    }
    // Un seul agent par session : le second s'efface sans bruit.
    let Some(_instance) = acquire_instance() else { return ExitCode::SUCCESS };
    let environment = platform::system();
    let (Some(settings), Some(game)) = (
        SettingsStore::for_machine(&environment as &dyn Environment),
        GameStore::for_machine(&environment),
    ) else {
        return ExitCode::from(FAILED);
    };
    let (mut agent, flow) =
        Agent::start(platform::game_host(), settings, game, delayed, release_first);
    if flow == Flow::Stop {
        return ExitCode::SUCCESS;
    }
    match run_agent(&mut |event| agent.handle(event)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(FAILED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arguments: &[&str]) -> Option<Command> {
        parse(arguments.iter().map(|argument| (*argument).to_owned()))
    }

    #[test]
    fn the_known_arguments_are_understood() {
        assert_eq!(parsed(&[]), Some(Command::Watch { delayed: false, release_first: false }));
        assert_eq!(
            parsed(&["--demarrage"]),
            Some(Command::Watch { delayed: true, release_first: false })
        );
        assert_eq!(
            parsed(&["--relacher"]),
            Some(Command::Watch { delayed: false, release_first: true })
        );
        assert_eq!(parsed(&["--quitter"]), Some(Command::Order(Order::Quit)));
        assert_eq!(parsed(&["--retirer"]), Some(Command::Order(Order::Remove)));
        assert_eq!(parsed(&["--version"]), Some(Command::Version));
    }

    #[test]
    fn anything_else_is_refused() {
        assert_eq!(parsed(&["--inconnu"]), None);
        assert_eq!(parsed(&["--quitter", "--retirer"]), None);
        assert_eq!(parsed(&["--demarrage", "x"]), None);
        assert_eq!(parsed(&[""]), None);
        assert_eq!(parsed(&["--Quitter"]), None);
    }
}
