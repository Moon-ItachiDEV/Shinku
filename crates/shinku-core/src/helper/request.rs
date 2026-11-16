//! La demande de démarrage : quatre mots, rien d'autre.
//!
//! ```text
//! shinku-helper.exe demarrage1 <couper|remettre> <run|run32|dossier> <hex>
//! ```
//!
//! - exactement quatre arguments ; les trois premiers sont des mots fixes, à la
//!   lettre près. Le `1` de `demarrage1` est la version de la demande ;
//! - `<hex>` est le **nom** de l'entrée (nom de valeur de registre, ou nom du
//!   fichier du dossier Démarrage commun, extension comprise), en UTF-16, quatre
//!   chiffres hexadécimaux **minuscules** par unité, de 1 à
//!   [`MAX_NAME`](crate::startup::MAX_NAME) unités ;
//! - le nom décodé est de l'UTF-16 valide et sans U+0000. Aucun autre filtre :
//!   l'assistant ne l'interprète jamais, il le compare.
//!
//! Tout est pur : le même code sert `shinku.exe` (qui écrit la demande) et
//! l'assistant (qui la lit).

use std::ffi::OsString;

use crate::startup::{MAX_NAME, Place};

/// Le mot qui ouvre la demande, version comprise.
pub const VERSION_WORD: &str = "demarrage1";

/// Couper ou remettre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupAction {
    /// Couper l'entrée (valeur d'approbation « coupée »).
    Cut,
    /// La remettre (valeur d'approbation « active »).
    Restore,
}

impl StartupAction {
    /// Le mot de la demande.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Cut => "couper",
            Self::Restore => "remettre",
        }
    }
}

/// Où l'entrée est inscrite : une des trois places **du PC**. Aucune place du
/// compte n'existe ici : l'assistant ne touche jamais à `HKCU`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StartupSlot {
    /// `HKLM\…\Run`, vue 64 bits.
    Run,
    /// La même clé, vue 32 bits.
    Run32,
    /// Le dossier Démarrage commun.
    Folder,
}

impl StartupSlot {
    /// Les trois places.
    pub const ALL: [Self; 3] = [Self::Run, Self::Run32, Self::Folder];

    /// Le mot de la demande.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Run32 => "run32",
            Self::Folder => "dossier",
        }
    }

    /// La place du module « Démarrage » qui lui correspond.
    pub const fn place(self) -> Place {
        match self {
            Self::Run => Place::MachineRun,
            Self::Run32 => Place::MachineRun32,
            Self::Folder => Place::MachineFolder,
        }
    }

    /// La place du PC qui correspond à cette place du module, `None` pour une
    /// place du compte.
    pub const fn of_place(place: Place) -> Option<Self> {
        match place {
            Place::MachineRun => Some(Self::Run),
            Place::MachineRun32 => Some(Self::Run32),
            Place::MachineFolder => Some(Self::Folder),
            Place::UserRun | Place::UserRun32 | Place::UserFolder => None,
        }
    }
}

/// Une demande de démarrage, comprise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupRequest {
    /// Couper ou remettre.
    pub action: StartupAction,
    /// Où l'entrée est inscrite.
    pub place: StartupSlot,
    /// Le nom exact de l'entrée.
    pub name: String,
}

/// Le nom en hexadécimal : quatre chiffres minuscules par unité UTF-16.
fn encode(name: &str) -> String {
    name.encode_utf16().map(|unit| format!("{unit:04x}")).collect()
}

/// Le nom décodé, ou `None` s'il n'est pas dans la forme exacte.
fn decode(hex: &str) -> Option<String> {
    if hex.is_empty() || !hex.len().is_multiple_of(4) || hex.len() / 4 > MAX_NAME {
        return None;
    }
    if !hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
        return None;
    }
    let units: Vec<u16> = hex
        .as_bytes()
        .chunks(4)
        .map(|chunk| u16::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok())
        .collect::<Option<_>>()?;
    let name = String::from_utf16(&units).ok()?;
    (!name.contains('\0')).then_some(name)
}

impl StartupRequest {
    /// Une demande, si le nom tient dans la forme exacte (1 à [`MAX_NAME`]
    /// unités, pas de U+0000).
    pub fn new(action: StartupAction, place: StartupSlot, name: &str) -> Option<Self> {
        let units = name.encode_utf16().count();
        ((1..=MAX_NAME).contains(&units) && !name.contains('\0')).then(|| Self {
            action,
            place,
            name: name.to_owned(),
        })
    }

    /// Les quatre mots, tels qu'on les met sur la ligne de commande.
    pub fn to_words(&self) -> [String; 4] {
        [
            VERSION_WORD.to_owned(),
            self.action.word().to_owned(),
            self.place.word().to_owned(),
            encode(&self.name),
        ]
    }

    /// Comprend les arguments (sans le nom du programme). Exactement quatre,
    /// de l'Unicode valide, dans la forme exacte ; sinon `None`.
    pub fn parse(arguments: &[OsString]) -> Option<Self> {
        let [version, action, place, name] = arguments else { return None };
        if version.to_str()? != VERSION_WORD {
            return None;
        }
        let action = match action.to_str()? {
            "couper" => StartupAction::Cut,
            "remettre" => StartupAction::Restore,
            _ => return None,
        };
        let place_word = place.to_str()?;
        let place = StartupSlot::ALL.into_iter().find(|slot| slot.word() == place_word)?;
        Some(Self { action, place, name: decode(name.to_str()?)? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn hex(name: &str) -> String {
        encode(name)
    }

    #[test]
    fn a_request_goes_there_and_back() {
        let long = "n".repeat(MAX_NAME);
        for name in [
            "a",
            "Essai Shinku PC",
            "dossier\\piège",
            " espace ",
            "日本語",
            "\u{1F600}",
            "a.lnk",
            &long,
        ] {
            for action in [StartupAction::Cut, StartupAction::Restore] {
                for place in StartupSlot::ALL {
                    let request = StartupRequest::new(action, place, name).unwrap();
                    let words: Vec<OsString> =
                        request.to_words().into_iter().map(Into::into).collect();
                    assert_eq!(StartupRequest::parse(&words), Some(request), "{name}");
                }
            }
        }
    }

    #[test]
    fn the_words_are_the_documented_ones() {
        let request = StartupRequest::new(StartupAction::Cut, StartupSlot::Run32, "Ab").unwrap();
        assert_eq!(request.to_words(), ["demarrage1", "couper", "run32", "00410062"]);
        let request =
            StartupRequest::new(StartupAction::Restore, StartupSlot::Folder, "é").unwrap();
        assert_eq!(request.to_words(), ["demarrage1", "remettre", "dossier", "00e9"]);
    }

    #[test]
    fn exactly_four_arguments_or_nothing() {
        let good = ["demarrage1", "couper", "run", "0041"];
        assert!(StartupRequest::parse(&args(&good)).is_some());
        for count in [0, 1, 2, 3] {
            assert_eq!(StartupRequest::parse(&args(&good[..count])), None, "{count}");
        }
        let mut five = good.to_vec();
        five.push("0041");
        assert_eq!(StartupRequest::parse(&args(&five)), None);
    }

    #[test]
    fn the_fixed_words_are_exact_to_the_letter() {
        for bad in [
            ["demarrage2", "couper", "run", "0041"],
            ["demarrage", "couper", "run", "0041"],
            ["Demarrage1", "couper", "run", "0041"],
            ["demarrage1", "Couper", "run", "0041"],
            ["demarrage1", "couper ", "run", "0041"],
            ["demarrage1", "supprimer", "run", "0041"],
            ["demarrage1", "couper", "RUN", "0041"],
            ["demarrage1", "couper", "run64", "0041"],
            ["demarrage1", "couper", "utilisateur", "0041"],
            ["demarrage1", "couper", "", "0041"],
            [" demarrage1", "couper", "run", "0041"],
        ] {
            assert_eq!(StartupRequest::parse(&args(&bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn the_name_is_four_lowercase_hex_digits_per_unit() {
        let with =
            |name: &str| StartupRequest::parse(&args(&["demarrage1", "couper", "run", name]));
        assert!(with("0041").is_some());
        for bad in [
            "", "41", "004", "00410", "0041 ", " 0041", "004G", "004A", "00A1", "0x41", "+041",
            "00-1", "0041\n",
        ] {
            assert_eq!(with(bad), None, "{bad:?}");
        }
        // Une unité NUL : refusée. Un seul substitut seul : refusé.
        assert_eq!(with("0000"), None);
        assert_eq!(with("00410000"), None);
        assert_eq!(with("d800"), None);
        assert_eq!(with("dc00d800"), None);
        // Une paire de substituts correcte passe.
        assert_eq!(with("d83dde00").map(|request| request.name), Some("\u{1F600}".to_owned()));
        // Aucun autre filtre : barre inverse, blancs, caractères de format.
        assert!(with(&hex("a\\b")).is_some());
        assert!(with(&hex("\u{202E} \t")).is_some());
    }

    #[test]
    fn the_name_length_is_bounded_in_units() {
        let with =
            |name: &str| StartupRequest::parse(&args(&["demarrage1", "couper", "run", name]));
        assert!(with(&"0041".repeat(MAX_NAME)).is_some());
        assert_eq!(with(&"0041".repeat(MAX_NAME + 1)), None);
        // Le nombre d'unités compte, pas de caractères : 128 émojis = 256 unités.
        assert_eq!(with(&hex(&"\u{1F600}".repeat(128))), None);
        assert!(with(&hex(&"\u{1F600}".repeat(127))).is_some());
        assert_eq!(StartupRequest::new(StartupAction::Cut, StartupSlot::Run, ""), None);
        assert_eq!(
            StartupRequest::new(StartupAction::Cut, StartupSlot::Run, &"n".repeat(MAX_NAME + 1)),
            None
        );
        assert_eq!(StartupRequest::new(StartupAction::Cut, StartupSlot::Run, "a\0b"), None);
    }

    #[cfg(unix)]
    #[test]
    fn an_argument_that_is_not_unicode_is_refused() {
        use std::os::unix::ffi::OsStringExt;
        let mut arguments = args(&["demarrage1", "couper", "run", "0041"]);
        for at in 0..4 {
            let mut broken = arguments.clone();
            broken[at] = OsString::from_vec(vec![0x66, 0xFF, 0x66]);
            assert_eq!(StartupRequest::parse(&broken), None, "{at}");
        }
        arguments[3] = OsString::from_vec(vec![0x30, 0x30, 0xC3]);
        assert_eq!(StartupRequest::parse(&arguments), None);
    }

    #[cfg(windows)]
    #[test]
    fn an_argument_that_is_not_unicode_is_refused() {
        use std::os::windows::ffi::OsStringExt;
        let broken = OsString::from_wide(&[0x66, 0xD800, 0x66]);
        for at in 0..4 {
            let mut arguments = args(&["demarrage1", "couper", "run", "0041"]);
            arguments[at] = broken.clone();
            assert_eq!(StartupRequest::parse(&arguments), None, "{at}");
        }
    }

    #[test]
    fn the_places_are_the_three_of_the_pc_only() {
        for slot in StartupSlot::ALL {
            assert_eq!(slot.place().scope(), crate::startup::Scope::Machine);
            assert_eq!(StartupSlot::of_place(slot.place()), Some(slot));
        }
        for place in [Place::UserRun, Place::UserRun32, Place::UserFolder] {
            assert_eq!(StartupSlot::of_place(place), None);
        }
    }
}
