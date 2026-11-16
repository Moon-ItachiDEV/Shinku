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

/// Retire d'un source ce qui n'est pas livré : les modules d'essais écrits tout
/// en haut d'un fichier (`#[cfg(test)]` suivi de `mod x;` ou de `mod x { … }`
/// fermé par une accolade en colonne 0) et les éléments isolés qu'une ligne
/// `#[cfg(test)]` précède. Le reste du fichier est gardé : le marqueur n'arrête
/// plus la lecture, et une écriture ajoutée **après** lui est vue.
fn shipped_part(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index] != "#[cfg(test)]" {
            kept.push(lines[index]);
            index += 1;
            continue;
        }
        // On saute l'attribut, ceux qui le suivent, puis l'élément qu'il porte.
        index += 1;
        while lines.get(index).is_some_and(|line| line.starts_with("#[")) {
            index += 1;
        }
        let Some(first) = lines.get(index) else { break };
        index += 1;
        if first.trim_end().ends_with('{') {
            while index < lines.len() && lines[index] != "}" {
                index += 1;
            }
            index += 1;
        }
    }
    kept.join("\n")
}

/// Les fichiers de module que les sources déclarent `#[cfg(test)] mod x;`,
/// chacun résolu à partir du fichier qui le déclare : `dir/x.rs` ou
/// `dir/x/mod.rs` (où `dir` est le dossier du déclarant, ou son dossier de même
/// nom s'il n'est pas un `mod.rs`, `lib.rs` ou `main.rs`), ou le `#[path]`
/// donné, relatif au dossier du déclarant.
fn test_module_files(texts: &[(PathBuf, String)]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for (declarer, text) in texts {
        let parent = declarer.parent().unwrap_or(Path::new(""));
        let stem = declarer.file_stem().and_then(|name| name.to_str()).unwrap_or_default();
        let dir = if matches!(stem, "mod" | "lib" | "main") {
            parent.to_path_buf()
        } else {
            parent.join(stem)
        };
        let lines: Vec<&str> = text.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            if *line != "#[cfg(test)]" {
                continue;
            }
            let mut explicit = None;
            for next in &lines[at + 1..] {
                if let Some(path) = next.strip_prefix("#[path = \"") {
                    explicit = Some(parent.join(path.trim_end_matches("\"]")));
                } else if next.starts_with("#[") {
                } else {
                    if let Some(module) =
                        next.strip_prefix("mod ").and_then(|m| m.strip_suffix(';'))
                    {
                        match explicit {
                            Some(path) => found.push(path),
                            None => {
                                found.push(dir.join(format!("{module}.rs")));
                                found.push(dir.join(module).join("mod.rs"));
                            }
                        }
                    }
                    break;
                }
            }
        }
    }
    found
}

#[test]
fn a_test_module_is_resolved_from_the_file_that_declares_it() {
    let declare = |path: &str, text: &str| (PathBuf::from(path), text.to_owned());
    let texts = vec![
        declare("/s/forget/mod.rs", "#[cfg(test)]\nmod fake;\n"),
        declare("/s/apps/mod.rs", "pub mod fake;\n#[cfg(test)]\nmod tests;\n"),
        declare("/s/startup/desk.rs", "#[cfg(test)]\n#[path = \"desk_pc_tests.rs\"]\nmod pc;\n"),
        declare("/s/privacy/mod.rs", "pub mod fake;\n"),
    ];
    let excluded = test_module_files(&texts);
    // Le faux hôte de forget n'est pas livré ; ceux d'apps et de privacy, si.
    assert!(excluded.contains(&PathBuf::from("/s/forget/fake.rs")));
    assert!(!excluded.contains(&PathBuf::from("/s/apps/fake.rs")));
    assert!(!excluded.contains(&PathBuf::from("/s/privacy/fake.rs")));
    assert!(excluded.contains(&PathBuf::from("/s/apps/tests.rs")));
    assert!(excluded.contains(&PathBuf::from("/s/startup/desk_pc_tests.rs")));
}

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
    let mut texts = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        texts.push((path, text));
    }
    // Les fichiers que le code déclare `#[cfg(test)] mod x;` ne sont pas livrés :
    // chacun est résolu à partir du fichier qui le déclare.
    let test_modules = test_module_files(&texts);
    let mut sources = Vec::new();
    for (path, text) in texts {
        if test_modules.contains(&path) {
            continue;
        }
        let relative = path
            .strip_prefix(&workspace)
            .map_or_else(|_| path.display().to_string(), |p| p.display().to_string())
            .replace('\\', "/");
        sources.push((relative, shipped_part(&text)));
    }
    sources.sort();
    sources
}

/// Combien de fois `token` apparaît, par fichier (les fichiers sans occurrence
/// ne sont pas listés).
fn count_of(token: &str) -> Vec<(String, usize)> {
    production_sources()
        .into_iter()
        .filter_map(|(path, text)| {
            let count = text.matches(token).count();
            (count > 0).then_some((path, count))
        })
        .collect()
}

fn counts(expected: &[(&str, usize)]) -> Vec<(String, usize)> {
    expected.iter().map(|(path, count)| ((*path).to_owned(), *count)).collect()
}

/// Les noms de fichiers que le code livré écrit entre guillemets avec une
/// extension de fichier de réglages ou de sauvegarde.
fn file_name_literals() -> Vec<(String, String)> {
    const EXTENSIONS: [&str; 8] = ["json", "tmp", "bak", "old", "log", "ini", "toml", "dat"];
    let mut found = Vec::new();
    for (path, text) in production_sources() {
        for literal in text.split('"').skip(1).step_by(2) {
            let Some((stem, extension)) = literal.rsplit_once('.') else { continue };
            let plain = !stem.is_empty()
                && literal.chars().all(|c| c.is_alphanumeric() || "._-".contains(c));
            if plain && EXTENSIONS.contains(&extension) {
                found.push((path.clone(), literal.to_owned()));
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

#[test]
fn the_shipped_part_keeps_what_follows_a_test_module_at_the_top() {
    let text = "use a;\n\n#[cfg(test)]\nmod fake;\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n\n\
                fn shipped() { write(1); }\n#[cfg(test)]\nuse b;\n";
    let kept = shipped_part(text);
    assert!(kept.contains("fn shipped()"), "{kept}");
    assert!(kept.contains("use a;"));
    assert!(!kept.contains("mod fake"));
    assert!(!kept.contains("fn t()"));
    assert!(!kept.contains("use b;"));
}

/// Ce que cet essai sait attraper : un nouveau fichier écrit par `write_replacing`
/// (la seule façon dont Shinku écrit ses fichiers), par un appel de plus dans un
/// des quatre magasins, ou par `File::create`, `fs::write`, `.create(true)`,
/// `.append(true)` ou `fs::rename` ailleurs que là où ils sont aujourd'hui ; et
/// un nom de fichier nouveau (`.json`, `.tmp`, `.bak`, `.log`…) écrit entre
/// guillemets dans le code livré. Les comptes sont **par fichier** : rien n'est
/// exempté en entier, et le code qui suit un module d'essais est lu.
///
/// Ce qu'il ne sait pas attraper : une écriture d'une autre sorte (une API de
/// Windows, `OpenOptions` composé d'une autre façon, un autre crate), un nom de
/// fichier construit au lieu d'être écrit en entier, un fichier écrit par
/// WebView2 ou par Tauri lui-même, un élément `#[cfg(test)]` écrit en retrait.
/// Pour ceux-là, la liste reste ce que les constructeurs y mettent.
#[test]
fn a_module_that_writes_a_new_file_must_be_in_the_closed_list() {
    const STORES: [&str; 4] = [
        "crates/shinku-core/src/clean/journal.rs",
        "crates/shinku-core/src/game/store.rs",
        "crates/shinku-core/src/settings.rs",
        "crates/shinku-core/src/startup/store.rs",
    ];
    let msg = "écriture ou nom nouveau. Un réglage ou un journal de Shinku : ajoute son nom à \
               forget::list (et à la conception). Un nom seulement lu ou nommé, que Shinku \
               n'écrit pas : épingle-le dans EXPECTED_FILE_NAMES (et mets le compte à jour \
               dans cet essai)";
    // Chaque magasin appelle `write_replacing` une seule fois (settings.rs la définit
    // en plus : deux occurrences).
    assert_eq!(
        count_of("write_replacing("),
        counts(&[(STORES[0], 1), (STORES[1], 1), (STORES[2], 2), (STORES[3], 1),]),
        "{msg}"
    );
    // Et chaque magasin ne définit qu'un nom de fichier.
    for store in [STORES[0], STORES[1], STORES[3]] {
        let names: Vec<_> =
            file_name_literals().into_iter().filter(|(path, _)| path == store).collect();
        assert_eq!(names.len(), 1, "{store} : {names:?}. {msg}");
    }
    // Les autres façons d'écrire.
    assert_eq!(
        count_of("File::create("),
        counts(&[("crates/shinku-core/src/settings.rs", 1)]),
        "{msg}"
    );
    for token in
        ["File::create_new(", "fs::write(", ".create(true)", ".create_new(true)", ".append(true)"]
    {
        assert_eq!(count_of(token), counts(&[]), "{token} : {msg}");
    }
    assert_eq!(
        count_of("fs::rename("),
        counts(&[
            ("crates/shinku-core/src/platform/mod.rs", 1),
            ("crates/shinku-core/src/settings.rs", 2),
        ]),
        "un nouveau fichier posé par fs::rename : {msg}"
    );
    // Les noms de fichiers écrits en entier : exactement ceux de la liste, plus
    // ce que le code livré lit ou nomme pour d'autres raisons (épinglé ici).
    let names: Vec<(String, String)> = file_name_literals();
    let pinned: Vec<(&str, &str)> = EXPECTED_FILE_NAMES.to_vec();
    let found: Vec<(&str, &str)> =
        names.iter().map(|(path, name)| (path.as_str(), name.as_str())).collect();
    assert_eq!(found, pinned, "un nom de fichier nouveau dans le code livré : {msg}");
    // La liste elle-même.
    let [roaming, local] = list::closed_list();
    assert_eq!(names_of(&roaming.files).len() + names_of(&local.files).len(), 9);
}

fn names_of(files: &[OsString]) -> Vec<&str> {
    names(files)
}

/// Les noms de fichiers que le code livré écrit en entier : les cinq de Shinku
/// (dans les magasins) et ce que d'autres modules lisent ou nomment.
const EXPECTED_FILE_NAMES: &[(&str, &str)] = &[
    ("crates/shinku-core/src/clean/journal.rs", "journal.json"),
    ("crates/shinku-core/src/game/store.rs", "game.json"),
    // Des noms que Shinku lit ou refuse de nettoyer, qu'il n'écrit pas.
    ("crates/shinku-core/src/guard.rs", "DumpStack.log.tmp"),
    ("crates/shinku-core/src/guard.rs", "Windows.old"),
    ("crates/shinku-core/src/platform/windows/helper_host.rs", "desktop.ini"),
    ("crates/shinku-core/src/platform/windows/startup.rs", "desktop.ini"),
    ("crates/shinku-core/src/settings.rs", "settings.illisible.json"),
    ("crates/shinku-core/src/settings.rs", "settings.json"),
    ("crates/shinku-core/src/startup/store.rs", "startup.json"),
];

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
fn an_unknown_folder_is_not_guessed_and_is_not_a_success() {
    let mut host = full_pc();
    host.folders.retain(|(folder, _)| *folder == KnownFolder::RoamingAppData);
    // Rien n'a été regardé du côté de %LOCALAPPDATA% : on ne dit pas « parti ».
    assert_eq!(run(&host), Outcome::Left);
    assert!(host.removals().iter().all(|line| !line.contains(LOCAL)), "{:?}", host.removals());
    assert!(host.removals().iter().any(|line| line.contains(ROAMING)));
}

#[test]
fn no_known_folder_at_all_is_code_one_and_nothing_is_touched() {
    let mut host = full_pc();
    host.folders.clear();
    assert_eq!(run(&host), Outcome::Left);
    assert!(host.removals().is_empty());
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
fn something_still_busy_after_ten_seconds_is_code_one() {
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
fn the_ten_seconds_are_for_everything_not_per_file() {
    // Quatre fichiers et une arborescence occupés pour toujours : le temps total
    // d'attente reste dix secondes, il ne se multiplie pas par le nombre d'occupés.
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
    // 40 pauses de 250 ms, donc 41 passes.
    assert_eq!(host.sleeps().len(), 40);
    assert_eq!(host.removals().len(), 13 * 41);
}

#[test]
fn time_spent_working_counts_against_the_ten_seconds() {
    // Chaque suppression coûte 100 ms : une passe de 13 appels en coûte 1,3 s.
    // Le budget s'épuise donc en moins de pauses, et jamais au-delà de 5 s + une passe.
    let mut host = full_pc();
    host.cost = Duration::from_millis(100);
    host.script_file(&roaming("settings.json"), &[Removed::Busy; 1000]);
    assert_eq!(run(&host), Outcome::Left);
    let passes = host.removals().len() / 13;
    assert!(passes <= 8, "{passes} passes : le temps de travail doit compter");
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

// --- L'échéance totale ---

#[test]
fn the_deadline_is_twenty_seconds_from_the_start_and_given_to_every_tree() {
    let host = full_pc();
    assert_eq!(run(&host), Outcome::Done);
    assert_eq!(*host.deadlines.borrow(), [TOTAL_DEADLINE, TOTAL_DEADLINE]);
}

#[test]
fn a_walk_longer_than_the_deadline_stops_inside_the_tree_with_code_one() {
    // Un million d'entrées à une milliseconde : mille secondes. L'arborescence
    // s'arrête d'elle-même à l'échéance, au milieu.
    let mut host = full_pc();
    host.tree_walk = Some((1_000_000, Duration::from_millis(1)));
    assert_eq!(run(&host), Outcome::Left);
    assert!(host.now() >= TOTAL_DEADLINE);
    assert!(host.now() <= TOTAL_DEADLINE + Duration::from_millis(1), "{:?}", host.now());
    // La seconde arborescence n'est même pas commencée.
    let trees = host.removals().iter().filter(|line| line.starts_with("tree ")).count();
    assert_eq!(trees, 1);
}

#[test]
fn a_slow_host_never_goes_past_the_deadline_by_more_than_one_gesture() {
    // Chaque geste coûte trois secondes ; les fichiers sont occupés pour toujours.
    let mut host = full_pc();
    host.cost = Duration::from_secs(3);
    host.file_default = Removed::Busy;
    assert_eq!(run(&host), Outcome::Left);
    assert!(host.now() <= TOTAL_DEADLINE + Duration::from_secs(3), "{:?}", host.now());
    // Rien n'est commencé une fois l'échéance passée.
    let calls = host.removals().len();
    assert!(calls <= 8, "{calls} gestes");
}

#[test]
fn the_retry_never_waits_past_the_deadline() {
    let host = full_pc();
    host.script_file(&roaming("settings.json"), &[Removed::Busy; 1000]);
    assert_eq!(run(&host), Outcome::Left);
    assert!(host.now() <= TOTAL_DEADLINE);
}
