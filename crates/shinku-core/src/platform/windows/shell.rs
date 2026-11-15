//! La corbeille de Windows : y envoyer un fichier, la vider.
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
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    COPYENGINE_E_ACCESS_DENIED_SRC, COPYENGINE_E_RECYCLE_BIN_NOT_FOUND,
    COPYENGINE_E_RECYCLE_FORCE_NUKE, COPYENGINE_E_RECYCLE_PATH_TOO_LONG,
    COPYENGINE_E_RECYCLE_SIZE_TOO_BIG, COPYENGINE_E_REQUIRES_ELEVATION,
    COPYENGINE_E_SHARING_VIOLATION_SRC, FILEOPERATION_FLAGS, FOF_ALLOWUNDO,
    FOF_NO_CONNECTED_ELEMENTS, FOF_NO_UI, FOFX_ADDUNDORECORD, FOFX_RECYCLEONDELETE, FileOperation,
    IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem,
    SHCreateItemFromParsingName, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
    SHEmptyRecycleBinW, SHQUERYRBINFO, SHQueryRecycleBinW, SIGDN_FILESYSPATH,
    TSF_DELETE_RECYCLE_IF_POSSIBLE,
};
use windows::core::{HRESULT, HSTRING, PCWSTR, Ref, implement};

use super::super::{RecycleBin, RecycleError};
use crate::fs::{fold_path, same_file_id, to_display_path, to_internal_path};

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
const RECYCLE_FLAGS: FILEOPERATION_FLAGS = FILEOPERATION_FLAGS(
    FOF_NO_UI.0
        | FOF_ALLOWUNDO.0
        | FOFX_ADDUNDORECORD.0
        | FOFX_RECYCLEONDELETE.0
        | FOF_NO_CONNECTED_ELEMENTS.0,
);

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
    /// Résultat de la suppression, une fois faite.
    deleted: Cell<Option<HRESULT>>,
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
        if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            self.seen.refused.set(true);
            return Err(E_ABORT.into());
        }
        Ok(())
    }

    fn PostDeleteItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        result: HRESULT,
        _in_bin: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        self.seen.deleted.set(Some(result));
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

/// Envoie un fichier à la corbeille. À appeler sur un fil où COM est prêt.
fn recycle_here(path: &Path) -> Result<(), RecycleError> {
    // Le shell ne comprend que la forme habituelle des chemins (`C:\…`).
    let display = HSTRING::from(to_display_path(path).as_os_str());

    // SAFETY: COM est initialisé sur ce fil ; l'identifiant de classe est une
    // constante et aucun objet englobant n'est donné.
    let operation: IFileOperation =
        unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) }.map_err(failed)?;
    // SAFETY: `operation` est un objet COM vivant ; l'appel ne prend qu'un nombre.
    unsafe { operation.SetOperationFlags(RECYCLE_FLAGS) }.map_err(failed)?;

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

    let seen = Rc::new(Seen::default());
    let sink: IFileOperationProgressSink = Sink { seen: Rc::clone(&seen) }.into();
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
    match seen.deleted.get() {
        Some(result) if result.is_err() => Err(recycle_error(result)),
        Some(_) if aborted.is_ok_and(|aborted| !aborted.as_bool()) => Ok(()),
        _ => Err(RecycleError::Failed("Windows a abandonné l'opération".to_owned())),
    }
}

/// Envoie un fichier à la corbeille. `path` est sous forme interne.
pub(super) fn recycle(path: &Path) -> Result<(), RecycleError> {
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
        let workspace = tempfile::tempdir().unwrap();
        let root = to_internal_path(workspace.path()).unwrap();
        let drive: String = to_display_path(&root).to_string_lossy().chars().take(3).collect();
        let drive = HSTRING::from(drive);
        let before = bin_of(&drive).expect("Windows mesure la corbeille de ce disque");

        // Un fichier que le moteur tient ouvert, comme pendant le grand vide.
        let photo = root.join("photo de l'essai.bin");
        std::fs::write(&photo, [7u8; 5000]).unwrap();
        let mut held = std::fs::OpenOptions::new().read(true).share_mode(0x7).open(&photo).unwrap();
        recycle(&photo).expect("un fichier ouvert en partage part à la corbeille");
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

    #[test]
    fn the_shell_thread_gives_back_what_the_work_returns() {
        // Sous Wine comme sous Windows, COM s'initialise sur un fil neuf.
        assert_eq!(on_shell_thread(|| 6 * 7), Some(42));
    }
}
