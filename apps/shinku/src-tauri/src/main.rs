//! `shinku.exe` : la fenêtre de Shinku.
//!
//! Ce programme affiche l'interface (une page web locale, dans WebView2) et
//! transmet ses demandes au cœur, [`shinku_core`]. Il ne contient aucune
//! logique de nettoyage : tout ce qu'il sait faire tient dans les trente-trois
//! commandes de `commands.rs`, qui appellent le guichet du cœur.
//!
//! Il tourne toujours avec les droits de l'utilisateur, jamais en
//! administrateur : un moteur web n'a rien à faire dans un processus élevé.
//!
//! Une seule autre forme existe : `shinku.exe --oublier`, que le désinstalleur
//! lance pour effacer les réglages (voir `shinku_core::forget`). Elle est
//! reconnue **en tout premier**, dans `main`, avant que le moindre élément de
//! la fenêtre existe : sa fonction ne reçoit rien de Tauri, et ni le contrôle
//! d'élévation (avec sa boîte), ni le rapport de plantage, ni WebView2, ni le
//! constructeur de Tauri, ni le module d'instance unique ne sont atteints.

// En version finale, pas de console derrière la fenêtre.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod commands;

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    // Tout premier geste : `--oublier` ne démarre rien de la fenêtre. Aucune
    // fenêtre, aucune boîte, rien d'écrit sur le disque, quel que soit le
    // résultat ; le code de sortie dit ce qui s'est passé.
    if shinku_core::forget::asked(std::env::args_os()) {
        let outcome = shinku_core::forget::run(&shinku_core::platform::forget_host());
        return ExitCode::from(outcome.exit_code());
    }
    app::run()
}

/// Shinku ne vise que Windows. Ailleurs, ce programme existe seulement pour
/// que tout l'espace de travail se compile et se teste.
#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("Shinku ne fonctionne que sous Windows 11.");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    /// `main` reconnaît `--oublier` avant d'appeler `app::run`, qui contient tout
    /// le reste (élévation et sa boîte, rapport de plantage, WebView2, Tauri,
    /// instance unique). Cet essai lit la source : il garde l'ordre quand un
    /// geste est ajouté à `main`. La preuve en conditions réelles est
    /// `essai-oublier` (voir `docs/conception.md`).
    #[test]
    fn the_forget_word_is_recognised_before_anything_of_the_window() {
        let source = include_str!("main.rs");
        let main = source.split("#[cfg(windows)]\nfn main()").nth(1).unwrap_or_default();
        let body = main.split("\n}\n").next().unwrap_or_default();
        let forget = body.find("shinku_core::forget::asked(").expect("main reconnaît --oublier");
        let window = body.find("app::run()").expect("main lance la fenêtre");
        assert!(forget < window, "--oublier doit être reconnu avant app::run()");
        // Avant le premier geste de main, rien d'autre que des commentaires.
        let first_code = body.lines().skip(1).find(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with("//")
        });
        assert!(first_code.is_some_and(|line| line.contains("forget::asked(")), "{first_code:?}");
    }
}
