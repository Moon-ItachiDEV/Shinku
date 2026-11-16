//! Reconnaître un jeu.
//!
//! Un jeu est reconnu par le chemin de son exécutable, jamais par un pid.
//! Trois sources, dans cet ordre : les jeux que la personne a ajoutés, ceux
//! qu'elle a écartés (« Ce n'est pas un jeu »), puis les bibliothèques de jeux
//! que Windows et les lanceurs connaissent (Steam, Epic, Xbox, GOG, Riot,
//! Ubisoft, EA…). Un veto global écarte les navigateurs, les lanceurs, les
//! lecteurs vidéo et les outils : ils ne sont jamais des jeux.
//!
//! La détection par plein écran et par la liste de jeux de Windows est
//! écartée : trop de faux positifs, et une clé de registre non documentée.
//!
//! Les chemins sont découpés ici, sur `\` et sur `/` : les tests passent des
//! chemins de style Windows, que `Path::components` ne saurait pas lire sous
//! Linux. Toutes les comparaisons ignorent la casse.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::settings::Settings;

/// D'où vient la reconnaissance d'un jeu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// La personne l'a ajouté.
    Added,
    /// Il vit dans une bibliothèque de jeux.
    Library,
}

/// Les listes de la personne : jeux ajoutés et « Ce n'est pas un jeu ».
#[derive(Clone, Copy, Debug)]
pub struct Lists<'a> {
    /// Jeux ajoutés.
    pub games: &'a [PathBuf],
    /// Exécutables que Shinku ne traite jamais comme des jeux.
    pub not_games: &'a [PathBuf],
}

impl<'a> Lists<'a> {
    /// Les listes des réglages.
    pub fn of(settings: &'a Settings) -> Self {
        Self { games: &settings.game_mode_games, not_games: &settings.game_mode_not_games }
    }
}

/// La forme sous laquelle deux chemins se comparent : en minuscules, avec des
/// `\`. Un chemin vide donne un texte vide.
pub fn key(path: &str) -> String {
    path.trim().replace('/', "\\").to_lowercase()
}

/// Les composants d'un chemin, en minuscules, séparés sur `\` et `/`.
fn components(path: &str) -> Vec<String> {
    path.split(['\\', '/']).filter(|part| !part.is_empty()).map(str::to_lowercase).collect()
}

/// Le nom du fichier d'un chemin (`C:\Jeux\hp.exe` donne `hp.exe`), en
/// minuscules.
pub fn file_name(path: &str) -> String {
    components(path).pop().unwrap_or_default()
}

/// Navigateurs.
const BROWSERS: [&str; 6] =
    ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe"];
/// Programmes de Windows.
pub(crate) const WINDOWS_PROGRAMS: [&str; 10] = [
    "explorer.exe",
    "applicationframehost.exe",
    "searchhost.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "lockapp.exe",
    "dwm.exe",
    "taskmgr.exe",
    "systemsettings.exe",
    "mstsc.exe",
];
/// Lecteurs et bureautique.
const PLAYERS_AND_OFFICE: [&str; 10] = [
    "vlc.exe",
    "wmplayer.exe",
    "mpc-hc64.exe",
    "mpc-hc.exe",
    "mpc-be64.exe",
    "mpv.exe",
    "potplayermini64.exe",
    "powerpnt.exe",
    "winword.exe",
    "excel.exe",
];
/// Outils.
const TOOLS: [&str; 10] = [
    "code.exe",
    "devenv.exe",
    "windowsterminal.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "obs64.exe",
    "vmconnect.exe",
    "vmware.exe",
    "virtualboxvm.exe",
];
/// Communication et musique.
const CHAT_AND_MUSIC: [&str; 5] =
    ["discord.exe", "spotify.exe", "zoom.exe", "ms-teams.exe", "teams.exe"];
/// Lanceurs de jeux.
const LAUNCHERS: [&str; 11] = [
    "steam.exe",
    "steamwebhelper.exe",
    "epicgameslauncher.exe",
    "galaxyclient.exe",
    "riotclientux.exe",
    "riotclientservices.exe",
    "upc.exe",
    "ubisoftconnect.exe",
    "eadesktop.exe",
    "origin.exe",
    "battle.net.exe",
];
/// Les programmes de Shinku.
pub(crate) const SHINKU_PROGRAMS: [&str; 3] =
    ["shinku.exe", "shinku-agent.exe", "shinku-helper.exe"];
/// Le lanceur de Rockstar, que la liste ci-dessus écrit sous deux noms.
const ROCKSTAR_LAUNCHER: &str = "rockstargameslauncher.exe";

/// Dossiers de lanceurs : un exécutable qui y vit n'est jamais un jeu.
const LAUNCHER_FOLDERS: [&[&str]; 5] = [
    &["epic games", "launcher"],
    &["riot games", "riot client"],
    &["rockstar games", "launcher"],
    &["rockstar games", "social club"],
    &["steam", "bin"],
];

/// Les dossiers qui, où qu'ils soient, font d'un exécutable un jeu de
/// bibliothèque (suite de composants entiers).
const LIBRARY_MARKERS: [&[&str]; 10] = [
    &["steamapps", "common"],
    &["xboxgames"],
    &["epic games"],
    &["gog galaxy", "games"],
    &["gog games"],
    &["riot games"],
    &["ubisoft game launcher", "games"],
    &["ea games"],
    &["origin games"],
    &["rockstar games"],
];

/// Vrai si ce nom de fichier est celui d'un lanceur de jeux (Steam, Epic,
/// Riot, Ubisoft, EA, GOG, Battle.net, Rockstar). Un lanceur est le parent des
/// jeux qu'il démarre : il n'est jamais mis en veilleuse.
pub fn is_launcher(name: &str) -> bool {
    let name = name.to_lowercase();
    LAUNCHERS.contains(&name.as_str()) || name == ROCKSTAR_LAUNCHER
}

/// Vrai si ce nom de fichier (en minuscules) est dans la liste du veto.
fn vetoed_name(name: &str) -> bool {
    [
        BROWSERS.as_slice(),
        WINDOWS_PROGRAMS.as_slice(),
        PLAYERS_AND_OFFICE.as_slice(),
        TOOLS.as_slice(),
        CHAT_AND_MUSIC.as_slice(),
        LAUNCHERS.as_slice(),
        SHINKU_PROGRAMS.as_slice(),
        &[ROCKSTAR_LAUNCHER],
    ]
    .iter()
    .any(|group| group.contains(&name))
}

/// Position du premier endroit où `sequence` apparaît dans `parts`.
fn find(parts: &[String], sequence: &[&str]) -> Option<usize> {
    if sequence.is_empty() || parts.len() < sequence.len() {
        return None;
    }
    parts
        .windows(sequence.len())
        .position(|window| window.iter().zip(sequence).all(|(a, b)| a == b))
}

/// Veto global : vrai si ce programme n'est jamais un jeu, à cause de son nom
/// de fichier ou d'un dossier de lanceur sur son chemin.
pub fn never_a_game(exe: &str) -> bool {
    let parts = components(exe);
    let Some((name, folders)) = parts.split_last() else { return false };
    vetoed_name(name) || LAUNCHER_FOLDERS.iter().any(|folder| find(folders, folder).is_some())
}

/// Où commence le nom du jeu dans un chemin de bibliothèque : le composant qui
/// suit le marqueur, s'il y en a un et qu'il n'est pas le fichier lui-même.
/// `None` si le chemin ne traverse aucune bibliothèque.
fn library_position(parts: &[String]) -> Option<Option<usize>> {
    let folders = parts.split_last().map_or(&[][..], |(_, folders)| folders);
    let mut best: Option<usize> = None;
    for marker in LIBRARY_MARKERS {
        if let Some(at) = find(folders, marker) {
            best = Some(best.map_or(at + marker.len(), |known| known.min(at + marker.len())));
        }
    }
    for (index, part) in folders.iter().enumerate() {
        if part == "games" || part == "jeux" {
            best = Some(best.map_or(index + 1, |known| known.min(index + 1)));
        }
    }
    best.map(|next| (next < folders.len()).then_some(next))
}

/// Reconnaît un jeu. `exe` est le chemin complet de l'exécutable (`C:\…`).
///
/// 1. un chemin ajouté par la personne : [`Source::Added`] (même sous veto :
///    c'est son choix écrit) ;
/// 2. un chemin écarté par la personne : pas un jeu ;
/// 3. le veto global : pas un jeu ;
/// 4. un chemin qui traverse une bibliothèque de jeux : [`Source::Library`] ;
/// 5. sinon, pas un jeu.
pub fn recognise(exe: &str, lists: &Lists<'_>) -> Option<Source> {
    let wanted = key(exe);
    if wanted.is_empty() {
        return None;
    }
    let listed = |list: &[PathBuf]| list.iter().any(|path| key(&path.to_string_lossy()) == wanted);
    if listed(lists.games) {
        return Some(Source::Added);
    }
    if listed(lists.not_games) || never_a_game(exe) {
        return None;
    }
    library_position(&components(exe)).map(|_| Source::Library)
}

/// Le nom du dossier d'un jeu dans sa bibliothèque : pour
/// `…\steamapps\common\Hollow Peaks\bin\hp.exe`, « Hollow Peaks ». Le nom est
/// rendu avec la casse du dossier. `None` si le chemin ne traverse aucune
/// bibliothèque, ou si le marqueur est suivi du fichier lui-même.
pub fn library_title(exe: &str) -> Option<String> {
    let parts = components(exe);
    let next = library_position(&parts)??;
    // Le nom garde la casse du chemin d'origine.
    let original: Vec<&str> = exe.split(['\\', '/']).filter(|part| !part.is_empty()).collect();
    original.get(next).map(|title| (*title).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with<'a>(games: &'a [PathBuf], not_games: &'a [PathBuf]) -> Lists<'a> {
        Lists { games, not_games }
    }

    fn library(exe: &str) -> bool {
        recognise(exe, &with(&[], &[])) == Some(Source::Library)
    }

    #[test]
    fn libraries_of_known_stores_are_recognised() {
        for exe in [
            r"D:\SteamLibrary\steamapps\common\Hollow Peaks\bin\hp.exe",
            r"C:\Program Files (x86)\Steam\steamapps\common\Portal 2\portal2.exe",
            r"C:\XboxGames\Forza Horizon 5\Content\ForzaHorizon5.exe",
            r"C:\Program Files\Epic Games\Fortnite\FortniteGame\Binaries\Win64\fortnite.exe",
            r"C:\GOG Games\Witcher 3\bin\witcher3.exe",
            r"C:\Program Files (x86)\GOG Galaxy\Games\Cyberpunk\Cyberpunk2077.exe",
            r"C:\Riot Games\VALORANT\live\VALORANT.exe",
            r"C:\Program Files (x86)\Ubisoft\Ubisoft Game Launcher\games\Far Cry 6\FarCry6.exe",
            r"C:\Program Files\EA Games\Battlefield\bf.exe",
            r"C:\Program Files (x86)\Origin Games\Titanfall\titanfall.exe",
            r"C:\Program Files\Rockstar Games\Red Dead Redemption 2\RDR2.exe",
            r"E:\Games\Hades\Hades.exe",
            r"E:\Jeux\Hades\Hades.exe",
            "d:/games/hades/hades.exe",
        ] {
            assert!(library(exe), "{exe}");
        }
    }

    #[test]
    fn other_places_are_not_libraries() {
        for exe in [
            r"C:\Users\a\Saved Games\notes.exe",
            r"C:\Program Files\Mozilla\app.exe",
            r"C:\Windows\System32\notepad.exe",
            r"C:\Users\a\Downloads\setup.exe",
            // Un marqueur qui n'est pas un composant entier ne compte pas.
            r"C:\Boardgames\Chess\chess.exe",
            r"C:\steamapps\hp.exe",
            // Le fichier lui-même ne fait pas un dossier de bibliothèque.
            r"C:\Tools\Games",
            "",
        ] {
            assert!(!library(exe), "{exe}");
        }
    }

    #[test]
    fn the_veto_wins_over_a_library() {
        for exe in [
            r"C:\Program Files\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe",
            r"C:\Program Files\Epic Games\Launcher\Engine\Binaries\Win64\helper.exe",
            r"C:\Riot Games\Riot Client\RiotClientServices.exe",
            r"C:\Program Files\Rockstar Games\Launcher\Launcher.exe",
            r"C:\Program Files\Rockstar Games\Social Club\socialclub.exe",
            r"C:\Program Files (x86)\Steam\steamapps\common\Some Game\steam.exe",
            r"C:\Program Files (x86)\Steam\bin\cef\cef.win7x64\steamwebhelper.exe",
            r"E:\Games\Tools\chrome.exe",
            r"E:\Games\RockstarGamesLauncher.exe",
        ] {
            assert!(never_a_game(exe), "{exe}");
            assert!(!library(exe), "{exe}");
        }
        assert!(!never_a_game(r"D:\SteamLibrary\steamapps\common\Hollow Peaks\hp.exe"));
        assert!(!never_a_game(r"C:\Steam Games\steam-demo\game.exe"));
        assert!(!never_a_game(""));
    }

    #[test]
    fn every_name_of_the_veto_is_vetoed_in_any_case() {
        for name in [
            "chrome.exe",
            "MSEDGE.EXE",
            "firefox.exe",
            "brave.exe",
            "opera.exe",
            "vivaldi.exe",
            "explorer.exe",
            "ApplicationFrameHost.exe",
            "SearchHost.exe",
            "StartMenuExperienceHost.exe",
            "ShellExperienceHost.exe",
            "LockApp.exe",
            "dwm.exe",
            "Taskmgr.exe",
            "SystemSettings.exe",
            "mstsc.exe",
            "vlc.exe",
            "wmplayer.exe",
            "mpc-hc64.exe",
            "mpc-hc.exe",
            "mpc-be64.exe",
            "mpv.exe",
            "PotPlayerMini64.exe",
            "POWERPNT.EXE",
            "WINWORD.EXE",
            "EXCEL.EXE",
            "Code.exe",
            "devenv.exe",
            "WindowsTerminal.exe",
            "cmd.exe",
            "powershell.exe",
            "pwsh.exe",
            "obs64.exe",
            "vmconnect.exe",
            "vmware.exe",
            "VirtualBoxVM.exe",
            "Discord.exe",
            "Spotify.exe",
            "Zoom.exe",
            "ms-teams.exe",
            "Teams.exe",
            "steam.exe",
            "steamwebhelper.exe",
            "EpicGamesLauncher.exe",
            "GalaxyClient.exe",
            "RiotClientUx.exe",
            "RiotClientServices.exe",
            "upc.exe",
            "UbisoftConnect.exe",
            "EADesktop.exe",
            "Origin.exe",
            "Battle.net.exe",
            "RockstarGamesLauncher.exe",
            "Shinku.exe",
            "shinku-agent.exe",
            "shinku-helper.exe",
        ] {
            assert!(never_a_game(&format!(r"D:\Games\X\{name}")), "{name}");
        }
    }

    #[test]
    fn the_lists_of_the_person_come_first() {
        let added = [PathBuf::from(r"C:\Program Files\Java\bin\JAVAW.exe")];
        let refused = [PathBuf::from(r"D:\Games\Hades\Hades.exe")];
        let lists = with(&added, &refused);
        // Ajouté : reconnu, quel que soit le dossier, sans tenir compte de la casse.
        assert_eq!(recognise(r"c:\program files\java\bin\javaw.exe", &lists), Some(Source::Added));
        assert_eq!(recognise("C:/Program Files/Java/bin/javaw.exe", &lists), Some(Source::Added));
        // Écarté : même dans une bibliothèque, ce n'est pas un jeu.
        assert_eq!(recognise(r"D:\GAMES\hades\HADES.EXE", &lists), None);
        // Les autres jeux de la bibliothèque restent reconnus.
        assert_eq!(recognise(r"D:\Games\Hollow\hp.exe", &lists), Some(Source::Library));
        // Le choix écrit de la personne l'emporte sur le veto.
        let own = [PathBuf::from(r"C:\Tools\chrome.exe")];
        assert_eq!(recognise(r"C:\Tools\chrome.exe", &with(&own, &[])), Some(Source::Added));
        // Ajouté et écarté à la fois : l'ajout passe en premier.
        let both = [PathBuf::from(r"C:\x\g.exe")];
        assert_eq!(recognise(r"C:\x\g.exe", &with(&both, &both)), Some(Source::Added));
        // Un chemin vide n'est rien.
        assert_eq!(recognise("", &lists), None);
    }

    #[test]
    fn the_title_is_the_folder_after_the_marker() {
        assert_eq!(
            library_title(r"D:\SteamLibrary\steamapps\common\Hollow Peaks\bin\hp.exe").as_deref(),
            Some("Hollow Peaks")
        );
        assert_eq!(
            library_title(r"C:\XboxGames\Forza Horizon 5\Content\f.exe").as_deref(),
            Some("Forza Horizon 5")
        );
        assert_eq!(library_title(r"E:\Jeux\Hades\Hades.exe").as_deref(), Some("Hades"));
        assert_eq!(library_title("e:/games/Hades Two/h2.exe").as_deref(), Some("Hades Two"));
        // Le composant qui suit est le fichier lui-même : pas de titre.
        assert_eq!(library_title(r"E:\Games\hades.exe"), None);
        assert_eq!(library_title(r"C:\Windows\notepad.exe"), None);
        assert_eq!(library_title(""), None);
    }

    #[test]
    fn paths_compare_without_case_or_slash_style() {
        assert_eq!(key("C:/Jeux/Hades.EXE"), r"c:\jeux\hades.exe");
        assert_eq!(key("  "), "");
        assert_eq!(file_name(r"C:\Jeux\Hades.EXE"), "hades.exe");
        assert_eq!(file_name("hades.exe"), "hades.exe");
        assert_eq!(file_name(""), "");
    }
}
