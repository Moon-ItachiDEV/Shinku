//! Le module « Désinstallation » : les apps installées, le désinstalleur de
//! l'éditeur lancé à l'écran, puis ce que l'app a laissé dans le compte.
//!
//! Ce que Shinku **ne fait jamais** :
//!
//! - retirer une app lui-même (ni fichiers du programme supprimés à sa place,
//!   ni option « silencieuse ») : c'est le désinstalleur de l'éditeur qui
//!   travaille, et Shinku n'ajoute aucune option à sa ligne de commande ;
//! - toucher au registre : le module le **lit** et ne l'écrit jamais, aucune
//!   méthode de [`host::AppsHost`] ne le permet ;
//! - proposer un reste coché d'avance, ou le supprimer plus loin que la
//!   corbeille : si la corbeille refuse, l'élément reste et la page le dit ;
//! - s'élever : si un désinstalleur a besoin de droits, c'est Windows qui les
//!   demande, par sa fenêtre.
//!
//! **Le registre est la seule vérité** : une app est désinstallée quand son
//! inscription a disparu, pas quand un processus est sorti (beaucoup de
//! désinstalleurs se recopient ailleurs et le premier processus sort tout de
//! suite). Seul l'installeur de Windows fait exception (voir [`follow`]).
//!
//! Dans le doute, on ne propose pas : **manquer un reste est sans gravité,
//! proposer le dossier d'une autre app ne l'est pas.**
//!
//! - [`host`] : la machine vue par le module (trait et hôte inerte) ;
//! - [`fake`] : une machine fabriquée de toutes pièces, pour les tests ;
//! - [`paths`], [`names`] : des chemins et des noms, comparés comme des textes ;
//! - [`launch`] : quelle commande lancer pour désinstaller ;
//! - [`follow`] : suivre une désinstallation jusqu'à ses restes ;
//! - [`notes`] : `uninstalls.json`, les désinstallations vues, pour le Journal ;
//! - [`claims`] : ce que les autres apps installées réclament ;
//! - [`leftovers`] : chercher, revérifier et retirer les restes ;
//! - [`view`] et [`desk`] : ce que la page reçoit et demande.

use std::fmt;

use serde::{Serialize, Serializer};

use crate::startup::clean_text;

use self::host::{AppsHost, RawNumber, RawRegistration, RawSource};
use self::names::fold_loose;

pub mod claims;
pub mod desk;
pub mod fake;
pub mod follow;
pub mod host;
pub mod launch;
pub mod leftovers;
pub mod names;
pub mod notes;
pub mod paths;
pub mod view;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_claims;
#[cfg(test)]
mod tests_follow;
#[cfg(test)]
mod tests_leftovers;
#[cfg(test)]
mod tests_notes;

/// Nombre de sous-clés lues dans une place, au plus. Au-delà, la place est dite
/// **incomplète** : on ne cache pas ce qu'on n'a pas lu, on le dit.
pub const MAX_KEYS: usize = 4096;

/// Nombre de restes gardés, au plus.
pub const MAX_LEFTOVERS: usize = 64;

/// Nombre de caractères d'une version affichée, au plus.
const VERSION_LIMIT: usize = 64;

/// Une des trois sources que Windows lit pour « Programmes et fonctionnalités ».
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    /// `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`, vue 64 bits.
    Machine64,
    /// La même clé, vue 32 bits.
    Machine32,
    /// `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall`.
    Account,
}

impl Source {
    /// Les trois sources, dans l'ordre où la page les lit.
    pub const ALL: [Self; 3] = [Self::Machine64, Self::Machine32, Self::Account];

    /// Les deux lettres de la source dans un identifiant.
    pub const fn letters(self) -> &'static str {
        match self {
            Self::Machine64 => "m6",
            Self::Machine32 => "m3",
            Self::Account => "ac",
        }
    }

    /// La source qui porte ces deux lettres.
    pub fn from_letters(letters: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|source| source.letters() == letters)
    }

    /// Le compte, ou le PC.
    pub const fn scope(self) -> Scope {
        match self {
            Self::Machine64 | Self::Machine32 => Scope::Machine,
            Self::Account => Scope::Account,
        }
    }

    /// La place du registre qui porte cette source.
    pub const fn place(self) -> Place {
        match self {
            Self::Machine64 => Place::Machine64,
            Self::Machine32 => Place::Machine32,
            Self::Account => Place::Account,
        }
    }
}

/// Une place du registre où l'hôte lit des inscriptions : les trois sources,
/// plus une quatrième lue **seulement pour protéger** (jamais listée) : les
/// voisines, voir [`claims`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Place {
    /// Voir [`Source::Machine64`].
    Machine64,
    /// Voir [`Source::Machine32`].
    Machine32,
    /// Voir [`Source::Account`].
    Account,
    /// `HKCU\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Uninstall`,
    /// chemin littéral (`HKCU\Software` n'est pas redirigé).
    AccountWow,
}

impl Place {
    /// Les quatre places.
    pub const ALL: [Self; 4] = [Self::Machine64, Self::Machine32, Self::Account, Self::AccountWow];
}

/// À qui s'applique une app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// Le compte de la personne.
    Account,
    /// Tous les comptes du PC : Windows demandera son accord.
    Machine,
}

/// Un indice, **par le nom**, sur ce qu'est l'app. Ce sont des ressemblances,
/// jamais des certitudes : les textes de la page le disent ainsi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Une app ordinaire.
    Plain,
    /// Ressemble à un composant dont d'autres apps ont besoin.
    Runtime,
    /// Ressemble à un pilote.
    Driver,
}

/// Ce que Shinku propose pour cette app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Removal {
    /// Le bouton « Désinstaller ».
    Offered,
    /// Windows cache lui-même son bouton (`NoRemove`).
    Refused,
    /// C'est Shinku : il ne se désinstalle pas depuis sa propre fenêtre.
    Own,
    /// Shinku s'affiche grâce à elle (WebView2) : la retirer fermerait la
    /// fenêtre au milieu du geste.
    Needed,
}

/// L'identifiant d'une app : la source, et le nom de sa sous-clé. Texte :
/// `m6:{clé}`, `m3:{clé}` ou `ac:{clé}`.
///
/// La page ne connaît que cet identifiant : elle n'envoie jamais un chemin ni
/// une commande. La commande à lancer est **relue dans le registre** au moment
/// de lancer.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AppId {
    source: Source,
    key: String,
}

impl AppId {
    /// L'identifiant d'une inscription.
    pub fn new(source: Source, key: &str) -> Self {
        Self { source, key: key.to_owned() }
    }

    /// Lit un identifiant. Seule la forme exacte est acceptée : deux lettres
    /// d'une source, deux-points, puis un nom de sous-clé (1 à 255 unités
    /// UTF-16, sans barre inverse, sans zéro, sans caractère de contrôle).
    pub fn parse(text: &str) -> Option<Self> {
        let (letters, key) = text.split_once(':')?;
        let source = Source::from_letters(letters)?;
        let valid = !key.is_empty()
            && key.encode_utf16().count() <= 255
            && !key.contains('\\')
            && !key.chars().any(char::is_control);
        valid.then(|| Self { source, key: key.to_owned() })
    }

    /// La source de l'inscription.
    pub const fn source(&self) -> Source {
        self.source
    }

    /// Le nom de la sous-clé.
    pub fn key(&self) -> &str {
        &self.key
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.source.letters(), self.key)
    }
}

impl Serialize for AppId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Une date de calendrier, rendue `AAAA-MM-JJ`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    /// L'année (1990 à 2100).
    pub year: u16,
    /// Le mois (1 à 12).
    pub month: u8,
    /// Le jour.
    pub day: u8,
}

impl Date {
    /// Lit `AAAAMMJJ` : huit chiffres, une année de 1990 à 2100, un mois et un
    /// jour qui existent. Sinon, `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.len() != 8 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let year: u16 = text[..4].parse().ok()?;
        let month: u8 = text[4..6].parse().ok()?;
        let day: u8 = text[6..].parse().ok()?;
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        ((1990..=2100).contains(&year) && (1..=days).contains(&day)).then_some(Self {
            year,
            month,
            day,
        })
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Une app installée, prête à être montrée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// Son identifiant.
    pub id: AppId,
    /// `DisplayName`, nettoyé.
    pub name: String,
    /// `DisplayVersion`, nettoyée, 64 caractères au plus.
    pub version: Option<String>,
    /// `Publisher`, nettoyé. Ce n'est pas une identité vérifiée.
    pub publisher: Option<String>,
    /// `EstimatedSize` × 1024. Zéro compte comme inconnu : c'est la taille que
    /// l'app **déclare**.
    pub bytes: Option<u64>,
    /// `InstallDate`, si elle est valide.
    pub installed: Option<Date>,
    /// Le compte, ou le PC.
    pub scope: Scope,
    /// Un indice par le nom.
    pub kind: Kind,
    /// Ce que Shinku propose.
    pub removal: Removal,
}

/// Les apps installées, et si la liste est complète.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// Les apps, triées par nom (sans casse ni accents), puis par identifiant.
    pub apps: Vec<App>,
    /// Faux si une source est illisible ou a été coupée par une borne.
    pub complete: bool,
}

/// Vrai si cette inscription est montrée : les conditions de Windows.
///
/// - un nom, après nettoyage ;
/// - `SystemComponent` ne vaut pas 1 (DWORD ou texte) ;
/// - pas de `ParentKeyName` ;
/// - `ReleaseType` ne contient ni `Update` ni `Hotfix` ;
/// - `WindowsInstaller` vaut 1, ou `UninstallString` n'est pas vide.
pub fn is_shown(raw: &RawRegistration) -> bool {
    let named = raw.display_name.as_deref().is_some_and(|name| !clean_text(name).is_empty());
    let hidden = raw.system_component.as_ref().is_some_and(RawNumber::is_one);
    let child = raw.parent_key_name.as_deref().is_some_and(|parent| !parent.trim().is_empty());
    let patch = raw.release_type.as_deref().is_some_and(|kind| {
        let kind = kind.to_lowercase();
        kind.contains("update") || kind.contains("hotfix")
    });
    let removable = raw.windows_installer == Some(1)
        || raw.uninstall_string.as_deref().is_some_and(|line| !line.trim().is_empty());
    named && !hidden && !child && !patch && removable
}

/// Le genre d'app d'après son nom. **C'est un indice par le nom**, sans casse :
/// un nom d'éditeur seul (`NVIDIA `, `Intel(R) `, `AMD `) ne suffit pas, l'app
/// NVIDIA et l'assistant d'Intel ne sont pas des pilotes.
pub fn kind_of(name: &str) -> Kind {
    let folded = names::fold(name);
    let runtime = [
        "microsoft visual c++",
        "microsoft .net",
        "microsoft windows desktop runtime",
        "microsoft asp.net core",
        "microsoft xna",
        "openal",
    ];
    if runtime.iter().any(|prefix| folded.starts_with(prefix)) || folded.contains("directx") {
        return Kind::Runtime;
    }
    let words = names::words(&folded);
    let driver_word = words
        .iter()
        .skip(1)
        .any(|word| matches!(*word, "driver" | "drivers" | "pilote" | "pilotes"));
    if driver_word || folded.contains("chipset") || folded.starts_with("realtek ") {
        return Kind::Driver;
    }
    Kind::Plain
}

/// Ce que Shinku propose pour cette inscription.
fn removal_of(
    raw: &RawRegistration,
    name: &str,
    own_dir: Option<&str>,
    host: &dyn AppsHost,
) -> Removal {
    let folded = names::fold(name);
    let own_key = raw.key.eq_ignore_ascii_case("shinku");
    let own_place = raw
        .install_location
        .as_deref()
        .and_then(|location| paths::clean_location(location, host))
        .zip(own_dir)
        .is_some_and(|(location, own)| paths::norm(&location) == paths::norm(own));
    if own_key || own_place {
        Removal::Own
    } else if folded.starts_with("microsoft edge webview2") {
        Removal::Needed
    } else if raw.no_remove == Some(1) {
        Removal::Refused
    } else {
        Removal::Offered
    }
}

/// L'app que cette inscription décrit, ou `None` si elle n'est pas montrée.
pub fn app_from(
    raw: &RawRegistration,
    source: Source,
    own_dir: Option<&str>,
    host: &dyn AppsHost,
) -> Option<App> {
    if !is_shown(raw) {
        return None;
    }
    let name = clean_text(raw.display_name.as_deref()?);
    let text =
        |value: &Option<String>| value.as_deref().map(clean_text).filter(|text| !text.is_empty());
    let version = text(&raw.display_version)
        .map(|version| {
            version.chars().take(VERSION_LIMIT).collect::<String>().trim_end().to_owned()
        })
        .filter(|version| !version.is_empty());
    let bytes = raw.estimated_size.filter(|&size| size > 0).map(|size| u64::from(size) * 1024);
    Some(App {
        id: AppId::new(source, &raw.key),
        kind: kind_of(&name),
        removal: removal_of(raw, &name, own_dir, host),
        publisher: text(&raw.publisher),
        installed: raw.install_date.as_deref().and_then(Date::parse),
        scope: source.scope(),
        version,
        bytes,
        name,
    })
}

/// Lit les trois sources et en tire les apps montrées. Une source illisible, ou
/// coupée par une borne, rend la liste **incomplète**.
pub fn list(host: &dyn AppsHost) -> Listing {
    let own_dir = host.own_dir();
    let mut listing = Listing { apps: Vec::new(), complete: true };
    for source in Source::ALL {
        let RawSource { entries, complete } = match host.registrations(source.place()) {
            Ok(raw) => raw,
            Err(_) => {
                listing.complete = false;
                continue;
            }
        };
        if !complete || entries.len() > MAX_KEYS {
            listing.complete = false;
        }
        listing.apps.extend(
            entries
                .iter()
                .take(MAX_KEYS)
                .filter_map(|raw| app_from(raw, source, own_dir.as_deref(), host)),
        );
    }
    listing.apps.sort_by_cached_key(|app| (fold_loose(&app.name), app.id.to_string()));
    listing
}
