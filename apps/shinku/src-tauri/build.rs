//! Prépare la compilation de la fenêtre de Shinku : permissions des commandes
//! et ressources Windows (icône, manifeste, numéro de version).

/// Les commandes que la fenêtre peut appeler, une par fonction de
/// `src/commands.rs`. Tauri en tire une permission `allow-…` par commande, et
/// `capabilities/default.json` dit lesquelles la fenêtre reçoit. Une commande
/// absente d'une de ces listes est refusée à l'exécution : le test
/// `tests/wiring.test.ts` de l'interface vérifie qu'elles disent la même chose.
const COMMANDS: &[&str] = &[
    "overview",
    "acknowledge_settings_issue",
    "start_scan",
    "cancel_scan",
    "session_view",
    "junk_details",
    "large_files_page",
    "duplicates_page",
    "set_theme",
    "add_excluded_folder",
    "remove_excluded_folder",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=windows-manifest.xml");

    // Shinku ne vise que Windows. Ailleurs, ce paquet n'est qu'un programme
    // qui le dit : il n'y a rien à préparer.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }

    let windows =
        tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-manifest.xml"));
    let attributes = tauri_build::Attributes::new()
        .windows_attributes(windows)
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if let Err(error) = tauri_build::try_build(attributes) {
        panic!("la préparation de l'application Tauri a échoué : {error:#}");
    }
}
