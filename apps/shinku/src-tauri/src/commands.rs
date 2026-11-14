//! Les commandes que l'interface peut appeler.
//!
//! Chacune ne fait que transmettre la demande au guichet du cœur
//! ([`shinku_core::backend::Backend`]), qui décide et qui se teste sans
//! fenêtre. Aucune ne reçoit de chemin à lire ou à écrire : le seul chemin
//! accepté, dans [`remove_excluded_folder`], est comparé à la liste des
//! dossiers exclus et à rien d'autre.
//!
//! Toutes s'exécutent hors du fil de la fenêtre (`async`) : une question à un
//! disque endormi ne fige pas l'interface.
//!
//! La liste des commandes est répétée dans `app.rs`, dans `build.rs`, dans
//! `capabilities/default.json` et, côté interface, dans `backend/tauri.ts`.
//! Le test `tests/wiring.test.ts` de l'interface vérifie que tout s'accorde.

use std::path::Path;

use shinku_core::backend::{Backend, BackendError, DuplicatesPage, LargeFilesPage, Overview};
use shinku_core::scan::report::JunkReport;
use shinku_core::session::SessionView;
use shinku_core::settings::{Settings, Theme};
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

/// Arrête l'analyse en cours et oublie ses résultats.
#[tauri::command(async)]
pub fn cancel_scan(backend: State<'_, Backend>) -> SessionView {
    backend.cancel_scan()
}

/// Où en est l'analyse. L'interface le demande plusieurs fois par seconde.
#[tauri::command(async)]
pub fn session_view(backend: State<'_, Backend>) -> SessionView {
    backend.view()
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
