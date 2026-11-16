//! Lecture du système de fichiers sous Windows, par handle.
//!
//! Chaque dossier est ouvert par un handle, avec l'option qui interdit de
//! traverser un lien, puis lu par blocs : taille, dates, attributs et
//! identifiant de chaque entrée arrivent avec la liste, sans ouvrir les fichiers.
#![allow(unsafe_code)]

use std::ffi::{OsStr, OsString, c_void};
use std::fs::{File, OpenOptions};
use std::io;
use std::mem::{offset_of, size_of, size_of_val};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, PathBuf, Prefix, PrefixComponent};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_DO_NOT_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX,
    FILE_DISPOSITION_INFO_EX_FLAGS, FILE_FULL_DIR_INFO, FILE_ID_BOTH_DIR_INFO,
    FILE_ID_EXTD_DIR_INFO, FILE_ID_INFO, FILE_INFO_BY_HANDLE_CLASS, FileAttributeTagInfo,
    FileBasicInfo, FileDispositionInfo, FileDispositionInfoEx, FileFullDirectoryInfo,
    FileIdBothDirectoryInfo, FileIdExtdDirectoryInfo, FileIdInfo, FileStandardInfo,
    GetFileInformationByHandleEx, SetFileInformationByHandle,
};

use super::attributes::{ATTR_REPARSE_POINT, classify};
use super::{
    EntryFlags, EntryKind, EntryMeta, Expected, Fingerprint, Identity, OpenError, ReadDirError,
    RemovalFacts, RemoveError, same_file_id,
};
use crate::model::{FileId, Timestamp};

// Droits d'accès et options de `CreateFileW` (valeurs documentées).
const FILE_LIST_DIRECTORY: u32 = 0x0001;
const FILE_READ_ATTRIBUTES: u32 = 0x0080;
/// Le droit de supprimer ou de renommer l'entrée ouverte.
const DELETE: u32 = 0x0001_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_SHARE_DELETE: u32 = 0x4;
/// On laisse les autres apps lire, écrire et supprimer pendant qu'on regarde :
/// Shinku ne doit jamais provoquer une erreur de partage chez quelqu'un d'autre.
const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
/// Nécessaire pour obtenir un handle sur un dossier.
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
/// Ouvre le lien lui-même au lieu de ce vers quoi il pointe.
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;

// Codes d'erreur Win32 utilisés ici.
const ERROR_INVALID_FUNCTION: i32 = 1;
const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_NO_MORE_FILES: i32 = 18;
const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;
const ERROR_NOT_SUPPORTED: i32 = 50;
const ERROR_INVALID_PARAMETER: i32 = 87;
const ERROR_CALL_NOT_IMPLEMENTED: i32 = 120;
const ERROR_INVALID_LEVEL: i32 = 124;
const ERROR_DIR_NOT_EMPTY: i32 = 145;

/// Taille du tampon de lecture d'un dossier : un gros tampon ramène des
/// centaines d'entrées par appel au système.
const BUFFER_BYTES: usize = 64 * 1024;

/// Convertit une erreur de la crate `windows` en erreur standard, en gardant
/// le code Win32 d'origine pour que `ErrorKind` soit juste.
fn to_io(error: windows::core::Error) -> io::Error {
    let hresult = error.code().0 as u32;
    if hresult >> 16 == 0x8007 {
        io::Error::from_raw_os_error((hresult & 0xFFFF) as i32)
    } else {
        io::Error::other(error)
    }
}

/// Une structure d'information qu'on peut demander pour un handle.
///
/// # Safety
///
/// `CLASS` doit être la classe dont le système remplit exactement une
/// structure de type `Self`, et tout motif de bits écrit par le système doit
/// être une valeur valide de `Self`.
unsafe trait HandleInfo: Default {
    const CLASS: FILE_INFO_BY_HANDLE_CLASS;
}

// SAFETY: `FileAttributeTagInfo` remplit une `FILE_ATTRIBUTE_TAG_INFO` (deux entiers).
unsafe impl HandleInfo for FILE_ATTRIBUTE_TAG_INFO {
    const CLASS: FILE_INFO_BY_HANDLE_CLASS = FileAttributeTagInfo;
}
// SAFETY: `FileBasicInfo` remplit une `FILE_BASIC_INFO` (des entiers).
unsafe impl HandleInfo for FILE_BASIC_INFO {
    const CLASS: FILE_INFO_BY_HANDLE_CLASS = FileBasicInfo;
}
// SAFETY: `FileIdInfo` remplit une `FILE_ID_INFO` (un entier et seize octets).
unsafe impl HandleInfo for FILE_ID_INFO {
    const CLASS: FILE_INFO_BY_HANDLE_CLASS = FileIdInfo;
}

/// Demande au système une information sur un fichier déjà ouvert.
fn query<T: HandleInfo>(file: &File) -> io::Result<T> {
    let mut info = T::default();
    // SAFETY: le handle appartient à `file`, qui vit pendant tout l'appel.
    // `info` est une structure du type attendu pour `T::CLASS` (garanti par
    // `HandleInfo`) et on annonce exactement sa taille.
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            T::CLASS,
            (&raw mut info).cast::<c_void>(),
            size_of::<T>() as u32,
        )
    }
    .map_err(to_io)?;
    Ok(info)
}

/// Ce que `FileStandardInfo` dit d'un fichier ouvert.
struct StandardInfo {
    size: u64,
    /// Nombre de noms que porte encore l'entrée. Zéro : elle a été supprimée,
    /// et ne survit que par les handles encore ouverts.
    links: u32,
    delete_pending: bool,
    directory: bool,
}

impl StandardInfo {
    /// Vrai si l'entrée n'existe plus pour personne d'autre : sa suppression
    /// est demandée, ou déjà faite.
    fn is_gone(&self) -> bool {
        self.delete_pending || self.links == 0
    }
}

/// Lit `FILE_STANDARD_INFO` dans un tampon brut : la structure contient deux
/// booléens d'un octet, et on ne veut rien supposer de la valeur exacte que le
/// système (ou un pilote) y écrit.
fn query_standard(file: &File) -> io::Result<StandardInfo> {
    // AllocationSize (8 octets), EndOfFile (8), NumberOfLinks (4),
    // DeletePending (1), Directory (1), puis du remplissage : 24 octets.
    let mut raw = [0u64; 3];
    // SAFETY: le handle appartient à `file`, vivant pendant l'appel. Le
    // tampon est une variable locale alignée sur 8 octets, dont on annonce la
    // taille exacte, suffisante pour la structure demandée.
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileStandardInfo,
            (&raw mut raw).cast::<c_void>(),
            size_of_val(&raw) as u32,
        )
    }
    .map_err(to_io)?;
    let flags = raw[2].to_le_bytes();
    Ok(StandardInfo {
        size: (raw[1] as i64).max(0) as u64,
        links: u32::from_le_bytes([flags[0], flags[1], flags[2], flags[3]]),
        delete_pending: flags[4] != 0,
        directory: flags[5] != 0,
    })
}

fn is_unsupported(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(
            ERROR_INVALID_FUNCTION
                | ERROR_NOT_SUPPORTED
                | ERROR_INVALID_PARAMETER
                | ERROR_CALL_NOT_IMPLEMENTED
                | ERROR_INVALID_LEVEL
        )
    )
}

/// Ouvre un handle sur ce que désigne `path` (fichier ou dossier), sans
/// suivre un lien final, avec les droits d'accès demandés.
fn open_no_follow(path: &Path, access: u32, extra_flags: u32) -> io::Result<File> {
    open_no_follow_sharing(path, access, SHARE_ALL, extra_flags)
}

/// Comme [`open_no_follow`], en choisissant ce que les autres programmes
/// peuvent faire de l'entrée pendant ce temps (`share`).
fn open_no_follow_sharing(
    path: &Path,
    access: u32,
    share: u32,
    extra_flags: u32,
) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(access)
        .share_mode(share)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | extra_flags)
        .open(path)
}

/// Les trois formes de liste de dossier, de la plus riche à la plus répandue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListClass {
    /// Identifiant sur 128 bits et étiquette de point d'analyse (NTFS, ReFS).
    Extended,
    /// Identifiant sur 64 bits.
    Both,
    /// Sans identifiant.
    Full,
}

impl ListClass {
    fn class(self) -> FILE_INFO_BY_HANDLE_CLASS {
        match self {
            Self::Extended => FileIdExtdDirectoryInfo,
            Self::Both => FileIdBothDirectoryInfo,
            Self::Full => FileFullDirectoryInfo,
        }
    }

    fn downgrade(self) -> Option<Self> {
        match self {
            Self::Extended => Some(Self::Both),
            Self::Both => Some(Self::Full),
            Self::Full => None,
        }
    }

    /// Décalage du nom dans une entrée.
    fn name_offset(self) -> usize {
        match self {
            Self::Extended => offset_of!(FILE_ID_EXTD_DIR_INFO, FileName),
            Self::Both => offset_of!(FILE_ID_BOTH_DIR_INFO, FileName),
            Self::Full => offset_of!(FILE_FULL_DIR_INFO, FileName),
        }
    }
}

// Le début d'une entrée est le même dans les trois formes.
const OFF_NEXT: usize = offset_of!(FILE_FULL_DIR_INFO, NextEntryOffset);
const OFF_CREATED: usize = offset_of!(FILE_FULL_DIR_INFO, CreationTime);
const OFF_ACCESSED: usize = offset_of!(FILE_FULL_DIR_INFO, LastAccessTime);
const OFF_MODIFIED: usize = offset_of!(FILE_FULL_DIR_INFO, LastWriteTime);
const OFF_SIZE: usize = offset_of!(FILE_FULL_DIR_INFO, EndOfFile);
const OFF_ALLOCATED: usize = offset_of!(FILE_FULL_DIR_INFO, AllocationSize);
const OFF_ATTRIBUTES: usize = offset_of!(FILE_FULL_DIR_INFO, FileAttributes);
const OFF_NAME_LENGTH: usize = offset_of!(FILE_FULL_DIR_INFO, FileNameLength);
const OFF_EA_SIZE: usize = offset_of!(FILE_FULL_DIR_INFO, EaSize);
const OFF_EXT_TAG: usize = offset_of!(FILE_ID_EXTD_DIR_INFO, ReparsePointTag);
const OFF_EXT_ID: usize = offset_of!(FILE_ID_EXTD_DIR_INFO, FileId);
const OFF_BOTH_ID: usize = offset_of!(FILE_ID_BOTH_DIR_INFO, FileId);

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

fn read_i64(bytes: &[u8], offset: usize) -> Option<i64> {
    Some(i64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?))
}

fn read_u128(bytes: &[u8], offset: usize) -> Option<u128> {
    Some(u128::from_le_bytes(bytes.get(offset..offset + 16)?.try_into().ok()?))
}

/// Décode l'entrée qui commence au début de `bytes`. Renvoie le décalage de
/// l'entrée suivante (0 pour la dernière du bloc), le nom en UTF-16 et les
/// métadonnées. `None` si le bloc est incohérent : on arrête alors de le lire.
fn decode_entry(bytes: &[u8], class: ListClass, name: &mut Vec<u16>) -> Option<(usize, EntryMeta)> {
    let name_offset = class.name_offset();
    let next = read_u32(bytes, OFF_NEXT)? as usize;
    let name_length = read_u32(bytes, OFF_NAME_LENGTH)? as usize;
    let name_bytes = bytes.get(name_offset..name_offset.checked_add(name_length)?)?;
    if !name_length.is_multiple_of(2) || (next != 0 && next < name_offset + name_length) {
        return None;
    }
    name.clear();
    name.extend(name_bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])));

    let attributes = read_u32(bytes, OFF_ATTRIBUTES)?;
    // Quand l'entrée est un point d'analyse, les formes sans champ dédié
    // rangent son étiquette dans `EaSize` (comportement documenté de NTFS).
    let reparse_tag = match class {
        ListClass::Extended => read_u32(bytes, OFF_EXT_TAG)?,
        ListClass::Both | ListClass::Full if attributes & ATTR_REPARSE_POINT != 0 => {
            read_u32(bytes, OFF_EA_SIZE)?
        }
        ListClass::Both | ListClass::Full => 0,
    };
    let file_id = match class {
        ListClass::Extended => read_u128(bytes, OFF_EXT_ID).map(valid_id)?,
        ListClass::Both => valid_id(read_i64(bytes, OFF_BOTH_ID)? as u64 as u128),
        ListClass::Full => None,
    };
    let (kind, flags) = classify(attributes, reparse_tag);
    let meta = EntryMeta {
        kind,
        flags,
        size: read_i64(bytes, OFF_SIZE)?.max(0) as u64,
        allocated: read_i64(bytes, OFF_ALLOCATED)?.max(0) as u64,
        created: Timestamp::from_filetime(read_i64(bytes, OFF_CREATED)?),
        modified: Timestamp::from_filetime(read_i64(bytes, OFF_MODIFIED)?),
        accessed: Timestamp::from_filetime(read_i64(bytes, OFF_ACCESSED)?),
        file_id,
    };
    Some((next, meta))
}

/// Un identifiant nul ou saturé veut dire « pas d'identifiant ».
fn valid_id(raw: u128) -> Option<FileId> {
    (raw != 0 && raw != u128::MAX && raw != u64::MAX as u128).then_some(FileId(raw))
}

pub(super) struct DirReader {
    /// Tampon aligné sur 8 octets, comme l'exige le système.
    buffer: Vec<u64>,
    name: Vec<u16>,
}

impl DirReader {
    pub(super) fn new() -> Self {
        Self { buffer: vec![0; BUFFER_BYTES / size_of::<u64>()], name: Vec::with_capacity(260) }
    }

    pub(super) fn read(
        &mut self,
        dir: &Path,
        expected: Option<FileId>,
        visit: &mut dyn FnMut(&OsStr, &EntryMeta),
    ) -> Result<(), ReadDirError> {
        let handle = open_no_follow(
            dir,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_FLAG_BACKUP_SEMANTICS,
        )?;

        // Qu'a-t-on ouvert ? Si un lien a pris la place du dossier depuis
        // qu'on l'a vu, on s'arrête ici : on ne liste jamais à travers un lien.
        let tag: FILE_ATTRIBUTE_TAG_INFO = query(&handle)?;
        let (kind, flags) = classify(tag.FileAttributes, tag.ReparseTag);
        if kind != EntryKind::Dir || flags.contains(EntryFlags::SPECIAL) {
            return Err(ReadDirError::NotADirectory);
        }

        // Est-ce bien le dossier que la liste du parent annonçait ? Un dossier
        // du chemin remplacé par un lien mènerait ailleurs.
        if let Some(expected) = expected {
            match query::<FILE_ID_INFO>(&handle) {
                Ok(info) => {
                    let actual = valid_id(u128::from_le_bytes(info.FileId.Identifier));
                    if actual.is_some_and(|actual| !same_file_id(actual, expected)) {
                        return Err(ReadDirError::Changed);
                    }
                }
                Err(error) if is_unsupported(&error) => {}
                Err(error) => return Err(error.into()),
            }
        }

        let mut class = ListClass::Extended;
        let mut first_call = true;
        loop {
            // SAFETY: le handle appartient à `handle`, vivant pendant l'appel.
            // Le tampon appartient à `self`, il est aligné sur 8 octets et on
            // annonce exactement sa taille en octets.
            let result = unsafe {
                GetFileInformationByHandleEx(
                    HANDLE(handle.as_raw_handle()),
                    class.class(),
                    self.buffer.as_mut_ptr().cast::<c_void>(),
                    BUFFER_BYTES as u32,
                )
            }
            .map_err(to_io);

            if let Err(error) = result {
                return match error.raw_os_error() {
                    Some(ERROR_NO_MORE_FILES) => Ok(()),
                    // La racine d'un volume vide n'a même pas d'entrée « . ».
                    Some(ERROR_FILE_NOT_FOUND) if first_call => Ok(()),
                    _ if first_call && is_unsupported(&error) => match class.downgrade() {
                        Some(simpler) => {
                            class = simpler;
                            continue;
                        }
                        None => Err(error.into()),
                    },
                    _ => Err(error.into()),
                };
            }
            first_call = false;

            // SAFETY: le tampon contient `BUFFER_BYTES` octets initialisés
            // (des `u64`), et tout octet est une valeur valide de `u8`.
            let bytes: &[u8] =
                unsafe { std::slice::from_raw_parts(self.buffer.as_ptr().cast(), BUFFER_BYTES) };
            let mut offset = 0;
            while let Some((next, meta)) =
                bytes.get(offset..).and_then(|rest| decode_entry(rest, class, &mut self.name))
            {
                if self.name != [b'.' as u16] && self.name != [b'.' as u16, b'.' as u16] {
                    visit(&OsString::from_wide(&self.name), &meta);
                }
                if next == 0 {
                    break;
                }
                offset += next;
            }
        }
    }
}

fn open_error(error: io::Error) -> OpenError {
    match error.raw_os_error() {
        Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION) => OpenError::Locked,
        _ => match error.kind() {
            io::ErrorKind::PermissionDenied => OpenError::AccessDenied,
            io::ErrorKind::NotFound => OpenError::NotFound,
            _ => OpenError::Io(error),
        },
    }
}

pub(super) fn open_verified(path: &Path, expected: &Fingerprint) -> Result<File, OpenError> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(SHARE_ALL)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN)
        .open(path)
        .map_err(open_error)?;
    check_open_file(&file, expected)?;
    Ok(file)
}

pub(super) fn check_open_file(file: &File, expected: &Fingerprint) -> Result<(), OpenError> {
    let tag: FILE_ATTRIBUTE_TAG_INFO = query(file).map_err(open_error)?;
    let (kind, flags) = classify(tag.FileAttributes, tag.ReparseTag);
    if flags.contains(EntryFlags::ONLINE_ONLY) {
        return Err(OpenError::OnlineOnly);
    }
    if kind != EntryKind::File || flags.contains(EntryFlags::SPECIAL) {
        return Err(OpenError::NotRegular);
    }

    let standard = query_standard(file).map_err(open_error)?;
    if standard.directory {
        return Err(OpenError::NotRegular);
    }
    if standard.is_gone() {
        return Err(OpenError::NotFound);
    }
    if standard.size != expected.size {
        return Err(OpenError::Changed);
    }

    let basic: FILE_BASIC_INFO = query(file).map_err(open_error)?;
    if Timestamp::from_filetime(basic.LastWriteTime) != expected.modified {
        return Err(OpenError::Changed);
    }

    if let Some(expected_id) = expected.file_id {
        match query::<FILE_ID_INFO>(file) {
            Ok(info) => {
                let actual = valid_id(u128::from_le_bytes(info.FileId.Identifier));
                if actual.is_some_and(|actual| !same_file_id(actual, expected_id)) {
                    return Err(OpenError::Changed);
                }
            }
            Err(error) if is_unsupported(&error) => {}
            Err(error) => return Err(open_error(error)),
        }
    }
    Ok(())
}

fn remove_error(error: io::Error) -> RemoveError {
    match error.raw_os_error() {
        Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION) => RemoveError::Locked,
        Some(ERROR_DIR_NOT_EMPTY) => RemoveError::NotEmpty,
        _ => match error.kind() {
            io::ErrorKind::PermissionDenied => RemoveError::AccessDenied,
            io::ErrorKind::NotFound => RemoveError::NotFound,
            io::ErrorKind::DirectoryNotEmpty => RemoveError::NotEmpty,
            _ => RemoveError::Io(error),
        },
    }
}

/// Demande au système de supprimer l'entrée ouverte quand son handle se
/// fermera (`delete` vrai), ou retire cette demande (`delete` faux).
fn set_disposition(file: &File, delete: bool) -> io::Result<()> {
    // La forme moderne supprime le nom dès la fermeture de ce handle, même si
    // une autre app garde l'entrée ouverte, et accepte un fichier en lecture
    // seule sans toucher à ses attributs. Elle ne force rien pour autant : un
    // programme en cours d'exécution ou un fichier projeté en mémoire restent
    // refusés par le système.
    let flags = if delete {
        FILE_DISPOSITION_INFO_EX_FLAGS(
            FILE_DISPOSITION_FLAG_DELETE.0
                | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS.0
                | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE.0,
        )
    } else {
        FILE_DISPOSITION_FLAG_DO_NOT_DELETE
    };
    let modern = FILE_DISPOSITION_INFO_EX { Flags: flags };
    // SAFETY: le handle appartient à `file`, vivant pendant l'appel. `modern`
    // est une structure locale du type attendu pour cette classe, dont on
    // annonce exactement la taille.
    let outcome = unsafe {
        SetFileInformationByHandle(
            HANDLE(file.as_raw_handle()),
            FileDispositionInfoEx,
            (&raw const modern).cast::<c_void>(),
            size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    }
    .map_err(to_io);
    match outcome {
        // Système de fichiers qui ne connaît que la forme d'origine (FAT, exFAT).
        Err(error) if is_unsupported(&error) => {
            let classic = FILE_DISPOSITION_INFO { DeleteFile: delete };
            // SAFETY: comme ci-dessus, avec la structure de la classe d'origine.
            unsafe {
                SetFileInformationByHandle(
                    HANDLE(file.as_raw_handle()),
                    FileDispositionInfo,
                    (&raw const classic).cast::<c_void>(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            }
            .map_err(to_io)
        }
        other => other,
    }
}

/// Vérifie, sur l'entrée ouverte, que c'est celle que le parcours a vue.
fn check_removal(file: &File, expected: &Expected) -> Result<(), RemoveError> {
    let tag: FILE_ATTRIBUTE_TAG_INFO = query(file).map_err(remove_error)?;
    let (kind, flags) = classify(tag.FileAttributes, tag.ReparseTag);
    // Un fichier devenu lien, un lien devenu dossier : ce n'est plus la même entrée.
    if kind != expected.kind || kind == EntryKind::Other {
        return Err(RemoveError::Changed);
    }
    if kind == EntryKind::Dir && flags.contains(EntryFlags::SPECIAL) {
        return Err(RemoveError::Changed);
    }

    let standard = query_standard(file).map_err(remove_error)?;
    if standard.is_gone() {
        return Err(RemoveError::NotFound);
    }
    if kind != EntryKind::Dir {
        if standard.size != expected.size {
            return Err(RemoveError::Changed);
        }
        let basic: FILE_BASIC_INFO = query(file).map_err(remove_error)?;
        if Timestamp::from_filetime(basic.LastWriteTime) != expected.modified {
            return Err(RemoveError::Changed);
        }
    }

    if let Some(expected_id) = expected.file_id {
        match query::<FILE_ID_INFO>(file) {
            Ok(info) => {
                let actual = valid_id(u128::from_le_bytes(info.FileId.Identifier));
                if actual.is_some_and(|actual| !same_file_id(actual, expected_id)) {
                    return Err(RemoveError::Changed);
                }
            }
            Err(error) if is_unsupported(&error) => {}
            Err(error) => return Err(remove_error(error)),
        }
    }
    Ok(())
}

/// Une entrée ouverte avec le droit de la supprimer.
pub(super) struct Removal {
    file: File,
    /// La suppression est demandée et pas encore confirmée.
    marked: bool,
}

impl Removal {
    pub(super) fn open(path: &Path, expected: &Expected) -> Result<Self, RemoveError> {
        // Un fichier est ouvert sans aucun partage : si un autre programme le
        // tient, pour le lire, l'écrire ou le supprimer, l'ouverture échoue
        // ici et le fichier est laissé. Beaucoup d'apps ouvrent leurs fichiers
        // en autorisant leur suppression : sans cela, ils partiraient sous
        // elles. Un dossier ou un lien garde un partage entier : l'Explorateur
        // et les antivirus les tiennent sans s'en servir, et un dossier ne
        // part de toute façon que vide.
        let share = if expected.kind == EntryKind::File { 0 } else { SHARE_ALL };
        let file = open_no_follow_sharing(
            path,
            DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            share,
            FILE_FLAG_BACKUP_SEMANTICS,
        )
        .map_err(remove_error)?;
        check_removal(&file, expected)?;
        Ok(Self { file, marked: false })
    }

    pub(super) fn facts(&self) -> Result<RemovalFacts, RemoveError> {
        let tag: FILE_ATTRIBUTE_TAG_INFO = query(&self.file).map_err(remove_error)?;
        let (kind, flags) = classify(tag.FileAttributes, tag.ReparseTag);
        let standard = query_standard(&self.file).map_err(remove_error)?;
        Ok(RemovalFacts {
            links: standard.links,
            online_only: flags.contains(EntryFlags::ONLINE_ONLY),
            regular: kind == EntryKind::File && !flags.contains(EntryFlags::SPECIAL),
        })
    }

    pub(super) fn modified(&self) -> Result<Timestamp, RemoveError> {
        let basic: FILE_BASIC_INFO = query(&self.file).map_err(remove_error)?;
        Ok(Timestamp::from_filetime(basic.LastWriteTime))
    }

    pub(super) fn mark(&mut self) -> Result<(), RemoveError> {
        set_disposition(&self.file, true).map_err(remove_error)?;
        self.marked = true;
        Ok(())
    }

    pub(super) fn commit(mut self) -> Result<(), RemoveError> {
        if !self.marked {
            self.mark()?;
        }
        // La demande reste posée : l'entrée disparaît à la fermeture du
        // handle, c'est-à-dire à la fin de cette fonction.
        self.marked = false;
        Ok(())
    }
}

impl Drop for Removal {
    fn drop(&mut self) {
        // Lâchée sans confirmation : la demande est retirée, l'entrée reste.
        if self.marked {
            let _ = set_disposition(&self.file, false);
        }
    }
}

/// Vrai si `path` désigne un vrai dossier (ni lien, ni fichier, ni point
/// d'analyse inconnu), sans suivre un lien final.
pub(super) fn is_real_dir(path: &Path) -> io::Result<bool> {
    let handle = open_no_follow(path, FILE_READ_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS)?;
    let tag: FILE_ATTRIBUTE_TAG_INFO = query(&handle)?;
    let (kind, flags) = classify(tag.FileAttributes, tag.ReparseTag);
    Ok(kind == EntryKind::Dir && !flags.contains(EntryFlags::SPECIAL))
}

pub(super) fn identity(path: &Path) -> io::Result<Option<Identity>> {
    let handle = open_no_follow(path, FILE_READ_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS)?;
    match query::<FILE_ID_INFO>(&handle) {
        Ok(info) => Ok(valid_id(u128::from_le_bytes(info.FileId.Identifier))
            .map(|file_id| Identity { volume: info.VolumeSerialNumber, file_id })),
        Err(error) if is_unsupported(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn invalid_path(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub(super) fn to_internal_path(path: &Path) -> io::Result<PathBuf> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(invalid_path("le chemin doit commencer par un lecteur ou un partage"));
    };
    let mut internal = match prefix.kind() {
        // Déjà sous forme littérale : on le prend tel quel.
        Prefix::Verbatim(_) | Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(..) => {
            PathBuf::from(prefix.as_os_str())
        }
        Prefix::Disk(letter) => {
            PathBuf::from(format!(r"\\?\{}:", (letter as char).to_ascii_uppercase()))
        }
        Prefix::UNC(server, share) => {
            let mut unc = OsString::from(r"\\?\UNC\");
            unc.push(server);
            unc.push(r"\");
            unc.push(share);
            PathBuf::from(unc)
        }
        Prefix::DeviceNS(_) => return Err(invalid_path("chemin de périphérique refusé")),
    };
    if components.next() != Some(Component::RootDir) {
        return Err(invalid_path("le chemin doit être absolu"));
    }
    // `push` d'un séparateur seul ancre le chemin à la racine du préfixe.
    internal.push(r"\");
    for component in components {
        match component {
            Component::Normal(name) => internal.push(name),
            Component::CurDir => {}
            _ => return Err(invalid_path("le chemin ne doit pas contenir « .. »")),
        }
    }
    Ok(internal)
}

pub(super) fn to_display_path(path: &Path) -> PathBuf {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path.to_path_buf();
    };
    let mut display = match prefix.kind() {
        Prefix::VerbatimDisk(letter) => PathBuf::from(format!("{}:", letter as char)),
        Prefix::VerbatimUNC(server, share) => {
            let mut unc = OsString::from(r"\\");
            unc.push(server);
            unc.push(r"\");
            unc.push(share);
            PathBuf::from(unc)
        }
        _ => return path.to_path_buf(),
    };
    for component in components {
        match component {
            Component::RootDir => display.push(r"\"),
            other => display.push(other.as_os_str()),
        }
    }
    display
}

pub(super) fn fold_prefix(prefix: &PrefixComponent<'_>) -> Option<String> {
    match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            Some(format!("{}:", (letter as char).to_ascii_lowercase()))
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => Some(format!(
            r"\\{}\{}",
            server.to_string_lossy().to_lowercase(),
            share.to_string_lossy().to_lowercase()
        )),
        Prefix::Verbatim(name) => Some(format!(r"\\?\{}", name.to_string_lossy().to_lowercase())),
        Prefix::DeviceNS(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_layouts_match_the_documented_offsets() {
        assert_eq!(ListClass::Full.name_offset(), 68);
        assert_eq!(ListClass::Both.name_offset(), 104);
        assert_eq!(ListClass::Extended.name_offset(), 88);
        assert_eq!((OFF_NEXT, OFF_CREATED, OFF_ACCESSED, OFF_MODIFIED), (0, 8, 16, 24));
        assert_eq!((OFF_SIZE, OFF_ALLOCATED, OFF_ATTRIBUTES), (40, 48, 56));
        assert_eq!((OFF_NAME_LENGTH, OFF_EA_SIZE), (60, 64));
        assert_eq!((OFF_EXT_TAG, OFF_EXT_ID, OFF_BOTH_ID), (68, 72, 96));
        // Le préfixe commun est bien commun.
        assert_eq!(offset_of!(FILE_ID_BOTH_DIR_INFO, EaSize), OFF_EA_SIZE);
        assert_eq!(offset_of!(FILE_ID_EXTD_DIR_INFO, EaSize), OFF_EA_SIZE);
        assert_eq!(offset_of!(FILE_ID_EXTD_DIR_INFO, FileNameLength), OFF_NAME_LENGTH);
    }

    /// Fabrique une entrée « étendue » comme le système la rendrait.
    fn extended_entry(name: &str, attributes: u32, tag: u32, next: u32) -> Vec<u8> {
        let wide: Vec<u16> = name.encode_utf16().collect();
        let mut bytes = vec![0u8; 88 + wide.len() * 2];
        bytes[OFF_NEXT..OFF_NEXT + 4].copy_from_slice(&next.to_le_bytes());
        bytes[OFF_MODIFIED..OFF_MODIFIED + 8]
            .copy_from_slice(&116_444_736_000_000_010_i64.to_le_bytes());
        bytes[OFF_SIZE..OFF_SIZE + 8].copy_from_slice(&1234_i64.to_le_bytes());
        bytes[OFF_ALLOCATED..OFF_ALLOCATED + 8].copy_from_slice(&4096_i64.to_le_bytes());
        bytes[OFF_ATTRIBUTES..OFF_ATTRIBUTES + 4].copy_from_slice(&attributes.to_le_bytes());
        bytes[OFF_NAME_LENGTH..OFF_NAME_LENGTH + 4]
            .copy_from_slice(&((wide.len() * 2) as u32).to_le_bytes());
        bytes[OFF_EXT_TAG..OFF_EXT_TAG + 4].copy_from_slice(&tag.to_le_bytes());
        bytes[OFF_EXT_ID..OFF_EXT_ID + 16].copy_from_slice(&77_u128.to_le_bytes());
        for (index, unit) in wide.iter().enumerate() {
            bytes[88 + index * 2..90 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn decodes_an_extended_entry() {
        let bytes = extended_entry("été.txt", 0x20, 0, 0);
        let mut name = Vec::new();
        let (next, meta) = decode_entry(&bytes, ListClass::Extended, &mut name).unwrap();
        assert_eq!(next, 0);
        assert_eq!(String::from_utf16(&name).unwrap(), "été.txt");
        assert_eq!(meta.kind, EntryKind::File);
        assert_eq!((meta.size, meta.allocated), (1234, 4096));
        assert_eq!(meta.modified.ticks(), 10);
        assert_eq!(meta.file_id, Some(FileId(77)));
    }

    #[test]
    fn a_junction_is_decoded_as_a_link() {
        let bytes = extended_entry("lien", 0x410, 0xA000_0003, 0);
        let (_, meta) = decode_entry(&bytes, ListClass::Extended, &mut Vec::new()).unwrap();
        assert_eq!(meta.kind, EntryKind::Link);
    }

    #[test]
    fn truncated_or_inconsistent_entries_are_rejected() {
        let bytes = extended_entry("fichier.bin", 0x20, 0, 0);
        let mut name = Vec::new();
        // Bloc coupé au milieu du nom.
        assert!(decode_entry(&bytes[..bytes.len() - 3], ListClass::Extended, &mut name).is_none());
        // Bloc coupé dans l'en-tête.
        assert!(decode_entry(&bytes[..40], ListClass::Extended, &mut name).is_none());
        // Entrée suivante annoncée au milieu de celle-ci.
        let overlapping = extended_entry("fichier.bin", 0x20, 0, 16);
        assert!(decode_entry(&overlapping, ListClass::Extended, &mut name).is_none());
        // Longueur de nom impaire.
        let mut odd = extended_entry("ab", 0x20, 0, 0);
        odd[OFF_NAME_LENGTH..OFF_NAME_LENGTH + 4].copy_from_slice(&3u32.to_le_bytes());
        assert!(decode_entry(&odd, ListClass::Extended, &mut name).is_none());
    }

    #[test]
    fn null_ids_mean_no_id() {
        assert_eq!(valid_id(0), None);
        assert_eq!(valid_id(u128::MAX), None);
        assert_eq!(valid_id(u64::MAX as u128), None);
        assert_eq!(valid_id(5), Some(FileId(5)));
    }

    #[test]
    fn short_and_long_ids_compare_on_their_low_half() {
        assert!(same_file_id(FileId(42), FileId(42)));
        assert!(same_file_id(FileId(42), FileId((7 << 64) | 42)));
        assert!(!same_file_id(FileId(42), FileId(43)));
        assert!(!same_file_id(FileId((1 << 64) | 42), FileId((2 << 64) | 42)));
    }

    #[test]
    fn internal_paths_are_verbatim() {
        let internal = to_internal_path(Path::new(r"c:\Users\Léa\Images")).unwrap();
        assert_eq!(internal, Path::new(r"\\?\C:\Users\Léa\Images"));
        assert_eq!(to_internal_path(Path::new(r"C:\")).unwrap(), Path::new(r"\\?\C:\"));
        assert_eq!(
            to_internal_path(Path::new(r"\\serveur\partage\dossier")).unwrap(),
            Path::new(r"\\?\UNC\serveur\partage\dossier")
        );
        // Déjà littéral : inchangé, y compris un nom qui finit par un point.
        let literal = Path::new(r"\\?\C:\photos.");
        assert_eq!(to_internal_path(literal).unwrap(), literal);
    }

    #[test]
    fn relative_and_dotted_paths_are_refused() {
        assert!(to_internal_path(Path::new(r"dossier\fichier")).is_err());
        assert!(to_internal_path(Path::new(r"C:dossier")).is_err());
        assert!(to_internal_path(Path::new(r"C:\a\..\b")).is_err());
        assert!(to_internal_path(Path::new(r"\\.\PhysicalDrive0")).is_err());
    }

    #[test]
    fn display_paths_drop_the_verbatim_prefix() {
        assert_eq!(to_display_path(Path::new(r"\\?\C:\Users\Léa")), Path::new(r"C:\Users\Léa"));
        assert_eq!(to_display_path(Path::new(r"\\?\C:\")), Path::new(r"C:\"));
        assert_eq!(
            to_display_path(Path::new(r"\\?\UNC\serveur\partage\dossier")),
            Path::new(r"\\serveur\partage\dossier")
        );
        assert_eq!(to_display_path(Path::new(r"C:\déjà\court")), Path::new(r"C:\déjà\court"));
    }

    #[test]
    fn round_trip_keeps_the_path() {
        for text in [r"C:\Users\Léa\Images", r"D:\", r"\\serveur\partage\a b\c"] {
            let internal = to_internal_path(Path::new(text)).unwrap();
            assert_eq!(to_display_path(&internal), Path::new(text));
        }
    }
}
