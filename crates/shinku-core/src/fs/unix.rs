//! Lecture du système de fichiers pour le développement et les tests.
//!
//! Shinku ne vise que Windows. Cette implémentation permet de compiler et de
//! tester le cœur sur Linux et macOS avec les mêmes règles : aucun lien n'est
//! suivi, et un fichier n'est lu qu'après vérification de son identité. Elle
//! n'est pas livrée.

use std::ffi::OsStr;
use std::fs::{self, File, Metadata};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf, PrefixComponent};

use super::{
    EntryFlags, EntryKind, EntryMeta, Expected, Fingerprint, Identity, OpenError, ReadDirError,
    RemovalFacts, RemoveError,
};
use crate::model::{FileId, Timestamp};

/// Nature d'une entrée, lue sans suivre de lien.
fn kind_of(metadata: &Metadata) -> EntryKind {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        EntryKind::Link
    } else if file_type.is_dir() {
        EntryKind::Dir
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

/// Traduit des métadonnées obtenues sans suivre de lien.
fn meta_from(name: &OsStr, metadata: &Metadata) -> EntryMeta {
    let kind = kind_of(metadata);
    // Convention Unix : un nom qui commence par un point est caché.
    let flags = if name.as_bytes().first() == Some(&b'.') {
        EntryFlags::HIDDEN
    } else {
        EntryFlags::empty()
    };
    let modified = modified_of(metadata);
    EntryMeta {
        kind,
        flags,
        size: metadata.len(),
        allocated: metadata.blocks().saturating_mul(512),
        // Tous les systèmes de fichiers ne gardent pas la date de création.
        created: metadata.created().map(Timestamp::from_system_time).unwrap_or(modified),
        modified,
        accessed: Timestamp::from_unix(metadata.atime(), metadata.atime_nsec().max(0) as u32),
        file_id: Some(FileId(u128::from(metadata.ino()))),
    }
}

fn modified_of(metadata: &Metadata) -> Timestamp {
    Timestamp::from_unix(metadata.mtime(), metadata.mtime_nsec().max(0) as u32)
}

pub(super) struct DirReader;

impl DirReader {
    pub(super) fn new() -> Self {
        Self
    }

    pub(super) fn read(
        &mut self,
        dir: &Path,
        expected: Option<FileId>,
        visit: &mut dyn FnMut(&OsStr, &EntryMeta),
    ) -> Result<(), ReadDirError> {
        // `read_dir` suivrait un lien final : on refuse d'abord tout ce qui
        // n'est pas un vrai dossier.
        let metadata = fs::symlink_metadata(dir)?;
        if !metadata.file_type().is_dir() {
            return Err(ReadDirError::NotADirectory);
        }
        if expected.is_some_and(|expected| expected != FileId(u128::from(metadata.ino()))) {
            return Err(ReadDirError::Changed);
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            // `DirEntry::metadata` ne suit pas les liens. Une entrée qui
            // disparaît pendant la lecture est simplement ignorée.
            let Ok(metadata) = entry.metadata() else { continue };
            let name = entry.file_name();
            visit(&name, &meta_from(&name, &metadata));
        }
        Ok(())
    }
}

fn open_error(error: io::Error) -> OpenError {
    match error.kind() {
        io::ErrorKind::PermissionDenied => OpenError::AccessDenied,
        io::ErrorKind::NotFound => OpenError::NotFound,
        _ => OpenError::Io(error),
    }
}

pub(super) fn open_verified(path: &Path, expected: &Fingerprint) -> Result<File, OpenError> {
    // Sans option « ne pas suivre » portable, on procède en trois temps :
    // regarder sans suivre, ouvrir, puis vérifier que le fichier ouvert est
    // bien celui qu'on a regardé. Un lien glissé entre-temps est ainsi détecté.
    let before = fs::symlink_metadata(path).map_err(open_error)?;
    if !before.file_type().is_file() {
        return Err(OpenError::NotRegular);
    }
    let file = File::open(path).map_err(open_error)?;
    let after = file.metadata().map_err(open_error)?;
    if (after.dev(), after.ino()) != (before.dev(), before.ino()) {
        return Err(OpenError::Changed);
    }
    check_open_file(&file, expected)?;
    Ok(file)
}

pub(super) fn check_open_file(file: &File, expected: &Fingerprint) -> Result<(), OpenError> {
    let metadata = file.metadata().map_err(open_error)?;
    if !metadata.file_type().is_file() {
        return Err(OpenError::NotRegular);
    }
    if metadata.nlink() == 0 {
        return Err(OpenError::NotFound);
    }
    let same_id =
        expected.file_id.is_none_or(|expected| expected == FileId(u128::from(metadata.ino())));
    if metadata.len() != expected.size || modified_of(&metadata) != expected.modified || !same_id {
        return Err(OpenError::Changed);
    }
    Ok(())
}

fn remove_error(error: io::Error) -> RemoveError {
    match error.kind() {
        io::ErrorKind::PermissionDenied => RemoveError::AccessDenied,
        io::ErrorKind::NotFound => RemoveError::NotFound,
        io::ErrorKind::DirectoryNotEmpty => RemoveError::NotEmpty,
        _ => RemoveError::Io(error),
    }
}

/// Une entrée vérifiée, à supprimer par son chemin.
///
/// Rien ne permet ici de tenir l'entrée entre la vérification et la
/// suppression : cette implémentation n'est pas livrée.
pub(super) struct Removal {
    path: PathBuf,
    dir: bool,
}

impl Removal {
    pub(super) fn open(path: &Path, expected: &Expected) -> Result<Self, RemoveError> {
        let metadata = fs::symlink_metadata(path).map_err(remove_error)?;
        let kind = kind_of(&metadata);
        if kind != expected.kind || kind == EntryKind::Other {
            return Err(RemoveError::Changed);
        }
        let same_id =
            expected.file_id.is_none_or(|expected| expected == FileId(u128::from(metadata.ino())));
        let same_content = kind == EntryKind::Dir
            || (metadata.len() == expected.size && modified_of(&metadata) == expected.modified);
        if !same_id || !same_content {
            return Err(RemoveError::Changed);
        }
        Ok(Self { path: path.to_path_buf(), dir: kind == EntryKind::Dir })
    }

    pub(super) fn facts(&self) -> Result<RemovalFacts, RemoveError> {
        let metadata = fs::symlink_metadata(&self.path).map_err(remove_error)?;
        Ok(RemovalFacts {
            links: u32::try_from(metadata.nlink()).unwrap_or(u32::MAX),
            online_only: false,
            regular: metadata.file_type().is_file(),
        })
    }

    pub(super) fn modified(&self) -> Result<Timestamp, RemoveError> {
        fs::symlink_metadata(&self.path)
            .map(|metadata| modified_of(&metadata))
            .map_err(remove_error)
    }

    pub(super) fn mark(&mut self) -> Result<(), RemoveError> {
        // Rien à réserver : la suppression a lieu à la confirmation.
        Ok(())
    }

    pub(super) fn commit(self) -> Result<(), RemoveError> {
        // `remove_file` supprime un lien en tant que lien, et `remove_dir`
        // refuse un dossier qui n'est pas vide.
        let removed =
            if self.dir { fs::remove_dir(&self.path) } else { fs::remove_file(&self.path) };
        removed.map_err(remove_error)
    }
}

pub(super) fn is_real_dir(path: &Path) -> io::Result<bool> {
    Ok(fs::symlink_metadata(path)?.file_type().is_dir())
}

pub(super) fn identity(path: &Path) -> io::Result<Option<Identity>> {
    let metadata = fs::symlink_metadata(path)?;
    Ok(Some(Identity { volume: metadata.dev(), file_id: FileId(u128::from(metadata.ino())) }))
}

pub(super) fn to_internal_path(path: &Path) -> io::Result<PathBuf> {
    let invalid = |message: &str| io::Error::new(io::ErrorKind::InvalidInput, message.to_owned());
    if !path.is_absolute() {
        return Err(invalid("le chemin doit être absolu"));
    }
    let mut internal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => internal.push("/"),
            Component::Normal(name) => internal.push(name),
            Component::CurDir => {}
            Component::ParentDir => return Err(invalid("le chemin ne doit pas contenir « .. »")),
            Component::Prefix(_) => return Err(invalid("préfixe de chemin inattendu")),
        }
    }
    Ok(internal)
}

pub(super) fn to_display_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

pub(super) fn fold_prefix(_prefix: &PrefixComponent<'_>) -> Option<String> {
    // Un chemin Unix n'a jamais de préfixe.
    None
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::os::unix::fs::symlink;

    use super::super::{DirReader, VerifiedFile};
    use super::*;

    fn list(dir: &Path) -> Vec<(String, EntryMeta)> {
        let mut entries = Vec::new();
        DirReader::new()
            .read(dir, None, &mut |name, meta| {
                entries.push((name.to_string_lossy().into_owned(), *meta));
            })
            .unwrap();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    #[test]
    fn lists_files_folders_and_links_without_following() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("a.txt"), b"bonjour").unwrap();
        fs::create_dir(root.join("dossier")).unwrap();
        fs::write(root.join(".cache"), b"").unwrap();
        symlink(root.join("dossier"), root.join("lien")).unwrap();

        let entries = list(root);
        let names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, [".cache", "a.txt", "dossier", "lien"]);

        assert!(entries[0].1.flags.contains(EntryFlags::HIDDEN));
        assert_eq!(entries[1].1.kind, EntryKind::File);
        assert_eq!(entries[1].1.size, 7);
        assert!(entries[1].1.file_id.is_some());
        assert!(entries[2].1.is_walkable_dir());
        assert_eq!(entries[3].1.kind, EntryKind::Link);
        assert!(!entries[3].1.is_walkable_dir());
    }

    #[test]
    fn refuses_to_list_through_a_link() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("vrai")).unwrap();
        symlink(temp.path().join("vrai"), temp.path().join("lien")).unwrap();

        let result = DirReader::new().read(&temp.path().join("lien"), None, &mut |_, _| {});
        assert!(matches!(result, Err(ReadDirError::NotADirectory)));
        let result = DirReader::new().read(&temp.path().join("absent"), None, &mut |_, _| {});
        assert!(matches!(result, Err(ReadDirError::NotFound)));
    }

    #[test]
    fn verified_open_reads_an_unchanged_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("photo.raw");
        fs::write(&path, b"contenu").unwrap();
        let (_, meta) = list(temp.path()).remove(0);

        let mut opened = VerifiedFile::open(&path, meta.fingerprint()).unwrap();
        let mut content = String::new();
        opened.file_mut().read_to_string(&mut content).unwrap();
        assert_eq!(content, "contenu");
        opened.recheck().unwrap();
    }

    #[test]
    fn verified_open_notices_a_change() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("notes.txt");
        fs::write(&path, b"avant").unwrap();
        let (_, meta) = list(temp.path()).remove(0);

        // Le fichier grossit après l'analyse.
        let opened = VerifiedFile::open(&path, meta.fingerprint()).unwrap();
        fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b" plus tard").unwrap();
        assert!(matches!(opened.recheck(), Err(OpenError::Changed)));
        assert!(matches!(VerifiedFile::open(&path, meta.fingerprint()), Err(OpenError::Changed)));
    }

    #[test]
    fn verified_open_refuses_a_link_swapped_in() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cache.bin");
        let target = temp.path().join("important.doc");
        fs::write(&path, b"12345").unwrap();
        fs::write(&target, b"12345").unwrap();
        let fingerprint = list(temp.path())
            .into_iter()
            .find(|(name, _)| name == "cache.bin")
            .map(|(_, meta)| meta.fingerprint())
            .unwrap();

        // Un lien vers un autre fichier prend la place du fichier analysé.
        fs::remove_file(&path).unwrap();
        symlink(&target, &path).unwrap();
        assert!(matches!(VerifiedFile::open(&path, fingerprint), Err(OpenError::NotRegular)));
        // Disparu : signalé comme tel.
        fs::remove_file(&path).unwrap();
        assert!(matches!(VerifiedFile::open(&path, fingerprint), Err(OpenError::NotFound)));
    }

    #[test]
    fn identity_follows_a_rename() {
        let temp = tempfile::tempdir().unwrap();
        let before = temp.path().join("avant");
        let after = temp.path().join("après");
        fs::create_dir(&before).unwrap();
        let id = identity(&before).unwrap();
        fs::rename(&before, &after).unwrap();
        assert_eq!(identity(&after).unwrap(), id);
        assert!(id.is_some());
    }

    #[test]
    fn internal_paths_must_be_absolute_and_plain() {
        assert_eq!(to_internal_path(Path::new("/a/./b")).unwrap(), Path::new("/a/b"));
        assert_eq!(to_internal_path(Path::new("/")).unwrap(), Path::new("/"));
        assert!(to_internal_path(Path::new("a/b")).is_err());
        assert!(to_internal_path(Path::new("/a/../b")).is_err());
    }
}
