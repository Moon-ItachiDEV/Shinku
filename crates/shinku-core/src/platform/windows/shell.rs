//! La corbeille de Windows : y envoyer un fichier ou un dossier, la vider.
//!
//! Les deux gestes passent par le shell, comme dans l'Explorateur. Le danger
//! connu est là : quand la corbeille ne peut pas recevoir un fichier (elle est
//! désactivée, trop petite, ou le disque n'en a pas), le shell le supprime
//! définitivement, et sans rien demander si on lui a dit de se taire. Shinku
//! s'intercale donc avant chaque suppression ([`Sink`]) et l'arrête si elle
//! n'est pas un envoi à la corbeille.
//!
//! Le shell parle COM : chaque demande se fait sur un fil d'exécution à part,
//! ouvert et refermé pour elle, afin de ne rien imposer au fil qui appelle.
#![allow(unsafe_code)]

use std::cell::Cell;
use std::ffi::{OsString, c_void};
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use windows::Win32::Foundation::{
    E_ABORT, E_ACCESSDENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION,
};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    COPYENGINE_E_ACCESS_DENIED_SRC, COPYENGINE_E_RECYCLE_BIN_NOT_FOUND,
    COPYENGINE_E_RECYCLE_FORCE_NUKE, COPYENGINE_E_RECYCLE_PATH_TOO_LONG,
    COPYENGINE_E_RECYCLE_SIZE_TOO_BIG, COPYENGINE_E_REQUIRES_ELEVATION,
    COPYENGINE_E_SHARING_VIOLATION_SRC, FILEOPERATION_FLAGS, FOF_ALLOWUNDO,
    FOF_NO_CONNECTED_ELEMENTS, FOF_NO_UI, FOFX_ADDUNDORECORD, FOFX_EARLYFAILURE,
    FOFX_RECYCLEONDELETE, FileOperation, IFileOperation, IFileOperationProgressSink,
    IFileOperationProgressSink_Impl, IShellItem, SHCreateItemFromParsingName, SHERB_NOCONFIRMATION,
    SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW, SHQUERYRBINFO, SHQueryRecycleBinW,
    SIGDN_FILESYSPATH, TSF_DELETE_RECYCLE_IF_POSSIBLE,
};
use windows::core::{HRESULT, HSTRING, PCWSTR, Ref, implement};

use super::super::bin_rule::{self, BinSettings};
use super::super::{RecycleBin, RecycleError, Sent};
use super::startup::{Hive, open_key};
use crate::fs::{DirReader, EntryKind, fold_path, same_file_id, to_display_path, to_internal_path};
use windows::Win32::System::Registry::KEY_READ;

/// Ce que Shinku demande au shell pour un envoi à la corbeille :
/// - aucune fenêtre, aucune question (`FOF_NO_UI`) ;
/// - la corbeille plutôt que la suppression (`FOF_ALLOWUNDO` et
///   `FOFX_RECYCLEONDELETE`), avec la possibilité d'annuler depuis
///   l'Explorateur (`FOFX_ADDUNDORECORD`) ;
/// - ce fichier et lui seul (`FOF_NO_CONNECTED_ELEMENTS`) : sans cela, jeter
///   `page.htm` emporte aussi le dossier `page_files` que Windows lui associe.
///
/// Jamais d'élévation : un fichier qui demande des droits d'administrateur
/// reste à sa place.
///
/// Pour un **dossier**, `FOFX_EARLYFAILURE` s'y ajoute ([`FOLDER_FLAGS`]) : sans
/// fenêtre, une erreur n'arrêterait que l'action en cours et l'opération
/// continuerait ; avec lui, elle s'arrête à la première erreur.
const RECYCLE_FLAGS: FILEOPERATION_FLAGS = FILEOPERATION_FLAGS(
    FOF_NO_UI.0
        | FOF_ALLOWUNDO.0
        | FOFX_ADDUNDORECORD.0
        | FOFX_RECYCLEONDELETE.0
        | FOF_NO_CONNECTED_ELEMENTS.0,
);

/// Les drapeaux d'un envoi de dossier : ceux d'un fichier, et l'arrêt à la
/// première erreur.
const FOLDER_FLAGS: FILEOPERATION_FLAGS =
    FILEOPERATION_FLAGS(RECYCLE_FLAGS.0 | FOFX_EARLYFAILURE.0);

/// Garde de COM sur le fil courant.
struct Com;

impl Com {
    fn enter() -> Option<Self> {
        // SAFETY: appel sans pointeur. Tout succès (y compris « déjà
        // initialisé ») est rendu par le `CoUninitialize` de `Drop`.
        let entered =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        entered.is_ok().then_some(Self)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: équilibre le `CoInitializeEx` réussi de `enter`, sur le même
        // fil : la garde n'est ni `Send` ni clonable, et tous les objets COM
        // créés entre-temps ont déjà été relâchés (voir `on_shell_thread`).
        unsafe { CoUninitialize() };
    }
}

/// Exécute `work` sur un fil d'exécution à part, où COM est prêt pour le
/// shell. `None` si ce fil n'a pas pu être préparé.
///
/// `T` ne doit contenir aucun objet COM : ils doivent tous disparaître dans
/// `work`, avant la garde.
fn on_shell_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::Builder::new()
        .name("shinku-corbeille".to_owned())
        .spawn(move || {
            let _com = Com::enter()?;
            Some(work())
        })
        .ok()?
        .join()
        .ok()?
}

/// Ce que le shell a dit pendant une suppression.
#[derive(Default)]
struct Seen {
    /// Shinku a arrêté l'opération : ce n'était pas un envoi à la corbeille.
    refused: Cell<bool>,
    /// Les drapeaux reçus par `PreDeleteItem`.
    pre_flags: Cell<Option<u32>>,
    /// `PostDeleteItem` : les drapeaux, le résultat, et si l'élément « dans la
    /// corbeille » est présent (nul quand Windows a supprimé pour de bon).
    post: Cell<Option<(u32, HRESULT, bool)>>,
}

/// Ce qu'une suppression a montré, pour un essai ou un rapport.
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct Facts {
    /// Drapeaux de `PreDeleteItem`.
    pub(super) pre_flags: Option<u32>,
    /// Drapeaux, résultat et élément de corbeille présent, de `PostDeleteItem`.
    pub(super) post: Option<(u32, i32, bool)>,
}

/// L'oreille de Shinku dans une opération du shell.
#[implement(IFileOperationProgressSink)]
struct Sink {
    seen: Rc<Seen>,
}

impl IFileOperationProgressSink_Impl for Sink_Impl {
    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn FinishOperations(&self, _result: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreRenameItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostRenameItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    /// Le shell annonce une suppression. Il signale par un drapeau celles qui
    /// iront à la corbeille : toute autre est une suppression définitive, que
    /// Shinku arrête ici, avant qu'elle ait lieu.
    fn PreDeleteItem(&self, flags: u32, _item: Ref<IShellItem>) -> windows::core::Result<()> {
        self.seen.pre_flags.set(Some(flags));
        if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            self.seen.refused.set(true);
            return Err(E_ABORT.into());
        }
        Ok(())
    }

    fn PostDeleteItem(
        &self,
        flags: u32,
        _item: Ref<IShellItem>,
        result: HRESULT,
        in_bin: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        self.seen.post.set(Some((flags, result, !in_bin.is_null())));
        Ok(())
    }

    fn PreNewItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn UpdateProgress(&self, _total: u32, _so_far: u32) -> windows::core::Result<()> {
        Ok(())
    }

    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Traduit un refus du shell.
fn recycle_error(code: HRESULT) -> RecycleError {
    let win32 = |error: windows::Win32::Foundation::WIN32_ERROR| error.to_hresult();
    // Les raisons pour lesquelles le shell renonce à la corbeille. Sans
    // Shinku, il proposerait à la place de supprimer définitivement.
    let no_bin = [
        COPYENGINE_E_RECYCLE_BIN_NOT_FOUND,
        COPYENGINE_E_RECYCLE_FORCE_NUKE,
        COPYENGINE_E_RECYCLE_PATH_TOO_LONG,
        COPYENGINE_E_RECYCLE_SIZE_TOO_BIG,
    ];
    let locked = [
        COPYENGINE_E_SHARING_VIOLATION_SRC,
        win32(ERROR_SHARING_VIOLATION),
        win32(ERROR_LOCK_VIOLATION),
    ];
    let denied = [COPYENGINE_E_ACCESS_DENIED_SRC, COPYENGINE_E_REQUIRES_ELEVATION, E_ACCESSDENIED];
    if no_bin.contains(&code) {
        RecycleError::Refused
    } else if locked.contains(&code) {
        RecycleError::Locked
    } else if denied.contains(&code) {
        RecycleError::AccessDenied
    } else {
        RecycleError::Failed(describe(code))
    }
}

/// Un code d'erreur de Windows, lisible dans un rapport.
fn describe(code: HRESULT) -> String {
    let message = code.message();
    let message = message.trim();
    if message.is_empty() {
        format!("erreur {:#010X}", code.0)
    } else {
        format!("{message} ({:#010X})", code.0)
    }
}

fn failed(error: windows::core::Error) -> RecycleError {
    recycle_error(error.code())
}

/// Le chemin que le shell a compris pour un élément.
fn parsed_path(item: &IShellItem) -> Option<PathBuf> {
    // SAFETY: `item` est un objet COM vivant ; l'appel n'a pas d'autre pointeur.
    let raw = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.ok()?;
    // SAFETY: en cas de succès, `raw` est une chaîne UTF-16 terminée par zéro,
    // allouée par le shell et valide jusqu'à sa libération ci-dessous.
    let text = OsString::from_wide(unsafe { raw.as_wide() });
    // SAFETY: la chaîne vient de `GetDisplayName`, qui demande de la libérer
    // avec `CoTaskMemFree`. Elle n'est plus utilisée ensuite.
    unsafe { CoTaskMemFree(Some(raw.0.cast::<c_void>().cast_const())) };
    to_internal_path(Path::new(&text)).ok()
}

/// Ce que `path` est, sans suivre aucun lien : un dossier, ou autre chose. Un
/// lien vers un dossier (jonction, lien symbolique) n'est jamais envoyé, ni un
/// dossier dont le chemin en traverse un : la corbeille déplacerait le lien, et
/// l'on ne saurait plus ce qui part.
fn is_folder(path: &Path) -> Result<bool, RecycleError> {
    use std::os::windows::fs::FileTypeExt;
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| RecycleError::Failed(error.to_string()))?;
    let kind = metadata.file_type();
    if kind.is_symlink_dir() {
        return Err(RecycleError::Failed("c'est un lien vers un dossier".to_owned()));
    }
    if !kind.is_dir() {
        return Ok(false);
    }
    match crate::fs::inspect_dir_chain(path) {
        Ok(crate::fs::PathState::RealDir) => Ok(true),
        _ => Err(RecycleError::Failed("un lien est sur le chemin de ce dossier".to_owned())),
    }
}

/// La clé des réglages de la corbeille du compte.
const BIN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket";
/// Éléments comptés au plus, pour peser un dossier.
const MAX_WEIGHED: u64 = 200_000;
/// Temps maximal pour peser un dossier.
const WEIGH_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Le point de montage du volume qui porte ce chemin (`C:\`, ou `C:\Montage\`).
fn mount_point(path: &str) -> Option<String> {
    let path = HSTRING::from(path);
    let mut buffer = [0u16; 1024];
    // SAFETY: `path` est une chaîne terminée par zéro ; `buffer` est un tampon
    // local dont la crate transmet la longueur.
    unsafe { GetVolumePathNameW(&path, &mut buffer) }.ok()?;
    let end = buffer.iter().position(|&unit| unit == 0)?;
    String::from_utf16(&buffer[..end]).ok()
}

/// Le GUID d'un volume, par son point de montage, sans accolades.
fn volume_guid(mount: &str) -> Option<String> {
    let mount = HSTRING::from(mount);
    let mut buffer = [0u16; 64];
    // SAFETY: `mount` est une chaîne terminée par zéro ; `buffer` est un tampon
    // local dont Windows ne dépasse pas la taille.
    unsafe { GetVolumeNameForVolumeMountPointW(&mount, &mut buffer) }.ok()?;
    let end = buffer.iter().position(|&unit| unit == 0)?;
    let name = String::from_utf16(&buffer[..end]).ok()?;
    name.split(['{', '}']).nth(1).map(str::to_owned)
}

/// La taille totale du volume monté là, si Windows la donne.
fn volume_total(mount: &str) -> Option<u64> {
    let mount = HSTRING::from(mount);
    let mut total = 0u64;
    // SAFETY: `mount` est une chaîne terminée par zéro ; le seul pointeur donné
    // désigne une variable locale vivante.
    unsafe { GetDiskFreeSpaceExW(&mount, None, Some(&raw mut total), None) }.ok()?;
    Some(total)
}

/// Une valeur `DWORD` de la corbeille ou de ses stratégies, lue sans rien
/// écrire. Absente ou illisible : `None`.
fn bin_dword(hive: Hive, key: &str, name: &str) -> Option<u32> {
    let key = open_key(hive, key, KEY_READ).ok()??;
    super::apps::dword(&key, name, &mut false).ok()?
}

/// La clé des stratégies de l'Explorateur (`NoRecycleFiles`).
const POLICY_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer";

/// Les réglages de la corbeille pour ce volume, lus sans rien écrire.
fn read_settings(mount: Option<&str>) -> BinSettings {
    let volume_key =
        mount.and_then(volume_guid).map(|guid| format!(r"{BIN_KEY}\Volume\{{{guid}}}"));
    let volume =
        |name: &str| volume_key.as_deref().and_then(|key| bin_dword(Hive::Current, key, name));
    // La stratégie du compte, puis celle de la machine : l'une ou l'autre à 1.
    let policy = [Hive::Current, Hive::Local]
        .into_iter()
        .filter_map(|hive| bin_dword(hive, POLICY_KEY, "NoRecycleFiles"))
        .max();
    BinSettings {
        nuke_global: bin_dword(Hive::Current, BIN_KEY, "NukeOnDelete"),
        nuke_volume: volume("NukeOnDelete"),
        policy_no_recycle: policy,
        max_capacity_mib: volume("MaxCapacity"),
    }
}

/// Ce que pèse ce qui part, sans suivre aucun lien. `None` si la mesure échoue
/// ou dépasse ses bornes.
fn weigh(path: &Path) -> Option<u64> {
    let started = std::time::Instant::now();
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.is_file() {
        return Some(metadata.len());
    }
    let (mut total, mut seen) = (0u64, 0u64);
    let mut pending = vec![path.to_path_buf()];
    let mut reader = DirReader::new();
    while let Some(folder) = pending.pop() {
        let mut inner: Vec<PathBuf> = Vec::new();
        let result = reader.read(&folder, None, &mut |name, meta| {
            seen += 1;
            // Au-delà des bornes, on ne descend plus et on ne compte plus.
            if seen > MAX_WEIGHED {
                return;
            }
            match meta.kind {
                EntryKind::File => total = total.saturating_add(meta.size),
                EntryKind::Dir if meta.is_walkable_dir() => inner.push(folder.join(name)),
                _ => {}
            }
        });
        if result.is_err() || seen > MAX_WEIGHED || started.elapsed() > WEIGH_LIMIT {
            return None;
        }
        pending.extend(inner);
    }
    Some(total)
}

/// La garde, **avant** de demander au shell (la décision est
/// [`bin_rule::judge`]) : la corbeille du compte est-elle désactivée
/// (`NukeOnDelete`, `NoRecycleFiles`), ou trop petite pour ce qui part ? Windows
/// supprimerait pour de bon, et ne le dirait qu'après. On vérifie d'abord ce
/// qu'est le chemin (un lien est refusé **avant** toute lecture), puis on le
/// mesure, toujours, et le volume est celui du chemin, pas celui de son lecteur.
/// Ne fait que lire.
fn bin_guard(path: &Path) -> Result<(), RecycleError> {
    is_folder(path)?;
    let size = weigh(path)
        .ok_or_else(|| RecycleError::Failed("la taille de l'élément ne se mesure pas".into()))?;
    let shown = to_display_path(path).to_string_lossy().into_owned();
    let mount = mount_point(&shown);
    let settings = read_settings(mount.as_deref());
    bin_rule::judge(&settings, size, mount.as_deref().and_then(volume_total))
}

/// Envoie un fichier ou un dossier à la corbeille, **sans** la garde de taille
/// ([`bin_guard`]) : ce que Windows fait seul, pour un essai. `extra` s'ajoute
/// aux drapeaux. À appeler sur un fil où COM est prêt.
pub(super) fn recycle_unchecked(
    path: &Path,
    extra: FILEOPERATION_FLAGS,
) -> (Result<Sent, RecycleError>, Facts) {
    let seen = Rc::new(Seen::default());
    let result = send(path, extra, &seen);
    let facts = Facts {
        pre_flags: seen.pre_flags.get(),
        post: seen.post.get().map(|(flags, result, in_bin)| (flags, result.0, in_bin)),
    };
    (result, facts)
}

/// Envoie un fichier ou un dossier à la corbeille, après la garde de taille.
/// À appeler sur un fil où COM est prêt.
fn recycle_here(path: &Path) -> Result<Sent, RecycleError> {
    bin_guard(path)?;
    recycle_unchecked(path, FILEOPERATION_FLAGS(0)).0
}

fn send(path: &Path, extra: FILEOPERATION_FLAGS, seen: &Rc<Seen>) -> Result<Sent, RecycleError> {
    // Le shell ne comprend que la forme habituelle des chemins (`C:\…`).
    let display = HSTRING::from(to_display_path(path).as_os_str());
    let folder = is_folder(path)?;

    // SAFETY: COM est initialisé sur ce fil ; l'identifiant de classe est une
    // constante et aucun objet englobant n'est donné.
    let operation: IFileOperation =
        unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) }.map_err(failed)?;
    // SAFETY: `operation` est un objet COM vivant ; l'appel ne prend qu'un nombre.
    unsafe {
        operation.SetOperationFlags(FILEOPERATION_FLAGS(
            if folder { FOLDER_FLAGS } else { RECYCLE_FLAGS }.0 | extra.0,
        ))
    }
    .map_err(failed)?;

    // SAFETY: `display` est une chaîne UTF-16 terminée par zéro, vivante
    // pendant l'appel ; aucun contexte de liaison n'est donné.
    let item: IShellItem =
        unsafe { SHCreateItemFromParsingName(&display, None) }.map_err(failed)?;

    // Le shell normalise les noms : il retire par exemple un point ou un
    // espace final, et désignerait alors un autre fichier que celui que
    // Shinku a vérifié. On ne continue que si le chemin qu'il a compris mène
    // au même fichier sur le disque. Les deux écritures peuvent différer sans
    // danger (un nom court comme `RUNNER~1` devient le nom long) : c'est le
    // fichier désigné qui compte.
    let same_file = |understood: &Path| {
        match (crate::fs::identity(understood), crate::fs::identity(path)) {
            (Ok(Some(theirs)), Ok(Some(ours))) => {
                theirs.volume == ours.volume && same_file_id(theirs.file_id, ours.file_id)
            }
            // Sans identifiant à comparer, seule une écriture identique rassure.
            _ => fold_path(path).is_some() && fold_path(understood) == fold_path(path),
        }
    };
    if !parsed_path(&item).is_some_and(|understood| same_file(&understood)) {
        return Err(RecycleError::Failed(
            "Windows désigne un autre fichier sous ce nom".to_owned(),
        ));
    }

    let sink: IFileOperationProgressSink = Sink { seen: Rc::clone(seen) }.into();
    // SAFETY: `operation`, `item` et `sink` sont des objets COM vivants, que
    // l'opération retient elle-même tant qu'elle en a besoin.
    unsafe { operation.DeleteItem(&item, &sink) }.map_err(failed)?;
    // SAFETY: `operation` est un objet COM vivant. L'appel est synchrone : il
    // rend la main une fois la suppression faite ou abandonnée.
    let performed = unsafe { operation.PerformOperations() };
    // SAFETY: `operation` est un objet COM vivant ; le résultat est écrit dans
    // une variable que la crate fournit.
    let aborted = unsafe { operation.GetAnyOperationsAborted() };

    if seen.refused.get() {
        return Err(RecycleError::Refused);
    }
    performed.map_err(failed)?;
    match seen.post.get() {
        Some((_, result, _)) if result.is_err() => Err(recycle_error(result)),
        Some(_) if !aborted.is_ok_and(|aborted| !aborted.as_bool()) => {
            Err(RecycleError::Failed("Windows a abandonné l'opération".to_owned()))
        }
        // Le code de réussite ne distingue rien : sur le vrai Windows, un envoi
        // réussi à la corbeille et une suppression définitive rendent le même
        // (`0x00270008`, vu le 16 novembre 2026). Ce qui les sépare est
        // l'élément de corbeille que reçoit `PostDeleteItem` : présent,
        // l'élément est restaurable.
        Some((_, _, true)) => Ok(Sent::InBin),
        // Sans élément de corbeille : ou bien Windows a supprimé pour de bon,
        // ou bien il n'a rien fait. Le disque tranche, et le doute est un échec.
        Some((_, result, false)) => match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Sent::Destroyed),
            _ => Err(RecycleError::Failed(format!(
                "Windows dit avoir fini sans rien mettre à la corbeille, et l'élément est \
                 toujours là ({})",
                describe(result)
            ))),
        },
        None => Err(RecycleError::Failed("Windows a abandonné l'opération".to_owned())),
    }
}

/// Envoie un fichier ou un dossier entier à la corbeille. `path` est sous forme
/// interne.
pub(super) fn recycle(path: &Path) -> Result<Sent, RecycleError> {
    if !path.is_absolute() {
        return Err(RecycleError::Failed("chemin relatif".to_owned()));
    }
    let path = path.to_path_buf();
    on_shell_thread(move || recycle_here(&path)).unwrap_or(Err(RecycleError::Unavailable))
}

/// Ce que contient la corbeille d'un disque (`C:\`), si Windows le dit.
pub(super) fn bin_of(root: &HSTRING) -> Option<RecycleBin> {
    let mut info =
        SHQUERYRBINFO { cbSize: size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    // SAFETY: `root` est une chaîne UTF-16 terminée par zéro et `info` une
    // structure locale dont le champ `cbSize` annonce la taille.
    unsafe { SHQueryRecycleBinW(root, &raw mut info) }.ok()?;
    // Copies par valeur : la structure peut être compactée.
    let (bytes, items) = (info.i64Size, info.i64NumItems);
    Some(RecycleBin { bytes: bytes.max(0) as u64, items: items.max(0) as u64 })
}

/// Vide la corbeille des disques donnés (`C:\`…).
pub(super) fn empty_bins(roots: Vec<String>) -> io::Result<()> {
    let emptied = on_shell_thread(move || {
        let mut failure = None;
        for root in roots {
            let root = HSTRING::from(root);
            // Rien à vider, ou une corbeille que Shinku ne mesure pas : on
            // n'y touche pas.
            if bin_of(&root).is_none_or(|bin| bin.items == 0) {
                continue;
            }
            // SAFETY: `root` est une chaîne UTF-16 terminée par zéro, vivante
            // pendant l'appel ; aucune fenêtre n'est donnée.
            let emptied = unsafe {
                SHEmptyRecycleBinW(
                    None,
                    &root,
                    SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
                )
            };
            // Windows échoue parfois alors que la corbeille s'est bien vidée :
            // seul compte ce qu'il y reste.
            if let Err(error) = emptied
                && bin_of(&root).is_none_or(|bin| bin.items > 0)
            {
                failure.get_or_insert(describe(error.code()));
            }
        }
        failure
    });
    match emptied {
        Some(None) => Ok(()),
        Some(Some(reason)) => Err(io::Error::other(reason)),
        None => Err(io::Error::other("le shell de Windows ne répond pas")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_refusals_are_told_apart() {
        assert!(matches!(recycle_error(COPYENGINE_E_RECYCLE_SIZE_TOO_BIG), RecycleError::Refused));
        assert!(matches!(recycle_error(COPYENGINE_E_RECYCLE_BIN_NOT_FOUND), RecycleError::Refused));
        assert!(matches!(recycle_error(COPYENGINE_E_SHARING_VIOLATION_SRC), RecycleError::Locked));
        assert!(matches!(
            recycle_error(ERROR_SHARING_VIOLATION.to_hresult()),
            RecycleError::Locked
        ));
        assert!(matches!(recycle_error(E_ACCESSDENIED), RecycleError::AccessDenied));
        match recycle_error(HRESULT(0x8000_FFFF_u32 as i32)) {
            RecycleError::Failed(reason) => assert!(reason.contains("0x8000FFFF"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_relative_path_is_never_sent_to_the_shell() {
        assert!(matches!(recycle(Path::new("fichier.txt")), Err(RecycleError::Failed(_))));
    }

    /// La vraie corbeille de Windows, sur des fichiers fabriqués pour l'essai.
    ///
    /// Sur demande seulement (`SHINKU_ESSAI_CORBEILLE=1`) : l'essai laisse
    /// trois petits fichiers dans la corbeille de la machine. Avec
    /// `SHINKU_ESSAI_VIDE_LA_CORBEILLE=1` en plus, il la vide ensuite pour de
    /// bon : à réserver à une machine jetable.
    #[test]
    fn the_real_recycle_bin_receives_one_file_and_keeps_it() {
        use std::io::Read;
        use std::os::windows::fs::OpenOptionsExt;

        if std::env::var_os("SHINKU_ESSAI_CORBEILLE").is_none() {
            return;
        }
        let _serial = bin_lock();
        let workspace = tempfile::tempdir().unwrap();
        let root = to_internal_path(workspace.path()).unwrap();
        let drive: String = to_display_path(&root).to_string_lossy().chars().take(3).collect();
        let drive = HSTRING::from(drive);
        let before = bin_of(&drive).expect("Windows mesure la corbeille de ce disque");

        // Un fichier que le moteur tient ouvert, comme pendant le grand vide.
        let photo = root.join("photo de l'essai.bin");
        std::fs::write(&photo, [7u8; 5000]).unwrap();
        let mut held = std::fs::OpenOptions::new().read(true).share_mode(0x7).open(&photo).unwrap();
        let sent = recycle(&photo).expect("un fichier ouvert en partage part à la corbeille");
        assert_eq!(sent, Sent::InBin, "dans la corbeille, pas supprimé pour de bon");
        assert!(!photo.exists(), "le fichier a quitté sa place");
        let mut content = Vec::new();
        held.read_to_end(&mut content).unwrap();
        assert_eq!(content, [7u8; 5000], "il existe toujours : il est dans la corbeille");
        let after = bin_of(&drive).unwrap();
        assert_eq!(after.items, before.items + 1, "la corbeille compte un élément de plus");
        assert!(after.bytes >= before.bytes + 5000, "mesurée en tailles de fichiers");

        // Une page web enregistrée : son dossier associé ne part pas avec elle.
        let page = root.join("page.htm");
        let attached = root.join("page_files");
        std::fs::write(&page, b"<html></html>").unwrap();
        std::fs::create_dir(&attached).unwrap();
        std::fs::write(attached.join("image.png"), b"png").unwrap();
        recycle(&page).unwrap();
        assert!(!page.exists());
        assert!(attached.join("image.png").exists(), "le dossier associé est resté");

        // Un fichier tenu sans partage : il reste, et le refus se lit.
        let busy = root.join("occupé.bin");
        std::fs::write(&busy, b"x").unwrap();
        let exclusive = std::fs::OpenOptions::new().read(true).share_mode(0).open(&busy).unwrap();
        let refused = recycle(&busy);
        assert!(matches!(refused, Err(RecycleError::Locked)), "{refused:?}");
        assert!(busy.exists());
        drop(exclusive);

        // Un nom que le shell lirait autrement (point final) : jamais envoyé.
        let dotted = root.join("note.");
        assert!(recycle(&dotted).is_err());

        if std::env::var_os("SHINKU_ESSAI_VIDE_LA_CORBEILLE").is_some() {
            empty_bins(vec![drive.to_string()]).expect("la corbeille se vide");
            assert_eq!(bin_of(&drive).map(|bin| bin.items), Some(0));
            // Vidée, elle a lâché le fichier que l'essai tenait encore.
            drop(held);
        }
    }

    /// La vraie corbeille de Windows, sur des **dossiers** fabriqués pour
    /// l'essai : le module « Désinstallation » y envoie des restes d'apps.
    ///
    /// Trois choses à prouver avant de s'y fier : un dossier part **en un seul
    /// élément** ; un dossier dont un fichier est tenu ouvert sans partage
    /// **reste entier** (jamais à moitié) ; un dossier qui contient une
    /// jonction part sans que la cible de la jonction soit touchée (Shinku ne
    /// laisse de toute façon jamais arriver un tel dossier jusqu'ici : l'essai
    /// prouve la seconde barrière). Sur demande seulement, comme l'essai
    /// ci-dessus (`SHINKU_ESSAI_CORBEILLE=1`) : il laisse deux dossiers dans la
    /// corbeille de la machine. Sous Wine, la corbeille du shell ne se prouve
    /// pas : cet essai ne compte que sur un vrai Windows.
    #[test]
    fn the_real_recycle_bin_receives_a_whole_folder_or_nothing() {
        use std::os::windows::fs::OpenOptionsExt;

        if std::env::var_os("SHINKU_ESSAI_CORBEILLE").is_none() {
            return;
        }
        let _serial = bin_lock();
        let workspace = tempfile::tempdir().unwrap();
        let root = to_internal_path(workspace.path()).unwrap();
        let drive: String = to_display_path(&root).to_string_lossy().chars().take(3).collect();
        let drive = HSTRING::from(drive);
        let before = bin_of(&drive).expect("Windows mesure la corbeille de ce disque");

        // Un dossier de trois niveaux part, en un seul élément.
        let tree = root.join("dossier de l'essai");
        std::fs::create_dir_all(tree.join("a").join("b")).unwrap();
        std::fs::write(tree.join("racine.txt"), [1u8; 2000]).unwrap();
        std::fs::write(tree.join("a").join("un.txt"), [2u8; 3000]).unwrap();
        std::fs::write(tree.join("a").join("b").join("deux.txt"), [3u8; 4000]).unwrap();
        let sent = recycle(&tree).expect("un dossier ordinaire part à la corbeille");
        assert_eq!(sent, Sent::InBin, "dans la corbeille, pas supprimé pour de bon");
        assert!(!tree.exists(), "le dossier entier a quitté sa place");
        let after = bin_of(&drive).unwrap();
        assert_eq!(after.items, before.items + 1, "un dossier est un seul élément");
        assert!(after.bytes >= before.bytes + 9000, "avec tout ce qu'il contient");

        // Un fichier tenu sans partage : le dossier reste entier, et le refus se lit.
        let busy = root.join("dossier occupé");
        std::fs::create_dir_all(busy.join("sous")).unwrap();
        std::fs::write(busy.join("libre.txt"), b"libre").unwrap();
        std::fs::write(busy.join("sous").join("tenu.bin"), b"tenu").unwrap();
        std::fs::write(busy.join("sous").join("autre.txt"), b"autre").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(busy.join("sous").join("tenu.bin"))
            .unwrap();
        let refused = recycle(&busy);
        assert!(refused.is_err(), "un dossier dont un fichier est tenu ne part pas : {refused:?}");
        for file in ["libre.txt", "sous\\tenu.bin", "sous\\autre.txt"] {
            assert!(busy.join(file).exists(), "{file} : le dossier n'est pas parti à moitié");
        }
        assert_eq!(
            bin_of(&drive).unwrap().items,
            after.items,
            "rien n'est entré dans la corbeille"
        );
        drop(held);

        // Un dossier qui contient une jonction : la cible n'est pas touchée.
        let target = root.join("cible de la jonction");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("precieux.txt"), b"precieux").unwrap();
        let holder = root.join("dossier à jonction");
        std::fs::create_dir(&holder).unwrap();
        let link = holder.join("lien");
        let made = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(to_display_path(&link))
            .arg(to_display_path(&target))
            .output()
            .expect("cmd.exe se lance");
        assert!(made.status.success(), "mklink /J : {made:?}");
        // La jonction elle-même n'est jamais envoyée.
        assert!(recycle(&link).is_err(), "un lien vers un dossier n'est pas envoyé");
        assert!(target.join("precieux.txt").exists());
        // Le dossier qui la contient part, et la cible reste où elle est.
        let sent = recycle(&holder);
        assert!(target.join("precieux.txt").exists(), "la cible de la jonction est intacte");
        assert_eq!(
            std::fs::read(target.join("precieux.txt")).unwrap(),
            b"precieux",
            "et son contenu aussi"
        );
        assert!(matches!(sent, Ok(Sent::InBin)), "{sent:?}");
        assert!(!holder.exists());

        if std::env::var_os("SHINKU_ESSAI_VIDE_LA_CORBEILLE").is_some() {
            empty_bins(vec![drive.to_string()]).expect("la corbeille se vide");
            assert_eq!(bin_of(&drive).map(|bin| bin.items), Some(0));
            assert!(target.join("precieux.txt").exists(), "vider ne suit pas une jonction");
        }
    }

    /// Une valeur `DWORD` de la corbeille du compte, notée avant d'être changée
    /// et **remise quoi qu'il arrive** (fin normale ou panique). Si l'ancienne
    /// valeur ne se lit pas pour une autre raison que « absente », l'essai
    /// s'arrête **avant** de rien changer. Une clé que l'essai a dû créer est
    /// retirée à la fin si elle est vide.
    struct BinSetting {
        key: String,
        name: &'static str,
        old: Option<u32>,
        created_key: bool,
    }

    fn reg(args: &[&str]) -> std::process::Output {
        std::process::Command::new("reg.exe").args(args).output().expect("reg.exe se lance")
    }

    impl BinSetting {
        /// `key` s'écrit `HKCU\…`.
        fn set(key: &str, name: &'static str, value: u32) -> Self {
            let path = key.strip_prefix(r"HKCU\").expect("une clé du compte");
            let (old, created_key) = match open_key(Hive::Current, path, KEY_READ) {
                Ok(Some(opened)) => match super::super::apps::dword(&opened, name, &mut false) {
                    Ok(old) => (old, false),
                    Err(error) => {
                        panic!("{key}\\{name} ne se lit pas ({error}) : rien n'est changé")
                    }
                },
                Ok(None) => (None, true),
                Err(error) => panic!("{key} ne s'ouvre pas ({error}) : rien n'est changé"),
            };
            println!("corbeille : {key}\\{name} valait {old:?}, passe à {value}");
            let done =
                reg(&["add", key, "/v", name, "/t", "REG_DWORD", "/d", &value.to_string(), "/f"]);
            assert!(done.status.success(), "reg add {key} : {done:?}");
            Self { key: key.to_owned(), name, old, created_key }
        }
    }

    impl Drop for BinSetting {
        fn drop(&mut self) {
            let result = match self.old {
                Some(old) => reg(&[
                    "add",
                    &self.key,
                    "/v",
                    self.name,
                    "/t",
                    "REG_DWORD",
                    "/d",
                    &old.to_string(),
                    "/f",
                ]),
                None => reg(&["delete", &self.key, "/v", self.name, "/f"]),
            };
            println!("corbeille : {}\\{} remis à {:?} ({result:?})", self.key, self.name, self.old);
            if self.created_key {
                // Une clé créée par l'essai est retirée si elle est vide (aucune
                // valeur, aucune sous-clé : une seule ligne, son nom).
                let listing = reg(&["query", &self.key]);
                let lines = String::from_utf8_lossy(&listing.stdout)
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .count();
                if listing.status.success() && lines <= 1 {
                    println!(
                        "corbeille : {} retirée (vide) : {:?}",
                        self.key,
                        reg(&["delete", &self.key, "/f"])
                    );
                }
            }
        }
    }

    /// Les essais de la vraie corbeille ne tournent **jamais en même temps** :
    /// ils changent et comptent la même corbeille.
    static BIN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn bin_lock() -> std::sync::MutexGuard<'static, ()> {
        BIN_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Le disque de l'essai, et les clés de sa corbeille : `(disque, BitBucket,
    /// BitBucket\Volume\{guid})`.
    fn bin_keys(root: &Path) -> (String, String, String) {
        let drive: String = to_display_path(root).to_string_lossy().chars().take(3).collect();
        // `mountvol C:\ /L` rend `\\?\Volume{guid}\`.
        let mounted = std::process::Command::new("mountvol.exe")
            .args([drive.as_str(), "/L"])
            .output()
            .expect("mountvol.exe se lance");
        let volume = String::from_utf8_lossy(&mounted.stdout).trim().to_owned();
        let guid = volume
            .split(['{', '}'])
            .nth(1)
            .unwrap_or_else(|| panic!("mountvol ne rend pas de volume : {volume:?}"))
            .to_owned();
        let bin = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket";
        println!("essai : disque {drive}, volume {{{guid}}}");
        (drive, bin.to_owned(), format!(r"{bin}\Volume\{{{guid}}}"))
    }

    /// Un fichier de 3 Mio et un dossier de trois niveaux (3 Mio), pour l'essai.
    fn big_things(root: &Path, why: &str) -> (PathBuf, PathBuf) {
        let file = root.join(format!("fichier {why}.bin"));
        std::fs::write(&file, vec![7u8; 3 * 1024 * 1024]).unwrap();
        let tree = root.join(format!("dossier {why}"));
        std::fs::create_dir_all(tree.join("a").join("b")).unwrap();
        std::fs::write(tree.join("racine.bin"), vec![1u8; 3 * 1024 * 1024]).unwrap();
        std::fs::write(tree.join("a").join("b").join("fond.txt"), b"fond").unwrap();
        (file, tree)
    }

    /// Le fichier et le dossier sont refusés (`Refused` ou `Unavailable`), restent
    /// **intacts** sur le disque, et la corbeille n'a rien reçu.
    fn assert_refused_and_intact(root: &Path, drive: &str, why: &str) {
        let drive = HSTRING::from(drive);
        let (file, tree) = big_things(root, why);
        for (label, path) in [("le fichier", &file), ("le dossier", &tree)] {
            let before = bin_of(&drive).map(|bin| bin.items);
            let sent = recycle(path);
            let after = bin_of(&drive).map(|bin| bin.items);
            let here = path.exists();
            println!(
                "essai ({why}) : {label} envoyé, réponse {sent:?}, sur le disque : {here}, \
                 éléments de la corbeille : {before:?} puis {after:?}"
            );
            assert!(
                matches!(sent, Err(RecycleError::Refused | RecycleError::Unavailable))
                    && here
                    && before == after,
                "{why} : {label} devait être refusé et rester intact ; réponse {sent:?}, sur le \
                 disque : {here}, corbeille : {before:?} puis {after:?}"
            );
            println!("essai ({why}) : {label} : refus, rien n'est parti");
        }
        assert_eq!(std::fs::read(tree.join("a").join("b").join("fond.txt")).unwrap(), b"fond");
        assert_eq!(std::fs::metadata(tree.join("racine.bin")).unwrap().len(), 3 * 1024 * 1024);
    }

    /// Une corbeille **désactivée** (`NukeOnDelete`, globale et pour le volume,
    /// comme la case « Ne pas déplacer les fichiers vers la Corbeille » de
    /// Windows) : Shinku refuse et laisse le fichier et le dossier intacts.
    /// Sur demande seulement (`SHINKU_ESSAI_CORBEILLE=1`), machine jetable : les
    /// valeurs changées sont remises ensuite, même si l'essai échoue.
    #[test]
    fn a_disabled_recycle_bin_refuses_and_never_deletes() {
        if std::env::var_os("SHINKU_ESSAI_CORBEILLE").is_none() {
            return;
        }
        let _serial = bin_lock();
        let workspace = tempfile::tempdir().unwrap();
        let root = to_internal_path(workspace.path()).unwrap();
        let (drive, bin, volume_key) = bin_keys(&root);
        let _global = BinSetting::set(&bin, "NukeOnDelete", 1);
        let _volume = BinSetting::set(&volume_key, "NukeOnDelete", 1);
        assert_refused_and_intact(&root, &drive, "corbeille désactivée");
        // Sans la garde : ce que Windows fait seul est, lui aussi, un refus.
        let drive_text = HSTRING::from(drive.as_str());
        let (file, tree) = big_things(&root, "désactivée sans garde");
        for (label, path) in [("le fichier", &file), ("le dossier", &tree)] {
            let before = bin_of(&drive_text).map(|bin| bin.items);
            let owned = path.clone();
            let (sent, facts) =
                on_shell_thread(move || recycle_unchecked(&owned, FILEOPERATION_FLAGS(0)))
                    .expect("le fil du shell");
            let after = bin_of(&drive_text).map(|bin| bin.items);
            let from_sink = facts
                .pre_flags
                .is_some_and(|flags| flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0);
            println!(
                "{} ; le refus vient du {} ; éléments de la corbeille : {before:?} puis {after:?}",
                fact_line(
                    &format!("{label}, corbeille désactivée, sans la garde"),
                    &sent,
                    &facts,
                    path.exists()
                ),
                if from_sink {
                    "puits (PreDeleteItem sans le drapeau)"
                } else {
                    "code d'erreur du shell"
                },
            );
            assert!(
                matches!(sent, Err(RecycleError::Refused)) && path.exists() && before == after,
                "{label} : sans la garde, Windows devait refuser et tout laisser intact ; \
                 réponse {sent:?}, sur le disque : {}, corbeille : {before:?} puis {after:?}",
                path.exists()
            );
        }
    }

    /// Une corbeille **trop petite** (`MaxCapacity` du volume, 1 Mio, pour 3 Mio) :
    /// la garde de taille de Shinku refuse avant de demander au shell, tout reste
    /// intact. Le vrai Windows a montré que, sans elle, le shell supprime pour de
    /// bon : voir l'essai d'observation ci-dessous.
    #[test]
    fn a_too_small_recycle_bin_is_refused_by_the_guard() {
        if std::env::var_os("SHINKU_ESSAI_CORBEILLE").is_none() {
            return;
        }
        let _serial = bin_lock();
        let workspace = tempfile::tempdir().unwrap();
        let root = to_internal_path(workspace.path()).unwrap();
        let (drive, _bin, volume_key) = bin_keys(&root);
        let _small = BinSetting::set(&volume_key, "MaxCapacity", 1);
        assert_refused_and_intact(&root, &drive, "corbeille trop petite");
    }

    /// Ce que le shell a fait, sur une ligne facile à retrouver dans le journal.
    fn fact_line(
        what: &str,
        sent: &Result<Sent, RecycleError>,
        facts: &Facts,
        here: bool,
    ) -> String {
        let recycle_if_possible =
            facts.pre_flags.map(|flags| flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 != 0);
        format!(
            "FAIT : {what} : PreDeleteItem drapeaux {} (TSF_DELETE_RECYCLE_IF_POSSIBLE : {:?}) ; \
             PostDeleteItem {} ; réponse {sent:?} ; sur le disque : {here}",
            facts.pre_flags.map_or("aucun".to_owned(), |flags| format!("{flags:#x}")),
            recycle_if_possible,
            facts.post.map_or("aucun".to_owned(), |(flags, result, in_bin)| format!(
                "drapeaux {flags:#x}, résultat {result:#010X}, élément de corbeille présent : {in_bin}"
            )),
        )
    }

    /// Ce que Windows fait **seul** (sans la garde de taille) avec une corbeille
    /// trop petite : il imprime les faits (`FAIT :`), et **prouve** ce sur quoi
    /// repose [`Sent`]. Si l'élément a disparu et que la corbeille n'a rien reçu
    /// de plus, la fonction DOIT avoir rendu `Destroyed` ; si la corbeille compte
    /// un élément de plus, elle DOIT avoir rendu `InBin`. Les essais passent un
    /// par un : le compte est fiable.
    #[test]
    fn what_windows_does_alone_with_a_too_small_bin_is_proved() {
        if std::env::var_os("SHINKU_ESSAI_CORBEILLE").is_none() {
            return;
        }
        let _serial = bin_lock();
        let workspace = tempfile::tempdir().unwrap();
        let root = to_internal_path(workspace.path()).unwrap();
        let (drive, _bin, volume_key) = bin_keys(&root);
        let _small = BinSetting::set(&volume_key, "MaxCapacity", 1);
        let (file, tree) = big_things(&root, "observation");
        let drive_text = HSTRING::from(drive.as_str());
        for (label, path) in [("le fichier", &file), ("le dossier", &tree)] {
            let before = bin_of(&drive_text).map(|bin| bin.items);
            let owned = path.clone();
            let (sent, facts) =
                on_shell_thread(move || recycle_unchecked(&owned, FILEOPERATION_FLAGS(0)))
                    .expect("le fil du shell");
            let after = bin_of(&drive_text).map(|bin| bin.items);
            let gone = !path.exists();
            println!(
                "{} ; éléments de la corbeille : {before:?} puis {after:?}",
                fact_line(
                    &format!("{label}, corbeille trop petite, sans la garde"),
                    &sent,
                    &facts,
                    !gone
                )
            );
            // Sans les deux comptes, l'essai ne prouverait rien : il échoue.
            let before = before.expect("Windows compte la corbeille avant l'envoi");
            let after = after.expect("Windows compte la corbeille après l'envoi");
            if !gone {
                assert!(sent.is_err(), "{label} : encore là, mais réponse {sent:?}");
            } else if after > before {
                assert!(
                    matches!(sent, Ok(Sent::InBin)),
                    "{label} : entré dans la corbeille : `InBin` était attendu, réponse {sent:?}"
                );
            } else {
                // Autant ou moins qu'avant : rien de plus n'y est entré.
                assert!(
                    matches!(sent, Ok(Sent::Destroyed)),
                    "{label} : disparu sans entrer dans la corbeille : `Destroyed` était \
                     attendu, réponse {sent:?}"
                );
            }
            if matches!(sent, Ok(Sent::InBin))
                && std::env::var_os("SHINKU_ESSAI_VIDE_LA_CORBEILLE").is_some()
            {
                let _ = empty_bins(vec![drive.clone()]);
            }
        }
    }

    /// Une corbeille **assez grande** accepte : avec `MaxCapacity` à 8 (Mio), un
    /// fichier de 1 Mio part dans la corbeille. Tue la confusion Mio/Kio aussi
    /// sur le vrai Windows.
    #[test]
    fn a_big_enough_recycle_bin_accepts() {
        if std::env::var_os("SHINKU_ESSAI_CORBEILLE").is_none() {
            return;
        }
        let _serial = bin_lock();
        let workspace = tempfile::tempdir().unwrap();
        let root = to_internal_path(workspace.path()).unwrap();
        let (drive, _bin, volume_key) = bin_keys(&root);
        let _roomy = BinSetting::set(&volume_key, "MaxCapacity", 8);
        let file = root.join("un mio.bin");
        std::fs::write(&file, vec![5u8; 1024 * 1024]).unwrap();
        let sent = recycle(&file);
        println!(
            "essai : 1 Mio dans une corbeille de 8 Mio : {sent:?}, sur le disque : {}",
            file.exists()
        );
        assert!(matches!(sent, Ok(Sent::InBin)), "{sent:?}");
        assert!(!file.exists());
        if std::env::var_os("SHINKU_ESSAI_VIDE_LA_CORBEILLE").is_some() {
            let _ = empty_bins(vec![drive]);
        }
    }

    #[test]
    fn the_shell_thread_gives_back_what_the_work_returns() {
        // Sous Wine comme sous Windows, COM s'initialise sur un fil neuf.
        assert_eq!(on_shell_thread(|| 6 * 7), Some(42));
    }
}
