//! Les essais du guichet pour les entrées du PC : chaque issue de l'assistant,
//! `Busy` pendant l'attente, la vue qui répond pendant l'attente, et
//! `startup.json` d'après la valeur relue.

use std::sync::Mutex;
use std::sync::mpsc::{channel, sync_channel};

use super::*;
use crate::startup::approval::{cut_bytes, restore_bytes};
use crate::startup::host::FakeStartupHost;
use crate::startup::launcher::FakeHelper;
use crate::startup::view::StartupEntry;
use crate::startup::{Mark, Place};

struct Fixture {
    _temp: tempfile::TempDir,
    host: Arc<FakeStartupHost>,
    helper: Arc<FakeHelper>,
    store: StartupStore,
    desk: Arc<StartupDesk>,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let store = StartupStore::at(temp.path().join("Shinku").join("startup.json"));
    let host = Arc::new(FakeStartupHost::new());
    host.add(Place::UserRun, "Discord", r"C:\Apps\d.exe");
    host.add(Place::MachineRun, "Essai Shinku PC", r"C:\Apps\essai.exe");
    host.add(Place::MachineRun32, "Vieux", r"C:\Apps\vieux.exe");
    host.add(Place::MachineFolder, "Bureau.lnk", r"C:\Apps\bureau.exe");
    let helper = Arc::new(FakeHelper::new());
    let desk = Arc::new(
        StartupDesk::new(host.clone(), Some(store.clone()))
            .with_own_command(None)
            .with_helper(helper.clone()),
    );
    Fixture { _temp: temp, host, helper, store, desk }
}

fn find<'a>(entries: &'a [StartupEntry], raw: &str) -> &'a StartupEntry {
    entries.iter().find(|entry| entry.name == raw).unwrap_or_else(|| panic!("{raw}"))
}

fn id_of(desk: &StartupDesk, name: &str) -> String {
    find(&desk.view().entries, name).id.to_string()
}

/// L'assistant « honnête » : il fait ce que la demande dit, puis sort avec 0.
fn honest(host: &Arc<FakeStartupHost>, helper: &FakeHelper) {
    let host = host.clone();
    helper.on_run(move |words| {
        let place = match words[2].as_str() {
            "run" => Place::MachineRun,
            "run32" => Place::MachineRun32,
            _ => Place::MachineFolder,
        };
        let name = {
            let hex = &words[3];
            let units: Vec<u16> = hex
                .as_bytes()
                .chunks(4)
                .map(|c| u16::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
                .collect();
            String::from_utf16(&units).unwrap()
        };
        let bytes =
            if words[1] == "couper" { cut_bytes(host.file_time_now()) } else { restore_bytes() };
        host.set_bytes(place, &name, &bytes);
        HelperRun::Exited(0)
    });
}

#[test]
fn the_view_says_who_can_adjust_the_pc_and_how_the_entries_are_registered() {
    let Fixture { desk, helper, .. } = fixture();
    let view = desk.view();
    assert_eq!(view.pc, PcAccess::Consent);
    assert_eq!(view.pending, None);
    let essai = find(&view.entries, "Essai Shinku PC");
    assert!(essai.adjustable);
    assert_eq!(essai.registered.as_deref(), Some("Essai Shinku PC"));
    assert_eq!(find(&view.entries, "Discord").registered, None);
    assert_eq!(find(&view.entries, "Bureau").registered.as_deref(), Some("Bureau.lnk"));
    assert!(find(&view.entries, "Bureau").adjustable);

    for pc in [PcAccess::Password, PcAccess::Direct] {
        helper.set_access(pc);
        let view = desk.view();
        assert_eq!(view.pc, pc);
        assert!(find(&view.entries, "Essai Shinku PC").adjustable);
    }
    helper.set_access(PcAccess::Unavailable);
    let view = desk.view();
    assert_eq!(view.pc, PcAccess::Unavailable);
    assert!(view.entries.iter().filter(|e| e.scope == Scope::Machine).all(|e| !e.adjustable));
    assert!(find(&view.entries, "Discord").adjustable, "le compte ne dépend pas de l'assistant");
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["pc"], "unavailable");
    assert_eq!(json["pending"], serde_json::Value::Null);
}

#[test]
fn an_unsafe_common_folder_keeps_the_display_of_lot_one_for_its_entries_only() {
    let Fixture { desk, helper, .. } = fixture();
    helper.set_folder_safe(false);
    let view = desk.view();
    assert!(!find(&view.entries, "Bureau").adjustable);
    assert!(find(&view.entries, "Essai Shinku PC").adjustable);
    let id = id_of(&desk, "Bureau");
    assert_eq!(desk.set(&id, false).unwrap_err(), StartupRefusal::NotAdjustable);
    assert!(helper.runs().is_empty());
}

#[test]
fn a_name_too_long_for_the_request_is_not_adjustable() {
    let Fixture { desk, host, helper, .. } = fixture();
    host.add(Place::MachineRun, &"n".repeat(crate::startup::MAX_NAME), "x");
    host.add(Place::MachineRun, &"m".repeat(crate::startup::MAX_NAME + 1), "x");
    let view = desk.view();
    let long_id = EntryId::new(Place::MachineRun, &"n".repeat(crate::startup::MAX_NAME));
    assert!(view.entries.iter().any(|entry| entry.id == long_id && entry.adjustable));
    let too_long = EntryId::new(Place::MachineRun, &"m".repeat(crate::startup::MAX_NAME + 1));
    assert!(view.entries.iter().any(|entry| entry.id == too_long && !entry.adjustable));
    assert_eq!(desk.set(&too_long.to_string(), false).unwrap_err(), StartupRefusal::NotAdjustable);
    assert!(helper.runs().is_empty());
}

#[test]
fn cutting_a_pc_entry_goes_through_the_helper_then_the_reread_value() {
    let Fixture { desk, host, helper, store, .. } = fixture();
    honest(&host, &helper);
    let id = id_of(&desk, "Essai Shinku PC");
    let view = desk.set_for(&id, false, 4242).unwrap();
    let essai = find(&view.entries, "Essai Shinku PC");
    assert_eq!(essai.state, State::Disabled);
    let now = host.now().unix_millis();
    assert_eq!(essai.mark, Some(Mark::CutByShinku { at_ms: now }));
    // La demande : quatre mots, la fenêtre de Shinku en propriétaire.
    let runs = helper.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].1, 4242);
    assert_eq!(runs[0].0[..3], ["demarrage1", "couper", "run"]);
    // `startup.json` : la ligne porte le FILETIME relu.
    let file = store.load();
    assert_eq!(file.cut.len(), 1);
    assert_eq!(file.cut[0].file_time, host.file_time_now());
    assert_eq!(view.pending, None);

    // Remettre : la ligne est oubliée.
    let view = desk.set_for(&id, true, 4242).unwrap();
    assert_eq!(find(&view.entries, "Essai Shinku PC").state, State::Enabled);
    assert!(store.load().cut.is_empty());
    assert_eq!(helper.runs()[1].0[1], "remettre");
}

#[test]
fn each_slot_has_its_own_word() {
    let Fixture { desk, host, helper, .. } = fixture();
    honest(&host, &helper);
    for name in ["Essai Shinku PC", "Vieux", "Bureau"] {
        let id = id_of(&desk, name);
        desk.set(&id, false).unwrap();
    }
    let words: Vec<String> = helper.runs().iter().map(|run| run.0[2].clone()).collect();
    assert_eq!(words, ["run", "run32", "dossier"]);
}

#[test]
fn every_outcome_of_the_helper_has_its_refusal_and_the_view_is_reread() {
    let cases = [
        (HelperRun::Cancelled, Some(StartupRefusal::Cancelled)),
        (HelperRun::Exited(3), Some(StartupRefusal::Cancelled)),
        (HelperRun::Exited(5), Some(StartupRefusal::NeedsAdmin)),
        (HelperRun::Exited(4), Some(StartupRefusal::UnknownEntry)),
        (HelperRun::Exited(0), None),
        (HelperRun::Exited(9), None),
        // 1 : le code d'un assistant tué, jamais rendu par lui.
        (HelperRun::Exited(1), Some(StartupRefusal::Failed)),
        (HelperRun::Exited(2), Some(StartupRefusal::Failed)),
        (HelperRun::Exited(6), Some(StartupRefusal::Failed)),
        (HelperRun::Exited(7), Some(StartupRefusal::Failed)),
        (HelperRun::Exited(8), Some(StartupRefusal::Failed)),
        (HelperRun::Exited(101), Some(StartupRefusal::Failed)),
        (HelperRun::Exited(0xC000_0005), Some(StartupRefusal::Failed)),
        (HelperRun::TimedOut, Some(StartupRefusal::Failed)),
        (HelperRun::NotStarted, Some(StartupRefusal::Failed)),
    ];
    for (outcome, expected) in cases {
        let Fixture { desk, helper, store, .. } = fixture();
        helper.on_run(move |_| outcome);
        let id = id_of(&desk, "Essai Shinku PC");
        let result = desk.set(&id, false);
        assert_eq!(result.as_ref().err().copied(), expected, "{outcome:?}");
        // Rien n'a changé : pas de ligne, entrée toujours active, plus de `pending`.
        assert!(store.load().cut.is_empty(), "{outcome:?}");
        let view = desk.view();
        assert_eq!(find(&view.entries, "Essai Shinku PC").state, State::Enabled);
        assert_eq!(view.pending, None);
        if let Ok(view) = result {
            assert_eq!(view.pending, None);
        }
    }
}

#[test]
fn a_helper_that_says_zero_without_changing_anything_leaves_no_line() {
    let Fixture { desk, helper, store, .. } = fixture();
    helper.on_run(|_| HelperRun::Exited(0));
    let id = id_of(&desk, "Essai Shinku PC");
    let view = desk.set(&id, false).unwrap();
    assert_eq!(
        find(&view.entries, "Essai Shinku PC").state,
        State::Enabled,
        "la relecture dit vrai"
    );
    assert!(store.load().cut.is_empty());
    // Le fichier brut ne garde rien non plus : une ligne à `FILETIME` nul serait
    // seulement écartée au chargement, ce qui cacherait l'erreur.
    let raw = std::fs::read_to_string(store.path()).unwrap_or_default();
    assert!(!raw.contains("Essai Shinku PC"), "{raw}");
}

#[test]
fn pending_stays_until_the_machine_is_reread() {
    let Fixture { desk, host, helper, .. } = fixture();
    honest(&host, &helper);
    let id = id_of(&desk, "Essai Shinku PC");
    let seen = Arc::new(Mutex::new(Vec::new()));
    // La relecture demande l'accès à l'assistant : on regarde `pending` à ce
    // moment, une fois l'assistant lancé.
    let (watched, spy, weak) = (helper.clone(), seen.clone(), Arc::downgrade(&desk));
    helper.on_access(move || {
        if let (false, Some(desk)) = (watched.runs().is_empty(), weak.upgrade()) {
            spy.lock().unwrap().push(desk.pending());
        }
    });
    let _ = desk.set(&id, false);
    let seen = seen.lock().unwrap().clone();
    assert!(!seen.is_empty(), "la relecture a eu lieu");
    // La première lecture est la relecture ; la dernière, la vue rendue, sans `pending`.
    assert!(seen[0].is_some(), "pending retiré avant la relecture : {seen:?}");
    assert_eq!(desk.view().pending, None);
}

#[test]
fn a_cut_done_by_someone_else_during_the_wait_is_not_credited_to_shinku_on_a_refusal() {
    let Fixture { desk, host, helper, store, .. } = fixture();
    let other = host.clone();
    helper.on_run(move |_| {
        // Le Gestionnaire des tâches coupe pendant que la personne hésite.
        other.set_bytes(Place::MachineRun, "Essai Shinku PC", &cut_bytes(other.file_time_now()));
        HelperRun::Cancelled
    });
    let id = id_of(&desk, "Essai Shinku PC");
    assert_eq!(desk.set(&id, false).unwrap_err(), StartupRefusal::Cancelled);
    assert!(store.load().cut.is_empty(), "seul un code 0 fait retenir la coupure");
    assert_eq!(find(&desk.view().entries, "Essai Shinku PC").mark, None);
}

#[test]
fn an_entry_already_in_the_requested_state_asks_for_no_consent() {
    let Fixture { desk, helper, .. } = fixture();
    let id = id_of(&desk, "Essai Shinku PC");
    assert!(desk.set(&id, true).is_ok());
    assert!(helper.runs().is_empty());
}

#[test]
fn an_unavailable_pc_runs_nothing() {
    let Fixture { desk, helper, host, .. } = fixture();
    let id = id_of(&desk, "Essai Shinku PC");
    helper.set_access(PcAccess::Unavailable);
    assert_eq!(desk.set(&id, false).unwrap_err(), StartupRefusal::NotAdjustable);
    assert!(helper.runs().is_empty() && host.writes().is_empty());
}

#[test]
fn an_unreadable_setting_or_a_duplicate_is_not_adjustable_for_the_pc_either() {
    let Fixture { desk, host, helper, .. } = fixture();
    host.set_bytes(Place::MachineRun, "Essai Shinku PC", &[6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    host.add(Place::MachineRun32, "Double", "x");
    host.add(Place::MachineRun32, "Double", "y");
    let view = desk.view();
    assert!(!find(&view.entries, "Essai Shinku PC").adjustable);
    let id = id_of(&desk, "Essai Shinku PC");
    assert_eq!(desk.set(&id, false).unwrap_err(), StartupRefusal::NotAdjustable);
    assert!(helper.runs().is_empty());
}

#[test]
fn while_the_helper_waits_the_view_answers_and_a_second_change_is_busy() {
    let Fixture { desk, host, helper, .. } = fixture();
    let (entered_tx, entered_rx) = sync_channel::<()>(0);
    let (release_tx, release_rx) = channel::<()>();
    let release_rx = Mutex::new(release_rx);
    let writer = host.clone();
    helper.on_run(move |_| {
        entered_tx.send(()).unwrap();
        release_rx.lock().unwrap().recv().unwrap();
        writer.set_bytes(Place::MachineRun, "Essai Shinku PC", &cut_bytes(writer.file_time_now()));
        HelperRun::Exited(0)
    });
    let id = id_of(&desk, "Essai Shinku PC");
    let discord = id_of(&desk, "Discord");
    let background = {
        let (desk, id) = (desk.clone(), id.clone());
        std::thread::spawn(move || desk.set_for(&id, false, 7))
    };
    entered_rx.recv().unwrap();

    // La vue répond tout de suite, avec `pending`.
    let view = desk.view();
    let pending = view.pending.expect("un changement attend");
    assert_eq!((pending.id.to_string(), pending.enabled), (id.clone(), false));
    assert_eq!(serde_json::to_value(&view).unwrap()["pending"]["enabled"], false);
    // Un second changement, du PC ou du compte : `Busy`.
    assert_eq!(desk.set(&id, false).unwrap_err(), StartupRefusal::Busy);
    assert_eq!(desk.set(&discord, false).unwrap_err(), StartupRefusal::Busy);
    assert!(host.writes().is_empty(), "le compte n'a rien reçu");

    release_tx.send(()).unwrap();
    let done = background.join().unwrap().unwrap();
    assert_eq!(done.pending, None);
    assert_eq!(find(&done.entries, "Essai Shinku PC").state, State::Disabled);
    assert!(desk.set(&discord, false).is_ok(), "libre de nouveau");
}

#[test]
fn a_helper_that_panics_does_not_leave_the_desk_waiting_for_ever() {
    let Fixture { desk, helper, .. } = fixture();
    helper.on_run(|_| panic!("le lanceur plante"));
    let id = id_of(&desk, "Essai Shinku PC");
    let background = {
        let (desk, id) = (desk.clone(), id.clone());
        std::thread::spawn(move || desk.set(&id, false))
    };
    assert!(background.join().is_err());
    assert_eq!(desk.view().pending, None);
    helper.on_run(|_| HelperRun::Exited(0));
    assert!(desk.set(&id, false).is_ok(), "ni `busy` ni `turn` ne restent pris");
}

#[test]
fn the_page_never_sends_a_path_or_a_name_only_an_id() {
    let Fixture { desk, helper, .. } = fixture();
    for bad in ["Essai Shinku PC", r"C:\Apps\essai.exe", "mr-0000000000000000"] {
        assert_eq!(desk.set(bad, false).unwrap_err(), StartupRefusal::UnknownEntry, "{bad}");
    }
    assert!(helper.runs().is_empty());
}

#[test]
fn the_serialized_refusals_and_view_have_the_names_the_page_expects() {
    let json = |refusal| serde_json::to_value(refusal).unwrap();
    assert_eq!(json(StartupRefusal::Cancelled), "cancelled");
    assert_eq!(json(StartupRefusal::NeedsAdmin), "needsAdmin");
    let Fixture { desk, .. } = fixture();
    let value = serde_json::to_value(desk.view()).unwrap();
    assert!(value["entries"][0].get("registered").is_some());
    assert_eq!(value["pc"], "consent");
    assert!(value["pending"].is_null());
}
