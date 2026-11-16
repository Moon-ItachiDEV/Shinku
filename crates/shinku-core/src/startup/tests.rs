//! Les tests du cœur du module « Démarrage » : identifiants, noms, entrées,
//! énumération et changement d'état, sur une machine fabriquée.

use super::approval::{cut_bytes, restore_bytes};
use super::host::{FakeStartupHost, InertStartupHost, RawEntry};
use super::view::StartupView;
use super::*;

const OWN: &str = r#""C:\Program Files\Shinku\shinku-agent.exe" --demarrage"#;

/// Un PC : des programmes avec leurs ressources de version, et un Windows.
fn pc() -> FakeStartupHost {
    let host = FakeStartupHost::new();
    host.add_program(r"C:\Apps\Discord\app.exe", Some("Update"), Some("Discord Inc."));
    host.add_program(r"C:\Apps\Steam\steam.exe", Some("Steam"), Some("Valve Corporation"));
    host.add_program(
        r"C:\Apps\OneDrive\OneDrive.exe",
        Some("Microsoft OneDrive"),
        Some("Microsoft Corporation"),
    );
    host.add_program(r"C:\Apps\Sans\sans.exe", None, None);
    host.add_file(r"C:\Windows\System32\rundll32.exe");
    host.add_file(r"C:\Windows\System32\SecurityHealthSystray.exe");
    host
}

fn entry<'a>(listing: &'a Listing, raw_name: &str) -> &'a Entry {
    let mut found = listing.entries.iter().filter(|entry| entry.raw_name == raw_name);
    let entry = found.next().unwrap_or_else(|| panic!("pas d'entrée « {raw_name} »"));
    assert!(found.next().is_none(), "deux entrées « {raw_name} »");
    entry
}

fn list(host: &FakeStartupHost) -> Listing {
    enumerate(host, Some(OWN), None)
}

fn id_of(host: &FakeStartupHost, name: &str) -> EntryId {
    entry(&list(host), name).id
}

// --- Identifiants ---

#[test]
fn an_id_is_two_letters_a_dash_and_sixteen_lowercase_hex_digits() {
    let id = EntryId::new(Place::UserRun, "Discord");
    assert_eq!(id.to_string(), "ur-8d8e4fcf12c5daf9");
    let letters: Vec<&str> = Place::ALL.iter().map(|place| place.letters()).collect();
    assert_eq!(letters, ["ur", "uw", "uf", "mr", "mw", "mf"]);
    for place in Place::ALL {
        let text = EntryId::new(place, "Discord").to_string();
        assert_eq!(text.len(), 19);
        assert!(text.starts_with(place.letters()));
        assert_eq!(&text[2..3], "-");
        assert!(text[3..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    }
}

#[test]
fn an_id_is_stable_and_pinned_to_the_utf16_units_of_the_raw_name() {
    // Valeurs écrites en dur : si le hachage change, les identifiants de
    // `startup.json` ne s'accordent plus avec ce que la page connaît.
    for (name, hash) in [
        ("Discord", "8d8e4fcf12c5daf9"),
        ("Shinku", "547b0a31f47472ff"),
        ("Essai Shinku Compte", "cea6f6e4686e0602"),
        ("Café", "1419e016553607b8"),
        ("😀", "f39a100fb654058a"),
        ("", "cbf29ce484222325"),
    ] {
        assert_eq!(EntryId::new(Place::UserRun, name).to_string(), format!("ur-{hash}"), "{name}");
    }
    // La casse compte : « discord » n'est pas « Discord ».
    assert_ne!(EntryId::new(Place::UserRun, "discord"), EntryId::new(Place::UserRun, "Discord"));
    assert_eq!(EntryId::new(Place::UserRun, "discord").to_string(), "ur-84b76ec43b4154d9");
    // Une lettre accentuée composée n'est pas la même que décomposée.
    assert_ne!(EntryId::new(Place::UserRun, "Café"), EntryId::new(Place::UserRun, "Cafe\u{301}"));
    // La place fait partie de l'identifiant.
    assert_ne!(EntryId::new(Place::UserRun, "Discord"), EntryId::new(Place::UserRun32, "Discord"));
}

#[test]
fn an_id_is_read_back_only_in_its_exact_form() {
    for place in Place::ALL {
        let id = EntryId::new(place, "Discord");
        assert_eq!(EntryId::parse(&id.to_string()), Some(id));
    }
    for bad in [
        "",
        "ur",
        "ur-",
        "ur-8d8e4fcf12c5daf",
        "ur-8d8e4fcf12c5daf9a",
        "ur-8D8E4FCF12C5DAF9",
        "ur-8d8e4fcf12c5dafg",
        "ur_8d8e4fcf12c5daf9",
        "ur8d8e4fcf12c5daf9-",
        "xx-8d8e4fcf12c5daf9",
        "UR-8d8e4fcf12c5daf9",
        "u-r8d8e4fcf12c5daf9",
        " ur-8d8e4fcf12c5daf9",
        "ur-8d8e4fcf12c5daf9 ",
        "ur-8d8e4fcf12c5daf9\n",
        "ur-+d8e4fcf12c5daf9",
        "ur--d8e4fcf12c5daf9",
        "ur-0x8e4fcf12c5daf9",
        "ur-8d8e4fcf12c5daé",
        "ur-8d8e4fcf12c5da９",
        "ur-8d8e4fcf12c5da\u{0}9",
    ] {
        assert_eq!(EntryId::parse(bad), None, "{bad:?}");
    }
}

// --- Les places ---

#[test]
fn the_six_places_have_a_scope_and_an_origin() {
    use Origin::{Folder, Registry};
    use Scope::{Machine, User};
    let expected = [
        (Place::UserRun, User, Registry),
        (Place::UserRun32, User, Registry),
        (Place::UserFolder, User, Folder),
        (Place::MachineRun, Machine, Registry),
        (Place::MachineRun32, Machine, Registry),
        (Place::MachineFolder, Machine, Folder),
    ];
    for (place, scope, origin) in expected {
        assert_eq!((place.scope(), place.origin()), (scope, origin), "{place:?}");
        assert_eq!(Place::from_letters(place.letters()), Some(place));
    }
    assert_eq!(Place::ALL.len(), 6);
    assert_eq!(Place::from_letters("zz"), None);
}

// --- Nettoyage des textes ---

#[test]
fn control_and_format_characters_are_removed_from_what_is_shown() {
    // U+202E renverse le sens du texte : « fdp.exe » s'afficherait « exe.pdf ».
    assert_eq!(clean_text("Mon\u{202e}exe.pdf"), "Monexe.pdf");
    assert_eq!(clean_text("a\u{200b}b\u{feff}c\u{ad}d\u{2066}e\u{e0041}"), "abcde");
    assert_eq!(clean_text("a\u{0}b\u{7}c\u{7f}d\u{9f}e"), "abcde");
    // Les blancs sont réduits à une espace, y compris ceux qui sont des contrôles.
    assert_eq!(clean_text("  Mon \t\n  app\u{a0}\u{2003}x  "), "Mon app x");
    assert_eq!(clean_text("\u{202e}\u{0}  "), "");
    assert_eq!(clean_text("Lancement rapide"), "Lancement rapide");
}

#[test]
fn a_shown_text_is_at_most_eighty_characters() {
    let long = "é".repeat(200);
    assert_eq!(clean_text(&long).chars().count(), 80);
    // Coupé sur une espace : pas d'espace en bout.
    let text = format!("{} suite", "a".repeat(79));
    assert_eq!(clean_text(&text), "a".repeat(79));
    assert_eq!(clean_text(&"b".repeat(80)).chars().count(), 80);
}

// --- Noms, éditeurs, remarques ---

#[test]
fn the_description_of_the_file_names_the_entry_unless_it_is_generic() {
    let host = pc();
    host.add(Place::UserRun, "OneDrive", r#""C:\Apps\OneDrive\OneDrive.exe" /background"#);
    host.add(Place::UserRun, "Discord", r#""C:\Apps\Discord\app.exe""#);
    host.add(Place::UserRun, "Sans description", r#""C:\Apps\Sans\sans.exe""#);
    let listing = list(&host);

    let onedrive = entry(&listing, "OneDrive");
    assert_eq!(onedrive.name, "Microsoft OneDrive");
    assert_eq!(onedrive.alias.as_deref(), Some("OneDrive"));

    // Description générique (« Update ») : le nom inscrit est plus parlant.
    let discord = entry(&listing, "Discord");
    assert_eq!(discord.name, "Discord");
    assert_eq!(discord.alias, None);

    let plain = entry(&listing, "Sans description");
    assert_eq!(plain.name, "Sans description");
    assert_eq!(plain.alias, None);
}

#[test]
fn every_generic_description_is_ignored() {
    let host = pc();
    for generic in [
        "update",
        "Updater",
        "LAUNCHER",
        "setup",
        "Helper",
        "electron",
        "Application",
        "app",
        "host",
        "service",
        "Agent",
        "tray",
        "STARTUP",
        "update.exe",
        "Launcher.EXE",
        "  Update  ",
    ] {
        host.add_program(r"C:\G\g.exe", Some(generic), None);
        host.add(Place::UserRun, "Mon outil", r#""C:\G\g.exe""#);
        let listing = list(&host);
        assert_eq!(entry(&listing, "Mon outil").name, "Mon outil", "{generic}");
        host.remove(Place::UserRun, "Mon outil");
    }
    // Un mot qui contient un mot générique n'est pas générique.
    host.add_program(r"C:\G\g.exe", Some("Updater Pro"), None);
    host.add(Place::UserRun, "Mon outil", r#""C:\G\g.exe""#);
    assert_eq!(entry(&list(&host), "Mon outil").name, "Updater Pro");
    // Une description vide, ou qui ne contient que des caractères invisibles.
    for empty in ["", "   ", "\u{200b}\u{202e}"] {
        host.add_program(r"C:\G\g.exe", Some(empty), None);
        assert_eq!(entry(&list(&host), "Mon outil").name, "Mon outil", "{empty:?}");
    }
}

#[test]
fn the_description_is_only_read_for_a_file_that_was_found() {
    let host = pc();
    // Un programme hôte : on n'en tire ni nom ni éditeur.
    host.add_program(r"C:\Windows\System32\rundll32.exe", Some("Hôte Windows"), Some("Microsoft"));
    host.add(Place::UserRun, "Lancement rapide", "rundll32.exe shell32.dll,Control_RunDLL");
    // Un chemin manquant (même si le fichier est décrit quelque part), un nom nu introuvable.
    host.add(Place::UserRun, "Manquante", r#""C:\Absent\absent.exe""#);
    host.add(Place::UserRun, "Nue", "inconnue.exe");
    let listing = list(&host);

    let hosted = entry(&listing, "Lancement rapide");
    assert_eq!(hosted.target, TargetKind::Hosted);
    assert_eq!(hosted.host.as_deref(), Some("rundll32.exe"));
    assert_eq!((hosted.name.as_str(), hosted.publisher.as_deref()), ("Lancement rapide", None));

    let missing = entry(&listing, "Manquante");
    assert_eq!(missing.target, TargetKind::Missing);
    assert_eq!((missing.host.as_deref(), missing.publisher.as_deref()), (None, None));

    let bare = entry(&listing, "Nue");
    assert_eq!(bare.target, TargetKind::Unknown, "jamais « manquante » sans chemin");
}

#[test]
fn the_publisher_is_what_the_file_declares_cleaned() {
    let host = pc();
    host.add_program(r"C:\Apps\Odd\odd.exe", Some("Odd"), Some("  Évil\u{202e}Corp \t Inc.  "));
    host.add(Place::UserRun, "Steam", r#""C:\Apps\Steam\steam.exe" -silent"#);
    host.add(Place::UserRun, "Odd", r#""C:\Apps\Odd\odd.exe""#);
    host.add(Place::UserRun, "Sans", r#""C:\Apps\Sans\sans.exe""#);
    let listing = list(&host);
    assert_eq!(entry(&listing, "Steam").publisher.as_deref(), Some("Valve Corporation"));
    assert_eq!(entry(&listing, "Odd").publisher.as_deref(), Some("ÉvilCorp Inc."));
    assert_eq!(entry(&listing, "Sans").publisher, None);
}

#[test]
fn a_shortcut_loses_its_lnk_extension_but_a_run_value_keeps_its_name() {
    let host = pc();
    host.add(Place::UserFolder, "Mon raccourci.LNK", r#""C:\Apps\Sans\sans.exe""#);
    host.add(Place::UserFolder, "script.bat", r#""C:\Apps\Sans\sans.exe""#);
    host.add(Place::UserRun, "outil.lnk", r#""C:\Apps\Sans\sans.exe""#);
    let listing = list(&host);
    assert_eq!(entry(&listing, "Mon raccourci.LNK").name, "Mon raccourci");
    assert_eq!(entry(&listing, "script.bat").name, "script.bat");
    assert_eq!(entry(&listing, "outil.lnk").name, "outil.lnk");
}

#[test]
fn the_raw_name_is_an_alias_only_when_it_differs_ignoring_case() {
    let host = pc();
    host.add_program(r"C:\Apps\Casse\c.exe", Some("discord"), None);
    host.add(Place::UserRun, "DISCORD", r#""C:\Apps\Casse\c.exe""#);
    host.add(Place::UserRun, "Steam", r#""C:\Apps\Steam\steam.exe""#);
    host.add(Place::UserFolder, "Steam.lnk", r#""C:\Apps\Steam\steam.exe""#);
    let listing = list(&host);
    assert_eq!(entry(&listing, "DISCORD").name, "discord");
    assert_eq!(entry(&listing, "DISCORD").alias, None);
    assert_eq!(entry(&listing, "Steam").alias, None);
    // Le raccourci, sans son extension, porte le même nom : pas d'alias non plus.
    assert_eq!(entry(&listing, "Steam.lnk").alias, None);
}

#[test]
fn what_is_shown_is_cleaned_and_a_nameless_entry_still_has_a_name() {
    let host = pc();
    host.add_program(r"C:\Apps\Odd\odd.exe", Some("Odd\u{202e}Tool"), None);
    host.add(Place::UserRun, "Nom\u{200b}  trop\tbizarre", r#""C:\Apps\Odd\odd.exe""#);
    host.add(Place::UserRun, "\u{202e}\u{200b}", r"C:\x\y.exe");
    host.add(Place::UserRun, &"n".repeat(200), "rundll32.exe x");
    let listing = list(&host);
    let odd = entry(&listing, "Nom\u{200b}  trop\tbizarre");
    assert_eq!(odd.name, "OddTool");
    assert_eq!(odd.alias.as_deref(), Some("Nom trop bizarre"));
    // Les noms bruts qui ne disent rien : un nom de remplacement.
    assert_eq!(entry(&listing, "\u{202e}\u{200b}").name, "Sans nom");
    let long = entry(&listing, &"n".repeat(200));
    assert_eq!(long.name.chars().count(), 80);
    assert_eq!(
        long.raw_name.chars().count(),
        200,
        "le nom brut sert à retrouver la valeur : intact"
    );
}

#[test]
fn a_file_of_the_sync_list_gets_the_sync_hint() {
    let host = pc();
    host.add_program(r"C:\Apps\Dropbox\Dropbox.EXE", Some("Dropbox"), None);
    host.add(Place::UserRun, "OneDrive", r#""C:\Apps\OneDrive\OneDrive.exe" /background"#);
    host.add(Place::UserRun, "Dropbox", r#""C:\Apps\Dropbox\Dropbox.EXE""#);
    host.add(Place::UserRun, "Steam", r#""C:\Apps\Steam\steam.exe""#);
    // Une app de synchro dont le fichier n'existe pas n'est pas reconnue : on ne devine pas.
    host.add(Place::UserRun, "Google Drive", r#""C:\Absent\googledrivefs.exe""#);
    let listing = list(&host);
    assert_eq!(entry(&listing, "OneDrive").hint, Some(Hint::Sync));
    assert_eq!(entry(&listing, "Dropbox").hint, Some(Hint::Sync));
    assert_eq!(entry(&listing, "Steam").hint, None);
    assert_eq!(entry(&listing, "Google Drive").hint, None);
}

#[test]
fn the_state_and_the_date_come_from_the_approval() {
    let host = pc();
    host.add(Place::UserRun, "Active", r#""C:\Apps\Sans\sans.exe""#);
    host.add(Place::UserRun, "Coupée", r#""C:\Apps\Sans\sans.exe""#);
    host.add(Place::UserRun, "Illisible", r#""C:\Apps\Sans\sans.exe""#);
    host.add(Place::UserRun, "Refusée", r#""C:\Apps\Sans\sans.exe""#);
    let stamp = host.file_time_now() - 3 * 24 * 3600 * 10_000_000;
    host.set_bytes(Place::UserRun, "Coupée", &cut_bytes(stamp));
    host.set_bytes(Place::UserRun, "Illisible", &[7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    host.make_approval_unreadable(Place::UserRun, "Refusée");
    let listing = list(&host);
    assert_eq!(entry(&listing, "Active").state, State::Enabled);
    let cut = entry(&listing, "Coupée");
    assert_eq!(cut.state, State::Disabled);
    let now_ms = host.now().unix_millis();
    assert_eq!(cut.disabled_ms, Some(now_ms - 3 * 24 * 3600 * 1000));
    assert_eq!(entry(&listing, "Illisible").state, State::Unknown);
    assert_eq!(entry(&listing, "Refusée").state, State::Unknown);
}

// --- Shinku lui-même ---

#[test]
fn the_entry_of_shinku_is_the_run_value_with_exactly_the_command_of_this_install() {
    let host = pc();
    host.add(Place::UserRun, "Shinku", OWN);
    host.add(Place::UserRun32, "Shinku", OWN);
    host.add(Place::UserFolder, "Shinku", OWN);
    host.add(Place::MachineRun, "Shinku", OWN);
    let listing = list(&host);
    let owns: Vec<Place> =
        listing.entries.iter().filter(|entry| entry.own).map(|entry| entry.place).collect();
    assert_eq!(owns, [Place::UserRun], "ni les autres places, ni le PC");
    assert!(!entry_in(&listing, Place::UserRun).adjustable);
}

fn entry_in(listing: &Listing, place: Place) -> &Entry {
    listing.entries.iter().find(|entry| entry.place == place).unwrap()
}

#[test]
fn a_value_named_shinku_with_another_command_is_an_ordinary_entry() {
    let host = pc();
    for command in [
        r#""C:\Ailleurs\shinku-agent.exe" --demarrage"#,
        r#""C:\Program Files\Shinku\shinku-agent.exe""#,
        r#""C:\Program Files\Shinku\shinku-agent.exe" --demarrage --autre"#,
        r#"C:\Program Files\Shinku\shinku-agent.exe --demarrage"#,
    ] {
        host.add(Place::UserRun, "Shinku", command);
        let listing = list(&host);
        let shinku = entry(&listing, "Shinku");
        assert!(!shinku.own, "{command}");
        assert!(shinku.adjustable, "{command}");
        host.remove(Place::UserRun, "Shinku");
    }
    // Un autre nom avec la bonne commande n'est pas celui de Shinku non plus.
    host.add(Place::UserRun, "Autre", OWN);
    assert!(!entry(&list(&host), "Autre").own);
    // Sans commande connue (installation introuvable), aucune entrée n'est la sienne.
    host.add(Place::UserRun, "Shinku", OWN);
    assert!(enumerate(&host, None, None).entries.iter().all(|entry| !entry.own));
}

// --- Ce qui se règle ---

#[test]
fn only_account_entries_with_a_known_state_and_a_unique_exact_id_are_adjustable() {
    let host = pc();
    host.add(Place::UserRun, "Active", r#""C:\Apps\Sans\sans.exe""#);
    host.add(Place::UserRun32, "Coupée", r#""C:\Apps\Sans\sans.exe""#);
    host.set_bytes(Place::UserRun32, "Coupée", &cut_bytes(host.file_time_now()));
    host.add(Place::UserFolder, "Raccourci.lnk", r#""C:\Apps\Sans\sans.exe""#);
    host.add(Place::UserRun, "Illisible", r#""C:\Apps\Sans\sans.exe""#);
    host.set_bytes(Place::UserRun, "Illisible", &[6, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]);
    host.add(Place::UserRun, "Shinku", OWN);
    host.add_entry(
        Place::UserRun,
        RawEntry {
            name: "Nom \u{fffd} approximatif".to_owned(),
            name_exact: false,
            command: "x".into(),
        },
    );
    host.add(Place::MachineRun, "Active PC", r#""C:\Apps\Sans\sans.exe""#);
    host.add(Place::MachineRun32, "Coupée PC", r#""C:\Apps\Sans\sans.exe""#);
    host.set_bytes(Place::MachineRun32, "Coupée PC", &cut_bytes(host.file_time_now()));
    host.add(Place::MachineFolder, "PC.lnk", r#""C:\Apps\Sans\sans.exe""#);
    let listing = list(&host);

    let adjustable: Vec<&str> = listing
        .entries
        .iter()
        .filter(|entry| entry.adjustable)
        .map(|entry| entry.raw_name.as_str())
        .collect();
    assert_eq!(adjustable, ["Active", "Coupée", "Raccourci.lnk"]);
    // Aucune entrée du PC ne se règle, quel que soit son état.
    assert!(listing.entries.iter().filter(|e| e.scope() == Scope::Machine).all(|e| !e.adjustable));
}

#[test]
fn two_entries_that_fall_on_the_same_id_are_both_unadjustable() {
    let host = pc();
    // Même place, même nom : possible seulement si la place répond mal, mais alors
    // personne ne sait laquelle on réglerait.
    host.add(Place::UserRun, "Doublon", r#""C:\Apps\Sans\sans.exe""#);
    host.add(Place::UserRun, "Doublon", r#""C:\Apps\Steam\steam.exe""#);
    host.add(Place::UserRun, "Seule", r#""C:\Apps\Sans\sans.exe""#);
    let listing = list(&host);
    let doubles: Vec<&Entry> =
        listing.entries.iter().filter(|entry| entry.raw_name == "Doublon").collect();
    assert_eq!(doubles.len(), 2);
    assert!(doubles.iter().all(|entry| !entry.adjustable));
    assert!(entry(&listing, "Seule").adjustable);
    // Et le changement le refuse : aucune entrée ne se retrouve de façon sûre.
    let id = EntryId::new(Place::UserRun, "Doublon");
    assert_eq!(toggle(&host, None, id, false), Err(ToggleError::UnknownEntry));
    assert!(host.writes().is_empty());
}

// --- L'énumération ---

#[test]
fn the_six_places_are_listed_account_first_then_the_pc_each_sorted() {
    let host = pc();
    host.add(Place::MachineRun, "zeta", r"C:\x.exe");
    host.add(Place::MachineFolder, "Alpha.lnk", r"C:\x.exe");
    host.add(Place::UserFolder, "beta.lnk", r"C:\x.exe");
    host.add(Place::UserRun32, "Zèbre", r"C:\x.exe");
    host.add(Place::UserRun, "alpha", r"C:\x.exe");
    host.add(Place::UserRun, "Gamma", r"C:\x.exe");
    host.add(Place::MachineRun32, "Beta PC", r"C:\x.exe");
    let listing = list(&host);
    let order: Vec<&str> = listing.entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(order, ["alpha", "beta", "Gamma", "Zèbre", "Alpha", "Beta PC", "zeta"]);
    let scopes: Vec<Scope> = listing.entries.iter().map(Entry::scope).collect();
    assert_eq!(
        scopes,
        [
            Scope::User,
            Scope::User,
            Scope::User,
            Scope::User,
            Scope::Machine,
            Scope::Machine,
            Scope::Machine
        ]
    );
}

#[test]
fn the_same_name_is_sorted_by_alias_then_by_id() {
    let host = pc();
    host.add_program(r"C:\Apps\A\a.exe", Some("Même nom"), None);
    host.add(Place::UserRun, "Zzz", r#""C:\Apps\A\a.exe""#);
    host.add(Place::UserRun, "Aaa", r#""C:\Apps\A\a.exe""#);
    host.add(Place::UserRun32, "Mmm", r#""C:\Apps\A\a.exe""#);
    host.add(Place::UserRun, "Même nom", r#""C:\Apps\A\a.exe""#);
    let listing = list(&host);
    let aliases: Vec<Option<&str>> = listing.entries.iter().map(|e| e.alias.as_deref()).collect();
    assert_eq!(aliases, [None, Some("Aaa"), Some("Mmm"), Some("Zzz")]);
    // Deux entrées de même nom et de même alias : par identifiant.
    let host = pc();
    host.add(Place::UserRun, "Twin", r"C:\x.exe");
    host.add(Place::UserRun32, "Twin", r"C:\x.exe");
    host.add(Place::UserFolder, "Twin", r"C:\x.exe");
    let ids: Vec<String> = list(&host).entries.iter().map(|entry| entry.id.to_string()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn an_unreadable_place_is_reported_and_the_others_are_still_listed() {
    let host = pc();
    host.add(Place::UserRun, "Visible", r"C:\x.exe");
    host.add(Place::UserFolder, "Cachée.lnk", r"C:\x.exe");
    host.add(Place::MachineRun32, "PC", r"C:\x.exe");
    host.make_unreadable(Place::UserFolder);
    host.make_unreadable(Place::MachineRun);
    let listing = list(&host);
    assert_eq!(listing.unreadable, [Place::UserFolder, Place::MachineRun]);
    let names: Vec<&str> = listing.entries.iter().map(|entry| entry.raw_name.as_str()).collect();
    assert_eq!(names, ["Visible", "PC"]);
}

#[test]
fn an_empty_machine_has_empty_places_and_nothing_unreadable() {
    let listing = list(&FakeStartupHost::new());
    assert_eq!(listing, Listing::default());
}

#[test]
fn counting_skips_shinku_and_the_coupled_and_gives_nothing_when_a_place_is_unreadable() {
    let host = pc();
    host.add(Place::UserRun, "A", r"C:\x.exe");
    host.add(Place::UserRun, "Shinku", OWN);
    host.add(Place::UserRun32, "B", r"C:\x.exe");
    host.add(Place::UserFolder, "C.lnk", r"C:\x.exe");
    host.add(Place::MachineRun, "D", r"C:\x.exe");
    host.add(Place::MachineRun32, "E", r"C:\x.exe");
    host.add(Place::MachineFolder, "F.lnk", r"C:\x.exe");
    // Coupée : ne compte pas. Illisible : ne compte pas non plus (pas « active »).
    host.set_bytes(Place::UserRun32, "B", &cut_bytes(host.file_time_now()));
    host.set_bytes(Place::MachineRun32, "E", &[6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(count_enabled(&host, Some(OWN)), Some(4));
    // Sans la commande de Shinku, son entrée est une entrée comme une autre.
    assert_eq!(count_enabled(&host, None), Some(5));
    host.make_unreadable(Place::MachineFolder);
    assert_eq!(count_enabled(&host, Some(OWN)), None);
    assert_eq!(Probe::new(Arc::new(pc()), None).count(), Some(0));
}

// --- Changer l'état ---

fn account_pc() -> FakeStartupHost {
    let host = pc();
    host.add(Place::UserRun, "Discord", r#""C:\Apps\Discord\app.exe" --background"#);
    host.add(Place::UserRun32, "Outil 32", r#""C:\Apps\Sans\sans.exe""#);
    host.add(Place::UserFolder, "Raccourci.lnk", r#""C:\Apps\Sans\sans.exe""#);
    host
}

#[test]
fn cutting_writes_03_and_the_time_of_now_then_reads_it_back() {
    let host = account_pc();
    let id = id_of(&host, "Discord");
    let change = toggle(&host, Some(OWN), id, false).unwrap();
    let stamp = host.file_time_now();
    assert_eq!(change, Change { name: "Discord".into(), file_time: stamp, changed: true });
    let writes = host.writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].place, Place::UserRun);
    assert_eq!(writes[0].name, "Discord");
    assert_eq!(writes[0].bytes, cut_bytes(stamp));
    assert_eq!(&writes[0].bytes[..4], [3, 0, 0, 0]);
    assert_eq!(entry(&list(&host), "Discord").state, State::Disabled);
}

#[test]
fn restoring_writes_02_and_eight_zeros() {
    let host = account_pc();
    host.set_bytes(Place::UserRun, "Discord", &cut_bytes(host.file_time_now()));
    let id = id_of(&host, "Discord");
    let change = toggle(&host, Some(OWN), id, true).unwrap();
    assert_eq!(change, Change { name: "Discord".into(), file_time: 0, changed: true });
    let writes = host.writes();
    assert_eq!(writes[0].bytes, restore_bytes());
    assert_eq!(writes[0].bytes, [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(entry(&list(&host), "Discord").state, State::Enabled);
}

#[test]
fn each_account_place_writes_under_its_own_place_and_exact_name() {
    let host = account_pc();
    for (place, name) in [
        (Place::UserRun, "Discord"),
        (Place::UserRun32, "Outil 32"),
        (Place::UserFolder, "Raccourci.lnk"),
    ] {
        let id = id_of(&host, name);
        assert_eq!(id.place(), place);
        toggle(&host, Some(OWN), id, false).unwrap();
    }
    let places: Vec<(Place, String)> =
        host.writes().into_iter().map(|write| (write.place, write.name)).collect();
    assert_eq!(
        places,
        [
            (Place::UserRun, "Discord".to_owned()),
            (Place::UserRun32, "Outil 32".to_owned()),
            (Place::UserFolder, "Raccourci.lnk".to_owned()),
        ]
    );
}

#[test]
fn nothing_is_written_when_the_state_is_already_right() {
    let host = account_pc();
    let id = id_of(&host, "Discord");
    let change = toggle(&host, Some(OWN), id, true).unwrap();
    assert!(!change.changed);
    host.set_bytes(Place::UserRun, "Discord", &cut_bytes(host.file_time_now() - 1_000_000));
    let change = toggle(&host, Some(OWN), id, false).unwrap();
    assert!(!change.changed);
    assert_eq!(change.file_time, host.file_time_now() - 1_000_000, "la date déjà là");
    assert!(host.writes().is_empty());
}

#[test]
fn an_unknown_or_ambiguous_id_changes_nothing() {
    let host = account_pc();
    let gone = EntryId::new(Place::UserRun, "Disparue");
    assert_eq!(toggle(&host, Some(OWN), gone, false), Err(ToggleError::UnknownEntry));
    // Une entrée disparue entre la vue et le geste.
    let id = id_of(&host, "Discord");
    host.remove(Place::UserRun, "Discord");
    assert_eq!(toggle(&host, Some(OWN), id, false), Err(ToggleError::UnknownEntry));
    assert!(host.writes().is_empty());
}

#[test]
fn what_cannot_be_adjusted_is_refused_without_a_write() {
    let host = account_pc();
    host.add(Place::UserRun, "Shinku", OWN);
    host.add(Place::UserRun, "06", r"C:\x.exe");
    host.set_bytes(Place::UserRun, "06", &[6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    host.add(Place::UserRun, "07", r"C:\x.exe");
    host.set_bytes(Place::UserRun, "07", &[7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    host.add(Place::UserRun, "Longue", r"C:\x.exe");
    host.set_bytes(Place::UserRun, "Longue", &[3, 0, 0, 0, 1, 2, 3]);
    host.add(Place::UserRun, "Refusée", r"C:\x.exe");
    host.make_approval_unreadable(Place::UserRun, "Refusée");
    host.add(Place::UserRun, "Approximatif", r"C:\x.exe");
    host.add_entry(
        Place::UserRun,
        RawEntry { name: "Nom \u{fffd}".into(), name_exact: false, command: "x".into() },
    );
    host.add(Place::MachineRun, "Du PC", r"C:\x.exe");
    host.add(Place::MachineRun32, "Du PC 32", r"C:\x.exe");
    host.add(Place::MachineFolder, "Du PC.lnk", r"C:\x.exe");
    let listing = list(&host);
    let refused: Vec<EntryId> =
        listing.entries.iter().filter(|entry| !entry.adjustable).map(|entry| entry.id).collect();
    assert_eq!(refused.len(), 9);
    for id in refused {
        for enabled in [true, false] {
            assert_eq!(
                toggle(&host, Some(OWN), id, enabled),
                Err(ToggleError::NotAdjustable),
                "{id} {enabled}"
            );
        }
    }
    assert!(host.writes().is_empty(), "rien n'a été écrit, pour aucune de ces entrées");
}

#[test]
fn a_refused_write_is_a_failure() {
    let host = account_pc();
    let id = id_of(&host, "Discord");
    host.refuse_writes();
    assert_eq!(toggle(&host, Some(OWN), id, false), Err(ToggleError::Failed));
    assert_eq!(entry(&list(&host), "Discord").state, State::Enabled);
}

#[test]
fn a_write_that_does_not_read_back_as_wanted_is_a_failure() {
    let host = account_pc();
    let id = id_of(&host, "Discord");
    // Windows dit oui, mais la valeur relue est celle d'avant : c'est la relecture qui le voit.
    host.ignore_writes();
    assert_eq!(toggle(&host, Some(OWN), id, false), Err(ToggleError::Failed));
    assert_eq!(host.writes().len(), 1, "l'écriture a bien été tentée");
    host.set_bytes(Place::UserRun, "Discord", &cut_bytes(host.file_time_now()));
    assert_eq!(toggle(&host, Some(OWN), id, true), Err(ToggleError::Failed));
}

#[test]
fn an_unreadable_place_makes_the_change_fail_rather_than_guess() {
    let host = account_pc();
    let id = id_of(&host, "Discord");
    host.make_unreadable(Place::UserRun);
    assert_eq!(toggle(&host, Some(OWN), id, false), Err(ToggleError::Failed));
    assert!(host.writes().is_empty());
}

#[test]
fn the_entry_is_looked_up_again_at_the_time_of_the_gesture() {
    let host = account_pc();
    let id = id_of(&host, "Discord");
    // Pendant ce temps, la personne a coupé l'entrée dans le Gestionnaire des tâches.
    host.set_bytes(Place::UserRun, "Discord", &cut_bytes(host.file_time_now()));
    assert!(!toggle(&host, Some(OWN), id, false).unwrap().changed);
    // Ou l'app a pris un réglage que Shinku ne lit pas : on n'y touche pas.
    host.set_bytes(Place::UserRun, "Discord", &[6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(toggle(&host, Some(OWN), id, false), Err(ToggleError::NotAdjustable));
    assert!(host.writes().is_empty());
}

#[test]
fn the_errors_speak_french() {
    assert_eq!(ToggleError::UnknownEntry.to_string(), "cette entrée n'existe plus");
    assert_eq!(ToggleError::NotAdjustable.to_string(), "cette entrée ne se règle pas ici");
    assert_eq!(ToggleError::Failed.to_string(), "le changement n'a pas pu être fait");
}

// --- Après relecture : noms approximatifs, lecteurs, incertitudes, bornes ---

#[test]
fn a_name_that_is_not_valid_utf16_has_no_state_and_is_not_counted_as_active() {
    let host = pc();
    // La valeur d'approbation « coupée » est posée sous le vrai nom ; l'hôte, lui,
    // n'a pu énumérer que le nom approximatif (caractère de remplacement).
    host.set_bytes(Place::UserRun, "Vrai X", &cut_bytes(host.file_time_now()));
    host.add_entry(
        Place::UserRun,
        RawEntry { name: "Vrai \u{fffd}".into(), name_exact: false, command: r"C:\x.exe".into() },
    );
    host.add(Place::UserRun, "Normale", r"C:\x.exe");
    let listing = list(&host);
    let approximate = entry(&listing, "Vrai \u{fffd}");
    // Chercher l'approbation sous ce nom ne trouverait rien, donc « active » : faux.
    assert_eq!(approximate.state, State::Unknown);
    assert!(!approximate.adjustable);
    assert!(!approximate.approval_present);
    assert_eq!(entry(&listing, "Normale").state, State::Enabled);
    // Elle ne compte pas parmi les apps qui se lancent.
    assert_eq!(count_enabled(&host, Some(OWN)), Some(1));
    // Et rien ne peut l'écrire.
    assert_eq!(toggle(&host, Some(OWN), approximate.id, false), Err(ToggleError::NotAdjustable));
    assert!(host.writes().is_empty());
}

#[test]
fn nothing_is_looked_up_or_read_on_a_drive_that_is_not_fixed() {
    let host = pc();
    host.add_remote_drive('Z');
    // Le fichier « existe » et porte une identité : un lecteur réseau ne doit
    // pourtant ni être interrogé ni être lu.
    host.add_program(r"Z:\Partage\app.exe", Some("Une app"), Some("Éditeur lointain"));
    host.add(Place::UserRun, "Lointaine", r#""Z:\Partage\app.exe" /x"#);
    host.add(Place::UserRun, "Locale", r#""C:\Apps\Steam\steam.exe""#);
    let listing = list(&host);
    let remote = entry(&listing, "Lointaine");
    assert_eq!(remote.target, TargetKind::Unknown);
    assert_eq!(remote.publisher, None);
    assert_eq!(remote.name, "Lointaine");
    assert_eq!(entry(&listing, "Locale").publisher.as_deref(), Some("Valve Corporation"));
    assert!(
        host.checked_paths().iter().all(|path| !path.starts_with("Z:")),
        "aucun chemin du lecteur réseau n'a été essayé : {:?}",
        host.checked_paths()
    );
}

#[test]
fn a_shortcut_to_a_folder_or_to_a_file_that_cannot_be_read_is_never_said_uninstalled() {
    let host = pc();
    host.add_folder(r"C:\Users\a\Documents");
    host.deny_access(r"C:\Prive\app.exe");
    host.add(Place::UserFolder, "Dossier.lnk", r#""C:\Users\a\Documents""#);
    host.add(Place::UserFolder, "Refusé.lnk", r#""C:\Prive\app.exe""#);
    host.add(Place::UserFolder, "Parti.lnk", r#""C:\Apps\Parti\parti.exe""#);
    let listing = list(&host);
    assert_eq!(entry(&listing, "Dossier.lnk").target, TargetKind::Unknown);
    assert_eq!(entry(&listing, "Refusé.lnk").target, TargetKind::Unknown);
    assert_eq!(entry(&listing, "Parti.lnk").target, TargetKind::Missing);
}

#[test]
fn an_entry_whose_command_is_empty_is_kept_with_an_unknown_target() {
    // Une commande trop longue, ou un lien du dossier Démarrage : l'hôte rend
    // l'entrée sans commande. Elle est montrée, réglable, de cible inconnue.
    let host = pc();
    host.add(Place::UserRun, "Sans commande", "");
    host.add(Place::UserFolder, "Lien.lnk", "");
    let listing = list(&host);
    for name in ["Sans commande", "Lien.lnk"] {
        let found = entry(&listing, name);
        assert_eq!(found.target, TargetKind::Unknown, "{name}");
        assert!(found.adjustable, "{name}");
    }
}

#[test]
fn a_place_with_more_than_three_hundred_entries_says_it_is_incomplete() {
    let host = pc();
    for n in 0..MAX_ENTRIES {
        host.add(Place::UserRun, &format!("app{n:03}"), r"C:\x.exe");
    }
    let listing = list(&host);
    assert_eq!(listing.entries.len(), MAX_ENTRIES);
    assert!(listing.incomplete.is_empty(), "300 entrées : tout est lu");
    assert_eq!(count_enabled(&host, Some(OWN)), Some(MAX_ENTRIES as u64));

    host.add(Place::UserRun, "la 301e", r"C:\x.exe");
    let listing = list(&host);
    assert_eq!(listing.entries.len(), MAX_ENTRIES, "la borne tient");
    assert_eq!(listing.incomplete, [Place::UserRun]);
    assert!(listing.unreadable.is_empty());
    // La liste dite incomplète vaut le même chiffre qu'une place illisible : aucun.
    assert_eq!(count_enabled(&host, Some(OWN)), None);
    // La page la reçoit comme une portée dont la lecture est partielle.
    let view = StartupView::from(&listing);
    assert_eq!(view.unreadable, [Scope::User]);
}

#[test]
fn a_place_the_host_says_it_left_out_something_of_is_incomplete() {
    let host = pc();
    host.add(Place::MachineRun, "Visible", r"C:\x.exe");
    host.make_incomplete(Place::MachineRun);
    let listing = list(&host);
    assert_eq!(listing.incomplete, [Place::MachineRun]);
    assert_eq!(listing.entries.len(), 1, "ce qui est lu est montré");
    assert_eq!(count_enabled(&host, Some(OWN)), None);
    assert_eq!(StartupView::from(&listing).unreadable, [Scope::Machine]);
}

#[test]
fn an_incomplete_place_still_lets_an_entry_it_shows_be_adjusted() {
    // Une liste tronquée ne doit pas servir à empêcher de régler ce qu'on voit.
    let host = account_pc();
    host.make_incomplete(Place::UserRun);
    let id = id_of(&host, "Discord");
    assert!(toggle(&host, Some(OWN), id, false).unwrap().changed);
}

#[test]
fn the_inert_host_counts_nothing_instead_of_saying_zero() {
    assert_eq!(count_enabled(&InertStartupHost, None), None);
    assert_eq!(Probe::new(Arc::new(InertStartupHost), None).count(), None);
    assert_eq!(list_with(&InertStartupHost), Listing::default());
}

fn list_with(host: &dyn StartupHost) -> Listing {
    enumerate(host, Some(OWN), None)
}
