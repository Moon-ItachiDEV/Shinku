//! Le démarrage de l'application et sa fenêtre.

use std::process::ExitCode;

use shinku_core::backend::Backend;
use shinku_core::platform;
use shinku_core::settings::Theme;
use tauri::webview::{Color, NewWindowResponse};
use tauri::{
    AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::commands;

/// Nom de la fenêtre, celui que `capabilities/default.json` autorise.
const MAIN_WINDOW: &str = "main";

/// Taille de la fenêtre à l'ouverture, réduite d'office si l'écran est plus petit.
const DEFAULT_SIZE: (f64, f64) = (1280.0, 800.0);
/// En dessous, l'interface ne tient plus. Un portable de 13 pouces réglé à
/// 150 % n'offre que 1280 × 672 : la fenêtre doit pouvoir y entrer.
const MINIMUM_SIZE: (f64, f64) = (960.0, 600.0);

/// Couleur de fond de la fenêtre, celle de `--bg` dans `styles/tokens.css`.
/// Elle se voit un instant à l'ouverture, et sur les bords quand on agrandit
/// la fenêtre plus vite que la page ne se redessine.
const DARK_BACKGROUND: Color = Color(0x2f, 0x28, 0x42, 0xff);
const LIGHT_BACKGROUND: Color = Color(0xf4, 0xf1, 0xee, 0xff);

/// Codes de sortie quand Shinku ne peut pas démarrer.
const START_FAILED: u8 = 1;
const WEBVIEW_MISSING: u8 = 2;
const REFUSED_ELEVATED: u8 = 3;

const ELEVATED_MESSAGE: &str = "Shinku ne se lance pas en tant qu’administrateur.\n\n\
    Il n’en a pas besoin : pour les rares actions qui touchent à Windows, il demande lui-même \
    ton accord, au moment voulu.\n\n\
    Ferme ce message, puis ouvre Shinku normalement, depuis le menu Démarrer.";

const WEBVIEW_MISSING_MESSAGE: &str = "Shinku ne peut pas afficher sa fenêtre.\n\n\
    Il lui manque WebView2, un composant de Windows 11 qui a été retiré de ce PC ou qui ne \
    fonctionne plus.\n\n\
    Relance l’installeur de Shinku : il le remet en place.";

const START_FAILED_MESSAGE: &str = "Shinku n’a pas pu ouvrir sa fenêtre.\n\n\
    Tu peux réessayer. Si l’erreur revient, relance l’installeur de Shinku : il remet ses \
    fichiers en place.";

#[cfg(panic = "abort")]
const CRASH_MESSAGE: &str = "Shinku a rencontré une erreur imprévue et doit se fermer.\n\n\
    Tu peux le rouvrir tout de suite. Si l’erreur revient, le détail ci-dessous aidera à la \
    corriger.";

/// Lance Shinku et rend la main quand la fenêtre est fermée.
pub fn run() -> ExitCode {
    // Avant tout le reste : pas de moteur web dans un processus élevé, et pas
    // d'analyse du mauvais profil si l'élévation a changé de compte.
    if platform::runs_elevated() {
        tell(ELEVATED_MESSAGE);
        return ExitCode::from(REFUSED_ELEVATED);
    }
    #[cfg(panic = "abort")]
    report_crashes();
    // Vérifié ici pour le dire une seule fois, et en français : sinon Tauri
    // affiche d'abord son propre message, en anglais.
    if tauri::webview_version().is_err() {
        tell(WEBVIEW_MISSING_MESSAGE);
        return ExitCode::from(WEBVIEW_MISSING);
    }

    let built = tauri::Builder::default()
        // Une seule fenêtre à la fois : un second lancement ramène la première
        // au premier plan. Les arguments du second lancement sont ignorés.
        .plugin(tauri_plugin_single_instance::init(|app, _arguments, _folder| {
            bring_to_front(app);
        }))
        .manage(Backend::for_machine())
        .invoke_handler(tauri::generate_handler![
            commands::overview,
            commands::acknowledge_settings_issue,
            commands::start_scan,
            commands::cancel_scan,
            commands::session_view,
            commands::clean_preview,
            commands::start_clean,
            commands::cancel_clean,
            commands::keep_duplicate,
            commands::junk_details,
            commands::large_files_page,
            commands::duplicates_page,
            commands::set_theme,
            commands::add_excluded_folder,
            commands::remove_excluded_folder,
            commands::game_view,
            commands::set_game_option,
            commands::set_quiet_app,
            commands::open_apps,
            commands::add_game,
            commands::add_quiet_app,
            commands::set_game_status,
            commands::release_game,
            commands::startup_view,
            commands::set_startup_entry,
            commands::apps_view,
            commands::start_uninstall,
            commands::stop_uninstall,
            commands::rescan_leftovers,
            commands::remove_leftovers,
            commands::open_windows_settings,
        ])
        .setup(|app| {
            // Une erreur rendue ici ferait paniquer Tauri, donc fermerait
            // Shinku sans un mot : on l'explique, puis on quitte.
            if let Err(error) = open_window(app.handle()) {
                could_not_start(&error);
            }
            // Met l'agent du mode jeu et la valeur `Run` d'accord avec les
            // réglages. Hors du fil de la fenêtre : lancer un programme et
            // écrire dans le registre ne doivent jamais retarder son ouverture.
            let handle = app.handle().clone();
            std::thread::spawn(move || handle.state::<Backend>().sync_agent());
            Ok(())
        })
        .build(tauri::generate_context!());

    let app = match built {
        Ok(app) => app,
        Err(error) => could_not_start(&error),
    };
    // Une fois la fenêtre ouverte, le processus se termine à sa fermeture :
    // on ne revient pas de cet appel.
    app.run(|app, event| {
        if matches!(event, RunEvent::Exit) {
            // La fenêtre vient de se fermer. Un grand vide en cours finit son
            // fichier et note son bilan au journal avant que Shinku s'arrête.
            app.state::<Backend>().shutdown();
        }
    });
    ExitCode::SUCCESS
}

/// Explique que Shinku n'a pas pu s'ouvrir, puis quitte.
fn could_not_start(error: &tauri::Error) -> ! {
    tell(&format!("{START_FAILED_MESSAGE}\n\nDétail : {error}"));
    std::process::exit(i32::from(START_FAILED))
}

/// En version finale, il n'y a pas de console et une erreur imprévue arrête
/// le programme sur-le-champ : sans ce message, Shinku disparaîtrait de
/// l'écran sans explication.
#[cfg(panic = "abort")]
fn report_crashes() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let detail = info.to_string();
        // Depuis un autre fil : celui qui vient d'échouer est peut-être en
        // train de traiter un message de la fenêtre.
        let shown =
            std::thread::spawn(move || tell(&format!("{CRASH_MESSAGE}\n\nDétail : {detail}")));
        let _ = shown.join();
    }));
}

/// Affiche un message de Windows, sans fenêtre de Shinku derrière.
fn tell(message: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Info)
        .set_title("Shinku")
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// Crée la fenêtre : sans cadre de Windows (l'interface dessine sa barre de
/// titre), à la couleur du mode d'affichage, et fermée à tout ce qui n'est
/// pas l'interface.
fn open_window(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
        .title("Shinku")
        .inner_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
        .min_inner_size(MINIMUM_SIZE.0, MINIMUM_SIZE.1)
        .prevent_overflow()
        .center()
        .decorations(false)
        .shadow(true)
        // Cachée le temps de prendre sa couleur : pas d'éclair blanc à l'ouverture.
        .visible(false)
        .on_navigation(stays_in_the_interface)
        // L'interface n'ouvre jamais d'autre fenêtre.
        .on_new_window(|_url, _features| NewWindowResponse::Deny)
        .build()?;

    paint_background(&window, app.state::<Backend>().theme());
    window.show()?;
    window.set_focus()?;

    // En mode « Système », le fond suit Windows quand il change de mode.
    let follower = window.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::ThemeChanged(_)) {
            paint_background(&follower, follower.state::<Backend>().theme());
        }
    });
    Ok(())
}

/// Vrai si cette adresse est une page de l'interface. Toute autre navigation
/// est refusée : la fenêtre n'affiche jamais de contenu venu d'ailleurs.
fn stays_in_the_interface(url: &Url) -> bool {
    match (url.scheme(), url.host_str()) {
        // L'interface livrée avec l'application.
        ("http" | "https", Some("tauri.localhost")) | ("tauri", Some("localhost")) => true,
        // La page vide du moteur web.
        ("about", None) => url.path() == "blank",
        // Pendant le développement, l'interface servie par Vite.
        ("http", Some("127.0.0.1")) => cfg!(dev) && url.port() == Some(5173),
        _ => false,
    }
}

/// Donne à la fenêtre le fond du mode d'affichage choisi.
pub fn paint_background(window: &WebviewWindow, theme: Theme) {
    let dark = match theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => window.theme().is_ok_and(|system| system == tauri::Theme::Dark),
    };
    let color = if dark { DARK_BACKGROUND } else { LIGHT_BACKGROUND };
    // Sans effet sur le fonctionnement : en cas d'échec, la fenêtre garde son fond.
    let _ = window.set_background_color(Some(color));
}

/// Ramène la fenêtre au premier plan.
fn bring_to_front(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
