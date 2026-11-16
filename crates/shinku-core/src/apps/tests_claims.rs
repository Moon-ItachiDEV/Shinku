//! Les voisines : ce qu'elles réclament, comparé **par le texte** aux écritures
//! du candidat, sans jamais toucher au disque pour elles.

use std::sync::Arc;

use crate::apps::claims;
use crate::apps::fake::{FakeAppsHost, LOCAL};
use crate::apps::host::RawRegistration;
use crate::apps::leftovers::LeftoverKind;
use crate::apps::tests::reg;
use crate::apps::tests_leftovers::{find, hades, hades_folder, neighbour, paths, snapshot_of};
use crate::apps::{Place, Source};

const HADES: &str = r"D:\Jeux\Hades";

/// Une voisine qui écrit ce texte comme désinstalleur.
fn with_line(line: &str) -> (Arc<FakeAppsHost>, bool) {
    let (host, snapshot) = hades();
    neighbour(&host, "voisine", "Outil de mods", |raw| raw.uninstall_string = Some(line.into()));
    let found = find(&host, &snapshot);
    assert!(found.complete, "{line} : le PC reste lisible");
    let proposed = hades_folder(&found);
    (host, proposed)
}

#[test]
fn the_neighbours_are_read_without_any_disk_call() {
    let (host, _) = hades();
    let before = host.disk_calls();
    for (key, location, line, icon) in [
        ("a", r"Z:\Apps\Outil", r"Z:\Apps\Outil\u.exe", r"Z:\Apps\Outil\i.ico,0"),
        (
            "b",
            r"C:\Users\Admin\AppData\Local\Outil",
            r"C:\PROGRA~2\VIEUXJ~1\u.exe",
            r"C:\PROGRA~2\X~1\i.ico",
        ),
        ("c", r"\\?\E:\~Apps\Outil", r#"RunDll32 C:\PROGRA~2\COMMON~1\Ctor.dll,Run"#, ""),
    ] {
        neighbour(&host, key, "Autre", |raw| {
            raw.install_location = Some(location.into());
            raw.uninstall_string = Some(line.into());
            raw.display_icon = Some(icon.into());
        });
    }
    host.add_remote_drive('Z');
    host.deny_access(r"C:\Users\Admin\AppData\Local\Outil");
    assert!(claims::read(host.as_ref()).is_some());
    assert_eq!(host.disk_calls(), before, "lire les voisines ne touche pas au disque");
}

#[test]
fn an_old_installshield_through_rundll32_keeps_the_pc_readable() {
    let (_, proposed) = with_line(
        r#"RunDll32 C:\PROGRA~2\COMMON~1\INSTAL~1\PROFES~1\RunTime\Ctor.dll,LaunchSetup "C:\PROGRA~2\INSTAL~1\{ABC}\Setup.exe""#,
    );
    assert!(proposed, "le dossier de l'autre app est toujours proposé");
}

#[test]
fn an_orphan_registration_in_short_names_keeps_the_pc_readable() {
    let (_, proposed) = with_line(r"C:\PROGRA~2\VIEUXJ~1\unins000.exe");
    assert!(proposed);
}

#[test]
fn a_missing_icon_in_short_names_keeps_the_pc_readable() {
    let (host, snapshot) = hades();
    neighbour(&host, "ico", "Avec icône", |raw| {
        raw.display_icon = Some(r"C:\PROGRA~2\VIEUXJ~1\jeu.ico,0".into());
    });
    let found = find(&host, &snapshot);
    assert!(found.complete && hades_folder(&found));
}

#[test]
fn a_tilde_in_a_real_long_name_is_just_a_name() {
    let (host, snapshot) = hades();
    neighbour(&host, "tilde", "Outil", |raw| raw.install_location = Some(r"E:\~Apps\Outil".into()));
    let found = find(&host, &snapshot);
    assert!(found.complete && hades_folder(&found));
}

#[test]
fn a_location_in_another_account_or_on_a_missing_drive_claims_nothing_and_blocks_nothing() {
    let (host, snapshot) = hades();
    host.deny_access(r"C:\Users\Admin\AppData\Local\Outil");
    host.add_remote_drive('Z');
    for (key, location) in [
        ("autre", r"C:\Users\Admin\AppData\Local\Outil"),
        ("reseau", r"Z:\Apps\Outil"),
        ("absent", r"Y:\Apps"),
    ] {
        neighbour(&host, key, key, |raw| raw.install_location = Some(location.into()));
    }
    let found = find(&host, &snapshot);
    assert!(found.complete && hades_folder(&found));
}

#[test]
fn an_argument_in_short_names_still_protects_the_folder() {
    // « /dir=C:\USERS\LEA~1\… » : le nom court de l'argument est une écriture du
    // candidat, `Hades` n'est pas proposé.
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(&format!(r"{LOCAL}\Hades\unins000.exe"), 1);
    let raw = RawRegistration {
        install_location: Some(format!(r"{LOCAL}\Hades")),
        ..reg("Hades", "Hades", &format!(r#""{LOCAL}\Hades\unins000.exe""#))
    };
    let snapshot = snapshot_of(&host, raw, Source::Account);
    host.set_short_path(&format!(r"{LOCAL}\Hades"), Some(r"C:\USERS\LEA~1\APPDATA\LOCAL\HADES"));
    neighbour(&host, "outil", "Outil de mods", |raw| {
        raw.uninstall_string = Some(
            r#""C:\Program Files\Outil\rm.exe" /dir=C:\USERS\LEA~1\AppData\Local\Hades\Mods\Outil\"#
                .into(),
        );
    });
    let found = find(&host, &snapshot);
    assert!(found.complete);
    assert!(!paths(&found).contains(&format!(r"{LOCAL}\Hades")));
}

#[test]
fn a_line_mentions_the_folder_by_its_ending_or_not() {
    // Le dernier composant : barre, fin, guillemet, blanc, virgule.
    for line in [
        r#""D:\Jeux\Hades\x.exe""#,
        r"D:\Jeux\Hades",
        r#"/dir="D:\Jeux\Hades""#,
        r"D:\Jeux\Hades /S",
        r"D:\Jeux\Hades,0",
        r#"cmd /c "D:\Jeux\Hades\u.bat""#,
    ] {
        assert!(!with_line(line).1, "{line} doit réclamer Hades");
    }
    // Un autre dossier qui commence pareil ne réclame rien.
    for line in [
        r"D:\Jeux\Hades2\x.exe",
        r"D:\Jeux2Hades\x.exe",
        r"D:\Jeux\HadesII",
        r"D:\Jeux\Autre\Hades\x.exe",
    ] {
        assert!(with_line(line).1, "{line} ne doit pas réclamer Hades");
    }
}

#[test]
fn a_line_with_dots_is_also_read_resolved() {
    assert!(!with_line(r"D:\Autre\..\Jeux\Hades\u.exe").1);
    assert!(!with_line(r"D:\Jeux\.\Hades\u.exe").1);
    // Jamais au-dessus du lecteur.
    assert!(with_line(r"D:\..\..\Autre\u.exe").1);
    assert!(!with_line(r"D:\..\Jeux\Hades\u.exe").1);
}

#[test]
fn a_location_is_compared_component_by_component() {
    let case = |location: &str| {
        let (host, snapshot) = hades();
        neighbour(&host, "loc", "Outil", |raw| raw.install_location = Some(location.into()));
        hades_folder(&find(&host, &snapshot))
    };
    // Dedans, ou le candidat même, avec ses écritures : réclamé.
    assert!(!case(r"D:\Jeux\Hades\Dlc"));
    assert!(!case(r"D:\Jeux\Hades"));
    assert!(!case(r#""D:\Jeux\Hades\""#));
    assert!(!case(r"\\?\D:\Jeux\Hades\Dlc"));
    assert!(!case(r"D:\Jeux\Hades\Mods\..\Dlc"));
    assert!(!case(r"d:/jeux/hades/dlc"));
    // Ce qui n'est pas dedans ne réclame rien.
    assert!(case(r"D:\Jeux\Hades2"));
    assert!(case(r"D:\Jeux\Autre"));
    assert!(case(r"D:\Jeux\Hades\..\Autre"));
    // Sans la forme `X:\…`, ou avec une variable inconnue : rien.
    assert!(case(r"Hades"));
    assert!(case(r"%INCONNUE%\Hades"));
    assert!(case(r"\\serveur\partage\Hades"));
}

#[test]
fn a_location_mixing_long_and_short_writings_is_recognised() {
    let (host, snapshot) = hades();
    host.set_short_path(HADES, Some(r"D:\JEUX~1\HADES"));
    neighbour(&host, "mixte", "Outil", |raw| {
        raw.install_location = Some(r"D:\JEUX~1\Hades\Dlc".into());
    });
    assert!(!hades_folder(&find(&host, &snapshot)));
}

#[test]
fn a_location_that_contains_the_folder_claims_it_unless_the_guard_refuses_it() {
    let (host, snapshot) = hades();
    neighbour(&host, "gros", "Gros", |raw| raw.install_location = Some(r"D:\Jeux".into()));
    assert!(!hades_folder(&find(&host, &snapshot)));
    // Un emplacement trop large pour le garde ne réclame rien.
    let (host, snapshot) = hades();
    neighbour(&host, "large", "Large", |raw| raw.install_location = Some(r"C:\Users".into()));
    assert!(paths(&find(&host, &snapshot)).iter().any(|path| path.ends_with(r"\Roaming\Hades")));
}

#[test]
fn a_candidate_without_a_short_writing_is_dropped_when_a_neighbour_has_a_short_name() {
    for known in [None, Some(r"D:\JEUX~1")] {
        let (host, snapshot) = hades();
        host.set_short_path(HADES, known);
        neighbour(&host, "court", "Autre", |raw| {
            raw.display_icon = Some(r"C:\PROGRA~2\Autre\i.ico".into());
        });
        let found = find(&host, &snapshot);
        assert!(found.complete, "le PC reste lisible");
        assert!(!hades_folder(&found), "écriture courte inconnue ({known:?}) : écarté");
        // Seul ce candidat est écarté : les réglages, eux, ont une écriture connue.
        assert!(found.leftovers.iter().any(|left| left.kind == LeftoverKind::Data));
    }
    // Une écriture courte connue : la voisine en nom court ne bloque rien.
    let (host, snapshot) = hades();
    neighbour(&host, "court", "Autre", |raw| {
        raw.display_icon = Some(r"C:\PROGRA~2\Autre\i.ico".into());
    });
    assert!(hades_folder(&find(&host, &snapshot)));
}

#[test]
fn the_registration_places_are_the_only_cause_of_unreadable_neighbours() {
    let (host, snapshot) = hades();
    host.make_unreadable(Place::AccountWow);
    assert!(!find(&host, &snapshot).complete);
}

#[test]
fn too_many_or_incomplete_registrations_make_the_neighbours_unreadable() {
    let (host, _) = hades();
    assert!(claims::read(host.as_ref()).is_some());
    for number in 0..=crate::apps::MAX_KEYS {
        host.add_registration(Place::Machine32, reg(&format!("k{number}"), "Autre", "x.exe"));
    }
    assert!(claims::read(host.as_ref()).is_none(), "plus de MAX_KEYS inscriptions");
    let (host, _) = hades();
    host.make_incomplete(Place::Machine64);
    assert!(claims::read(host.as_ref()).is_none(), "une place incomplète");
}

#[test]
fn a_family_needs_the_whole_first_word() {
    let (host, _) = hades();
    neighbour(&host, "a", "Hadesian", |_| {});
    neighbour(&host, "b", "Hades Launcher", |_| {});
    let list = claims::read(host.as_ref()).expect("lisible");
    assert!(list.has_family("hades"), "« Hades Launcher » est de la famille");
    let (host, _) = hades();
    neighbour(&host, "a", "Hadesian", |_| {});
    let list = claims::read(host.as_ref()).expect("lisible");
    assert!(!list.has_family("hades"), "« Hadesian » n'en est pas");
}

#[test]
fn only_a_tilde_and_a_digit_make_a_short_name() {
    let (host, snapshot) = hades();
    host.set_short_path(HADES, None);
    neighbour(&host, "t", "Autre", |raw| raw.display_icon = Some(r"E:\Tilde~a\i.ico".into()));
    assert!(hades_folder(&find(&host, &snapshot)), "`~a` n'est pas un nom court");
}

/// Vrai si l'emplacement d'une voisine laisse le dossier de Hades proposé.
fn location_leaves_hades(location: &str) -> bool {
    let (host, snapshot) = hades();
    neighbour(&host, "loc", "Outil", |raw| raw.install_location = Some(location.into()));
    let found = find(&host, &snapshot);
    assert!(found.complete, "{location} : le PC reste lisible");
    hades_folder(&found)
}

#[test]
fn a_location_is_put_in_the_same_form_as_windows_reads_it() {
    for location in [
        r"D:\Jeux\\Hades\Dlc",
        r"D:\\Jeux\Hades\Dlc",
        r"D:\Jeux/\Hades\Dlc",
        r"D:\Jeux\Hades.\Dlc",
        r"D:\Jeux\Hades.",
        r"D:\Jeux\Hades ",
        r"D:\Jeux\Hades. .\Dlc",
        r"\\.\D:\Jeux\Hades\Dlc",
        r"\\?\D:\Jeux\\Hades",
        r"D:\Jeux\Hades\100% Mod",
        r"D:\Jeux\Hades\%INCONNUE%",
        // `..` retire le composant d'avant, même d'une seule lettre.
        r"D:\Jeux\A\..\Hades\Dlc",
        r"D:\A\..\Jeux\Hades\Dlc",
    ] {
        assert!(!location_leaves_hades(location), "{location} doit réclamer Hades");
    }
    // Un `%` dans un vrai nom de dossier n'écarte pas l'emplacement.
    assert!(!location_leaves_hades(r"D:\Jeux\Hades\100% Jus"));
    for location in [r"D:\Jeux\Hades2\Dlc", r"D:\Jeux\Hades.old\Dlc", r"D:\Jeux\.Hades\Dlc"] {
        assert!(location_leaves_hades(location), "{location} ne doit pas réclamer Hades");
    }
}

#[test]
fn a_line_is_put_in_the_same_form_as_windows_reads_it() {
    for line in [
        r"D:\Jeux\\Hades\u.exe",
        r"D:\\Jeux\Hades\u.exe",
        r"D:\Jeux/\Hades\u.exe",
        r"D:\Jeux.\Hades\u.exe",
        r"D:\Jeux \Hades\u.exe",
        r"D:\Jeux\Hades.\u.exe",
        r"D:\Jeux\Hades \u.exe",
        r"d:/jeux/hades/u.exe",
        r"\\?\D:\Jeux\Hades\u.exe",
        r"\\.\D:\Jeux\Hades\u.exe",
    ] {
        assert!(!with_line(line).1, "{line} doit réclamer Hades");
    }
}

#[test]
fn a_mention_holds_unless_a_letter_or_a_digit_follows() {
    for line in [
        r"x.exe -Target 'D:\Jeux\Hades'",
        r"x.exe /dirs=D:\Jeux\Hades;E:\x",
        r"x.exe (D:\Jeux\Hades)",
        r"D:\Jeux\Hades=1",
        r"D:\Jeux\Hades|x",
        r"D:\Jeux\Hades:flux",
        r"x.exe D:\Jeux\Hades.",
        r"x.exe D:\Jeux\Hades>log",
        r"x.exe D:\Jeux\Hades;",
    ] {
        assert!(!with_line(line).1, "{line} doit réclamer Hades");
    }
    for line in [r"D:\Jeux\Hades2\x.exe", r"D:\Jeux\HadesII\x.exe", r"x.exe D:\Jeux\Hadesé"] {
        assert!(with_line(line).1, "{line} ne doit pas réclamer Hades");
    }
}

#[test]
fn names_are_folded_one_character_at_a_time_like_windows() {
    let host = Arc::new(FakeAppsHost::new());
    host.add_file(r"D:\Jeux\ΟΔΟΣ\unins000.exe", 10);
    let raw = RawRegistration {
        install_location: Some(r"D:\Jeux\ΟΔΟΣ".into()),
        ..reg("Odos", "Odos", r#""D:\Jeux\ΟΔΟΣ\unins000.exe""#)
    };
    let snapshot = snapshot_of(&host, raw, Source::Machine64);
    // Le sigma final (ς) et le sigma (σ) sont la même lettre pour Windows.
    neighbour(&host, "grec", "Outil", |raw| {
        raw.install_location = Some(r"D:\Jeux\οδοσ\Dlc".into())
    });
    let found = find(&host, &snapshot);
    assert!(found.complete);
    assert!(!paths(&found).contains(&r"D:\Jeux\ΟΔΟΣ".to_owned()));
    // `ß` reste `ß` : ce n'est pas `SS`.
    assert_eq!(super::claims::tests_fold("ß"), "ß");
    assert_eq!(super::claims::tests_fold("οδος"), super::claims::tests_fold("ΟΔΟΣ"));
}

#[test]
fn a_short_name_in_the_location_of_a_neighbour_drops_a_candidate_without_short_writing() {
    let (host, snapshot) = hades();
    host.set_short_path(HADES, None);
    neighbour(&host, "court", "Outil", |raw| {
        raw.install_location = Some(r"D:\JEUX~1\Hades\Dlc".into())
    });
    let found = find(&host, &snapshot);
    assert!(found.complete && !hades_folder(&found));
}

#[test]
fn the_guard_receives_the_prefix_of_the_length_of_the_location() {
    // `D:\Jeux` est refusé comme racine : un emplacement `D:\Jeux` ne réclame rien.
    let (host, snapshot) = hades();
    host.protect_itself(r"D:\Jeux");
    neighbour(&host, "gros", "Gros", |raw| raw.install_location = Some(r"D:\Jeux".into()));
    assert!(hades_folder(&find(&host, &snapshot)));
    // Un parent non protégé réclame ; un autre préfixe protégé n'y change rien.
    let (host, snapshot) = hades();
    host.protect_itself(r"D:\Jeux\Hades\Dlc");
    neighbour(&host, "gros", "Gros", |raw| raw.install_location = Some(r"D:\Jeux".into()));
    assert!(!hades_folder(&find(&host, &snapshot)));
    // Écrit avec une barre finale, le parent réclame tout autant.
    let (host, snapshot) = hades();
    neighbour(&host, "gros", "Gros", |raw| raw.install_location = Some(r"D:\Jeux\".into()));
    assert!(!hades_folder(&find(&host, &snapshot)));
}

#[test]
fn the_limit_of_registrations_is_exact() {
    let fill = |count: usize| {
        let host = FakeAppsHost::new();
        for number in 0..count {
            host.add_registration(Place::Machine32, reg(&format!("k{number}"), "Autre", "x.exe"));
        }
        claims::read(&host).is_some()
    };
    assert!(fill(crate::apps::MAX_KEYS), "MAX_KEYS inscriptions : lisible");
    assert!(!fill(crate::apps::MAX_KEYS + 1), "une de plus : illisible");
}
