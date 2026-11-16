//! Lire ce que Windows dit des droits : le propriétaire et la liste de droits
//! d'un objet, son chemin final, et le jeton du processus. **Lecture seule.**
//!
//! La décision (« cet objet est-il sûr ? ») n'est pas ici : elle est pure, dans
//! [`crate::helper::location`]. Ce module ne fait que lui apporter ce que
//! Windows répond.
//!
//! Un objet est ouvert **une seule fois**, avec une poignée qui sert à tout :
//! son chemin final (`GetFinalPathNameByHandleW`) et son descripteur de sécurité
//! (`GetSecurityInfo`), jamais à lire son contenu. Celui qui garde la poignée
//! ouverte, en partage de lecture seule, empêche qu'on remplace ou supprime le
//! fichier pendant qu'il le vérifie puis le lance.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;

use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
    CheckTokenMembership, CreateWellKnownSid, DACL_SECURITY_INFORMATION, GetAce, GetAclInformation,
    GetSidIdentifierAuthority, GetSidSubAuthority, GetSidSubAuthorityCount, IsValidSid,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TokenElevationType,
    TokenElevationTypeFull, TokenElevationTypeLimited, WinBuiltinAdministratorsSid,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED,
    FILE_SHARE_MODE, GetFileInformationByHandle, GetFinalPathNameByHandleW, OPEN_EXISTING,
};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::core::{BOOL, PCWSTR};

use crate::helper::location::{Ace, AceKind, Placed, Security};
use crate::startup::access::ElevationType;

/// `READ_CONTROL` : lire le descripteur de sécurité.
const READ_CONTROL: u32 = 0x0002_0000;
/// `FILE_READ_ATTRIBUTES`.
const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
/// `ACCESS_ALLOWED_ACE_TYPE` et `ACCESS_DENIED_ACE_TYPE`.
const ACE_ALLOWED: u8 = 0;
const ACE_DENIED: u8 = 1;
/// `INHERIT_ONLY_ACE`.
const INHERIT_ONLY: u8 = 0x08;
/// Plafond de lignes lues dans une liste de droits : au-delà, l'objet n'est
/// pas sûr (une liste démesurée n'est pas celle de Windows).
const MAX_ACES: u32 = 4096;

/// Le texte d'un SID (`S-1-5-32-544`), `None` s'il n'est pas valide.
fn sid_text(sid: PSID) -> Option<String> {
    if sid.0.is_null() {
        return None;
    }
    // SAFETY: `sid` désigne un SID rendu par Windows et vivant (descripteur de
    // sécurité non encore libéré) ; `IsValidSid` ne fait que le vérifier.
    if !unsafe { IsValidSid(sid) }.as_bool() {
        return None;
    }
    // SAFETY: le SID est valide : l'autorité et le nombre de sous-autorités
    // sont lisibles, et chaque rang est lu dans les bornes annoncées.
    let (authority, count) = unsafe {
        let authority = (*GetSidIdentifierAuthority(sid)).Value;
        (authority, *GetSidSubAuthorityCount(sid))
    };
    let number = authority.iter().fold(0u64, |total, byte| (total << 8) | u64::from(*byte));
    let mut text = format!("S-1-{number}");
    for rank in 0..u32::from(count) {
        // SAFETY: `rank` est plus petit que le nombre de sous-autorités du SID valide.
        let sub = unsafe { *GetSidSubAuthority(sid, rank) };
        text.push_str(&format!("-{sub}"));
    }
    Some(text)
}

/// La liste de droits : `None` pour une liste nulle ou démesurée.
fn read_acl(acl: *mut ACL) -> Option<Vec<Ace>> {
    if acl.is_null() {
        return None;
    }
    let mut info = ACL_SIZE_INFORMATION::default();
    // SAFETY: `acl` est la liste d'un descripteur vivant ; `info` est une
    // structure locale dont on donne la taille exacte.
    unsafe {
        GetAclInformation(
            acl,
            (&raw mut info).cast::<c_void>(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    }
    .ok()?;
    if info.AceCount > MAX_ACES {
        return None;
    }
    let mut aces = Vec::new();
    for index in 0..info.AceCount {
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: `index` est plus petit que le nombre d'entrées annoncé ; Windows
        // écrit dans `raw` l'adresse d'une entrée de la liste.
        unsafe { GetAce(acl, index, &raw mut raw) }.ok()?;
        if raw.is_null() {
            return None;
        }
        // SAFETY: toute entrée commence par un `ACE_HEADER`.
        let header = unsafe { *raw.cast::<ACE_HEADER>() };
        let kind = match header.AceType {
            ACE_ALLOWED => AceKind::Allow,
            ACE_DENIED => AceKind::Deny,
            _ => AceKind::Other,
        };
        if kind == AceKind::Other {
            aces.push(Ace {
                kind,
                sid: String::new(),
                mask: 0,
                inherit_only: header.AceFlags & INHERIT_ONLY != 0,
            });
            continue;
        }
        let ace = raw.cast::<ACCESS_ALLOWED_ACE>();
        // SAFETY: une entrée d'autorisation ou de refus simple a la forme de
        // `ACCESS_ALLOWED_ACE` : un masque, puis le SID à partir de `SidStart`.
        let (mask, sid) = unsafe {
            ((*ace).Mask, PSID(std::ptr::addr_of!((*ace).SidStart).cast_mut().cast::<c_void>()))
        };
        let Some(sid) = sid_text(sid) else {
            // Un SID qu'on ne sait pas lire : entrée inconnue, donc pas sûre.
            aces.push(Ace { kind: AceKind::Other, sid: String::new(), mask, inherit_only: false });
            continue;
        };
        aces.push(Ace { kind, sid, mask, inherit_only: header.AceFlags & INHERIT_ONLY != 0 });
    }
    Some(aces)
}

/// Le descripteur de sécurité (propriétaire, liste de droits) d'un objet ouvert.
/// Illisible : un descripteur sans propriétaire ni liste, que la décision tient
/// pour pas sûr.
pub(super) fn security_of(handle: HANDLE) -> Security {
    let mut owner = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `handle` est ouvert avec `READ_CONTROL` ; les trois sorties sont
    // des variables locales. Le descripteur rendu est libéré plus bas.
    let result = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&raw mut owner),
            None,
            Some(&raw mut dacl),
            None,
            Some(&raw mut descriptor),
        )
    };
    if result != ERROR_SUCCESS {
        return Security { owner: None, acl: None };
    }
    let security = Security { owner: sid_text(owner), acl: read_acl(dacl) };
    // SAFETY: le descripteur a été alloué par `GetSecurityInfo`, qui demande de
    // le libérer avec `LocalFree` ; `owner` et `dacl` n'ont plus servi depuis.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    security
}

/// Un objet ouvert une fois : sa poignée (à garder pour empêcher qu'on le
/// remplace) et ce qu'on a lu de lui.
pub(super) struct Opened {
    /// La poignée. Tant qu'elle est ouverte, le partage choisi à l'ouverture
    /// s'applique à tous les autres.
    #[allow(dead_code)] // Elle n'est pas lue : sa seule raison d'être est de rester ouverte.
    pub handle: OwnedHandle,
    /// Chemin final, droits, point d'analyse.
    pub placed: Placed,
}

/// Ouvre un fichier ou un dossier **sans suivre un lien**, pour en lire le
/// chemin final et les droits. `share` : ce que les autres peuvent faire pendant
/// que la poignée est ouverte (pour un fichier qu'on va lancer : lecture seule).
pub(super) fn open_object(path: &Path, share: FILE_SHARE_MODE) -> Option<Opened> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return None;
    }
    wide.push(0);
    // SAFETY: `wide` est une chaîne terminée par zéro, vivante pendant l'appel.
    // Aucun accès en lecture du contenu : seulement `READ_CONTROL` et les
    // attributs. `FILE_FLAG_OPEN_REPARSE_POINT` : un lien est ouvert lui-même.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            share,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .ok()?;
    // SAFETY: `CreateFileW` a réussi : la poignée est ouverte et n'appartient
    // qu'à cette fonction ; `OwnedHandle` la fermera, une seule fois.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let raw = HANDLE(handle.as_raw_handle());

    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `raw` est une poignée ouverte ; `info` est une structure locale.
    unsafe { GetFileInformationByHandle(raw, &raw mut info) }.ok()?;
    let reparse = info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;

    let mut buffer = vec![0u16; 1024];
    let length = loop {
        // SAFETY: `raw` est ouverte ; `buffer` est un tampon local dont la
        // taille (en caractères) est celle qu'on donne.
        let length =
            unsafe { GetFinalPathNameByHandleW(raw, &mut buffer, FILE_NAME_NORMALIZED) } as usize;
        if length == 0 {
            return None;
        }
        if length < buffer.len() {
            break length;
        }
        // Trop court : `length` est la taille nécessaire. Borné.
        if length > 32_768 {
            return None;
        }
        buffer.resize(length + 1, 0);
    };
    let path = String::from_utf16(&buffer[..length]).ok()?;
    Some(Opened { handle, placed: Placed { path, security: security_of(raw), reparse } })
}

/// Le groupe Administrateurs est-il **activé** dans le jeton de ce processus ?
pub(super) fn administrators_enabled() -> bool {
    let mut sid = [0u8; 68];
    let mut size = sid.len() as u32;
    let psid = PSID(sid.as_mut_ptr().cast::<c_void>());
    // SAFETY: `sid` est un tampon local de 68 octets (la taille maximale d'un
    // SID), `size` annonce sa taille.
    if unsafe { CreateWellKnownSid(WinBuiltinAdministratorsSid, None, Some(psid), &raw mut size) }
        .is_err()
    {
        return false;
    }
    let mut member = BOOL(0);
    // SAFETY: `psid` est un SID valide construit ci-dessus ; sans jeton donné,
    // Windows regarde celui du fil appelant ; `member` est une variable locale.
    unsafe { CheckTokenMembership(None, psid, &raw mut member) }.is_ok() && member.as_bool()
}

/// Le type d'élévation du jeton de ce processus.
pub(super) fn elevation_type() -> ElevationType {
    // SAFETY: `GetCurrentProcess` rend un pseudo-handle toujours valide.
    let Some(token) = super::token_of(unsafe { GetCurrentProcess() }) else {
        return ElevationType::Default;
    };
    match super::token_number(&token, TokenElevationType) {
        Some(kind) if kind == TokenElevationTypeFull.0 as u32 => ElevationType::Full,
        Some(kind) if kind == TokenElevationTypeLimited.0 as u32 => ElevationType::Limited,
        _ => ElevationType::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    const ALL: FILE_SHARE_MODE =
        FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0);

    #[test]
    fn a_folder_is_opened_with_its_final_path_and_a_readable_descriptor() {
        let temp = tempfile::tempdir().unwrap();
        let opened = open_object(temp.path(), ALL).expect("ouvert");
        assert!(opened.placed.path.starts_with(r"\\?\"), "{}", opened.placed.path);
        assert!(!opened.placed.reparse);
        // Le descripteur se lit : un propriétaire texte, une liste (ou pas, sous Wine).
        let owner = opened.placed.security.owner.expect("un propriétaire");
        assert!(owner.starts_with("S-1-"), "{owner}");
        assert!(owner[4..].chars().all(|c| c.is_ascii_digit() || c == '-'));
    }

    #[test]
    fn a_missing_path_is_not_opened() {
        let temp = tempfile::tempdir().unwrap();
        assert!(open_object(&temp.path().join("absent"), ALL).is_none());
    }

    #[test]
    fn a_handle_kept_open_without_write_sharing_stops_a_rewrite() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("shinku-helper.exe");
        std::fs::write(&file, b"x").unwrap();
        let kept = open_object(&file, FILE_SHARE_READ).expect("ouvert");
        if std::path::Path::new(r"Z:\").exists() {
            // Wine (Z: = racine Linux) ne fait pas respecter le partage : preuve
            // seulement sur un vrai Windows (voir conception.md, D10).
            return;
        }
        assert!(std::fs::OpenOptions::new().write(true).open(&file).is_err(), "écriture refusée");
        assert!(std::fs::remove_file(&file).is_err(), "suppression refusée");
        assert!(std::fs::read(&file).is_ok(), "la lecture reste permise");
        drop(kept);
        assert!(std::fs::remove_file(&file).is_ok());
    }

    #[test]
    fn the_administrators_sid_is_written_in_text() {
        let mut sid = [0u8; 68];
        let mut size = sid.len() as u32;
        let psid = PSID(sid.as_mut_ptr().cast::<c_void>());
        // SAFETY: tampon local de la taille maximale d'un SID.
        unsafe { CreateWellKnownSid(WinBuiltinAdministratorsSid, None, Some(psid), &raw mut size) }
            .unwrap();
        assert_eq!(sid_text(psid).as_deref(), Some(crate::helper::location::ADMINISTRATORS));
    }

    #[test]
    fn the_token_questions_answer_without_failing() {
        let _ = administrators_enabled();
        let _ = elevation_type();
    }
}
