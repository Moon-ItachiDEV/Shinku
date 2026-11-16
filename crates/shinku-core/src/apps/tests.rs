//! Les apps listées, la commande à lancer, et ce que le guichet refuse.

use std::sync::Arc;

use crate::apps::desk::AppsDesk;
use crate::apps::fake::{FakeAppsHost, LaunchPlan, ROAMING};
use crate::apps::host::{AppsHost, RawNumber, RawRegistration};
use crate::apps::launch::{Plan, is_guid, plan};
use crate::apps::view::{FollowView, UninstallRefusal};
use crate::apps::{AppId, Date, Kind, Place, Removal, Scope, Source, kind_of, list};

const GUID: &str = "{1A2B3C4D-0000-1111-2222-333344445555}";

/// Une inscription ordinaire, avec son désinstalleur.
pub(super) fn reg(key: &str, name: &str, uninstall: &str) -> RawRegistration {
    RawRegistration {
        key: key.to_owned(),
        display_name: Some(name.to_owned()),
        uninstall_string: Some(uninstall.to_owned()),
        ..RawRegistration::default()
    }
}

/// Une machine avec une app `Essai` installée dans `C:\Program Files\Essai`.
pub(super) fn machine() -> Arc<FakeAppsHost> {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"C:\Program Files\Essai\unins000.exe", 10);
    host.add_registration(
        Place::Machine64,
        RawRegistration {
            install_location: Some(r"C:\Program Files\Essai".to_owned()),
            ..reg("Essai", "Essai", r#""C:\Program Files\Essai\unins000.exe" /SILENT"#)
        },
    );
    host
}

#[test]
fn an_app_needs_a_name_and_a_way_to_remove_it() {
    let host = FakeAppsHost::new();
    host.add_registration(Place::Machine64, reg("a", "Alpha", "x.exe"));
    host.add_registration(Place::Machine64, RawRegistration::new("b"));
    host.add_registration(
        Place::Machine64,
        RawRegistration { display_name: Some("Sans ligne".into()), ..RawRegistration::new("c") },
    );
    host.add_registration(
        Place::Machine64,
        RawRegistration { windows_installer: Some(1), ..reg("d", "Par MSI", "") },
    );
    let names: Vec<String> = list(&host).apps.into_iter().map(|app| app.name).collect();
    assert_eq!(names, ["Alpha", "Par MSI"]);
}

#[test]
fn hidden_components_updates_and_children_are_not_listed() {
    let host = FakeAppsHost::new();
    let hidden = RawRegistration {
        system_component: Some(RawNumber::Dword(1)),
        ..reg("a", "Composant", "x.exe")
    };
    let child =
        RawRegistration { parent_key_name: Some("P".into()), ..reg("b", "Enfant", "x.exe") };
    let patch =
        RawRegistration { release_type: Some("Hotfix".into()), ..reg("c", "Correctif", "x.exe") };
    let shown = RawRegistration {
        system_component: Some(RawNumber::Dword(0)),
        ..reg("d", "Visible", "x.exe")
    };
    for raw in [hidden, child, patch, shown] {
        host.add_registration(Place::Account, raw);
    }
    let apps = list(&host).apps;
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "Visible");
    assert_eq!(apps[0].scope, Scope::Account);
}

#[test]
fn the_list_is_sorted_without_case_or_accents() {
    let host = FakeAppsHost::new();
    for (key, name) in [("1", "zeta"), ("2", "Émile"), ("3", "alpha"), ("4", "Eric")] {
        host.add_registration(Place::Machine64, reg(key, name, "x.exe"));
    }
    let names: Vec<String> = list(&host).apps.into_iter().map(|app| app.name).collect();
    assert_eq!(names, ["alpha", "Émile", "Eric", "zeta"]);
}

#[test]
fn an_unreadable_or_cut_source_makes_the_list_incomplete() {
    let host = FakeAppsHost::new();
    host.add_registration(Place::Machine64, reg("a", "Alpha", "x.exe"));
    assert!(list(&host).complete);
    host.make_unreadable(Place::Machine32);
    let listing = list(&host);
    assert!(!listing.complete);
    assert_eq!(listing.apps.len(), 1);
    host.make_readable(Place::Machine32);
    host.make_incomplete(Place::Account);
    assert!(!list(&host).complete);
}

#[test]
fn declared_size_and_date_are_read_strictly() {
    let host = FakeAppsHost::new();
    host.add_registration(
        Place::Machine64,
        RawRegistration {
            estimated_size: Some(2),
            install_date: Some("20240229".into()),
            display_version: Some("1.2".into()),
            ..reg("a", "Alpha", "x.exe")
        },
    );
    host.add_registration(
        Place::Machine64,
        RawRegistration {
            estimated_size: Some(0),
            install_date: Some("20230229".into()),
            ..reg("b", "Beta", "x.exe")
        },
    );
    let apps = list(&host).apps;
    assert_eq!(apps[0].bytes, Some(2048));
    assert_eq!(apps[0].installed.map(|date| date.to_string()).as_deref(), Some("2024-02-29"));
    assert_eq!(apps[0].version.as_deref(), Some("1.2"));
    assert_eq!(apps[1].bytes, None);
    assert_eq!(apps[1].installed, None);
    assert_eq!(Date::parse("2024-02-29"), None);
}

#[test]
fn kinds_are_hints_by_name() {
    assert_eq!(kind_of("Microsoft Visual C++ 2015-2022 Redistributable (x64)"), Kind::Runtime);
    assert_eq!(kind_of("Microsoft .NET Runtime - 8.0.1"), Kind::Runtime);
    assert_eq!(kind_of("NVIDIA Graphics Driver 551.23"), Kind::Driver);
    assert_eq!(kind_of("Realtek High Definition Audio"), Kind::Driver);
    assert_eq!(kind_of("NVIDIA GeForce Experience"), Kind::Plain);
    assert_eq!(kind_of("Intel(R) Driver & Support Assistant"), Kind::Driver);
    assert_eq!(kind_of("Driver Booster"), Kind::Plain);
    assert_eq!(kind_of("VLC media player"), Kind::Plain);
}

#[test]
fn removal_is_offered_refused_own_or_needed() {
    let host = FakeAppsHost::new();
    host.add_registration(Place::Machine64, reg("a", "Alpha", "x.exe"));
    host.add_registration(
        Place::Machine64,
        RawRegistration { no_remove: Some(1), ..reg("b", "Bloquée", "x.exe") },
    );
    host.add_registration(Place::Machine64, reg("c", "Microsoft Edge WebView2 Runtime", "x.exe"));
    host.add_registration(Place::Account, reg("Shinku", "Shinku", "x.exe"));
    host.add_registration(
        Place::Machine32,
        RawRegistration {
            install_location: Some(r"C:\Program Files\Shinku".into()),
            ..reg("autre", "Autre nom", "x.exe")
        },
    );
    let removals: Vec<(String, Removal)> =
        list(&host).apps.into_iter().map(|app| (app.name, app.removal)).collect();
    let of = |name: &str| removals.iter().find(|(n, _)| n == name).map(|(_, r)| *r);
    assert_eq!(of("Alpha"), Some(Removal::Offered));
    assert_eq!(of("Bloquée"), Some(Removal::Refused));
    assert_eq!(of("Microsoft Edge WebView2 Runtime"), Some(Removal::Needed));
    assert_eq!(of("Shinku"), Some(Removal::Own));
    assert_eq!(of("Autre nom"), Some(Removal::Own));
}

#[test]
fn identifiers_are_strict() {
    let id = AppId::parse("m6:{ABC}").expect("forme valide");
    assert_eq!(id.source(), Source::Machine64);
    assert_eq!(id.key(), "{ABC}");
    assert_eq!(id.to_string(), "m6:{ABC}");
    for bad in ["", "m6:", "x9:a", "m6:a\\b", "m6:a\u{0}b", "M6:a", "ac", "m6:a\nb"] {
        assert!(AppId::parse(bad).is_none(), "{bad:?} doit être refusé");
    }
    assert!(AppId::parse(&format!("ac:{}", "a".repeat(256))).is_none());
    assert!(AppId::parse(&format!("ac:{}", "a".repeat(255))).is_some());
}

#[test]
fn guids_are_recognised() {
    assert!(is_guid(GUID));
    assert!(!is_guid("{1A2B3C4D-0000-1111-2222-33334444555}"));
    assert!(!is_guid("1A2B3C4D-0000-1111-2222-333344445555"));
    assert!(!is_guid("{Essai}"));
}

fn run_plan(host: &FakeAppsHost, raw: &RawRegistration, source: Source) -> Plan {
    plan(raw, source, &raw.key, host)
}

#[test]
fn windows_installer_runs_msiexec_from_the_system_folder() {
    let host = FakeAppsHost::new();
    let raw = RawRegistration {
        windows_installer: Some(1),
        ..reg(GUID, "Par MSI", r"MsiExec.exe /I{AUTRE} /quiet")
    };
    assert_eq!(
        run_plan(&host, &raw, Source::Machine64),
        Plan::Run {
            program: r"C:\Windows\System32\msiexec.exe".into(),
            parameters: format!("/x {GUID}"),
            elevated: false,
            installer: true,
        }
    );
}

#[test]
fn a_quoted_line_keeps_its_parameters_as_written() {
    let host = machine();
    let raw = reg("k", "Essai", r#""C:\Program Files\Essai\unins000.exe" /A  "b c""#);
    assert_eq!(
        run_plan(&host, &raw, Source::Machine64),
        Plan::Run {
            program: r"C:\Program Files\Essai\unins000.exe".into(),
            parameters: r#"/A  "b c""#.into(),
            elevated: true,
            installer: false,
        }
    );
    let Plan::Run { elevated, .. } = run_plan(&host, &raw, Source::Account) else {
        panic!("un plan était attendu");
    };
    assert!(!elevated, "une inscription du compte n'est jamais élevée");
}

#[test]
fn an_unquoted_line_takes_the_shortest_existing_prefix() {
    let host = FakeAppsHost::new();
    host.add_file(r"C:\Program Files\Mon App\uninst.exe", 1);
    let raw = reg("k", "Mon App", r"C:\Program Files\Mon App\uninst.exe /S");
    let Plan::Run { program, parameters, .. } = run_plan(&host, &raw, Source::Machine64) else {
        panic!("un plan était attendu");
    };
    assert_eq!(program, r"C:\Program Files\Mon App\uninst.exe");
    assert_eq!(parameters, "/S");
}

#[test]
fn a_bare_program_is_looked_for_only_in_the_system_folders() {
    let host = FakeAppsHost::new();
    host.add_file(r"C:\Windows\hh.exe", 1);
    let raw = reg("k", "Aide", "rundll32.exe dfshim.dll,ShOpenVerbApplication x");
    let Plan::Run { program, .. } = run_plan(&host, &raw, Source::Machine64) else {
        panic!("un plan était attendu");
    };
    assert_eq!(program, r"C:\Windows\System32\rundll32.exe");
    let raw = reg("k", "Aide", "hh.exe /x");
    let Plan::Run { program, .. } = run_plan(&host, &raw, Source::Machine64) else {
        panic!("un plan était attendu");
    };
    assert_eq!(program, r"C:\Windows\hh.exe");
    let raw = reg("k", "Autre", "inconnu.exe /x");
    assert_eq!(run_plan(&host, &raw, Source::Machine64), Plan::Unusable);
}

#[test]
fn unusable_lines_are_refused() {
    let host = FakeAppsHost::new();
    host.add_file(r"C:\Program Files\X\setup.msi", 1);
    host.add_file(r"C:\Program Files\X\u.exe", 1);
    host.add_remote_drive('Z');
    host.add_file(r"Z:\u.exe", 1);
    for line in [
        "",
        "   ",
        r#""C:\Program Files\X\setup.msi""#,
        r"C:\Program Files\X\setup.msi /x",
        r"\\serveur\partage\u.exe",
        r"Z:\u.exe",
        r#""""#,
        r"C:\Program Files\X\absent.exe",
    ] {
        let raw = reg("k", "X", line);
        assert_eq!(run_plan(&host, &raw, Source::Machine64), Plan::Unusable, "{line:?}");
    }
    // Plus de 64 préfixes à essayer : on n'essaie pas.
    let long = format!(r"C:\Program Files\X\{}", "a ".repeat(100));
    assert_eq!(run_plan(&host, &reg("k", "X", &long), Source::Machine64), Plan::Unusable);
    // Existence indéterminée : on ne lance rien.
    host.deny_access(r"C:\Program Files\X\u.exe");
    let raw = reg("k", "X", r#""C:\Program Files\X\u.exe""#);
    assert_eq!(run_plan(&host, &raw, Source::Machine64), Plan::Unusable);
}

#[test]
fn start_reads_the_registration_again_and_never_uses_the_page() {
    let host = machine();
    let desk = AppsDesk::new(host.clone());
    // Entre l'affichage de la liste et le clic, la commande change.
    host.add_file(r"C:\Program Files\Essai\autre.exe", 1);
    host.replace_registration(
        Place::Machine64,
        RawRegistration {
            install_location: Some(r"C:\Program Files\Essai".to_owned()),
            ..reg("Essai", "Essai", r#""C:\Program Files\Essai\autre.exe" /x"#)
        },
    );
    let view = desk.start("m6:Essai").expect("lancé");
    assert!(matches!(view.follow, FollowView::Waiting { .. }));
    let launches = host.launches();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].program, r"C:\Program Files\Essai\autre.exe");
    assert_eq!(launches[0].parameters, "/x");
    assert!(launches[0].elevated);
    assert!(!launches[0].watched);
}

#[test]
fn start_refuses_what_is_not_offered() {
    let host = machine();
    host.add_registration(
        Place::Machine64,
        RawRegistration { no_remove: Some(1), ..reg("nr", "Bloquée", "x.exe") },
    );
    host.add_registration(Place::Machine64, reg("Shinku", "Shinku", "x.exe"));
    host.add_registration(Place::Machine64, reg("cassee", "Cassée", r"C:\nulle\part.exe"));
    let desk = AppsDesk::new(host.clone());
    let refusal = |id: &str| desk.start(id).expect_err("refusé");
    assert_eq!(refusal("n'importe quoi"), UninstallRefusal::NotOffered);
    assert_eq!(refusal("m6:nr"), UninstallRefusal::NotOffered);
    assert_eq!(refusal("m6:Shinku"), UninstallRefusal::NotOffered);
    assert_eq!(refusal("m6:absente"), UninstallRefusal::Vanished);
    assert_eq!(refusal("m6:cassee"), UninstallRefusal::Broken);
    host.make_registration_unreadable(Source::Machine64, "Essai");
    assert_eq!(refusal("m6:Essai"), UninstallRefusal::Unreadable);
    assert!(host.launches().is_empty());
}

#[test]
fn start_maps_what_windows_answers() {
    for (plan, expected) in [
        (LaunchPlan::Cancelled, UninstallRefusal::Cancelled),
        (LaunchPlan::Missing, UninstallRefusal::Broken),
        (LaunchPlan::Failed(5), UninstallRefusal::Failed),
    ] {
        let host = machine();
        host.set_launch(plan);
        let desk = AppsDesk::new(host);
        assert_eq!(desk.start("m6:Essai").expect_err("refusé"), expected);
        assert!(matches!(desk.view().follow, FollowView::Idle), "rien ne reste en cours");
    }
}

#[test]
fn a_second_start_is_refused_while_the_first_waits_for_windows() {
    let host = machine();
    host.add_registration(Place::Machine64, reg("autre", "Autre", "x.exe"));
    let desk = Arc::new(AppsDesk::new(host.clone()));
    let (entered, release) = host.block_next_launch();
    let first = {
        let desk = Arc::clone(&desk);
        std::thread::spawn(move || desk.start("m6:Essai").map(|_| ()))
    };
    entered.recv().expect("le lancement est en cours");
    // La page répond pendant ce temps, et le second geste est refusé.
    assert!(matches!(desk.view().follow, FollowView::Launching { .. }));
    assert_eq!(desk.start("m6:autre").expect_err("occupé"), UninstallRefusal::Busy);
    desk.stop();
    assert!(matches!(desk.view().follow, FollowView::Launching { .. }), "stop ne coupe rien");
    release.send(()).expect("libéré");
    assert_eq!(first.join().expect("fil"), Ok(()));
    assert!(matches!(desk.view().follow, FollowView::Waiting { .. }));
}

#[test]
fn the_exclusive_right_is_taken_without_waiting() {
    let desk = AppsDesk::new(machine());
    let held = desk.try_exclusive().expect("libre");
    assert!(desk.try_exclusive().is_none());
    assert_eq!(desk.rescan().expect_err("occupé"), UninstallRefusal::Busy);
    assert_eq!(desk.remove(&[1], &|| false).expect_err("occupé"), UninstallRefusal::Busy);
    drop(held);
    assert!(desk.try_exclusive().is_some());
}

#[test]
fn a_big_clean_blocks_removal() {
    let desk = AppsDesk::new(machine());
    assert_eq!(desk.remove(&[1], &|| true).expect_err("occupé"), UninstallRefusal::Busy);
}

#[test]
fn the_module_never_writes_to_the_registry_or_the_disk_by_itself() {
    // Une lecture de la liste ne lance rien et ne met rien à la corbeille.
    let host = machine();
    let desk = AppsDesk::new(host.clone());
    let _ = desk.view();
    assert!(host.launches().is_empty());
    assert!(host.recycled().is_empty());
    assert_eq!(host.own_dir().as_deref(), Some(r"C:\Program Files\Shinku"));
    let _ = ROAMING;
}
