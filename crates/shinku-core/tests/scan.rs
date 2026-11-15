//! L'analyse de bout en bout, sur un faux PC construit dans un dossier temporaire.
//!
//! Chaque test pose quelques fichiers aux emplacements que Shinku connaît,
//! lance une vraie analyse et lit le rapport. C'est ici que se vérifie ce que
//! l'utilisateur verra à l'écran Résultats.

// Dans un test, un `unwrap` qui échoue est un test qui échoue : c'est voulu.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use shinku_core::fs::{DirReader, to_display_path, to_internal_path};
use shinku_core::hash::{CacheKey, CachedDigests, DigestCache, MemoryCache, NoCache};
use shinku_core::model::{Lot, ModuleId, Timestamp, Trigger};
use shinku_core::platform::{KnownFolder, RecycleBin, StaticEnvironment, Volume};
use shinku_core::rules::JunkCategory;
use shinku_core::scan::report::{CategoryState, DuplicateGroup, DuplicatesState, JunkLine, Report};
use shinku_core::scan::{Control, Phase, ScanOptions, run, start};

mod common;
use common::{create_hidden_dir, link_dir, write_hidden};

const DAY: Duration = Duration::from_secs(24 * 3600);

/// Un PC de test. L'horloge de la machine est avancée de trente jours : tout
/// fichier créé par un test est donc « vieux » par défaut, et on rajeunit
/// ceux qui doivent être récents.
struct Pc {
    _temp: tempfile::TempDir,
    c: PathBuf,
    profile: PathBuf,
    local: PathBuf,
    roaming: PathBuf,
    environment: StaticEnvironment,
}

impl Pc {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("dossier temporaire");
        let root = to_internal_path(&fs::canonicalize(temp.path()).unwrap()).unwrap();
        let c = root.join("c");
        let profile = c.join("Users").join("Léa");
        let local = profile.join("AppData").join("Local");
        let roaming = profile.join("AppData").join("Roaming");
        let libraries = ["Desktop", "Documents", "Downloads", "Music", "Pictures", "Videos"];
        for dir in [c.join("Windows"), c.join("Program Files"), local.join("Temp"), roaming.clone()]
        {
            fs::create_dir_all(dir).unwrap();
        }
        for library in libraries {
            fs::create_dir_all(profile.join(library)).unwrap();
        }
        let environment = StaticEnvironment::new()
            .with_now(Timestamp::from_system_time(SystemTime::now() + 30 * DAY))
            .with_parallelism(4)
            .with_volume(Volume {
                root: c.clone(),
                label: "Windows".to_owned(),
                file_system: "NTFS".to_owned(),
                total: 256_000_000_000,
                free: 40_000_000_000,
                fixed: true,
                seek_penalty: Some(false),
                system: true,
            })
            .with_folder(KnownFolder::Windows, c.join("Windows"))
            .with_folder(KnownFolder::ProgramFiles, c.join("Program Files"))
            .with_folder(KnownFolder::Profile, profile.clone())
            .with_folder(KnownFolder::Desktop, profile.join("Desktop"))
            .with_folder(KnownFolder::Documents, profile.join("Documents"))
            .with_folder(KnownFolder::Downloads, profile.join("Downloads"))
            .with_folder(KnownFolder::Music, profile.join("Music"))
            .with_folder(KnownFolder::Pictures, profile.join("Pictures"))
            .with_folder(KnownFolder::Videos, profile.join("Videos"))
            .with_folder(KnownFolder::LocalAppData, local.clone())
            .with_folder(KnownFolder::RoamingAppData, roaming.clone())
            .with_folder(KnownFolder::Temp, local.join("Temp"));
        Self { _temp: temp, c, profile, local, roaming, environment }
    }

    fn documents(&self) -> PathBuf {
        self.profile.join("Documents")
    }

    fn pictures(&self) -> PathBuf {
        self.profile.join("Pictures")
    }

    fn downloads(&self) -> PathBuf {
        self.profile.join("Downloads")
    }

    fn temp(&self) -> PathBuf {
        self.local.join("Temp")
    }

    /// Analyse avec des seuils adaptés à de petits fichiers de test.
    fn scan(&self) -> Arc<Report> {
        self.scan_with(|_| {})
    }

    fn scan_with(&self, adjust: impl FnOnce(&mut ScanOptions)) -> Arc<Report> {
        let options = self.options(adjust);
        run(&self.environment, &NoCache, &options, &Control::new()).expect("analyse menée au bout")
    }

    fn options(&self, adjust: impl FnOnce(&mut ScanOptions)) -> ScanOptions {
        let mut options = ScanOptions::new(Trigger::Button);
        options.large_threshold = 100_000;
        options.duplicate_min_size = 4096;
        adjust(&mut options);
        options
    }
}

/// Écrit un fichier, en créant ses dossiers.
fn write(path: &Path, content: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Contenu pseudo-aléatoire reproductible.
fn noise(length: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// Rajeunit un fichier : modifié « il y a une heure » pour la machine de test.
fn make_recent(path: &Path) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + 30 * DAY - Duration::from_secs(3600)).unwrap();
}

/// Laisse passer le temps qu'il faut pour que deux fichiers n'aient pas la même date.
fn pause() {
    std::thread::sleep(Duration::from_millis(30));
}

/// Place occupée sur le disque, lue comme le moteur la lit.
fn on_disk(path: &Path) -> u64 {
    let mut found = None;
    DirReader::new()
        .read(path.parent().unwrap(), None, &mut |name, meta| {
            if Some(name) == path.file_name() {
                found = Some(meta.allocated);
            }
        })
        .unwrap();
    found.expect("fichier présent")
}

fn line<'a>(report: &'a Report, category: JunkCategory, label: &str) -> Option<&'a JunkLine> {
    report
        .junk
        .categories
        .iter()
        .find(|found| found.category == category)?
        .lines
        .iter()
        .find(|line| line.label == label)
}

fn names(group: &DuplicateGroup) -> Vec<String> {
    group
        .files
        .iter()
        .map(|file| file.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

fn group_with<'a>(report: &'a Report, name: &str) -> Option<&'a DuplicateGroup> {
    report.duplicates.groups.iter().find(|group| names(group).iter().any(|found| found == name))
}

// ── Fichiers inutiles ──────────────────────────────────────────────────

#[test]
fn an_empty_pc_has_nothing_to_clean() {
    let pc = Pc::new();
    let report = pc.scan();
    assert!(report.complete);
    assert_eq!((report.junk.bytes, report.junk.files), (0, 0));
    assert_eq!(report.junk.categories.len(), 4, "les catégories vides restent listées");
    assert_eq!(report.large_files.count, 0);
    assert!(report.duplicates.groups.is_empty());
    assert_eq!(report.duplicates.state, DuplicatesState::Done);
    assert_eq!(report.summary().auto_bytes, 0, "« Faire le vide » reste grisé");
    assert!(report.skipped.refused_roots.is_empty(), "{:?}", report.skipped.refused_roots);
}

#[test]
fn browser_caches_are_grouped_by_app() {
    let pc = Pc::new();
    let chrome = pc.local.join("Google").join("Chrome").join("User Data");
    let files = [
        chrome.join("Default").join("Cache").join("Cache_Data").join("f_000001"),
        chrome.join("Default").join("Cache").join("Cache_Data").join("f_000002"),
        chrome.join("Profile 1").join("Code Cache").join("js").join("index"),
    ];
    for (index, file) in files.iter().enumerate() {
        write(file, &noise(9000 + index * 1000, index as u64));
    }
    // À côté des caches : des données du profil, qu'aucune règle ne désigne.
    write(&chrome.join("Default").join("History"), &noise(20_000, 9));
    write(&chrome.join("Default").join("Local Storage").join("leveldb").join("1.ldb"), b"x");
    write(&pc.roaming.join("discord").join("Cache").join("data_0"), &noise(8192, 3));

    let report = pc.scan();
    let chrome_line =
        line(&report, JunkCategory::AppCaches, "Google Chrome").expect("ligne Chrome");
    assert_eq!(chrome_line.files, 3);
    assert_eq!(chrome_line.bytes, files.iter().map(|file| on_disk(file)).sum::<u64>());
    assert_eq!(chrome_line.rules, ["chrome.cache", "chrome.code-cache"]);
    assert_eq!(chrome_line.close_first, ["chrome.exe"]);

    let discord = line(&report, JunkCategory::AppCaches, "Discord").expect("ligne Discord");
    assert_eq!(discord.files, 1);
    assert_eq!(report.junk.files, 4, "rien d'autre n'est compté");
    assert_eq!(report.junk.bytes, chrome_line.bytes + discord.bytes);
    // Le plus gros d'abord.
    let caches = &report.junk.categories[0];
    assert_eq!(caches.category, JunkCategory::AppCaches);
    assert_eq!(caches.lines[0].label, "Google Chrome");
}

#[test]
fn recent_temporary_files_are_spared() {
    let pc = Pc::new();
    let temp = pc.temp();
    write(&temp.join("vieux.tmp"), &noise(5000, 1));
    write(&temp.join("récent.tmp"), &noise(5000, 2));
    make_recent(&temp.join("récent.tmp"));
    // Le dossier d'un installeur en cours : un seul fichier récent suffit à
    // épargner tout le dossier.
    write(&temp.join("installeur").join("a.bin"), &noise(5000, 3));
    write(&temp.join("installeur").join("données").join("b.bin"), &noise(5000, 4));
    make_recent(&temp.join("installeur").join("données").join("b.bin"));
    // Un dossier où rien n'a bougé depuis longtemps.
    write(&temp.join("ancien").join("x").join("y.bin"), &noise(5000, 5));

    let report = pc.scan();
    let found = line(&report, JunkCategory::TempFiles, "Fichiers temporaires de Windows").unwrap();
    assert_eq!(found.files, 2, "vieux.tmp et ancien/x/y.bin");
    let expected = on_disk(&temp.join("vieux.tmp")) + on_disk(&temp.join("ancien/x/y.bin"));
    assert_eq!(found.bytes, expected);
    // `%TEMP%` et son emplacement habituel sont ici le même dossier : la
    // première règle l'emporte, rien n'est compté deux fois.
    assert_eq!(found.rules, ["windows.temp"]);
    assert_eq!(report.junk.files, 2);
    assert_eq!(report.junk.spared_files, 3);
}

#[test]
fn only_old_crash_dumps_match_the_rule() {
    let pc = Pc::new();
    let dumps = pc.local.join("CrashDumps");
    write(&dumps.join("app.exe.1234.dmp"), &noise(7000, 1));
    write(&dumps.join("APP.EXE.5678.DMP"), &noise(7000, 2));
    write(&dumps.join("récent.dmp"), &noise(7000, 3));
    make_recent(&dumps.join("récent.dmp"));
    write(&dumps.join("notes.txt"), &noise(7000, 4));
    write(&dumps.join("sous-dossier").join("autre.dmp"), &noise(7000, 5));

    let report = pc.scan();
    let found =
        line(&report, JunkCategory::LogsAndReports, "Rapports de plantage des apps").unwrap();
    // Sous Windows, la casse ne compte pas : les deux vieux rapports sont pris.
    let expected = if cfg!(windows) { 2 } else { 1 };
    assert_eq!(found.files, expected);
    assert_eq!(report.junk.spared_files, 1);
    assert_eq!(report.junk.files, expected, "ni notes.txt ni le sous-dossier");
}

#[test]
fn a_large_file_in_a_cache_is_junk_not_a_large_file() {
    let pc = Pc::new();
    write(&pc.temp().join("énorme.tmp"), &noise(300_000, 1));
    let report = pc.scan();
    assert_eq!(report.junk.files, 1);
    assert_eq!(report.large_files.count, 0);
}

#[test]
fn a_temp_folder_redirected_to_personal_files_is_refused() {
    let pc = Pc::new();
    let hijacked = pc.c.join("Données");
    write(&hijacked.join("thèse.doc"), &noise(9000, 1));
    let environment = pc.environment.clone().with_folder(KnownFolder::Temp, hijacked.clone());
    let report = run(&environment, &NoCache, &pc.options(|_| {}), &Control::new()).unwrap();

    assert_eq!(report.junk.files, 0, "rien de ce dossier n'est un fichier temporaire");
    let refused: Vec<_> =
        report.skipped.refused_roots.iter().map(|root| root.purpose.as_str()).collect();
    assert_eq!(refused, ["windows.temp"]);
    assert_eq!(report.skipped.refused_roots[0].path, hijacked);
}

#[test]
fn a_temp_folder_that_is_a_protected_place_is_refused() {
    // `%TEMP%` pointé sur Documents : la liste d'interdits s'y oppose, même
    // si le dossier s'appelait « Temp ».
    let pc = Pc::new();
    write(&pc.documents().join("contrat.pdf"), &noise(9000, 1));
    let environment = pc.environment.clone().with_folder(KnownFolder::Temp, pc.documents());
    let report = run(&environment, &NoCache, &pc.options(|_| {}), &Control::new()).unwrap();
    assert_eq!(report.junk.files, 0);
    assert_eq!(report.skipped.refused_roots.len(), 1);
}

#[test]
fn a_link_planted_in_temp_does_not_lead_to_personal_files() {
    let pc = Pc::new();
    write(&pc.documents().join("thèse.doc"), &noise(50_000, 1));
    write(&pc.temp().join("vrai.tmp"), &noise(5000, 2));
    if !link_dir(&pc.documents(), &pc.temp().join("piège")) {
        eprintln!("liens indisponibles ici : test ignoré");
        return;
    }

    let report = pc.scan();
    // Le lien lui-même est un élément à supprimer ; ce vers quoi il pointe n'est pas compté.
    assert_eq!(report.junk.files, 2);
    assert!(report.junk.bytes < 50_000, "{} octets comptés", report.junk.bytes);
}

#[test]
fn a_rule_location_reached_through_a_link_is_refused() {
    // Le dossier du cache de Discord remplacé par un lien vers Documents.
    let pc = Pc::new();
    write(&pc.documents().join("Cache").join("thèse.doc"), &noise(9000, 1));
    fs::create_dir_all(pc.roaming.join("discord")).unwrap();
    // Et tout le dossier d'une autre app remplacé par un lien.
    write(&pc.documents().join("Slack").join("Cache").join("photo.raw"), &noise(9000, 2));
    let linked = link_dir(&pc.documents().join("Cache"), &pc.roaming.join("discord").join("Cache"))
        && link_dir(&pc.documents().join("Slack"), &pc.roaming.join("Slack"));
    if !linked {
        eprintln!("liens indisponibles ici : test ignoré");
        return;
    }

    let report = pc.scan();
    assert_eq!(report.junk.files, 0, "aucun fichier personnel n'est pris pour un cache");
    let refused: Vec<_> =
        report.skipped.refused_roots.iter().map(|root| root.purpose.as_str()).collect();
    assert!(refused.contains(&"discord.cache"), "{refused:?}");
    assert!(refused.contains(&"slack.cache"), "{refused:?}");
}

#[cfg(windows)]
#[test]
fn locations_match_whatever_the_case() {
    // Windows ne distingue pas les majuscules : une règle, un interdit ou une
    // exclusion valent pour toutes les écritures d'un même nom.
    let pc = Pc::new();
    let chrome = pc.local.join("GOOGLE").join("chrome").join("USER DATA");
    write(&chrome.join("default").join("CACHE").join("f_1"), &noise(9000, 1));
    write(&pc.c.join("WINDOWS.OLD").join("gros.bin"), &noise(300_000, 2));
    write(&pc.documents().join("Privé").join("film.mkv"), &noise(300_000, 3));
    let shouted = to_display_path(&pc.documents().join("Privé")).to_string_lossy().to_uppercase();

    let report = pc.scan_with(|options| options.excluded = vec![PathBuf::from(shouted)]);
    let chrome_line = line(&report, JunkCategory::AppCaches, "Google Chrome").expect("le cache");
    assert_eq!(chrome_line.files, 1);
    assert_eq!(report.large_files.count, 0, "{:?}", report.large_files.files);
    assert!(report.skipped.refused_roots.is_empty(), "{:?}", report.skipped.refused_roots);
}

// ── Gros fichiers ──────────────────────────────────────────────────────

#[test]
fn large_files_are_listed_by_size_outside_protected_places() {
    let pc = Pc::new();
    write(&pc.c.join("Jeux").join("gros.iso"), &noise(300_000, 1));
    write(&pc.documents().join("film.mkv"), &noise(200_000, 2));
    write(&pc.documents().join("petit.txt"), &noise(50_000, 3));
    // Interdits : rien de tout ça ne doit apparaître.
    write(&pc.c.join("Windows").join("System32").join("énorme.sys"), &noise(400_000, 4));
    write(&pc.c.join("Program Files").join("App").join("data.bin"), &noise(400_000, 5));
    write(&pc.c.join("pagefile.sys"), &noise(400_000, 6));
    write(&pc.c.join("$Recycle.Bin").join("S-1-5-21").join("$R1.iso"), &noise(400_000, 7));

    let report = pc.scan();
    let listed: Vec<PathBuf> = report.large_files.files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(listed, [pc.c.join("Jeux").join("gros.iso"), pc.documents().join("film.mkv")]);
    assert_eq!(report.large_files.count, 2);
    let first = &report.large_files.files[0];
    assert_eq!(first.size, 300_000);
    assert_eq!(first.on_disk, on_disk(&pc.c.join("Jeux").join("gros.iso")));
    assert_eq!(report.large_files.bytes, first.on_disk + report.large_files.files[1].on_disk);
    assert!(!first.synced);
    assert_eq!(report.large_files.threshold, 100_000);
}

#[test]
fn what_explorer_hides_is_not_proposed() {
    let pc = Pc::new();
    let content = noise(10_000, 4);
    write(&pc.documents().join("visible.mkv"), &noise(300_000, 3));
    write(&pc.pictures().join("copie.bin"), &content);
    // Fichiers cachés : ni gros fichier, ni doublon.
    let hidden = write_hidden(&pc.documents(), "brouillon.mkv", &noise(300_000, 2))
        .and(write_hidden(&pc.documents(), "copie.bin", &content));
    if hidden.is_none() {
        eprintln!("fichiers cachés indisponibles ici : test ignoré");
        return;
    }
    // Dossier caché : rien de ce qu'il contient n'est proposé.
    match create_hidden_dir(&pc.documents(), "archives") {
        Some(archives) => {
            write(&archives.join("machine.vhdx"), &noise(300_000, 1));
            write(&archives.join("copie (2).bin"), &content);
        }
        None => eprintln!("dossiers cachés indisponibles ici : cette partie est ignorée"),
    }

    let report = pc.scan();
    let listed: Vec<_> = report.large_files.files.iter().map(|file| file.path.clone()).collect();
    assert_eq!(listed, [pc.documents().join("visible.mkv")]);
    assert!(report.duplicates.groups.is_empty(), "{:?}", report.duplicates.groups);
    assert_eq!(report.duplicates.candidates, 2, "les deux fichiers visibles, et eux seuls");
}

#[test]
fn the_large_file_threshold_counts_space_on_disk() {
    let pc = Pc::new();
    write(&pc.documents().join("juste-sous.bin"), &noise(60_000, 1));
    let threshold = on_disk(&pc.documents().join("juste-sous.bin")) + 1;
    let report = pc.scan_with(|options| options.large_threshold = threshold);
    assert_eq!(report.large_files.count, 0);
    let report = pc.scan_with(|options| options.large_threshold = threshold - 1);
    assert_eq!(report.large_files.count, 1);
}

// ── Doublons ───────────────────────────────────────────────────────────

#[test]
fn identical_files_form_a_group_and_the_oldest_is_kept() {
    let pc = Pc::new();
    let content = noise(10_000, 1);
    write(&pc.documents().join("photo.jpg"), &content);
    pause();
    write(&pc.pictures().join("photo-copie.jpg"), &content);
    pause();
    write(&pc.downloads().join("photo (2).jpg"), &content);

    let report = pc.scan();
    assert_eq!(report.duplicates.groups.len(), 1);
    let group = &report.duplicates.groups[0];
    assert_eq!(names(group), ["photo.jpg", "photo-copie.jpg", "photo (2).jpg"]);
    assert_eq!(group.files.iter().map(|file| file.keep).collect::<Vec<_>>(), [true, false, false]);
    assert_eq!(group.size, 10_000);
    assert_eq!(group.digest, blake3::hash(&content).to_hex().to_string());
    let copies = on_disk(&pc.pictures().join("photo-copie.jpg"))
        + on_disk(&pc.downloads().join("photo (2).jpg"));
    assert_eq!(group.reclaimable, copies);
    assert_eq!((report.duplicates.copies, report.duplicates.bytes), (2, copies));
    assert_eq!(report.duplicates.candidates, 3);
}

#[test]
fn files_that_only_look_alike_are_not_duplicates() {
    let pc = Pc::new();
    // Même taille, contenus différents.
    write(&pc.documents().join("a.bin"), &noise(10_000, 1));
    write(&pc.pictures().join("b.bin"), &noise(10_000, 2));
    // Même début et même fin, un octet différent au milieu.
    let content = noise(300_000, 3);
    let mut altered = content.clone();
    altered[150_000] ^= 0xFF;
    write(&pc.documents().join("film.mkv"), &content);
    write(&pc.pictures().join("film-monté.mkv"), &altered);
    // Identiques, mais trop petits pour valoir le risque.
    write(&pc.documents().join("note.txt"), b"bonjour");
    write(&pc.pictures().join("note.txt"), b"bonjour");

    let report = pc.scan_with(|options| options.large_threshold = u64::MAX);
    assert!(report.duplicates.groups.is_empty(), "{:?}", report.duplicates.groups);
    assert_eq!(report.duplicates.candidates, 4);
    assert_eq!(report.duplicates.skipped_files, 0);
}

#[test]
fn hard_links_are_one_file_not_two_copies() {
    let pc = Pc::new();
    let content = noise(10_000, 1);
    write(&pc.documents().join("original.bin"), &content);
    if fs::hard_link(pc.documents().join("original.bin"), pc.pictures().join("lien.bin")).is_err() {
        eprintln!("liens physiques indisponibles ici : test ignoré");
        return;
    }
    let report = pc.scan();
    assert!(report.duplicates.groups.is_empty(), "deux noms d'un même fichier");
    assert_eq!(report.duplicates.hard_links, 1);

    // Avec une vraie copie en plus : un groupe de deux, pas de trois.
    pause();
    write(&pc.downloads().join("vraie-copie.bin"), &content);
    let report = pc.scan();
    assert_eq!(report.duplicates.groups.len(), 1);
    assert_eq!(report.duplicates.groups[0].files.len(), 2);
    assert_eq!(report.duplicates.copies, 1);
}

#[test]
fn projects_and_app_folders_are_left_alone() {
    let pc = Pc::new();
    let content = noise(10_000, 1);
    // Un dépôt de code : les fichiers identiques y sont voulus.
    let project = pc.documents().join("mon-site");
    fs::create_dir_all(project.join(".git")).unwrap();
    write(&project.join("assets").join("logo.bin"), &content);
    write(&project.join("public").join("logo.bin"), &content);
    // Des dépendances téléchargées par un outil.
    let modules = pc.documents().join("autre").join("node_modules");
    write(&modules.join("x").join("lib.bin"), &content);
    write(&modules.join("y").join("lib.bin"), &content);
    // Une app décompressée dans Téléchargements.
    let app = pc.downloads().join("MonApp");
    write(&app.join("moteur.dll"), &noise(5000, 2));
    write(&app.join("données").join("a.pak"), &content);
    write(&app.join("données").join("b.pak"), &content);
    // Un dossier d'outil, reconnaissable à son point.
    write(&pc.documents().join(".cache-outil").join("1.bin"), &content);
    write(&pc.documents().join(".cache-outil").join("2.bin"), &content);

    let report = pc.scan();
    assert!(report.duplicates.groups.is_empty(), "{:?}", report.duplicates.groups);
    assert_eq!(report.duplicates.candidates, 0);
}

#[test]
fn a_downloaded_library_does_not_turn_downloads_into_an_app() {
    let pc = Pc::new();
    let installer = noise(20_000, 1);
    write(&pc.downloads().join("outil.dll"), &noise(5000, 2));
    write(&pc.downloads().join("setup.exe"), &installer);
    pause();
    write(&pc.downloads().join("setup (1).exe"), &installer);
    let report = pc.scan();
    let group = group_with(&report, "setup.exe").expect("les deux installeurs");
    assert_eq!(names(group), ["setup.exe", "setup (1).exe"]);
}

#[test]
fn copies_in_a_synced_folder_carry_a_warning() {
    let pc = Pc::new();
    let onedrive = pc.profile.join("OneDrive");
    let content = noise(10_000, 1);
    write(&pc.pictures().join("vacances.jpg"), &content);
    pause();
    write(&onedrive.join("Images").join("vacances.jpg"), &content);
    write(&onedrive.join("Images").join("gros.mov"), &noise(300_000, 2));
    let environment = pc.environment.clone().with_sync_root(onedrive.clone());
    let report = run(&environment, &NoCache, &pc.options(|_| {}), &Control::new()).unwrap();

    let group = group_with(&report, "vacances.jpg").expect("la copie dans OneDrive");
    let synced: Vec<bool> = group.files.iter().map(|file| file.synced).collect();
    assert_eq!(synced, [false, true], "seule la copie de OneDrive est synchronisée");
    assert!(group.files[0].keep);
    let large = &report.large_files.files[0];
    assert_eq!(large.path, onedrive.join("Images").join("gros.mov"));
    assert!(large.synced, "supprimer ce fichier le supprimerait aussi dans le cloud");
}

#[test]
fn digests_are_reused_from_the_cache() {
    /// Cache qui compte les empreintes qu'il a fallu calculer.
    #[derive(Debug, Default)]
    struct Counting {
        inner: MemoryCache,
        computed: AtomicUsize,
    }
    impl DigestCache for Counting {
        fn get(&self, key: &CacheKey) -> Option<CachedDigests> {
            self.inner.get(key)
        }
        fn put(&self, key: CacheKey, digests: CachedDigests) {
            self.computed.fetch_add(1, Ordering::Relaxed);
            self.inner.put(key, digests);
        }
    }

    let pc = Pc::new();
    let content = noise(300_000, 1);
    write(&pc.documents().join("film.mkv"), &content);
    pause();
    write(&pc.pictures().join("film (1).mkv"), &content);
    // Deux petits fichiers : une seule lecture chacun suffit.
    let small = noise(10_000, 2);
    write(&pc.documents().join("photo.jpg"), &small);
    pause();
    write(&pc.pictures().join("photo (1).jpg"), &small);
    let cache = Counting::default();
    let options = pc.options(|options| options.large_threshold = u64::MAX);

    let first = run(&pc.environment, &cache, &options, &Control::new()).unwrap();
    assert_eq!(first.duplicates.groups.len(), 2);
    // Extrait puis contenu pour chaque film, une lecture pour chaque photo.
    assert_eq!(cache.computed.load(Ordering::Relaxed), 6);
    assert_eq!(cache.inner.len(), 4);

    let second = run(&pc.environment, &cache, &options, &Control::new()).unwrap();
    assert_eq!(second.duplicates.groups, first.duplicates.groups);
    assert_eq!(cache.computed.load(Ordering::Relaxed), 6, "aucun fichier n'a été relu");

    // Un fichier modifié n'est plus celui du cache : lui seul est relu, et
    // son extrait suffit à voir qu'il n'est plus une copie.
    pause();
    write(&pc.pictures().join("film (1).mkv"), &noise(300_000, 3));
    let third = run(&pc.environment, &cache, &options, &Control::new()).unwrap();
    assert_eq!(third.duplicates.groups.len(), 1);
    assert_eq!(names(&third.duplicates.groups[0]), ["photo.jpg", "photo (1).jpg"]);
    assert_eq!(cache.computed.load(Ordering::Relaxed), 7);
}

// ── Dossiers jamais touchés ────────────────────────────────────────────

#[test]
fn excluded_folders_are_invisible_to_every_module() {
    let pc = Pc::new();
    let private = pc.documents().join("privé");
    let content = noise(10_000, 1);
    write(&private.join("copie.bin"), &content);
    write(&pc.pictures().join("copie.bin"), &content);
    write(&private.join("gros.iso"), &noise(300_000, 2));
    let chrome = pc.local.join("Google").join("Chrome").join("User Data");
    write(&chrome.join("Default").join("Cache").join("f_1"), &noise(9000, 3));
    write(&chrome.join("Profile 1").join("Cache").join("f_2"), &noise(9000, 4));
    // Un dossier de travail dans %TEMP%, qui contient un dossier exclu.
    write(&pc.temp().join("travail").join("précieux").join("x.bin"), &noise(9000, 5));
    write(&pc.temp().join("travail").join("reste.tmp"), &noise(9000, 6));
    write(&pc.temp().join("autre.tmp"), &noise(9000, 7));

    let excluded = vec![
        // Donnés comme l'utilisateur les écrit : sous la forme habituelle.
        to_display_path(&private),
        to_display_path(&chrome.join("Default").join("Cache")),
        to_display_path(&pc.temp().join("travail").join("précieux")),
    ];
    let report = pc.scan_with(|options| options.excluded = excluded);

    assert!(report.duplicates.groups.is_empty());
    assert_eq!(report.large_files.count, 0);
    let chrome_line = line(&report, JunkCategory::AppCaches, "Google Chrome").unwrap();
    assert_eq!(chrome_line.files, 1, "seul le profil non exclu est compté");
    let temp = line(&report, JunkCategory::TempFiles, "Fichiers temporaires de Windows").unwrap();
    assert_eq!(temp.files, 1, "un dossier qui contient un dossier exclu n'est pas nettoyé");
    assert!(report.skipped.refused_roots.is_empty());
}

#[test]
fn an_exclusion_given_through_a_link_protects_the_real_folder() {
    let pc = Pc::new();
    let content = noise(10_000, 1);
    write(&pc.documents().join("privé").join("copie.bin"), &content);
    write(&pc.pictures().join("copie.bin"), &content);
    // L'ancien nom du dossier Documents, que Windows garde sous forme de lien.
    let legacy = pc.profile.join("Mes documents");
    if !link_dir(&pc.documents(), &legacy) {
        eprintln!("liens indisponibles ici : test ignoré");
        return;
    }

    let report = pc.scan_with(|options| options.excluded = vec![legacy.join("privé")]);
    assert!(report.duplicates.groups.is_empty(), "{:?}", report.duplicates.groups);
    assert_eq!(report.duplicates.candidates, 1);
    assert!(report.skipped.refused_roots.is_empty());
}

#[test]
fn an_exclusion_that_cannot_be_applied_is_reported() {
    let pc = Pc::new();
    let report = pc.scan_with(|options| options.excluded = vec![PathBuf::from("relatif/privé")]);
    assert_eq!(report.skipped.refused_roots.len(), 1);
    assert_eq!(report.skipped.refused_roots[0].purpose, "exclusion");
}

// ── Déroulement ────────────────────────────────────────────────────────

#[test]
fn the_summary_lists_the_five_categories_of_the_big_clean() {
    let pc = Pc::new();
    write(&pc.temp().join("vieux.tmp"), &noise(9000, 1));
    let content = noise(10_000, 2);
    write(&pc.documents().join("a.jpg"), &content);
    write(&pc.pictures().join("a.jpg"), &content);
    write(&pc.documents().join("film.mkv"), &noise(300_000, 3));
    let environment =
        pc.environment.clone().with_recycle_bin(RecycleBin { bytes: 5_000_000, items: 12 });
    let report = run(&environment, &NoCache, &pc.options(|_| {}), &Control::new()).unwrap();
    let summary = report.summary();

    let modules: Vec<ModuleId> = summary.categories.iter().map(|c| c.module).collect();
    assert_eq!(
        modules,
        [
            ModuleId::RecycleBin,
            ModuleId::JunkFiles,
            ModuleId::Duplicates,
            ModuleId::LargeFiles,
            ModuleId::Startup
        ]
    );
    let lots: Vec<Lot> = summary.categories.iter().map(|category| category.lot).collect();
    assert_eq!(lots, [Lot::Auto, Lot::Auto, Lot::Auto, Lot::Review, Lot::Review]);
    assert_eq!((summary.categories[0].bytes, summary.categories[0].items), (5_000_000, 12));
    assert_eq!(summary.categories[1].items, 1);
    assert_eq!(summary.categories[2].items, 1);
    assert_eq!(summary.categories[3].items, 1);
    assert_eq!(summary.categories[4].state, CategoryState::Unavailable);
    // Les gros fichiers ne partent jamais avec « Faire le vide ».
    assert_eq!(
        summary.auto_bytes,
        5_000_000 + report.junk.bytes + report.duplicates.bytes,
        "corbeille, fichiers inutiles et doublons"
    );
    assert!(summary.complete);
    assert!(!report.is_stale(report.finished));
    assert!(report.is_stale(Timestamp::from_ticks(report.finished.ticks() + 11 * 60 * 10_000_000)));
}

#[test]
fn modules_can_be_switched_off() {
    let pc = Pc::new();
    write(&pc.temp().join("vieux.tmp"), &noise(9000, 1));
    let content = noise(10_000, 2);
    write(&pc.documents().join("a.jpg"), &content);
    write(&pc.pictures().join("a.jpg"), &content);
    let report = pc.scan_with(|options| {
        options.modules.duplicates = false;
        options.modules.junk = false;
    });
    assert_eq!(report.junk.files, 0);
    assert_eq!(report.duplicates.state, DuplicatesState::Off);
    assert!(report.complete);
    assert_eq!(report.summary().categories[2].state, CategoryState::Unavailable);
}

#[test]
fn a_cancelled_scan_leaves_no_results() {
    let pc = Pc::new();
    write(&pc.temp().join("vieux.tmp"), &noise(9000, 1));
    let control = Control::new();
    control.cancel();
    let report = run(&pc.environment, &NoCache, &pc.options(|_| {}), &control);
    assert!(report.is_none());
    assert!(control.latest_report().is_none());
    assert_eq!(control.progress().phase, Phase::Cancelled);
}

#[test]
fn a_scan_can_run_in_its_own_thread() {
    let pc = Pc::new();
    write(&pc.temp().join("vieux.tmp"), &noise(9000, 1));
    let handle =
        start(Arc::new(pc.environment.clone()), Arc::new(MemoryCache::new()), pc.options(|_| {}))
            .unwrap();
    let control = Arc::clone(handle.control());
    let report = handle.wait().expect("analyse menée au bout");
    assert_eq!(report.junk.files, 1);
    let progress = control.progress();
    assert_eq!(progress.phase, Phase::Done);
    assert!(progress.folders > 5 && progress.files >= 1, "{progress:?}");
    assert_eq!(control.latest_report().map(|latest| latest.junk.files), Some(1));
}

#[test]
fn the_report_travels_to_the_interface_as_plain_json() {
    let pc = Pc::new();
    let content = noise(300_000, 1);
    write(&pc.documents().join("film.mkv"), &content);
    write(&pc.pictures().join("film.mkv"), &content);
    let report = pc.scan();
    let json = serde_json::to_value(&*report).unwrap();

    assert_eq!(json["trigger"], "button");
    assert_eq!(json["complete"], true);
    assert!(json["finished"].is_i64(), "une date voyage en millisecondes");
    assert_eq!(json["junk"]["categories"][0]["label"], "Caches des apps");
    assert_eq!(json["duplicates"]["state"], "done");
    let shown = json["largeFiles"]["files"][0]["path"].as_str().unwrap();
    assert_eq!(Path::new(shown), to_display_path(&pc.documents().join("film.mkv")));
    assert!(!shown.starts_with(r"\\?\"), "l'interface ne voit pas la forme interne");
    assert_eq!(json["largeFiles"]["files"][0]["onDisk"], on_disk(&pc.documents().join("film.mkv")));
    assert_eq!(json["duplicates"]["groups"][0]["files"][0]["keep"], true);
    assert_eq!(json["volumes"][0]["fileSystem"], "NTFS");
    let summary = serde_json::to_value(report.summary()).unwrap();
    assert_eq!(summary["categories"][0]["module"], "recycleBin");
    assert_eq!(summary["categories"][0]["risk"], "D");
}

#[test]
fn folders_and_files_are_counted() {
    let pc = Pc::new();
    for index in 0..40 {
        write(
            &pc.documents().join(format!("dossier-{}", index % 4)).join(format!("{index}.txt")),
            b"x",
        );
    }
    let report = pc.scan();
    assert!(report.stats.files >= 40, "{:?}", report.stats);
    assert!(report.stats.folders >= 12, "{:?}", report.stats);
    assert_eq!(report.skipped.denied_folders, 0);
}
