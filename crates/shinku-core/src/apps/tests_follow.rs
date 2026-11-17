//! Suivre une désinstallation : les phases, leurs délais, leurs sorties.

use std::time::Duration;

use crate::apps::desk::AppsDesk;
use crate::apps::fake::ROAMING;
use crate::apps::host::RawRegistration;
use crate::apps::tests::{machine, reg};
use crate::apps::view::FollowView;
use crate::apps::{Place, Source};

const GUID: &str = "{1A2B3C4D-0000-1111-2222-333344445555}";

fn secs(count: u64) -> Duration {
    Duration::from_secs(count)
}

fn phase(desk: &AppsDesk) -> FollowView {
    desk.view().follow
}

/// L'app a fini : son inscription n'existe plus.
fn uninstalled(host: &crate::apps::fake::FakeAppsHost) {
    host.remove_registration(Place::Machine64, "Essai");
}

#[test]
fn the_registry_is_the_only_truth() {
    let host = machine();
    let desk = AppsDesk::bare(host.clone());
    desk.start("m6:Essai").expect("lancé");
    // Des minutes passent, le processus est sorti : tant que l'inscription est
    // là, on attend.
    host.end_process();
    host.advance(secs(600));
    assert!(matches!(phase(&desk), FollowView::Waiting { seconds, .. } if seconds >= 600));
    uninstalled(&host);
    assert!(matches!(phase(&desk), FollowView::Settling { .. }));
}

#[test]
fn settling_waits_five_seconds_and_a_stable_folder() {
    let host = machine();
    let desk = AppsDesk::bare(host.clone());
    desk.start("m6:Essai").expect("lancé");
    uninstalled(&host);
    assert!(matches!(phase(&desk), FollowView::Settling { .. }));
    // Première lecture du dossier : rien à comparer encore.
    host.advance(secs(3));
    assert!(matches!(phase(&desk), FollowView::Settling { .. }));
    // Le désinstalleur vide encore son dossier : le contenu change.
    host.remove_path(r"C:\Program Files\Essai\unins000.exe");
    host.advance(secs(3));
    assert!(matches!(phase(&desk), FollowView::Settling { .. }));
    // Stable depuis deux lectures, 5 secondes passées.
    host.advance(secs(3));
    assert!(matches!(phase(&desk), FollowView::Removed { .. }));
}

#[test]
fn settling_gives_up_waiting_after_sixty_seconds() {
    let host = machine();
    let desk = AppsDesk::bare(host.clone());
    desk.start("m6:Essai").expect("lancé");
    uninstalled(&host);
    let _ = phase(&desk);
    // Le contenu change à chaque lecture : jamais stable.
    for round in 0..40 {
        host.add_file(&format!(r"C:\Program Files\Essai\f{round}.tmp"), 1);
        host.advance(secs(2));
        if matches!(phase(&desk), FollowView::Removed { .. }) {
            assert!(round >= 28, "pas avant les 60 secondes, vu à la lecture {round}");
            return;
        }
    }
    panic!("la recherche aurait dû partir au bout de 60 secondes");
}

#[test]
fn without_a_location_only_the_five_seconds_count() {
    let host = machine();
    host.replace_registration(
        Place::Machine64,
        reg("Essai", "Essai", r#""C:\Program Files\Essai\unins000.exe""#),
    );
    host.replace_registration(
        Place::Machine64,
        RawRegistration {
            install_location: Some(r"C:\Windows".into()),
            ..reg("Essai", "Essai", r#""C:\Windows\System32\cmd.exe" /c"#)
        },
    );
    let desk = AppsDesk::bare(host.clone());
    desk.start("m6:Essai").expect("lancé");
    uninstalled(&host);
    assert!(matches!(phase(&desk), FollowView::Settling { .. }));
    host.advance(secs(6));
    assert!(matches!(phase(&desk), FollowView::Removed { .. }));
}

#[test]
fn the_registration_coming_back_returns_to_waiting() {
    let host = machine();
    let desk = AppsDesk::bare(host.clone());
    desk.start("m6:Essai").expect("lancé");
    let original = reg("Essai", "Essai", r#""C:\Program Files\Essai\unins000.exe""#);
    uninstalled(&host);
    assert!(matches!(phase(&desk), FollowView::Settling { .. }));
    host.add_registration(Place::Machine64, original);
    assert!(matches!(phase(&desk), FollowView::Waiting { .. }));
}

#[test]
fn an_unreadable_registration_concludes_nothing() {
    let host = machine();
    let desk = AppsDesk::bare(host.clone());
    desk.start("m6:Essai").expect("lancé");
    host.make_registration_unreadable(Source::Machine64, "Essai");
    host.advance(secs(10));
    assert!(matches!(phase(&desk), FollowView::Waiting { .. }));
}

#[test]
fn waiting_ends_after_thirty_minutes() {
    let host = machine();
    let desk = AppsDesk::bare(host.clone());
    desk.start("m6:Essai").expect("lancé");
    host.advance(secs(29 * 60));
    assert!(matches!(phase(&desk), FollowView::Waiting { .. }));
    host.advance(secs(2 * 60));
    assert!(matches!(phase(&desk), FollowView::Idle));
}

fn msi_machine() -> std::sync::Arc<crate::apps::fake::FakeAppsHost> {
    let host = machine();
    host.add_registration(
        Place::Machine64,
        RawRegistration { windows_installer: Some(1), ..reg(GUID, "Par MSI", "") },
    );
    host
}

#[test]
fn windows_installer_leaving_while_the_app_stays_means_the_person_cancelled() {
    let host = msi_machine();
    let desk = AppsDesk::bare(host.clone());
    let id = format!("m6:{GUID}");
    desk.start(&id).expect("lancé");
    assert!(host.launches()[0].watched);
    host.end_process();
    // Une première lecture ne suffit pas, deux de suite oui.
    let first = desk.view();
    assert!(matches!(first.follow, FollowView::Waiting { .. }));
    assert!(first.outcome.is_none());
    let second = desk.view();
    assert!(matches!(second.follow, FollowView::Idle));
    assert!(second.outcome.is_some());
    // Dit une seule fois.
    assert!(desk.view().outcome.is_none());
}

#[test]
fn another_installer_leaving_is_not_a_cancellation() {
    let host = machine();
    let desk = AppsDesk::bare(host.clone());
    desk.start("m6:Essai").expect("lancé");
    host.end_process();
    for _ in 0..3 {
        assert!(matches!(phase(&desk), FollowView::Waiting { .. }));
    }
}

#[test]
fn stop_goes_back_to_the_list_and_drops_a_late_search() {
    let host = machine();
    let desk = AppsDesk::bare(host.clone());
    desk.start("m6:Essai").expect("lancé");
    desk.stop();
    assert!(matches!(phase(&desk), FollowView::Idle));
    // On peut en lancer une autre aussitôt.
    desk.start("m6:Essai").expect("relancé");
    assert_eq!(host.launches().len(), 2);
    let _ = ROAMING;
}

#[test]
fn a_search_that_arrives_after_going_back_to_the_list_is_thrown_away() {
    use crate::apps::fake::FakeAppsHost;
    use crate::apps::follow::{Follow, Step, Tracked};
    use crate::apps::host::{AppsHost, Launch};
    use crate::apps::leftovers::{Found, Leftover, LeftoverKind, Snapshot};
    use crate::apps::{AppId, Scope};

    let host = FakeAppsHost::new();
    let mut follow = Follow::new();
    let tracked = |name: &str| Tracked {
        id: AppId::new(Source::Account, name),
        name: name.to_owned(),
        snapshot: Snapshot {
            name: name.to_owned(),
            bare: name.to_lowercase(),
            publisher: None,
            scope: Scope::Account,
            location: None,
        },
        installer: false,
        since: host.uptime(),
    };
    // Lance, désinstalle, et attend la recherche des restes.
    let searching = |follow: &mut Follow, name: &str| {
        let generation = follow.begin(tracked(name)).expect("libre");
        follow.launched(generation, Launch::Started(None), host.uptime()).expect("lancé");
        let _ = follow.poll(&host);
        host.advance(secs(6));
        let Step::Search { generation, .. } = follow.poll(&host) else {
            panic!("la recherche aurait dû partir");
        };
        generation
    };
    let first = searching(&mut follow, "Une");
    follow.stop();
    let second = searching(&mut follow, "Deux");
    // Le résultat de la première arrive pendant la recherche de la seconde.
    let stale = Found {
        leftovers: vec![Leftover {
            key: 1,
            kind: LeftoverKind::Data,
            path: r"C:\Une".into(),
            bytes: None,
            files: None,
        }],
        family: false,
        complete: true,
    };
    follow.apply_search(first, stale, 2);
    assert!(matches!(follow.view(host.uptime()), FollowView::Settling { .. }));
    let fresh = Found { leftovers: Vec::new(), family: false, complete: true };
    follow.apply_search(second, fresh, 2);
    assert!(matches!(follow.view(host.uptime()), FollowView::Removed { .. }));
}

// --- Le drapeau « l'app est partie » ---

mod gone_flag {
    use super::*;
    use crate::apps::AppId;
    use crate::apps::follow::{Follow, Step, Tracked};
    use crate::apps::host::{AppsHost, Launch};
    use crate::apps::tests_leftovers::snapshot_of;

    fn follow_on(host: &std::sync::Arc<crate::apps::fake::FakeAppsHost>) -> Follow {
        let raw = reg("Essai", "Essai", r#""C:\Program Files\Essai\unins000.exe""#);
        let snapshot = snapshot_of(host, raw, Source::Machine64);
        // `snapshot_of` retire l'inscription : on la remet, l'app est encore là.
        host.add_registration(
            Place::Machine64,
            reg("Essai", "Essai", r#""C:\Program Files\Essai\unins000.exe""#),
        );
        let tracked = Tracked {
            id: AppId::new(Source::Machine64, "Essai"),
            name: "Essai".to_owned(),
            snapshot,
            installer: false,
            since: host.uptime(),
        };
        let mut follow = Follow::new();
        let generation = follow.begin(tracked).expect("libre");
        follow.launched(generation, Launch::Started(None), host.uptime()).expect("lancé");
        follow
    }

    #[test]
    fn it_is_raised_on_settling_to_searching_and_nowhere_else() {
        let host = machine();
        let mut follow = follow_on(&host);
        // Inscription encore là : rien.
        host.advance(secs(10));
        assert!(matches!(follow.poll(host.as_ref()), Step::Nothing));
        assert_eq!(follow.take_gone(), None);
        // Partie, mais le dossier n'est pas stable : rien encore.
        uninstalled(&host);
        assert!(matches!(follow.poll(host.as_ref()), Step::Nothing));
        assert_eq!(follow.take_gone(), None);
        host.advance(secs(3));
        assert!(matches!(follow.poll(host.as_ref()), Step::Nothing));
        host.advance(secs(3));
        let Step::Search { generation, .. } = follow.poll(host.as_ref()) else {
            panic!("la recherche devait partir");
        };
        assert_eq!(follow.take_gone().as_deref(), Some("Essai"));
        assert_eq!(follow.take_gone(), None, "dit une seule fois");
        // Le résultat de la recherche, puis « Chercher à nouveau » : pas de second drapeau.
        follow.apply_search(
            generation,
            crate::apps::leftovers::Found { leftovers: Vec::new(), family: false, complete: true },
            0,
        );
        let (generation, _, _) = follow.rescan_input().expect("removed");
        follow.apply_search(
            generation,
            crate::apps::leftovers::Found { leftovers: Vec::new(), family: false, complete: true },
            0,
        );
        assert_eq!(follow.take_gone(), None);
    }

    #[test]
    fn it_stays_raised_when_the_person_goes_back_during_the_search() {
        let host = machine();
        let mut follow = follow_on(&host);
        uninstalled(&host);
        let _ = follow.poll(host.as_ref());
        host.advance(secs(3));
        let _ = follow.poll(host.as_ref());
        host.advance(secs(3));
        let Step::Search { generation, .. } = follow.poll(host.as_ref()) else {
            panic!("la recherche devait partir");
        };
        follow.stop();
        // Le résultat arrive après : jeté. Le drapeau, lui, a été levé.
        follow.apply_search(
            generation,
            crate::apps::leftovers::Found { leftovers: Vec::new(), family: false, complete: true },
            0,
        );
        assert!(follow.is_idle());
        assert_eq!(follow.take_gone().as_deref(), Some("Essai"));
    }

    #[test]
    fn it_is_not_raised_when_the_registration_comes_back_or_the_wait_runs_out() {
        let host = machine();
        let mut follow = follow_on(&host);
        uninstalled(&host);
        let _ = follow.poll(host.as_ref());
        host.add_registration(
            Place::Machine64,
            reg("Essai", "Essai", r#""C:\Program Files\Essai\unins000.exe""#),
        );
        let _ = follow.poll(host.as_ref());
        host.advance(secs(40 * 60));
        let _ = follow.poll(host.as_ref());
        assert!(follow.is_idle());
        assert_eq!(follow.take_gone(), None);
    }
}
