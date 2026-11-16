//! Noter les désinstallations : quand la note s'écrit, ce qu'elle compte, et
//! ce qui ne la change pas.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use crate::apps::Place;
use crate::apps::desk::AppsDesk;
use crate::apps::fake::{FakeAppsHost, FakeRecycle, ROAMING};
use crate::apps::host::RawRegistration;
use crate::apps::notes::{NotesFile, UninstallNote, UninstallStore};
use crate::apps::tests::{machine, reg};
use crate::apps::view::{FollowView, LeftoverRow};

fn secs(count: u64) -> Duration {
    Duration::from_secs(count)
}

/// Un guichet qui note dans un dossier temporaire, et l'heure murale qu'on règle.
struct Rig {
    _temp: tempfile::TempDir,
    store: UninstallStore,
    clock: Arc<AtomicI64>,
    desk: AppsDesk,
}

fn rig(host: &Arc<FakeAppsHost>) -> Rig {
    let temp = tempfile::tempdir().unwrap();
    let store = UninstallStore::at(temp.path().join("Shinku").join("uninstalls.json"));
    let clock = Arc::new(AtomicI64::new(1_000));
    let reader = Arc::clone(&clock);
    let desk = AppsDesk::new(
        host.clone(),
        Some(store.clone()),
        Arc::new(move || reader.load(Ordering::SeqCst)),
    );
    Rig { _temp: temp, store, clock, desk }
}

/// Une app dans `D:\Jeux\<nom>`, avec des réglages (40 octets) dans `%APPDATA%`.
fn add_app(host: &FakeAppsHost, key: &str, name: &str) {
    host.add_file(&format!(r"D:\Jeux\{name}\unins000.exe"), 10);
    host.add_file(&format!(r"{ROAMING}\{name}\save.sav"), 40);
    host.add_registration(
        Place::Machine64,
        RawRegistration {
            install_location: Some(format!(r"D:\Jeux\{name}")),
            ..reg(key, name, &format!(r#""D:\Jeux\{name}\unins000.exe""#))
        },
    );
}

fn hades() -> Arc<FakeAppsHost> {
    let host = Arc::new(FakeAppsHost::new());
    add_app(&host, "Hades", "Hades");
    host
}

/// L'app part : on suit le désinstalleur jusqu'au bout de la recherche.
fn uninstall_and_wait(rig: &Rig, host: &FakeAppsHost, id: &str, key: &str) -> Vec<LeftoverRow> {
    rig.desk.start(id).expect("lancé");
    host.remove_registration(Place::Machine64, key);
    let _ = rig.desk.view();
    host.advance(secs(3));
    let _ = rig.desk.view();
    host.advance(secs(3));
    match rig.desk.view().follow {
        FollowView::Removed { leftovers, .. } => leftovers,
        other => panic!("la phase « removed » était attendue : {other:?}"),
    }
}

fn notes(rig: &Rig) -> Vec<UninstallNote> {
    rig.store.load().notes
}

fn data_key(leftovers: &[LeftoverRow]) -> u32 {
    leftovers
        .iter()
        .find(|row| row.path.ends_with(r"\Hades") && row.path.contains("Roaming"))
        .map_or_else(
            || leftovers.iter().find(|row| row.path.starts_with(ROAMING)).expect("réglages").key,
            |row| row.key,
        )
}

#[test]
fn nothing_is_written_while_the_app_is_still_registered_or_settling() {
    let host = hades();
    let rig = rig(&host);
    rig.desk.start("m6:Hades").expect("lancé");
    let _ = rig.desk.view();
    assert!(notes(&rig).is_empty(), "l'inscription est encore là");
    host.remove_registration(Place::Machine64, "Hades");
    let view = rig.desk.view();
    assert!(matches!(view.follow, FollowView::Settling { .. }));
    assert!(notes(&rig).is_empty(), "le dossier n'est pas encore stable");
    assert!(!rig.store.path().exists());
}

#[test]
fn the_note_is_written_once_when_the_folder_has_settled_with_the_wall_clock_time() {
    let host = hades();
    let rig = rig(&host);
    rig.clock.store(5_000, Ordering::SeqCst);
    let _ = uninstall_and_wait(&rig, &host, "m6:Hades", "Hades");
    let written = notes(&rig);
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].name, "Hades");
    assert_eq!(written[0].at_ms, 5_000);
    assert_eq!((written[0].leftovers, written[0].leftover_bytes), (0, 0));
    // Relire la vue ne note rien de plus.
    for _ in 0..3 {
        let _ = rig.desk.view();
    }
    assert_eq!(notes(&rig).len(), 1);
    assert_eq!(rig.desk.notes(), written, "le guichet relit le même fichier");
}

#[test]
fn a_cancelled_or_failed_launch_and_a_giving_up_leave_no_note() {
    use crate::apps::fake::LaunchPlan;
    for plan in [LaunchPlan::Cancelled, LaunchPlan::Missing, LaunchPlan::Failed(5)] {
        let host = hades();
        host.set_launch(plan);
        let rig = rig(&host);
        assert!(rig.desk.start("m6:Hades").is_err());
        let _ = rig.desk.view();
        assert!(notes(&rig).is_empty());
    }
    // Trente minutes sans que l'inscription parte : le suivi rend la main.
    let host = hades();
    let rig = rig(&host);
    rig.desk.start("m6:Hades").expect("lancé");
    host.advance(secs(31 * 60));
    assert!(matches!(rig.desk.view().follow, FollowView::Idle));
    host.remove_registration(Place::Machine64, "Hades");
    let _ = rig.desk.view();
    assert!(notes(&rig).is_empty(), "rendu la main : l'app partie ensuite n'est pas notée");
}

#[test]
fn a_windows_installer_that_left_with_the_app_still_there_leaves_no_note() {
    let host = machine();
    host.add_registration(
        Place::Machine64,
        RawRegistration {
            windows_installer: Some(1),
            ..reg("{1A2B3C4D-0000-1111-2222-333344445555}", "Par MSI", "")
        },
    );
    let rig = rig(&host);
    rig.desk.start("m6:{1A2B3C4D-0000-1111-2222-333344445555}").expect("lancé");
    host.end_process();
    for _ in 0..3 {
        let _ = rig.desk.view();
    }
    assert!(notes(&rig).is_empty());
}

#[test]
fn searching_again_does_not_write_a_second_note() {
    let host = hades();
    let rig = rig(&host);
    let _ = uninstall_and_wait(&rig, &host, "m6:Hades", "Hades");
    for _ in 0..3 {
        rig.clock.fetch_add(1000, Ordering::SeqCst);
        rig.desk.rescan().expect("recherche");
    }
    assert_eq!(notes(&rig).len(), 1);
}

#[test]
fn only_what_really_went_to_the_recycle_bin_is_counted() {
    let host = hades();
    let rig = rig(&host);
    let leftovers = uninstall_and_wait(&rig, &host, "m6:Hades", "Hades");
    assert_eq!(leftovers.len(), 2);
    let data = data_key(&leftovers);
    let folder = leftovers.iter().find(|row| row.key != data).expect("le dossier").key;
    host.fail_recycle(r"D:\Jeux\Hades", FakeRecycle::Locked);
    // Le dossier refuse, les réglages partent, une clé inconnue est refusée : un seul compte.
    let done = rig.desk.remove(&[data, folder, 999], &|| false).expect("rendu");
    assert_eq!(done.bytes, 40);
    let written = notes(&rig);
    assert_eq!((written[0].leftovers, written[0].leftover_bytes), (1, 40));
    // Les mêmes clés encore : plus rien ne part, rien ne s'ajoute.
    let _ = rig.desk.remove(&[data, 999], &|| false).expect("rendu");
    assert_eq!(notes(&rig)[0].leftovers, 1);
}

#[test]
fn a_leftover_already_gone_is_not_counted() {
    let host = hades();
    let rig = rig(&host);
    let leftovers = uninstall_and_wait(&rig, &host, "m6:Hades", "Hades");
    let data = data_key(&leftovers);
    // La personne l'a supprimé entre-temps.
    host.remove_path(&format!(r"{ROAMING}\Hades"));
    host.remove_path(&format!(r"{ROAMING}\Hades\save.sav"));
    let done = rig.desk.remove(&[data], &|| false).expect("rendu");
    assert_eq!(done.results[0].outcome, crate::apps::leftovers::RemoveOutcome::AlreadyGone);
    assert_eq!(notes(&rig)[0].leftovers, 0);
}

#[test]
fn a_neighbour_of_the_family_or_an_unreadable_machine_gives_a_note_without_leftovers() {
    // Une app du même nom reste : l'inscription suivie est bien partie.
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"C:\Python312\python.exe", 1);
    host.add_file(r"C:\Python312\unins000.exe", 1);
    host.add_registration(
        Place::Machine64,
        RawRegistration {
            install_location: Some(r"C:\Python312".into()),
            ..reg("py312", "Python 3.12.1 (64-bit)", r#""C:\Python312\unins000.exe""#)
        },
    );
    host.add_registration(Place::Account, reg("py311", "Python 3.11.5 (64-bit)", "x.exe"));
    let rig_a = rig(&host);
    let leftovers = uninstall_and_wait(&rig_a, &host, "m6:py312", "py312");
    assert!(leftovers.is_empty());
    let written = notes(&rig_a);
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].name, "Python 3.12.1 (64-bit)");
    assert_eq!(written[0].leftovers, 0);

    // Une place illisible : la liste n'est pas complète, la note est écrite quand même.
    let host = hades();
    host.make_unreadable(Place::Machine32);
    let rig_b = rig(&host);
    let leftovers = uninstall_and_wait(&rig_b, &host, "m6:Hades", "Hades");
    assert!(leftovers.is_empty());
    assert_eq!(notes(&rig_b).len(), 1);
}

#[test]
fn going_back_to_the_list_before_the_folder_settled_leaves_no_note() {
    let host = hades();
    let rig = rig(&host);
    rig.desk.start("m6:Hades").expect("lancé");
    host.remove_registration(Place::Machine64, "Hades");
    let _ = rig.desk.view();
    assert!(matches!(rig.desk.stop().follow, FollowView::Idle));
    host.advance(secs(10));
    let _ = rig.desk.view();
    assert!(notes(&rig).is_empty(), "le drapeau n'a jamais été levé");
}

#[test]
fn going_back_to_the_list_after_the_note_keeps_it_and_nothing_is_added_later() {
    let host = hades();
    let rig = rig(&host);
    let leftovers = uninstall_and_wait(&rig, &host, "m6:Hades", "Hades");
    assert_eq!(notes(&rig).len(), 1);
    rig.desk.stop();
    assert_eq!(notes(&rig).len(), 1);
    // Hors de la phase « removed », la mise à la corbeille est refusée : aucune note ne change.
    let early = rig.desk.remove(&[leftovers[0].key], &|| false).expect("rendu");
    assert!(
        early
            .results
            .iter()
            .all(|result| { result.outcome == crate::apps::leftovers::RemoveOutcome::Refused })
    );
    assert_eq!(notes(&rig)[0].leftovers, 0);
}

#[test]
fn a_write_that_fails_changes_nothing_for_the_tracking() {
    let host = hades();
    let temp = tempfile::tempdir().unwrap();
    // Le « dossier » de Shinku est un fichier : ni créer le dossier ni écrire n'est possible.
    let blocker = temp.path().join("Shinku");
    std::fs::write(&blocker, b"un fichier").unwrap();
    let store = UninstallStore::at(blocker.join("uninstalls.json"));
    let desk = AppsDesk::new(host.clone(), Some(store), Arc::new(|| 7));
    desk.start("m6:Hades").expect("lancé");
    host.remove_registration(Place::Machine64, "Hades");
    let _ = desk.view();
    host.advance(secs(3));
    let _ = desk.view();
    host.advance(secs(3));
    let FollowView::Removed { leftovers, .. } = desk.view().follow else {
        panic!("la phase « removed » était attendue");
    };
    assert_eq!(leftovers.len(), 2, "le suivi va jusqu'au bout");
    // Les restes partent aussi sans note.
    let data = leftovers.iter().find(|row| row.path.starts_with(ROAMING)).expect("réglages").key;
    let done = desk.remove(&[data], &|| false).expect("rendu");
    assert_eq!(done.results.len(), 1);
    assert!(desk.notes().is_empty());
    assert_eq!(std::fs::read(&blocker).unwrap(), b"un fichier");
}

#[test]
fn without_a_store_nothing_is_noted_and_nothing_breaks() {
    let host = hades();
    let desk = AppsDesk::bare(host.clone());
    desk.start("m6:Hades").expect("lancé");
    host.remove_registration(Place::Machine64, "Hades");
    let _ = desk.view();
    host.advance(secs(3));
    let _ = desk.view();
    host.advance(secs(3));
    assert!(matches!(desk.view().follow, FollowView::Removed { .. }));
    assert!(desk.notes().is_empty());
    desk.forget_notes();
}

#[test]
fn forgetting_empties_the_file_and_a_later_removal_does_not_bring_the_note_back() {
    let host = hades();
    let rig = rig(&host);
    let leftovers = uninstall_and_wait(&rig, &host, "m6:Hades", "Hades");
    assert_eq!(notes(&rig).len(), 1);
    rig.desk.forget_notes();
    assert!(notes(&rig).is_empty());
    assert!(rig.desk.notes().is_empty());
    // Les restes partent ensuite : la note oubliée ne revient pas.
    let data = data_key(&leftovers);
    let done = rig.desk.remove(&[data], &|| false).expect("rendu");
    assert_eq!(done.bytes, 40);
    assert!(notes(&rig).is_empty());
    assert_eq!(rig.store.load(), NotesFile::default());
    // Une désinstallation suivante est notée normalement.
    rig.desk.stop();
    add_app(&host, "Zeta", "Zeta");
    let _ = uninstall_and_wait(&rig, &host, "m6:Zeta", "Zeta");
    let written = notes(&rig);
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].name, "Zeta");
}

#[test]
fn each_uninstall_has_its_own_note_and_its_own_leftovers() {
    let host = hades();
    add_app(&host, "Zeta", "Zeta");
    let rig = rig(&host);
    let first = uninstall_and_wait(&rig, &host, "m6:Hades", "Hades");
    rig.desk.remove(&[data_key(&first)], &|| false).expect("rendu");
    rig.desk.stop();
    rig.clock.store(9_000, Ordering::SeqCst);
    let second = uninstall_and_wait(&rig, &host, "m6:Zeta", "Zeta");
    let zeta_data =
        second.iter().find(|row| row.path.starts_with(ROAMING)).expect("réglages de Zeta").key;
    rig.desk.remove(&[zeta_data], &|| false).expect("rendu");
    let written = notes(&rig);
    assert_eq!(written.len(), 2);
    assert_eq!((written[0].name.as_str(), written[0].leftovers), ("Hades", 1));
    assert_eq!(
        (written[1].name.as_str(), written[1].leftovers, written[1].at_ms),
        ("Zeta", 1, 9_000)
    );
}

#[test]
fn a_note_that_could_not_be_written_never_borrows_the_previous_notes_key() {
    let host = hades();
    add_app(&host, "Zeta", "Zeta");
    let rig = rig(&host);
    let first = uninstall_and_wait(&rig, &host, "m6:Hades", "Hades");
    assert_eq!(first.len(), 2);
    rig.desk.stop();
    // La seconde note ne peut pas partir : le voisin temporaire est un dossier.
    let blocker = rig.store.path().with_file_name("uninstalls.json.tmp");
    std::fs::create_dir(&blocker).unwrap();
    rig.clock.store(9_000, Ordering::SeqCst);
    let second = uninstall_and_wait(&rig, &host, "m6:Zeta", "Zeta");
    assert_eq!(notes(&rig).len(), 1, "la note de Zeta n'a pas pu être écrite");
    std::fs::remove_dir(&blocker).unwrap();
    // Les restes de Zeta partent : ils ne s'ajoutent pas à la note de Hades.
    let zeta_data =
        second.iter().find(|row| row.path.starts_with(ROAMING)).expect("réglages de Zeta").key;
    rig.desk.remove(&[zeta_data], &|| false).expect("rendu");
    let written = notes(&rig);
    assert_eq!(written.len(), 1);
    assert_eq!((written[0].name.as_str(), written[0].leftovers), ("Hades", 0));
}
