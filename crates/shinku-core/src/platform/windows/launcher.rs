//! Le lanceur de l'assistant élevé, côté `shinku.exe` ([`ElevatedHelper`]).
//!
//! `shinku.exe` ne s'élève jamais lui-même. Avant de lancer, il refait la
//! vérification d'emplacement de l'assistant ([`open_helper`]) : le fichier est
//! `shinku-helper.exe` dans le dossier de `shinku.exe`, sûr, et son dossier est
//! l'enfant direct de `Program Files`. **La poignée du fichier reste ouverte**
//! (accès aux données, partage en lecture seule), du contrôle jusqu'au retour de
//! `ShellExecuteExW` : personne ne peut réécrire ni supprimer le fichier entre
//! les deux. C'est un garde-fou contre nos erreurs, pas une barrière (voir
//! `crate::helper`, section 0). C'est le **chemin final vérifié** qui est lancé.
//! Que la poignée tienne jusqu'au retour de `ShellExecuteExW` n'est tenu que par
//! relecture : aucun essai ne le voit.
//!
//! Le lancement : verbe `runas`, la fenêtre de Shinku en propriétaire (pour que
//! l'accord de Windows s'ouvre devant), dossier de travail `System32`,
//! `SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI`. L'attente de
//! la fin est bornée : dix minutes à compter du retour de `ShellExecuteExW` (un
//! filet : l'assistant a sa propre échéance de deux minutes).
#![allow(unsafe_code)]

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::time::Duration;

use windows::Win32::Foundation::{ERROR_CANCELLED, HANDLE, HWND, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HRESULT, HSTRING, PCWSTR};

use super::helper_host::{common_folder, open_helper};
use super::security;
use crate::helper::location::HELPER_FILE;
use crate::startup::access::{self, AccessFacts, PcAccess};
use crate::startup::launcher::{ElevatedHelper, HelperRun};

/// Combien de temps on attend l'assistant, à compter du retour de `ShellExecuteExW`.
const WAIT: Duration = Duration::from_secs(600);

/// `HKLM\…\Policies\System` : `EnableLUA` et `ConsentPromptBehaviorUser`.
const POLICIES_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";

/// L'assistant élevé, lancé par Windows.
#[derive(Debug, Default)]
pub(crate) struct SystemHelper {
    /// Un autre fichier que celui qui est à côté de `shinku.exe` (pour les essais).
    path: Option<PathBuf>,
}

impl SystemHelper {
    /// Le lanceur de la vraie installation : `shinku-helper.exe` à côté de `shinku.exe`.
    pub(crate) fn new() -> Self {
        Self { path: None }
    }

    /// Un lanceur sur ce fichier (pour les essais sur une installation donnée).
    #[cfg(test)]
    pub(crate) fn at(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn helper_path(&self) -> Option<PathBuf> {
        match &self.path {
            Some(path) => Some(path.clone()),
            None => Some(std::env::current_exe().ok()?.parent()?.join(HELPER_FILE)),
        }
    }
}

/// Une valeur `DWORD` de `Policies\System`, lue sans rien écrire.
fn policy(name: &str) -> Option<u32> {
    let key = super::startup::open_key(
        super::startup::Hive::Local,
        POLICIES_KEY,
        windows::Win32::System::Registry::KEY_READ,
    )
    .ok()??;
    super::apps::dword(&key, name, &mut false).ok()?
}

/// Le chemin sous une forme que `ShellExecuteExW` comprend (sans `\\?\`).
fn launchable(final_path: &str) -> String {
    match final_path.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => rest.to_owned(),
        _ => final_path.to_owned(),
    }
}

/// Le dossier `System32`.
fn system_directory() -> Option<String> {
    let mut buffer = [0u16; 260];
    // SAFETY: `buffer` est un tampon local ; la taille donnée est la sienne.
    let length = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    (length > 0 && length < buffer.len()).then(|| String::from_utf16_lossy(&buffer[..length]))
}

/// Lance `file` avec ces paramètres et attend sa fin, au plus `wait`. `kept` est
/// la poignée du fichier vérifié : elle reste ouverte **jusqu'au retour de
/// `ShellExecuteExW`**, pas pendant l'attente de la fin de l'assistant.
fn launch(
    file: &str,
    parameters: &str,
    owner: isize,
    wait: Duration,
    kept: Option<security::Opened>,
) -> HelperRun {
    // `ShellExecuteExW` demande COM sur le fil appelant (comme la Désinstallation).
    let _com = super::startup::ComScope::enter();
    let verb = HSTRING::from("runas");
    let file = HSTRING::from(file);
    let parameters = HSTRING::from(parameters);
    let directory = system_directory().map(|text| HSTRING::from(text.as_str()));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        hwnd: HWND(owner as *mut std::ffi::c_void),
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        lpDirectory: directory.as_ref().map_or(PCWSTR::null(), |text| PCWSTR(text.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..SHELLEXECUTEINFOW::default()
    };
    // SAFETY: `info` est entièrement initialisée ; les chaînes qu'elle désigne
    // (`verb`, `file`, `parameters`, `directory`) vivent jusqu'à la fin de la
    // fonction ; `hwnd` est la fenêtre de Shinku, ou nulle.
    let started = unsafe { ShellExecuteExW(&raw mut info) };
    drop(kept);
    if let Err(error) = started {
        return if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
            HelperRun::Cancelled
        } else {
            HelperRun::NotStarted
        };
    }
    if info.hProcess.is_invalid() {
        return HelperRun::NotStarted;
    }
    // SAFETY: `SEE_MASK_NOCLOSEPROCESS` nous donne un handle de processus
    // ouvert, qui n'appartient qu'à cette fonction : `OwnedHandle` le fermera.
    let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess.0) };
    let handle = HANDLE(process.as_raw_handle());
    let milliseconds = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: `handle` est un processus ouvert, vivant autant que `process`.
    let waited = unsafe { WaitForSingleObject(handle, milliseconds) };
    if waited == WAIT_TIMEOUT {
        return HelperRun::TimedOut;
    }
    if waited != WAIT_OBJECT_0 {
        return HelperRun::TimedOut;
    }
    let mut code = 0u32;
    // SAFETY: `handle` est un processus terminé et ouvert ; `code` est locale.
    if unsafe { GetExitCodeProcess(handle, &raw mut code) }.is_err() {
        return HelperRun::TimedOut;
    }
    HelperRun::Exited(code)
}

impl ElevatedHelper for SystemHelper {
    fn access(&self) -> PcAccess {
        let helper_safe = self.helper_path().and_then(|path| open_helper(&path)).is_some();
        access::decide(&AccessFacts {
            helper_safe,
            elevation: security::elevation_type(),
            admin_enabled: security::administrators_enabled(),
            enable_lua: policy("EnableLUA"),
            consent_user: policy("ConsentPromptBehaviorUser"),
        })
    }

    fn folder_safe(&self) -> bool {
        common_folder().is_some()
    }

    fn run(&self, words: &[String; 4], owner: isize) -> HelperRun {
        let Some(path) = self.helper_path() else { return HelperRun::NotStarted };
        // Le contrôle, poignée gardée ouverte : refus avant tout lancement.
        let Some(kept) = open_helper(&path) else { return HelperRun::NotStarted };
        let file = launchable(&kept.placed.path);
        let parameters = words.join(" ");
        launch(&file, &parameters, owner, WAIT, Some(kept))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper::request::{StartupAction, StartupRequest, StartupSlot};

    #[test]
    fn a_final_path_is_given_to_the_shell_without_its_prefix() {
        assert_eq!(
            launchable(r"\\?\C:\Program Files\Shinku\shinku-helper.exe"),
            r"C:\Program Files\Shinku\shinku-helper.exe"
        );
        assert_eq!(launchable(r"\\?\UNC\srv\p\x.exe"), r"\\?\UNC\srv\p\x.exe");
        assert_eq!(launchable(r"C:\x.exe"), r"C:\x.exe");
    }

    #[test]
    fn a_helper_that_is_missing_or_badly_placed_is_never_launched() {
        let temp = tempfile::tempdir().unwrap();
        let copy = temp.path().join(HELPER_FILE);
        std::fs::write(&copy, b"MZ").unwrap();
        let helper = SystemHelper::at(copy);
        // Dans un dossier temporaire : ni sûr ni enfant de Program Files.
        assert_eq!(helper.access(), PcAccess::Unavailable);
        let request = StartupRequest::new(StartupAction::Cut, StartupSlot::Run, "x").unwrap();
        assert_eq!(helper.run(&request.to_words(), 0), HelperRun::NotStarted);
        let missing = SystemHelper::at(temp.path().join("absent").join(HELPER_FILE));
        assert_eq!(missing.run(&request.to_words(), 0), HelperRun::NotStarted);
    }

    /// L'essai de la vraie installation : activé par `SHINKU_ESSAI_ASSISTANT`
    /// (le chemin de l'assistant installé). Il appelle le vrai lanceur avec une
    /// demande pour un nom qui n'existe pas : code 4, aucune fenêtre. Cela prouve
    /// d'un coup que la règle d'emplacement passe sur une installation normale,
    /// que la poignée gardée ouverte n'empêche pas le lancement, que l'assistant
    /// reçoit bien ses quatre mots (sinon code 2), et que le code se lit. Sur une
    /// copie dans le dossier temporaire : refus avant tout lancement.
    #[test]
    fn the_installed_helper_answers_not_found_for_a_name_that_does_not_exist() {
        let Some(installed) = std::env::var_os("SHINKU_ESSAI_ASSISTANT") else { return };
        let installed = PathBuf::from(installed);
        let helper = SystemHelper::at(installed.clone());
        let access = helper.access();
        println!("FAIT : accès lu pour l'assistant installé {} : {access:?}", installed.display());
        assert_ne!(
            access,
            PcAccess::Unavailable,
            "l'emplacement d'une installation normale est sûr"
        );
        let name = "Shinku essai : un nom qui n'existe pas";
        let request = StartupRequest::new(StartupAction::Cut, StartupSlot::Run, name).unwrap();
        let run = helper.run(&request.to_words(), 0);
        println!("FAIT : l'assistant installé répond {run:?} pour un nom inconnu");
        assert_eq!(run, HelperRun::Exited(4));

        // Une copie hors de Program Files : refusée avant tout lancement.
        let temp = tempfile::tempdir().unwrap();
        let copy = temp.path().join(HELPER_FILE);
        std::fs::copy(&installed, &copy).unwrap();
        let outside = SystemHelper::at(copy);
        assert_eq!(outside.run(&request.to_words(), 0), HelperRun::NotStarted);
        assert_eq!(outside.access(), PcAccess::Unavailable);
    }
}
