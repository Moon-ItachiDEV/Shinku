//! Effacer : les décisions, séparées de la machine.
//!
//! - **Les fichiers d'un profil** ([`erase_lot`]) : tout ou rien, avec
//!   [`crate::fs::Removal`] et les quatre étapes de la conception. La base d'un
//!   historique SQLite ne part jamais sans ses annexes, ni les annexes sans leur
//!   base : `partial` n'existe pas pour les fichiers (critique C1).
//! - **Le registre** ([`erase_typed`]) : les valeurs de trois clés, l'index
//!   d'abord, la clé reste, puis relecture : ce qui reste ou revient donne
//!   `partial`. Toute la décision est ici ; l'hôte ne fait que lister et supprimer.
//! - **Les récents** ([`erase_recent`]) et **le presse-papiers**
//!   ([`erase_clipboard`]) : un appel de Windows, dont le résultat est relu.

use std::path::PathBuf;

use crate::fs::{Expected, Removal, RemoveError};
use crate::privacy::host::{
    CurrentClear, HistoryClear, PrivacyHost, RegistryError, RegistryValues,
};
use crate::privacy::rules::{self, TypedKey, ValueName};
use crate::privacy::view::TraceOutcome;

// --- Les fichiers : tout ou rien ---

/// Un fichier du lot : son nom (celui de la règle), son chemin, ce que la liste du dossier en a vu.
#[derive(Clone, Debug)]
pub struct LotFile {
    /// Le nom du fichier, tel que la règle le donne.
    pub name: String,
    /// Son chemin, sous forme interne.
    pub path: PathBuf,
    /// Ce qu'il faut retrouver, sur le fichier ouvert, pour le supprimer.
    pub expected: Expected,
}

/// Ouvre un fichier du lot pour le supprimer.
pub type Opener<'a> = &'a dyn Fn(&LotFile) -> Result<Removal, RemoveError>;

/// L'ouverture réelle : sans aucun partage, sans suivre de lien, vérifiée.
pub fn open_real(file: &LotFile) -> Result<Removal, RemoveError> {
    Removal::open(&file.path, &file.expected)
}

/// Ce qu'une ouverture ratée dit du lot.
fn opening_failed(error: &RemoveError) -> TraceOutcome {
    match error {
        RemoveError::Locked => TraceOutcome::InUse,
        // Le fichier n'est plus ce que la liste a vu : la page doit regarder de nouveau.
        RemoveError::Changed | RemoveError::NotFound => TraceOutcome::Changed,
        RemoveError::AccessDenied | RemoveError::NotEmpty | RemoveError::Io(_) => {
            TraceOutcome::Failed
        }
    }
}

/// Efface tout le lot, ou rien.
///
/// 1. aucun nom interdit ([`rules::is_forbidden`]) ;
/// 2. **tout le lot est ouvert** sans partage ni lien ; chaque fichier doit être
///    ordinaire, n'avoir **qu'un seul nom** et ne pas être « en ligne
///    uniquement ». Une seule ouverture qui échoue : on lâche tout, rien n'est touché ;
/// 3. poignées tenues, `closed` revérifie que le navigateur est fermé ;
/// 4. **tout est marqué** (la base d'abord), puis **tout est confirmé** (les
///    annexes, puis la base). Si un marquage échoue, lâcher les poignées retire
///    les demandes : rien ne part.
pub fn erase_lot(lot: &[LotFile], open: Opener<'_>, closed: &dyn Fn() -> bool) -> TraceOutcome {
    if lot.is_empty() {
        return TraceOutcome::Nothing;
    }
    if lot.iter().any(|file| rules::is_forbidden(&file.name)) {
        return TraceOutcome::Failed;
    }
    let mut open_files: Vec<Removal> = Vec::with_capacity(lot.len());
    for file in lot {
        let removal = match open(file) {
            Ok(removal) => removal,
            Err(error) => return opening_failed(&error),
        };
        match removal.facts() {
            Ok(facts) if facts.regular && facts.links == 1 && !facts.online_only => {}
            Ok(_) => return TraceOutcome::Failed,
            Err(error) => return opening_failed(&error),
        }
        open_files.push(removal);
    }
    // Les poignées sont tenues : le navigateur ne peut plus ouvrir ces fichiers.
    // C'est maintenant qu'on est sûr qu'il est fermé.
    if !closed() {
        return TraceOutcome::InUse;
    }
    for removal in &mut open_files {
        if let Err(error) = removal.mark() {
            // `open_files` est lâché en sortant : les demandes déjà posées sont retirées.
            return match error {
                RemoveError::Locked => TraceOutcome::InUse,
                _ => TraceOutcome::Failed,
            };
        }
    }
    // Tout est marqué : même un arrêt brutal ici supprime l'ensemble, Windows
    // fermant les poignées avec la demande posée. Confirmé dans l'ordre inverse.
    let mut failed = false;
    while let Some(removal) = open_files.pop() {
        if removal.commit().is_err() {
            failed = true;
        }
    }
    if failed { TraceOutcome::Failed } else { TraceOutcome::Erased }
}

// --- Le registre ---

/// Ce qui est arrivé à une clé.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Absente, ou déjà vide : rien à faire.
    Nothing,
    /// Vidée, et relue vide.
    Erased,
    /// Des valeurs sont restées ou revenues.
    Partial,
    /// Rien n'a pu être fait (lien, accès refusé).
    Failed,
}

/// L'ordre de suppression : la valeur d'index d'abord, puis les autres telles
/// qu'énumérées.
pub fn deletion_order(key: TypedKey, names: &[ValueName]) -> Vec<ValueName> {
    let mut ordered: Vec<ValueName> =
        names.iter().filter(|name| name.is_index_of(key)).cloned().collect();
    ordered.extend(names.iter().filter(|name| !name.is_index_of(key)).cloned());
    ordered
}

/// Ce qui reste d'une clé après coup : des valeurs encore là, ou une liste incomplète.
fn left_over(after: &RegistryValues) -> bool {
    !after.names.is_empty() || !after.complete
}

/// Vide une clé : toutes les valeurs énumérées sont supprimées **par nom**,
/// l'index d'abord ; la clé reste. Puis la clé est **relue** : ce qui reste, ou
/// ce que l'Explorateur a réécrit entre-temps, donne [`KeyOutcome::Partial`].
pub fn erase_key(host: &dyn PrivacyHost, key: TypedKey) -> KeyOutcome {
    let first = match host.registry_values(key) {
        Err(_) => return KeyOutcome::Failed,
        Ok(None) => return KeyOutcome::Nothing,
        Ok(Some(values)) => values,
    };
    if first.names.is_empty() && first.complete {
        return KeyOutcome::Nothing;
    }
    let mut deleted = 0usize;
    for name in deletion_order(key, &first.names) {
        match host.registry_delete(key, &name) {
            Ok(()) => deleted += 1,
            // Devenue un lien entre-temps : on s'arrête net.
            Err(RegistryError::Link) => return KeyOutcome::Failed,
            // Une valeur qui résiste : les autres sont supprimées, la relecture dira.
            Err(RegistryError::Io(_)) => {}
        }
    }
    match host.registry_values(key) {
        Ok(None) => KeyOutcome::Erased,
        Ok(Some(after)) if !left_over(&after) => KeyOutcome::Erased,
        Ok(Some(_)) | Err(_) if deleted > 0 => KeyOutcome::Partial,
        Ok(Some(_)) | Err(_) => KeyOutcome::Failed,
    }
}

/// Ce que valent les trois clés ensemble.
pub fn combine(outcomes: &[KeyOutcome]) -> TraceOutcome {
    let has = |wanted: KeyOutcome| outcomes.contains(&wanted);
    if has(KeyOutcome::Partial) {
        TraceOutcome::Partial
    } else if has(KeyOutcome::Failed) {
        // Une clé vidée et une autre refusée : une partie seulement est partie.
        if has(KeyOutcome::Erased) { TraceOutcome::Partial } else { TraceOutcome::Failed }
    } else if has(KeyOutcome::Erased) {
        TraceOutcome::Erased
    } else {
        TraceOutcome::Nothing
    }
}

/// Efface ce que la personne a tapé : `RunMRU`, `TypedPaths`, `WordWheelQuery`.
pub fn erase_typed(host: &dyn PrivacyHost) -> TraceOutcome {
    let outcomes: Vec<KeyOutcome> = TypedKey::ALL.iter().map(|key| erase_key(host, *key)).collect();
    combine(&outcomes)
}

// --- Les récents et le presse-papiers ---

/// Efface les fichiers et dossiers récents, puis relit leur nombre : un
/// appel qui ne rend rien ne prouve rien. Zéro : `erased` ; moins qu'avant :
/// `partial` ; autant ou plus (ou illisible) : `failed`.
pub fn erase_recent(host: &dyn PrivacyHost) -> TraceOutcome {
    let Ok(before) = host.recent_count() else { return TraceOutcome::Failed };
    if before == 0 {
        return TraceOutcome::Nothing;
    }
    if host.clear_recent().is_err() {
        return TraceOutcome::Failed;
    }
    match host.recent_count() {
        Ok(0) => TraceOutcome::Erased,
        // Une partie est partie, une autre reste : « pas pu être effacé » serait faux.
        Ok(left) if left < before => TraceOutcome::Partial,
        _ => TraceOutcome::Failed,
    }
}

/// Vide l'historique du presse-papiers puis son contenu actuel. Un historique
/// désactivé est un cas normal ; `ClearHistory` faux alors qu'il est actif,
/// non.
pub fn erase_clipboard(host: &dyn PrivacyHost, owner: isize) -> TraceOutcome {
    let result = host.clear_clipboard(owner);
    match (result.history, result.current) {
        (HistoryClear::Failed, _) | (_, CurrentClear::Failed) => TraceOutcome::Failed,
        (_, CurrentClear::Busy) => TraceOutcome::InUse,
        (HistoryClear::Cleared | HistoryClear::Disabled, CurrentClear::Emptied) => {
            TraceOutcome::Erased
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::fake::{FakePrivacyHost, RecentClear};
    use crate::privacy::host::ClipboardClear;

    fn text(names: &[ValueName]) -> Vec<String> {
        names.iter().map(|n| String::from_utf16_lossy(&n.0)).collect()
    }

    #[test]
    fn the_index_value_is_deleted_first_then_the_others_as_listed() {
        let names: Vec<ValueName> =
            ["a", "b", "MRUList", "c"].iter().map(|n| ValueName::new(n)).collect();
        assert_eq!(text(&deletion_order(TypedKey::RunMru, &names)), ["MRUList", "a", "b", "c"]);
        let names: Vec<ValueName> =
            ["url1", "mruListEx", "url2"].iter().map(|n| ValueName::new(n)).collect();
        assert_eq!(
            text(&deletion_order(TypedKey::WordWheelQuery, &names)),
            ["mruListEx", "url1", "url2"]
        );
        // `TypedPaths` n'a pas d'index : l'ordre est celui de l'énumération.
        assert_eq!(
            text(&deletion_order(TypedKey::TypedPaths, &names)),
            ["url1", "mruListEx", "url2"]
        );
    }

    #[test]
    fn a_key_is_emptied_by_name_index_first_and_stays() {
        let host = FakePrivacyHost::new(None);
        host.set_values(TypedKey::RunMru, &["a", "b", "MRUList", "c"]);
        assert_eq!(erase_key(&host, TypedKey::RunMru), KeyOutcome::Erased);
        let deleted = host.deleted();
        assert_eq!(deleted[0], (TypedKey::RunMru, "MRUList".to_owned()));
        assert_eq!(deleted.len(), 4);
        assert!(host.remaining(TypedKey::RunMru).is_empty());
        // La clé est toujours là : relue, elle existe et elle est vide.
        assert!(host.registry_values(TypedKey::RunMru).unwrap().is_some());
        // Seule cette clé a été touchée.
        assert!(deleted.iter().all(|(key, _)| *key == TypedKey::RunMru));
    }

    #[test]
    fn an_absent_or_empty_key_has_nothing_to_erase() {
        let host = FakePrivacyHost::new(None);
        assert_eq!(erase_key(&host, TypedKey::RunMru), KeyOutcome::Nothing);
        host.set_values(TypedKey::TypedPaths, &[]);
        assert_eq!(erase_key(&host, TypedKey::TypedPaths), KeyOutcome::Nothing);
        assert!(host.deleted().is_empty());
        assert_eq!(erase_typed(&host), TraceOutcome::Nothing);
    }

    #[test]
    fn values_that_come_back_make_it_partial_never_erased() {
        let host = FakePrivacyHost::new(None);
        host.set_values(TypedKey::TypedPaths, &["url1", "url2"]);
        host.respawn_after_delete(TypedKey::TypedPaths, &["url1", "url2"]);
        assert_eq!(erase_key(&host, TypedKey::TypedPaths), KeyOutcome::Partial);
        assert_eq!(host.remaining(TypedKey::TypedPaths), ["url1"]);
        let host = FakePrivacyHost::new(None);
        host.set_values(TypedKey::TypedPaths, &["url1", "url2"]);
        host.respawn_after_delete(TypedKey::TypedPaths, &["url1"]);
        assert_eq!(erase_typed(&host), TraceOutcome::Partial);
    }

    #[test]
    fn a_value_that_resists_makes_it_partial_and_the_others_still_go() {
        let host = FakePrivacyHost::new(None);
        host.set_values(TypedKey::RunMru, &["a", "b", "MRUList"]);
        host.refuse_delete(TypedKey::RunMru, "b");
        assert_eq!(erase_key(&host, TypedKey::RunMru), KeyOutcome::Partial);
        assert_eq!(host.remaining(TypedKey::RunMru), ["b"]);
    }

    #[test]
    fn nothing_deleted_and_something_left_is_a_failure() {
        let host = FakePrivacyHost::new(None);
        host.set_values(TypedKey::RunMru, &["a"]);
        host.refuse_delete(TypedKey::RunMru, "a");
        assert_eq!(erase_key(&host, TypedKey::RunMru), KeyOutcome::Failed);
    }

    #[test]
    fn a_registry_link_or_an_unreadable_key_is_never_touched() {
        let host = FakePrivacyHost::new(None);
        host.set_values(TypedKey::RunMru, &["a"]);
        host.make_link(TypedKey::RunMru);
        host.set_values(TypedKey::TypedPaths, &["url1"]);
        host.make_key_unreadable(TypedKey::TypedPaths);
        assert_eq!(erase_key(&host, TypedKey::RunMru), KeyOutcome::Failed);
        assert_eq!(erase_key(&host, TypedKey::TypedPaths), KeyOutcome::Failed);
        assert!(host.deleted().is_empty(), "rien n'est supprimé d'un lien");
        assert_eq!(erase_typed(&host), TraceOutcome::Failed);
    }

    #[test]
    fn a_list_cut_by_a_bound_cannot_be_called_erased() {
        let host = FakePrivacyHost::new(None);
        host.set_values(TypedKey::RunMru, &["a"]);
        host.make_key_incomplete(TypedKey::RunMru);
        assert_eq!(erase_key(&host, TypedKey::RunMru), KeyOutcome::Partial);
    }

    #[test]
    fn the_three_keys_are_judged_together() {
        use KeyOutcome::{Erased, Failed, Nothing, Partial};
        assert_eq!(combine(&[Nothing, Nothing, Nothing]), TraceOutcome::Nothing);
        assert_eq!(combine(&[Erased, Nothing, Erased]), TraceOutcome::Erased);
        assert_eq!(combine(&[Erased, Partial, Nothing]), TraceOutcome::Partial);
        assert_eq!(combine(&[Erased, Failed, Nothing]), TraceOutcome::Partial);
        assert_eq!(combine(&[Failed, Nothing, Nothing]), TraceOutcome::Failed);
        assert_eq!(combine(&[Failed, Failed, Failed]), TraceOutcome::Failed);
        assert_eq!(combine(&[Failed, Partial, Nothing]), TraceOutcome::Partial);
    }

    #[test]
    fn the_three_keys_are_all_emptied() {
        let host = FakePrivacyHost::new(None);
        host.set_values(TypedKey::RunMru, &["a", "MRUList"]);
        host.set_values(TypedKey::TypedPaths, &["url1"]);
        host.set_values(TypedKey::WordWheelQuery, &["0", "MRUListEx"]);
        assert_eq!(erase_typed(&host), TraceOutcome::Erased);
        for key in TypedKey::ALL {
            assert!(host.remaining(key).is_empty(), "{key:?}");
        }
    }

    #[test]
    fn recent_is_read_again_after_the_call() {
        let host = FakePrivacyHost::new(None);
        assert_eq!(erase_recent(&host), TraceOutcome::Nothing);
        assert_eq!(host.recent_calls(), 0);
        host.set_recent(5, RecentClear::All);
        assert_eq!(erase_recent(&host), TraceOutcome::Erased);
        assert_eq!(host.recent_count().unwrap(), 0);
        // L'appel ne rend rien et les raccourcis sont restés : `failed`, jamais `erased`.
        host.set_recent(5, RecentClear::Nothing);
        assert_eq!(erase_recent(&host), TraceOutcome::Failed);
        // Plus qu'avant : `failed` aussi.
        host.set_recent(5, RecentClear::Leaves(8));
        assert_eq!(erase_recent(&host), TraceOutcome::Failed);
        // Moins qu'avant : une partie est partie, `partial`.
        host.set_recent(5, RecentClear::Leaves(2));
        assert_eq!(erase_recent(&host), TraceOutcome::Partial);
        host.set_recent(5, RecentClear::Fails);
        assert_eq!(erase_recent(&host), TraceOutcome::Failed);
        host.make_recent_unreadable();
        assert_eq!(erase_recent(&host), TraceOutcome::Failed);
    }

    #[test]
    fn the_clipboard_outcome_follows_both_gestures() {
        let result = |history, current| ClipboardClear { history, current };
        let host = FakePrivacyHost::new(None);
        for (clear, wanted) in [
            (result(HistoryClear::Cleared, CurrentClear::Emptied), TraceOutcome::Erased),
            // Historique désactivé : le cas normal.
            (result(HistoryClear::Disabled, CurrentClear::Emptied), TraceOutcome::Erased),
            (result(HistoryClear::Failed, CurrentClear::Emptied), TraceOutcome::Failed),
            (result(HistoryClear::Cleared, CurrentClear::Busy), TraceOutcome::InUse),
            (result(HistoryClear::Cleared, CurrentClear::Failed), TraceOutcome::Failed),
            (result(HistoryClear::Failed, CurrentClear::Busy), TraceOutcome::Failed),
        ] {
            host.set_clipboard(true, clear);
            assert_eq!(erase_clipboard(&host, 7), wanted, "{clear:?}");
        }
        assert_eq!(host.clipboard_calls(), vec![7; 6]);
    }

    mod lot {
        use super::*;
        use crate::fs::{DirReader, EntryMeta, to_internal_path};
        use std::cell::RefCell;
        use std::ffi::OsStr;
        use std::fs;
        use std::path::Path;

        /// Un dossier de profil avec ces fichiers, et le lot tel que la liste le voit.
        fn lot_of(dir: &Path, names: &[&str]) -> Vec<LotFile> {
            for name in names {
                fs::write(dir.join(name), format!("contenu de {name}")).unwrap();
            }
            let dir = to_internal_path(dir).unwrap();
            let mut metas: Vec<(String, EntryMeta)> = Vec::new();
            DirReader::new()
                .read(&dir, None, &mut |name: &OsStr, meta: &EntryMeta| {
                    metas.push((name.to_string_lossy().into_owned(), *meta));
                })
                .unwrap();
            names
                .iter()
                .map(|name| {
                    let meta = metas.iter().find(|(n, _)| n == name).unwrap().1;
                    LotFile {
                        name: (*name).to_owned(),
                        path: dir.join(name),
                        expected: meta.expected(),
                    }
                })
                .collect()
        }

        fn present(dir: &Path, names: &[&str]) -> Vec<bool> {
            names.iter().map(|name| dir.join(name).exists()).collect()
        }

        const NAMES: [&str; 4] = ["History", "History-journal", "History-wal", "Favicons"];
        const ALWAYS_CLOSED: &dyn Fn() -> bool = &|| true;

        #[test]
        fn the_whole_lot_goes() {
            let temp = tempfile::tempdir().unwrap();
            let lot = lot_of(temp.path(), &NAMES);
            fs::write(temp.path().join("Bookmarks"), "favoris").unwrap();
            assert_eq!(erase_lot(&lot, &open_real, ALWAYS_CLOSED), TraceOutcome::Erased);
            assert_eq!(present(temp.path(), &NAMES), [false; 4]);
            assert!(temp.path().join("Bookmarks").exists(), "les voisins restent");
        }

        #[test]
        fn one_file_that_cannot_be_opened_leaves_everything_in_place_annexes_included() {
            for (refused, _) in NAMES.iter().enumerate() {
                let temp = tempfile::tempdir().unwrap();
                let lot = lot_of(temp.path(), &NAMES);
                let opened = RefCell::new(Vec::new());
                let open = |file: &LotFile| {
                    if file.name == NAMES[refused] {
                        return Err(RemoveError::Locked);
                    }
                    opened.borrow_mut().push(file.name.clone());
                    open_real(file)
                };
                assert_eq!(erase_lot(&lot, &open, ALWAYS_CLOSED), TraceOutcome::InUse, "{refused}");
                assert_eq!(present(temp.path(), &NAMES), [true; 4], "{refused}");
            }
        }

        #[test]
        fn an_open_that_fails_otherwise_is_a_failure_or_a_change() {
            for (error, wanted) in [
                (RemoveError::AccessDenied, TraceOutcome::Failed),
                (RemoveError::Changed, TraceOutcome::Changed),
                (RemoveError::NotFound, TraceOutcome::Changed),
                (RemoveError::Io(std::io::Error::other("x")), TraceOutcome::Failed),
            ] {
                let temp = tempfile::tempdir().unwrap();
                let lot = lot_of(temp.path(), &NAMES);
                let refuse = RefCell::new(Some(error));
                let open = |file: &LotFile| {
                    if file.name == "History-wal" {
                        return Err(refuse.borrow_mut().take().unwrap());
                    }
                    open_real(file)
                };
                assert_eq!(erase_lot(&lot, &open, ALWAYS_CLOSED), wanted);
                assert_eq!(present(temp.path(), &NAMES), [true; 4]);
            }
        }

        #[test]
        fn a_file_with_a_second_name_refuses_the_whole_lot() {
            let temp = tempfile::tempdir().unwrap();
            let lot = lot_of(temp.path(), &NAMES);
            let other = tempfile::tempdir().unwrap();
            fs::hard_link(temp.path().join("History"), other.path().join("ma-copie")).unwrap();
            assert_eq!(erase_lot(&lot, &open_real, ALWAYS_CLOSED), TraceOutcome::Failed);
            assert_eq!(present(temp.path(), &NAMES), [true; 4]);
            assert!(other.path().join("ma-copie").exists());
        }

        #[test]
        fn a_changed_file_is_a_change_and_nothing_goes() {
            let temp = tempfile::tempdir().unwrap();
            let lot = lot_of(temp.path(), &NAMES);
            fs::write(temp.path().join("History-wal"), "autre chose, plus long qu'avant").unwrap();
            assert_eq!(erase_lot(&lot, &open_real, ALWAYS_CLOSED), TraceOutcome::Changed);
            assert_eq!(present(temp.path(), &NAMES), [true; 4]);
        }

        #[cfg(unix)]
        #[test]
        fn a_link_in_place_of_a_file_is_refused_and_its_target_untouched() {
            let temp = tempfile::tempdir().unwrap();
            let lot = lot_of(temp.path(), &NAMES);
            let target = tempfile::tempdir().unwrap();
            fs::write(target.path().join("cible"), "précieux").unwrap();
            fs::remove_file(temp.path().join("History")).unwrap();
            std::os::unix::fs::symlink(target.path().join("cible"), temp.path().join("History"))
                .unwrap();
            let outcome = erase_lot(&lot, &open_real, ALWAYS_CLOSED);
            assert_ne!(outcome, TraceOutcome::Erased);
            assert_eq!(fs::read_to_string(target.path().join("cible")).unwrap(), "précieux");
            assert_eq!(present(temp.path(), &NAMES[1..]), [true; 3]);
        }

        #[test]
        fn a_browser_found_open_once_the_handles_are_held_stops_everything() {
            let temp = tempfile::tempdir().unwrap();
            let lot = lot_of(temp.path(), &NAMES);
            let all_open = lot.len();
            let opened = RefCell::new(0usize);
            let open = |file: &LotFile| {
                *opened.borrow_mut() += 1;
                open_real(file)
            };
            let closed = || {
                // Au moment de la revérification, tout le lot est déjà ouvert.
                assert_eq!(*opened.borrow(), all_open);
                false
            };
            assert_eq!(erase_lot(&lot, &open, &closed), TraceOutcome::InUse);
            assert_eq!(present(temp.path(), &NAMES), [true; 4]);
        }

        #[test]
        fn a_forbidden_name_in_the_lot_is_refused_before_any_file_is_opened() {
            let temp = tempfile::tempdir().unwrap();
            let mut lot = lot_of(temp.path(), &["History", "Login Data"]);
            lot[1].name = "Login Data".to_owned();
            let open =
                |_: &LotFile| -> Result<Removal, RemoveError> { panic!("rien ne doit s'ouvrir") };
            assert_eq!(erase_lot(&lot, &open, ALWAYS_CLOSED), TraceOutcome::Failed);
            assert_eq!(present(temp.path(), &["History", "Login Data"]), [true; 2]);
        }

        /// Sous Windows (et Wine) : un vrai fichier tenu ouvert sans partage, comme le
        /// fait un navigateur, refuse tout le lot, annexes comprises.
        #[cfg(windows)]
        #[test]
        fn a_file_really_held_open_refuses_the_whole_lot() {
            use std::os::windows::fs::OpenOptionsExt;
            for (held, _) in NAMES.iter().enumerate() {
                let temp = tempfile::tempdir().unwrap();
                let lot = lot_of(temp.path(), &NAMES);
                let _held = fs::OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .open(temp.path().join(NAMES[held]))
                    .unwrap();
                assert_eq!(
                    erase_lot(&lot, &open_real, ALWAYS_CLOSED),
                    TraceOutcome::InUse,
                    "{held}"
                );
                let others: Vec<&str> =
                    NAMES.iter().copied().filter(|n| *n != NAMES[held]).collect();
                assert_eq!(present(temp.path(), &others), vec![true; 3], "{held}");
            }
        }

        #[test]
        fn an_empty_lot_has_nothing_to_erase() {
            assert_eq!(erase_lot(&[], &open_real, ALWAYS_CLOSED), TraceOutcome::Nothing);
        }
    }
}
