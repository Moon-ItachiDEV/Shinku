//! Le grand vide de bout en bout, sur un faux PC construit dans un dossier
//! temporaire.
//!
//! Chaque test pose quelques fichiers, lance une vraie analyse puis un vrai
//! grand vide, et regarde ce qu'il reste sur le disque. Rien n'est supprimé
//! hors du dossier temporaire : la corbeille elle-même y est un simple dossier.

// Dans un test, un `unwrap` qui échoue est un test qui échoue : c'est voulu.
#![allow(clippy::unwrap_used)]

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use shinku_core::clean::journal::{JournalStore, RunState};
use shinku_core::clean::report::{
    CleanOutcome, CleanReport, LeftOpen, SkippedFiles, StepState, StopReason,
};
use shinku_core::clean::{
    CleanControl, CleanPhase, CleanRefusal, CleanRequest, CleanStartError, check, preview, run,
    start,
};
use shinku_core::fs::to_display_path;
use shinku_core::model::Timestamp;
use shinku_core::platform::{
    Environment, KnownFolder, RecycleBin, RecycleError, Sent, StaticEnvironment, Volume,
};
use shinku_core::scan::report::{DuplicateFile, KeepChoice, Report};

mod common;
use common::{DAY, Pc, link_dir, link_file, listed, make_recent, noise, on_disk, pause, write};

/// Un PC de test avec une corbeille : un dossier à la racine du disque, là où
/// Windows range la sienne. L'analyse n'y entre jamais.
fn pc_with_bin() -> (Pc, PathBuf) {
    let mut pc = Pc::new();
    let bin = pc.c.join("$Recycle.Bin");
    fs::create_dir_all(&bin).unwrap();
    pc.environment = pc.environment.clone().with_recycle_folder(&bin);
    (pc, bin)
}

/// Ce que la corbeille de test contient, trié.
fn bin_content(bin: &Path) -> Vec<Vec<u8>> {
    let mut files: Vec<PathBuf> =
        fs::read_dir(bin).unwrap().map(|entry| entry.unwrap().path()).collect();
    files.sort();
    files.iter().map(|file| fs::read(file).unwrap()).collect()
}

/// Une demande qui ne coche rien, pour l'analyse de ce rapport.
fn nothing(report: &Report) -> CleanRequest {
    CleanRequest {
        analysis: report.started.unix_millis(),
        recycle_bin: false,
        junk_files: false,
        duplicates: false,
    }
}

fn junk_only(report: &Report) -> CleanRequest {
    CleanRequest { junk_files: true, ..nothing(report) }
}

fn duplicates_only(report: &Report) -> CleanRequest {
    CleanRequest { duplicates: true, ..nothing(report) }
}

fn everything(report: &Report) -> CleanRequest {
    CleanRequest { recycle_bin: true, junk_files: true, duplicates: true, ..nothing(report) }
}

/// Mène un grand vide jusqu'au bout.
fn clean(
    environment: &dyn Environment,
    report: &Report,
    request: CleanRequest,
) -> Arc<CleanReport> {
    run(environment, report, &request, &[], None, &CleanControl::new())
        .expect("le grand vide est accepté")
}

/// Trois exemplaires d'une même photo. Le premier rendu est le plus ancien :
/// c'est celui que Shinku garde.
fn three_photos(pc: &Pc) -> (PathBuf, PathBuf, PathBuf) {
    let content = noise(9000, 7);
    let kept = pc.documents().join("photo.jpg");
    let second = pc.pictures().join("photo.jpg");
    let third = pc.downloads().join("photo (1).jpg");
    write(&kept, &content);
    pause();
    write(&second, &content);
    pause();
    write(&third, &content);
    (kept, second, third)
}

/// Date un fichier d'après l'analyse, pour la machine de test.
fn touch_after_the_scan(path: &Path) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + 31 * DAY).unwrap();
}

/// Remplace le contenu d'un fichier sans changer ni sa taille ni sa date.
fn rewrite_keeping_the_date(path: &Path, content: &[u8]) {
    let modified = fs::metadata(path).unwrap().modified().unwrap();
    assert_eq!(fs::metadata(path).unwrap().len(), content.len() as u64);
    fs::File::options().write(true).open(path).unwrap().write_all(content).unwrap();
    fs::File::options().write(true).open(path).unwrap().set_modified(modified).unwrap();
}

/// Une machine dont la corbeille se comporte comme le test le décide. Tout le
/// reste est le PC de test.
struct Rigged<F> {
    pc: StaticEnvironment,
    recycle: F,
    /// Si elle est fixée, la corbeille annonce toujours cette taille.
    frozen_bin: Option<RecycleBin>,
    /// Appelé chaque fois que le moteur demande quels programmes tournent.
    on_process_list: Option<Box<dyn Fn() + Send + Sync>>,
    /// Ce que la machine annonce après l'envoi (Windows peut dire « détruit »
    /// alors que le disque montre l'élément rangé).
    answer: Sent,
}

impl<F> fmt::Debug for Rigged<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rigged").field("pc", &self.pc).finish_non_exhaustive()
    }
}

impl<F> Environment for Rigged<F>
where
    F: Fn(&StaticEnvironment, &Path) -> Result<(), RecycleError> + Send + Sync,
{
    fn now(&self) -> Timestamp {
        self.pc.now()
    }

    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        self.pc.known_folder(folder)
    }

    fn volumes(&self) -> Vec<Volume> {
        self.pc.volumes()
    }

    fn sync_roots(&self) -> Vec<PathBuf> {
        self.pc.sync_roots()
    }

    fn recycle_bin(&self) -> Option<RecycleBin> {
        self.frozen_bin.or_else(|| self.pc.recycle_bin())
    }

    fn parallelism(&self) -> usize {
        self.pc.parallelism()
    }

    fn running_processes(&self) -> Option<Vec<String>> {
        if let Some(hook) = &self.on_process_list {
            hook();
        }
        self.pc.running_processes()
    }

    fn empty_recycle_bin(&self) -> std::io::Result<()> {
        self.pc.empty_recycle_bin()
    }

    fn recycle(&self, path: &Path) -> Result<Sent, RecycleError> {
        (self.recycle)(&self.pc, path).map(|()| self.answer)
    }
}

fn rigged<F>(pc: &Pc, recycle: F) -> Rigged<F>
where
    F: Fn(&StaticEnvironment, &Path) -> Result<(), RecycleError> + Send + Sync,
{
    Rigged {
        pc: pc.environment.clone(),
        recycle,
        frozen_bin: None,
        on_process_list: None,
        answer: Sent::InBin,
    }
}

// ── Fichiers inutiles ──────────────────────────────────────────────────

#[test]
fn junk_files_are_removed_for_good_and_nothing_else_is() {
    let pc = Pc::new();
    let chrome = pc.local.join("Google").join("Chrome").join("User Data").join("Default");
    let cache = chrome.join("Cache").join("Cache_Data");
    write(&cache.join("f_000001"), &noise(9000, 1));
    write(&cache.join("f_000002"), &noise(12_000, 2));
    // À côté du cache : des données du profil, qu'aucune règle ne désigne.
    write(&chrome.join("History"), &noise(20_000, 3));
    let temp = pc.temp();
    write(&temp.join("vieux.tmp"), &noise(5000, 4));
    write(&temp.join("récent.tmp"), &noise(5000, 5));
    make_recent(&temp.join("récent.tmp"));
    // Un dossier où rien n'a bougé depuis longtemps : il part en entier.
    write(&temp.join("ancien").join("x").join("y.bin"), &noise(5000, 6));
    write(&temp.join("ancien").join("z.bin"), &noise(5000, 7));
    fs::create_dir_all(temp.join("ancien").join("vide")).unwrap();
    fs::create_dir_all(temp.join("vieux dossier vide")).unwrap();
    // Le dossier d'un installeur en cours : un fichier récent le garde entier,
    // jusqu'à ses sous-dossiers vides.
    write(&temp.join("installeur").join("a.bin"), &noise(5000, 8));
    write(&temp.join("installeur").join("b.bin"), &noise(5000, 9));
    make_recent(&temp.join("installeur").join("b.bin"));
    fs::create_dir_all(temp.join("installeur").join("journaux")).unwrap();
    // Un dossier vide dans un cache : l'app compte peut-être dessus.
    fs::create_dir_all(chrome.join("Cache").join("index-dir")).unwrap();
    // Des fichiers personnels, dont deux identiques.
    write(&pc.documents().join("lettre.txt"), &noise(9000, 10));
    write(&pc.pictures().join("lettre.txt"), &noise(9000, 10));

    let report = pc.scan();
    assert_eq!(report.junk.files, 5);
    let seen = report.junk.bytes;
    let bilan = clean(&pc.environment, &report, junk_only(&report));

    assert_eq!(bilan.outcome, CleanOutcome::Completed);
    assert_eq!(bilan.junk.state, StepState::Done);
    assert_eq!((bilan.junk.files, bilan.junk.bytes), (5, seen), "ce que l'analyse a montré");
    assert_eq!((bilan.freed_bytes, bilan.recoverable_bytes), (seen, 0));
    assert_eq!(bilan.skipped, SkippedFiles::default());
    assert!(bilan.junk.left_open.is_empty());
    assert_eq!(bilan.recycle_bin.state, StepState::NotRequested);
    assert_eq!(bilan.duplicates.state, StepState::NotRequested);

    // Les fichiers du cache sont partis, son dossier reste pour l'app.
    assert!(!cache.join("f_000001").exists() && !cache.join("f_000002").exists());
    assert!(cache.is_dir());
    assert!(chrome.join("History").exists());
    // Dans %TEMP% : le vieux fichier et le vieux dossier sont partis, le
    // reste est intact, et %TEMP% lui-même est toujours là.
    assert!(!temp.join("vieux.tmp").exists());
    assert!(!temp.join("ancien").exists(), "le dossier part avec son contenu");
    assert!(!temp.join("vieux dossier vide").exists());
    assert!(temp.join("récent.tmp").exists());
    assert!(temp.join("installeur").join("a.bin").exists());
    assert!(temp.join("installeur").join("b.bin").exists());
    assert!(temp.join("installeur").join("journaux").is_dir(), "gardé en entier");
    assert!(chrome.join("Cache").join("index-dir").is_dir(), "seuls les fichiers partent");
    assert!(temp.is_dir());
    // Les fichiers personnels n'ont pas bougé : les doublons n'étaient pas cochés.
    assert!(pc.documents().join("lettre.txt").exists());
    assert!(pc.pictures().join("lettre.txt").exists());

    // Une nouvelle analyse ne trouve plus rien à nettoyer.
    assert_eq!(pc.scan().junk.files, 0);
}

#[test]
fn an_open_app_keeps_its_cache() {
    let mut pc = Pc::new();
    let chrome = pc.local.join("Google").join("Chrome").join("User Data").join("Default");
    let cached = chrome.join("Cache").join("Cache_Data").join("f_000001");
    let code = chrome.join("Code Cache").join("js").join("index");
    let discord = pc.roaming.join("discord").join("Cache").join("data_0");
    let temp = pc.temp().join("vieux.tmp");
    for (seed, file) in [&cached, &code, &discord, &temp].into_iter().enumerate() {
        write(file, &noise(9000, seed as u64));
    }
    let chrome_bytes = on_disk(&cached) + on_disk(&code);
    pc.environment = pc.environment.clone().with_process("explorer.exe").with_process("CHROME.EXE");

    let report = pc.scan();
    // L'écran de confirmation prévient avant de commencer.
    let before = preview(&pc.environment, Some(&report)).unwrap();
    let chrome_left = LeftOpen { label: "Google Chrome", bytes: chrome_bytes };
    assert_eq!(before.junk.open_apps, std::slice::from_ref(&chrome_left));

    let bilan = clean(&pc.environment, &report, junk_only(&report));
    assert_eq!(bilan.junk.state, StepState::Done);
    assert_eq!(bilan.junk.left_open, [chrome_left]);
    assert_eq!(bilan.junk.files, 2, "Discord et le fichier temporaire");
    assert_eq!(bilan.junk.bytes, report.junk.bytes - chrome_bytes);
    assert!(cached.exists() && code.exists(), "Chrome est ouvert : son cache reste");
    assert!(!discord.exists() && !temp.exists());
    // Ce n'est pas un échec : ces fichiers ne comptent pas parmi les ignorés.
    assert_eq!(bilan.skipped.total(), 0);
}

#[test]
fn a_machine_that_cannot_list_programs_treats_every_app_as_open() {
    let mut pc = Pc::new();
    let cached = pc.local.join("Google/Chrome/User Data/Default/Cache/f_000001");
    let temp = pc.temp().join("vieux.tmp");
    write(&cached, &noise(9000, 1));
    write(&temp, &noise(9000, 2));
    pc.environment = pc.environment.clone().without_process_list();

    let report = pc.scan();
    let bilan = clean(&pc.environment, &report, junk_only(&report));
    assert!(cached.exists(), "dans le doute, le cache d'une app reste");
    assert!(!temp.exists(), "ce qui ne dépend d'aucune app part");
    assert_eq!(bilan.junk.left_open.len(), 1);
    assert_eq!(bilan.junk.files, 1);
}

#[test]
fn what_changed_since_the_analysis_is_judged_as_it_is_now() {
    let pc = Pc::new();
    let cache = pc.local.join("Google/Chrome/User Data/Default/Cache");
    let (touched, untouched) = (cache.join("f_000001"), cache.join("f_000002"));
    write(&touched, &noise(9000, 1));
    write(&untouched, &noise(9000, 2));
    let temp = pc.temp();
    write(&temp.join("ancien").join("a.bin"), &noise(5000, 3));
    write(&temp.join("autre").join("b.bin"), &noise(5000, 4));

    let report = pc.scan();
    assert_eq!(report.junk.files, 4);

    // Entre l'analyse et le grand vide : Chrome réécrit un fichier de son
    // cache, un programme dépose un fichier dans un vieux dossier
    // temporaire, et un fichier tout neuf arrive dans le cache.
    fs::write(&touched, noise(9000, 5)).unwrap();
    touch_after_the_scan(&touched);
    write(&temp.join("ancien").join("nouveau.bin"), &noise(5000, 6));
    touch_after_the_scan(&temp.join("ancien").join("nouveau.bin"));
    write(&cache.join("f_000003"), &noise(9000, 7));
    touch_after_the_scan(&cache.join("f_000003"));

    let bilan = clean(&pc.environment, &report, junk_only(&report));
    assert_eq!(bilan.junk.state, StepState::Done);
    assert_eq!(bilan.junk.files, 2, "f_000002 et autre/b.bin");
    assert!(touched.exists(), "réécrit depuis l'analyse : il reste");
    assert!(cache.join("f_000003").exists(), "arrivé depuis l'analyse : il reste");
    assert!(!untouched.exists());
    // Le vieux dossier a reçu un fichier : il reste en entier.
    assert!(temp.join("ancien").join("a.bin").exists());
    assert!(temp.join("ancien").join("nouveau.bin").exists());
    assert!(!temp.join("autre").exists());
}

#[test]
fn a_link_in_a_cache_is_removed_without_touching_its_target() {
    let pc = Pc::new();
    let cache = pc.local.join("Google/Chrome/User Data/Default/Cache");
    write(&cache.join("f_000001"), &noise(9000, 1));
    let diary = pc.documents().join("journal intime.txt");
    write(&diary, b"ne pas perdre");
    let album = pc.pictures().join("album");
    write(&album.join("photo.jpg"), &noise(9000, 2));
    // Quelqu'un a planté dans le cache un lien vers un fichier personnel, et
    // un autre vers un dossier de photos.
    let file_link = link_file(&diary, &cache.join("f_000002"));
    let dir_link = link_dir(&album, &cache.join("index-dir"));
    if !file_link && !dir_link {
        eprintln!("cette machine ne permet pas de créer de lien : test incomplet");
    }

    let report = pc.scan();
    let bilan = clean(&pc.environment, &report, junk_only(&report));
    assert_eq!(bilan.junk.state, StepState::Done);
    assert!(!cache.join("f_000001").exists());
    // Les liens sont partis, en tant que liens.
    assert!(listed(&cache.join("f_000002")).is_none());
    assert!(listed(&cache.join("index-dir")).is_none());
    // Ce qu'ils désignaient n'a pas bougé.
    assert_eq!(fs::read(&diary).unwrap(), b"ne pas perdre");
    assert!(album.join("photo.jpg").exists());
}

#[test]
fn a_folder_never_to_touch_protects_what_surrounds_it_in_temp() {
    let pc = Pc::new();
    let temp = pc.temp();
    let kept = temp.join("travail").join("à garder");
    write(&kept.join("important.bin"), &noise(5000, 1));
    write(&temp.join("travail").join("reste.bin"), &noise(5000, 2));
    write(&temp.join("ancien").join("a.bin"), &noise(5000, 3));

    let excluded = vec![to_display_path(&kept)];
    let report = pc.scan_with(|options| options.excluded = excluded.clone());
    assert_eq!(report.junk.files, 1, "seulement ancien/a.bin");
    let request = junk_only(&report);
    let bilan =
        run(&pc.environment, &report, &request, &excluded, None, &CleanControl::new()).unwrap();
    assert_eq!(bilan.junk.files, 1);
    assert!(!temp.join("ancien").exists());
    // Le dossier exclu, et celui qui y mène, restent tels quels.
    assert!(kept.join("important.bin").exists());
    assert!(temp.join("travail").join("reste.bin").exists());
}

#[test]
fn an_exclusion_that_cannot_be_applied_stops_everything_but_the_bin() {
    let (mut pc, _bin) = pc_with_bin();
    pc.environment = pc.environment.clone().with_recycle_bin(RecycleBin { bytes: 4096, items: 2 });
    let temp = pc.temp().join("vieux.tmp");
    write(&temp, &noise(5000, 1));
    let (kept, second, third) = three_photos(&pc);

    let report = pc.scan();
    // Les réglages contiennent maintenant un « dossier jamais touché » que
    // Shinku ne sait pas situer : il ne protège rien, donc rien ne part.
    let excluded = vec![PathBuf::from("Documents/Privé")];
    let request = everything(&report);
    let bilan =
        run(&pc.environment, &report, &request, &excluded, None, &CleanControl::new()).unwrap();
    assert_eq!(bilan.junk.state, StepState::Failed);
    assert_eq!(bilan.duplicates.state, StepState::Failed);
    assert_eq!((bilan.junk.files, bilan.duplicates.copies), (0, 0));
    assert!(temp.exists() && kept.exists() && second.exists() && third.exists());
    // La corbeille ne dépend d'aucun dossier : elle a été vidée.
    assert_eq!(bilan.recycle_bin.state, StepState::Done);
    assert_eq!(bilan.freed_bytes, 4096);
    assert_eq!(bilan.outcome, CleanOutcome::Completed);
}

#[cfg(windows)]
#[test]
fn a_file_held_by_a_program_is_left_and_keeps_its_whole_folder() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;

    let pc = Pc::new();
    let cache = pc.local.join("Google").join("Chrome").join("User Data").join("Default");
    let (free, held) = (cache.join("Cache").join("f_1"), cache.join("Cache").join("f_2"));
    write(&free, &noise(9000, 1));
    write(&held, &noise(9000, 2));
    let temp = pc.temp();
    let running = temp.join("installeur").join("setup.exe");
    write(&running, &noise(9000, 3));
    write(&temp.join("installeur").join("données").join("a.bin"), &noise(9000, 4));
    write(&temp.join("ancien").join("b.bin"), &noise(9000, 5));

    let report = pc.scan();
    assert_eq!(report.junk.files, 5);
    // Deux programmes tiennent un fichier sans permettre qu'on le supprime.
    let hold = |path: &Path| {
        fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(path)
            .unwrap()
    };
    let holders = (hold(&held), hold(&running));

    let bilan = clean(&pc.environment, &report, junk_only(&report));
    assert_eq!(bilan.junk.state, StepState::Done);
    assert_eq!(bilan.junk.files, 2, "f_1 et ancien/b.bin");
    // Le fichier tenu reste, et le dossier de l'installeur reste en entier :
    // lui retirer le reste casserait le programme qui tourne.
    assert_eq!(bilan.skipped.in_use, 3);
    assert_eq!(bilan.skipped.total(), 3);
    assert!(!free.exists() && held.exists());
    assert!(running.exists());
    assert!(temp.join("installeur").join("données").join("a.bin").exists());
    assert!(!temp.join("ancien").exists());
    drop(holders);
}

// ── Doublons ───────────────────────────────────────────────────────────

#[test]
fn copies_go_to_the_recycle_bin_and_the_oldest_stays() {
    let (pc, bin) = pc_with_bin();
    let (kept, second, third) = three_photos(&pc);
    let content = fs::read(&kept).unwrap();
    let copies_bytes = on_disk(&second) + on_disk(&third);
    // Un fichier inutile : il n'est pas coché, il ne part pas.
    let temp = pc.temp().join("vieux.tmp");
    write(&temp, &noise(5000, 1));

    let report = pc.scan();
    assert_eq!(report.duplicates.copies, 2);
    assert!(report.duplicates.groups[0].files[0].keep);
    assert_eq!(report.duplicates.groups[0].files[0].path, kept);

    let before = preview(&pc.environment, Some(&report)).unwrap();
    assert_eq!(before.analysis, report.started.unix_millis());
    assert_eq!((before.duplicates.copies, before.duplicates.bytes), (2, copies_bytes));
    assert_eq!((before.duplicates.synced_copies, before.duplicates.other_disk_copies), (0, 0));

    let bilan = clean(&pc.environment, &report, duplicates_only(&report));
    assert_eq!(bilan.outcome, CleanOutcome::Completed);
    assert_eq!(bilan.duplicates.state, StepState::Done);
    assert_eq!((bilan.duplicates.copies, bilan.duplicates.bytes), (2, copies_bytes));
    assert_eq!(bilan.duplicates.stop, None);
    // Rien n'est « libéré » : les copies attendent dans la corbeille.
    assert_eq!((bilan.freed_bytes, bilan.recoverable_bytes), (0, copies_bytes));
    assert_eq!(bilan.skipped.total(), 0);
    assert_eq!(bilan.junk.state, StepState::NotRequested);

    assert_eq!(fs::read(&kept).unwrap(), content, "l'exemplaire gardé est intact");
    assert!(!second.exists() && !third.exists());
    assert_eq!(bin_content(&bin), [content.clone(), content], "les copies sont récupérables");
    assert!(temp.exists());

    assert!(pc.scan().duplicates.groups.is_empty(), "il ne reste qu'un exemplaire");
}

#[test]
fn the_copy_the_person_chose_is_the_one_that_stays() {
    let (pc, bin) = pc_with_bin();
    let (oldest, second, third) = three_photos(&pc);
    let content = fs::read(&oldest).unwrap();
    let report = pc.scan();
    let group = &report.duplicates.groups[0];
    let wanted = group.files.iter().position(|file| file.path == third).unwrap() as u32;
    let choice = |index, whole_folder| KeepChoice {
        digest: group.digest.clone(),
        size: group.size,
        index,
        whole_folder,
    };

    // Un choix qui ne désigne rien ne change rien.
    assert!(report.keeping(&choice(99, false)).is_none());
    let stranger = KeepChoice { digest: "0".repeat(64), ..choice(wanted, false) };
    assert!(report.keeping(&stranger).is_none());

    let chosen = report.keeping(&choice(wanted, false)).unwrap();
    let kept: Vec<&Path> = chosen.duplicates.groups[0]
        .files
        .iter()
        .filter(|file| file.keep)
        .map(|file| file.path.as_path())
        .collect();
    assert_eq!(kept, [third.as_path()], "un exemplaire gardé, et un seul");
    assert_eq!(chosen.duplicates.copies, 2);
    assert_eq!(chosen.duplicates.bytes, on_disk(&oldest) + on_disk(&second));
    assert_eq!(chosen.started, report.started, "c'est toujours la même analyse");

    let bilan = clean(&pc.environment, &chosen, duplicates_only(&chosen));
    assert_eq!((bilan.duplicates.state, bilan.duplicates.copies), (StepState::Done, 2));
    assert_eq!(fs::read(&third).unwrap(), content, "l'exemplaire choisi est intact");
    assert!(!oldest.exists() && !second.exists());
    assert_eq!(bin_content(&bin), [content.clone(), content], "les autres sont récupérables");
}

#[test]
fn keeping_a_folder_keeps_its_copy_in_every_group() {
    let (pc, _bin) = pc_with_bin();
    let (sorted, unsorted) =
        (pc.profile.join("Desktop").join("à garder"), pc.profile.join("Desktop").join("vrac"));
    for (name, seed) in [("film.mov", 1), ("clip.mov", 2), ("photo.jpg", 3)] {
        // Le vrac d'abord : c'est lui le plus ancien, donc gardé d'office.
        write(&unsorted.join(name), &noise(6000, seed));
        write(&sorted.join(name), &noise(6000, seed));
    }
    // Un doublon sans exemplaire dans le dossier choisi : il ne bouge pas.
    write(&unsorted.join("seul.bin"), &noise(7000, 9));
    write(&pc.profile.join("Documents").join("seul.bin"), &noise(7000, 9));

    let report = pc.scan();
    assert_eq!(report.duplicates.groups.len(), 4);
    let (rank, index) = report
        .duplicates
        .groups
        .iter()
        .enumerate()
        .find_map(|(rank, group)| {
            let index = group.files.iter().position(|file| file.path.starts_with(&sorted))?;
            Some((rank, index))
        })
        .unwrap();
    let group = &report.duplicates.groups[rank];
    let chosen = report
        .keeping(&KeepChoice {
            digest: group.digest.clone(),
            size: group.size,
            index: index as u32,
            whole_folder: true,
        })
        .unwrap();

    for (before, after) in report.duplicates.groups.iter().zip(&chosen.duplicates.groups) {
        let kept: Vec<&DuplicateFile> = after.files.iter().filter(|file| file.keep).collect();
        assert_eq!(kept.len(), 1, "un exemplaire gardé, et un seul");
        if after.files.iter().any(|file| file.path.starts_with(&sorted)) {
            assert!(kept[0].path.starts_with(&sorted), "{:?}", kept[0].path);
        } else {
            let unchanged = before.files.iter().position(|file| file.keep);
            assert_eq!(after.files.iter().position(|file| file.keep), unchanged);
        }
    }
}

#[test]
fn keeping_a_folder_leaves_a_copy_already_chosen_there() {
    let (pc, _bin) = pc_with_bin();
    let (sorted, unsorted) =
        (pc.profile.join("Desktop").join("à garder"), pc.profile.join("Desktop").join("vrac"));
    // Deux exemplaires dans le dossier trié, un troisième dans le vrac.
    write(&unsorted.join("photo.jpg"), &noise(6000, 1));
    write(&sorted.join("photo.jpg"), &noise(6000, 1));
    write(&sorted.join("photo (1).jpg"), &noise(6000, 1));
    write(&unsorted.join("film.mov"), &noise(8000, 2));
    write(&sorted.join("film.mov"), &noise(8000, 2));

    let report = pc.scan();
    let find = |report: &Report, name: &str| {
        let wanted = sorted.join(name);
        report
            .duplicates
            .groups
            .iter()
            .find_map(|group| {
                let index = group.files.iter().position(|file| file.path == wanted)?;
                Some((group.digest.clone(), group.size, index as u32))
            })
            .unwrap()
    };
    let kept = |report: &Report| -> Vec<PathBuf> {
        let files = report.duplicates.groups.iter().flat_map(|group| &group.files);
        let mut kept: Vec<PathBuf> =
            files.filter(|file| file.keep).map(|file| file.path.clone()).collect();
        kept.sort();
        kept
    };

    // La personne choisit « photo (1).jpg » à la main, puis garde tout le dossier trié.
    let (digest, size, index) = find(&report, "photo (1).jpg");
    let by_hand = report.keeping(&KeepChoice { digest, size, index, whole_folder: false }).unwrap();
    let (digest, size, index) = find(&by_hand, "film.mov");
    let folder = by_hand.keeping(&KeepChoice { digest, size, index, whole_folder: true }).unwrap();

    let mut expected = vec![sorted.join("film.mov"), sorted.join("photo (1).jpg")];
    expected.sort();
    assert_eq!(
        kept(&folder),
        expected,
        "le choix fait à la main n'est pas remplacé par son voisin"
    );
}

#[test]
fn the_bin_is_emptied_first_so_the_copies_stay_recoverable() {
    let (mut pc, bin) = pc_with_bin();
    pc.environment =
        pc.environment.clone().with_recycle_bin(RecycleBin { bytes: 70_000, items: 4 });
    let (kept, second, third) = three_photos(&pc);
    let content = fs::read(&kept).unwrap();
    let temp = pc.temp().join("vieux.tmp");
    write(&temp, &noise(5000, 1));

    let report = pc.scan();
    let (junk_bytes, copies_bytes) = (report.junk.bytes, report.duplicates.bytes);
    let before = preview(&pc.environment, Some(&report)).unwrap();
    assert_eq!(before.recycle_bin, Some(RecycleBin { bytes: 70_000, items: 4 }));

    let control = CleanControl::new();
    let request = everything(&report);
    let bilan = run(&pc.environment, &report, &request, &[], None, &control).unwrap();
    assert_eq!(bilan.outcome, CleanOutcome::Completed);
    assert_eq!(bilan.recycle_bin.state, StepState::Done);
    assert_eq!((bilan.recycle_bin.bytes, bilan.recycle_bin.items), (70_000, 4));
    assert_eq!(bilan.junk.files, 1);
    assert_eq!(bilan.duplicates.copies, 2);
    // Les deux chiffres ne se mélangent pas.
    assert_eq!(bilan.freed_bytes, 70_000 + junk_bytes);
    assert_eq!(bilan.recoverable_bytes, copies_bytes);

    assert!(!temp.exists() && !second.exists() && !third.exists() && kept.exists());
    // La corbeille ne contient plus que les deux copies.
    assert_eq!(bin_content(&bin), [content.clone(), content]);
    assert_eq!(pc.environment.recycle_bin(), Some(RecycleBin { bytes: 18_000, items: 2 }));

    // Le contrôle porte le bilan et l'avancement final.
    assert_eq!(control.report().as_deref(), Some(&*bilan));
    let progress = control.progress();
    assert_eq!(progress.phase, CleanPhase::Done);
    assert_eq!(progress.freed_bytes, 70_000 + junk_bytes);
    assert_eq!(progress.recycled_bytes, copies_bytes);
    assert_eq!((progress.files, progress.skipped_files), (3, 0));
}

#[test]
fn a_copy_modified_since_the_analysis_stays() {
    let (pc, bin) = pc_with_bin();
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();

    // La copie des Images est retouchée après l'analyse.
    fs::write(&second, noise(9000, 99)).unwrap();
    touch_after_the_scan(&second);

    let bilan = clean(&pc.environment, &report, duplicates_only(&report));
    assert_eq!(bilan.duplicates.state, StepState::Done);
    assert_eq!(bilan.duplicates.copies, 1);
    assert_eq!(bilan.skipped.changed, 1);
    assert_eq!(bilan.skipped.total(), 1);
    assert!(kept.exists() && second.exists() && !third.exists());
    assert_eq!(bin_content(&bin).len(), 1);
}

#[test]
fn the_same_date_and_size_are_not_enough_the_content_is_compared() {
    let (pc, bin) = pc_with_bin();
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();
    assert_eq!(report.duplicates.copies, 2);

    // Un outil retouche une copie en gardant sa taille et sa date : rien,
    // dans la liste du dossier, ne la distingue de ce que l'analyse a vu.
    let mut retouched = fs::read(&second).unwrap();
    retouched[4500] ^= 0xFF;
    rewrite_keeping_the_date(&second, &retouched);

    let bilan = clean(&pc.environment, &report, duplicates_only(&report));
    assert_eq!(bilan.duplicates.copies, 1, "seule la vraie copie part");
    assert_eq!(bilan.skipped.changed, 1);
    assert_eq!(fs::read(&second).unwrap(), retouched, "la photo retouchée est restée");
    assert!(kept.exists() && !third.exists());
    assert_eq!(bin_content(&bin).len(), 1);
}

#[test]
fn when_the_kept_copy_is_gone_or_changed_nothing_leaves() {
    for damage in ["supprimé", "retouché", "retouché sans changer de date"] {
        let (pc, bin) = pc_with_bin();
        let (kept, second, third) = three_photos(&pc);
        let report = pc.scan();
        match damage {
            "supprimé" => fs::remove_file(&kept).unwrap(),
            "retouché" => {
                fs::write(&kept, noise(9000, 99)).unwrap();
                touch_after_the_scan(&kept);
            }
            _ => rewrite_keeping_the_date(&kept, &noise(9000, 99)),
        }

        let bilan = clean(&pc.environment, &report, duplicates_only(&report));
        assert_eq!(bilan.duplicates.state, StepState::Done, "{damage}");
        assert_eq!(bilan.duplicates.copies, 0, "{damage}");
        assert_eq!(bilan.skipped.changed, 2, "{damage}");
        assert!(second.exists() && third.exists(), "{damage} : les copies restent");
        assert!(bin_content(&bin).is_empty(), "{damage}");
        assert_eq!(bilan.recoverable_bytes, 0, "{damage}");
    }
}

#[test]
fn a_copy_the_bin_cannot_take_stays_in_place() {
    // Une corbeille trop petite pour la photo, puis pas de corbeille du tout.
    let (mut small, bin) = pc_with_bin();
    small.environment = small.environment.clone().with_recycle_limit(1000);
    let none = Pc::new();
    for (pc, bin) in [(small, Some(bin)), (none, None)] {
        let (kept, second, third) = three_photos(&pc);
        let report = pc.scan();
        let bilan = clean(&pc.environment, &report, duplicates_only(&report));
        // Shinku ne supprime jamais définitivement à la place.
        assert_eq!(bilan.duplicates.state, StepState::Done);
        assert_eq!(bilan.duplicates.copies, 0);
        assert_eq!(bilan.skipped.unrecyclable, 2);
        assert_eq!(bilan.skipped.total(), 2);
        assert!(kept.exists() && second.exists() && third.exists());
        if let Some(bin) = bin {
            assert!(bin_content(&bin).is_empty());
        }
    }
}

#[test]
fn a_bin_that_deletes_for_good_stops_the_step_at_once() {
    let (pc, _bin) = pc_with_bin();
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();
    // Une « corbeille » qui supprime pour de bon en répondant que tout va bien.
    let environment = rigged(&pc, |_, path| {
        fs::remove_file(path).unwrap();
        Ok(())
    });

    let bilan = clean(&environment, &report, duplicates_only(&report));
    assert_eq!(bilan.duplicates.state, StepState::Stopped);
    let stop = bilan.duplicates.stop.as_ref().expect("un arrêt de sécurité");
    assert_eq!(stop.reason, StopReason::NotInRecycleBin);
    // Un seul fichier a été perdu : l'étape s'est arrêtée dessus, et le dit.
    let lost = [&second, &third].into_iter().filter(|copy| !copy.exists()).count();
    assert_eq!(lost, 1);
    assert!(!stop.file.exists());
    assert!(kept.exists());
    assert_eq!((bilan.duplicates.copies, bilan.recoverable_bytes), (0, 0));
    // Ce n'est pas l'utilisateur qui a arrêté.
    assert_eq!(bilan.outcome, CleanOutcome::Completed);
}

#[test]
fn a_copy_that_windows_says_it_destroyed_is_never_counted_even_if_the_disk_shows_it_moved() {
    let (pc, bin) = pc_with_bin();
    let (kept, _second, _third) = three_photos(&pc);
    let report = pc.scan();
    // Le disque dit « rangée » (la copie a quitté sa place et existe), la machine
    // dit « détruite » : c'est la machine qui a raison sur ce qu'elle a fait.
    let mut environment = rigged(&pc, |pc, path| pc.recycle(path).map(|_| ()));
    environment.answer = Sent::Destroyed;

    let bilan = clean(&environment, &report, duplicates_only(&report));
    assert_eq!(bilan.duplicates.state, StepState::Stopped);
    let stop = bilan.duplicates.stop.as_ref().expect("un arrêt de sécurité");
    assert_eq!(stop.reason, StopReason::NotInRecycleBin);
    assert_eq!((bilan.duplicates.copies, bilan.duplicates.bytes), (0, 0));
    assert_eq!(bilan.recoverable_bytes, 0);
    assert_eq!(bin_content(&bin).len(), 1, "le disque, lui, l'a rangée");
    assert!(kept.exists());
}

#[test]
fn a_copy_that_windows_destroyed_is_never_counted_as_sent_to_the_bin() {
    // Windows supprime pour de bon (corbeille trop petite que la garde n'a pas
    // vue) et le dit : l'étape s'arrête, sans un octet « à la corbeille ».
    let (pc, bin) = pc_with_bin();
    let (kept, _second, _third) = three_photos(&pc);
    let report = pc.scan();
    let environment = pc.environment.clone().with_recycle_destroying();

    let bilan = clean(&environment, &report, duplicates_only(&report));
    assert_eq!(bilan.duplicates.state, StepState::Stopped);
    let stop = bilan.duplicates.stop.as_ref().expect("un arrêt de sécurité");
    assert_eq!(stop.reason, StopReason::NotInRecycleBin);
    assert_eq!((bilan.duplicates.copies, bilan.duplicates.bytes), (0, 0));
    assert_eq!(bilan.recoverable_bytes, 0);
    assert!(bin_content(&bin).is_empty());
    assert!(kept.exists());
}

#[test]
fn a_kept_copy_touched_while_a_copy_leaves_stops_the_step() {
    let (pc, bin) = pc_with_bin();
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();
    // Pendant que la copie part à la corbeille, un programme écrit dans
    // l'exemplaire gardé.
    let target = kept.clone();
    let environment = rigged(&pc, move |pc, path| {
        pc.recycle(path)?;
        fs::File::options().append(true).open(&target).unwrap().write_all(b"!").unwrap();
        Ok(())
    });

    let bilan = clean(&environment, &report, duplicates_only(&report));
    assert_eq!(bilan.duplicates.state, StepState::Stopped);
    let stop = bilan.duplicates.stop.as_ref().expect("un arrêt de sécurité");
    assert_eq!(stop.reason, StopReason::KeptCopyChanged);
    // La copie partie est bien dans la corbeille, et comptée : elle se restaure.
    assert_eq!(bilan.duplicates.copies, 1);
    assert_eq!(bin_content(&bin).len(), 1);
    assert_eq!(bilan.recoverable_bytes, bilan.duplicates.bytes);
    // L'autre copie n'a pas été touchée.
    let left = [&second, &third].into_iter().filter(|copy| copy.exists()).count();
    assert_eq!(left, 1);
    assert!(kept.exists());
}

#[test]
fn the_step_looks_at_the_disk_not_at_what_the_bin_answers() {
    // Une corbeille qui répond « fait » sans rien faire.
    let (pc, bin) = pc_with_bin();
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();
    let bilan = clean(&rigged(&pc, |_, _| Ok(())), &report, duplicates_only(&report));
    assert_eq!(bilan.duplicates.state, StepState::Done);
    assert_eq!((bilan.duplicates.copies, bilan.skipped.other), (0, 2));
    assert!(kept.exists() && second.exists() && third.exists());
    assert!(bin_content(&bin).is_empty());

    // Une corbeille qui prend le fichier et répond qu'elle a échoué.
    let (pc, bin) = pc_with_bin();
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();
    let environment = rigged(&pc, |pc, path| {
        pc.recycle(path)?;
        Err(RecycleError::Failed("réponse trompeuse".to_owned()))
    });
    let bilan = clean(&environment, &report, duplicates_only(&report));
    assert_eq!((bilan.duplicates.copies, bilan.skipped.total()), (2, 0));
    assert!(kept.exists() && !second.exists() && !third.exists());
    assert_eq!(bin_content(&bin).len(), 2);
    assert_eq!(bilan.recoverable_bytes, bilan.duplicates.bytes);
}

#[test]
fn a_full_bin_that_drops_what_it_receives_is_not_called_recoverable() {
    let (pc, _bin) = pc_with_bin();
    let (_kept, second, third) = three_photos(&pc);
    let report = pc.scan();
    // La corbeille reçoit les copies, mais sa taille ne bouge pas : pleine,
    // elle abandonne au fur et à mesure.
    let mut environment = rigged(&pc, |pc, path| pc.recycle(path).map(|_| ()));
    environment.frozen_bin = Some(RecycleBin { bytes: 1_000_000, items: 50 });

    let bilan = clean(&environment, &report, duplicates_only(&report));
    assert_eq!(bilan.duplicates.copies, 2);
    assert!(bilan.duplicates.bytes > 0);
    assert_eq!(bilan.recoverable_bytes, 0, "l'interface doit pouvoir le dire");
    assert!(!second.exists() && !third.exists());
}

#[test]
fn a_folder_never_to_touch_keeps_its_copies_even_if_the_report_lists_them() {
    let (pc, bin) = pc_with_bin();
    let content = noise(9000, 7);
    let kept = pc.documents().join("photo.jpg");
    let private = pc.documents().join("Privé").join("photo.jpg");
    let other = pc.pictures().join("photo.jpg");
    write(&kept, &content);
    pause();
    write(&private, &content);
    pause();
    write(&other, &content);

    // L'analyse a été faite sans l'exclusion : le rapport liste la copie.
    let report = pc.scan();
    assert_eq!(report.duplicates.copies, 2);
    // Au moment de nettoyer, le dossier est « jamais touché ».
    let excluded = vec![to_display_path(&pc.documents().join("Privé"))];
    let request = duplicates_only(&report);
    let bilan =
        run(&pc.environment, &report, &request, &excluded, None, &CleanControl::new()).unwrap();
    assert_eq!(bilan.duplicates.copies, 1);
    assert_eq!(bilan.skipped.other, 1);
    assert!(kept.exists() && private.exists() && !other.exists());
    assert_eq!(bin_content(&bin).len(), 1);
}

#[test]
fn a_copy_reached_through_a_link_is_left() {
    let (pc, bin) = pc_with_bin();
    let content = noise(9000, 7);
    let kept = pc.documents().join("photo.jpg");
    let album = pc.pictures().join("album");
    write(&kept, &content);
    pause();
    write(&album.join("photo.jpg"), &content);
    let report = pc.scan();
    assert_eq!(report.duplicates.copies, 1);

    // Après l'analyse, le dossier de la copie est déplacé et un lien prend sa
    // place : le même chemin mène toujours au même fichier, mais par un lien.
    let moved = pc.pictures().join("album déplacé");
    fs::rename(&album, &moved).unwrap();
    if !link_dir(&moved, &album) {
        eprintln!("cette machine ne permet pas de créer de lien : test sauté");
        return;
    }
    assert!(album.join("photo.jpg").exists());

    let bilan = clean(&pc.environment, &report, duplicates_only(&report));
    assert_eq!((bilan.duplicates.copies, bilan.skipped.other), (0, 1));
    assert!(moved.join("photo.jpg").exists() && kept.exists());
    assert!(bin_content(&bin).is_empty());
}

#[test]
fn a_doctored_report_cannot_make_the_engine_drop_the_last_copy() {
    let (pc, bin) = pc_with_bin();
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();
    let untouched = |bilan: &CleanReport, why: &str| {
        assert_eq!(bilan.duplicates.copies, 0, "{why}");
        assert!(kept.exists() && second.exists() && third.exists(), "{why}");
        assert!(bin_content(&bin).is_empty(), "{why}");
    };

    // Plus aucun exemplaire « gardé » : tout le groupe partirait.
    let mut none_kept = (*report).clone();
    none_kept.duplicates.groups[0].files[0].keep = false;
    let bilan = clean(&pc.environment, &none_kept, duplicates_only(&report));
    untouched(&bilan, "aucun exemplaire gardé");
    assert_eq!(bilan.skipped.other, 3);

    // Deux exemplaires « gardés » : ce n'est pas un groupe de l'analyse.
    let mut two_kept = (*report).clone();
    two_kept.duplicates.groups[0].files[1].keep = true;
    let bilan = clean(&pc.environment, &two_kept, duplicates_only(&report));
    untouched(&bilan, "deux exemplaires gardés");

    // Une « copie » qui est l'exemplaire gardé sous un autre nom : l'envoyer
    // à la corbeille y enverrait le seul exemplaire.
    let alias = pc.documents().join("alias.jpg");
    fs::hard_link(&kept, &alias).unwrap();
    let mut same_file = (*report).clone();
    {
        let files = &mut same_file.duplicates.groups[0].files;
        let (keeper, copy) = (files[0].clone(), &mut files[1]);
        copy.path = alias.clone();
        copy.file_id = keeper.file_id;
        copy.volume = keeper.volume;
        copy.modified = keeper.modified;
    }
    let bilan = clean(&pc.environment, &same_file, duplicates_only(&report));
    assert!(alias.exists() && kept.exists(), "deux noms d'un même fichier");
    assert_eq!(bilan.skipped.other, 1);
    assert_eq!(bilan.duplicates.copies, 1, "la vraie copie restante part");
    assert!(second.exists() && !third.exists());
}

#[test]
fn a_copy_outside_the_places_shinku_cleans_is_never_sent() {
    let (pc, bin) = pc_with_bin();
    let (kept, second, _third) = three_photos(&pc);
    let content = fs::read(&kept).unwrap();
    let report = pc.scan();

    // Le rapport désigne maintenant, comme « copies », un fichier de Windows
    // et un fichier d'une app : identiques à la photo, mais hors de tout
    // endroit où Shinku cherche des doublons.
    let system = pc.c.join("Windows").join("photo.jpg");
    let app = pc.roaming.join("MonApp").join("photo.jpg");
    write(&system, &content);
    write(&app, &content);
    let mut doctored = (*report).clone();
    for (copy, path) in doctored.duplicates.groups[0].files[1..].iter_mut().zip([&system, &app]) {
        let meta = listed(path).unwrap();
        copy.path = path.clone();
        copy.file_id = meta.file_id;
        copy.modified = meta.modified;
    }

    let bilan = clean(&pc.environment, &doctored, duplicates_only(&report));
    assert_eq!((bilan.duplicates.copies, bilan.skipped.other), (0, 2));
    assert!(system.exists() && app.exists() && kept.exists() && second.exists());
    assert!(bin_content(&bin).is_empty());
}

#[test]
fn copies_in_a_synced_folder_or_on_another_disk_are_announced() {
    let (mut pc, _bin) = pc_with_bin();
    let cloud = pc.profile.join("OneDrive");
    fs::create_dir_all(&cloud).unwrap();
    pc.environment = pc.environment.clone().with_sync_root(&cloud);
    let content = noise(9000, 7);
    write(&pc.documents().join("photo.jpg"), &content);
    pause();
    write(&cloud.join("photo.jpg"), &content);
    pause();
    write(&pc.pictures().join("photo.jpg"), &content);

    let report = pc.scan();
    let before = preview(&pc.environment, Some(&report)).unwrap();
    assert_eq!(before.duplicates.copies, 2);
    assert_eq!(before.duplicates.synced_copies, 1);
    assert_eq!(before.duplicates.other_disk_copies, 0);

    // La même analyse, si la dernière copie se trouvait sur un autre disque.
    let mut elsewhere = (*report).clone();
    elsewhere.duplicates.groups[0].files[2].volume += 1;
    let before = preview(&pc.environment, Some(&elsewhere)).unwrap();
    assert_eq!(before.duplicates.other_disk_copies, 1);
}

// ── Demandes, arrêt, journal ───────────────────────────────────────────

#[test]
fn a_request_is_checked_before_anything_is_touched() {
    let (mut pc, _bin) = pc_with_bin();
    let temp = pc.temp().join("vieux.tmp");
    write(&temp, &noise(5000, 1));
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();
    let environment = &pc.environment;
    let refused = |report: Option<&Report>, request: CleanRequest| {
        check(environment, report, &request).expect_err("demande refusée")
    };

    assert_eq!(refused(None, everything(&report)), CleanRefusal::NoResults);
    // Les doublons se cherchent encore : le rapport n'est pas complet.
    let mut partial = (*report).clone();
    partial.complete = false;
    assert_eq!(refused(Some(&partial), everything(&report)), CleanRefusal::NoResults);
    // L'écran montrait une autre analyse.
    let other = CleanRequest { analysis: report.started.unix_millis() + 1, ..everything(&report) };
    assert_eq!(refused(Some(&report), other), CleanRefusal::OtherAnalysis);
    // Rien n'est coché, ou seulement une corbeille vide.
    assert_eq!(refused(Some(&report), nothing(&report)), CleanRefusal::NothingToClean);
    let bin_only = CleanRequest { recycle_bin: true, ..nothing(&report) };
    assert_eq!(refused(Some(&report), bin_only), CleanRefusal::NothingToClean);
    assert_eq!(check(environment, Some(&report), &junk_only(&report)), Ok(()));
    assert_eq!(check(environment, Some(&report), &duplicates_only(&report)), Ok(()));

    // Onze minutes plus tard, les résultats ne servent plus à nettoyer.
    let later = Timestamp::from_ticks(report.finished.ticks() + 11 * 60 * 10_000_000);
    let stale = pc.environment.clone().with_now(later);
    assert_eq!(check(&stale, Some(&report), &everything(&report)), Err(CleanRefusal::StaleResults));
    assert_eq!(preview(&stale, Some(&report)), Err(CleanRefusal::StaleResults));
    assert_eq!(preview(&stale, None), Err(CleanRefusal::NoResults));
    let request = everything(&report);
    let outcome = run(&stale, &report, &request, &[], None, &CleanControl::new());
    assert_eq!(outcome.unwrap_err(), CleanRefusal::StaleResults);
    let started = start(Arc::new(stale), Arc::clone(&report), request, Vec::new(), None);
    assert!(matches!(started, Err(CleanStartError::Refused(CleanRefusal::StaleResults))));

    // Aucun de ces refus n'a touché à quoi que ce soit.
    assert!(temp.exists() && kept.exists() && second.exists() && third.exists());

    // Une case cochée sans rien à nettoyer est simplement « faite ».
    pc.environment = pc.environment.clone().with_recycle_bin(RecycleBin { bytes: 4096, items: 1 });
    fs::remove_file(&temp).unwrap();
    let empty = pc.scan();
    assert_eq!(empty.junk.files, 0);
    let request = CleanRequest { recycle_bin: true, junk_files: true, ..nothing(&empty) };
    let bilan = clean(&pc.environment, &empty, request);
    assert_eq!(bilan.junk.state, StepState::Done);
    assert_eq!((bilan.junk.files, bilan.recycle_bin.items), (0, 1));
}

#[test]
fn a_clean_stopped_before_it_starts_touches_nothing() {
    let (mut pc, _bin) = pc_with_bin();
    pc.environment = pc.environment.clone().with_recycle_bin(RecycleBin { bytes: 4096, items: 1 });
    let temp = pc.temp().join("vieux.tmp");
    write(&temp, &noise(5000, 1));
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();

    let control = CleanControl::new();
    control.cancel();
    let request = everything(&report);
    let bilan = run(&pc.environment, &report, &request, &[], None, &control).unwrap();
    assert_eq!(bilan.outcome, CleanOutcome::Cancelled);
    for state in [bilan.recycle_bin.state, bilan.junk.state, bilan.duplicates.state] {
        assert_eq!(state, StepState::Stopped);
    }
    assert_eq!((bilan.freed_bytes, bilan.recoverable_bytes), (0, 0));
    assert!(temp.exists() && kept.exists() && second.exists() && third.exists());
    assert_eq!(pc.environment.recycle_bin(), Some(RecycleBin { bytes: 4096, items: 1 }));
}

#[test]
fn stopping_lets_the_current_file_finish_and_gives_a_partial_result() {
    let (pc, bin) = pc_with_bin();
    let temp = pc.temp().join("vieux.tmp");
    write(&temp, &noise(5000, 1));
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();

    // L'utilisateur clique sur « Arrêter » pendant que la première copie part.
    let control = Arc::new(CleanControl::new());
    let stopper = Arc::clone(&control);
    let environment = rigged(&pc, move |pc, path| {
        stopper.cancel();
        pc.recycle(path).map(|_| ())
    });
    let request = CleanRequest { junk_files: true, duplicates: true, ..nothing(&report) };
    let bilan = run(&environment, &report, &request, &[], None, &control).unwrap();

    assert_eq!(bilan.outcome, CleanOutcome::Cancelled);
    assert_eq!(bilan.junk.state, StepState::Done, "l'étape d'avant était finie");
    assert_eq!(bilan.duplicates.state, StepState::Stopped);
    assert_eq!(bilan.duplicates.stop, None, "ce n'est pas un arrêt de sécurité");
    // La copie en cours est allée au bout ; la suivante n'a pas été touchée.
    assert_eq!(bilan.duplicates.copies, 1);
    assert_eq!(bin_content(&bin).len(), 1);
    assert_eq!([&second, &third].into_iter().filter(|copy| copy.exists()).count(), 1);
    assert!(kept.exists() && !temp.exists());
    assert_eq!(bilan.freed_bytes, report.junk.bytes);
    assert_eq!(bilan.recoverable_bytes, bilan.duplicates.bytes);
}

#[test]
fn stopping_during_the_junk_step_keeps_what_was_not_reached() {
    let (pc, bin) = pc_with_bin();
    let edge = pc.local.join("Microsoft/Edge/User Data/Default/Cache/f_1");
    let chrome = pc.local.join("Google/Chrome/User Data/Default/Cache/f_1");
    let temp = pc.temp().join("vieux.tmp");
    for (seed, file) in [&edge, &chrome, &temp].into_iter().enumerate() {
        write(file, &noise(9000, seed as u64));
    }
    let edge_bytes = on_disk(&edge);
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();
    assert_eq!(report.junk.files, 3);

    // L'utilisateur clique sur « Arrêter » pendant que le cache d'Edge part :
    // le moteur s'en aperçoit en arrivant à celui de Chrome.
    let control = Arc::new(CleanControl::new());
    let stopper = Arc::clone(&control);
    let asked = std::sync::atomic::AtomicUsize::new(0);
    let mut environment = rigged(&pc, |pc, path| pc.recycle(path).map(|_| ()));
    environment.on_process_list = Some(Box::new(move || {
        if asked.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 1 {
            stopper.cancel();
        }
    }));
    let request = CleanRequest { junk_files: true, duplicates: true, ..nothing(&report) };
    let bilan = run(&environment, &report, &request, &[], None, &control).unwrap();

    assert_eq!(bilan.outcome, CleanOutcome::Cancelled);
    assert_eq!(bilan.junk.state, StepState::Stopped);
    assert_eq!((bilan.junk.files, bilan.junk.bytes), (1, edge_bytes));
    assert!(!edge.exists(), "ce qui est parti reste parti");
    assert!(chrome.exists() && temp.exists(), "ce qui n'a pas été atteint reste");
    // L'étape suivante n'a pas commencé.
    assert_eq!(bilan.duplicates.state, StepState::Stopped);
    assert_eq!(bilan.duplicates.copies, 0);
    assert!(kept.exists() && second.exists() && third.exists());
    assert!(bin_content(&bin).is_empty());
    assert_eq!(bilan.freed_bytes, bilan.junk.bytes);
}

#[test]
fn the_journal_tells_how_each_clean_ended() {
    let (pc, _bin) = pc_with_bin();
    let journal = JournalStore::at(pc.roaming.join("Shinku").join("journal.json"));
    let temp = pc.temp().join("vieux.tmp");
    write(&temp, &noise(5000, 1));
    three_photos(&pc);
    let report = pc.scan();

    let request = junk_only(&report);
    let control = CleanControl::new();
    let bilan = run(&pc.environment, &report, &request, &[], Some(&journal), &control).unwrap();
    let runs = journal.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].state, RunState::Completed);
    assert_eq!(runs[0].started_ms, bilan.started.unix_millis());
    assert_eq!(runs[0].finished_ms, Some(bilan.finished.unix_millis()));
    assert_eq!(runs[0].freed_bytes, bilan.freed_bytes);
    assert_eq!((runs[0].recoverable_bytes, runs[0].skipped_files), (0, 0));

    // Un grand vide arrêté par l'utilisateur est noté comme tel : au
    // lancement suivant, Shinku ne le prendra pas pour une coupure.
    let later = pc.environment.clone().with_now(Timestamp::from_ticks(report.started.ticks() + 1));
    let later = later.with_now(Timestamp::from_ticks(report.started.ticks() + 20_000));
    let stopped = CleanControl::new();
    stopped.cancel();
    let request = duplicates_only(&report);
    run(&later, &report, &request, &[], Some(&journal), &stopped).unwrap();
    let runs = journal.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].state, RunState::Cancelled);
    assert_eq!(journal.close_interrupted(), None);

    // Le journal ne contient que des dates et des totaux : aucun chemin.
    let text = fs::read_to_string(journal.path()).unwrap();
    assert!(!text.contains("photo") && !text.contains("Users"), "{text}");
}

#[test]
fn a_journal_that_cannot_be_written_never_blocks_a_clean() {
    let pc = Pc::new();
    let temp = pc.temp().join("vieux.tmp");
    write(&temp, &noise(5000, 1));
    let report = pc.scan();
    // Le « dossier » du journal est un fichier : rien ne peut s'y écrire.
    let blocked = pc.roaming.join("bloqué");
    fs::write(&blocked, b"x").unwrap();
    let journal = JournalStore::at(blocked.join("Shinku").join("journal.json"));

    let request = junk_only(&report);
    let bilan =
        run(&pc.environment, &report, &request, &[], Some(&journal), &CleanControl::new()).unwrap();
    assert_eq!(bilan.junk.files, 1);
    assert!(!temp.exists());
    assert!(journal.runs().is_empty());
}

#[test]
fn a_clean_can_run_in_its_own_thread() {
    let (pc, bin) = pc_with_bin();
    let temp = pc.temp().join("vieux.tmp");
    write(&temp, &noise(5000, 1));
    let (kept, second, third) = three_photos(&pc);
    let report = pc.scan();
    let request = CleanRequest { junk_files: true, duplicates: true, ..nothing(&report) };

    let handle =
        start(Arc::new(pc.environment.clone()), Arc::clone(&report), request, Vec::new(), None)
            .unwrap();
    let control = Arc::clone(handle.control());
    let bilan = handle.wait();
    assert_eq!(bilan.outcome, CleanOutcome::Completed);
    assert_eq!((bilan.junk.files, bilan.duplicates.copies), (1, 2));
    assert!(!control.is_cancelled(), "attendre la fin n'arrête rien");
    assert_eq!(control.report().as_deref(), Some(&*bilan));
    assert!(!temp.exists() && kept.exists() && !second.exists() && !third.exists());
    assert_eq!(bin_content(&bin).len(), 2);
}

#[test]
fn the_report_travels_to_the_interface_as_plain_json() {
    let (mut pc, _bin) = pc_with_bin();
    pc.environment = pc
        .environment
        .clone()
        .with_recycle_bin(RecycleBin { bytes: 4096, items: 1 })
        .with_process("chrome.exe");
    write(&pc.local.join("Google/Chrome/User Data/Default/Cache/f_1"), &noise(9000, 1));
    write(&pc.temp().join("vieux.tmp"), &noise(5000, 2));
    three_photos(&pc);
    let report = pc.scan();

    let before = serde_json::to_value(preview(&pc.environment, Some(&report)).unwrap()).unwrap();
    assert_eq!(before["analysis"], report.started.unix_millis());
    assert_eq!(before["recycleBin"]["items"], 1);
    assert_eq!(before["junk"]["files"], 2);
    assert_eq!(before["junk"]["openApps"][0]["label"], "Google Chrome");
    assert_eq!(before["duplicates"]["copies"], 2);
    assert_eq!(before["duplicates"]["syncedCopies"], 0);
    assert_eq!(before["duplicates"]["otherDiskCopies"], 0);

    let bilan = clean(&pc.environment, &report, everything(&report));
    let json = serde_json::to_value(&*bilan).unwrap();
    assert_eq!(json["outcome"], "completed");
    assert_eq!(json["recycleBin"]["state"], "done");
    assert_eq!(json["recycleBin"]["items"], 1);
    assert_eq!(json["junk"]["state"], "done");
    assert_eq!(json["junk"]["files"], 1);
    assert_eq!(json["junk"]["leftOpen"][0]["label"], "Google Chrome");
    assert_eq!(json["duplicates"]["copies"], 2);
    assert_eq!(json["duplicates"]["stop"], serde_json::Value::Null);
    assert_eq!(json["skipped"]["inUse"], 0);
    assert_eq!(json["skipped"]["unrecyclable"], 0);
    assert!(json["freedBytes"].as_u64().unwrap() > 4096);
    assert_eq!(json["recoverableBytes"], bilan.duplicates.bytes);
    assert!(json["started"].is_i64() && json["durationMs"].is_u64());
}
