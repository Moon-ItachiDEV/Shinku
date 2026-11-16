//! Ce que l'assistant élevé (`shinku-helper.exe`) accepte qu'on lui demande.
//!
//! **Contre qui l'assistant protège.** Un programme qui tourne sous le compte,
//! sans droits, contrôle entièrement `shinku.exe` : même utilisateur, même
//! niveau d'intégrité. Rien de ce que `shinku.exe` vérifie n'est donc une
//! barrière contre lui : ce sont des garde-fous contre nos propres erreurs. La
//! seule barrière est l'accord de Windows (voir `docs/conception.md`, section
//! « Contre qui l'assistant protège »).
//!
//! Il y a deux vocabulaires, tous deux **fermés** :
//!
//! - les verbes de nettoyage ci-dessous ([`Verb`]), vocabulaire du futur canal
//!   de nettoyage : sans paramètre, **pas encore exécutés** ;
//! - la demande de démarrage ([`request`]), seule exécutée aujourd'hui : quatre
//!   mots, dont **un nom** d'entrée. Elle n'est **pas** « sans danger même si un
//!   programme malveillant l'envoie » : au pire, elle coupe ou remet une entrée
//!   qui existe déjà sous `HKLM` ou dans le dossier Démarrage commun, après une
//!   question posée par l'assistant lui-même.
//!
//! L'assistant refait sa propre analyse avec ses propres règles, compilées dans
//! son binaire, et ne touche jamais au profil d'un utilisateur.
//!
//! - [`request`] : la demande, pure ;
//! - [`location`] : où l'assistant a le droit de se trouver, pure ;
//! - [`startup`] : ce qu'il fait, écrit contre un trait.

pub mod location;
pub mod request;
pub mod startup;

use std::ffi::OsString;
use std::fmt;

/// Fait ce que dit la ligne de commande de l'assistant (quatre mots, voir
/// [`request`]) et rend le code de sortie ([`startup::Code`]). Sans machine
/// Windows (ailleurs que sous Windows), c'est un échec : l'assistant n'a rien à
/// y faire.
pub fn run_command_line(arguments: &[OsString]) -> u8 {
    match crate::platform::machine_startup() {
        Some(machine) => startup::run(machine.as_ref(), arguments),
        None => startup::Code::Failed as u8,
    }
}

/// Une zone du système que seul un administrateur peut nettoyer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SystemArea {
    /// `C:\Windows\Temp`.
    WindowsTemp,
    /// Restes des mises à jour de Windows déjà installées.
    UpdateLeftovers,
    /// Journaux et rapports d'erreur du système.
    SystemLogs,
}

impl SystemArea {
    /// Toutes les zones.
    pub const ALL: [Self; 3] = [Self::WindowsTemp, Self::UpdateLeftovers, Self::SystemLogs];

    /// Nom affiché.
    pub const fn label(self) -> &'static str {
        match self {
            Self::WindowsTemp => "Fichiers temporaires de Windows",
            Self::UpdateLeftovers => "Restes de mises à jour",
            Self::SystemLogs => "Journaux du système",
        }
    }
}

/// Une demande faite à l'assistant élevé.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verb {
    /// Mesurer ce que l'assistant saurait nettoyer. Lecture seule.
    Measure,
    /// Nettoyer une zone, avec les règles de l'assistant.
    Clean(SystemArea),
    /// S'arrêter.
    Quit,
}

impl Verb {
    /// Tous les verbes. Il n'y en a pas d'autres.
    pub const ALL: [Self; 5] = [
        Self::Measure,
        Self::Clean(SystemArea::WindowsTemp),
        Self::Clean(SystemArea::UpdateLeftovers),
        Self::Clean(SystemArea::SystemLogs),
        Self::Quit,
    ];

    /// Le mot envoyé sur le canal.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Measure => "mesurer",
            Self::Clean(SystemArea::WindowsTemp) => "nettoyer:temp-windows",
            Self::Clean(SystemArea::UpdateLeftovers) => "nettoyer:restes-mises-a-jour",
            Self::Clean(SystemArea::SystemLogs) => "nettoyer:journaux-systeme",
            Self::Quit => "quitter",
        }
    }

    /// Reconnaît un verbe, à la lettre près. Tout le reste est refusé : ni
    /// espace, ni majuscule, ni paramètre.
    pub fn parse(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|verb| verb.word() == word)
    }

    /// Vrai si le verbe modifie quelque chose sur la machine.
    pub const fn writes(self) -> bool {
        matches!(self, Self::Clean(_))
    }
}

impl fmt::Display for Verb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_verb_has_its_own_word() {
        for verb in Verb::ALL {
            assert_eq!(Verb::parse(verb.word()), Some(verb));
            assert_eq!(verb.to_string(), verb.word());
        }
        let mut words: Vec<&str> = Verb::ALL.iter().map(|verb| verb.word()).collect();
        words.sort_unstable();
        words.dedup();
        assert_eq!(words.len(), Verb::ALL.len());
        // Chaque zone a son verbe de nettoyage.
        for area in SystemArea::ALL {
            assert!(Verb::ALL.contains(&Verb::Clean(area)), "{}", area.label());
        }
    }

    #[test]
    fn a_word_is_plain_and_carries_no_parameter() {
        for verb in Verb::ALL {
            let word = verb.word();
            assert!(
                word.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'-' || byte == b':'),
                "{word}"
            );
        }
    }

    #[test]
    fn anything_else_is_refused() {
        let refused = [
            "",
            " mesurer",
            "mesurer ",
            "Mesurer",
            "mesurer\n",
            "nettoyer",
            "nettoyer:",
            "nettoyer:C:\\Users",
            "nettoyer:temp-windows C:\\Users\\Public",
            "nettoyer:temp-windows\0",
            "nettoyer:..\\..\\Users",
            "supprimer C:\\Windows",
            "quitter;mesurer",
        ];
        for word in refused {
            assert_eq!(Verb::parse(word), None, "{word:?}");
        }
    }

    #[test]
    fn only_cleaning_writes() {
        assert!(!Verb::Measure.writes());
        assert!(!Verb::Quit.writes());
        assert!(SystemArea::ALL.into_iter().all(|area| Verb::Clean(area).writes()));
    }
}
