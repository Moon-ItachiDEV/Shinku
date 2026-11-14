//! Attributs de fichier et étiquettes de point d'analyse de Windows.
//!
//! Le classement fait ici décide de ce que Shinku parcourt, lit ou laisse.
//! C'est une fonction pure : elle ne sert qu'à Windows, mais elle est testée
//! sur toutes les plateformes.
#![cfg_attr(not(windows), allow(dead_code))]

use super::{EntryFlags, EntryKind};

// Attributs de fichier de Windows (valeurs documentées, stables).
pub(crate) const ATTR_HIDDEN: u32 = 0x0000_0002;
pub(crate) const ATTR_SYSTEM: u32 = 0x0000_0004;
pub(crate) const ATTR_DIRECTORY: u32 = 0x0000_0010;
pub(crate) const ATTR_REPARSE_POINT: u32 = 0x0000_0400;
pub(crate) const ATTR_OFFLINE: u32 = 0x0000_1000;
pub(crate) const ATTR_RECALL_ON_OPEN: u32 = 0x0004_0000;
pub(crate) const ATTR_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

// Étiquettes de point d'analyse (« reparse tags »).
/// Bit « substitut de nom » : le point d'analyse renvoie vers un autre endroit
/// (jonction, lien symbolique). C'est le test de `IsReparseTagNameSurrogate`.
pub(crate) const TAG_NAME_SURROGATE_BIT: u32 = 0x2000_0000;
pub(crate) const TAG_DEDUP: u32 = 0x8000_0013;
pub(crate) const TAG_WOF: u32 = 0x8000_0017;
/// Fichiers et dossiers d'un fournisseur de synchronisation (OneDrive…) :
/// `IO_REPARSE_TAG_CLOUD` et ses seize variantes ne diffèrent que par ce masque.
pub(crate) const TAG_CLOUD: u32 = 0x9000_001A;
pub(crate) const TAG_CLOUD_VARIANT_MASK: u32 = 0x0000_F000;

/// Classe une entrée à partir de ses attributs Windows et de son étiquette de
/// point d'analyse. Fonction pure : elle est testée sur toutes les plateformes.
pub(crate) fn classify(attributes: u32, reparse_tag: u32) -> (EntryKind, EntryFlags) {
    let is_dir = attributes & ATTR_DIRECTORY != 0;
    let mut kind = if is_dir { EntryKind::Dir } else { EntryKind::File };
    let mut flags = EntryFlags::empty();

    if attributes & ATTR_HIDDEN != 0 {
        flags = flags.with(EntryFlags::HIDDEN);
    }
    if attributes & ATTR_SYSTEM != 0 {
        flags = flags.with(EntryFlags::SYSTEM);
    }
    if attributes & (ATTR_OFFLINE | ATTR_RECALL_ON_OPEN | ATTR_RECALL_ON_DATA_ACCESS) != 0 {
        flags = flags.with(EntryFlags::ONLINE_ONLY);
    }

    if attributes & ATTR_REPARSE_POINT != 0 {
        if reparse_tag & TAG_NAME_SURROGATE_BIT != 0 {
            // Jonction, lien symbolique, point de montage : un renvoi vers ailleurs.
            kind = EntryKind::Link;
        } else if reparse_tag & !TAG_CLOUD_VARIANT_MASK == TAG_CLOUD {
            // Vrai fichier ou vrai dossier, géré par un fournisseur de synchronisation.
            flags = flags.with(EntryFlags::CLOUD);
        } else if !is_dir && (reparse_tag == TAG_WOF || reparse_tag == TAG_DEDUP) {
            // Compression ou déduplication transparente : un fichier ordinaire à la lecture.
        } else {
            // Étiquette inconnue (ou absente) : on ne parcourt pas, on ne lit pas.
            flags = flags.with(EntryFlags::SPECIAL);
        }
    }
    (kind, flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::EntryMeta;
    use crate::model::Timestamp;

    const DIR: u32 = ATTR_DIRECTORY;
    const REPARSE: u32 = ATTR_REPARSE_POINT;

    #[test]
    fn plain_entries() {
        assert_eq!(classify(0x20, 0), (EntryKind::File, EntryFlags::empty()));
        assert_eq!(classify(DIR, 0), (EntryKind::Dir, EntryFlags::empty()));
    }

    #[test]
    fn junctions_and_symlinks_are_links() {
        // Jonction (IO_REPARSE_TAG_MOUNT_POINT) et lien symbolique vers un dossier.
        assert_eq!(classify(DIR | REPARSE, 0xA000_0003).0, EntryKind::Link);
        assert_eq!(classify(DIR | REPARSE, 0xA000_000C).0, EntryKind::Link);
        // Lien symbolique vers un fichier, lien symbolique de WSL.
        assert_eq!(classify(REPARSE, 0xA000_000C).0, EntryKind::Link);
        assert_eq!(classify(REPARSE, 0xA000_001D).0, EntryKind::Link);
    }

    #[test]
    fn cloud_folders_stay_folders() {
        // Un dossier OneDrive présent sur le disque se parcourt normalement.
        for variant in 0..16u32 {
            let tag = TAG_CLOUD | (variant << 12);
            let (kind, flags) = classify(DIR | REPARSE, tag);
            assert_eq!(kind, EntryKind::Dir, "variante {variant}");
            assert!(flags.contains(EntryFlags::CLOUD));
            assert!(!flags.contains(EntryFlags::SPECIAL));
        }
    }

    #[test]
    fn online_only_files_are_never_readable() {
        for attribute in [ATTR_OFFLINE, ATTR_RECALL_ON_OPEN, ATTR_RECALL_ON_DATA_ACCESS] {
            let (kind, flags) = classify(0x20 | attribute, 0);
            let meta = meta_with(kind, flags);
            assert!(flags.contains(EntryFlags::ONLINE_ONLY));
            assert!(!meta.is_readable_file());
        }
        // Même verdict quand le fournisseur expose son point d'analyse.
        let (kind, flags) = classify(0x20 | REPARSE | ATTR_RECALL_ON_DATA_ACCESS, TAG_CLOUD);
        assert!(!meta_with(kind, flags).is_readable_file());
    }

    #[test]
    fn online_only_folders_are_not_walked() {
        let (kind, flags) = classify(DIR | ATTR_RECALL_ON_OPEN, 0);
        assert!(!meta_with(kind, flags).is_walkable_dir());
    }

    #[test]
    fn transparent_tags_read_like_plain_files() {
        for tag in [TAG_WOF, TAG_DEDUP] {
            let (kind, flags) = classify(0x20 | REPARSE, tag);
            assert!(meta_with(kind, flags).is_readable_file());
        }
    }

    #[test]
    fn unknown_tags_are_left_alone() {
        // Alias d'exécution d'app, fichier projeté, étiquette absente.
        for tag in [0x8000_001B, 0x9000_001C, 0] {
            let (kind, flags) = classify(0x20 | REPARSE, tag);
            assert!(!meta_with(kind, flags).is_readable_file(), "étiquette {tag:#x}");
            let (kind, flags) = classify(DIR | REPARSE, tag);
            assert!(!meta_with(kind, flags).is_walkable_dir(), "étiquette {tag:#x}");
        }
        // Un dossier dédupliqué ou compressé n'existe pas : on n'y entre pas.
        let (kind, flags) = classify(DIR | REPARSE, TAG_WOF);
        assert!(!meta_with(kind, flags).is_walkable_dir());
    }

    #[test]
    fn hidden_and_system_are_reported() {
        let (_, flags) = classify(ATTR_HIDDEN | ATTR_SYSTEM, 0);
        assert!(flags.contains(EntryFlags::HIDDEN));
        assert!(flags.contains(EntryFlags::SYSTEM));
        assert_eq!(format!("{flags:?}"), "{HIDDEN, SYSTEM}");
    }

    fn meta_with(kind: EntryKind, flags: EntryFlags) -> EntryMeta {
        EntryMeta {
            kind,
            flags,
            size: 1,
            allocated: 1,
            created: Timestamp::UNIX_EPOCH,
            modified: Timestamp::UNIX_EPOCH,
            accessed: Timestamp::UNIX_EPOCH,
            file_id: None,
        }
    }
}
