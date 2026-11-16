//! Suppressions sans retour, pour `shinku.exe --oublier` : un fichier nommé,
//! un dossier vide, une arborescence entière.
//!
//! Elles reposent sur les règles du module : aucun lien n'est traversé, et ce
//! qui part est une entrée **ouverte et revérifiée** ([`Removal`]), supprimée
//! par ce même handle. Pas de Corbeille : ce sont des fichiers de Shinku
//! lui-même, qui n'ont de valeur pour personne.
//!
//! Chaque fonction répond par un [`Removed`] (ou un [`TreeReport`]) qui
//! distingue ce qu'un nouvel essai peut régler (une app tient le fichier : il
//! est « occupé ») de ce qu'il ne règle pas (un lien, un droit refusé : il est
//! « gardé »). L'attente entre deux essais n'est pas ici : elle est à
//! l'appelant, qui tient l'horloge.
//!
//! Limite, dite : Windows n'offre pas ici d'ouverture relative à un handle de
//! dossier. Chaque dossier est lu par un handle ouvert sans suivre de lien et
//! revérifié par son identifiant, puis chaque entrée est ouverte par son chemin
//! et revérifiée (nature, taille, date, identifiant) avant d'être supprimée sur
//! ce handle. Un dossier du chemin remplacé par un lien **entre** la lecture et
//! l'ouverture d'une entrée ne ferait partir que ce qui a exactement la même
//! identité, la même taille et la même date : rien d'autre.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::{
    DirReader, EntryKind, EntryMeta, Expected, PathState, ReadDirError, Removal, RemoveError,
    fold_name, identity, inspect_dir_chain,
};
use crate::model::Timestamp;

/// Ce qu'est devenue une entrée que l'on a voulu supprimer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removed {
    /// Elle est partie.
    Gone,
    /// Elle n'était pas là : rien à faire.
    Missing,
    /// Une app la tient : un nouvel essai, un peu plus tard, peut réussir.
    Busy,
    /// Elle est laissée en place, et un nouvel essai n'y changera rien : un
    /// lien, un fichier à plusieurs noms, un droit refusé, une entrée qui a
    /// changé, un dossier qui n'est pas vide.
    Kept,
}

/// Bornes d'une suppression d'arborescence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeLimits {
    /// Nombre de dossiers imbriqués sous la racine au-delà duquel on ne descend
    /// pas.
    pub max_depth: usize,
    /// Nombre d'entrées vues, en tout, au-delà duquel on s'arrête.
    pub max_entries: usize,
}

impl Default for TreeLimits {
    fn default() -> Self {
        // Le cache de WebView2 compte quelques milliers de fichiers sur dix
        // niveaux au plus : ces bornes ne gênent que ce qui n'est pas lui.
        Self { max_depth: 40, max_entries: 200_000 }
    }
}

/// Le bilan d'une suppression d'arborescence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TreeReport {
    /// Fichiers et dossiers supprimés.
    pub removed: u64,
    /// Entrées qu'une app tient : un nouvel essai peut les faire partir.
    pub busy: u64,
    /// Entrées laissées en place pour de bon (liens, bornes atteintes…).
    pub kept: u64,
}

impl TreeReport {
    /// Vrai si l'arborescence a disparu, ou n'existait pas.
    pub fn is_clean(&self) -> bool {
        self.busy == 0 && self.kept == 0
    }
}

fn removal_failed(error: &RemoveError) -> Removed {
    match error {
        RemoveError::NotFound => Removed::Missing,
        RemoveError::Locked => Removed::Busy,
        RemoveError::Changed
        | RemoveError::AccessDenied
        | RemoveError::NotEmpty
        | RemoveError::Io(_) => Removed::Kept,
    }
}

/// Supprime l'entrée ouverte, si elle est un fichier ordinaire. Un fichier à
/// plusieurs noms est refusé quand `single_name` est vrai : supprimer un nom ne
/// détruirait pas les données, et ce n'est pas ce qu'on veut d'un fichier de
/// réglages.
fn take_file(path: &Path, expected: &Expected, single_name: bool) -> Removed {
    let mut removal = match Removal::open(path, expected) {
        Ok(removal) => removal,
        Err(error) => return removal_failed(&error),
    };
    match removal.facts() {
        Ok(facts) if facts.regular && !facts.online_only && (!single_name || facts.links == 1) => {}
        Ok(_) => return Removed::Kept,
        Err(error) => return removal_failed(&error),
    }
    match removal.mark().and_then(|()| removal.commit()) {
        Ok(()) => Removed::Gone,
        Err(error) => removal_failed(&error),
    }
}

/// Supprime l'entrée ouverte, si elle est un dossier vide.
fn take_empty_dir(path: &Path, expected: &Expected) -> Result<(), RemoveError> {
    let mut removal = Removal::open(path, expected)?;
    removal.mark()?;
    removal.commit()
}

/// Ce qu'on retrouve d'un dossier : sa nature seule compte, pas sa taille ni sa
/// date (y supprimer un fichier les change).
fn dir_expectation(file_id: Option<crate::model::FileId>) -> Expected {
    Expected { kind: EntryKind::Dir, size: 0, modified: Timestamp::UNIX_EPOCH, file_id }
}

/// Supprime le fichier `name` de `dir`.
///
/// Le chemin de `dir` ne doit traverser aucun lien (voir
/// [`inspect_dir_chain`]). Le fichier doit être ordinaire, avec **un seul
/// nom**, et n'être pas « en ligne uniquement » ; il est supprimé par le handle
/// qui a servi à le contrôler. `name` est comparé sans tenir compte de la casse
/// sous Windows.
pub fn remove_plain_file(dir: &Path, name: &OsStr) -> Removed {
    match inspect_dir_chain(dir) {
        Ok(PathState::RealDir) => {}
        Ok(PathState::Missing) => return Removed::Missing,
        Ok(PathState::NotPlain) | Err(_) => return Removed::Kept,
    }
    let wanted = fold_name(name);
    let mut found: Option<(OsString, EntryMeta)> = None;
    let listed = DirReader::new().read(dir, None, &mut |entry, meta| {
        if found.is_none() && fold_name(entry) == wanted {
            found = Some((entry.to_owned(), *meta));
        }
    });
    match listed {
        Ok(()) => {}
        Err(ReadDirError::NotFound) => return Removed::Missing,
        Err(_) => return Removed::Kept,
    }
    let Some((actual, meta)) = found else { return Removed::Missing };
    if meta.kind != EntryKind::File {
        // Un lien, un dossier ou autre chose porte ce nom : ce n'est pas à nous.
        return Removed::Kept;
    }
    take_file(&dir.join(actual), &meta.expected(), true)
}

/// Supprime le dossier `dir`, **seulement s'il est vide**. Son chemin ne doit
/// traverser aucun lien, lui compris.
pub fn remove_empty_dir(dir: &Path) -> Removed {
    match inspect_dir_chain(dir) {
        Ok(PathState::RealDir) => {}
        Ok(PathState::Missing) => return Removed::Missing,
        Ok(PathState::NotPlain) | Err(_) => return Removed::Kept,
    }
    let file_id = match identity(dir) {
        Ok(id) => id.map(|id| id.file_id),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Removed::Missing,
        Err(_) => return Removed::Kept,
    };
    match take_empty_dir(dir, &dir_expectation(file_id)) {
        Ok(()) => Removed::Gone,
        Err(error) => removal_failed(&error),
    }
}

/// Supprime l'arborescence qui commence à `root`, `root` comprise.
///
/// - `root` doit être un vrai dossier : un lien (jonction, lien symbolique) ou
///   un fichier à sa place est laissé, et compté comme gardé ;
/// - un lien rencontré plus bas n'est **jamais** traversé : il est laissé en
///   place, comme tout ce qui n'est ni un fichier ni un vrai dossier, et son
///   dossier parent reste donc (il n'est pas vide) ;
/// - un fichier à plusieurs noms est supprimé comme un nom : les données
///   restent sous l'autre nom, rien n'y est touché ;
/// - la profondeur et le nombre d'entrées sont bornés ([`TreeLimits`]) ;
/// - ce qui part est supprimé par le handle qui a servi au contrôle.
///
/// Une racine absente est un succès : il n'y avait rien.
pub fn remove_tree(root: &Path, limits: TreeLimits) -> TreeReport {
    let mut walk =
        Walk { reader: DirReader::new(), limits, seen: 0, report: TreeReport::default() };
    let file_id = match identity(root) {
        Ok(id) => id.map(|id| id.file_id),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return walk.report,
        Err(_) => {
            walk.report.kept += 1;
            return walk.report;
        }
    };
    if walk.empty(root, file_id, 0) {
        walk.take_dir(root, file_id);
    }
    walk.report
}

struct Walk {
    reader: DirReader,
    limits: TreeLimits,
    seen: usize,
    report: TreeReport,
}

impl Walk {
    /// Compte le résultat d'une suppression. Vrai si l'entrée n'est plus là.
    fn count(&mut self, outcome: Removed) -> bool {
        match outcome {
            Removed::Gone => {
                self.report.removed += 1;
                true
            }
            Removed::Missing => true,
            Removed::Busy => {
                self.report.busy += 1;
                false
            }
            Removed::Kept => {
                self.report.kept += 1;
                false
            }
        }
    }

    /// Supprime un dossier qu'on vient de vider. Un dossier qui n'est plus vide
    /// (une app y a écrit entre-temps) est « occupé » : un nouvel essai le
    /// vide.
    fn take_dir(&mut self, path: &Path, file_id: Option<crate::model::FileId>) -> bool {
        let outcome = match take_empty_dir(path, &dir_expectation(file_id)) {
            Ok(()) => Removed::Gone,
            Err(RemoveError::NotEmpty) => Removed::Busy,
            Err(error) => removal_failed(&error),
        };
        self.count(outcome)
    }

    /// Vide le dossier `path`. Vrai s'il est vide à la fin (ou n'existe plus).
    fn empty(&mut self, path: &Path, file_id: Option<crate::model::FileId>, depth: usize) -> bool {
        let mut entries: Vec<(OsString, EntryMeta)> = Vec::new();
        let read = self.reader.read(path, file_id, &mut |name, meta| {
            entries.push((name.to_owned(), *meta));
        });
        match read {
            Ok(()) => {}
            Err(ReadDirError::NotFound) => return true,
            // Un lien ou un fichier à la place du dossier, un autre dossier à
            // sa place, un droit refusé : on n'y entre pas.
            Err(_) => {
                self.report.kept += 1;
                return false;
            }
        }
        let mut clean = true;
        for (name, meta) in entries {
            self.seen += 1;
            if self.seen > self.limits.max_entries {
                self.report.kept += 1;
                return false;
            }
            let child = path.join(&name);
            let gone = match meta.kind {
                EntryKind::File => {
                    let outcome = take_file(&child, &meta.expected(), false);
                    self.count(outcome)
                }
                EntryKind::Dir if meta.is_walkable_dir() => {
                    if depth + 1 >= self.limits.max_depth {
                        self.report.kept += 1;
                        false
                    } else if self.empty(&child, meta.file_id, depth + 1) {
                        self.take_dir(&child, meta.file_id)
                    } else {
                        false
                    }
                }
                // Lien, point d'analyse inconnu, périphérique : laissé en place.
                _ => {
                    self.report.kept += 1;
                    false
                }
            };
            clean &= gone;
        }
        clean
    }
}
