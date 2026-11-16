//! L'identifiant d'une trace : ce que la page renvoie, et rien d'autre.
//!
//! Pour l'historique d'un profil, il porte le navigateur, le numéro du profil,
//! **l'identité du dossier de profil** (volume et identifiant de fichier), et une
//! empreinte du nom affiché. À l'effacement, le cœur refait la liste, retrouve le
//! dossier par son nom compilé et exige la même identité et le même nom : un
//! profil supprimé puis recréé sous le même numéro n'est pas le même profil
//! (critique C6).
//!
//! Le décodage est **strict** : seul ce que le cœur fabrique est accepté, à
//! l'octet près. Tout le reste (une casse, un zéro en trop, un tiret de plus)
//! est refusé.
//!
//! ```text
//! hc-0-0000000000000001-00000000000000000000000000000002-0000000000000000
//! │└ navigateur (c e b v)                                  └ empreinte du nom
//! └ history       │ └ numéro du profil, décimal sans zéro en tête
//! wr   wt   wc    : recent, typed, clipboard
//! ```

use std::fmt;

use serde::{Serialize, Serializer};

use crate::fs::Identity;
use crate::model::FileId;
use crate::privacy::rules::{Browser, MAX_SLOT_DIGITS, Slot};

/// Longueur maximale d'un identifiant que le cœur accepte de lire.
pub const MAX_ID_LEN: usize = 96;

/// L'empreinte d'un nom affiché : FNV-1a 64 bits de son texte UTF-8, ou 0 s'il
/// n'y a pas de nom lisible.
pub fn name_stamp(name: Option<&str>) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let Some(name) = name else { return 0 };
    let mut hash = OFFSET;
    for byte in name.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Ce qu'un identifiant désigne.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TraceId {
    /// L'historique d'un profil de navigateur.
    History {
        /// Le navigateur.
        browser: Browser,
        /// Le numéro du profil.
        slot: Slot,
        /// L'identité du dossier de profil.
        identity: Identity,
        /// L'empreinte du nom affiché (voir [`name_stamp`]).
        name: u64,
    },
    /// Fichiers et dossiers récents.
    Recent,
    /// Ce qui a été tapé.
    Typed,
    /// Le presse-papiers.
    Clipboard,
}

fn is_lower_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl TraceId {
    /// Lit un identifiant. `None` pour tout ce que [`fmt::Display`] ne fabriquerait pas.
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() > MAX_ID_LEN || !text.is_ascii() {
            return None;
        }
        let parsed = match text {
            "wr" => Self::Recent,
            "wt" => Self::Typed,
            "wc" => Self::Clipboard,
            _ => Self::parse_history(text)?,
        };
        // Strict, jusqu'au bout : ce qu'on relit s'écrit comme ce qu'on a reçu.
        (parsed.to_string() == text).then_some(parsed)
    }

    fn parse_history(text: &str) -> Option<Self> {
        let mut parts = text.split('-');
        let head = parts.next()?;
        let slot = parts.next()?;
        let volume = parts.next()?;
        let file = parts.next()?;
        let name = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        let mut head = head.chars();
        if head.next()? != 'h' {
            return None;
        }
        let browser = Browser::from_letter(head.next()?)?;
        if head.next().is_some() {
            return None;
        }
        if slot.is_empty()
            || slot.len() > MAX_SLOT_DIGITS
            || !slot.bytes().all(|b| b.is_ascii_digit())
            || (slot.len() > 1 && slot.starts_with('0'))
        {
            return None;
        }
        let number: u32 = slot.parse().ok()?;
        let slot = if number == 0 { Slot::DEFAULT } else { Slot::numbered(number)? };
        if !is_lower_hex(volume, 16) || !is_lower_hex(file, 32) || !is_lower_hex(name, 16) {
            return None;
        }
        Some(Self::History {
            browser,
            slot,
            identity: Identity {
                volume: u64::from_str_radix(volume, 16).ok()?,
                file_id: FileId(u128::from_str_radix(file, 16).ok()?),
            },
            name: u64::from_str_radix(name, 16).ok()?,
        })
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::History { browser, slot, identity, name } => write!(
                f,
                "h{}-{}-{:016x}-{:032x}-{:016x}",
                browser.letter(),
                slot.number(),
                identity.volume,
                identity.file_id.0,
                name
            ),
            Self::Recent => f.write_str("wr"),
            Self::Typed => f.write_str("wt"),
            Self::Clipboard => f.write_str("wc"),
        }
    }
}

impl Serialize for TraceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(browser: Browser, number: u32, volume: u64, file: u128, name: u64) -> TraceId {
        TraceId::History {
            browser,
            slot: if number == 0 { Slot::DEFAULT } else { Slot::numbered(number).unwrap() },
            identity: Identity { volume, file_id: FileId(file) },
            name,
        }
    }

    #[test]
    fn an_id_goes_there_and_back() {
        let ids = [
            TraceId::Recent,
            TraceId::Typed,
            TraceId::Clipboard,
            history(Browser::Chrome, 0, 1, 2, 0),
            history(Browser::Edge, 77, u64::MAX, u128::MAX, u64::MAX),
            history(Browser::Brave, 999_999, 0, 0, name_stamp(Some("Léa"))),
            history(Browser::Vivaldi, 1, 0xdead_beef, 0x1234_5678_9abc_def0, 42),
        ];
        for id in ids {
            let text = id.to_string();
            assert!(text.len() <= MAX_ID_LEN, "{text}");
            assert!(text.is_ascii());
            assert_eq!(TraceId::parse(&text), Some(id), "{text}");
        }
        assert_eq!(
            history(Browser::Chrome, 0, 1, 2, 0).to_string(),
            "hc-0-0000000000000001-00000000000000000000000000000002-0000000000000000"
        );
    }

    #[test]
    fn the_json_of_an_id_is_its_text() {
        let json = serde_json::to_value(history(Browser::Chrome, 3, 1, 2, 0)).unwrap();
        assert_eq!(json, "hc-3-0000000000000001-00000000000000000000000000000002-0000000000000000");
        assert_eq!(serde_json::to_value(TraceId::Typed).unwrap(), "wt");
    }

    #[test]
    fn what_the_core_would_not_make_is_refused() {
        let good = history(Browser::Chrome, 5, 1, 2, 3).to_string();
        let bad = [
            String::new(),
            "w".to_owned(),
            "WR".to_owned(),
            "wr ".to_owned(),
            " wr".to_owned(),
            "wr\n".to_owned(),
            "wx".to_owned(),
            "hc".to_owned(),
            good.to_uppercase(),
            good.replacen("hc", "hx", 1),
            good.replacen("hc", "he-", 1),
            format!("{good}-0"),
            format!("{good} "),
            good.replacen("-5-", "-05-", 1),
            good.replacen("-5-", "--", 1),
            good.replacen("-5-", "-1000000-", 1),
            good.replacen("-5-", "-+5-", 1),
            good.replacen("-5-", "-５-", 1),
            good.replacen("-0000000000000001-", "-1-", 1),
            good.replacen("-0000000000000001-", "-000000000000000G-", 1),
            good.replacen("-0000000000000001-", "-0000000000000001x-", 1),
            good.replace('-', "_"),
            "hc-0-0000000000000001-00000000000000000000000000000002".to_owned(),
            "a".repeat(MAX_ID_LEN + 1),
        ];
        for text in bad {
            assert_eq!(TraceId::parse(&text), None, "{text:?}");
        }
        // Un volume écrit avec des majuscules n'est pas ce que le cœur fabrique.
        let upper = history(Browser::Chrome, 0, 0xab, 0, 0).to_string().replacen("ab", "AB", 1);
        assert_eq!(TraceId::parse(&upper), None);
    }

    #[test]
    fn a_path_or_a_name_is_never_an_id() {
        for text in [
            r"C:\Users\a\AppData\Local\Google\Chrome\User Data\Default\History",
            "../History",
            "History",
            "RunMRU",
            "Software\\Microsoft",
        ] {
            assert_eq!(TraceId::parse(text), None);
        }
    }

    #[test]
    fn the_name_stamp_tells_names_apart() {
        assert_eq!(name_stamp(None), 0);
        assert_ne!(name_stamp(Some("Travail")), name_stamp(Some("Perso")));
        assert_eq!(name_stamp(Some("Perso")), name_stamp(Some("Perso")));
        assert_ne!(name_stamp(Some("")), 0);
    }
}
