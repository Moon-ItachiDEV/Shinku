//! `forget::run` de bout en bout, sur de vrais dossiers : la décision du cœur
//! avec les vraies suppressions de `shinku_core::fs`. Seuls l'élévation, les
//! dossiers connus et l'horloge sont fabriqués (l'horloge ne dort pas).
//!
//! Sous Wine, ces essais font tourner la vraie implémentation Windows de `fs`.
//! Ils ne prouvent pas la décision d'élévation de la vraie machine (Wine dit
//! toujours « élevé » : voir `docs/conception.md`).

// Dans un test, un `unwrap` qui échoue est un test qui échoue : c'est voulu.
#![allow(clippy::unwrap_used)]

use std::cell::Cell;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use shinku_core::forget::{self, ForgetHost, Outcome};
use shinku_core::fs::{self as core_fs, Removed, TreeLimits, TreeReport};
use shinku_core::platform::{Environment, KnownFolder};

mod common;
use common::{Pc, link_dir, write};

/// Les vraies suppressions, sur le PC fabriqué de `common::Pc`.
struct Host<'a> {
    pc: &'a Pc,
    elevated: bool,
    clock: Cell<Duration>,
}

impl<'a> Host<'a> {
    fn new(pc: &'a Pc) -> Self {
        Self { pc, elevated: false, clock: Cell::new(Duration::ZERO) }
    }
}

impl ForgetHost for Host<'_> {
    fn elevated(&self) -> bool {
        self.elevated
    }
    fn folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        self.pc.environment.known_folder(folder)
    }
    fn elapsed(&self) -> Duration {
        self.clock.get()
    }
    fn sleep(&self, duration: Duration) {
        self.clock.set(self.clock.get() + duration);
    }
    fn remove_file(&self, dir: &Path, name: &OsStr) -> Removed {
        core_fs::remove_plain_file(dir, name)
    }
    fn remove_empty_dir(&self, dir: &Path) -> Removed {
        core_fs::remove_empty_dir(dir)
    }
    fn remove_tree(&self, root: &Path, deadline: Duration) -> TreeReport {
        core_fs::remove_tree(root, TreeLimits::default(), &|| self.elapsed() >= deadline)
    }
}

/// Pose, comme Shinku et le modèle de Tauri les laissent, des fichiers de
/// réglages et un cache, et des témoins que `--oublier` ne doit pas toucher.
fn seed(pc: &Pc) {
    for name in ["settings.json", "settings.json.tmp", "settings.illisible.json", "journal.json"] {
        write(&pc.roaming.join("Shinku").join(name), b"{}");
    }
    write(&pc.roaming.join("Shinku").join("journal.json.tmp"), b"{}");
    for name in ["game.json", "game.json.tmp", "startup.json", "startup.json.tmp"] {
        write(&pc.local.join("Shinku").join(name), b"{}");
    }
    for base in [&pc.roaming, &pc.local] {
        let tree = base.join(forget::TAURI_IDENTIFIER);
        write(&tree.join("EBWebView").join("Default").join("Cache").join("data_0"), &[7; 32]);
        write(&tree.join("EBWebView").join("Local State"), b"{}");
        fs::create_dir_all(tree.join("vide").join("encore")).unwrap();
        // Les témoins : un voisin, et une application qui n'est pas la nôtre.
        write(&base.join("Autre").join("temoin.txt"), b"a quelqu'un d'autre");
        write(&base.join("fr.shinku.apple").join("temoin.txt"), b"nom voisin");
    }
}

fn witnesses_stay(pc: &Pc) {
    for base in [&pc.roaming, &pc.local] {
        assert_eq!(
            fs::read(base.join("Autre").join("temoin.txt")).unwrap(),
            b"a quelqu'un d'autre"
        );
        assert_eq!(
            fs::read(base.join("fr.shinku.apple").join("temoin.txt")).unwrap(),
            b"nom voisin"
        );
    }
}

#[test]
fn everything_of_shinku_goes_and_nothing_else() {
    let pc = Pc::new();
    seed(&pc);
    let host = Host::new(&pc);
    let outcome = forget::run(&host);
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(outcome.exit_code(), 0);
    for base in [&pc.roaming, &pc.local] {
        assert!(!base.join("Shinku").exists(), "le dossier vide est retiré");
        assert!(!base.join(forget::TAURI_IDENTIFIER).exists(), "le cache est parti");
    }
    witnesses_stay(&pc);
    assert!(host.elapsed() >= forget::MIN_LIFETIME);
}

#[test]
fn a_file_out_of_the_list_stays_with_its_folder_and_the_code_is_one() {
    let pc = Pc::new();
    seed(&pc);
    write(&pc.local.join("Shinku").join("mien.txt"), b"pas dans la liste");
    let outcome = forget::run(&Host::new(&pc));
    assert_eq!(outcome, Outcome::Left);
    assert_eq!(outcome.exit_code(), 1);
    // Ce qui est dans la liste est parti, ce qui n'y est pas est resté.
    assert_eq!(fs::read_dir(pc.local.join("Shinku")).unwrap().count(), 1);
    assert!(pc.local.join("Shinku").join("mien.txt").exists());
    assert!(!pc.roaming.join("Shinku").exists());
    assert!(!pc.local.join(forget::TAURI_IDENTIFIER).exists());
    witnesses_stay(&pc);
}

#[test]
fn a_file_with_the_name_of_a_setting_but_not_in_the_list_stays() {
    let pc = Pc::new();
    seed(&pc);
    write(&pc.roaming.join("Shinku").join("settings.json.bak"), b"sauvegarde de la personne");
    write(&pc.roaming.join("Shinku").join("Settings.json.old"), b"autre");
    assert_eq!(forget::run(&Host::new(&pc)), Outcome::Left);
    assert!(pc.roaming.join("Shinku").join("settings.json.bak").exists());
    assert!(pc.roaming.join("Shinku").join("Settings.json.old").exists());
    assert!(!pc.roaming.join("Shinku").join("settings.json").exists());
}

#[test]
fn nothing_to_do_is_code_zero() {
    let pc = Pc::new();
    let host = Host::new(&pc);
    assert_eq!(forget::run(&host), Outcome::Done);
    assert!(host.elapsed() >= forget::MIN_LIFETIME);
}

#[test]
fn an_elevated_process_touches_nothing_on_the_disk() {
    let pc = Pc::new();
    seed(&pc);
    let mut host = Host::new(&pc);
    host.elevated = true;
    assert_eq!(forget::run(&host), Outcome::Refused);
    assert!(pc.roaming.join("Shinku").join("settings.json").exists());
    assert!(pc.local.join("Shinku").join("game.json").exists());
    assert!(pc.roaming.join(forget::TAURI_IDENTIFIER).join("EBWebView").exists());
    assert!(pc.local.join(forget::TAURI_IDENTIFIER).join("vide").join("encore").exists());
    witnesses_stay(&pc);
    assert_eq!(host.elapsed(), forget::MIN_LIFETIME, "même refusé, il vit une seconde");
}

#[test]
fn a_link_in_place_of_a_cache_is_left_with_everything_behind_it() {
    let pc = Pc::new();
    seed(&pc);
    // Le cache de %LOCALAPPDATA% est remplacé par une jonction vers un dossier à elle.
    let behind = pc.profile.join("Documents").join("a-moi");
    write(&behind.join("precieux.doc"), b"ne pas toucher");
    let tree = pc.local.join(forget::TAURI_IDENTIFIER);
    fs::remove_dir_all(&tree).unwrap();
    if !link_dir(&behind, &tree) {
        eprintln!("SAUTÉ : liens de dossier indisponibles");
        return;
    }
    let outcome = forget::run(&Host::new(&pc));
    assert_eq!(outcome, Outcome::Left);
    assert_eq!(fs::read(behind.join("precieux.doc")).unwrap(), b"ne pas toucher");
    assert!(fs::symlink_metadata(&tree).is_ok(), "le lien reste en place");
    // Le reste de ce que Shinku avait laissé est parti.
    assert!(!pc.roaming.join(forget::TAURI_IDENTIFIER).exists());
    assert!(!pc.local.join("Shinku").exists());
}
