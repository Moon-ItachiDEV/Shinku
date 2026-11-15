//! Les apps d'arrière-plan qu'on peut mettre en veilleuse, et celles qu'on ne
//! touche jamais.
//!
//! « Mettre en veilleuse » veut dire : priorité « sous la normale » et mode
//! efficacité de Windows. Le processus reste ouvert, il passe seulement après
//! le jeu. On ne fait que baisser : un processus déjà en priorité basse garde
//! sa classe.
//!
//! La liste de départ est volontairement courte : les apps de synchronisation
//! sont cochées d'office ; les navigateurs sont proposés, mais décochés (la
//! personne y regarde peut-être une vidéo ou y passe un appel).

use crate::game::detect::{SHINKU_PROGRAMS, WINDOWS_PROGRAMS};
use crate::settings::Settings;

/// Une app connue de Shinku.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltInApp {
    /// Nom de l'exécutable, en minuscules.
    pub exe: &'static str,
    /// Nom affiché.
    pub label: &'static str,
    /// Cochée d'office.
    pub default_on: bool,
}

/// Les apps proposées d'office, dans l'ordre où la page les montre.
pub const BUILT_IN: [BuiltInApp; 9] = [
    BuiltInApp { exe: "onedrive.exe", label: "OneDrive", default_on: true },
    BuiltInApp { exe: "dropbox.exe", label: "Dropbox", default_on: true },
    BuiltInApp { exe: "googledrivefs.exe", label: "Google Drive", default_on: true },
    BuiltInApp { exe: "chrome.exe", label: "Google Chrome", default_on: false },
    BuiltInApp { exe: "msedge.exe", label: "Microsoft Edge", default_on: false },
    BuiltInApp { exe: "firefox.exe", label: "Firefox", default_on: false },
    BuiltInApp { exe: "brave.exe", label: "Brave", default_on: false },
    BuiltInApp { exe: "opera.exe", label: "Opera", default_on: false },
    BuiltInApp { exe: "vivaldi.exe", label: "Vivaldi", default_on: false },
];

/// Processus de Windows qu'on ne ralentit jamais, en plus de ceux du veto.
const SYSTEM_PROGRAMS: [&str; 10] = [
    "csrss.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "svchost.exe",
    "audiodg.exe",
    "fontdrvhost.exe",
    "sihost.exe",
    "ctfmon.exe",
    "runtimebroker.exe",
];

/// L'app intégrée de ce nom d'exécutable, s'il y en a une.
pub fn built_in(exe: &str) -> Option<&'static BuiltInApp> {
    BUILT_IN.iter().find(|app| app.exe.eq_ignore_ascii_case(exe))
}

/// Vrai si cet exécutable n'est jamais mis en veilleuse, quelle que soit la
/// liste : les programmes de Windows et de Shinku, et les processus du système.
/// (Le processus du jeu en cours, lui, est écarté par la machine à états.)
pub fn protected(exe_name: &str) -> bool {
    let name = exe_name.to_lowercase();
    [WINDOWS_PROGRAMS.as_slice(), SHINKU_PROGRAMS.as_slice(), SYSTEM_PROGRAMS.as_slice()]
        .iter()
        .any(|group| group.contains(&name.as_str()))
}

/// Les noms d'exécutables mis en veilleuse avec ces réglages : les apps
/// cochées d'office, moins celles que la personne a décochées, plus celles
/// qu'elle a cochées. Sans ceux qui sont protégés. Noms en minuscules, sans
/// doublon.
pub fn effective(settings: &Settings) -> Vec<String> {
    let listed =
        |list: &[String], exe: &str| list.iter().any(|name| name.eq_ignore_ascii_case(exe));
    let mut names: Vec<String> = BUILT_IN
        .iter()
        .filter(|app| app.default_on && !listed(&settings.game_mode_quiet_removed, app.exe))
        .map(|app| app.exe.to_owned())
        .collect();
    for added in &settings.game_mode_quiet_added {
        let added = added.to_lowercase();
        if !names.contains(&added) {
            names.push(added);
        }
    }
    names.retain(|name| !protected(name));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_apps_are_on_and_browsers_are_off_by_default() {
        let on: Vec<&str> = BUILT_IN.iter().filter(|a| a.default_on).map(|a| a.exe).collect();
        assert_eq!(on, ["onedrive.exe", "dropbox.exe", "googledrivefs.exe"]);
        let off: Vec<&str> = BUILT_IN.iter().filter(|a| !a.default_on).map(|a| a.exe).collect();
        assert_eq!(
            off,
            ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe"]
        );
        assert_eq!(built_in("OneDrive.EXE").map(|a| a.label), Some("OneDrive"));
        assert_eq!(built_in("notepad.exe"), None);
    }

    #[test]
    fn the_effective_list_follows_the_two_lists_of_the_person() {
        let mut settings = Settings::default();
        assert_eq!(effective(&settings), ["onedrive.exe", "dropbox.exe", "googledrivefs.exe"]);

        settings.game_mode_quiet_removed = vec!["dropbox.exe".to_owned()];
        settings.game_mode_quiet_added = vec!["chrome.exe".to_owned(), "onedrive.exe".to_owned()];
        // Une app cochée d'office et ajoutée n'est comptée qu'une fois.
        assert_eq!(effective(&settings), ["onedrive.exe", "googledrivefs.exe", "chrome.exe"]);
    }

    #[test]
    fn protected_programs_are_never_in_the_effective_list() {
        for name in [
            "explorer.exe",
            "DWM.EXE",
            "shinku.exe",
            "shinku-agent.exe",
            "shinku-helper.exe",
            "csrss.exe",
            "winlogon.exe",
            "services.exe",
            "lsass.exe",
            "svchost.exe",
            "audiodg.exe",
            "fontdrvhost.exe",
            "sihost.exe",
            "ctfmon.exe",
            "runtimebroker.exe",
        ] {
            assert!(protected(name), "{name}");
        }
        // Les navigateurs et les lanceurs peuvent être mis en veilleuse.
        for name in ["chrome.exe", "steam.exe", "discord.exe", "onedrive.exe"] {
            assert!(!protected(name), "{name}");
        }
        let settings = Settings {
            game_mode_quiet_added: vec!["svchost.exe".to_owned(), "discord.exe".to_owned()],
            ..Settings::default()
        };
        let effective = effective(&settings);
        assert!(!effective.contains(&"svchost.exe".to_owned()));
        assert!(effective.contains(&"discord.exe".to_owned()));
    }
}
