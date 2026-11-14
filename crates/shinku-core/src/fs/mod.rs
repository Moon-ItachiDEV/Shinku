//! Lecture du système de fichiers, sans jamais suivre un lien.
//!
//! Tout le moteur d'analyse passe par ce module. Il garantit trois choses :
//!
//! - une jonction ou un lien symbolique n'est jamais traversé, ni pour lister
//!   un dossier ni pour lire un fichier ;
//! - un fichier « en ligne uniquement » (OneDrive et autres) n'est jamais
//!   ouvert : le lire le téléchargerait ;
//! - la taille, les dates et l'identifiant d'un fichier viennent de la liste
//!   de son dossier, sans ouvrir le fichier.
//!
//! Sous Windows, l'implémentation ouvre chaque dossier par un handle et le lit
//! par blocs. L'implémentation Unix sert au développement et aux tests : elle
//! n'est pas livrée.

use std::ffi::OsStr;
use std::fmt;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::{FileId, Timestamp};

mod attributes;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

/// Nature d'une entrée de dossier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// Un fichier ordinaire.
    File,
    /// Un vrai dossier.
    Dir,
    /// Une jonction, un lien symbolique ou tout autre renvoi : jamais suivi.
    Link,
    /// Autre chose (périphérique, tube…) : jamais ouvert.
    Other,
}

/// Particularités d'une entrée, utiles aux garde-fous.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct EntryFlags(u8);

impl EntryFlags {
    /// Contenu absent du disque : le lire déclencherait un téléchargement.
    pub const ONLINE_ONLY: Self = Self(1 << 0);
    /// Attribut « caché ».
    pub const HIDDEN: Self = Self(1 << 1);
    /// Attribut « système ».
    pub const SYSTEM: Self = Self(1 << 2);
    /// Géré par un fournisseur de synchronisation (OneDrive…).
    pub const CLOUD: Self = Self(1 << 3);
    /// Point d'analyse inconnu : ni parcouru ni lu.
    pub const SPECIAL: Self = Self(1 << 4);

    /// Aucune particularité.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Vrai si toutes les particularités de `other` sont présentes.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Ajoute les particularités de `other`.
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl fmt::Debug for EntryFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = [
            (Self::ONLINE_ONLY, "ONLINE_ONLY"),
            (Self::HIDDEN, "HIDDEN"),
            (Self::SYSTEM, "SYSTEM"),
            (Self::CLOUD, "CLOUD"),
            (Self::SPECIAL, "SPECIAL"),
        ];
        let mut set = f.debug_set();
        for (flag, name) in names {
            if self.contains(flag) {
                set.entry(&format_args!("{name}"));
            }
        }
        set.finish()
    }
}

/// Ce que la liste d'un dossier dit d'une entrée, sans l'ouvrir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryMeta {
    /// Nature de l'entrée.
    pub kind: EntryKind,
    /// Particularités.
    pub flags: EntryFlags,
    /// Taille logique, en octets.
    pub size: u64,
    /// Place réellement occupée sur le disque, en octets. Un fichier « en
    /// ligne uniquement », compressé ou creux pèse moins que sa taille logique.
    pub allocated: u64,
    /// Date de création à cet emplacement.
    pub created: Timestamp,
    /// Date de dernière modification. C'est celle que Shinku affiche.
    pub modified: Timestamp,
    /// Date de dernier accès : une indication seulement, Windows peut ne pas la tenir à jour.
    pub accessed: Timestamp,
    /// Identifiant du fichier sur son volume, quand le système de fichiers en fournit un.
    pub file_id: Option<FileId>,
}

impl EntryMeta {
    /// Vrai si l'on peut entrer dans cette entrée pour la parcourir.
    pub fn is_walkable_dir(&self) -> bool {
        self.kind == EntryKind::Dir
            && !self.flags.contains(EntryFlags::SPECIAL)
            && !self.flags.contains(EntryFlags::ONLINE_ONLY)
    }

    /// Vrai si l'on peut lire le contenu de ce fichier sans effet de bord.
    pub fn is_readable_file(&self) -> bool {
        self.kind == EntryKind::File
            && !self.flags.contains(EntryFlags::SPECIAL)
            && !self.flags.contains(EntryFlags::ONLINE_ONLY)
    }

    /// Ce qu'il faut retrouver à l'identique pour considérer le fichier inchangé.
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint { size: self.size, modified: self.modified, file_id: self.file_id }
    }
}

/// Ce qui identifie un fichier et son état à un instant donné.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    /// Taille logique.
    pub size: u64,
    /// Date de dernière modification.
    pub modified: Timestamp,
    /// Identifiant sur le volume, s'il existe.
    pub file_id: Option<FileId>,
}

/// Identité d'un fichier ou d'un dossier : volume et identifiant sur ce volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    /// Numéro de série du volume.
    pub volume: u64,
    /// Identifiant sur ce volume.
    pub file_id: FileId,
}

/// Pourquoi un dossier n'a pas pu être lu.
#[derive(Debug)]
pub enum ReadDirError {
    /// Le compte n'a pas le droit de lister ce dossier.
    AccessDenied,
    /// Le dossier a disparu depuis qu'on l'a vu.
    NotFound,
    /// Ce n'est pas (ou plus) un vrai dossier : un lien l'a remplacé, par exemple.
    NotADirectory,
    /// Ce n'est plus le dossier qu'on avait vu : un autre a pris sa place.
    Changed,
    /// Autre erreur du système.
    Io(io::Error),
}

impl fmt::Display for ReadDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccessDenied => f.write_str("accès refusé"),
            Self::NotFound => f.write_str("dossier introuvable"),
            Self::NotADirectory => f.write_str("ce n'est pas un vrai dossier"),
            Self::Changed => f.write_str("le dossier a été remplacé depuis l'analyse"),
            Self::Io(error) => write!(f, "erreur du système : {error}"),
        }
    }
}

impl std::error::Error for ReadDirError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ReadDirError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::PermissionDenied => Self::AccessDenied,
            io::ErrorKind::NotFound => Self::NotFound,
            io::ErrorKind::NotADirectory => Self::NotADirectory,
            _ => Self::Io(error),
        }
    }
}

/// Lit le contenu des dossiers. Garde un tampon entre deux lectures : un
/// lecteur par fil d'exécution.
pub struct DirReader {
    inner: imp::DirReader,
}

impl DirReader {
    /// Crée un lecteur.
    pub fn new() -> Self {
        Self { inner: imp::DirReader::new() }
    }

    /// Appelle `visit` pour chaque entrée de `dir` (sauf `.` et `..`), avec son
    /// nom et ce que la liste du dossier en dit.
    ///
    /// `dir` doit être un chemin absolu, sous la forme rendue par
    /// [`to_internal_path`]. S'il désigne un lien, la lecture est refusée.
    ///
    /// `expected` est l'identifiant que la liste du dossier parent a donné
    /// pour `dir`. S'il est fourni, la lecture est refusée quand le dossier
    /// ouvert n'est pas celui-là : c'est ce qui arrive si un dossier du chemin
    /// a été remplacé par un lien entre les deux lectures.
    ///
    /// Cette vérification suffit pour une analyse, qui ne modifie rien. Une
    /// suppression avec des droits élevés devra, elle, ouvrir chaque dossier à
    /// partir du handle de son parent.
    pub fn read(
        &mut self,
        dir: &Path,
        expected: Option<FileId>,
        visit: &mut dyn FnMut(&OsStr, &EntryMeta),
    ) -> Result<(), ReadDirError> {
        self.inner.read(dir, expected, visit)
    }
}

impl Default for DirReader {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for DirReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DirReader").finish_non_exhaustive()
    }
}

/// Pourquoi un fichier n'a pas été ouvert en lecture.
#[derive(Debug)]
pub enum OpenError {
    /// Taille, date ou identifiant ne correspondent plus à l'analyse.
    Changed,
    /// Ce n'est pas un fichier ordinaire (lien, dossier, point d'analyse inconnu).
    NotRegular,
    /// Le contenu n'est pas sur le disque : on ne le télécharge pas.
    OnlineOnly,
    /// Une app tient le fichier.
    Locked,
    /// Le compte n'a pas le droit de le lire.
    AccessDenied,
    /// Le fichier a disparu.
    NotFound,
    /// Autre erreur du système.
    Io(io::Error),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed => f.write_str("le fichier a changé depuis l'analyse"),
            Self::NotRegular => f.write_str("ce n'est pas un fichier ordinaire"),
            Self::OnlineOnly => f.write_str("le fichier n'est disponible qu'en ligne"),
            Self::Locked => f.write_str("le fichier est utilisé par une app"),
            Self::AccessDenied => f.write_str("accès refusé"),
            Self::NotFound => f.write_str("fichier introuvable"),
            Self::Io(error) => write!(f, "erreur du système : {error}"),
        }
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// Un fichier ouvert en lecture seule, dont on a vérifié sur le handle qu'il
/// est bien celui que l'analyse a vu.
#[derive(Debug)]
pub struct VerifiedFile {
    file: File,
    expected: Fingerprint,
}

impl VerifiedFile {
    /// Ouvre `path` sans suivre de lien et sans gêner les autres apps, puis
    /// vérifie sur le fichier ouvert qu'il s'agit d'un fichier ordinaire,
    /// présent sur le disque, de même taille, même date et même identifiant
    /// que `expected`.
    pub fn open(path: &Path, expected: Fingerprint) -> Result<Self, OpenError> {
        let file = imp::open_verified(path, &expected)?;
        Ok(Self { file, expected })
    }

    /// Le fichier, pour le lire.
    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    /// Revérifie le fichier après lecture : s'il a été modifié pendant qu'on
    /// le lisait, ce qu'on a lu ne vaut rien.
    pub fn recheck(&self) -> Result<(), OpenError> {
        imp::check_open_file(&self.file, &self.expected)
    }
}

/// Identité (volume et identifiant) de ce que désigne `path`, sans suivre un
/// lien final. `None` si le système de fichiers ne fournit pas d'identifiant.
pub fn identity(path: &Path) -> io::Result<Option<Identity>> {
    imp::identity(path)
}

/// Compare deux identifiants de fichier en tenant compte de leur origine : la
/// liste d'un dossier peut n'en donner que les 64 bits de poids faible, là où
/// une question directe au fichier en donne 128.
pub fn same_file_id(left: FileId, right: FileId) -> bool {
    left == right || (left.0 >> 64 == 0 || right.0 >> 64 == 0) && left.0 as u64 == right.0 as u64
}

/// Ce qu'on trouve au bout d'un chemin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathState {
    /// Un vrai dossier, atteint sans traverser aucun lien.
    RealDir,
    /// Un des dossiers du chemin est un lien (jonction, lien symbolique) ou
    /// n'est pas un dossier : on n'y va pas.
    NotPlain,
    /// Le chemin n'existe pas.
    Missing,
}

/// Vérifie qu'un chemin interne mène à un vrai dossier sans passer par aucun
/// lien, en examinant chaque dossier du chemin depuis la racine du volume.
///
/// Les emplacements que le moteur reçoit de l'extérieur (dossiers connus,
/// `%TEMP%`, dossiers choisis) passent par ici avant toute lecture : ouvrir
/// `C:\a\b\c` sans suivre de lien ne protège que le dernier composant.
pub fn inspect_dir_chain(path: &Path) -> io::Result<PathState> {
    let mut current = PathBuf::new();
    let mut reached_root = false;
    for component in path.components() {
        current.push(component);
        match component {
            std::path::Component::Prefix(_) => continue,
            std::path::Component::RootDir => reached_root = true,
            std::path::Component::Normal(_) => {}
            _ => return Ok(PathState::NotPlain),
        }
        match imp::is_real_dir(&current) {
            Ok(true) => {}
            Ok(false) => return Ok(PathState::NotPlain),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(PathState::Missing),
            Err(error) => return Err(error),
        }
    }
    Ok(if reached_root { PathState::RealDir } else { PathState::NotPlain })
}

/// Met un chemin absolu sous la forme que le moteur utilise en interne.
///
/// Sous Windows, c'est la forme `\\?\C:\…`. Elle lève la limite des 260
/// caractères et désactive les réécritures de Win32 : un dossier nommé
/// `photos.` reste `photos.` et n'est jamais confondu avec `photos`.
///
/// Renvoie une erreur si le chemin n'est pas absolu.
pub fn to_internal_path(path: &Path) -> io::Result<PathBuf> {
    imp::to_internal_path(path)
}

/// Remet un chemin interne sous sa forme habituelle, pour l'afficher.
pub fn to_display_path(path: &Path) -> PathBuf {
    imp::to_display_path(path)
}

/// Plie un nom de fichier pour le comparer comme le fait le système de
/// fichiers : sans tenir compte de la casse sous Windows, tel quel ailleurs.
pub fn fold_name(name: &OsStr) -> String {
    let text = name.to_string_lossy();
    if cfg!(windows) { text.to_lowercase() } else { text.into_owned() }
}

/// Composants d'un chemin absolu, pliés pour la comparaison. Le lecteur
/// (`C:`) ou le partage réseau compte pour un composant ; les formes `C:\…`
/// et `\\?\C:\…` donnent le même résultat.
///
/// Renvoie `None` si le chemin est relatif ou contient `.` ou `..`.
pub fn fold_path(path: &Path) -> Option<Vec<String>> {
    use std::path::Component;

    let mut parts = Vec::new();
    let mut absolute = false;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => parts.push(imp::fold_prefix(&prefix)?),
            Component::RootDir => absolute = true,
            Component::Normal(name) => parts.push(fold_name(name)),
            Component::CurDir | Component::ParentDir => return None,
        }
    }
    absolute.then_some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_cannot_be_folded() {
        assert_eq!(fold_path(Path::new("relatif/chemin")), None);
        assert_eq!(fold_path(Path::new("")), None);
    }

    #[cfg(unix)]
    #[test]
    fn dot_components_are_rejected() {
        assert_eq!(fold_path(Path::new("/a/../b")), None);
        assert_eq!(fold_path(Path::new("/a/b")), Some(vec!["a".to_owned(), "b".to_owned()]));
        assert_eq!(fold_path(Path::new("/")), Some(vec![]));
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_fold_to_the_same_key() {
        let expected = Some(vec!["c:".to_owned(), "users".to_owned(), "léa".to_owned()]);
        assert_eq!(fold_path(Path::new(r"C:\Users\Léa")), expected);
        assert_eq!(fold_path(Path::new(r"\\?\c:\USERS\LÉA")), expected);
        assert_eq!(fold_path(Path::new(r"C:Users")), None);
        assert_eq!(
            fold_path(Path::new(r"\\Serveur\Partage\Dossier")),
            fold_path(Path::new(r"\\?\UNC\serveur\partage\dossier")),
        );
        assert_eq!(fold_path(Path::new(r"C:\a\..\b")), None);
    }
}
