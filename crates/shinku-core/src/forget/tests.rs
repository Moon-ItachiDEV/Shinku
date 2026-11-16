//! La décision de `--oublier`, avec un faux hôte : la liste, l'ordre, les trois
//! codes, le refus, le réessai borné en tout, la seconde de vie minimale.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::fake::{FakeHost, LOCAL, ROAMING};
use super::*;

fn done_tree() -> TreeReport {
    TreeReport { removed: 3, busy: 0, kept: 0 }
}

fn busy_tree() -> TreeReport {
    TreeReport { removed: 0, busy: 1, kept: 0 }
}

fn kept_tree() -> TreeReport {
    TreeReport { removed: 0, busy: 0, kept: 1 }
}

/// Un PC où tout est présent et part sans histoire.
fn full_pc() -> FakeHost {
    let mut host = FakeHost::new();
    host.file_default = Removed::Gone;
    host.dir_default = Removed::Gone;
    host.tree_default = done_tree();
    host
}

fn roaming(name: &str) -> String {
    format!("{ROAMING}/Shinku/{name}")
}

fn local(name: &str) -> String {
    format!("{LOCAL}/Shinku/{name}")
}

// --- La ligne de commande ---

#[test]
fn only_the_exact_single_word_is_the_command() {
    assert!(asked(["shinku.exe", "--oublier"]));
    // Le nom du programme ne compte pas, mais il doit être là.
    assert!(asked([r"C:\Program Files\Shinku\shinku.exe", "--oublier"]));
    assert!(!asked(["shinku.exe"]), "sans argument : Shinku démarre");
    assert!(!asked(Vec::<&str>::new()), "pas même un nom de programme");
    assert!(!asked(["shinku.exe", "--oublier", "--oublier"]), "un mot de trop");
    assert!(!asked(["shinku.exe", "--oublier", "x"]));
    assert!(!asked(["shinku.exe", "x", "--oublier"]), "le mot n'est pas le premier");
    assert!(!asked(["shinku.exe", "--OUBLIER"]), "la casse compte");
    assert!(!asked(["shinku.exe", "--oublier=1"]));
    assert!(!asked(["shinku.exe", "-oublier"]));
    assert!(!asked(["shinku.exe", "oublier"]));
    assert!(!asked(["shinku.exe", "--oublier "]), "une espace de plus");
    assert!(!asked(["shinku.exe", ""]));
    assert!(!asked(["shinku.exe", "--retirer"]), "les autres mots sont ceux de l'agent");
}

// --- La liste fermée ---

fn names(files: &[OsString]) -> Vec<&str> {
    files.iter().filter_map(|name| name.to_str()).collect()
}

#[test]
fn the_closed_list_is_exactly_the_files_shinku_writes() {
    let [roaming, local] = list::closed_list();
    assert_eq!(roaming.folder, KnownFolder::RoamingAppData);
    assert_eq!(roaming.dir, "Shinku");
    assert_eq!(
        names(&roaming.files),
        [
            "settings.json",
            "settings.json.tmp",
            "settings.illisible.json",
            "journal.json",
            "journal.json.tmp"
        ]
    );
    assert_eq!(local.folder, KnownFolder::LocalAppData);
    assert_eq!(local.dir, "Shinku");
    assert_eq!(
        names(&local.files),
        ["game.json", "game.json.tmp", "startup.json", "startup.json.tmp"]
    );
}

/// Ce que l'ancienne macro `ShinkuDeleteSettings` du désinstalleur effaçait
/// (`habillage.nsh`, avant ce lot) : la liste du cœur doit la couvrir.
#[test]
fn the_closed_list_covers_what_the_old_installer_macro_deleted() {
    let old_roaming = [
        "settings.json",
        "settings.illisible.json",
        "journal.json",
        "settings.json.tmp",
        "journal.json.tmp",
    ];
    let old_local = ["game.json", "game.json.tmp", "startup.json", "startup.json.tmp"];
    let [roaming, local] = list::closed_list();
    for name in old_roaming {
        assert!(names(&roaming.files).contains(&name), "{name} manque à %APPDATA%\\Shinku");
    }
    for name in old_local {
        assert!(names(&local.files).contains(&name), "{name} manque à %LOCALAPPDATA%\\Shinku");
    }
}

// --- La garde contre un nouveau fichier de réglages ---

/// Les fichiers de code du dépôt qui peuvent écrire un fichier de Shinku, hors
/// essais : le cœur, l'assistant, l'agent et la fenêtre.
fn production_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }
    let core = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = std::fs::canonicalize(core.join("..").join("..")).unwrap();
    let core = workspace.join("crates").join("shinku-core");
    let mut files = Vec::new();
    for root in [
        core.join("src"),
        workspace.join("crates").join("shinku-helper").join("src"),
        workspace.join("crates").join("shinku-agent").join("src"),
        workspace.join("apps").join("shinku").join("src-tauri").join("src"),
    ] {
        walk(&root, &mut files);
    }
    let mut sources = Vec::new();
    for path in files {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        // Les fichiers d'essais, et ce module (qui cite ces mots pour les chercher).
        if matches!(name, "tests.rs" | "testing.rs" | "fake.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        // On ne regarde que le code livré : ce qui précède le module d'essais.
        let shipped = text.split("\n#[cfg(test)]").next().unwrap_or_default().to_owned();
        let relative = path
            .strip_prefix(&workspace)
            .map_or_else(|_| path.display().to_string(), |p| p.display().to_string())
            .replace('\\', "/");
        sources.push((relative, shipped));
    }
    sources.sort();
    sources
}

fn files_with(token: &str) -> Vec<String> {
    production_sources()
        .into_iter()
        .filter(|(_, text)| text.contains(token))
        .map(|(path, _)| path)
        .collect()
}

/// Ce que cet essai sait attraper : un nouveau module qui écrit son fichier par
/// `write_replacing` (la seule façon dont Shinku écrit ses fichiers), ou qui
/// l'écrit par `File::create`, `fs::write`, `create(true)`… sans passer par elle.
/// Il force alors à relire la liste fermée.
///
/// Ce qu'il ne sait pas attraper : une écriture d'un autre genre (une API de
/// Windows, un autre crate, un fichier écrit par WebView2 ou par Tauri lui-même,
/// un chemin qui n'est pas sous les deux dossiers `Shinku`). Pour ceux-là, la
/// liste reste ce que les constructeurs y mettent.
#[test]
fn a_module_that_writes_a_new_file_must_be_in_the_closed_list() {
    let stores = [
        "crates/shinku-core/src/clean/journal.rs",
        "crates/shinku-core/src/game/store.rs",
        "crates/shinku-core/src/settings.rs",
        "crates/shinku-core/src/startup/store.rs",
    ];
    // `write_replacing(` : sa définition (settings.rs) et ses quatre appelants.
    assert_eq!(
        files_with("write_replacing("),
        stores,
        "un module écrit un fichier par write_replacing sans que forget::list le sache : \
         ajoute-le à la liste fermée (et au désinstalleur de la conception)"
    );
    // Les autres façons d'écrire : seule `write_replacing` (settings.rs) écrit les
    // réglages ; platform/mod.rs a la corbeille factice des essais.
    for token in [
        "File::create(",
        "File::create_new(",
        "fs::write(",
        ".create(true)",
        ".create_new(true)",
        ".append(true)",
    ] {
        let found = files_with(token);
        assert!(
            found.iter().all(|path| path == "crates/shinku-core/src/settings.rs"),
            "{token} apparaît ailleurs que dans settings.rs : {found:?}. Si ce fichier est \
             un réglage ou un journal de Shinku, il doit être dans forget::list."
        );
    }
    assert_eq!(
        files_with("fs::rename("),
        ["crates/shinku-core/src/platform/mod.rs", "crates/shinku-core/src/settings.rs"],
        "un nouveau fichier posé par fs::rename doit être dans forget::list"
    );
    // Chaque nom de la liste est bien celui d'un module qui l'écrit.
    let [roaming, local] = list::closed_list();
    assert_eq!(names(&roaming.files).len() + names(&local.files).len(), 9);
}

// --- Le nom de Tauri : une seule source ---

#[test]
fn the_tauri_identifier_is_the_one_in_tauri_conf_json() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("apps")
        .join("shinku")
        .join("src-tauri")
        .join("tauri.conf.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let config: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        config["identifier"].as_str(),
        Some(TAURI_IDENTIFIER),
        "tauri.conf.json et forget::TAURI_IDENTIFIER ne disent plus la même chose : \
         --oublier viserait le mauvais dossier"
    );
}

// --- L'ordre ---

#[test]
fn files_first_then_the_empty_folders_then_the_two_trees() {
    let host = full_pc();
    assert_eq!(run(&host), Outcome::Done);
    let mut expected: Vec<String> = [
        "settings.json",
        "settings.json.tmp",
        "settings.illisible.json",
        "journal.json",
        "journal.json.tmp",
    ]
    .iter()
    .map(|name| format!("file {}", roaming(name)))
    .collect();
    expected.extend(
        ["game.json", "game.json.tmp", "startup.json", "startup.json.tmp"]
            .iter()
            .map(|name| format!("file {}", local(name))),
    );
    expected.push(format!("dir {ROAMING}/Shinku"));
    expected.push(format!("dir {LOCAL}/Shinku"));
    expected.push(format!("tree {ROAMING}/fr.shinku.app"));
    expected.push(format!("tree {LOCAL}/fr.shinku.app"));
    assert_eq!(host.removals(), expected);
}

#[test]
fn an_unknown_folder_is_skipped_not_guessed() {
    let mut host = full_pc();
    host.folders.retain(|(folder, _)| *folder == KnownFolder::RoamingAppData);
    assert_eq!(run(&host), Outcome::Done);
    assert!(host.removals().iter().all(|line| !line.contains(LOCAL)), "{:?}", host.removals());
    assert!(host.removals().iter().any(|line| line.contains(ROAMING)));
}

// --- Les trois codes ---

#[test]
fn everything_gone_is_code_zero() {
    let host = full_pc();
    let outcome = run(&host);
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(outcome.exit_code(), 0);
}

#[test]
fn nothing_to_do_is_code_zero_too() {
    let host = FakeHost::new();
    let outcome = run(&host);
    assert_eq!(outcome.exit_code(), 0);
    // Il a quand même regardé partout.
    assert_eq!(host.removals().len(), 9 + 2 + 2);
}

#[test]
fn something_left_is_code_one() {
    // Un fichier que rien ne fera partir (un lien, un fichier à deux noms…).
    let host = full_pc();
    host.script_file(&roaming("journal.json"), &[Removed::Kept; 40]);
    let outcome = run(&host);
    assert_eq!(outcome, Outcome::Left);
    assert_eq!(outcome.exit_code(), 1);
    // Ce qui est gardé ne se réessaie pas : une seule passe, donc pas d'attente
    // autre que celle qui complète la seconde de vie.
    assert_eq!(host.removals().len(), 13);
}

#[test]
fn a_folder_that_is_not_empty_is_code_one() {
    // Un fichier hors liste dans %APPDATA%\Shinku : le dossier reste.
    let host = full_pc();
    host.script_dir(&format!("{ROAMING}/Shinku"), &[Removed::Kept]);
    assert_eq!(run(&host).exit_code(), 1);
}

#[test]
fn a_tree_that_keeps_a_link_is_code_one() {
    let host = full_pc();
    host.script_tree(&format!("{LOCAL}/fr.shinku.app"), &[kept_tree()]);
    assert_eq!(run(&host), Outcome::Left);
    // Les autres suppressions ont eu lieu.
    assert!(host.removals().contains(&format!("tree {ROAMING}/fr.shinku.app")));
}

#[test]
fn something_still_busy_after_five_seconds_is_code_one() {
    let host = full_pc();
    host.script_file(&local("game.json"), &[Removed::Busy; 1000]);
    assert_eq!(run(&host), Outcome::Left);
}

#[test]
fn an_elevated_process_refuses_with_code_two_and_touches_nothing() {
    let mut host = full_pc();
    host.elevated = true;
    let outcome = run(&host);
    assert_eq!(outcome, Outcome::Refused);
    assert_eq!(outcome.exit_code(), 2);
    // Pas une suppression, pas même une demande de dossier : rien n'est regardé.
    assert_eq!(host.log(), ["elevated?"]);
}

// --- Le réessai, borné en tout ---

#[test]
fn a_busy_file_is_retried_and_leaves_when_it_frees_up() {
    let host = full_pc();
    host.script_file(&roaming("settings.json"), &[Removed::Busy, Removed::Busy, Removed::Gone]);
    assert_eq!(run(&host), Outcome::Done);
    // Trois passes complètes : tout est repris, pas seulement le fichier occupé.
    let calls = host.removals().iter().filter(|line| line.ends_with("settings.json")).count();
    assert_eq!(calls, 3);
    assert_eq!(host.removals().len(), 13 * 3);
    // Deux pauses, puis la seconde de vie minimale est déjà acquise.
    assert_eq!(host.sleeps()[..2], [RETRY_PAUSE, RETRY_PAUSE]);
}

#[test]
fn a_busy_tree_is_retried_too() {
    let host = full_pc();
    host.script_tree(&format!("{LOCAL}/fr.shinku.app"), &[busy_tree(), done_tree()]);
    assert_eq!(run(&host), Outcome::Done);
    assert_eq!(host.removals().len(), 13 * 2);
}

#[test]
fn the_five_seconds_are_for_everything_not_per_file() {
    // Quatre fichiers et une arborescence occupés pour toujours : le temps total
    // d'attente reste cinq secondes, il ne se multiplie pas par le nombre d'occupés.
    let host = full_pc();
    for name in ["settings.json", "journal.json"] {
        host.script_file(&roaming(name), &[Removed::Busy; 1000]);
    }
    for name in ["game.json", "startup.json"] {
        host.script_file(&local(name), &[Removed::Busy; 1000]);
    }
    host.script_tree(&format!("{ROAMING}/fr.shinku.app"), &[busy_tree(); 1000]);
    assert_eq!(run(&host), Outcome::Left);
    assert_eq!(host.total_slept(), RETRY_BUDGET);
    assert_eq!(host.now(), RETRY_BUDGET);
    // 20 pauses de 250 ms, donc 21 passes.
    assert_eq!(host.sleeps().len(), 20);
    assert_eq!(host.removals().len(), 13 * 21);
}

#[test]
fn time_spent_working_counts_against_the_five_seconds() {
    // Chaque suppression coûte 100 ms : une passe de 13 appels en coûte 1,3 s.
    // Le budget s'épuise donc en moins de pauses, et jamais au-delà de 5 s + une passe.
    let mut host = full_pc();
    host.cost = Duration::from_millis(100);
    host.script_file(&roaming("settings.json"), &[Removed::Busy; 1000]);
    assert_eq!(run(&host), Outcome::Left);
    let passes = host.removals().len() / 13;
    assert!(passes <= 4, "{passes} passes : le temps de travail doit compter");
    assert!(host.now() < RETRY_BUDGET + Duration::from_millis(1300) + RETRY_PAUSE);
    assert!(host.total_slept() < RETRY_BUDGET);
}

#[test]
fn the_last_pause_never_goes_past_the_budget() {
    // 13 appels à 90 ms = 1,17 s par passe ; au bout de 4 passes et 3 pauses de
    // 250 ms, il reste moins qu'une pause : la dernière est raccourcie.
    let mut host = full_pc();
    host.cost = Duration::from_millis(90);
    host.script_file(&roaming("settings.json"), &[Removed::Busy; 1000]);
    assert_eq!(run(&host), Outcome::Left);
    assert!(host.sleeps().iter().all(|pause| *pause <= RETRY_PAUSE));
    // L'horloge ne dépasse jamais d'une passe la fin du budget.
    assert!(host.now() <= RETRY_BUDGET + Duration::from_millis(13 * 90));
}

// --- La seconde de vie minimale ---

#[test]
fn it_lives_at_least_one_second_even_with_nothing_to_do() {
    let host = FakeHost::new();
    assert_eq!(run(&host), Outcome::Done);
    assert_eq!(host.total_slept(), MIN_LIFETIME);
    assert_eq!(host.now(), MIN_LIFETIME);
}

#[test]
fn it_lives_at_least_one_second_even_when_it_refuses() {
    let mut host = full_pc();
    host.elevated = true;
    assert_eq!(run(&host), Outcome::Refused);
    assert_eq!(host.now(), MIN_LIFETIME);
}

#[test]
fn it_lives_at_least_one_second_when_everything_goes_fast() {
    let host = full_pc();
    assert_eq!(run(&host), Outcome::Done);
    assert_eq!(host.now(), MIN_LIFETIME);
}

#[test]
fn it_only_sleeps_what_is_missing_of_the_second() {
    // 13 appels à 50 ms = 650 ms de travail : il manque 350 ms.
    let mut host = full_pc();
    host.cost = Duration::from_millis(50);
    assert_eq!(run(&host), Outcome::Done);
    assert_eq!(host.sleeps(), [Duration::from_millis(350)]);
    assert_eq!(host.now(), MIN_LIFETIME);
}

#[test]
fn it_does_not_sleep_more_when_the_work_already_took_a_second() {
    let mut host = full_pc();
    host.cost = Duration::from_millis(200);
    assert_eq!(run(&host), Outcome::Done);
    assert!(host.sleeps().is_empty());
    assert_eq!(host.now(), Duration::from_millis(2600));
}
