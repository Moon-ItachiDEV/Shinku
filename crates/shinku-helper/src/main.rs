//! `shinku-helper.exe` : l'assistant élevé de Shinku.
//!
//! C'est le seul exécutable de Shinku qui tourne en administrateur, et
//! seulement le temps d'une session de nettoyage. Il n'accepte que les verbes
//! de [`shinku_core::helper`], refait sa propre analyse avec ses propres
//! règles et ne touche jamais au profil d'un utilisateur.
//!
//! À cette étape, il ne sait que dire ce qu'il acceptera : aucune action
//! élevée n'est encore écrite. Le canal nommé et le nettoyage du système
//! arrivent avec l'exécuteur.

use std::process::ExitCode;

use shinku_core::helper::Verb;

/// Code de sortie d'une demande refusée.
const REFUSED: u8 = 2;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    match (arguments.next().as_deref(), arguments.next()) {
        (Some("--verbes"), None) => {
            for verb in Verb::ALL {
                println!("{verb}");
            }
            ExitCode::SUCCESS
        }
        (Some("--version"), None) => {
            println!("shinku-helper {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("shinku-helper : aucune action élevée n'est disponible dans cette version.");
            ExitCode::from(REFUSED)
        }
    }
}
