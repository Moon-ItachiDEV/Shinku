//! Les commandes que l'interface peut appeler.
//!
//! Chacune ne fait que transmettre la demande au guichet du cœur
//! ([`shinku_core::backend::Backend`]), qui décide et qui se teste sans
//! fenêtre. Aucune ne reçoit de chemin à lire, à écrire ou à supprimer : le
//! seul chemin accepté, dans [`remove_excluded_folder`], est comparé à la
//! liste des dossiers exclus et à rien d'autre. Même [`start_clean`] ne reçoit
//! que trois cases cochées et la date de l'analyse affichée : ce qui part est
//! décidé par le cœur.
//!
//! Toutes s'exécutent hors du fil de la fenêtre (`async`) : une question à un
//! disque endormi ne fige pas l'interface.
//!
//! La liste des commandes est répétée dans `app.rs`, dans `build.rs`, dans
//! `capabilities/default.json` et, côté interface, dans `backend/tauri.ts`.
//! Le test `tests/wiring.test.ts` de l'interface vérifie que tout s'accorde.

use std::path::Path;

use shinku_core::apps::view::{AppsView, RemoveLeftovers};
use shinku_core::backend::{Backend, BackendError, DuplicatesPage, LargeFilesPage, Overview};
use shinku_core::clean::{CleanPreview, CleanRequest};
use shinku_core::game::view::{GameOption, GameStatus, GameView, OpenApp};
use shinku_core::platform::agent::WindowsPage;
use shinku_core::scan::report::{JunkReport, KeepChoice};
use shinku_core::session::SessionView;
use shinku_core::settings::{Settings, Theme};
use shinku_core::startup::view::StartupView;
use tauri::{State, WebviewWindow};

use crate::app::paint_background;

/// Ce que la fenêtre affiche dès son ouverture : disques, corbeille, réglages.
#[tauri::command(async)]
pub fn overview(backend: State<'_, Backend>) -> Overview {
    backend.overview()
}

/// L'utilisateur a lu l'avertissement sur ses réglages.
#[tauri::command(async)]
pub fn acknowledge_settings_issue(backend: State<'_, Backend>) -> Overview {
    backend.acknowledge_settings_issue();
    backend.overview()
}

/// Lance une analyse.
#[tauri::command(async)]
pub fn start_scan(backend: State<'_, Backend>) -> Result<SessionView, BackendError> {
    backend.start_scan()
}

/// Arrête l'analyse en cours et oublie ses résultats, ou referme le bilan
/// d'un grand vide terminé.
#[tauri::command(async)]
pub fn cancel_scan(backend: State<'_, Backend>) -> SessionView {
    backend.cancel_scan()
}

/// Où en est l'analyse ou le grand vide. L'interface le demande plusieurs
/// fois par seconde.
#[tauri::command(async)]
pub fn session_view(backend: State<'_, Backend>) -> SessionView {
    backend.view()
}

/// Ce que l'écran de confirmation doit dire avant un grand vide.
#[tauri::command(async)]
pub fn clean_preview(backend: State<'_, Backend>) -> Result<CleanPreview, BackendError> {
    backend.clean_preview()
}

/// Lance le grand vide que l'utilisateur vient de confirmer. C'est la seule
/// commande qui supprime.
#[tauri::command(async)]
pub fn start_clean(
    backend: State<'_, Backend>,
    request: CleanRequest,
) -> Result<SessionView, BackendError> {
    backend.start_clean(request)
}

/// Demande l'arrêt du grand vide en cours : le fichier en cours est mené à
/// son terme, puis le bilan partiel arrive.
#[tauri::command(async)]
pub fn cancel_clean(backend: State<'_, Backend>) -> SessionView {
    backend.cancel_clean()
}

/// Enregistre l'exemplaire à garder d'un doublon : son groupe et son rang,
/// jamais un chemin. Faux si le choix ne désigne rien.
#[tauri::command(async)]
pub fn keep_duplicate(backend: State<'_, Backend>, choice: KeepChoice) -> bool {
    backend.keep_duplicate(&choice)
}

/// Le détail des fichiers inutiles.
#[tauri::command(async)]
pub fn junk_details(backend: State<'_, Backend>) -> Option<JunkReport> {
    backend.junk()
}

/// Une page de gros fichiers.
#[tauri::command(async)]
pub fn large_files_page(
    backend: State<'_, Backend>,
    offset: usize,
    limit: usize,
) -> Option<LargeFilesPage> {
    backend.large_files(offset, limit)
}

/// Une page de groupes de doublons.
#[tauri::command(async)]
pub fn duplicates_page(
    backend: State<'_, Backend>,
    offset: usize,
    limit: usize,
) -> Option<DuplicatesPage> {
    backend.duplicates(offset, limit)
}

/// Change le mode d'affichage, et le fond de la fenêtre avec lui.
#[tauri::command(async)]
pub fn set_theme(
    window: WebviewWindow,
    backend: State<'_, Backend>,
    theme: Theme,
) -> Result<Settings, BackendError> {
    let settings = backend.set_theme(theme)?;
    paint_background(&window, settings.theme);
    Ok(settings)
}

/// Ouvre le sélecteur de dossier de Windows et ajoute le dossier choisi aux
/// « dossiers jamais touchés ». Rend `None` si l'utilisateur annule.
///
/// L'interface ne fournit pas le chemin : c'est l'utilisateur qui le désigne,
/// dans une boîte de dialogue de Windows.
#[tauri::command]
pub async fn add_excluded_folder(
    window: WebviewWindow,
    backend: State<'_, Backend>,
) -> Result<Option<Settings>, BackendError> {
    // La boîte de dialogue tourne sur son propre fil : la fenêtre reste
    // dessinée derrière elle, et inactive tant qu'elle est ouverte.
    let picked = rfd::AsyncFileDialog::new()
        .set_title("Choisir un dossier à ne jamais toucher")
        .set_parent(&window)
        .pick_folder()
        .await;
    match picked {
        Some(folder) => backend.exclude_folder(folder.path()).map(Some),
        None => Ok(None),
    }
}

/// Retire un dossier des « dossiers jamais touchés ».
#[tauri::command(async)]
pub fn remove_excluded_folder(
    backend: State<'_, Backend>,
    path: String,
) -> Result<Settings, BackendError> {
    backend.include_folder(Path::new(&path))
}

/// Tout ce que la page « Mode jeu » affiche.
#[tauri::command(async)]
pub fn game_view(backend: State<'_, Backend>) -> GameView {
    backend.game_view()
}

/// Allume ou éteint un interrupteur du mode jeu.
#[tauri::command(async)]
pub fn set_game_option(
    backend: State<'_, Backend>,
    option: GameOption,
    on: bool,
) -> Result<GameView, BackendError> {
    backend.set_game_option(option, on)
}

/// Met une app de la liste en veilleuse pendant les parties, ou la laisse
/// tranquille. `exe` n'est que comparé aux apps connues.
#[tauri::command(async)]
pub fn set_quiet_app(
    backend: State<'_, Backend>,
    exe: String,
    on: bool,
) -> Result<GameView, BackendError> {
    backend.set_quiet_app(&exe, on)
}

/// Les apps ouvertes en ce moment, pour les sélecteurs de la page.
#[tauri::command(async)]
pub fn open_apps(backend: State<'_, Backend>) -> Vec<OpenApp> {
    backend.open_apps()
}

/// Ajoute aux jeux l'app de ce numéro, dans la dernière liste rendue par
/// [`open_apps`]. `path` doit être exactement son chemin dans cette liste : il
/// n'est que comparé (si la liste a changé, la demande est refusée).
#[tauri::command(async)]
pub fn add_game(
    backend: State<'_, Backend>,
    id: u32,
    path: String,
) -> Result<GameView, BackendError> {
    backend.add_game(id, &path)
}

/// Ajoute aux apps en veilleuse l'app de ce numéro, dans la dernière liste
/// rendue par [`open_apps`]. Même règle pour `path`.
#[tauri::command(async)]
pub fn add_quiet_app(
    backend: State<'_, Backend>,
    id: u32,
    path: String,
) -> Result<GameView, BackendError> {
    backend.add_quiet_app(id, &path)
}

/// Dit ce que la personne pense d'un jeu de la liste. `path` n'est que
/// comparé aux jeux connus.
#[tauri::command(async)]
pub fn set_game_status(
    backend: State<'_, Backend>,
    path: String,
    status: GameStatus,
) -> Result<GameView, BackendError> {
    backend.set_game_status(&path, status)
}

/// « Tout remettre maintenant » : l'agent rend à Windows ce qu'il a changé.
#[tauri::command(async)]
pub fn release_game(backend: State<'_, Backend>) -> GameView {
    backend.release_game()
}

/// Tout ce que la page « Démarrage » affiche : les apps inscrites au démarrage
/// de Windows, pour le compte et pour le PC.
#[tauri::command(async)]
pub fn startup_view(backend: State<'_, Backend>) -> StartupView {
    backend.startup_view()
}

/// Coupe ou remet une app inscrite au démarrage du compte. `id` est celui que
/// la page a reçu dans [`startup_view`] : le cœur ne s'en sert que pour
/// retrouver l'entrée, la page n'envoie jamais un chemin ni un nom de valeur.
#[tauri::command(async)]
pub fn set_startup_entry(
    backend: State<'_, Backend>,
    id: String,
    enabled: bool,
) -> Result<StartupView, BackendError> {
    backend.set_startup_entry(&id, enabled)
}

/// Tout ce que la page « Désinstallation » affiche : les apps installées et où
/// en est la désinstallation en cours. La page la rappelle toutes les deux
/// secondes pendant qu'elle attend.
#[tauri::command(async)]
pub fn apps_view(backend: State<'_, Backend>) -> AppsView {
    backend.apps_view()
}

/// Lance le désinstalleur de l'app de cet identifiant. La commande est relue
/// dans le registre : la page n'envoie jamais un chemin. Ne rend la main
/// qu'après la réponse à la fenêtre de Windows.
#[tauri::command(async)]
pub fn start_uninstall(backend: State<'_, Backend>, id: String) -> Result<AppsView, BackendError> {
    backend.start_uninstall(&id)
}

/// Revient à la liste. Ne tue rien, ne touche à rien.
#[tauri::command(async)]
pub fn stop_uninstall(backend: State<'_, Backend>) -> AppsView {
    backend.stop_uninstall()
}

/// Cherche de nouveau ce que l'app désinstallée a laissé.
#[tauri::command(async)]
pub fn rescan_leftovers(backend: State<'_, Backend>) -> Result<AppsView, BackendError> {
    backend.rescan_leftovers()
}

/// Met à la corbeille les restes de ces clés, et rien d'autre.
#[tauri::command(async)]
pub fn remove_leftovers(
    backend: State<'_, Backend>,
    keys: Vec<u32>,
) -> Result<RemoveLeftovers, BackendError> {
    backend.remove_leftovers(&keys)
}

/// Ouvre l'une des quatre pages des réglages de Windows que la page propose.
#[tauri::command(async)]
pub fn open_windows_settings(backend: State<'_, Backend>, page: WindowsPage) {
    backend.open_windows_settings(page);
}
