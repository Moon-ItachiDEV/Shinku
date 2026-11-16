//! `shinku-helper.exe` : l'assistant élevé de Shinku.
//!
//! C'est le seul exécutable de Shinku qui tourne en administrateur, et
//! seulement le temps d'un changement : un processus par changement, pas de
//! canal, pas d'assistant qui reste en marche. Il fait ce que dit sa ligne de
//! commande (quatre mots, voir [`shinku_core::helper::request`]), après avoir
//! montré lui-même quelle entrée il va changer, puis sort avec un code
//! ([`shinku_core::helper::startup::Code`]).
//!
//! Il n'a **pas** de fenêtre noire (`windows_subsystem = "windows"`) : il n'écrit
//! rien dans une console, donc plus de `--verbes` ni de `--version`. Tout le
//! travail est dans le cœur ; ce fichier ne fait que durcir, brancher, sortir.
#![windows_subsystem = "windows"]

use std::process::ExitCode;

fn main() -> ExitCode {
    // Avant tout : ne charger de bibliothèque que depuis System32.
    if !shinku_core::platform::harden_helper_process() {
        return ExitCode::from(shinku_core::helper::startup::Code::Failed as u8);
    }
    let arguments = shinku_core::platform::helper_arguments();
    ExitCode::from(shinku_core::helper::run_command_line(&arguments))
}
