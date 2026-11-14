//! Contrat de la couche de lecture du disque.
//!
//! Ces tests passent tels quels sur Windows (y compris sous Wine) et sur Unix :
//! ils décrivent ce que le moteur d'analyse attend de `shinku_core::fs`, quelle
//! que soit l'implémentation.

// Dans un test, un `unwrap` qui échoue est un test qui échoue : c'est voulu.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use shinku_core::fs::{
    DirReader, EntryKind, EntryMeta, OpenError, PathState, ReadDirError, VerifiedFile, identity,
    inspect_dir_chain, to_display_path, to_internal_path,
};

/// Dossier temporaire, sous sa forme interne (chemin réel, littéral sous Windows).
fn workspace() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("dossier temporaire");
    let real = fs::canonicalize(temp.path()).expect("chemin réel");
    let root = to_internal_path(&real).expect("chemin interne");
    (temp, root)
}

fn list(dir: &Path) -> BTreeMap<String, EntryMeta> {
    let mut entries = BTreeMap::new();
    DirReader::new()
        .read(dir, None, &mut |name, meta| {
            entries.insert(name.to_string_lossy().into_owned(), *meta);
        })
        .expect("lecture du dossier");
    entries
}

/// Vrai si `link` existe bel et bien comme lien. Certaines machines (Wine,
/// par exemple) répondent « fait » sans rien créer.
fn is_a_link(link: &Path) -> bool {
    let Some(parent) = link.parent() else { return false };
    let name = link.file_name().map(|name| name.to_string_lossy().into_owned());
    let mut found = false;
    let _ = DirReader::new().read(parent, None, &mut |entry, meta| {
        if Some(entry.to_string_lossy().into_owned()) == name {
            found = meta.kind == EntryKind::Link;
        }
    });
    found
}

/// Crée un lien de dossier. Renvoie `false` si la machine ne le permet pas
/// (sous Windows, un lien symbolique demande un droit particulier).
fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    let created = {
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

fn link_file(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    let created = std::os::windows::fs::symlink_file(target, link).is_ok();
    created && is_a_link(link)
}

#[test]
fn a_listing_gives_sizes_dates_and_ids_without_opening_files() {
    let (_temp, root) = workspace();
    fs::write(root.join("été.txt"), b"bonjour").unwrap();
    fs::write(root.join("vide.bin"), b"").unwrap();
    fs::create_dir(root.join("Dossier")).unwrap();
    fs::write(root.join("Dossier").join("dedans.txt"), b"x").unwrap();

    let entries = list(&root);
    assert_eq!(entries.keys().collect::<Vec<_>>(), ["Dossier", "vide.bin", "été.txt"]);

    let file = &entries["été.txt"];
    assert_eq!(file.kind, EntryKind::File);
    assert_eq!(file.size, 7);
    assert!(file.is_readable_file());
    assert!(file.modified.unix_millis() > 1_600_000_000_000, "date plausible");
    assert_eq!(entries["vide.bin"].size, 0);

    let dir = &entries["Dossier"];
    assert_eq!(dir.kind, EntryKind::Dir);
    assert!(dir.is_walkable_dir());
    assert_eq!(list(&root.join("Dossier")).len(), 1);
}

#[test]
fn an_empty_folder_lists_nothing() {
    let (_temp, root) = workspace();
    assert!(list(&root).is_empty());
}

#[test]
fn a_large_folder_is_listed_completely() {
    // Plus d'entrées que n'en contient un bloc de lecture.
    let (_temp, root) = workspace();
    for index in 0..1500 {
        fs::write(root.join(format!("fichier-au-nom-plutôt-long-{index:05}.tmp")), b"").unwrap();
    }
    let entries = list(&root);
    assert_eq!(entries.len(), 1500);
    assert!(entries.contains_key("fichier-au-nom-plutôt-long-01499.tmp"));
}

#[test]
fn a_folder_link_is_seen_but_never_followed() {
    let (_temp, root) = workspace();
    let outside = root.join("ailleurs");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("précieux.doc"), b"ne pas toucher").unwrap();
    let scanned = root.join("analysé");
    fs::create_dir(&scanned).unwrap();
    if !link_dir(&outside, &scanned.join("piège")) {
        eprintln!("liens de dossier indisponibles ici : test ignoré");
        return;
    }

    let entries = list(&scanned);
    let trap = &entries["piège"];
    assert_eq!(trap.kind, EntryKind::Link, "un lien n'est pas un dossier");
    assert!(!trap.is_walkable_dir());

    // Même si on le demande explicitement, on ne liste pas à travers un lien.
    let result = DirReader::new().read(&scanned.join("piège"), None, &mut |_, _| {});
    assert!(matches!(result, Err(ReadDirError::NotADirectory)), "obtenu : {result:?}");
}

#[test]
fn a_missing_folder_is_reported_as_missing() {
    let (_temp, root) = workspace();
    let result = DirReader::new().read(&root.join("absent"), None, &mut |_, _| {});
    assert!(matches!(result, Err(ReadDirError::NotFound)), "obtenu : {result:?}");
}

#[test]
fn a_file_is_not_a_folder() {
    let (_temp, root) = workspace();
    fs::write(root.join("fichier"), b"x").unwrap();
    let result = DirReader::new().read(&root.join("fichier"), None, &mut |_, _| {});
    assert!(matches!(result, Err(ReadDirError::NotADirectory)), "obtenu : {result:?}");
}

#[test]
fn an_unchanged_file_can_be_read() {
    let (_temp, root) = workspace();
    let path = root.join("photo.raw");
    fs::write(&path, b"contenu").unwrap();
    let meta = list(&root)["photo.raw"];

    let mut opened = VerifiedFile::open(&path, meta.fingerprint()).expect("ouverture vérifiée");
    let mut content = String::new();
    opened.file_mut().read_to_string(&mut content).unwrap();
    assert_eq!(content, "contenu");
    opened.recheck().expect("toujours le même fichier");
}

#[test]
fn a_file_that_changed_is_refused() {
    let (_temp, root) = workspace();
    let path = root.join("notes.txt");
    fs::write(&path, b"avant").unwrap();
    let meta = list(&root)["notes.txt"];

    let opened = VerifiedFile::open(&path, meta.fingerprint()).expect("ouverture vérifiée");
    fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b" puis la suite").unwrap();
    assert!(matches!(opened.recheck(), Err(OpenError::Changed)));
    drop(opened);
    let again = VerifiedFile::open(&path, meta.fingerprint());
    assert!(matches!(again, Err(OpenError::Changed)), "obtenu : {again:?}");
}

#[test]
fn a_file_replaced_by_another_is_refused() {
    let (_temp, root) = workspace();
    let path = root.join("cache.bin");
    fs::write(&path, b"12345").unwrap();
    let meta = list(&root)["cache.bin"];
    assert!(meta.file_id.is_some(), "le système de fichiers de test fournit un identifiant");

    // Même nom, même taille, mais c'est un autre fichier.
    fs::write(root.join("autre.bin"), b"abcde").unwrap();
    fs::remove_file(&path).unwrap();
    fs::rename(root.join("autre.bin"), &path).unwrap();
    let result = VerifiedFile::open(&path, meta.fingerprint());
    assert!(matches!(result, Err(OpenError::Changed)), "obtenu : {result:?}");
}

#[test]
fn a_file_link_is_never_opened() {
    let (_temp, root) = workspace();
    let target = root.join("important.doc");
    fs::write(&target, b"12345").unwrap();
    let meta = list(&root)["important.doc"];
    let link = root.join("cache.bin");
    if !link_file(&target, &link) {
        eprintln!("liens de fichier indisponibles ici : test ignoré");
        return;
    }
    assert_eq!(list(&root)["cache.bin"].kind, EntryKind::Link);
    let result = VerifiedFile::open(&link, meta.fingerprint());
    assert!(matches!(result, Err(OpenError::NotRegular)), "obtenu : {result:?}");
}

#[test]
fn a_missing_file_is_reported_as_missing() {
    let (_temp, root) = workspace();
    let path = root.join("éphémère.tmp");
    fs::write(&path, b"x").unwrap();
    let meta = list(&root)["éphémère.tmp"];
    fs::remove_file(&path).unwrap();
    let result = VerifiedFile::open(&path, meta.fingerprint());
    assert!(matches!(result, Err(OpenError::NotFound)), "obtenu : {result:?}");
}

#[test]
fn identity_survives_a_rename_and_matches_the_listing() {
    let (_temp, root) = workspace();
    let before = root.join("avant");
    fs::create_dir(&before).unwrap();
    let Some(id) = identity(&before).unwrap() else {
        eprintln!("pas d'identifiant sur ce système de fichiers : test ignoré");
        return;
    };
    let listed = list(&root)["avant"].file_id.expect("identifiant dans la liste");
    assert_eq!(listed.0 as u64, id.file_id.0 as u64, "même identifiant par les deux voies");

    let after = root.join("après");
    fs::rename(&before, &after).unwrap();
    assert_eq!(identity(&after).unwrap(), Some(id));
    // Deux dossiers différents n'ont pas la même identité.
    fs::create_dir(&before).unwrap();
    assert_ne!(identity(&before).unwrap(), Some(id));
}

#[test]
fn hard_links_share_one_identifier() {
    let (_temp, root) = workspace();
    fs::write(root.join("original.bin"), b"contenu").unwrap();
    if fs::hard_link(root.join("original.bin"), root.join("copie.bin")).is_err() {
        eprintln!("liens physiques indisponibles ici : test ignoré");
        return;
    }
    let entries = list(&root);
    assert!(entries["original.bin"].file_id.is_some());
    assert_eq!(entries["original.bin"].file_id, entries["copie.bin"].file_id);
}

#[test]
fn long_paths_are_supported() {
    let (_temp, root) = workspace();
    let mut deep = root.clone();
    // Bien au-delà des 260 caractères historiques de Windows.
    for level in 0..12 {
        deep.push(format!("niveau-{level:02}-avec-un-nom-de-dossier-assez-long"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("au-fond.txt"), b"ok").unwrap();
    assert!(deep.as_os_str().len() > 400);

    let entries = list(&deep);
    let meta = entries["au-fond.txt"];
    assert_eq!(meta.size, 2);
    VerifiedFile::open(&deep.join("au-fond.txt"), meta.fingerprint()).expect("ouverture");
}

#[test]
fn display_paths_round_trip() {
    let (_temp, root) = workspace();
    let shown = to_display_path(&root);
    assert_eq!(to_internal_path(&shown).unwrap(), root);
    #[cfg(windows)]
    assert!(!shown.to_string_lossy().starts_with(r"\\?\"));
}

#[test]
fn a_folder_swapped_for_another_is_refused() {
    let (_temp, root) = workspace();
    fs::create_dir(root.join("cache")).unwrap();
    fs::create_dir(root.join("documents")).unwrap();
    fs::write(root.join("documents").join("thèse.doc"), b"x").unwrap();
    let seen = list(&root)["cache"].file_id;
    assert!(seen.is_some());

    // Entre la liste du parent et la lecture, un autre dossier prend le nom.
    fs::remove_dir(root.join("cache")).unwrap();
    fs::rename(root.join("documents"), root.join("cache")).unwrap();
    let result = DirReader::new().read(&root.join("cache"), seen, &mut |_, _| {});
    assert!(matches!(result, Err(ReadDirError::Changed)), "obtenu : {result:?}");

    // Avec le bon identifiant, la lecture passe.
    let now = list(&root)["cache"].file_id;
    let mut count = 0;
    DirReader::new().read(&root.join("cache"), now, &mut |_, _| count += 1).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn a_path_is_inspected_folder_by_folder() {
    let (_temp, root) = workspace();
    let deep = root.join("a").join("b");
    fs::create_dir_all(&deep).unwrap();
    fs::write(root.join("fichier"), b"x").unwrap();

    assert_eq!(inspect_dir_chain(&deep).unwrap(), PathState::RealDir);
    assert_eq!(inspect_dir_chain(&root.join("absent").join("x")).unwrap(), PathState::Missing);
    assert_eq!(inspect_dir_chain(&root.join("fichier")).unwrap(), PathState::NotPlain);

    if link_dir(&root.join("a"), &root.join("lien")) {
        // Le lien est au milieu du chemin : c'est justement ce qu'on cherche.
        assert_eq!(inspect_dir_chain(&root.join("lien").join("b")).unwrap(), PathState::NotPlain);
        assert_eq!(inspect_dir_chain(&root.join("lien")).unwrap(), PathState::NotPlain);
    }
}
