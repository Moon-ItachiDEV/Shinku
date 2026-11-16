//! Les suppressions de `--oublier` sur de vrais dossiers : un fichier nommé, un
//! dossier vide, une arborescence. Ces essais passent tels quels sous Unix et
//! sous Windows (y compris Wine) : ils décrivent ce que `shinku_core::fs` fait,
//! quelle que soit l'implémentation.
//!
//! Un essai qui a besoin d'un lien de dossier ou d'un lien physique que la
//! machine ne sait pas créer écrit « SAUTÉ » sur la sortie d'erreur et rend la
//! main : cherche ce mot avec `--nocapture` pour savoir ce qui n'a pas tourné.

// Dans un test, un `unwrap` qui échoue est un test qui échoue : c'est voulu.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use shinku_core::fs::{
    Removed, TreeLimits, TreeReport, remove_empty_dir, remove_plain_file, remove_tree,
};

mod common;
use common::{link_dir, link_file, workspace, write};

fn file(dir: &Path, name: &str) -> Removed {
    remove_plain_file(dir, name.as_ref())
}

fn skipped(why: &str) {
    eprintln!("SAUTÉ : {why}");
}

// --- Un fichier nommé ---

#[test]
fn a_plain_file_goes_and_its_neighbours_stay() {
    let (_temp, root) = workspace();
    let dir = root.join("Shinku");
    write(&dir.join("settings.json"), b"{}");
    write(&dir.join("autre.txt"), b"a moi");
    assert_eq!(file(&dir, "settings.json"), Removed::Gone);
    assert!(!dir.join("settings.json").exists());
    assert!(dir.join("autre.txt").exists(), "un fichier hors liste n'est pas touché");
    // Une seconde fois : il n'y a plus rien.
    assert_eq!(file(&dir, "settings.json"), Removed::Missing);
}

#[test]
fn a_file_in_a_missing_folder_is_missing() {
    let (_temp, root) = workspace();
    assert_eq!(file(&root.join("absent"), "settings.json"), Removed::Missing);
}

#[test]
fn a_file_with_two_names_is_kept_whole() {
    let (_temp, root) = workspace();
    let dir = root.join("Shinku");
    write(&dir.join("settings.json"), b"contenu");
    if fs::hard_link(dir.join("settings.json"), root.join("ailleurs.json")).is_err() {
        return skipped("liens physiques indisponibles");
    }
    assert_eq!(file(&dir, "settings.json"), Removed::Kept);
    assert_eq!(fs::read(dir.join("settings.json")).unwrap(), b"contenu");
    assert_eq!(fs::read(root.join("ailleurs.json")).unwrap(), b"contenu");
}

#[test]
fn a_link_with_the_name_of_a_file_is_kept_and_its_target_untouched() {
    let (_temp, root) = workspace();
    let dir = root.join("Shinku");
    write(&root.join("cible.txt"), b"precieux");
    if !link_file(&root.join("cible.txt"), &dir_with(&dir).join("settings.json")) {
        return skipped("liens de fichier indisponibles");
    }
    assert_eq!(file(&dir, "settings.json"), Removed::Kept);
    assert_eq!(fs::read(root.join("cible.txt")).unwrap(), b"precieux");
    assert!(fs::symlink_metadata(dir.join("settings.json")).is_ok(), "le lien reste");
}

fn dir_with(dir: &Path) -> &Path {
    fs::create_dir_all(dir).unwrap();
    dir
}

#[test]
fn a_folder_with_the_name_of_a_file_is_kept() {
    let (_temp, root) = workspace();
    let dir = root.join("Shinku");
    write(&dir.join("settings.json").join("dedans"), b"x");
    assert_eq!(file(&dir, "settings.json"), Removed::Kept);
    assert!(dir.join("settings.json").join("dedans").exists());
}

#[test]
fn a_file_behind_a_link_in_its_folder_chain_is_never_deleted() {
    let (_temp, root) = workspace();
    let real = root.join("vrai");
    write(&real.join("settings.json"), b"temoin");
    if !link_dir(&real, &root.join("Shinku")) {
        return skipped("liens de dossier indisponibles");
    }
    // `Shinku` est une jonction : le fichier de l'autre côté est hors d'atteinte.
    assert_eq!(file(&root.join("Shinku"), "settings.json"), Removed::Kept);
    assert_eq!(fs::read(real.join("settings.json")).unwrap(), b"temoin");
    // Un lien plus haut dans la chaîne, de même : c'est le piège que le seul
    // dernier composant ne retient pas (un dossier du milieu qui est un lien).
    write(&real.join("sous").join("settings.json"), b"temoin profond");
    let deep = root.join("Shinku").join("sous");
    assert_eq!(file(&deep, "settings.json"), Removed::Kept);
    assert_eq!(fs::read(real.join("sous").join("settings.json")).unwrap(), b"temoin profond");
}

// --- Un dossier vide ---

#[test]
fn an_empty_folder_goes_and_a_full_one_stays() {
    let (_temp, root) = workspace();
    let empty = root.join("vide");
    fs::create_dir(&empty).unwrap();
    assert_eq!(remove_empty_dir(&empty), Removed::Gone);
    assert!(!empty.exists());
    assert_eq!(remove_empty_dir(&empty), Removed::Missing);

    let full = root.join("plein");
    write(&full.join("hors-liste.txt"), b"x");
    assert_eq!(remove_empty_dir(&full), Removed::Kept);
    assert!(full.join("hors-liste.txt").exists());
}

#[test]
fn a_link_in_place_of_the_folder_is_kept_with_its_target() {
    let (_temp, root) = workspace();
    let real = root.join("vrai");
    fs::create_dir(&real).unwrap();
    if !link_dir(&real, &root.join("Shinku")) {
        return skipped("liens de dossier indisponibles");
    }
    assert_eq!(remove_empty_dir(&root.join("Shinku")), Removed::Kept);
    assert!(real.exists());
    assert!(fs::symlink_metadata(root.join("Shinku")).is_ok(), "le lien reste");
}

// --- Une arborescence ---

fn limits() -> TreeLimits {
    TreeLimits::default()
}

fn build_cache(root: &Path) {
    write(&root.join("EBWebView").join("Default").join("Cache").join("data_0"), &[1; 64]);
    write(&root.join("EBWebView").join("Default").join("Cache").join("data_1"), &[2; 64]);
    write(&root.join("EBWebView").join("Local State"), b"{}");
    fs::create_dir_all(root.join("EBWebView").join("vide").join("encore")).unwrap();
    write(&root.join("a-la-racine.bin"), b"x");
}

#[test]
fn a_whole_tree_goes_with_its_root() {
    let (_temp, root) = workspace();
    let tree = root.join("fr.shinku.app");
    build_cache(&tree);
    write(&root.join("voisin.txt"), b"a moi");
    let report = remove_tree(&tree, limits());
    assert!(report.is_clean(), "{report:?}");
    // 4 fichiers, 6 dossiers (EBWebView, Default, Cache, vide, encore, la racine).
    assert_eq!(report.removed, 4 + 6);
    assert!(!tree.exists());
    assert!(root.join("voisin.txt").exists(), "ce qui est à côté n'est pas touché");
}

#[test]
fn a_missing_root_is_nothing_to_do() {
    let (_temp, root) = workspace();
    let report = remove_tree(&root.join("fr.shinku.app"), limits());
    assert_eq!(report, TreeReport::default());
    assert!(report.is_clean());
}

#[test]
fn a_root_that_is_a_link_is_left_and_nothing_behind_it_is_touched() {
    let (_temp, root) = workspace();
    let behind = root.join("derriere");
    write(&behind.join("temoin.txt"), b"precieux");
    write(&behind.join("sous").join("temoin2.txt"), b"precieux aussi");
    if !link_dir(&behind, &root.join("fr.shinku.app")) {
        return skipped("liens de dossier indisponibles");
    }
    let report = remove_tree(&root.join("fr.shinku.app"), limits());
    assert_eq!(report.removed, 0);
    assert_eq!(report.kept, 1, "{report:?}");
    assert!(!report.is_clean());
    assert_eq!(fs::read(behind.join("temoin.txt")).unwrap(), b"precieux");
    assert_eq!(fs::read(behind.join("sous").join("temoin2.txt")).unwrap(), b"precieux aussi");
    assert!(fs::symlink_metadata(root.join("fr.shinku.app")).is_ok(), "le lien reste en place");
}

#[test]
fn a_root_that_is_a_file_is_left() {
    let (_temp, root) = workspace();
    write(&root.join("fr.shinku.app"), b"un fichier");
    let report = remove_tree(&root.join("fr.shinku.app"), limits());
    assert_eq!((report.removed, report.kept), (0, 1));
    assert!(root.join("fr.shinku.app").exists());
}

#[test]
fn a_link_lower_down_is_never_crossed_and_stays_in_place() {
    let (_temp, root) = workspace();
    let tree = root.join("fr.shinku.app");
    build_cache(&tree);
    let behind = root.join("derriere");
    write(&behind.join("temoin.txt"), b"precieux");
    write(&behind.join("sous").join("temoin2.txt"), b"precieux aussi");
    if !link_dir(&behind, &tree.join("EBWebView").join("Default").join("piege")) {
        return skipped("liens de dossier indisponibles");
    }
    let report = remove_tree(&tree, limits());
    assert_eq!(report.kept, 1, "le lien est laissé, et compté : {report:?}");
    assert_eq!(report.busy, 0);
    assert!(!report.is_clean(), "« en partie »");
    // Derrière le lien, rien n'a bougé.
    assert_eq!(fs::read(behind.join("temoin.txt")).unwrap(), b"precieux");
    assert_eq!(fs::read(behind.join("sous").join("temoin2.txt")).unwrap(), b"precieux aussi");
    // Le lien est resté, avec les dossiers qui y mènent ; le reste est parti.
    let trap = tree.join("EBWebView").join("Default").join("piege");
    assert!(fs::symlink_metadata(&trap).is_ok());
    assert!(!tree.join("EBWebView").join("Default").join("Cache").exists());
    assert!(!tree.join("EBWebView").join("Local State").exists());
    assert!(!tree.join("a-la-racine.bin").exists());
    assert!(tree.join("EBWebView").join("Default").exists());
}

#[test]
fn a_file_link_lower_down_is_left_and_its_target_untouched() {
    let (_temp, root) = workspace();
    let tree = root.join("fr.shinku.app");
    build_cache(&tree);
    write(&root.join("cible.txt"), b"precieux");
    if !link_file(&root.join("cible.txt"), &tree.join("lien.txt")) {
        return skipped("liens de fichier indisponibles");
    }
    let report = remove_tree(&tree, limits());
    assert_eq!(report.kept, 1, "{report:?}");
    assert_eq!(fs::read(root.join("cible.txt")).unwrap(), b"precieux");
    assert!(fs::symlink_metadata(tree.join("lien.txt")).is_ok());
}

#[test]
fn a_file_with_two_names_goes_as_one_name_and_the_other_stays_whole() {
    let (_temp, root) = workspace();
    let tree = root.join("fr.shinku.app");
    build_cache(&tree);
    write(&root.join("memes-donnees.bin"), b"donnees partagees");
    if fs::hard_link(root.join("memes-donnees.bin"), tree.join("EBWebView").join("lien.bin"))
        .is_err()
    {
        return skipped("liens physiques indisponibles");
    }
    let report = remove_tree(&tree, limits());
    assert!(report.is_clean(), "un fichier à plusieurs noms part comme un nom : {report:?}");
    assert!(!tree.exists());
    assert_eq!(fs::read(root.join("memes-donnees.bin")).unwrap(), b"donnees partagees");
}

#[test]
fn the_depth_is_bounded() {
    let (_temp, root) = workspace();
    let tree = root.join("fr.shinku.app");
    let mut deep = tree.clone();
    for level in 0..8 {
        deep = deep.join(format!("n{level}"));
    }
    write(&deep.join("profond.bin"), b"x");
    write(&tree.join("n0").join("haut.bin"), b"x");

    // Largement assez de profondeur : tout part.
    let ample = TreeLimits { max_depth: 20, max_entries: 1000 };
    assert!(remove_tree(&tree, ample).is_clean());
    assert!(!tree.exists());

    // Trop peu : le fond est laissé, et dit.
    let mut deep = tree.clone();
    for level in 0..8 {
        deep = deep.join(format!("n{level}"));
    }
    write(&deep.join("profond.bin"), b"x");
    write(&tree.join("n0").join("haut.bin"), b"x");
    let short = TreeLimits { max_depth: 4, max_entries: 1000 };
    let report = remove_tree(&tree, short);
    assert!(report.kept >= 1, "{report:?}");
    assert!(!report.is_clean());
    assert!(deep.join("profond.bin").exists(), "au-delà de la borne : jamais atteint");
    assert!(!tree.join("n0").join("haut.bin").exists(), "en deçà : parti");
}

#[test]
fn the_number_of_entries_is_bounded() {
    let (_temp, root) = workspace();
    let tree = root.join("fr.shinku.app");
    for index in 0..50 {
        write(&tree.join(format!("f{index:02}.bin")), b"x");
    }
    let small = TreeLimits { max_depth: 10, max_entries: 20 };
    let report = remove_tree(&tree, small);
    assert!(!report.is_clean());
    assert_eq!(report.removed, 20, "on s'arrête à la borne : {report:?}");
    assert_eq!(fs::read_dir(&tree).unwrap().count(), 30);
    // Avec de quoi finir, le reste part : la borne n'est pas un état.
    assert!(remove_tree(&tree, TreeLimits { max_depth: 10, max_entries: 1000 }).is_clean());
    assert!(!tree.exists());
}

// --- Un fichier occupé (Windows seulement : Unix laisse supprimer un fichier ouvert) ---

#[cfg(windows)]
mod busy {
    use std::os::windows::fs::OpenOptionsExt;

    use super::*;

    /// Ouvre le fichier sans aucun partage : personne d'autre ne peut le
    /// supprimer tant que le handle vit, comme un programme qui le tient.
    fn hold(path: &Path) -> fs::File {
        fs::OpenOptions::new().read(true).share_mode(0).open(path).unwrap()
    }

    #[test]
    fn a_file_held_by_another_program_is_busy_then_goes_when_released() {
        let (_temp, root) = workspace();
        let dir = root.join("Shinku");
        write(&dir.join("settings.json"), b"{}");
        let held = hold(&dir.join("settings.json"));
        assert_eq!(file(&dir, "settings.json"), Removed::Busy);
        assert!(dir.join("settings.json").exists());
        drop(held);
        assert_eq!(file(&dir, "settings.json"), Removed::Gone);
    }

    #[test]
    fn a_busy_file_in_a_tree_is_busy_and_keeps_its_folders() {
        let (_temp, root) = workspace();
        let tree = root.join("fr.shinku.app");
        build_cache(&tree);
        let held = hold(&tree.join("EBWebView").join("Default").join("Cache").join("data_0"));
        let report = remove_tree(&tree, limits());
        assert_eq!(report.busy, 1, "{report:?}");
        assert_eq!(report.kept, 0);
        assert!(!report.is_clean());
        drop(held);
        assert!(remove_tree(&tree, limits()).is_clean());
        assert!(!tree.exists());
    }
}
