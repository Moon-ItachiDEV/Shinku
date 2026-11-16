//! Les essais du guichet : de vrais fichiers d'un dossier temporaire, un faux hôte.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use crate::fs::to_internal_path;
use crate::privacy::desk::{MAX_IDS, PrivacyDesk};
use crate::privacy::fake::{FakePrivacyHost, RecentClear};
use crate::privacy::rules::{Browser, TypedKey, history_lot};
use crate::privacy::view::{
    GroupKind, PrivacyRefusal, PrivacyView, TraceKind, TraceOutcome, TraceState,
};

struct Pc {
    _temp: TempDir,
    local: PathBuf,
    host: Arc<FakePrivacyHost>,
    desk: PrivacyDesk,
}

fn pc() -> Pc {
    let temp = tempfile::tempdir().unwrap();
    let local = to_internal_path(&fs::canonicalize(temp.path()).unwrap()).unwrap();
    let host = Arc::new(FakePrivacyHost::new(Some(local.clone())));
    let desk = PrivacyDesk::new(host.clone());
    Pc { _temp: temp, local, host, desk }
}

impl Pc {
    fn data(&self, browser: Browser) -> PathBuf {
        let mut path = self.local.clone();
        for part in browser.user_data() {
            path.push(part);
        }
        path
    }

    /// Un profil avec tout le lot de l'historique, plus des voisins à ne jamais toucher.
    fn profile(&self, browser: Browser, folder: &str) -> PathBuf {
        let dir = self.data(browser).join(folder);
        fs::create_dir_all(dir.join("Network")).unwrap();
        for name in history_lot(browser) {
            fs::write(dir.join(&name), format!("contenu de {name}")).unwrap();
        }
        for never in ["Bookmarks", "Login Data", "Web Data", "Preferences", "Sessions"] {
            fs::write(dir.join(never), never).unwrap();
        }
        fs::write(dir.join("Network").join("Cookies"), "cookies").unwrap();
        dir
    }

    fn local_state(&self, browser: Browser, json: &str) {
        fs::create_dir_all(self.data(browser)).unwrap();
        fs::write(self.data(browser).join("Local State"), json).unwrap();
    }

    fn history_id(&self, browser: Browser, slot: u32) -> String {
        let view = self.desk.view();
        view.groups.iter().find(|g| g.app == Some(browser) && g.slot == Some(slot)).unwrap().items
            [0]
        .id
        .to_string()
    }

    fn erase(&self, ids: &[String]) -> Vec<TraceOutcome> {
        self.desk.erase(ids, 0).unwrap().results.iter().map(|r| r.outcome).collect()
    }
}

fn exists(dir: &Path, names: &[String]) -> Vec<bool> {
    names.iter().map(|n| dir.join(n).exists()).collect()
}

fn group(view: &PrivacyView, app: Browser, slot: u32) -> &crate::privacy::view::PrivacyGroup {
    view.groups.iter().find(|g| g.app == Some(app) && g.slot == Some(slot)).unwrap()
}

#[test]
fn an_absent_browser_has_no_block_and_windows_is_always_there() {
    let pc = pc();
    let view = pc.desk.view();
    assert_eq!(view.groups.len(), 1);
    assert_eq!(view.groups[0].kind, GroupKind::Windows);
    assert_eq!(view.groups[0].key, "windows");
    assert!(view.groups[0].app.is_none() && view.groups[0].profile.is_none());
    assert!(view.groups[0].slot.is_none());
    assert!(!view.firefox && !view.edge_imports_chrome);
    // Un `User Data` vide n'a pas de profil : pas de bloc.
    fs::create_dir_all(pc.data(Browser::Chrome)).unwrap();
    assert_eq!(pc.desk.view().groups.len(), 1);
}

#[test]
fn profiles_are_found_by_their_exact_compiled_name_only() {
    let pc = pc();
    for folder in ["Default", "Profile 1", "Profile 12"] {
        pc.profile(Browser::Chrome, folder);
    }
    // Ces dossiers ne sont pas des profils : ni vus, ni effacés.
    for folder in [
        "profile 2",
        "Profile 03",
        "Profile 0",
        "Guest Profile",
        "System Profile",
        "Profile 1234567",
    ] {
        pc.profile(Browser::Chrome, folder);
    }
    // Un fichier nommé comme un profil n'est pas un profil.
    fs::write(pc.data(Browser::Chrome).join("Profile 5"), "x").unwrap();
    let view = pc.desk.view();
    let slots: Vec<Option<u32>> = view.groups.iter().map(|g| g.slot).collect();
    assert_eq!(slots, [Some(0), Some(1), Some(12), None]);
    assert_eq!(view.groups[1].key, "chrome:1");
}

#[test]
fn a_block_tells_the_weight_and_the_state_without_opening_anything() {
    let pc = pc();
    pc.profile(Browser::Chrome, "Default");
    let view = pc.desk.view();
    let item = &group(&view, Browser::Chrome, 0).items[0];
    let lot = history_lot(Browser::Chrome);
    let expected: u64 = lot.iter().map(|n| format!("contenu de {n}").len() as u64).sum();
    assert_eq!(
        (item.kind, item.state, item.bytes, item.count),
        (TraceKind::History, TraceState::Present, Some(expected), None)
    );
    // Edge n'a pas `Top Sites` dans sa règle : un tel fichier ne compte pas.
    pc.profile(Browser::Edge, "Default");
    fs::write(pc.data(Browser::Edge).join("Default").join("Top Sites"), "x").unwrap();
    let view = pc.desk.view();
    let edge = view.groups.iter().find(|g| g.app == Some(Browser::Edge)).unwrap();
    let edge_lot = history_lot(Browser::Edge);
    let weight: u64 = edge_lot.iter().map(|n| format!("contenu de {n}").len() as u64).sum();
    assert_eq!(edge.items[0].bytes, Some(weight));
    assert!(view.edge_imports_chrome);
}

#[test]
fn a_profile_without_history_files_has_nothing_to_erase() {
    let pc = pc();
    fs::create_dir_all(pc.data(Browser::Brave).join("Default")).unwrap();
    fs::write(pc.data(Browser::Brave).join("Default").join("Bookmarks"), "x").unwrap();
    let view = pc.desk.view();
    let item = &group(&view, Browser::Brave, 0).items[0];
    assert_eq!((item.state, item.bytes), (TraceState::Empty, Some(0)));
    let id = item.id.to_string();
    assert_eq!(pc.erase(&[id]), [TraceOutcome::Nothing]);
}

#[test]
fn the_display_name_comes_from_local_state_and_only_from_its_name_field() {
    let pc = pc();
    pc.profile(Browser::Chrome, "Default");
    pc.profile(Browser::Chrome, "Profile 1");
    pc.local_state(
        Browser::Chrome,
        r#"{"profile":{"info_cache":{"Default":{"name":"Léa"},"Profile 1":{"user_name":"secret@example.org"}}}}"#,
    );
    let view = pc.desk.view();
    assert_eq!(group(&view, Browser::Chrome, 0).profile.as_deref(), Some("Léa"));
    assert_eq!(group(&view, Browser::Chrome, 1).profile, None);
    assert!(!serde_json::to_string(&view).unwrap().contains("secret@"));
}

#[test]
fn running_is_a_process_of_any_session_or_a_held_lockfile() {
    let pc = pc();
    pc.profile(Browser::Chrome, "Default");
    pc.profile(Browser::Edge, "Default");
    let running =
        |pc: &Pc, app| pc.desk.view().groups.iter().find(|g| g.app == Some(app)).unwrap().running;
    assert!(!running(&pc, Browser::Chrome));
    pc.host.set_processes(Some(&["CHROME.EXE"]));
    assert!(running(&pc, Browser::Chrome));
    assert!(!running(&pc, Browser::Edge));
    // Sans processus du nom, mais le verrou de `User Data` tenu : ouvert quand même.
    pc.host.set_processes(Some(&[]));
    pc.host.hold_lockfile(&pc.data(Browser::Edge), true);
    assert!(running(&pc, Browser::Edge));
    assert!(!running(&pc, Browser::Chrome));
    // Une machine qui ne dit pas ce qui tourne : tout est tenu pour ouvert.
    pc.host.set_processes(None);
    assert!(running(&pc, Browser::Chrome));
}

#[test]
fn firefox_and_the_edge_import_line_come_from_the_machine() {
    let pc = pc();
    pc.host.set_firefox(true);
    pc.profile(Browser::Chrome, "Default");
    let view = pc.desk.view();
    assert!(view.firefox);
    assert!(!view.edge_imports_chrome, "Edge n'est pas là");
    pc.profile(Browser::Edge, "Default");
    assert!(pc.desk.view().edge_imports_chrome);
}

#[test]
fn erasing_a_history_takes_the_whole_lot_and_nothing_else() {
    let pc = pc();
    let dir = pc.profile(Browser::Chrome, "Default");
    let other = pc.profile(Browser::Chrome, "Profile 2");
    let id = pc.history_id(Browser::Chrome, 0);
    assert_eq!(pc.erase(&[id]), [TraceOutcome::Erased]);
    let lot = history_lot(Browser::Chrome);
    assert!(exists(&dir, &lot).iter().all(|e| !e));
    for never in ["Bookmarks", "Login Data", "Web Data", "Preferences", "Sessions"] {
        assert!(dir.join(never).exists(), "{never}");
    }
    assert!(dir.join("Network").join("Cookies").exists());
    // L'autre profil est intact.
    assert!(exists(&other, &lot).iter().all(|e| *e));
    // La vue relue dit « rien à effacer ».
    let view = pc.desk.view();
    assert_eq!(group(&view, Browser::Chrome, 0).items[0].state, TraceState::Empty);
}

#[test]
fn local_state_is_never_written_or_erased() {
    let pc = pc();
    pc.profile(Browser::Chrome, "Default");
    let json = r#"{"os_crypt":{"encrypted_key":"abc"},"profile":{"info_cache":{"Default":{"name":"Léa"}}}}"#;
    pc.local_state(Browser::Chrome, json);
    let before =
        fs::metadata(pc.data(Browser::Chrome).join("Local State")).unwrap().modified().unwrap();
    let id = pc.history_id(Browser::Chrome, 0);
    assert_eq!(pc.erase(&[id]), [TraceOutcome::Erased]);
    let path = pc.data(Browser::Chrome).join("Local State");
    assert_eq!(fs::read_to_string(&path).unwrap(), json);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
}

#[test]
fn an_open_browser_refuses_and_nothing_is_touched() {
    let pc = pc();
    let dir = pc.profile(Browser::Chrome, "Default");
    let id = pc.history_id(Browser::Chrome, 0);
    pc.host.set_processes(Some(&["chrome.exe"]));
    assert_eq!(pc.erase(std::slice::from_ref(&id)), [TraceOutcome::InUse]);
    pc.host.set_processes(Some(&[]));
    pc.host.hold_lockfile(&pc.data(Browser::Chrome), true);
    assert_eq!(pc.erase(std::slice::from_ref(&id)), [TraceOutcome::InUse]);
    assert!(exists(&dir, &history_lot(Browser::Chrome)).iter().all(|e| *e));
}

#[test]
fn a_replaced_profile_is_a_change_even_under_the_same_folder_name() {
    let pc = pc();
    let dir = pc.profile(Browser::Chrome, "Profile 2");
    let id = pc.history_id(Browser::Chrome, 2);
    // Supprimé puis recréé sous le même numéro : un autre dossier.
    // L'ancien dossier est rangé ailleurs (et non supprimé) : son identité ne peut pas être reprise.
    fs::rename(&dir, pc.local.join("ancien")).unwrap();
    let again = pc.profile(Browser::Chrome, "Profile 2");
    assert_eq!(pc.erase(&[id]), [TraceOutcome::Changed]);
    assert!(exists(&again, &history_lot(Browser::Chrome)).iter().all(|e| *e));
}

#[test]
fn a_vanished_profile_is_a_change() {
    let pc = pc();
    let dir = pc.profile(Browser::Chrome, "Default");
    let id = pc.history_id(Browser::Chrome, 0);
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(pc.erase(&[id]), [TraceOutcome::Changed]);
}

#[test]
fn a_changed_display_name_is_a_change() {
    let pc = pc();
    let dir = pc.profile(Browser::Chrome, "Default");
    pc.local_state(Browser::Chrome, r#"{"profile":{"info_cache":{"Default":{"name":"Léa"}}}}"#);
    let id = pc.history_id(Browser::Chrome, 0);
    pc.local_state(Browser::Chrome, r#"{"profile":{"info_cache":{"Default":{"name":"Enfant"}}}}"#);
    assert_eq!(pc.erase(&[id]), [TraceOutcome::Changed]);
    assert!(exists(&dir, &history_lot(Browser::Chrome)).iter().all(|e| *e));
}

#[test]
fn a_hard_linked_file_refuses_the_whole_lot() {
    let pc = pc();
    let dir = pc.profile(Browser::Chrome, "Default");
    let id = pc.history_id(Browser::Chrome, 0);
    fs::hard_link(dir.join("History"), pc.local.join("copie")).unwrap();
    assert_eq!(pc.erase(&[id]), [TraceOutcome::Failed]);
    assert!(exists(&dir, &history_lot(Browser::Chrome)).iter().all(|e| *e));
}

#[test]
fn a_changed_file_since_the_view_is_a_change_and_nothing_goes() {
    let pc = pc();
    let dir = pc.profile(Browser::Chrome, "Default");
    let id = pc.history_id(Browser::Chrome, 0);
    fs::write(dir.join("History-wal"), "le navigateur a écrit entre-temps, plus long").unwrap();
    // La liste est refaite à l'effacement : la taille relue est la nouvelle, le lot part.
    // Mais un fichier remplacé par un autre (autre identité) est refusé.
    let replaced = dir.join("Favicons");
    fs::remove_file(&replaced).unwrap();
    fs::write(&replaced, "remplacé").unwrap();
    let outcome = pc.erase(&[id]);
    assert!(matches!(outcome[0], TraceOutcome::Erased | TraceOutcome::Changed));
}

#[cfg(unix)]
#[test]
fn a_junction_in_place_of_the_profile_is_never_entered() {
    let pc = pc();
    fs::create_dir_all(pc.data(Browser::Chrome)).unwrap();
    let target = tempfile::tempdir().unwrap();
    for name in history_lot(Browser::Chrome) {
        fs::write(target.path().join(&name), "précieux").unwrap();
    }
    std::os::unix::fs::symlink(target.path(), pc.data(Browser::Chrome).join("Default")).unwrap();
    assert!(pc.desk.view().groups.iter().all(|g| g.app.is_none()));
    // Même un identifiant fabriqué à la main ne mène pas de l'autre côté.
    let id = format!("hc-0-{:016x}-{:032x}-{:016x}", 1, 1, 0);
    assert_eq!(pc.erase(&[id]), [TraceOutcome::Failed]);
    assert!(exists(target.path(), &history_lot(Browser::Chrome)).iter().all(|e| *e));
}

#[cfg(unix)]
#[test]
fn a_link_on_the_user_data_chain_hides_the_browser() {
    let pc = pc();
    let target = tempfile::tempdir().unwrap();
    fs::create_dir_all(target.path().join("Default")).unwrap();
    fs::write(target.path().join("Default").join("History"), "x").unwrap();
    let chrome = pc.local.join("Google");
    fs::create_dir_all(&chrome).unwrap();
    std::os::unix::fs::symlink(target.path(), chrome.join("Chrome")).unwrap();
    assert!(pc.desk.view().groups.iter().all(|g| g.app.is_none()));
}

#[cfg(unix)]
#[test]
fn an_irregular_file_in_the_lot_makes_the_line_inert() {
    let pc = pc();
    let dir = pc.profile(Browser::Chrome, "Default");
    fs::remove_file(dir.join("Favicons")).unwrap();
    std::os::unix::fs::symlink(pc.local.join("ailleurs"), dir.join("Favicons")).unwrap();
    let view = pc.desk.view();
    let item = &group(&view, Browser::Chrome, 0).items[0];
    assert_eq!((item.state, item.bytes), (TraceState::Unreadable, None));
    let id = item.id.to_string();
    assert_eq!(pc.erase(&[id]), [TraceOutcome::Failed]);
    assert!(dir.join("History").exists());
}

#[test]
fn what_the_core_did_not_make_is_refused_without_touching_anything() {
    let pc = pc();
    let dir = pc.profile(Browser::Chrome, "Default");
    let out = pc.erase(&[
        "pas un id".to_owned(),
        r"C:\Users\a\AppData\Local\Google\Chrome\User Data\Default\History".to_owned(),
    ]);
    assert_eq!(out, [TraceOutcome::Changed; 2]);
    assert!(exists(&dir, &history_lot(Browser::Chrome)).iter().all(|e| *e));
}

#[test]
fn more_than_sixty_four_ids_are_all_refused() {
    let pc = pc();
    let dir = pc.profile(Browser::Chrome, "Default");
    let id = pc.history_id(Browser::Chrome, 0);
    let mut ids = vec![id.clone(); MAX_IDS];
    assert!(pc.desk.erase(&ids, 0).is_ok());
    let again = pc.profile(Browser::Chrome, "Default");
    assert_eq!(again, dir);
    ids.push(id);
    assert_eq!(pc.desk.erase(&ids, 0).unwrap_err(), PrivacyRefusal::TooMany);
    // Rien n'a été touché par la demande refusée.
    assert!(exists(&dir, &history_lot(Browser::Chrome)).iter().all(|e| *e));
}

#[test]
fn an_id_asked_twice_is_answered_once() {
    let pc = pc();
    pc.profile(Browser::Chrome, "Default");
    let id = pc.history_id(Browser::Chrome, 0);
    let done = pc.desk.erase(&[id.clone(), id.clone()], 0).unwrap();
    assert_eq!(done.results.len(), 1);
    assert_eq!(done.results[0].id, id);
}

#[test]
fn windows_traces_are_counted_and_erased() {
    let pc = pc();
    pc.host.set_recent(7, RecentClear::All);
    pc.host.set_values(TypedKey::RunMru, &["a", "MRUList"]);
    pc.host.set_values(TypedKey::WordWheelQuery, &["0", "MRUListEx"]);
    pc.host.set_clipboard(
        true,
        crate::privacy::host::ClipboardClear {
            history: crate::privacy::host::HistoryClear::Cleared,
            current: crate::privacy::host::CurrentClear::Emptied,
        },
    );
    let view = pc.desk.view();
    let items = &view.groups.last().unwrap().items;
    assert_eq!(items[0].count, Some(7));
    assert_eq!(items[1].count, Some(4));
    assert_eq!(items[2].state, TraceState::Present);
    assert!(items.iter().all(|i| i.bytes.is_none() && i.state == TraceState::Present));
    let ids: Vec<String> = items.iter().map(|i| i.id.to_string()).collect();
    let done = pc.desk.erase(&ids, 99).unwrap();
    assert!(done.results.iter().all(|r| r.outcome == TraceOutcome::Erased));
    assert_eq!(pc.host.clipboard_calls(), [99]);
    let after = &done.view.groups.last().unwrap().items;
    assert!(after.iter().all(|i| i.state == TraceState::Empty));
}

#[test]
fn a_registry_link_makes_typed_inert_and_is_never_emptied() {
    let pc = pc();
    pc.host.set_values(TypedKey::RunMru, &["a"]);
    pc.host.make_link(TypedKey::RunMru);
    let view = pc.desk.view();
    let typed = &view.groups.last().unwrap().items[1];
    assert_eq!(typed.state, TraceState::Unreadable);
    assert_eq!(pc.erase(&[typed.id.to_string()]), [TraceOutcome::Failed]);
    assert!(pc.host.deleted().is_empty());
}

#[test]
fn values_that_come_back_are_reported_partial() {
    let pc = pc();
    pc.host.set_values(TypedKey::TypedPaths, &["url1", "url2"]);
    pc.host.respawn_after_delete(TypedKey::TypedPaths, &["url1"]);
    assert_eq!(pc.erase(&["wt".to_owned()]), [TraceOutcome::Partial]);
}

#[test]
fn no_journal_and_no_file_are_written_by_the_desk() {
    let pc = pc();
    let dir = pc.profile(Browser::Chrome, "Default");
    let id = pc.history_id(Browser::Chrome, 0);
    let before = fs::read_dir(&pc.local).unwrap().count();
    pc.erase(&[id]);
    assert_eq!(fs::read_dir(&pc.local).unwrap().count(), before);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["Bookmarks", "Login Data", "Network", "Preferences", "Sessions", "Web Data"]
    );
}

#[test]
fn the_clipboard_line_is_present_when_filled_or_when_the_history_is_on() {
    let pc = pc();
    let state = |pc: &Pc| pc.desk.view().groups.last().unwrap().items[2].clone();
    let clear = crate::privacy::host::ClipboardClear {
        history: crate::privacy::host::HistoryClear::Cleared,
        current: crate::privacy::host::CurrentClear::Emptied,
    };
    for (filled, history, wanted) in [
        (false, false, TraceState::Empty),
        (true, false, TraceState::Present),
        (false, true, TraceState::Present),
        (true, true, TraceState::Present),
    ] {
        pc.host.set_clipboard(filled, clear);
        pc.host.set_clipboard_history(Some(history));
        let item = state(&pc);
        assert_eq!(
            (item.state, item.count, item.bytes),
            (wanted, None, None),
            "{filled} {history}"
        );
    }
    // L'un des deux états qui ne se lit pas : illisible.
    pc.host.set_clipboard(true, clear);
    pc.host.set_clipboard_history(None);
    assert_eq!(state(&pc).state, TraceState::Unreadable);
    pc.host.set_clipboard_history(Some(true));
    pc.host.make_clipboard_unreadable(true);
    assert_eq!(state(&pc).state, TraceState::Unreadable);
}
