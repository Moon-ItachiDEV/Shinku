//! « Voir leurs licences » : ouvrir `LICENCES-TIERS.txt`, posé à côté de
//! `shinku.exe`, dans le programme que le compte associe aux `.txt`.
//!
//! La page ne dit **jamais** quel fichier : le chemin est calculé ici (le
//! dossier de l'exécutable, puis le nom fixe). La décision d'ouvrir est
//! **pure** ([`decide`]) et se juge sur ce que le fichier est, pas sur son nom :
//! un fichier ordinaire (ni dossier, ni lien, ni point de réanalyse), non vide,
//! de moins de 4 Mio. Sinon [`LicencesOpened::Missing`].
//!
//! Ce qui est accepté, et dit comme tel : le programme qui s'ouvre est celui que
//! le compte associe aux `.txt` (un programme du compte qui détourne cette
//! association a déjà les droits de la personne), et sans association Windows
//! peut montrer « Ouvrir avec » au lieu d'échouer. Rien n'est élevé. Hors
//! Windows (et dans la démo), la réponse est `missing`.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Nom du fichier des licences des composants tiers.
pub const FILE_NAME: &str = "LICENCES-TIERS.txt";
/// Taille maximale d'un fichier qu'on accepte d'ouvrir (le vrai fait environ
/// 430 Kio).
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Ce que la fenêtre apprend de la demande.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LicencesOpened {
    /// Windows a ouvert le fichier.
    Opened,
    /// Le fichier n'est pas là, ou n'est pas celui qu'on attend.
    Missing,
    /// Windows n'a pas pu l'ouvrir.
    Failed,
}

/// Ce que Shinku sait d'un fichier avant de l'ouvrir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileFacts {
    /// Un fichier ordinaire (pas un dossier, pas autre chose).
    pub regular: bool,
    /// Un lien symbolique ou un point de réanalyse.
    pub reparse: bool,
    /// Sa taille, en octets.
    pub len: u64,
}

/// La machine vue par « Voir leurs licences ».
pub trait LicencesHost: Send + Sync + fmt::Debug {
    /// Le chemin du fichier, à côté de l'exécutable, si on le connaît.
    fn file(&self) -> Option<PathBuf>;
    /// Ce qu'est ce chemin, sans suivre un lien. `None` : il n'existe pas.
    fn facts(&self, path: &Path) -> Option<FileFacts>;
    /// Ouvre ce fichier dans le programme associé aux `.txt`.
    fn open(&self, path: &Path) -> io::Result<()>;
}

/// Faut-il ouvrir ? Vrai pour un fichier ordinaire, sans lien, non vide, de
/// moins de 4 Mio.
pub fn decide(facts: Option<FileFacts>) -> bool {
    facts.is_some_and(|facts| {
        facts.regular && !facts.reparse && facts.len > 0 && facts.len < MAX_BYTES
    })
}

/// Ouvre les licences : le chemin vient de l'hôte, la décision d'[`decide`].
pub fn open(host: &dyn LicencesHost) -> LicencesOpened {
    let Some(path) = host.file() else { return LicencesOpened::Missing };
    if !decide(host.facts(&path)) {
        return LicencesOpened::Missing;
    }
    match host.open(&path) {
        Ok(()) => LicencesOpened::Opened,
        Err(_) => LicencesOpened::Failed,
    }
}

/// Le chemin sans `\\?\` (sauf pour un chemin réseau), forme que le shell
/// comprend.
pub fn without_verbatim(path: &str) -> String {
    match path.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => rest.to_owned(),
        _ => path.to_owned(),
    }
}

/// Un hôte qui ne trouve jamais le fichier (la démo, les essais).
#[derive(Debug)]
pub struct InertLicences;

impl LicencesHost for InertLicences {
    fn file(&self) -> Option<PathBuf> {
        None
    }
    fn facts(&self, _path: &Path) -> Option<FileFacts> {
        None
    }
    fn open(&self, _path: &Path) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "pas de fichier des licences"))
    }
}

/// Cette machine : le fichier est dans le dossier de l'exécutable en cours.
#[derive(Debug)]
pub struct SystemLicences;

impl LicencesHost for SystemLicences {
    fn file(&self) -> Option<PathBuf> {
        // Hors Windows, `shinku` ne s'installe pas : rien à ouvrir.
        if !cfg!(windows) {
            return None;
        }
        let exe = std::env::current_exe().ok()?;
        let dir = without_verbatim(exe.parent()?.to_str()?);
        Some(Path::new(&dir).join(FILE_NAME))
    }

    fn facts(&self, path: &Path) -> Option<FileFacts> {
        let metadata = std::fs::symlink_metadata(path).ok()?;
        Some(FileFacts {
            regular: metadata.file_type().is_file(),
            reparse: is_reparse(&metadata),
            len: metadata.len(),
        })
    }

    fn open(&self, path: &Path) -> io::Result<()> {
        crate::platform::agent::open_file(path)
    }
}

/// Un lien symbolique, ou (Windows) un point de réanalyse.
#[cfg(windows)]
fn is_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    /// `FILE_ATTRIBUTE_REPARSE_POINT`.
    const REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink() || metadata.file_attributes() & REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug)]
    struct Fake {
        path: Option<PathBuf>,
        facts: Option<FileFacts>,
        fails: bool,
        opened: Mutex<Vec<PathBuf>>,
    }

    impl Fake {
        fn with(facts: Option<FileFacts>) -> Self {
            Self {
                path: Some(PathBuf::from("dossier").join(FILE_NAME)),
                facts,
                fails: false,
                opened: Mutex::new(Vec::new()),
            }
        }
        fn opened(&self) -> Vec<PathBuf> {
            self.opened.lock().unwrap().clone()
        }
    }

    impl LicencesHost for Fake {
        fn file(&self) -> Option<PathBuf> {
            self.path.clone()
        }
        fn facts(&self, _path: &Path) -> Option<FileFacts> {
            self.facts
        }
        fn open(&self, path: &Path) -> io::Result<()> {
            self.opened.lock().unwrap().push(path.to_path_buf());
            if self.fails { Err(io::Error::other("refusé")) } else { Ok(()) }
        }
    }

    const GOOD: FileFacts = FileFacts { regular: true, reparse: false, len: 436_302 };

    #[test]
    fn an_ordinary_non_empty_small_file_is_opened_by_its_computed_path() {
        let host = Fake::with(Some(GOOD));
        assert_eq!(open(&host), LicencesOpened::Opened);
        assert_eq!(host.opened(), [PathBuf::from("dossier").join("LICENCES-TIERS.txt")]);
    }

    #[test]
    fn anything_else_is_missing_and_nothing_is_launched() {
        let refused = [
            None,
            Some(FileFacts { regular: false, ..GOOD }),
            Some(FileFacts { reparse: true, ..GOOD }),
            Some(FileFacts { len: 0, ..GOOD }),
            Some(FileFacts { len: MAX_BYTES, ..GOOD }),
            Some(FileFacts { len: u64::MAX, ..GOOD }),
        ];
        for facts in refused {
            let host = Fake::with(facts);
            assert_eq!(open(&host), LicencesOpened::Missing, "{facts:?}");
            assert!(host.opened().is_empty(), "{facts:?}");
        }
        assert!(decide(Some(FileFacts { len: MAX_BYTES - 1, ..GOOD })));
        assert!(decide(Some(FileFacts { len: 1, ..GOOD })));
    }

    #[test]
    fn an_unknown_location_is_missing() {
        let mut host = Fake::with(Some(GOOD));
        host.path = None;
        assert_eq!(open(&host), LicencesOpened::Missing);
        assert!(host.opened().is_empty());
        assert_eq!(open(&InertLicences), LicencesOpened::Missing);
    }

    #[test]
    fn a_refusal_from_windows_is_failed() {
        let mut host = Fake::with(Some(GOOD));
        host.fails = true;
        assert_eq!(open(&host), LicencesOpened::Failed);
        assert_eq!(host.opened().len(), 1);
    }

    #[test]
    fn the_verbatim_prefix_goes_but_a_network_share_keeps_its_own() {
        assert_eq!(without_verbatim(r"\\?\C:\Program Files\Shinku"), r"C:\Program Files\Shinku");
        assert_eq!(without_verbatim(r"C:\Shinku"), r"C:\Shinku");
        assert_eq!(without_verbatim(r"\\?\UNC\serveur\partage"), r"\\?\UNC\serveur\partage");
    }

    #[test]
    fn the_answer_names_are_the_contract_words() {
        assert_eq!(serde_json::to_value(LicencesOpened::Opened).unwrap(), "opened");
        assert_eq!(serde_json::to_value(LicencesOpened::Missing).unwrap(), "missing");
        assert_eq!(serde_json::to_value(LicencesOpened::Failed).unwrap(), "failed");
    }

    #[cfg(unix)]
    #[test]
    fn a_real_file_is_seen_for_what_it_is_and_a_link_is_not_followed() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("vrai.txt");
        std::fs::write(&file, b"licences").unwrap();
        let link = temp.path().join("lien.txt");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let system = SystemLicences;
        assert_eq!(system.facts(&file), Some(FileFacts { regular: true, reparse: false, len: 8 }));
        assert!(!decide(system.facts(&link)), "un lien n'est pas ouvert");
        assert!(!decide(system.facts(temp.path())), "un dossier non plus");
        assert_eq!(system.facts(&temp.path().join("absent")), None);
        assert_eq!(system.file(), None, "hors Windows, rien à ouvrir");
    }
}
