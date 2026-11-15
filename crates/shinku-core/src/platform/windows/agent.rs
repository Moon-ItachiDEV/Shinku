//! L'agent du mode jeu, sous Windows : une fenêtre cachée, un mutex nommé, des
//! messages, la valeur `Run`.
//!
//! - L'agent a une vraie fenêtre de premier niveau (classe `Shinku.Agent`),
//!   jamais visible, qui reçoit : la minuterie de deux secondes, les
//!   changements d'alimentation, la fin de session, les changements de session
//!   (verrouillage, changement d'utilisateur) et trois messages enregistrés,
//!   postés par `shinku.exe` et l'installeur : relâcher, quitter, retirer.
//! - Un mutex nommé (`Local\Shinku.Agent`) garantit un seul agent par session.
//! - La valeur `Run` « Shinku » du compte est la seule écriture de registre.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::io;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, GetLastError, HINSTANCE, HWND,
    LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_BINARY, RegCloseKey,
    RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcessId, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW,
    GW_OWNER, GWL_EXSTYLE, GWLP_USERDATA, GetMessageW, GetWindow, GetWindowLongPtrW,
    GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, KillTimer, MSG,
    PBT_APMPOWERSTATUSCHANGE, PostMessageW, PostQuitMessage, RegisterClassExW,
    RegisterWindowMessageW, SW_SHOWNORMAL, SetTimer, SetWindowLongPtrW, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_ENDSESSION, WM_NCCREATE, WM_POWERBROADCAST,
    WM_QUERYENDSESSION, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSEXW, WS_EX_TOOLWINDOW,
    WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT, WTS_REMOTE_CONNECT, WTS_REMOTE_DISCONNECT,
};
use windows::core::{BOOL, HSTRING, PCWSTR};

use super::game::path_of_pid;
use crate::game::agent::{AgentEvent, Flow};
use crate::game::{STEP, apps};
use crate::platform::agent::{
    AgentState, Autostart, OpenProgram, Order, RUN_VALUE, WindowsPage, agent_file,
};

/// Le mutex qui garantit un seul agent par session.
const MUTEX_NAME: &str = r"Local\Shinku.Agent";
/// La classe de la fenêtre cachée.
const WINDOW_CLASS: &str = "Shinku.Agent";
/// Les trois ordres postés à l'agent.
const MESSAGE_RELEASE: &str = "Shinku.Agent.Relacher";
const MESSAGE_QUIT: &str = "Shinku.Agent.Quitter";
const MESSAGE_REMOVE: &str = "Shinku.Agent.Retirer";
/// La clé `Run` du compte.
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Où Windows note les démarrages que la personne a désactivés.
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// Pas de fenêtre pour le processus lancé.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Numéro de la minuterie de l'agent.
const TIMER_ID: usize = 1;
/// Plus long chemin que `Run` sait contenir : bien au-delà de ce qu'on écrit.
const MAX_RUN_VALUE: usize = 2048;

/// L'instance unique : le mutex tient tant que cette valeur vit.
#[derive(Debug)]
pub(in crate::platform) struct Guard {
    _mutex: OwnedHandle,
}

pub(in crate::platform) fn acquire_instance() -> Option<Guard> {
    let name = HSTRING::from(MUTEX_NAME);
    // SAFETY: `name` est une chaîne terminée par zéro, valide pendant l'appel ;
    // pas d'attributs de sécurité, le mutex n'est pas hérité.
    let handle = unsafe { CreateMutexW(None, false, &name) }.ok()?;
    // SAFETY: handle ouvert à l'instant, qui n'appartient qu'à cette fonction :
    // `OwnedHandle` le fermera, une seule fois.
    let mutex = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    // SAFETY: appel sans argument, juste après `CreateMutexW`.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return None;
    }
    Some(Guard { _mutex: mutex })
}

/// La fenêtre cachée de l'agent qui tourne, s'il y en a une.
fn agent_window() -> Option<HWND> {
    let class = HSTRING::from(WINDOW_CLASS);
    // SAFETY: `class` est une chaîne terminée par zéro, valide pendant l'appel.
    unsafe { FindWindowW(&class, PCWSTR::null()) }.ok().filter(|window| !window.is_invalid())
}

pub(in crate::platform) fn agent_running() -> bool {
    if agent_window().is_some() {
        return true;
    }
    // L'agent démarre : le mutex existe avant la fenêtre.
    let name = HSTRING::from(MUTEX_NAME);
    // SAFETY: `name` est une chaîne terminée par zéro, valide pendant l'appel.
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &name) } {
        Ok(handle) => {
            // SAFETY: handle ouvert à l'instant, qui n'appartient qu'à cette
            // branche : `OwnedHandle` le fermera, une seule fois.
            drop(unsafe { OwnedHandle::from_raw_handle(handle.0) });
            true
        }
        Err(_) => false,
    }
}

pub(in crate::platform) fn spawn_agent(release_first: bool) -> AgentState {
    let Some(file) = agent_file().filter(|file| file.is_file()) else {
        return AgentState::Missing;
    };
    if agent_running() {
        return AgentState::Running;
    }
    let mut command = Command::new(file);
    if release_first {
        command.arg("--relacher");
    }
    // Dossier de travail neutre : l'agent ne garde pas le dossier d'installation.
    // Ni entrée ni sorties héritées : si `shinku.exe` a été lancé avec des
    // tubes (un essai, un script), l'agent ne doit pas les garder ouverts.
    command
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    match command.spawn() {
        // Le processus n'est pas attendu : il vit sa vie.
        Ok(child) => {
            drop(child);
            AgentState::Running
        }
        Err(_) => AgentState::Stopped,
    }
}

/// Le numéro du message enregistré pour cet ordre.
fn message_of(order: Order) -> u32 {
    let name = match order {
        Order::Release => MESSAGE_RELEASE,
        Order::Quit => MESSAGE_QUIT,
        Order::Remove => MESSAGE_REMOVE,
    };
    register_message(name)
}

fn register_message(name: &str) -> u32 {
    let name = HSTRING::from(name);
    // SAFETY: `name` est une chaîne terminée par zéro, valide pendant l'appel.
    unsafe { RegisterWindowMessageW(&name) }
}

pub(in crate::platform) fn post_agent(order: Order) -> bool {
    let Some(window) = agent_window() else { return false };
    let message = message_of(order);
    if message == 0 {
        return false;
    }
    // SAFETY: `window` vient d'être trouvée ; si elle disparaît entre-temps,
    // l'appel échoue simplement. Aucun pointeur n'est transmis.
    unsafe { PostMessageW(Some(window), message, WPARAM(0), LPARAM(0)) }.is_ok()
}

/// Ce que la procédure de fenêtre sait de la boucle.
struct Context<'a> {
    handler: RefCell<&'a mut dyn FnMut(AgentEvent) -> Flow>,
    release: u32,
    quit: u32,
    remove: u32,
}

impl Context<'_> {
    /// Passe un événement au gestionnaire. Un événement qui arrive pendant
    /// qu'un autre est traité est ignoré : il ne peut pas arriver, la boucle
    /// n'a qu'un fil, mais on ne parie pas dessus.
    fn deliver(&self, event: AgentEvent) {
        let Ok(mut handler) = self.handler.try_borrow_mut() else { return };
        if (*handler)(event) == Flow::Stop {
            // SAFETY: appel sans pointeur, depuis le fil de la boucle.
            unsafe { PostQuitMessage(0) };
        }
    }
}

/// La procédure de la fenêtre cachée.
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        // SAFETY: pour ce message, `lparam` pointe vers un `CREATESTRUCTW`
        // valide ; `lpCreateParams` est le pointeur donné à `CreateWindowExW`.
        let created = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        // SAFETY: la fenêtre est celle qu'on est en train de créer.
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, created.lpCreateParams as isize) };
        // SAFETY: les paramètres sont ceux reçus, passés tels quels.
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    // SAFETY: la valeur est soit 0, soit le pointeur vers le `Context` posé
    // par `run_agent`, qui vit plus longtemps que la fenêtre.
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const Context<'_>;
    // SAFETY: voir ci-dessus ; on ne crée qu'une référence partagée, le
    // gestionnaire est derrière un `RefCell`.
    let Some(context) = (unsafe { pointer.as_ref() }) else {
        // SAFETY: les paramètres sont ceux reçus, passés tels quels.
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    };

    match message {
        WM_TIMER if wparam.0 == TIMER_ID => {
            context.deliver(AgentEvent::Tick);
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wparam.0 as u32 == PBT_APMPOWERSTATUSCHANGE {
                context.deliver(AgentEvent::PowerChanged);
            }
            // TRUE : la demande est acceptée.
            LRESULT(1)
        }
        // Jamais d'objection : Shinku ne retient pas la fermeture de Windows.
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                // Tout est remis en place avant de rendre la main : Windows
                // ferme le processus juste après.
                context.deliver(AgentEvent::SessionEnding);
            }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            match wparam.0 as u32 {
                WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => {
                    context.deliver(AgentEvent::Disconnected);
                }
                WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => {
                    context.deliver(AgentEvent::Reconnected);
                }
                _ => {}
            }
            LRESULT(0)
        }
        _ if message != 0 && message == context.release => {
            context.deliver(AgentEvent::Release);
            LRESULT(0)
        }
        _ if message != 0 && message == context.quit => {
            context.deliver(AgentEvent::Quit);
            LRESULT(0)
        }
        _ if message != 0 && message == context.remove => {
            context.deliver(AgentEvent::Remove);
            LRESULT(0)
        }
        // SAFETY: les paramètres sont ceux reçus, passés tels quels.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

pub(in crate::platform) fn run_agent(
    handler: &mut dyn FnMut(AgentEvent) -> Flow,
) -> io::Result<()> {
    // SAFETY: appel sans argument ; rend le module de ce programme.
    let module = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
    let instance = HINSTANCE(module.0);
    let class = HSTRING::from(WINDOW_CLASS);
    let description = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: PCWSTR(class.as_ptr()),
        ..Default::default()
    };
    // SAFETY: `description` est complète ; la classe et la procédure vivent
    // autant que le programme. Un échec (classe déjà enregistrée) se voit à la
    // création de la fenêtre.
    unsafe { RegisterClassExW(&description) };

    let context = Context {
        handler: RefCell::new(handler),
        release: register_message(MESSAGE_RELEASE),
        quit: register_message(MESSAGE_QUIT),
        remove: register_message(MESSAGE_REMOVE),
    };
    // SAFETY: la classe est enregistrée ; `context` vit plus longtemps que la
    // fenêtre (elle est détruite avant la fin de cette fonction) et son
    // adresse est transmise à `WM_NCCREATE`. Fenêtre de premier niveau, de
    // taille nulle, jamais affichée.
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            &class,
            &HSTRING::from("Shinku Agent"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            Some((&raw const context).cast::<c_void>()),
        )
    }?;

    // SAFETY: `window` vient d'être créée, sur ce fil.
    if unsafe { SetTimer(Some(window), TIMER_ID, STEP.as_millis() as u32, None) } == 0 {
        let error = io::Error::last_os_error();
        // SAFETY: fenêtre créée par ce fil.
        let _ = unsafe { DestroyWindow(window) };
        return Err(error);
    }
    // Sans cette inscription, les changements de session ne sont pas reçus :
    // l'agent reste utile, il ne sait juste pas se mettre au repos.
    // SAFETY: `window` est valide.
    let registered =
        unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) }.is_ok();

    let mut outcome = Ok(());
    let mut message = MSG::default();
    loop {
        // SAFETY: `message` est une structure locale ; tous les messages de ce fil.
        let got = unsafe { GetMessageW(&raw mut message, None, 0, 0) }.0;
        if got == 0 {
            break;
        }
        if got < 0 {
            outcome = Err(io::Error::last_os_error());
            break;
        }
        // SAFETY: `message` vient d'être remplie par `GetMessageW`.
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    if registered {
        // SAFETY: `window` est encore valide.
        let _ = unsafe { WTSUnRegisterSessionNotification(window) };
    }
    // SAFETY: `window` est encore valide ; elle est détruite avant que
    // `context` disparaisse.
    unsafe {
        let _ = KillTimer(Some(window), TIMER_ID);
        let _ = DestroyWindow(window);
    }
    outcome
}

/// Une clé du registre de ce compte, ouverte avec ces droits.
struct Key(HKEY);

impl Key {
    fn open(path: &str, access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<Self> {
        let mut key = HKEY::default();
        // SAFETY: `path` est une chaîne terminée par zéro, valide pendant
        // l'appel ; `key` est une variable locale où Windows écrit le handle.
        let result = unsafe {
            RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, access, &raw mut key)
        };
        (result == ERROR_SUCCESS).then_some(Self(key))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: la clé a été ouverte par `Key::open` et n'est fermée qu'ici.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// La valeur `Run` que Shinku écrit : l'agent, entre guillemets, avec son argument.
fn wanted_run_value() -> Option<String> {
    let file = agent_file()?;
    Some(format!("\"{}\" --demarrage", file.display()))
}

/// Vrai si la valeur `Run` existe, quel que soit son type et sa taille.
fn run_value_exists(key: &Key) -> bool {
    // SAFETY: la clé est ouverte pour `key` ; aucun tampon n'est donné, Windows
    // dit seulement si la valeur existe.
    let result =
        unsafe { RegQueryValueExW(key.0, &HSTRING::from(RUN_VALUE), None, None, None, None) };
    result == ERROR_SUCCESS
}

/// Lit une valeur texte de `Run`.
fn read_run_value(key: &Key) -> Option<String> {
    let mut buffer = vec![0u16; MAX_RUN_VALUE];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: la clé est ouverte pour `key` ; le tampon est local et `size`
    // annonce sa taille en octets.
    let result = unsafe {
        RegGetValueW(
            key.0,
            PCWSTR::null(),
            &HSTRING::from(RUN_VALUE),
            windows::Win32::System::Registry::RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if result != ERROR_SUCCESS {
        return None;
    }
    let units = (size as usize / 2).min(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..units]);
    Some(text.trim_end_matches('\0').to_owned())
}

pub(in crate::platform) fn set_autostart(on: bool) -> io::Result<()> {
    if on {
        let Some(wanted) = wanted_run_value() else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "emplacement de l'agent inconnu"));
        };
        if !agent_file().is_some_and(|file| file.is_file()) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "l'agent est absent"));
        }
        let key = Key::open(RUN_KEY, KEY_READ | KEY_SET_VALUE)
            .ok_or_else(|| io::Error::other("la clé Run n'a pas pu être ouverte"))?;
        // Lire avant d'écrire : un registre déjà bon n'est pas réécrit.
        if read_run_value(&key).is_some_and(|current| current == wanted) {
            return Ok(());
        }
        let units: Vec<u16> = wanted.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes: Vec<u8> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
        // SAFETY: la clé est ouverte pour `key` ; `bytes` est un tampon local
        // de texte UTF-16 terminé par zéro, comme `REG_SZ` l'exige.
        let result =
            unsafe { RegSetValueExW(key.0, &HSTRING::from(RUN_VALUE), None, REG_SZ, Some(&bytes)) };
        return if result == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(result.0 as i32))
        };
    }
    let Some(key) = Key::open(RUN_KEY, KEY_READ | KEY_SET_VALUE) else { return Ok(()) };
    // Effacée sans la lire d'abord : une valeur d'un autre type ou trop longue
    // pour `read_run_value` doit partir aussi. « Introuvable » est un succès.
    // SAFETY: la clé est ouverte pour `key`.
    let result = unsafe { RegDeleteValueW(key.0, &HSTRING::from(RUN_VALUE)) };
    if result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result.0 as i32))
    }
}

pub(in crate::platform) fn autostart() -> Autostart {
    let Some(key) = Key::open(RUN_KEY, KEY_READ) else { return Autostart::Absent };
    if !run_value_exists(&key) {
        return Autostart::Absent;
    }
    // `StartupApproved` : douze octets ; un premier octet impair veut dire
    // que la personne a désactivé ce démarrage dans les réglages de Windows.
    let Some(approved) = Key::open(APPROVED_KEY, KEY_READ) else { return Autostart::Enabled };
    let mut data = [0u8; 64];
    let mut size = data.len() as u32;
    // SAFETY: la clé est ouverte pour `approved` ; le tampon est local et
    // `size` annonce sa taille en octets.
    let result = unsafe {
        RegGetValueW(
            approved.0,
            PCWSTR::null(),
            &HSTRING::from(RUN_VALUE),
            RRF_RT_REG_BINARY,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if result == ERROR_SUCCESS && size >= 1 && data[0] & 1 == 1 {
        Autostart::DisabledInWindows
    } else {
        Autostart::Enabled
    }
}

/// Ce que l'énumération des fenêtres ramasse.
struct Found {
    own: u32,
    programs: Vec<OpenProgram>,
}

/// Regarde une fenêtre de premier niveau : visible, sans propriétaire, avec un
/// titre, ni palette d'outils, ni cachée par le bureau virtuel.
unsafe extern "system" fn look_at(window: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` est le pointeur vers `Found` donné par `open_apps`, qui
    // vit pendant toute l'énumération ; seul ce fil y touche.
    let found = unsafe { &mut *(lparam.0 as *mut Found) };
    // SAFETY: appel sans pointeur, sur la fenêtre que Windows vient de donner.
    let visible = unsafe { IsWindowVisible(window) }.as_bool();
    // SAFETY: appel sans pointeur, sur la même fenêtre.
    let titled = unsafe { GetWindowTextLengthW(window) } != 0;
    if !visible || !titled {
        return BOOL(1);
    }
    // SAFETY: appel sans pointeur, sur la même fenêtre.
    let owned = unsafe { GetWindow(window, GW_OWNER) }.is_ok_and(|owner| !owner.is_invalid());
    if owned {
        return BOOL(1);
    }
    // SAFETY: appel sans pointeur, sur la même fenêtre.
    let style = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) };
    // Un style étendu tient sur 32 bits : le reste est du remplissage.
    let style = u32::try_from(style as usize & 0xFFFF_FFFF).unwrap_or(0);
    if style & WS_EX_TOOLWINDOW.0 != 0 {
        return BOOL(1);
    }
    let mut cloaked = 0u32;
    // SAFETY: `cloaked` est une variable locale dont on annonce la taille
    // exacte (4 octets), où Windows écrit.
    let cloak =
        unsafe { DwmGetWindowAttribute(window, DWMWA_CLOAKED, (&raw mut cloaked).cast(), 4) };
    if cloak.is_ok() && cloaked != 0 {
        return BOOL(1);
    }
    let mut pid = 0u32;
    // SAFETY: `pid` est une variable locale où Windows écrit un nombre.
    unsafe { GetWindowThreadProcessId(window, Some(&raw mut pid)) };
    if pid == 0 || pid == found.own {
        return BOOL(1);
    }
    if let Some(path) = path_of_pid(pid) {
        let name = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_owned();
        if !apps::protected(&name.to_lowercase()) {
            found.programs.push(OpenProgram { name, path });
        }
    }
    BOOL(1)
}

pub(in crate::platform) fn open_apps() -> Vec<OpenProgram> {
    let mut found = Found {
        // SAFETY: appel sans argument.
        own: unsafe { GetCurrentProcessId() },
        programs: Vec::new(),
    };
    // SAFETY: `found` vit pendant l'appel, qui est synchrone ; la procédure
    // `look_at` n'en fait que ce qui est dit plus haut.
    let _ = unsafe {
        windows::Win32::UI::WindowsAndMessaging::EnumWindows(
            Some(look_at),
            LPARAM((&raw mut found) as isize),
        )
    };
    let mut programs = found.programs;
    programs.sort_by_key(|program| (program.name.to_lowercase(), program.path.to_lowercase()));
    // Une seule entrée par exécutable.
    programs.dedup_by(|later, earlier| later.path.eq_ignore_ascii_case(&earlier.path));
    programs
}

pub(in crate::platform) fn open_windows_settings(page: WindowsPage) -> io::Result<()> {
    // Un fil à part, dont l'appartement COM n'appartient qu'à cet appel :
    // `ShellExecuteW` demande que COM soit initialisé, et le fil qui appelle
    // (une tâche asynchrone de Tauri) n'en a pas la garde.
    let uri = page.uri();
    std::thread::spawn(move || shell_open(uri))
        .join()
        .map_err(|_| io::Error::other("l'ouverture des réglages a échoué"))?
}

/// Ouvre une adresse fixe `ms-settings:` avec le shell, dans un appartement COM
/// initialisé pour la durée de l'appel.
fn shell_open(uri: &'static str) -> io::Result<()> {
    // SAFETY: appel sans pointeur, sur un fil qui n'a encore rien initialisé ;
    // l'appartement est libéré plus bas, une fois, si l'initialisation a réussi.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    // SAFETY: les trois chaînes sont terminées par zéro et valides pendant
    // l'appel ; l'adresse est l'une des deux constantes de `WindowsPage`.
    let result = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &HSTRING::from(uri),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    let outcome =
        // ShellExecuteW rend une valeur supérieure à 32 quand elle réussit.
        if result.0 as usize > 32 { Ok(()) } else { Err(io::Error::last_os_error()) };
    if com.is_ok() {
        // SAFETY: appariée à l'initialisation réussie de ce même fil, plus haut.
        unsafe { CoUninitialize() };
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_instance_holds_the_mutex_and_it_is_seen_from_outside() {
        let first = acquire_instance();
        // Si un vrai agent tourne sur cette machine de test, on ne peut rien prouver.
        let Some(first) = first else { return };
        assert!(agent_running(), "le mutex tenu doit se voir de l'extérieur");
        assert!(acquire_instance().is_none(), "un second agent ne doit pas pouvoir démarrer");
        drop(first);
    }

    #[test]
    fn without_a_window_orders_go_nowhere() {
        if agent_window().is_none() {
            assert!(!post_agent(Order::Release));
            assert!(!post_agent(Order::Quit));
            assert!(!post_agent(Order::Remove));
        }
    }

    #[test]
    fn the_three_messages_are_registered_and_distinct() {
        let numbers =
            [message_of(Order::Release), message_of(Order::Quit), message_of(Order::Remove)];
        assert!(numbers.iter().all(|&number| number >= 0xC000));
        assert!(numbers[0] != numbers[1] && numbers[1] != numbers[2] && numbers[0] != numbers[2]);
        assert_eq!(numbers[0], message_of(Order::Release));
    }

    #[test]
    fn the_hidden_window_receives_the_timer_and_the_orders() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        // Si un vrai agent tourne ici, la fenêtre de test se confondrait avec la sienne.
        if agent_running() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let runner = std::thread::spawn(move || {
            let mut seen = Vec::new();
            run_agent(&mut |event| {
                seen.push(event);
                let _ = sender.send(event);
                if event == AgentEvent::Quit { Flow::Stop } else { Flow::Continue }
            })
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while agent_window().is_none() {
            if runner.is_finished() {
                let outcome = runner.join().expect("le fil de l'agent");
                panic!("la boucle s'est arrêtée avant d'ouvrir sa fenêtre : {outcome:?}");
            }
            assert!(Instant::now() < deadline, "la fenêtre n'est jamais apparue");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(post_agent(Order::Release));
        assert_eq!(receiver.recv_timeout(Duration::from_secs(10)), Ok(AgentEvent::Release));
        assert!(post_agent(Order::Quit));
        loop {
            match receiver.recv_timeout(Duration::from_secs(10)) {
                Ok(AgentEvent::Quit) => break,
                Ok(_) => {}
                Err(error) => panic!("l'ordre de quitter n'est pas arrivé : {error}"),
            }
        }
        runner.join().expect("le fil de l'agent").expect("boucle sans erreur");
        assert!(agent_window().is_none(), "la fenêtre est détruite à la fin");
    }

    #[test]
    fn the_run_value_is_the_quoted_agent_with_its_argument() {
        let value = wanted_run_value().expect("un dossier courant");
        assert!(value.starts_with('"') && value.ends_with("\" --demarrage"), "{value}");
        assert!(value.to_lowercase().contains("shinku-agent.exe"), "{value}");
    }

    #[test]
    fn open_apps_are_sorted_and_without_duplicates() {
        let apps = open_apps();
        let mut sorted = apps.clone();
        sorted.sort_by_key(|program| (program.name.to_lowercase(), program.path.to_lowercase()));
        assert_eq!(apps, sorted);
        let own = std::env::current_exe().unwrap().to_string_lossy().to_lowercase();
        assert!(apps.iter().all(|program| program.path.to_lowercase() != own));
    }
}
