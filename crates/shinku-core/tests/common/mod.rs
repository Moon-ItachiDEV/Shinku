//! Outils communs aux tests d'intégration.

// Chaque fichier de tests n'utilise qu'une partie de ces outils.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use shinku_core::fs::{DirReader, EntryFlags, EntryKind, EntryMeta, to_internal_path};

/// Dossier temporaire, sous sa forme interne (chemin réel, littéral sous Windows).
pub fn workspace() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("dossier temporaire");
    let real = fs::canonicalize(temp.path()).expect("chemin réel");
    let root = to_internal_path(&real).expect("chemin interne");
    (temp, root)
}

/// Ce que la liste du dossier parent dit de `path`.
pub fn listed(path: &Path) -> Option<EntryMeta> {
    let name = path.file_name()?;
    let mut found = None;
    let _ = DirReader::new().read(path.parent()?, None, &mut |entry, meta| {
        if entry == name {
            found = Some(*meta);
        }
    });
    found
}

/// Vrai si `link` existe bel et bien comme lien. Certaines machines (Wine,
/// par exemple) répondent « fait » sans rien créer.
pub fn is_a_link(link: &Path) -> bool {
    listed(link).is_some_and(|meta| meta.kind == EntryKind::Link)
}

/// Crée un lien de dossier. Renvoie `false` si la machine ne le permet pas
/// (sous Windows, un lien symbolique demande un droit particulier).
pub fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    let created = {
        use shinku_core::fs::to_display_path;
        // Une jonction ne demande aucun droit : c'est le piège le plus courant.
        let junction = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(to_display_path(link))
            .arg(to_display_path(target))
            .output()
            .is_ok_and(|output| output.status.success());
        (junction && is_a_link(link)) || std::os::windows::fs::symlink_dir(target, link).is_ok()
    };
    created && is_a_link(link)
}

/// Crée un lien de fichier. Renvoie `false` si la machine ne le permet pas.
pub fn link_file(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    let created = std::os::windows::fs::symlink_file(target, link).is_ok();
    created && is_a_link(link)
}

/// Vrai si l'Explorateur ne montrerait pas `path`.
pub fn is_hidden(path: &Path) -> bool {
    listed(path).is_some_and(|meta| meta.flags.contains(EntryFlags::HIDDEN))
}

/// Écrit dans `dir` un fichier que l'Explorateur ne montre pas, et rend son
/// chemin : un point en tête du nom sous Unix, l'attribut « caché » sous
/// Windows. `None` si la machine n'en garde pas la trace.
pub fn write_hidden(dir: &Path, name: &str, content: &[u8]) -> Option<PathBuf> {
    fs::create_dir_all(dir).ok()?;
    #[cfg(unix)]
    let path = {
        let path = dir.join(format!(".{name}"));
        fs::write(&path, content).ok()?;
        path
    };
    #[cfg(windows)]
    let path = {
        use std::io::Write;
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        let path = dir.join(name);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .attributes(FILE_ATTRIBUTE_HIDDEN)
            .open(&path)
            .ok()?;
        file.write_all(content).ok()?;
        path
    };
    is_hidden(&path).then_some(path)
}

/// Crée dans `parent` un dossier que l'Explorateur ne montre pas, et rend son
/// chemin. `None` si la machine ne sait pas le faire.
pub fn create_hidden_dir(parent: &Path, name: &str) -> Option<PathBuf> {
    #[cfg(unix)]
    let path = {
        let path = parent.join(format!(".{name}"));
        fs::create_dir_all(&path).ok()?;
        path
    };
    #[cfg(windows)]
    let path = {
        use shinku_core::fs::to_display_path;
        let path = parent.join(name);
        fs::create_dir_all(&path).ok()?;
        let hidden = std::process::Command::new("attrib")
            .arg("+h")
            .arg(to_display_path(&path))
            .output()
            .is_ok_and(|output| output.status.success());
        if !hidden {
            return None;
        }
        path
    };
    is_hidden(&path).then_some(path)
}
