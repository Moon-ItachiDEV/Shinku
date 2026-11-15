//! `shinku.exe` : la fenêtre de Shinku.
//!
//! Ce programme affiche l'interface (une page web locale, dans WebView2) et
//! transmet ses demandes au cœur, [`shinku_core`]. Il ne contient aucune
//! logique de nettoyage : tout ce qu'il sait faire tient dans les quinze
//! commandes de `commands.rs`, qui appellent le guichet du cœur.
//!
//! Il tourne toujours avec les droits de l'utilisateur, jamais en
//! administrateur : un moteur web n'a rien à faire dans un processus élevé.

// En version finale, pas de console derrière la fenêtre.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod commands;

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    app::run()
}

/// Shinku ne vise que Windows. Ailleurs, ce programme existe seulement pour
/// que tout l'espace de travail se compile et se teste.
#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("Shinku ne fonctionne que sous Windows 11.");
    ExitCode::FAILURE
}
