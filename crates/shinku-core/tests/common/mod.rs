//! Outils communs aux tests d'intégration.

// Chaque fichier de tests n'utilise qu'une partie de ces outils.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use shinku_core::fs::{DirReader, EntryFlags, EntryKind, EntryMeta, to_internal_path};
use shinku_core::hash::NoCache;
use shinku_core::model::{Timestamp, Trigger};
use shinku_core::platform::{KnownFolder, StaticEnvironment, Volume};
use shinku_core::scan::report::Report;
use shinku_core::scan::{Control, ScanOptions, run};

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

pub const DAY: Duration = Duration::from_secs(24 * 3600);

/// Un PC de test. L'horloge de la machine est avancée de trente jours : tout
/// fichier créé par un test est donc « vieux » par défaut, et on rajeunit
/// ceux qui doivent être récents.
pub struct Pc {
    pub _temp: tempfile::TempDir,
    pub c: PathBuf,
    pub profile: PathBuf,
    pub local: PathBuf,
    pub roaming: PathBuf,
    pub environment: StaticEnvironment,
}

impl Pc {
    pub fn new() -> Self {
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

    pub fn documents(&self) -> PathBuf {
        self.profile.join("Documents")
    }

    pub fn pictures(&self) -> PathBuf {
        self.profile.join("Pictures")
    }

    pub fn downloads(&self) -> PathBuf {
        self.profile.join("Downloads")
    }

    pub fn temp(&self) -> PathBuf {
        self.local.join("Temp")
    }

    /// Analyse avec des seuils adaptés à de petits fichiers de test.
    pub fn scan(&self) -> Arc<Report> {
        self.scan_with(|_| {})
    }

    pub fn scan_with(&self, adjust: impl FnOnce(&mut ScanOptions)) -> Arc<Report> {
        let options = self.options(adjust);
        run(&self.environment, &NoCache, &options, &Control::new()).expect("analyse menée au bout")
    }

    pub fn options(&self, adjust: impl FnOnce(&mut ScanOptions)) -> ScanOptions {
        let mut options = ScanOptions::new(Trigger::Button);
        options.large_threshold = 100_000;
        options.duplicate_min_size = 4096;
        adjust(&mut options);
        options
    }
}

/// Écrit un fichier, en créant ses dossiers.
pub fn write(path: &Path, content: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Contenu pseudo-aléatoire reproductible.
pub fn noise(length: usize, seed: u64) -> Vec<u8> {
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
pub fn make_recent(path: &Path) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + 30 * DAY - Duration::from_secs(3600)).unwrap();
}

/// Laisse passer le temps qu'il faut pour que deux fichiers n'aient pas la même date.
pub fn pause() {
    std::thread::sleep(Duration::from_millis(30));
}

/// Place occupée sur le disque, lue comme le moteur la lit.
pub fn on_disk(path: &Path) -> u64 {
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
