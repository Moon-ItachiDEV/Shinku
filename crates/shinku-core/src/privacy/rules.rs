//! Le catalogue de la « Confidentialité » : **pur**, sans disque ni registre.
//!
//! Chaque trace est une règle compilée : des fichiers de noms exacts dans un
//! dossier de forme exacte, ou les valeurs de trois clés nommées, ou un appel
//! de Windows. Rien ici ne vient de la page, ni du disque : ce sont des
//! constantes. Un essai parcourt toutes les règles et échoue si l'un des noms
//! de la liste « Jamais » ([`NEVER`]) y apparaît.

use serde::Serialize;

/// Un navigateur de la famille Chromium, version stable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Browser {
    /// Google Chrome.
    Chrome,
    /// Microsoft Edge.
    Edge,
    /// Brave.
    Brave,
    /// Vivaldi.
    Vivaldi,
}

impl Browser {
    /// Les quatre navigateurs, dans l'ordre où la page les montre.
    pub const ALL: [Self; 4] = [Self::Chrome, Self::Edge, Self::Brave, Self::Vivaldi];

    /// Les composants du dossier `User Data`, sous `%LOCALAPPDATA%`. Jamais un
    /// chemin écrit d'un bloc : chaque composant est ajouté un par un.
    pub const fn user_data(self) -> &'static [&'static str] {
        match self {
            Self::Chrome => &["Google", "Chrome", "User Data"],
            Self::Edge => &["Microsoft", "Edge", "User Data"],
            Self::Brave => &["BraveSoftware", "Brave-Browser", "User Data"],
            Self::Vivaldi => &["Vivaldi", "User Data"],
        }
    }

    /// Le nom de l'exécutable, pour savoir si le navigateur tourne.
    pub const fn exe(self) -> &'static str {
        match self {
            Self::Chrome => "chrome.exe",
            Self::Edge => "msedge.exe",
            Self::Brave => "brave.exe",
            Self::Vivaldi => "vivaldi.exe",
        }
    }

    /// Le nom court, tel que la page le reçoit (`app`) et qu'il entre dans la clé d'un bloc.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Edge => "edge",
            Self::Brave => "brave",
            Self::Vivaldi => "vivaldi",
        }
    }

    /// La lettre du navigateur dans un identifiant.
    pub const fn letter(self) -> char {
        match self {
            Self::Chrome => 'c',
            Self::Edge => 'e',
            Self::Brave => 'b',
            Self::Vivaldi => 'v',
        }
    }

    /// Le navigateur qui porte cette lettre.
    pub fn from_letter(letter: char) -> Option<Self> {
        Self::ALL.into_iter().find(|browser| browser.letter() == letter)
    }
}

/// Nombre de chiffres décimaux d'un numéro de profil, au plus.
pub const MAX_SLOT_DIGITS: usize = 6;

/// Le numéro d'un profil : 0 pour `Default`, N pour `Profile N`.
///
/// Un dossier est un profil si, et seulement si, son nom est **exactement**
/// `Default`, ou `Profile ` suivi de chiffres décimaux ASCII sans zéro en tête
/// (de un à six chiffres). `profile 1`, `Profile 01`, `Profile 0`,
/// `Profile 1234567`, `Guest Profile`, `System Profile` n'en sont pas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slot(u32);

impl Slot {
    /// Le profil `Default`.
    pub const DEFAULT: Self = Self(0);

    /// Le profil `Profile N` (N de 1 à 999 999).
    pub fn numbered(number: u32) -> Option<Self> {
        (1..10u32.pow(MAX_SLOT_DIGITS as u32)).contains(&number).then_some(Self(number))
    }

    /// Le numéro (0 pour `Default`).
    pub const fn number(self) -> u32 {
        self.0
    }

    /// Le nom du dossier de ce profil.
    pub fn folder_name(self) -> String {
        if self.0 == 0 { "Default".to_owned() } else { format!("Profile {}", self.0) }
    }

    /// Le profil que ce nom de dossier désigne, comparé **exactement** (la
    /// casse comprise) au nom lu sur le disque.
    pub fn from_folder(name: &str) -> Option<Self> {
        if name == "Default" {
            return Some(Self::DEFAULT);
        }
        let digits = name.strip_prefix("Profile ")?;
        if digits.is_empty()
            || digits.len() > MAX_SLOT_DIGITS
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
            || digits.starts_with('0')
        {
            return None;
        }
        Self::numbered(digits.parse().ok()?)
    }
}

/// Les fichiers de la trace `history`, noms exacts. `Top Sites` est écarté pour
/// Edge : on ne sait pas si ses « liens rapides » y sont (critique T6).
const HISTORY_FILES: [&str; 7] = [
    "History",
    "Archived History",
    "Visited Links",
    "Favicons",
    "Shortcuts",
    "Network Action Predictor",
    "Top Sites",
];

/// Les seules annexes d'une base : rien d'autre ne suit un nom.
pub const ANNEXES: [&str; 3] = ["-journal", "-wal", "-shm"];

/// Les noms des fichiers de la trace `history` d'un navigateur : chaque base,
/// suivie de ses annexes. C'est **le lot** : il part en entier ou pas du tout.
pub fn history_lot(browser: Browser) -> Vec<String> {
    let mut names = Vec::new();
    for base in HISTORY_FILES {
        if base == "Top Sites" && browser == Browser::Edge {
            continue;
        }
        names.push(base.to_owned());
        names.extend(ANNEXES.iter().map(|annex| format!("{base}{annex}")));
    }
    names
}

/// Ce que Shinku ne supprime **jamais**, quel que soit le navigateur : la clé de
/// chiffrement des mots de passe, les mots de passe, la saisie automatique, les
/// favoris, les réglages, les onglets, les cookies, les données des sites et
/// les extensions. Un essai en parcourt les règles ; l'effacement lui-même
/// refuse un lot où l'un de ces noms apparaîtrait ([`is_forbidden`]).
pub const NEVER: [&str; 12] = [
    "Local State",
    "Login Data",
    "Web Data",
    "Bookmarks",
    "Preferences",
    "Sessions",
    "Cookies",
    "Network",
    "Local Storage",
    "IndexedDB",
    "Extensions",
    "Local Extension Settings",
];

/// Vrai si ce nom est interdit : un nom de [`NEVER`] (sans tenir compte de la
/// casse, ni d'une annexe), ou un nom qui contient un séparateur, un joker, un
/// deux-points, `..` ou un caractère de contrôle. Ceinture et bretelles : les
/// règles ne contiennent rien de tout cela, et l'effacement le revérifie.
pub fn is_forbidden(name: &str) -> bool {
    if name.is_empty()
        || name.contains("..")
        || name.chars().any(|c| {
            matches!(c, '\\' | '/' | '*' | '?' | ':' | '"' | '<' | '>' | '|') || c.is_control()
        })
    {
        return true;
    }
    let lower = name.to_lowercase();
    let stem = ANNEXES.iter().find_map(|annex| lower.strip_suffix(annex)).unwrap_or(&lower);
    NEVER.iter().any(|never| {
        let never = never.to_lowercase();
        lower == never || stem == never
    })
}

/// Les trois clés du registre où Windows garde ce que la personne a tapé. Une
/// **énumération compilée** : l'hôte ne reçoit jamais un chemin de registre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypedKey {
    /// `…\Explorer\RunMRU` : la boîte « Exécuter ».
    RunMru,
    /// `…\Explorer\TypedPaths` : la barre d'adresse de l'Explorateur.
    TypedPaths,
    /// `…\Explorer\WordWheelQuery` : la recherche de l'Explorateur.
    WordWheelQuery,
}

impl TypedKey {
    /// Les trois clés, dans l'ordre où elles sont traitées.
    pub const ALL: [Self; 3] = [Self::RunMru, Self::TypedPaths, Self::WordWheelQuery];

    /// Le chemin de la clé sous `HKEY_CURRENT_USER`. Lu par l'hôte Windows
    /// **seulement** : il ne passe jamais par le trait de l'hôte.
    pub const fn path(self) -> &'static str {
        match self {
            Self::RunMru => r"Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU",
            Self::TypedPaths => r"Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths",
            Self::WordWheelQuery => {
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\WordWheelQuery"
            }
        }
    }

    /// Le nom de la valeur qui range l'ordre des autres : elle est supprimée la première.
    pub const fn index_name(self) -> &'static str {
        match self {
            Self::RunMru => "MRUList",
            Self::TypedPaths => "",
            Self::WordWheelQuery => "MRUListEx",
        }
    }
}

/// Nombre de valeurs lues dans une clé, au plus : au-delà, la clé est dite incomplète.
pub const MAX_VALUES: usize = 4096;

/// Longueur maximale (en unités UTF-16) d'un nom de valeur que Shinku supprime.
pub const MAX_VALUE_NAME: usize = 255;

/// Le nom d'une valeur de registre, en unités UTF-16 telles que Windows les donne :
/// ce qu'on a énuméré est exactement ce qu'on supprime, sans conversion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueName(pub Vec<u16>);

impl ValueName {
    /// Le nom de ce texte.
    pub fn new(text: &str) -> Self {
        Self(text.encode_utf16().collect())
    }

    /// Vrai si c'est la valeur d'index de cette clé (`MRUList`, `MRUListEx`),
    /// sans tenir compte de la casse : le registre n'en tient pas.
    pub fn is_index_of(&self, key: TypedKey) -> bool {
        let index: Vec<u16> = key.index_name().encode_utf16().collect();
        !index.is_empty()
            && index.len() == self.0.len()
            && index.iter().zip(&self.0).all(|(wanted, unit)| {
                char::from_u32(u32::from(*unit)).is_some_and(|c| {
                    char::from_u32(u32::from(*wanted)).is_some_and(|w| w.eq_ignore_ascii_case(&c))
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_folder_is_default_or_profile_and_decimal_digits() {
        assert_eq!(Slot::from_folder("Default"), Some(Slot::DEFAULT));
        assert_eq!(Slot::from_folder("Profile 1"), Slot::numbered(1));
        assert_eq!(Slot::from_folder("Profile 12"), Slot::numbered(12));
        assert_eq!(Slot::from_folder("Profile 999999"), Slot::numbered(999_999));
    }

    #[test]
    fn anything_else_is_not_a_profile_folder() {
        for name in [
            "default",
            "DEFAULT",
            "Default ",
            " Default",
            "profile 1",
            "PROFILE 1",
            "Profile 01",
            "Profile 0",
            "Profile  1",
            "Profile 1 ",
            "Profile1",
            "Profile ",
            "Profile",
            "Profile 1234567",
            "Profile -1",
            "Profile +1",
            "Profile 1.0",
            "Profile ١٢",
            "Profile １",
            "Guest Profile",
            "System Profile",
            "Profile 1\0",
            "Profile *",
            "",
            "Local State",
            "..",
            "Default\\x",
        ] {
            assert_eq!(Slot::from_folder(name), None, "{name:?}");
        }
    }

    #[test]
    fn a_slot_goes_back_to_its_folder_name() {
        for number in [0, 1, 9, 10, 77, 999_999] {
            let slot = if number == 0 { Slot::DEFAULT } else { Slot::numbered(number).unwrap() };
            assert_eq!(Slot::from_folder(&slot.folder_name()), Some(slot));
            assert_eq!(slot.number(), number);
        }
        assert_eq!(Slot::numbered(0), None);
        assert_eq!(Slot::numbered(1_000_000), None);
    }

    #[test]
    fn the_history_lot_is_exactly_the_seven_files_and_their_three_annexes() {
        let chrome = history_lot(Browser::Chrome);
        assert_eq!(chrome.len(), 7 * 4);
        for base in [
            "History",
            "Archived History",
            "Visited Links",
            "Favicons",
            "Shortcuts",
            "Network Action Predictor",
            "Top Sites",
        ] {
            for suffix in ["", "-journal", "-wal", "-shm"] {
                assert!(chrome.contains(&format!("{base}{suffix}")), "{base}{suffix}");
            }
        }
        // Chaque base est suivie de ses annexes, la base d'abord.
        assert_eq!(chrome[..4], ["History", "History-journal", "History-wal", "History-shm"]);
        for browser in [Browser::Brave, Browser::Vivaldi] {
            assert_eq!(history_lot(browser), chrome);
        }
    }

    #[test]
    fn edge_never_loses_its_top_sites() {
        let edge = history_lot(Browser::Edge);
        assert_eq!(edge.len(), 6 * 4);
        assert!(edge.iter().all(|name| !name.starts_with("Top Sites")));
        assert!(
            history_lot(Browser::Chrome).iter().any(|name| name == "Top Sites"),
            "les autres navigateurs gardent `Top Sites`"
        );
    }

    #[test]
    fn no_rule_of_any_browser_ever_names_a_forbidden_place() {
        for browser in Browser::ALL {
            let lot = history_lot(browser);
            assert!(!lot.is_empty());
            for name in &lot {
                for never in NEVER {
                    assert!(
                        !name.eq_ignore_ascii_case(never),
                        "{browser:?} nomme {never} dans sa règle"
                    );
                    for annex in ANNEXES {
                        assert!(
                            !name.eq_ignore_ascii_case(&format!("{never}{annex}")),
                            "{browser:?} nomme {never}{annex}"
                        );
                    }
                }
                assert!(!is_forbidden(name), "{browser:?} : {name}");
                assert!(
                    !name.contains(['\\', '/', '*', '?', ':']) && !name.contains(".."),
                    "{browser:?} : {name} contient un séparateur, un joker ou `..`"
                );
            }
        }
        // Les composants du dossier `User Data` obéissent aux mêmes règles.
        for browser in Browser::ALL {
            for part in browser.user_data() {
                assert!(!part.contains(['\\', '/', '*', '?', ':']) && !part.contains(".."));
            }
        }
    }

    #[test]
    fn the_guard_catches_every_forbidden_name_in_every_disguise() {
        for never in NEVER {
            assert!(is_forbidden(never), "{never}");
            assert!(is_forbidden(&never.to_uppercase()), "{never}");
            assert!(is_forbidden(&format!("{never}-wal")), "{never}");
        }
        for name in
            ["", "a\\b", "a/b", "a*", "a?", "a:b", "..", "a..b", "a\u{7}", "Network\\Cookies"]
        {
            assert!(is_forbidden(name), "{name:?}");
        }
        assert!(!is_forbidden("History"));
        assert!(!is_forbidden("Top Sites-journal"));
    }

    #[test]
    fn the_registry_keys_are_the_three_explorer_keys_of_the_account() {
        let prefix = r"Software\Microsoft\Windows\CurrentVersion\Explorer\";
        let paths: Vec<&str> = TypedKey::ALL.iter().map(|key| key.path()).collect();
        assert_eq!(
            paths,
            [
                format!("{prefix}RunMRU"),
                format!("{prefix}TypedPaths"),
                format!("{prefix}WordWheelQuery"),
            ]
        );
    }

    #[test]
    fn an_index_value_is_recognised_without_regard_to_case() {
        assert!(ValueName::new("MRUList").is_index_of(TypedKey::RunMru));
        assert!(ValueName::new("mrulist").is_index_of(TypedKey::RunMru));
        assert!(ValueName::new("MRUListEx").is_index_of(TypedKey::WordWheelQuery));
        assert!(!ValueName::new("MRUListEx").is_index_of(TypedKey::RunMru));
        assert!(!ValueName::new("a").is_index_of(TypedKey::RunMru));
        // `TypedPaths` n'a pas d'index : aucune de ses valeurs n'est « l'index ».
        assert!(!ValueName::new("").is_index_of(TypedKey::TypedPaths));
        assert!(!ValueName::new("url1").is_index_of(TypedKey::TypedPaths));
    }
}
