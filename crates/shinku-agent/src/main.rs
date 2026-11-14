//! `shinku-agent.exe` : l'agent de Shinku, lancé avec Windows.
//!
//! Il n'a ni fenêtre ni droits administrateur. Son rôle : l'analyse planifiée,
//! l'alerte de la corbeille, puis la proposition du mode jeu. Il attend deux
//! minutes après l'ouverture de session avant de faire quoi que ce soit.
//!
//! À cette étape, il ne fait rien et s'arrête aussitôt : il arrive avec
//! l'analyse planifiée.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    if let (Some("--version"), None) = (arguments.next().as_deref(), arguments.next()) {
        println!("shinku-agent {}", env!("CARGO_PKG_VERSION"));
    }
    ExitCode::SUCCESS
}
