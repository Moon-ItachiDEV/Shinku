//! Lire une ligne de commande de démarrage : de quel programme s'agit-il ?
//!
//! Ce sont des fonctions pures : la machine n'est interrogée que par
//! [`StartupHost`] (variables, fichiers, dossier de Windows). Rien n'est
//! lancé, rien n'est « réparé », et rien n'est cherché ailleurs que sur un
//! lecteur **fixe** : un chemin réseau, un lecteur réseau ou amovible n'est
//! jamais approché, car lire une entrée ne doit pas pouvoir déclencher une
//! connexion ni attendre un délai.
//!
//! Les règles, dans l'ordre :
//!
//! 1. les variables `%…%` sont développées, les blancs de tête retirés ;
//! 2. un chemin réseau (`\\…`) est inconnu, avant tout accès au disque ; un
//!    chemin dont le lecteur n'est pas fixe l'est aussi ;
//! 3. une commande qui commence par `"` désigne le programme jusqu'au
//!    guillemet fermant (non fermé : jusqu'à la fin) ;
//! 4. sinon, comme `CreateProcess` : on essaie les préfixes coupés aux
//!    espaces, du plus court au plus long, avec et sans `.exe` ajouté ; le
//!    premier qui est un fichier gagne, et si aucun n'existe le programme est
//!    le premier mot ;
//! 5. un programme sans dossier (`foo.exe`) est cherché dans `System32` puis
//!    dans le dossier de Windows ; introuvable, il est inconnu (on ne connaît
//!    pas le `PATH` : jamais « introuvable ») ;
//! 6. un chemin absolu n'est **manquant** que si la machine dit « absent » avec
//!    certitude (introuvable). Un dossier, un accès refusé, un lien ou toute
//!    autre incertitude donnent « inconnu » : jamais « semble désinstallée » à
//!    tort ;
//! 7. un programme **hôte** (`rundll32.exe`, `cmd.exe`, `java.exe`…) n'est que
//!    le porteur : on n'en tire ni nom ni éditeur.

use crate::startup::host::{Existence, StartupHost};

/// Les programmes qui en lancent d'autres : le programme réellement démarré
/// est dans les arguments, pas dans le fichier.
pub const HOSTS: [&str; 17] = [
    "rundll32.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "wscript.exe",
    "cscript.exe",
    "mshta.exe",
    "msiexec.exe",
    "explorer.exe",
    "conhost.exe",
    "regsvr32.exe",
    "schtasks.exe",
    "java.exe",
    "javaw.exe",
    "python.exe",
    "pythonw.exe",
    "node.exe",
];

/// Ce qu'une ligne de commande désigne.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Un fichier qui existe.
    Found(String),
    /// Un chemin absolu que la machine dit absent, avec certitude.
    Missing(String),
    /// Un programme hôte (le nom de son fichier, avec la casse du chemin).
    Hosted(String),
    /// Impossible à dire : chemin réseau ou lecteur qui n'est pas fixe, dossier,
    /// accès refusé, nom nu introuvable, ligne vide.
    Unknown,
}

/// De quel programme s'agit-il ?
pub fn target_of(command: &str, host: &dyn StartupHost) -> Target {
    let expanded = host.expand(command);
    let text = expanded.trim_start();
    if text.is_empty() || is_network(text.trim_start_matches('"')) {
        return Target::Unknown;
    }

    let program = if let Some(quoted) = text.strip_prefix('"') {
        let end = quoted.find('"').unwrap_or(quoted.len());
        let program = quoted[..end].trim();
        if program.is_empty() || is_network(program) {
            return Target::Unknown;
        }
        // Entre guillemets, le programme est exactement ce texte : on essaie
        // seulement `.exe` en plus, comme `CreateProcess`.
        match first_file(host, [program]) {
            Search::Found(path) => path,
            Search::Nothing { .. } => program.to_owned(),
            Search::Unsure => return Target::Unknown,
        }
    } else {
        let (candidates, cut) = prefixes(text);
        match first_file(host, candidates) {
            Search::Found(path) => path,
            // Des préfixes n'ont pas été essayés : `CreateProcess` aurait pu
            // trouver le fichier plus loin. Ni « manquant » ni « trouvé ».
            Search::Nothing { .. } if cut => return Target::Unknown,
            // La commande entière est un dossier (un raccourci vers un
            // dossier, par exemple) : ce n'est pas « désinstallé ».
            Search::Nothing { last_is_folder: true } => return Target::Unknown,
            Search::Nothing { .. } => text.split_whitespace().next().unwrap_or_default().to_owned(),
            Search::Unsure => return Target::Unknown,
        }
    };

    resolve(&program, host)
}

/// Vrai pour un chemin réseau (UNC), y compris sous les formes `\\?\` et `//`.
fn is_network(path: &str) -> bool {
    path.starts_with("\\\\") || path.starts_with("//")
}

/// Vrai pour `C:\…` ou `C:/…`.
fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// Nombre maximal de préfixes essayés : une commande de 2 048 caractères
/// pleine d'espaces ne doit pas coûter des milliers d'accès au disque.
const MAX_PREFIXES: usize = 64;

/// Les préfixes de la commande coupés aux blancs, du plus court au plus long
/// (les 64 premiers), puis la commande entière. Le booléen dit si des préfixes
/// ont été laissés de côté.
fn prefixes(text: &str) -> (Vec<&str>, bool) {
    let mut found: Vec<&str> = Vec::new();
    let mut cut = false;
    for (at, character) in text.char_indices() {
        if character.is_whitespace() && at > 0 {
            if found.len() >= MAX_PREFIXES {
                cut = true;
                break;
            }
            let prefix = text[..at].trim_end();
            if !prefix.is_empty() && found.last() != Some(&prefix) {
                found.push(prefix);
            }
        }
    }
    let whole = text.trim_end();
    if found.last() != Some(&whole) {
        found.push(whole);
    }
    (found, cut)
}

/// Le résultat d'une recherche de fichier.
enum Search {
    /// Le premier chemin qui est un fichier.
    Found(String),
    /// Aucun n'est un fichier, et la machine l'a dit avec certitude (un dossier
    /// ne compte pas : `CreateProcess` passe son chemin). `last_is_folder` :
    /// le dernier chemin essayé (la commande entière, pour des préfixes) est
    /// un dossier.
    Nothing { last_is_folder: bool },
    /// On ne peut pas conclure : lecteur qui n'est pas fixe, accès refusé…
    Unsure,
}

/// Le premier de ces chemins absolus qui est un fichier, tel quel puis avec
/// `.exe` ajouté. Un chemin qui n'est pas absolu n'est jamais essayé : il
/// dépendrait du dossier courant. Un chemin hors d'un lecteur fixe n'est pas
/// non plus essayé : la recherche s'arrête, incertaine, **avant** tout accès.
fn first_file<'a>(host: &dyn StartupHost, candidates: impl IntoIterator<Item = &'a str>) -> Search {
    let mut last_is_folder = false;
    for candidate in candidates {
        if !is_absolute(candidate) {
            continue;
        }
        if !host.is_local_fixed(candidate) {
            return Search::Unsure;
        }
        last_is_folder = false;
        for path in [candidate.to_owned(), format!("{candidate}.exe")] {
            match host.existence(&path) {
                Existence::File => return Search::Found(path),
                Existence::Unsure => return Search::Unsure,
                Existence::Folder => last_is_folder = path == candidate,
                Existence::Absent => {}
            }
        }
    }
    Search::Nothing { last_is_folder }
}

/// Le nom du fichier d'un chemin, avec sa casse.
fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Un programme sans dossier ni lecteur.
fn is_bare(program: &str) -> bool {
    !program.contains(['\\', '/', ':'])
}

fn is_host(name: &str) -> bool {
    let name = name.to_lowercase();
    HOSTS.contains(&name.as_str())
}

/// Du programme désigné à ce qu'il est : fichier, manquant, hôte, inconnu.
fn resolve(program: &str, host: &dyn StartupHost) -> Target {
    if is_bare(program) {
        let windows = host.windows_dir();
        let windows = windows.trim_end_matches(['\\', '/']);
        let places = [format!(r"{windows}\System32"), windows.to_owned()];
        for folder in &places {
            match first_file(host, [format!(r"{folder}\{program}").as_str()]) {
                Search::Found(path) => return hosted_or(path, Target::Found),
                Search::Unsure => return Target::Unknown,
                Search::Nothing { .. } => {}
            }
        }
        return Target::Unknown;
    }
    if !is_absolute(program) {
        // Un chemin relatif ou une forme que Shinku ne sait pas lire.
        return Target::Unknown;
    }
    if !host.is_local_fixed(program) {
        return Target::Unknown;
    }
    match host.existence(program) {
        Existence::File => hosted_or(program.to_owned(), Target::Found),
        Existence::Absent => Target::Missing(program.to_owned()),
        // Un dossier, un accès refusé, un lien : on ne dit pas « désinstallée ».
        Existence::Folder | Existence::Unsure => Target::Unknown,
    }
}

/// Un fichier qui existe : un hôte s'il en est un, sinon le fichier.
fn hosted_or(path: String, found: fn(String) -> Target) -> Target {
    if is_host(file_name(&path)) {
        Target::Hosted(file_name(&path).to_owned())
    } else {
        found(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::host::FakeStartupHost;

    fn host() -> FakeStartupHost {
        let host = FakeStartupHost::new();
        host.add_file(r"C:\Program Files\App\app.exe");
        host.add_file(r"C:\Program Files\Mon App\mon app.exe");
        host.add_file(r"C:\Apps\simple.exe");
        host.add_file(r"C:\Apps\sans extension.exe");
        host.add_file(r"C:\Windows\System32\ctfmon.exe");
        host.add_file(r"C:\Windows\System32\rundll32.exe");
        host.add_file(r"C:\Windows\regedit.exe");
        host.add_file(r"C:\Java\bin\javaw.exe");
        host.set_variable("ProgramFiles", r"C:\Program Files");
        host.set_variable("SystemRoot", r"C:\Windows");
        host
    }

    fn found(path: &str) -> Target {
        Target::Found(path.to_owned())
    }

    #[test]
    fn a_quoted_path_is_the_program() {
        let host = host();
        assert_eq!(
            target_of(r#""C:\Program Files\App\app.exe""#, &host),
            found(r"C:\Program Files\App\app.exe")
        );
        // Les arguments après le guillemet fermant ne comptent pas.
        assert_eq!(
            target_of(r#""C:\Program Files\App\app.exe" --background /min"#, &host),
            found(r"C:\Program Files\App\app.exe")
        );
        // Un guillemet dans les arguments non plus.
        assert_eq!(
            target_of(r#""C:\Apps\simple.exe" -x "a b""#, &host),
            found(r"C:\Apps\simple.exe")
        );
    }

    #[test]
    fn an_unquoted_path_with_spaces_is_found_like_createprocess_does() {
        let host = host();
        assert_eq!(
            target_of(r"C:\Program Files\App\app.exe /min", &host),
            found(r"C:\Program Files\App\app.exe")
        );
        assert_eq!(
            target_of(r"C:\Program Files\App\app.exe", &host),
            found(r"C:\Program Files\App\app.exe")
        );
        assert_eq!(
            target_of(r"C:\Program Files\Mon App\mon app.exe -silent -x", &host),
            found(r"C:\Program Files\Mon App\mon app.exe")
        );
    }

    #[test]
    fn the_shortest_existing_prefix_wins() {
        let host = host();
        host.add_file(r"C:\Program.exe");
        // CreateProcess essaie `C:\Program` + `.exe` d'abord : c'est ce que Windows lancerait.
        assert_eq!(target_of(r"C:\Program Files\App\app.exe", &host), found(r"C:\Program.exe"));
    }

    #[test]
    fn an_exe_extension_is_tried_when_it_is_missing() {
        let host = host();
        assert_eq!(target_of(r"C:\Apps\simple /silent", &host), found(r"C:\Apps\simple.exe"));
        assert_eq!(target_of(r#""C:\Apps\simple""#, &host), found(r"C:\Apps\simple.exe"));
    }

    #[test]
    fn variables_are_expanded_before_anything_else() {
        let host = host();
        assert_eq!(
            target_of(r#""%ProgramFiles%\App\app.exe" --x"#, &host),
            found(r"C:\Program Files\App\app.exe")
        );
        assert_eq!(
            target_of(r"%ProgramFiles%\App\app.exe /min", &host),
            found(r"C:\Program Files\App\app.exe")
        );
        // Une variable inconnue reste telle quelle : le chemin n'existe pas.
        assert_eq!(target_of(r#""%Inconnue%\app.exe""#, &host), Target::Unknown);
    }

    #[test]
    fn a_missing_absolute_path_is_missing() {
        let host = host();
        assert_eq!(
            target_of(r#""C:\Program Files\Parti\parti.exe" --x"#, &host),
            Target::Missing(r"C:\Program Files\Parti\parti.exe".to_owned())
        );
        // Sans guillemets : le premier mot (aucun préfixe n'est un fichier).
        assert_eq!(
            target_of(r"D:\Outils\vieux.exe /min", &host),
            Target::Missing(r"D:\Outils\vieux.exe".to_owned())
        );
    }

    #[test]
    fn a_bare_name_is_searched_in_the_windows_folders_only() {
        let host = host();
        assert_eq!(target_of("ctfmon.exe", &host), found(r"C:\Windows\System32\ctfmon.exe"));
        assert_eq!(target_of("regedit.exe /s", &host), found(r"C:\Windows\regedit.exe"));
        assert_eq!(target_of("ctfmon", &host), found(r"C:\Windows\System32\ctfmon.exe"));
        // Introuvable : on ne connaît pas le PATH, donc jamais « manquant ».
        assert_eq!(target_of("inconnu.exe", &host), Target::Unknown);
        assert_eq!(target_of(r#""inconnu.exe" -x"#, &host), Target::Unknown);
    }

    #[test]
    fn the_windows_folder_comes_from_the_host_not_from_a_variable() {
        let host = host();
        host.set_variable("SystemRoot", r"C:\Faux");
        host.set_windows_dir(r"C:\Windows\");
        assert_eq!(target_of("ctfmon.exe", &host), found(r"C:\Windows\System32\ctfmon.exe"));
    }

    #[test]
    fn a_host_program_carries_another_program() {
        let host = host();
        assert_eq!(
            target_of(r"rundll32.exe shell32.dll,Control_RunDLL", &host),
            Target::Hosted("rundll32.exe".to_owned())
        );
        assert_eq!(
            target_of(r#""C:\Java\bin\javaw.exe" -jar "C:\x\app.jar""#, &host),
            Target::Hosted("javaw.exe".to_owned())
        );
        // La casse du fichier est gardée, la comparaison ne la regarde pas.
        host.add_file(r"C:\Windows\System32\CMD.EXE");
        assert_eq!(
            target_of(r"C:\Windows\System32\CMD.EXE /c start x", &host),
            Target::Hosted("CMD.EXE".to_owned())
        );
    }

    #[test]
    fn every_listed_host_is_a_host() {
        let host = host();
        for name in HOSTS {
            host.add_file(&format!(r"C:\Windows\System32\{name}"));
            assert_eq!(
                target_of(&format!(r#""C:\Windows\System32\{name}" x"#), &host),
                Target::Hosted(name.to_owned()),
                "{name}"
            );
        }
        assert_eq!(HOSTS.len(), 17);
        // Un programme qui n'est pas un hôte n'en est pas un.
        host.add_file(r"C:\Windows\System32\cmdx.exe");
        assert_eq!(
            target_of(r"C:\Windows\System32\cmdx.exe", &host),
            found(r"C:\Windows\System32\cmdx.exe")
        );
    }

    #[test]
    fn a_network_path_is_unknown_and_never_touches_the_disk() {
        let host = host();
        for command in [
            r"\\serveur\partage\app.exe",
            r#""\\serveur\partage\app.exe" /min"#,
            r"  \\serveur\partage\mon app.exe /min",
            r#"  "\\?\UNC\serveur\partage\app.exe""#,
            "//serveur/partage/app.exe",
            r#""//serveur/partage/app.exe""#,
            r"\\?\C:\Apps\simple.exe",
        ] {
            assert_eq!(target_of(command, &host), Target::Unknown, "{command}");
        }
        assert_eq!(host.checked_paths(), Vec::<String>::new(), "aucun accès au disque");
        // Même un chemin réseau né d'une variable.
        host.set_variable("Serveur", r"\\serveur\partage");
        assert_eq!(target_of(r"%Serveur%\app.exe", &host), Target::Unknown);
        assert_eq!(host.checked_paths(), Vec::<String>::new());
    }

    #[test]
    fn an_empty_command_or_only_blanks_is_unknown() {
        let host = host();
        for command in ["", " ", "   \t  ", r#""""#, r#"  "   ""#, "\""] {
            assert_eq!(target_of(command, &host), Target::Unknown, "{command:?}");
        }
        assert_eq!(host.checked_paths(), Vec::<String>::new());
    }

    #[test]
    fn leading_blanks_are_ignored() {
        let host = host();
        assert_eq!(target_of(r#"   "C:\Apps\simple.exe" -x"#, &host), found(r"C:\Apps\simple.exe"));
        assert_eq!(target_of("\t C:\\Apps\\simple.exe -x", &host), found(r"C:\Apps\simple.exe"));
    }

    #[test]
    fn an_unclosed_quote_runs_to_the_end() {
        let host = host();
        assert_eq!(
            target_of(r#""C:\Program Files\App\app.exe"#, &host),
            found(r"C:\Program Files\App\app.exe")
        );
        assert_eq!(
            target_of(r#""C:\Program Files\Parti\parti.exe --x"#, &host),
            Target::Missing(r"C:\Program Files\Parti\parti.exe --x".to_owned())
        );
    }

    #[test]
    fn a_relative_path_is_never_looked_up() {
        let host = host();
        assert_eq!(target_of(r"outils\app.exe", &host), Target::Unknown);
        assert_eq!(target_of(r#""..\app.exe""#, &host), Target::Unknown);
        assert_eq!(target_of(r"C:app.exe", &host), Target::Unknown);
        assert_eq!(host.checked_paths(), Vec::<String>::new());
    }

    #[test]
    fn a_name_with_a_space_in_a_quoted_path_is_not_cut() {
        let host = host();
        assert_eq!(
            target_of(r#""C:\Program Files\Mon App\mon app.exe""#, &host),
            found(r"C:\Program Files\Mon App\mon app.exe")
        );
        assert_eq!(
            target_of(r#""C:\Apps\sans extension" /x"#, &host),
            found(r"C:\Apps\sans extension.exe")
        );
    }

    #[test]
    fn a_drive_that_is_not_fixed_is_never_approached() {
        let host = host();
        host.add_remote_drive('Z');
        host.add_file(r"Z:\app.exe");
        for command in [
            r"Z:\app.exe /x",
            r#""Z:\app.exe" /x"#,
            r#""z:\dossier avec espaces\app.exe""#,
            r"Z:\Mon dossier\app.exe -x",
            r"Z:/app.exe",
        ] {
            assert_eq!(target_of(command, &host), Target::Unknown, "{command}");
        }
        assert_eq!(host.checked_paths(), Vec::<String>::new(), "aucun accès, aucun essai");
        // Un lecteur fixe voisin reste lu.
        assert_eq!(target_of(r#""C:\Apps\simple.exe""#, &host), found(r"C:\Apps\simple.exe"));
    }

    #[test]
    fn a_missing_file_is_only_said_missing_when_the_machine_is_sure() {
        let host = host();
        host.deny_access(r"C:\Prive\app.exe");
        // Accès refusé : on ne sait pas, donc jamais « désinstallée ».
        assert_eq!(target_of(r#""C:\Prive\app.exe" -x"#, &host), Target::Unknown);
        assert_eq!(target_of(r"C:\Prive\app.exe", &host), Target::Unknown);
        // Un dossier n'est pas un programme, mais n'est pas « absent » non plus.
        host.add_folder(r"C:\Program Files\Dossier");
        assert_eq!(target_of(r#""C:\Program Files\Dossier""#, &host), Target::Unknown);
        assert_eq!(target_of(r"C:\Program Files\Dossier", &host), Target::Unknown);
        // Un « introuvable » sûr reste « manquant ».
        assert_eq!(
            target_of(r#""C:\Program Files\Parti\parti.exe""#, &host),
            Target::Missing(r"C:\Program Files\Parti\parti.exe".to_owned())
        );
    }

    #[test]
    fn an_uncertain_prefix_makes_the_whole_command_unknown() {
        let host = host();
        host.deny_access(r"C:\Program");
        assert_eq!(target_of(r"C:\Program Files\App\app.exe /x", &host), Target::Unknown);
    }

    #[test]
    fn resolving_a_path_on_a_drive_that_is_not_fixed_touches_nothing() {
        // Défense en profondeur : même appelée directement, la résolution ne
        // demande rien à un lecteur qui n'est pas fixe.
        let host = host();
        host.add_remote_drive('Z');
        host.add_file(r"Z:\app.exe");
        assert_eq!(resolve(r"Z:\app.exe", &host), Target::Unknown);
        assert_eq!(host.checked_paths(), Vec::<String>::new());
    }

    #[test]
    fn a_folder_prefix_does_not_stop_the_search() {
        let host = host();
        // `C:\Program Files` est un dossier : `CreateProcess` passe au suivant.
        host.add_folder(r"C:\Program Files");
        host.add_file(r"C:\Program Files (x86)\App\app.exe");
        assert_eq!(
            target_of(r"C:\Program Files (x86)\App\app.exe /x", &host),
            found(r"C:\Program Files (x86)\App\app.exe")
        );
    }

    #[test]
    fn a_command_with_more_prefixes_than_the_bound_is_unknown_not_missing() {
        let host = host();
        // 70 espaces dans un chemin non cité : seuls 64 préfixes sont essayés.
        let long = format!(r"C:\{}.exe", vec!["a"; 71].join(" "));
        assert_eq!(target_of(&long, &host), Target::Unknown);
        // Sous la borne, un fichier absent reste « manquant ».
        let short = format!(r"C:\{}.exe", ["a"; 10].join(" "));
        assert!(matches!(target_of(&short, &host), Target::Missing(_)));
        // Et un fichier trouvé dans les 64 premiers préfixes reste trouvé.
        host.add_file(r"C:\a a.exe");
        assert_eq!(target_of(&long, &host), found(r"C:\a a.exe"));
    }
}
