//! Les restes : ce qui est proposé, ce qui ne l'est jamais, et ce qui est
//! revérifié avant la corbeille.

use std::sync::Arc;

use crate::apps::desk::AppsDesk;
use crate::apps::fake::{
    DESKTOP, FakeAppsHost, FakeRecycle, LOCAL, LOCAL_LOW, PROFILE, ROAMING, START_MENU,
};
use crate::apps::host::{AppsHost, RawRegistration};
use crate::apps::launch::plan;
use crate::apps::leftovers::{
    Found, LeftoverKind, RemoveOutcome, RemoveReason, Snapshot, remove, search,
};
use crate::apps::tests::reg;
use crate::apps::{Place, Scope, Source, app_from};

/// Installe l'app, prend sa photo, puis la désinstalle (son inscription part).
pub(super) fn snapshot_of(
    host: &Arc<FakeAppsHost>,
    raw: RawRegistration,
    source: Source,
) -> Snapshot {
    let key = raw.key.clone();
    host.add_registration(source.place(), raw.clone());
    let own = host.own_dir();
    let app = app_from(&raw, source, own.as_deref(), host.as_ref()).expect("montrée");
    let plan = plan(&raw, source, &key, host.as_ref());
    let snapshot =
        Snapshot::take(&raw, &app.name, app.publisher.as_deref(), app.scope, &plan, host.as_ref());
    host.remove_registration(source.place(), &key);
    snapshot
}

pub(super) fn find(host: &FakeAppsHost, snapshot: &Snapshot) -> Found {
    let mut next = 0;
    search(host, snapshot, &mut next)
}

pub(super) fn paths(found: &Found) -> Vec<String> {
    found.leftovers.iter().map(|leftover| leftover.path.clone()).collect()
}

/// Une app ordinaire de `D:\Jeux\Hades`, avec réglages dans `%APPDATA%`.
pub(super) fn hades() -> (Arc<FakeAppsHost>, Snapshot) {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"D:\Jeux\Hades\unins000.exe", 10);
    host.add_file(r"D:\Jeux\Hades\Hades.exe", 500);
    host.add_file(&format!(r"{ROAMING}\Hades\save.sav"), 40);
    let raw = RawRegistration {
        install_location: Some(r"D:\Jeux\Hades".into()),
        ..reg("Hades", "Hades", r#""D:\Jeux\Hades\unins000.exe""#)
    };
    let snapshot = snapshot_of(&host, raw, Source::Machine64);
    (host, snapshot)
}

#[test]
fn the_app_folder_and_its_settings_are_proposed_once_the_app_is_gone() {
    let (host, snapshot) = hades();
    let found = find(&host, &snapshot);
    assert!(found.complete && !found.family);
    assert_eq!(paths(&found), [r"D:\Jeux\Hades".to_owned(), format!(r"{ROAMING}\Hades")]);
    assert_eq!(found.leftovers[0].kind, LeftoverKind::Folder);
    assert_eq!(found.leftovers[0].bytes, Some(510));
    assert_eq!(found.leftovers[0].files, Some(2));
    assert_eq!(found.leftovers[1].kind, LeftoverKind::Data);
}

#[test]
fn a_shared_parent_folder_is_never_proposed() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"D:\Jeux\Autre\a.exe", 1);
    host.add_file(r"D:\Jeux\Hades\unins000.exe", 1);
    let raw = RawRegistration {
        install_location: Some(r"D:\Jeux".into()),
        ..reg("Hades", "Hades", r#""D:\Jeux\Hades\unins000.exe""#)
    };
    let snapshot = snapshot_of(&host, raw, Source::Machine64);
    assert!(paths(&find(&host, &snapshot)).is_empty());
}

#[test]
fn a_neighbour_of_the_same_family_blocks_everything() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"C:\Python312\python.exe", 1);
    host.add_file(&format!(r"{LOCAL}\Python\x.txt"), 1);
    let raw = RawRegistration {
        install_location: Some(r"C:\Python312".into()),
        ..reg("py312", "Python 3.12.1 (64-bit)", "x.exe")
    };
    host.add_file("x.exe", 1);
    let snapshot = snapshot_of(&host, raw, Source::Account);
    assert!(!find(&host, &snapshot).family);
    host.add_registration(Place::Account, reg("py311", "Python 3.11.5 (64-bit)", "x.exe"));
    let found = find(&host, &snapshot);
    assert!(found.family);
    assert!(found.leftovers.is_empty());
}

#[test]
fn unreadable_registrations_propose_nothing_and_say_so() {
    let (host, snapshot) = hades();
    host.make_unreadable(Place::Machine32);
    let found = find(&host, &snapshot);
    assert!(!found.complete);
    assert!(found.leftovers.is_empty());
    host.make_readable(Place::Machine32);
    host.make_incomplete(Place::AccountWow);
    assert!(!find(&host, &snapshot).complete);
}

#[test]
fn a_neighbour_that_claims_the_folder_protects_it() {
    let (host, snapshot) = hades();
    host.add_registration(
        Place::Machine32,
        RawRegistration {
            install_location: Some(r"D:\Jeux\Hades\Dlc".into()),
            ..reg("dlc", "Extension", "x.exe")
        },
    );
    assert!(!paths(&find(&host, &snapshot)).contains(&r"D:\Jeux\Hades".to_owned()));
}

#[test]
fn names_match_exactly_never_by_containing() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(&format!(r"{ROAMING}\Discord PTB\a"), 1);
    host.add_file(&format!(r"{ROAMING}\Discordia\a"), 1);
    host.add_file(&format!(r"{ROAMING}\discord\a"), 1);
    let raw = RawRegistration {
        install_location: Some(format!(r"{LOCAL}\Discord")),
        ..reg("Discord", "Discord", "x.exe")
    };
    host.add_file("x.exe", 1);
    let snapshot = snapshot_of(&host, raw, Source::Account);
    assert_eq!(paths(&find(&host, &snapshot)), [format!(r"{ROAMING}\discord")]);
}

#[test]
fn a_launcher_folder_is_not_the_folder_of_its_game() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"D:\Steam\steamapps\common\Hades\a.exe", 1);
    host.add_file(r"D:\Steam\steam.exe", 1);
    let raw = RawRegistration {
        install_location: Some(r"D:\Steam".into()),
        ..reg("Steam App 1145360", "Hades", r"D:\Steam\steam.exe steam://uninstall/1145360")
    };
    let snapshot = snapshot_of(&host, raw, Source::Machine64);
    assert!(paths(&find(&host, &snapshot)).is_empty());
}

#[test]
fn a_whole_word_of_the_location_names_the_settings_folder() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"C:\Program Files\VideoLAN\VLC\vlc.exe", 1);
    host.add_file(&format!(r"{ROAMING}\vlc\vlcrc"), 1);
    host.add_file(&format!(r"{ROAMING}\Application\x"), 1);
    host.add_file("u.exe", 1);
    let raw = RawRegistration {
        install_location: Some(r"C:\Program Files\VideoLAN\VLC".into()),
        ..reg("VLC media player", "VLC media player", "u.exe")
    };
    let snapshot = snapshot_of(&host, raw, Source::Machine64);
    assert_eq!(paths(&find(&host, &snapshot)), [format!(r"{ROAMING}\vlc")]);
}

#[test]
fn a_subfolder_of_the_publisher_folder_is_proposed_but_not_the_publisher() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(&format!(r"{LOCAL_LOW}\Team Cherry\Hollow Knight\save"), 5);
    host.add_file(&format!(r"{LOCAL_LOW}\Team Cherry\Silksong\save"), 5);
    host.add_file("u.exe", 1);
    let raw = RawRegistration {
        publisher: Some("Team Cherry".into()),
        ..reg("HK", "Hollow Knight", "u.exe")
    };
    let snapshot = snapshot_of(&host, raw, Source::Account);
    assert_eq!(paths(&find(&host, &snapshot)), [format!(r"{LOCAL_LOW}\Team Cherry\Hollow Knight")]);
}

#[test]
fn what_the_guard_or_a_link_or_a_sync_root_forbids_is_not_proposed() {
    for case in ["root", "link-on-chain", "sync", "unmeasurable", "inner-link", "own"] {
        let (host, snapshot) = hades();
        match case {
            "root" => host.protect_tree(r"D:\Jeux"),
            "link-on-chain" => {
                host.remove_path(r"D:\Jeux\Hades");
                host.remove_path(r"D:\Jeux");
                host.add_link(r"D:\Jeux");
                host.add_dir(r"D:\Jeux\Hades");
            }
            "sync" => host.add_sync_root(r"D:\Jeux"),
            "unmeasurable" => host.make_unmeasurable(r"D:\Jeux\Hades"),
            "inner-link" => host.add_link(r"D:\Jeux\Hades\lien"),
            "own" => host.set_own_dir(Some(r"D:\Jeux\Hades\Shinku")),
            _ => unreachable!(),
        }
        let found = find(&host, &snapshot);
        assert!(!paths(&found).contains(&r"D:\Jeux\Hades".to_owned()), "{case}");
    }
}

#[test]
fn a_folder_that_contains_shinku_is_never_proposed() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"C:\Program Files\Shinku\shinku.exe", 1);
    host.add_file("u.exe", 1);
    let raw = RawRegistration {
        install_location: Some(r"C:\Program Files\Shinku".into()),
        ..reg("Shinku", "Shinku", "u.exe")
    };
    let snapshot = snapshot_of(&host, raw, Source::Machine64);
    assert!(paths(&find(&host, &snapshot)).is_empty());
}

#[test]
fn a_drive_root_or_a_system_folder_is_never_a_location() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"C:\Windows\Installer\unins.exe", 1);
    let raw = RawRegistration {
        install_location: Some(r"C:\".into()),
        ..reg("Racine", "Racine", r#""C:\Windows\Installer\unins.exe""#)
    };
    let snapshot = snapshot_of(&host, raw, Source::Machine64);
    assert_eq!(snapshot.location, None);
    assert!(paths(&find(&host, &snapshot)).is_empty());
}

#[test]
fn a_dangling_shortcut_of_the_app_is_proposed_a_live_one_is_not() {
    let (host, snapshot) = hades();
    host.add_shortcut(
        &format!(r"{START_MENU}\Programs\Hades.lnk"),
        Some(r"D:\Jeux\Hades\Hades.exe"),
    );
    host.add_shortcut(&format!(r"{DESKTOP}\Hades.lnk"), Some(r"D:\Jeux\Hades\Hades.exe"));
    host.add_shortcut(&format!(r"{DESKTOP}\Autre.lnk"), Some(r"D:\Autre\a.exe"));
    host.add_shortcut(&format!(r"{DESKTOP}\Sans cible.lnk"), None);
    host.remove_path(r"D:\Jeux\Hades");
    let found = find(&host, &snapshot);
    let shortcuts: Vec<&str> = found
        .leftovers
        .iter()
        .filter(|leftover| leftover.kind == LeftoverKind::Shortcut)
        .map(|leftover| leftover.path.as_str())
        .collect();
    assert_eq!(shortcuts.len(), 2, "{shortcuts:?}");
    // La cible existe encore : le raccourci n'est pas un reste.
    host.add_file(r"D:\Jeux\Hades\Hades.exe", 1);
    let found = find(&host, &snapshot);
    assert!(found.leftovers.iter().all(|leftover| leftover.kind != LeftoverKind::Shortcut));
}

#[test]
fn keys_keep_counting_across_searches() {
    let (host, snapshot) = hades();
    let mut next = 7;
    let first = search(host.as_ref(), &snapshot, &mut next);
    assert_eq!(first.leftovers.iter().map(|l| l.key).collect::<Vec<_>>(), [7, 8]);
    let second = search(host.as_ref(), &snapshot, &mut next);
    assert_eq!(second.leftovers.iter().map(|l| l.key).collect::<Vec<_>>(), [9, 10]);
}

#[test]
fn removal_goes_to_the_recycle_bin_only_and_only_what_was_asked() {
    let (host, snapshot) = hades();
    let found = find(&host, &snapshot);
    let key = found.leftovers[1].key;
    let (results, bytes) = remove(host.as_ref(), &snapshot, &found.leftovers, &[key, key]);
    assert_eq!(results.len(), 1, "une fois chaque clé");
    assert_eq!(results[0].outcome, RemoveOutcome::Removed);
    assert_eq!(bytes, 40);
    assert_eq!(host.recycled(), [format!(r"{ROAMING}\Hades")]);
    // Redemandé : déjà parti.
    let (again, _) = remove(host.as_ref(), &snapshot, &found.leftovers, &[key]);
    assert_eq!(again[0].outcome, RemoveOutcome::AlreadyGone);
}

#[test]
fn an_unknown_key_is_refused() {
    let (host, snapshot) = hades();
    let found = find(&host, &snapshot);
    let (results, _) = remove(host.as_ref(), &snapshot, &found.leftovers, &[999]);
    assert_eq!(results[0].outcome, RemoveOutcome::Refused);
    assert!(host.recycled().is_empty());
}

#[test]
fn everything_is_checked_again_at_the_moment_of_removal() {
    // Une app de la même famille apparaît entre la recherche et la corbeille.
    let (host, snapshot) = hades();
    let found = find(&host, &snapshot);
    host.add_registration(Place::Account, reg("h2", "Hades II", "x.exe"));
    let (results, bytes) = remove(host.as_ref(), &snapshot, &found.leftovers, &[0, 1]);
    assert!(results.iter().all(|r| r.outcome == RemoveOutcome::Refused));
    assert_eq!(bytes, 0);
    assert!(host.recycled().is_empty());

    // Un lien s'est glissé dans le dossier.
    let (host, snapshot) = hades();
    let found = find(&host, &snapshot);
    host.add_link(r"D:\Jeux\Hades\lien");
    let (results, _) = remove(host.as_ref(), &snapshot, &found.leftovers, &[0]);
    assert_eq!(results[0].outcome, RemoveOutcome::Refused);

    // Les inscriptions ne se relisent plus.
    let (host, snapshot) = hades();
    let found = find(&host, &snapshot);
    host.make_unreadable(Place::Account);
    let (results, _) = remove(host.as_ref(), &snapshot, &found.leftovers, &[0, 1]);
    assert!(results.iter().all(|r| r.outcome == RemoveOutcome::Refused));
}

#[test]
fn a_shortcut_that_now_points_to_something_is_kept() {
    let (host, snapshot) = hades();
    let lnk = format!(r"{DESKTOP}\Hades.lnk");
    host.add_shortcut(&lnk, Some(r"D:\Jeux\Hades\Hades.exe"));
    host.remove_path(r"D:\Jeux\Hades");
    let found = find(&host, &snapshot);
    let key = found.leftovers.iter().find(|l| l.kind == LeftoverKind::Shortcut).expect("là").key;
    host.add_file(r"D:\Jeux\Hades\Hades.exe", 1);
    let (results, _) = remove(host.as_ref(), &snapshot, &found.leftovers, &[key]);
    assert_eq!(results[0].outcome, RemoveOutcome::Refused);
    assert!(host.recycled().is_empty());
}

#[test]
fn recycle_failures_say_why_and_never_fall_back_to_deleting() {
    for (how, reason) in [
        (FakeRecycle::Locked, Some(RemoveReason::InUse)),
        (FakeRecycle::Refused, Some(RemoveReason::Refused)),
        (FakeRecycle::Denied, None),
        (FakeRecycle::Other, None),
        (FakeRecycle::LeavesSome, Some(RemoveReason::Partial)),
    ] {
        let (host, snapshot) = hades();
        let found = find(&host, &snapshot);
        host.fail_recycle(r"D:\Jeux\Hades", how);
        let (results, bytes) = remove(host.as_ref(), &snapshot, &found.leftovers, &[0]);
        assert_eq!(results[0].outcome, RemoveOutcome::Failed, "{how:?}");
        assert_eq!(results[0].reason, reason, "{how:?}");
        assert_eq!(bytes, 0);
    }
}

#[test]
fn through_the_desk_the_removed_phase_lists_and_shrinks() {
    use crate::apps::view::FollowView;
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"D:\Jeux\Hades\unins000.exe", 10);
    host.add_file(&format!(r"{ROAMING}\Hades\save.sav"), 40);
    host.add_registration(
        Place::Machine64,
        RawRegistration {
            install_location: Some(r"D:\Jeux\Hades".into()),
            ..reg("Hades", "Hades", r#""D:\Jeux\Hades\unins000.exe""#)
        },
    );
    let desk = AppsDesk::new(host.clone());
    // Hors de la phase « removed », tout est refusé.
    let early = desk.remove(&[0, 0, 1], &|| false).expect("rendu");
    assert_eq!(early.results.len(), 2);
    assert!(early.results.iter().all(|r| r.outcome == RemoveOutcome::Refused));
    assert!(matches!(desk.rescan().expect("rendu").follow, FollowView::Idle));

    desk.start("m6:Hades").expect("lancé");
    host.remove_registration(Place::Machine64, "Hades");
    let _ = desk.view();
    host.advance(std::time::Duration::from_secs(3));
    let _ = desk.view();
    host.advance(std::time::Duration::from_secs(3));
    let FollowView::Removed { leftovers, .. } = desk.view().follow else {
        panic!("la phase « removed » était attendue");
    };
    assert_eq!(leftovers.len(), 2);
    let done = desk.remove(&[leftovers[1].key], &|| false).expect("rendu");
    assert_eq!(done.bytes, 40);
    let FollowView::Removed { leftovers: rest, .. } = done.view.follow else {
        panic!("la phase « removed » était attendue");
    };
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].key, leftovers[0].key, "les clés ne bougent pas");
    // Une nouvelle recherche garde le compte des clés.
    let again = desk.rescan().expect("rendu");
    let FollowView::Removed { leftovers: rest, .. } = again.follow else {
        panic!("la phase « removed » était attendue");
    };
    assert_eq!(rest.len(), 1);
    assert!(rest[0].key > leftovers[1].key);
    let _ = (PROFILE, Scope::Account);
}

#[test]
fn a_failure_stays_in_the_list_and_only_what_left_goes_out() {
    use crate::apps::view::FollowView;
    for how in [FakeRecycle::Locked, FakeRecycle::Destroys, FakeRecycle::LeavesSome] {
        let host = Arc::new(FakeAppsHost::new());
        host.add_file(r"D:\Jeux\Hades\unins000.exe", 10);
        host.add_file(&format!(r"{ROAMING}\Hades\save.sav"), 40);
        host.add_registration(
            Place::Machine64,
            RawRegistration {
                install_location: Some(r"D:\Jeux\Hades".into()),
                ..reg("Hades", "Hades", r#""D:\Jeux\Hades\unins000.exe""#)
            },
        );
        let desk = AppsDesk::new(host.clone());
        desk.start("m6:Hades").expect("lancé");
        host.remove_registration(Place::Machine64, "Hades");
        let _ = desk.view();
        host.advance(std::time::Duration::from_secs(3));
        let _ = desk.view();
        host.advance(std::time::Duration::from_secs(3));
        let FollowView::Removed { leftovers, .. } = desk.view().follow else {
            panic!("la phase « removed » était attendue");
        };
        let failing = leftovers
            .iter()
            .find(|left| left.path.ends_with("Hades") && left.path.starts_with("D:"));
        let failing = failing.expect("le dossier de l'app");
        let other = leftovers.iter().find(|left| left.key != failing.key).expect("l'autre reste");
        host.fail_recycle(&failing.path, how);
        let done = desk.remove(&[failing.key, other.key], &|| false).expect("rendu");
        let FollowView::Removed { leftovers: rest, .. } = done.view.follow else {
            panic!("la phase « removed » était attendue");
        };
        let keys: Vec<u32> = rest.iter().map(|left| left.key).collect();
        assert_eq!(keys, [failing.key], "{how:?} : l'échec reste, ce qui est parti s'en va");
    }
}

// --- Les règles de sûreté, une par une (chacune tuait une mutation) ---

/// Une voisine installée, avec ce qu'on veut qu'elle réclame.
pub(super) fn neighbour(
    host: &FakeAppsHost,
    key: &str,
    name: &str,
    edit: impl FnOnce(&mut RawRegistration),
) {
    let mut raw = reg(key, name, "x.exe");
    edit(&mut raw);
    host.add_registration(Place::Machine32, raw);
}

pub(super) fn hades_folder(found: &Found) -> bool {
    paths(found).contains(&r"D:\Jeux\Hades".to_owned())
}

#[test]
fn the_app_folder_needs_the_exact_name_not_a_longer_one() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"D:\Jeux\Hades II\unins000.exe", 1);
    let raw = RawRegistration {
        install_location: Some(r"D:\Jeux\Hades II".into()),
        ..reg("Hades", "Hades", r#""D:\Jeux\Hades II\unins000.exe""#)
    };
    let snapshot = snapshot_of(&host, raw, Source::Machine64);
    assert!(paths(&find(&host, &snapshot)).is_empty());
}

#[test]
fn a_neighbour_whose_location_contains_the_folder_protects_it() {
    let (host, snapshot) = hades();
    neighbour(&host, "gros", "Gros logiciel", |raw| raw.install_location = Some(r"D:\Jeux".into()));
    assert!(!hades_folder(&find(&host, &snapshot)));
}

#[test]
fn a_badly_written_neighbour_does_not_put_every_leftover_out() {
    // `C:\Users` contient le profil, mais le garde le refuse comme racine : il
    // ne réclame rien, et les réglages sont proposés.
    let (host, snapshot) = hades();
    neighbour(&host, "mal", "Mal écrite", |raw| raw.install_location = Some(r"C:\Users".into()));
    assert!(paths(&find(&host, &snapshot)).contains(&format!(r"{ROAMING}\Hades")));
}

#[test]
fn a_neighbour_uninstaller_or_icon_inside_the_folder_protects_it() {
    for case in ["uninstall", "icon", "host"] {
        let (host, snapshot) = hades();
        neighbour(&host, "mod", "Outil de mods", |raw| match case {
            "uninstall" => raw.uninstall_string = Some(r#""D:\Jeux\Hades\Mods\rm.exe" /S"#.into()),
            "icon" => raw.display_icon = Some(r"D:\Jeux\Hades\Mods\i.ico,0".into()),
            // Un désinstalleur qui passe par un hôte : le chemin n'est pas en tête.
            _ => raw.uninstall_string = Some(r#"cmd.exe /c "D:\Jeux\Hades\Mods\u.bat""#.into()),
        });
        assert!(!hades_folder(&find(&host, &snapshot)), "{case}");
    }
}

#[test]
fn a_folder_name_is_a_settings_name_only_as_a_whole_word_of_the_app_name() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"D:\Editeur\Foo\u.exe", 1);
    host.add_file(&format!(r"{ROAMING}\Foo\reglages"), 1);
    let raw = RawRegistration {
        install_location: Some(r"D:\Editeur\Foo".into()),
        ..reg("Hades", "Hades", r#""D:\Editeur\Foo\u.exe""#)
    };
    let snapshot = snapshot_of(&host, raw, Source::Machine64);
    assert!(paths(&find(&host, &snapshot)).iter().all(|path| !path.ends_with(r"\Foo")));
}

#[test]
fn a_launcher_host_is_never_taken_for_the_app_folder() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"D:\Outils\cmd.exe", 1);
    host.add_file(r"D:\Outils\unins.exe", 1);
    let with = |key: &str, line: &str| {
        let raw = reg(key, "Outil", line);
        snapshot_of(&host, raw, Source::Machine64).location
    };
    assert_eq!(with("a", r#""D:\Outils\cmd.exe" /c x"#), None);
    assert_eq!(with("b", r#""D:\Outils\unins.exe""#).as_deref(), Some(r"D:\Outils"));
}

#[test]
fn a_folder_inside_the_shinku_folder_is_never_proposed() {
    let (host, snapshot) = hades();
    host.set_own_dir(Some(r"D:\Jeux"));
    assert!(!hades_folder(&find(&host, &snapshot)));
}

#[test]
fn only_the_first_word_of_an_unquoted_line_can_be_a_bare_name() {
    use crate::apps::launch::{Plan, plan};
    let host = FakeAppsHost::new();
    host.add_file(r"C:\Windows\System32\mon prog.exe", 1);
    let raw = reg("k", "X", "mon prog.exe /x");
    assert_eq!(plan(&raw, Source::Machine64, "k", &host), Plan::Unusable);
}

#[test]
fn the_second_name_is_dropped_when_a_neighbour_has_family_there() {
    // « Foo Studio » vit dans `Local\Foo` ; « Foo 2.0 » est encore installée.
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(&format!(r"{LOCAL}\Foo\u.exe"), 1);
    host.add_file(&format!(r"{ROAMING}\Foo\donnees"), 1);
    let raw = RawRegistration {
        install_location: Some(format!(r"{LOCAL}\Foo")),
        ..reg("fs", "Foo Studio", &format!(r#""{LOCAL}\Foo\u.exe""#))
    };
    let snapshot = snapshot_of(&host, raw, Source::Account);
    host.add_registration(Place::Machine32, reg("foo2", "Foo 2.0", "x.exe"));
    let found = find(&host, &snapshot);
    assert!(!found.family, "le nom nu n'a pas de famille");
    assert!(found.leftovers.is_empty(), "{:?}", paths(&found));
    // Sans la voisine, les réglages sont proposés, mais jamais l'emplacement.
    host.remove_registration(Place::Machine32, "foo2");
    assert_eq!(paths(&find(&host, &snapshot)), [format!(r"{ROAMING}\Foo")]);
}

#[test]
fn a_settings_folder_is_kept_when_a_neighbour_of_its_name_appears_before_removal() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(&format!(r"{LOCAL}\Foo\u.exe"), 1);
    host.add_file(&format!(r"{ROAMING}\Foo\donnees"), 1);
    let raw = RawRegistration {
        install_location: Some(format!(r"{LOCAL}\Foo")),
        ..reg("fs", "Foo Studio", &format!(r#""{LOCAL}\Foo\u.exe""#))
    };
    let snapshot = snapshot_of(&host, raw, Source::Account);
    let found = find(&host, &snapshot);
    assert_eq!(paths(&found), [format!(r"{ROAMING}\Foo")]);
    host.add_registration(Place::Machine32, reg("foo2", "Foo 2.0", "x.exe"));
    let (results, _) =
        remove(host.as_ref(), &snapshot, &found.leftovers, &[found.leftovers[0].key]);
    assert_eq!(results[0].outcome, RemoveOutcome::Refused);
    assert!(host.recycled().is_empty());
}

#[test]
fn settings_are_never_the_location_nor_around_it() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(&format!(r"{ROAMING}\Hades\u.exe"), 1);
    host.add_file(&format!(r"{ROAMING}\Hades\Data\x"), 1);
    let raw = RawRegistration {
        install_location: Some(format!(r"{ROAMING}\Hades")),
        ..reg("h", "Hades", &format!(r#""{ROAMING}\Hades\u.exe""#))
    };
    let snapshot = snapshot_of(&host, raw, Source::Account);
    // L'emplacement est aussi le dossier de réglages : A le propose (preuve
    // par le nom), B ne le propose pas une seconde fois.
    let found = find(&host, &snapshot);
    assert_eq!(found.leftovers.len(), 1);
    assert_eq!(found.leftovers[0].kind, LeftoverKind::Folder);
}

#[test]
fn more_than_sixty_four_keys_are_all_refused() {
    use crate::apps::view::FollowView;
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"D:\Jeux\Hades\unins000.exe", 10);
    host.add_registration(
        Place::Machine64,
        RawRegistration {
            install_location: Some(r"D:\Jeux\Hades".into()),
            ..reg("Hades", "Hades", r#""D:\Jeux\Hades\unins000.exe""#)
        },
    );
    let desk = AppsDesk::new(host.clone());
    desk.start("m6:Hades").expect("lancé");
    host.remove_registration(Place::Machine64, "Hades");
    for _ in 0..3 {
        let _ = desk.view();
        host.advance(std::time::Duration::from_secs(3));
    }
    let FollowView::Removed { leftovers, .. } = desk.view().follow else {
        panic!("la phase « removed » était attendue");
    };
    let key = leftovers[0].key;
    // Une clé valide au milieu de 65 : tout est refusé, rien ne part.
    let mut keys = vec![key];
    keys.extend(1000..1064);
    let refused = desk.remove(&keys, &|| false).expect("rendu");
    assert!(refused.results.iter().all(|r| r.outcome == RemoveOutcome::Refused));
    assert!(host.recycled().is_empty());
    // 64 clés, dont la valide : elle part.
    keys.pop();
    let done = desk.remove(&keys, &|| false).expect("rendu");
    assert_eq!(done.results[0].outcome, RemoveOutcome::Removed);
}

#[test]
fn what_windows_destroyed_is_never_counted_as_sent_to_the_recycle_bin() {
    let (host, snapshot) = hades();
    let found = find(&host, &snapshot);
    host.fail_recycle(r"D:\Jeux\Hades", FakeRecycle::Destroys);
    let (results, bytes) = remove(host.as_ref(), &snapshot, &found.leftovers, &[0]);
    assert_eq!(results[0].outcome, RemoveOutcome::Failed);
    assert_eq!(results[0].reason, Some(RemoveReason::Destroyed));
    assert_eq!(bytes, 0, "pas un octet à la corbeille");
    assert!(host.recycled().is_empty());
    assert_eq!(serde_json::to_value(RemoveReason::Destroyed).unwrap(), "destroyed");
}

#[test]
fn a_destroyed_answer_is_checked_against_the_disk() {
    let (host, snapshot) = hades();
    let found = find(&host, &snapshot);
    host.fail_recycle(r"D:\Jeux\Hades", FakeRecycle::SaysDestroyed);
    let (results, bytes) = remove(host.as_ref(), &snapshot, &found.leftovers, &[0]);
    assert_eq!(results[0].outcome, RemoveOutcome::Failed);
    assert_eq!(results[0].reason, None, "le chemin est encore là : pas de raison `destroyed`");
    assert_eq!(bytes, 0);
}
