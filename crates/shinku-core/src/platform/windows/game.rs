//! La machine du mode jeu, sous Windows : [`GameHost`] sur de vrais appels
//! documentés de l'API Windows.
//!
//! Ce module ne modifie que deux choses : le mode d'alimentation actif
//! (`PowerSetActiveScheme`) et, pour un processus que Shinku a le droit
//! d'ouvrir, sa classe de priorité et son mode efficacité. Tout le reste est
//! de la lecture.
//!
//! Un numéro de processus se recycle : chaque accès à un processus repasse
//! par [`open_verified`], qui compare sa date de création à celle que le
//! journal a notée.
#![allow(unsafe_code)]

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::OnceLock;

use windows::Win32::Foundation::{
    ERROR_INVALID_PARAMETER, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, FILETIME, HANDLE, HLOCAL,
    LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Power::{
    ACCESS_SCHEME, GetSystemPowerStatus, PowerEnumerate, PowerGetActiveScheme,
    PowerSetActiveScheme, SYSTEM_POWER_STATUS,
};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetExitCodeProcess, GetPriorityClass, GetProcessInformation,
    GetProcessTimes, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_CREATION_FLAGS,
    PROCESS_NAME_FORMAT, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    PROCESS_SYNCHRONIZE, ProcessPowerThrottling, QueryFullProcessImageNameW, SetPriorityClass,
    SetProcessInformation, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::{GUID, HSTRING, PCWSTR, PWSTR};

use super::text_from;
use crate::game::host::{
    Foreground, GameHost, Liveness, PowerPlan, ProcessRef, ProcessState, RunningProcess, priority,
    stem,
};
use crate::model::Timestamp;

/// Valeur de `GetExitCodeProcess` d'un processus qui tourne encore.
const STILL_ACTIVE: u32 = 259;
/// Nombre maximal de modes d'alimentation examinés.
const MAX_PLANS: u32 = 64;
/// Longueur maximale d'un nom affiché.
const MAX_NAME: usize = 80;

/// La machine du mode jeu.
#[derive(Debug, Default)]
pub(in crate::platform) struct SystemHost {
    processor: OnceLock<String>,
}

impl SystemHost {
    pub(in crate::platform) fn new() -> Self {
        Self::default()
    }
}

/// Ouvre un processus avec ces droits. `None` si Windows refuse ou si le
/// processus n'existe pas.
fn open_process(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Option<OwnedHandle> {
    // SAFETY: appel sans pointeur ; les droits demandés sont ceux qu'annonce
    // l'appelant, et le handle rendu est vérifié avant tout usage.
    let handle = unsafe { OpenProcess(access, false, pid) }.ok()?;
    // SAFETY: handle ouvert à l'instant, qui n'appartient qu'à cette fonction :
    // `OwnedHandle` le fermera, une seule fois.
    Some(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// Un `FILETIME` sous forme d'entier signé de 64 bits.
fn filetime(time: FILETIME) -> i64 {
    (i64::from(time.dwHighDateTime) << 32) | i64::from(time.dwLowDateTime)
}

/// Date de création d'un processus ouvert.
fn created(handle: &OwnedHandle) -> Option<i64> {
    let (mut start, mut exit, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: le handle vit autant que `handle` ; les quatre pointeurs sont
    // des variables locales où Windows écrit un `FILETIME` chacune.
    unsafe {
        GetProcessTimes(raw(handle), &raw mut start, &raw mut exit, &raw mut kernel, &raw mut user)
    }
    .ok()?;
    Some(filetime(start))
}

/// Vrai si le processus ouvert tourne encore.
fn alive(handle: &OwnedHandle) -> bool {
    let mut code = 0u32;
    // SAFETY: le handle vit autant que `handle` ; `code` est une variable locale.
    unsafe { GetExitCodeProcess(raw(handle), &raw mut code) }.is_ok() && code == STILL_ACTIVE
}

/// Ouvre un processus et rend l'erreur de Windows telle quelle.
fn try_open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> windows::core::Result<OwnedHandle> {
    // SAFETY: appel sans pointeur ; le handle rendu est vérifié avant tout usage.
    let handle = unsafe { OpenProcess(access, false, pid) }?;
    // SAFETY: handle ouvert à l'instant, qui n'appartient qu'à cette fonction :
    // `OwnedHandle` le fermera, une seule fois.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

/// Ce que Windows dit de ce processus, avec des droits limités : `Dead` si son
/// numéro n'existe pas ou appartient maintenant à une autre session Windows (un
/// processus ne change jamais de session), si sa date de création diffère ou
/// s'il est terminé ; `Unknown` pour tout le reste (accès refusé, autre erreur).
///
/// « Terminé » se lit avec `SYNCHRONIZE` et une attente nulle, qui ne dépend pas
/// du code de sortie (un processus peut sortir avec 259, `STILL_ACTIVE`) ; si
/// `SYNCHRONIZE` est refusé, on se rabat sur le code de sortie.
fn liveness_of(process: ProcessRef) -> Liveness {
    let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;
    let (handle, waitable) = match try_open(process.pid, access) {
        Ok(handle) => (handle, true),
        Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => {
            return Liveness::Dead;
        }
        Err(_) => match try_open(process.pid, PROCESS_QUERY_LIMITED_INFORMATION) {
            Ok(handle) => (handle, false),
            Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => {
                return Liveness::Dead;
            }
            Err(_) => {
                // SAFETY: appel sans argument.
                let own = session_of(unsafe { GetCurrentProcessId() });
                return match session_of(process.pid) {
                    Some(session) if Some(session) != own => Liveness::Dead,
                    _ => Liveness::Unknown,
                };
            }
        },
    };
    let Some(started) = created(&handle) else { return Liveness::Unknown };
    if started != process.started {
        return Liveness::Dead;
    }
    if waitable {
        // SAFETY: le handle vit autant que `handle` ; une attente nulle ne bloque pas.
        let waited = unsafe { WaitForSingleObject(raw(&handle), 0) };
        return if waited == WAIT_TIMEOUT {
            Liveness::Alive
        } else if waited == WAIT_OBJECT_0 {
            Liveness::Dead
        } else {
            Liveness::Unknown
        };
    }
    let mut code = 0u32;
    // SAFETY: le handle vit autant que `handle` ; `code` est une variable locale.
    if unsafe { GetExitCodeProcess(raw(&handle), &raw mut code) }.is_err() {
        return Liveness::Unknown;
    }
    if code == STILL_ACTIVE { Liveness::Alive } else { Liveness::Dead }
}

/// Ouvre ce processus, à condition qu'il tourne encore et que sa date de
/// création soit bien celle qui a été notée : un pid repris par un autre
/// processus n'est pas ouvert.
fn open_verified(process: ProcessRef, access: PROCESS_ACCESS_RIGHTS) -> Option<OwnedHandle> {
    let handle = open_process(process.pid, access | PROCESS_QUERY_LIMITED_INFORMATION)?;
    (created(&handle)? == process.started && alive(&handle)).then_some(handle)
}

/// Le chemin complet de l'exécutable d'un processus ouvert.
fn image_path(handle: &OwnedHandle) -> Option<String> {
    image_path_in(handle, &mut vec![0u16; 32_768])
}

/// Comme [`image_path`], dans un tampon que l'appelant réutilise (un relevé en
/// lit plusieurs centaines).
fn image_path_in(handle: &OwnedHandle, buffer: &mut [u16]) -> Option<String> {
    let mut length = buffer.len() as u32;
    // SAFETY: le handle vit autant que `handle` ; le tampon est exclusif à cet
    // appel et `length` annonce sa taille en caractères ; Windows y écrit au
    // plus autant, et met à jour `length`.
    unsafe {
        QueryFullProcessImageNameW(
            raw(handle),
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &raw mut length,
        )
    }
    .ok()?;
    let text = String::from_utf16_lossy(buffer.get(..length as usize)?);
    (!text.is_empty()).then_some(text)
}

/// Le chemin de l'exécutable du processus de ce numéro, sans vérifier sa
/// date de création : pour lister, pas pour agir.
pub(super) fn path_of_pid(pid: u32) -> Option<String> {
    let handle = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    image_path(&handle)
}

/// Le numéro de session Windows d'un processus.
fn session_of(pid: u32) -> Option<u32> {
    let mut session = 0u32;
    // SAFETY: `session` est une variable locale où Windows écrit un nombre.
    unsafe { ProcessIdToSessionId(pid, &raw mut session) }.ok()?;
    Some(session)
}

/// Ce que Windows dit des deux réglages que Shinku change.
fn read_state(handle: &OwnedHandle) -> Option<ProcessState> {
    // SAFETY: appel sans pointeur sur un handle qui vit autant que `handle`.
    let class = unsafe { GetPriorityClass(raw(handle)) };
    if class == 0 {
        return None;
    }
    let (control, state) = read_throttle(handle).unwrap_or((0, 0));
    Some(ProcessState {
        priority: class,
        throttle_control: control & PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        throttle_state: state & PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    })
}

/// Les masques du mode efficacité, tels que Windows les rend. `None` si cette
/// version de Windows ne sait pas les lire.
fn read_throttle(handle: &OwnedHandle) -> Option<(u32, u32)> {
    let mut info = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: 0,
        StateMask: 0,
    };
    // SAFETY: le handle vit autant que `handle` ; `info` est une structure
    // locale dont on annonce la taille exacte.
    unsafe {
        GetProcessInformation(
            raw(handle),
            ProcessPowerThrottling,
            (&raw mut info).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
    .ok()?;
    Some((info.ControlMask, info.StateMask))
}

/// Un GUID de Windows sous la forme d'un mode d'alimentation.
fn plan_of(guid: &GUID) -> PowerPlan {
    PowerPlan::from_u128(guid.to_u128())
}

fn os_error(code: u32) -> io::Error {
    io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
}

impl GameHost for SystemHost {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn uptime_ms(&self) -> u64 {
        // SAFETY: appel sans argument ni pointeur.
        unsafe { GetTickCount64() }
    }

    fn foreground(&self) -> Option<Foreground> {
        // SAFETY: appel sans argument ; rend un handle nul s'il n'y a pas de
        // fenêtre au premier plan.
        let window = unsafe { GetForegroundWindow() };
        if window.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        // SAFETY: `pid` est une variable locale où Windows écrit un nombre.
        unsafe { GetWindowThreadProcessId(window, Some(&raw mut pid)) };
        if pid == 0 {
            return None;
        }
        let handle = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let started = created(&handle)?;
        let exe = image_path(&handle)?;
        Some(Foreground { process: ProcessRef { pid, started }, exe })
    }

    fn processes(&self) -> Vec<RunningProcess> {
        // SAFETY: appel sans pointeur ; le second paramètre est ignoré pour
        // une liste de processus.
        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return Vec::new();
        };
        // SAFETY: handle ouvert à l'instant, qui n'appartient qu'à cette
        // fonction : `OwnedHandle` le fermera, une seule fois.
        let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot.0) };
        let mut entry =
            PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        // SAFETY: appel sans pointeur.
        let own = session_of(unsafe { GetCurrentProcessId() });
        let mut found = Vec::new();
        let mut buffer = vec![0u16; 32_768];
        // SAFETY: le handle vit autant que `snapshot`, et `entry` est une
        // structure locale dont le champ `dwSize` annonce la taille.
        let mut more = unsafe { Process32FirstW(raw(&snapshot), &raw mut entry) }.is_ok();
        while more {
            let pid = entry.th32ProcessID;
            if pid != 0
                && session_of(pid) == own
                && let Some(handle) = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)
                && let Some(started) = created(&handle)
            {
                // Le nom se lit sur le même handle que la date de création : si
                // le numéro a été repris entre la capture et l'ouverture, le nom
                // de la capture n'est plus celui de ce processus, et la ligne
                // est écartée (sinon un étranger porterait un nom listé).
                let listed = text_from(&entry.szExeFile).to_lowercase();
                let opened = image_path_in(&handle, &mut buffer)
                    .map(|path| path.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase());
                if opened.as_deref() == Some(listed.as_str()) {
                    found.push(RunningProcess {
                        process: ProcessRef { pid, started },
                        parent: entry.th32ParentProcessID,
                        exe_name: listed,
                    });
                }
            }
            // SAFETY: comme ci-dessus.
            more = unsafe { Process32NextW(raw(&snapshot), &raw mut entry) }.is_ok();
        }
        found
    }

    fn liveness(&self, process: ProcessRef) -> Liveness {
        liveness_of(process)
    }

    fn exe_path(&self, process: ProcessRef) -> Option<String> {
        image_path(&open_verified(process, PROCESS_QUERY_LIMITED_INFORMATION)?)
    }

    fn display_name(&self, exe: &str) -> String {
        description_of(exe)
            .map(|text| text.trim().chars().take(MAX_NAME).collect::<String>())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| stem(exe))
    }

    fn on_battery(&self) -> bool {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: `status` est une structure locale où Windows écrit.
        if unsafe { GetSystemPowerStatus(&raw mut status) }.is_err() {
            return false;
        }
        // 0 : sur batterie ; 1 : sur secteur ; 255 : inconnu.
        status.ACLineStatus == 0
    }

    fn power_plan(&self) -> Option<PowerPlan> {
        let mut active: *mut GUID = std::ptr::null_mut();
        // SAFETY: `active` est une variable locale où Windows écrit l'adresse
        // d'un GUID qu'il alloue ; sans clé racine (`None`), Windows répond
        // pour l'utilisateur courant.
        let result = unsafe { PowerGetActiveScheme(None, &raw mut active) };
        if result != ERROR_SUCCESS || active.is_null() {
            return None;
        }
        // SAFETY: en cas de succès, `active` pointe vers un GUID complet,
        // valide jusqu'à sa libération ci-dessous.
        let plan = plan_of(unsafe { &*active });
        // SAFETY: Windows demande de libérer ce GUID avec `LocalFree`. Il
        // n'est plus utilisé ensuite.
        unsafe { LocalFree(Some(HLOCAL(active.cast()))) };
        Some(plan)
    }

    fn has_power_plan(&self, plan: PowerPlan) -> bool {
        for index in 0..MAX_PLANS {
            let mut guid = [0u8; 16];
            let mut size = 16u32;
            // SAFETY: `guid` est un tampon local de 16 octets, taille annoncée
            // par `size` ; sans clé racine, Windows répond pour l'utilisateur courant.
            let result = unsafe {
                PowerEnumerate(
                    None,
                    None,
                    None,
                    ACCESS_SCHEME,
                    index,
                    Some(guid.as_mut_ptr()),
                    &raw mut size,
                )
            };
            // Plus de mode à lister (ou une erreur) : inutile d'interroger la suite.
            if result == ERROR_NO_MORE_ITEMS || result != ERROR_SUCCESS {
                return false;
            }
            // Un GUID est rangé en mémoire dans l'ordre de la structure `GUID`.
            let found = GUID {
                data1: u32::from_le_bytes([guid[0], guid[1], guid[2], guid[3]]),
                data2: u16::from_le_bytes([guid[4], guid[5]]),
                data3: u16::from_le_bytes([guid[6], guid[7]]),
                data4: [
                    guid[8], guid[9], guid[10], guid[11], guid[12], guid[13], guid[14], guid[15],
                ],
            };
            if plan_of(&found) == plan {
                return true;
            }
        }
        false
    }

    fn set_power_plan(&self, plan: PowerPlan) -> io::Result<()> {
        let guid = GUID::from_u128(plan.as_u128());
        // SAFETY: `guid` est une variable locale valide pendant l'appel ; sans
        // clé racine, Windows agit pour l'utilisateur courant.
        let result = unsafe { PowerSetActiveScheme(None, Some(&raw const guid)) };
        if result == ERROR_SUCCESS { Ok(()) } else { Err(os_error(result.0)) }
    }

    fn processor_name(&self) -> String {
        self.processor.get_or_init(read_processor_name).clone()
    }

    fn inspect(&self, process: ProcessRef) -> Option<ProcessState> {
        read_state(&open_verified(process, PROCESS_QUERY_INFORMATION)?)
    }

    fn apply(&self, process: ProcessRef, state: ProcessState) -> io::Result<()> {
        if !priority::is_known(state.priority) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "classe de priorité inconnue"));
        }
        let handle = open_verified(process, PROCESS_QUERY_INFORMATION | PROCESS_SET_INFORMATION)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "processus introuvable"))?;
        let current = read_state(&handle).ok_or_else(|| io::Error::other("état illisible"))?;
        if current.priority != state.priority {
            // SAFETY: le handle vit autant que `handle` et porte le droit de
            // modifier ce processus ; la classe a été contrôlée plus haut.
            unsafe { SetPriorityClass(raw(&handle), PROCESS_CREATION_FLAGS(state.priority)) }?;
        }
        if current.throttle_control != state.throttle_control
            || current.throttle_state != state.throttle_state
        {
            // Les autres réglages d'économie d'énergie du processus restent tels quels.
            let (control, status) = read_throttle(&handle).unwrap_or((0, 0));
            let bit = PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
            let info = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: (control & !bit) | (state.throttle_control & bit),
                StateMask: (status & !bit) | (state.throttle_state & bit),
            };
            // SAFETY: le handle vit autant que `handle` et porte le droit de
            // modifier ce processus ; `info` est une structure locale dont on
            // annonce la taille exacte.
            let result = unsafe {
                SetProcessInformation(
                    raw(&handle),
                    ProcessPowerThrottling,
                    (&raw const info).cast(),
                    size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
                )
            };
            if let Err(error) = result {
                // Rien ne doit rester à moitié fait : une priorité baissée par
                // ce geste est remise avant de rendre l'erreur (sinon ce
                // processus serait ralenti sans que personne ne le sache). Une
                // remise, elle, garde la priorité rendue : la défaire laisserait
                // le processus ralenti pour de bon, et l'erreur suffit pour que
                // la suite retente le reste.
                if priority::lowers(current.priority, state.priority) {
                    // SAFETY: le handle vit autant que `handle` et porte le
                    // droit de modifier ce processus ; la classe est celle que
                    // Windows vient de lire.
                    let _ = unsafe {
                        SetPriorityClass(raw(&handle), PROCESS_CREATION_FLAGS(current.priority))
                    };
                }
                return Err(error.into());
            }
        }
        Ok(())
    }
}

/// « ProcessorNameString », dans la ruche de la machine.
fn read_processor_name() -> String {
    let mut buffer = [0u16; 256];
    let mut size = size_of_val(&buffer) as u32;
    // SAFETY: les chaînes de chemin sont valides pendant l'appel ; `buffer`
    // est local et `size` annonce sa taille en octets.
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"),
            &HSTRING::from("ProcessorNameString"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if result != ERROR_SUCCESS {
        return String::new();
    }
    text_from(&buffer).trim().to_owned()
}

/// La description du fichier (`FileDescription`) dans ses ressources de version.
fn description_of(exe: &str) -> Option<String> {
    version_string(exe, "FileDescription")
}

/// Une chaîne des ressources de version d'un fichier (`FileDescription`,
/// `CompanyName`…), dans la langue du fichier si elle y est, sinon en anglais.
pub(super) fn version_string(exe: &str, field: &str) -> Option<String> {
    let path = HSTRING::from(exe);
    // SAFETY: `path` est une chaîne terminée par zéro, valide pendant l'appel.
    let size = unsafe { GetFileVersionInfoSizeW(&path, None) };
    if size == 0 || size > 4 * 1024 * 1024 {
        return None;
    }
    // Un tampon de mots de 32 bits : les chaînes de la ressource sont alignées.
    let mut block = vec![0u32; (size as usize).div_ceil(4)];
    // SAFETY: `block` est un tampon local d'au moins `size` octets.
    unsafe { GetFileVersionInfoW(&path, None, size, block.as_mut_ptr().cast()) }.ok()?;
    let start = block.as_ptr() as usize;
    let end = start + block.len() * 4;

    let mut languages: Vec<(u16, u16)> = Vec::new();
    if let Some((pointer, length)) = query(&block, r"\VarFileInfo\Translation") {
        let address = pointer as usize;
        if address >= start && address + length as usize <= end {
            for index in 0..(length as usize / 4).min(8) {
                // SAFETY: l'adresse et la longueur sont dans `block`, vivant.
                let pair = unsafe {
                    pointer.cast::<u8>().add(index * 4).cast::<[u8; 4]>().read_unaligned()
                };
                languages.push((
                    u16::from_le_bytes([pair[0], pair[1]]),
                    u16::from_le_bytes([pair[2], pair[3]]),
                ));
            }
        }
    }
    languages.push((0x0409, 0x04b0));
    languages.push((0x0409, 0x04e4));
    for (language, page) in languages {
        let name = format!(r"\StringFileInfo\{language:04x}{page:04x}\{field}");
        let Some((pointer, length)) = query(&block, &name) else { continue };
        let address = pointer as usize;
        // `length` compte des caractères, terminateur compris.
        if length == 0 || address < start || address + length as usize * 2 > end {
            continue;
        }
        if !address.is_multiple_of(2) {
            continue;
        }
        // SAFETY: l'adresse et la longueur sont dans `block`, vivant, et
        // l'adresse est alignée sur 2 octets (contrôlé ci-dessus).
        let units = unsafe { std::slice::from_raw_parts(pointer.cast::<u16>(), length as usize) };
        let text = text_from(units);
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    None
}

/// Cherche une sous-valeur dans un bloc de ressources de version.
fn query(block: &[u32], sub_block: &str) -> Option<(*const u8, u32)> {
    let name = HSTRING::from(sub_block);
    let mut pointer: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut length = 0u32;
    // SAFETY: `block` est le bloc rendu par `GetFileVersionInfoW`, vivant ;
    // `pointer` et `length` sont des variables locales.
    let found = unsafe {
        VerQueryValueW(
            block.as_ptr().cast(),
            PCWSTR(name.as_ptr()),
            &raw mut pointer,
            &raw mut length,
        )
    };
    (found.as_bool() && !pointer.is_null()).then_some((pointer.cast::<u8>().cast_const(), length))
}

/// Pour les tests : la machine est bien celle de ce programme.
#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> ProcessRef {
        // SAFETY: appel sans argument.
        let pid = unsafe { GetCurrentProcessId() };
        let host = SystemHost::new();
        host.processes()
            .into_iter()
            .find(|entry| entry.process.pid == pid)
            .expect("ce programme est dans la liste")
            .process
    }

    #[test]
    fn this_program_is_among_the_processes_with_its_path() {
        let host = SystemHost::new();
        let me = me();
        let path = host.exe_path(me).expect("le chemin de ce programme");
        let own = std::env::current_exe().unwrap();
        assert!(
            path.eq_ignore_ascii_case(&own.to_string_lossy())
                || path
                    .to_lowercase()
                    .ends_with(&own.file_name().unwrap().to_string_lossy().to_lowercase()),
            "{path} / {own:?}"
        );
    }

    #[test]
    fn a_wrong_creation_date_is_not_this_process() {
        let host = SystemHost::new();
        let mut me = me();
        me.started += 1;
        assert_eq!(host.exe_path(me), None);
        assert_eq!(host.inspect(me), None);
    }

    #[test]
    fn this_program_is_alive_and_a_wrong_creation_date_is_dead() {
        let host = SystemHost::new();
        let mut me = me();
        assert_eq!(host.liveness(me), Liveness::Alive);
        me.started += 1;
        assert_eq!(host.liveness(me), Liveness::Dead, "un pid repris n'est pas ce processus");
    }

    #[test]
    fn a_pid_that_does_not_exist_is_dead() {
        let host = SystemHost::new();
        let nobody = ProcessRef { pid: 0xFFFF_FFF0, started: 1 };
        assert_eq!(host.liveness(nobody), Liveness::Dead, "OpenProcess : paramètre invalide");
    }

    #[test]
    fn a_finished_child_is_dead_and_a_living_one_is_alive() {
        use std::process::{Command, Stdio};

        let mut child = Command::new("cmd.exe")
            .args(["/c", "pause"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("un processus fils");
        let host = SystemHost::new();
        let process = host
            .processes()
            .into_iter()
            .find(|entry| entry.process.pid == child.id())
            .map(|entry| entry.process);
        let Some(process) = process else {
            let _ = child.kill();
            panic!("le fils n'est pas listé");
        };
        let alive = host.liveness(process);
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(alive, Liveness::Alive);
        assert_eq!(host.liveness(process), Liveness::Dead, "un processus fini est mort");
    }

    #[test]
    fn the_name_in_the_snapshot_is_the_name_of_the_opened_process() {
        let host = SystemHost::new();
        let me = me();
        let name = host
            .processes()
            .into_iter()
            .find(|entry| entry.process == me)
            .map(|entry| entry.exe_name)
            .expect("ce programme est listé");
        let own = std::env::current_exe().unwrap();
        assert_eq!(
            name.to_lowercase(),
            own.file_name().unwrap().to_string_lossy().to_lowercase(),
            "le nom du relevé est celui du fichier ouvert"
        );
    }

    #[test]
    fn a_process_is_lowered_and_put_back() {
        use std::process::{Command, Stdio};

        // `cmd /c pause` attend une touche : le fils reste là tant qu'on ne le tue pas.
        let mut child = Command::new("cmd.exe")
            .args(["/c", "pause"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("un processus fils");
        let host = SystemHost::new();
        let process = host
            .processes()
            .into_iter()
            .find(|entry| entry.process.pid == child.id())
            .map(|entry| entry.process);
        let Some(process) = process else {
            let _ = child.kill();
            panic!("le fils n'est pas listé");
        };
        let before = host.inspect(process);
        if let Some(before) = before {
            let lowered = ProcessState { priority: priority::BELOW_NORMAL, ..before };
            let applied = host.apply(process, lowered);
            let after = host.inspect(process);
            let restored = host.apply(process, before);
            let back = host.inspect(process);
            let _ = child.kill();
            applied.expect("priorité baissée");
            assert_eq!(after.map(|state| state.priority), Some(priority::BELOW_NORMAL));
            restored.expect("priorité remise");
            assert_eq!(back, Some(before));
        } else {
            let _ = child.kill();
            panic!("le fils ne s'inspecte pas");
        }
        let _ = child.wait();
        assert_eq!(host.inspect(process), None, "un processus fini ne s'ouvre plus");
    }

    #[test]
    fn a_failed_second_step_puts_the_priority_back() {
        use std::process::{Command, Stdio};

        let mut child = Command::new("cmd.exe")
            .args(["/c", "pause"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("un processus fils");
        let host = SystemHost::new();
        let process = host
            .processes()
            .into_iter()
            .find(|entry| entry.process.pid == child.id())
            .map(|entry| entry.process);
        let Some(process) = process else {
            let _ = child.kill();
            panic!("le fils n'est pas listé");
        };
        let Some(before) = host.inspect(process) else {
            let _ = child.kill();
            panic!("le fils ne s'inspecte pas");
        };
        // Priorité et mode efficacité à la fois : sous Wine, le second geste
        // n'existe pas, et le premier ne doit rien laisser derrière lui.
        let both = ProcessState {
            priority: priority::BELOW_NORMAL,
            throttle_control: 1,
            throttle_state: 1,
        };
        let applied = host.apply(process, both);
        let after = host.inspect(process);
        match applied {
            Ok(()) => {
                assert_eq!(after, Some(both));
                host.apply(process, before).expect("état d'avant remis");
            }
            Err(_) => assert_eq!(
                after.map(|state| state.priority),
                Some(before.priority),
                "un échec ne laisse pas la priorité baissée"
            ),
        }
        let back = host.inspect(process);
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(back, Some(before));
    }

    #[test]
    fn a_failed_second_step_keeps_a_restored_priority() {
        use std::process::{Command, Stdio};

        let mut child = Command::new("cmd.exe")
            .args(["/c", "pause"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("un processus fils");
        let host = SystemHost::new();
        let process = host
            .processes()
            .into_iter()
            .find(|entry| entry.process.pid == child.id())
            .map(|entry| entry.process);
        let Some(process) = process else {
            let _ = child.kill();
            panic!("le fils n'est pas listé");
        };
        let Some(before) = host.inspect(process) else {
            let _ = child.kill();
            panic!("le fils ne s'inspecte pas");
        };
        // Un état de départ « sous la normale » sans mode efficacité : un seul geste.
        let lowered = ProcessState {
            priority: priority::BELOW_NORMAL,
            throttle_control: before.throttle_control,
            throttle_state: before.throttle_state,
        };
        host.apply(process, lowered).expect("priorité baissée");
        // Une remise à « normale » avec le mode efficacité imposé : sous Wine le
        // second geste échoue, et la priorité rendue ne doit pas être défaite.
        let wanted =
            ProcessState { priority: priority::NORMAL, throttle_control: 1, throttle_state: 1 };
        let applied = host.apply(process, wanted);
        let after = host.inspect(process);
        let _ = host.apply(process, before);
        let _ = child.kill();
        let _ = child.wait();
        match applied {
            Ok(()) => assert_eq!(after, Some(wanted)),
            Err(_) => assert_eq!(
                after.map(|state| state.priority),
                Some(priority::NORMAL),
                "une remise qui échoue à moitié garde la priorité rendue"
            ),
        }
    }

    #[test]
    fn the_uptime_only_goes_forward() {
        let host = SystemHost::new();
        let first = host.uptime_ms();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = host.uptime_ms();
        assert!(first > 0 && second >= first + 15, "{first} puis {second}");
    }

    #[test]
    fn a_missing_power_plan_is_not_found() {
        let host = SystemHost::new();
        let nowhere = PowerPlan::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);
        assert!(!host.has_power_plan(nowhere));
    }

    #[test]
    fn unknown_priority_classes_are_refused() {
        let host = SystemHost::new();
        let me = me();
        let state = ProcessState { priority: 0x100, throttle_control: 0, throttle_state: 0 };
        assert!(host.apply(me, state).is_err());
    }

    #[test]
    fn the_active_power_plan_can_be_read_and_exists() {
        let host = SystemHost::new();
        // Sous Wine ou sans service d'alimentation, aucune réponse n'est une
        // réponse permise ; si Windows répond, ce mode doit exister.
        if let Some(plan) = host.power_plan() {
            assert!(host.has_power_plan(plan), "le mode actif {plan} doit exister");
        }
        let _ = host.on_battery();
        let first = host.processor_name();
        assert_eq!(first, host.processor_name());
    }

    #[test]
    fn a_name_is_always_found() {
        let host = SystemHost::new();
        let own = std::env::current_exe().unwrap();
        let name = host.display_name(&own.to_string_lossy());
        assert!(!name.is_empty());
        assert_eq!(host.display_name(r"C:\nulle\part\Jeu.exe"), "Jeu");
    }

    #[test]
    fn this_machine_answers_like_the_environment() {
        let host = SystemHost::new();
        let names = crate::platform::Environment::running_processes(
            &super::super::SystemEnvironment::new(),
        )
        .unwrap_or_default();
        let listed = host.processes();
        // Chaque processus listé figure dans la liste des noms (sauf s'il vient de naître).
        let known = listed
            .iter()
            .filter(|entry| names.iter().any(|n| n.to_lowercase() == entry.exe_name))
            .count();
        assert!(known * 2 >= listed.len(), "{known} sur {}", listed.len());
    }
}
