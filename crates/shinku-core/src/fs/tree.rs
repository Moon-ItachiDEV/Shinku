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
//! identité, la même taille et la même date : rien d'autre, **sur un système de
//! fichiers qui donne un identifiant** (NTFS, ReFS). Sans identifiant (FAT,
//! exFAT), rien n'est supprimé par ces fonctions : une entrée sans identité reste.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::{
    DirReader, EntryKind, EntryMeta, Expected, PathState, ReadDirError, Removal, RemovalFacts,
    RemoveError, fold_name, identity, inspect_dir_chain,
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
        // niveaux au plus : ces bornes ne gênent que ce qui n'est pas lui. Le
        // nombre d'entrées est aussi une borne de temps (chaque entrée coûte
        // une ouverture, quelques lectures, une suppression et une fermeture) :
        // 50 000 laissent dix fois la marge du vrai cache, et l'échéance de
        // l'appelant reste la vraie borne.
        Self { max_depth: 40, max_entries: 50_000 }
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

/// Ce que l'entrée ouverte doit être pour partir : un fichier ordinaire, présent
/// sur le disque, et (pour un fichier de réglages) avec un seul nom.
fn acceptable(facts: &RemovalFacts, single_name: bool) -> bool {
    facts.regular && !facts.online_only && (!single_name || facts.links == 1)
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
        Ok(facts) if acceptable(&facts, single_name) => {}
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
    if plan_for(&actual, &meta) != Plan::File {
        // Un lien, un dossier ou autre chose porte ce nom : ce n'est pas à nous.
        // Sans identifiant de fichier (FAT, exFAT), on ne peut pas vérifier que
        // c'est le même fichier au moment de le supprimer : il reste.
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
        Ok(Some(id)) => Some(id.file_id),
        // Sans identifiant, pas de vérification possible : il reste.
        Ok(None) => return Removed::Kept,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Removed::Missing,
        Err(_) => return Removed::Kept,
    };
    match take_empty_dir(dir, &dir_expectation(file_id)) {
        Ok(()) => Removed::Gone,
        Err(error) => removal_failed(&error),
    }
}

/// Ce que `remove_tree` va faire d'une entrée, décidé sur ce que la liste du
/// dossier en dit (pur : essayé sans disque).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plan {
    File,
    Dir,
    Keep,
}

/// Un nom qu'on peut joindre à un chemin : ni vide, ni `.`, ni `..`, sans
/// séparateur. Un système de fichiers n'en rend pas ; c'est une défense.
fn usable_name(name: &OsStr) -> bool {
    let text = name.to_string_lossy();
    !text.is_empty() && text != "." && text != ".." && !text.contains(['/', '\\'])
}

fn plan_for(name: &OsStr, meta: &EntryMeta) -> Plan {
    // Sans identifiant de fichier (FAT, exFAT), rien ne permet de vérifier que
    // l'entrée est la même au moment de la supprimer, et un dossier ne se
    // reconnaît qu'à sa nature : « sans identité, pas effaçable ».
    if !usable_name(name) || meta.file_id.is_none() {
        return Plan::Keep;
    }
    match meta.kind {
        EntryKind::File => Plan::File,
        EntryKind::Dir if meta.is_walkable_dir() => Plan::Dir,
        // Lien, point d'analyse inconnu, périphérique : laissé en place.
        _ => Plan::Keep,
    }
}

/// Un instant de la suppression d'une arborescence, que les essais utilisent
/// pour glisser un changement entre deux gestes.
// Les chemins ne sont lus que par les essais.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step<'a> {
    /// Juste avant de lire ce dossier (son identifiant est déjà connu).
    BeforeRead(&'a Path),
    /// Juste avant de supprimer ce dossier qu'on vient de vider.
    BeforeRemove(&'a Path),
}

/// Supprime l'arborescence qui commence à `root`, `root` comprise.
///
/// - le chemin **au-dessus** de `root` ne doit traverser aucun lien (comme pour
///   un fichier nommé) : un profil derrière une jonction est refusé en entier ;
/// - `root` doit être un vrai dossier : un lien (jonction, lien symbolique) ou
///   un fichier à sa place est laissé, et compté comme gardé ;
/// - un lien rencontré plus bas n'est **jamais** traversé : il est laissé en
///   place, comme tout ce qui n'est ni un fichier ni un vrai dossier, et son
///   dossier parent reste donc (il n'est pas vide) ;
/// - une entrée sans identifiant de fichier, ou au nom impossible, est laissée ;
/// - un fichier à plusieurs noms est supprimé comme un nom : les données
///   restent sous l'autre nom, rien n'y est touché ;
/// - la profondeur et le nombre d'entrées sont bornés ([`TreeLimits`]) ;
/// - `expired` est regardée entre deux entrées : dès qu'elle dit vrai, on
///   s'arrête et le reste est compté comme gardé ;
/// - ce qui part est supprimé par le handle qui a servi au contrôle.
///
/// Une racine absente est un succès : il n'y avait rien.
///
/// Limite : un dossier est lu d'un seul bloc par le système. Sa liste n'est
/// gardée en mémoire que dans la limite des entrées permises, mais la lecture
/// d'un dossier immense n'est pas interrompue par `expired`.
pub fn remove_tree(root: &Path, limits: TreeLimits, expired: &dyn Fn() -> bool) -> TreeReport {
    remove_tree_watched(root, limits, expired, &mut |_| {})
}

pub(crate) fn remove_tree_watched(
    root: &Path,
    limits: TreeLimits,
    expired: &dyn Fn() -> bool,
    watch: &mut dyn FnMut(Step<'_>),
) -> TreeReport {
    let mut walk = Walk {
        reader: DirReader::new(),
        limits,
        expired,
        watch,
        stopped: false,
        seen: 0,
        report: TreeReport::default(),
    };
    // Le chemin au-dessus de la racine : aucun lien.
    match root.parent().map(inspect_dir_chain) {
        Some(Ok(PathState::RealDir)) => {}
        Some(Ok(PathState::Missing)) => return walk.report,
        _ => {
            walk.report.kept += 1;
            return walk.report;
        }
    }
    let file_id = match identity(root) {
        Ok(Some(id)) => id.file_id,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return walk.report,
        // Sans identifiant, ou illisible : on n'y touche pas.
        Ok(None) | Err(_) => {
            walk.report.kept += 1;
            return walk.report;
        }
    };
    if walk.empty(root, file_id, 0) {
        walk.take_dir(root, file_id);
    }
    walk.report
}

struct Walk<'a> {
    reader: DirReader,
    limits: TreeLimits,
    expired: &'a dyn Fn() -> bool,
    watch: &'a mut dyn FnMut(Step<'_>),
    /// L'échéance est passée : on ne fait plus rien.
    stopped: bool,
    seen: usize,
    report: TreeReport,
}

impl Walk<'_> {
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

    /// Vrai si l'échéance est passée (comptée une seule fois comme « gardé »).
    fn out_of_time(&mut self) -> bool {
        if !self.stopped && (self.expired)() {
            self.stopped = true;
            self.report.kept += 1;
        }
        self.stopped
    }

    /// Supprime un dossier qu'on vient de vider. Un dossier qui n'est plus vide
    /// (une app y a écrit entre-temps) est « occupé » : un nouvel essai le
    /// vide. Il n'est **jamais** compté comme parti.
    fn take_dir(&mut self, path: &Path, file_id: crate::model::FileId) -> bool {
        (self.watch)(Step::BeforeRemove(path));
        let outcome = match take_empty_dir(path, &dir_expectation(Some(file_id))) {
            Ok(()) => Removed::Gone,
            Err(RemoveError::NotEmpty) => Removed::Busy,
            Err(error) => removal_failed(&error),
        };
        self.count(outcome)
    }

    /// Vide le dossier `path`, que la liste de son parent (ou `identity`) a
    /// donné pour `file_id`. Vrai s'il est vide à la fin (ou n'existe plus).
    fn empty(&mut self, path: &Path, file_id: crate::model::FileId, depth: usize) -> bool {
        if self.out_of_time() {
            return false;
        }
        (self.watch)(Step::BeforeRead(path));
        // On ne garde en mémoire que ce que la borne permet de voir encore.
        let room = self.limits.max_entries.saturating_sub(self.seen);
        let mut entries: Vec<(OsString, EntryMeta)> = Vec::new();
        let mut truncated = false;
        let read = self.reader.read(path, Some(file_id), &mut |name, meta| {
            if entries.len() < room {
                entries.push((name.to_owned(), *meta));
            } else {
                truncated = true;
            }
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
        let mut clean = !truncated;
        if truncated {
            self.report.kept += 1;
        }
        for (name, meta) in entries {
            self.seen += 1;
            if self.out_of_time() {
                return false;
            }
            let child = path.join(&name);
            let gone = match (plan_for(&name, &meta), meta.file_id) {
                (Plan::File, _) => {
                    let outcome = take_file(&child, &meta.expected(), false);
                    self.count(outcome)
                }
                (Plan::Dir, Some(child_id)) => {
                    if depth + 1 >= self.limits.max_depth {
                        self.report.kept += 1;
                        false
                    } else if self.empty(&child, child_id, depth + 1) {
                        self.take_dir(&child, child_id)
                    } else {
                        false
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::EntryFlags;
    use crate::model::FileId;

    fn meta(kind: EntryKind, id: Option<u128>) -> EntryMeta {
        EntryMeta {
            kind,
            flags: EntryFlags::empty(),
            size: 0,
            allocated: 0,
            created: Timestamp::UNIX_EPOCH,
            modified: Timestamp::UNIX_EPOCH,
            accessed: Timestamp::UNIX_EPOCH,
            file_id: id.map(FileId),
        }
    }

    #[test]
    fn a_file_and_a_real_folder_with_an_identity_are_taken() {
        assert_eq!(plan_for("a".as_ref(), &meta(EntryKind::File, Some(1))), Plan::File);
        assert_eq!(plan_for("a".as_ref(), &meta(EntryKind::Dir, Some(1))), Plan::Dir);
    }

    #[test]
    fn without_an_identity_nothing_is_taken() {
        assert_eq!(plan_for("a".as_ref(), &meta(EntryKind::File, None)), Plan::Keep);
        assert_eq!(plan_for("a".as_ref(), &meta(EntryKind::Dir, None)), Plan::Keep);
    }

    #[test]
    fn links_and_other_things_are_never_taken() {
        assert_eq!(plan_for("a".as_ref(), &meta(EntryKind::Link, Some(1))), Plan::Keep);
        assert_eq!(plan_for("a".as_ref(), &meta(EntryKind::Other, Some(1))), Plan::Keep);
        let mut special = meta(EntryKind::Dir, Some(1));
        special.flags = EntryFlags::SPECIAL;
        assert_eq!(plan_for("a".as_ref(), &special), Plan::Keep);
    }

    #[test]
    fn an_impossible_name_is_never_joined_to_a_path() {
        for name in ["", ".", "..", "a/b", "a\\b", "/", "\\x", "../x"] {
            assert_eq!(
                plan_for(name.as_ref(), &meta(EntryKind::File, Some(1))),
                Plan::Keep,
                "{name:?}"
            );
        }
        assert!(usable_name("...".as_ref()), "trois points sont un nom comme un autre");
        assert!(usable_name("Local State".as_ref()));
    }

    #[test]
    fn only_a_plain_present_file_with_one_name_is_acceptable() {
        let facts = |links, online_only, regular| RemovalFacts { links, online_only, regular };
        assert!(acceptable(&facts(1, false, true), true));
        assert!(!acceptable(&facts(2, false, true), true), "deux noms : pas un réglage");
        assert!(acceptable(&facts(2, false, true), false), "deux noms : un nom du cache part");
        assert!(!acceptable(&facts(1, true, true), true), "en ligne seulement");
        assert!(!acceptable(&facts(1, true, true), false));
        assert!(!acceptable(&facts(1, false, false), true), "ni lien ni point d'analyse");
        assert!(!acceptable(&facts(1, false, false), false));
    }

    // --- Sur de vrais dossiers, avec un changement glissé entre deux gestes ---

    use std::cell::Cell;
    use std::fs;

    fn workspace() -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let real = fs::canonicalize(temp.path()).unwrap();
        let root = crate::fs::to_internal_path(&real).unwrap();
        (temp, root)
    }

    fn never() -> bool {
        false
    }

    /// `root/dossier/{a.bin,b.bin}`, `root/haut.bin`.
    fn small_tree(root: &Path) -> std::path::PathBuf {
        let tree = root.join("arbre");
        fs::create_dir_all(tree.join("dossier")).unwrap();
        fs::write(tree.join("dossier").join("a.bin"), b"a").unwrap();
        fs::write(tree.join("dossier").join("b.bin"), b"b").unwrap();
        fs::write(tree.join("haut.bin"), b"h").unwrap();
        tree
    }

    /// Remplace `path` par un autre dossier, qui contient un témoin.
    fn swap_for_another(path: &Path) {
        let parked = path
            .with_file_name(format!("{}-mis-de-cote", path.file_name().unwrap().to_string_lossy()));
        fs::rename(path, &parked).unwrap();
        fs::create_dir(path).unwrap();
        fs::write(path.join("temoin.txt"), b"precieux").unwrap();
    }

    #[test]
    fn a_root_replaced_after_its_identity_was_read_is_not_entered() {
        let (_temp, root) = workspace();
        let tree = small_tree(&root);
        let mut done = false;
        let report = remove_tree_watched(&tree, TreeLimits::default(), &never, &mut |step| {
            if let Step::BeforeRead(path) = step
                && path == tree
                && !done
            {
                done = true;
                swap_for_another(&tree);
            }
        });
        assert!(!report.is_clean(), "{report:?}");
        assert_eq!(report.removed, 0);
        assert_eq!(fs::read(tree.join("temoin.txt")).unwrap(), b"precieux");
    }

    #[test]
    fn a_folder_replaced_after_its_parent_was_listed_is_not_entered() {
        let (_temp, root) = workspace();
        let tree = small_tree(&root);
        let child = tree.join("dossier");
        let report = remove_tree_watched(&tree, TreeLimits::default(), &never, &mut |step| {
            if matches!(step, Step::BeforeRead(path) if path == child) {
                swap_for_another(&child);
            }
        });
        assert!(!report.is_clean(), "{report:?}");
        assert_eq!(fs::read(child.join("temoin.txt")).unwrap(), b"precieux");
        assert!(!tree.join("haut.bin").exists(), "le reste est parti");
    }

    #[test]
    fn a_folder_that_vanishes_before_its_read_is_not_a_failure() {
        let (_temp, root) = workspace();
        let tree = small_tree(&root);
        let child = tree.join("dossier");
        let report = remove_tree_watched(&tree, TreeLimits::default(), &never, &mut |step| {
            if matches!(step, Step::BeforeRead(path) if path == child) {
                fs::remove_dir_all(&child).unwrap();
            }
        });
        assert!(report.is_clean(), "{report:?}");
        assert!(!tree.exists(), "la racine part aussi");
    }

    #[test]
    fn a_folder_refilled_before_its_removal_stays_and_is_never_counted_gone() {
        let (_temp, root) = workspace();
        let tree = small_tree(&root);
        let child = tree.join("dossier");
        let report = remove_tree_watched(&tree, TreeLimits::default(), &never, &mut |step| {
            if matches!(step, Step::BeforeRemove(path) if path == child) {
                fs::write(child.join("nouveau.bin"), b"ecrit entre-temps").unwrap();
            }
        });
        assert_eq!(report.busy, 1, "{report:?}");
        assert!(!report.is_clean(), "un dossier qui reste n'est pas « parti »");
        assert!(child.join("nouveau.bin").exists());
        assert!(tree.exists(), "la racine reste : elle n'est pas vide");
    }

    #[test]
    fn the_deadline_stops_the_walk_between_two_entries() {
        let (_temp, root) = workspace();
        let tree = root.join("arbre");
        fs::create_dir(&tree).unwrap();
        for index in 0..10 {
            fs::write(tree.join(format!("f{index}.bin")), b"x").unwrap();
        }
        let asked = Cell::new(0);
        let expired = || {
            asked.set(asked.get() + 1);
            asked.get() > 4
        };
        let report = remove_tree(&tree, TreeLimits::default(), &expired);
        assert!(!report.is_clean());
        assert!(report.removed < 10, "{report:?}");
        assert_eq!(report.kept, 1, "l'échéance est comptée une fois : {report:?}");
        let left = fs::read_dir(&tree).unwrap().count();
        assert_eq!(left as u64, 10 - report.removed);
        // Passée, elle n'est plus interrogée : plus rien ne se fait.
        assert!(asked.get() <= 6, "{}", asked.get());
    }

    #[test]
    fn an_expired_deadline_touches_nothing() {
        let (_temp, root) = workspace();
        let tree = small_tree(&root);
        let report = remove_tree(&tree, TreeLimits::default(), &|| true);
        assert_eq!(report.removed, 0);
        assert!(!report.is_clean());
        assert!(tree.join("haut.bin").exists());
    }
}
