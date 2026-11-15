//! Le module « Démarrage » : les apps inscrites au démarrage de Windows, et
//! l'interrupteur qui les coupe **au même endroit que le Gestionnaire des
//! tâches**.
//!
//! Shinku ne supprime aucune entrée et ne désinstalle rien : il ne fait que
//! l'approuver ou la désapprouver, là où Windows le fait (les valeurs
//! `StartupApproved`, voir [`approval`]). L'app reste installée et se remet
//! d'un clic, ici ou dans Windows.
//!
//! Six **places** sont lues ([`Place`]) : les valeurs `Run` du compte (vue 64
//! bits et 32 bits), le dossier Démarrage du compte, et les trois mêmes pour
//! le PC. Ce lot règle les entrées **du compte** ; celles du PC sont montrées
//! en lecture seule (les régler demande un administrateur : lot suivant).
//!
//! Ce que Shinku ne voit pas, il ne le dit pas : ni les apps du Microsoft
//! Store, ni les tâches planifiées, ni les services ne sont ici.
//!
//! - [`command`] : de quel programme parle une ligne de commande ;
//! - [`approval`] : les douze octets de l'approbation, lus strictement ;
//! - [`host`] : la machine vue par le module (trait et faux hôte) ;
//! - [`store`] : `startup.json`, ce que Shinku a coupé ;
//! - [`view`] et [`desk`] : ce que la page reçoit et demande.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde::{Serialize, Serializer};

use crate::game::apps;
use crate::game::detect::file_name;
use crate::platform::agent::RUN_VALUE;

use self::approval::Approval;
use self::command::Target;
use self::host::{FileIdentity, RawEntry, StartupHost};
use self::store::StartupStore;

pub mod approval;
pub mod command;
pub mod desk;
pub mod host;
pub mod store;
pub mod view;

/// Nombre de caractères d'un nom affiché, au plus.
pub const DISPLAY_LIMIT: usize = 80;

/// Nombre d'entrées lues dans une place, au plus. Au-delà, la place est dite
/// **incomplète** : on ne cache pas ce qu'on n'a pas lu, on le dit.
pub const MAX_ENTRIES: usize = 300;

/// Descriptions de fichiers qui ne disent rien de l'app : le nom inscrit au
/// démarrage est alors plus parlant (sans casse, après retrait d'un `.exe`).
const GENERIC: [&str; 13] = [
    "update",
    "updater",
    "launcher",
    "setup",
    "helper",
    "electron",
    "application",
    "app",
    "host",
    "service",
    "agent",
    "tray",
    "startup",
];

/// Une des six places où Windows cherche des programmes à lancer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Place {
    /// Valeurs de `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
    UserRun,
    /// Valeurs de `HKCU\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Run`.
    UserRun32,
    /// Fichiers du dossier Démarrage du compte.
    UserFolder,
    /// Valeurs de `HKLM\Software\Microsoft\Windows\CurrentVersion\Run`, vue 64 bits.
    MachineRun,
    /// La même clé, vue 32 bits.
    MachineRun32,
    /// Fichiers du dossier Démarrage commun.
    MachineFolder,
}

impl Place {
    /// Les six places, dans l'ordre où la page les montre.
    pub const ALL: [Self; 6] = [
        Self::UserRun,
        Self::UserRun32,
        Self::UserFolder,
        Self::MachineRun,
        Self::MachineRun32,
        Self::MachineFolder,
    ];

    /// Les deux lettres de la place dans un identifiant.
    pub const fn letters(self) -> &'static str {
        match self {
            Self::UserRun => "ur",
            Self::UserRun32 => "uw",
            Self::UserFolder => "uf",
            Self::MachineRun => "mr",
            Self::MachineRun32 => "mw",
            Self::MachineFolder => "mf",
        }
    }

    /// La place qui porte ces deux lettres.
    pub fn from_letters(letters: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|place| place.letters() == letters)
    }

    /// Le compte, ou le PC.
    pub const fn scope(self) -> Scope {
        match self {
            Self::UserRun | Self::UserRun32 | Self::UserFolder => Scope::User,
            Self::MachineRun | Self::MachineRun32 | Self::MachineFolder => Scope::Machine,
        }
    }

    /// Une valeur de registre, ou un fichier d'un dossier.
    pub const fn origin(self) -> Origin {
        match self {
            Self::UserRun | Self::UserRun32 | Self::MachineRun | Self::MachineRun32 => {
                Origin::Registry
            }
            Self::UserFolder | Self::MachineFolder => Origin::Folder,
        }
    }
}

/// À qui s'applique une entrée.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// Le compte de la personne.
    User,
    /// Tous les comptes du PC.
    Machine,
}

/// D'où vient une entrée.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// Une valeur de registre.
    Registry,
    /// Un fichier du dossier Démarrage.
    Folder,
}

/// L'état d'une entrée, tel que Windows l'approuve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Elle se lance à l'ouverture de la session.
    Enabled,
    /// Elle est coupée.
    Disabled,
    /// Shinku ne sait pas lire son réglage : il n'y touche pas.
    Unknown,
}

/// Ce que la ligne de commande désigne.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    /// Un fichier qui existe.
    Found,
    /// Un chemin absolu qui n'existe pas : l'app semble désinstallée.
    Missing,
    /// Un programme hôte lance autre chose.
    Hosted,
    /// Impossible à dire.
    Unknown,
}

/// Une remarque sur une entrée.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Hint {
    /// Une app de synchronisation de fichiers.
    Sync,
}

/// Ce que Shinku sait de l'histoire d'une entrée, grâce à `startup.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Mark {
    /// Shinku l'a coupée, et elle l'est toujours.
    CutByShinku {
        /// Date de la coupure, en millisecondes depuis 1970.
        #[serde(rename = "at")]
        at_ms: i64,
    },
    /// Shinku l'avait coupée, et elle s'est remise sans lui.
    CameBack {
        /// Date de la coupure, en millisecondes depuis 1970.
        #[serde(rename = "cut")]
        cut_ms: i64,
    },
}

/// L'identifiant d'une entrée : les deux lettres de sa place, un tiret, puis le
/// FNV-1a 64 bits du nom brut en 16 chiffres hexadécimaux minuscules.
///
/// Le hachage porte sur les unités UTF-16 du nom, **telles qu'énumérées, sans
/// changer la casse**, chacune donnée en deux octets (poids faible d'abord).
/// La page ne connaît que cet identifiant : elle n'envoie jamais un nom de
/// valeur ni un chemin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId {
    place: Place,
    hash: u64,
}

impl EntryId {
    /// L'identifiant d'une entrée de cette place.
    pub fn new(place: Place, raw_name: &str) -> Self {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut hash = OFFSET;
        for unit in raw_name.encode_utf16() {
            for byte in unit.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(PRIME);
            }
        }
        Self { place, hash }
    }

    /// Lit un identifiant. Seule la forme exacte est acceptée : 19 caractères,
    /// deux lettres d'une place, un tiret, seize chiffres hexadécimaux
    /// minuscules.
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != 19 || !text.is_ascii() {
            return None;
        }
        let place = Place::from_letters(&text[..2])?;
        if text.as_bytes()[2] != b'-' {
            return None;
        }
        let digits = &text[3..];
        if !digits.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            return None;
        }
        Some(Self { place, hash: u64::from_str_radix(digits, 16).ok()? })
    }

    /// La place de l'entrée.
    pub const fn place(self) -> Place {
        self.place
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{:016x}", self.place.letters(), self.hash)
    }
}

impl Serialize for EntryId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Une entrée de démarrage, prête à être montrée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Son identifiant.
    pub id: EntryId,
    /// Sa place.
    pub place: Place,
    /// Son nom, tel que la place le donne (c'est ce nom qui désigne son
    /// approbation).
    pub raw_name: String,
    /// Le nom affiché en premier.
    pub name: String,
    /// Le nom brut nettoyé, s'il diffère de `name`.
    pub alias: Option<String>,
    /// La société que le fichier déclare. Ce n'est pas une identité vérifiée.
    pub publisher: Option<String>,
    /// L'état.
    pub state: State,
    /// Vrai si une valeur d'approbation existe dans le registre (voir
    /// [`Approval::present`]).
    pub approval_present: bool,
    /// Le `FILETIME` de l'approbation « coupée », tel que lu (0 sinon).
    pub file_time: u64,
    /// Date de la coupure lue dans l'approbation, si elle est plausible.
    pub disabled_ms: Option<i64>,
    /// Ce que `startup.json` en sait.
    pub mark: Option<Mark>,
    /// Ce que la commande désigne.
    pub target: TargetKind,
    /// Le programme hôte, si la commande en passe par un.
    pub host: Option<String>,
    /// Une remarque.
    pub hint: Option<Hint>,
    /// L'entrée de Shinku lui-même (son agent du mode jeu).
    pub own: bool,
    /// Vrai si Shinku sait la régler.
    pub adjustable: bool,
    name_exact: bool,
}

impl Entry {
    /// À qui l'entrée s'applique.
    pub const fn scope(&self) -> Scope {
        self.place.scope()
    }
}

/// Ce que les six places contiennent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// Les entrées : le compte d'abord, puis le PC, chaque portée triée.
    pub entries: Vec<Entry>,
    /// Les places qui n'ont pas pu être lues.
    pub unreadable: Vec<Place>,
    /// Les places lues **en partie** : une borne a été atteinte (trop
    /// d'entrées, nom ou valeur démesurés). Ce qui est montré est vrai, mais
    /// ce n'est peut-être pas tout.
    pub incomplete: Vec<Place>,
}

/// Ce qu'un changement a fait.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Le nom affiché de l'entrée.
    pub name: String,
    /// Le `FILETIME` écrit pour une coupure (0 pour une remise).
    pub file_time: u64,
    /// Faux si l'entrée était déjà dans l'état voulu : rien n'a été écrit.
    pub changed: bool,
}

/// Pourquoi un changement n'a pas eu lieu. Rien n'a été écrit (sauf `Failed`
/// après une relecture qui ne donne pas l'état voulu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleError {
    /// Aucune entrée, ou plusieurs, ne porte cet identifiant.
    UnknownEntry,
    /// Cette entrée ne se règle pas ici.
    NotAdjustable,
    /// Windows a refusé l'écriture, ou la relecture ne donne pas l'état voulu.
    Failed,
}

impl fmt::Display for ToggleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownEntry => "cette entrée n'existe plus",
            Self::NotAdjustable => "cette entrée ne se règle pas ici",
            Self::Failed => "le changement n'a pas pu être fait",
        })
    }
}

impl std::error::Error for ToggleError {}

// --- Nettoyage des textes affichés ---

/// Caractères de format (catégorie Cf d'Unicode) : invisibles, ils servent à
/// tromper l'œil (U+202E renverse le sens du texte).
fn is_format(character: char) -> bool {
    matches!(
        u32::from(character),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

/// Un texte prêt à être montré : caractères de contrôle et de format retirés,
/// blancs réduits à une espace, 80 caractères au plus.
pub fn clean_text(text: &str) -> String {
    let spaced: String = text
        .chars()
        .map(|character| if character.is_whitespace() { ' ' } else { character })
        .filter(|&character| !character.is_control() && !is_format(character))
        .collect();
    let words: Vec<&str> = spaced.split_whitespace().collect();
    let joined = words.join(" ");
    joined.chars().take(DISPLAY_LIMIT).collect::<String>().trim_end().to_owned()
}

/// Vrai pour une description qui ne dit rien de l'app.
fn is_generic(description: &str) -> bool {
    let lower = description.to_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    GENERIC.contains(&stem)
}

/// Le nom brut d'une entrée, sans `.lnk` pour un raccourci, nettoyé.
fn raw_label(place: Place, raw_name: &str) -> String {
    let name = match (place.origin(), raw_name.len().checked_sub(4)) {
        (Origin::Folder, Some(at))
            if raw_name.is_char_boundary(at) && raw_name[at..].eq_ignore_ascii_case(".lnk") =>
        {
            &raw_name[..at]
        }
        _ => raw_name,
    };
    clean_text(name)
}

// --- Énumération ---

/// Vrai pour l'entrée de Shinku lui-même : la valeur `Run` de Shinku, dans la
/// place du compte, **et** portant exactement la commande que le mode jeu y
/// écrit pour cette installation. Une valeur du même nom qui porte autre chose
/// est une entrée ordinaire.
fn is_own(place: Place, name: &str, command: &str, own: Option<&str>) -> bool {
    place == Place::UserRun
        && name.eq_ignore_ascii_case(RUN_VALUE)
        && own.is_some_and(|wanted| wanted == command)
}

fn build(host: &dyn StartupHost, own: Option<&str>, place: Place, raw: RawEntry) -> Entry {
    // Un nom qui n'est pas de l'UTF-16 valide est approximatif : l'approbation
    // se chercherait sous un autre nom que le vrai, et son absence ferait
    // passer l'entrée pour « active ». On ne la lit pas : état inconnu.
    let approval: Approval = if raw.name_exact {
        approval::read(&host.approval(place, &raw.name), host.file_time_now())
    } else {
        Approval::unreadable()
    };
    let (target, path, hosted) = match command::target_of(&raw.command, host) {
        Target::Found(path) => (TargetKind::Found, Some(path), None),
        Target::Missing(_) => (TargetKind::Missing, None, None),
        Target::Hosted(name) => (TargetKind::Hosted, None, Some(clean_text(&name))),
        Target::Unknown => (TargetKind::Unknown, None, None),
    };
    let identity = path.as_deref().map(|path| host.identity(path)).unwrap_or_default();
    let FileIdentity { description, company } = identity;

    let label = raw_label(place, &raw.name);
    let described = description
        .map(|text| clean_text(&text))
        .filter(|text| !text.is_empty() && !is_generic(text));
    let name = described.unwrap_or_else(|| label.clone());
    let name = if name.is_empty() { "Sans nom".to_owned() } else { name };
    let alias = (!label.is_empty() && label.to_lowercase() != name.to_lowercase()).then_some(label);
    let publisher = company.map(|text| clean_text(&text)).filter(|text| !text.is_empty());
    let hint = path
        .as_deref()
        .is_some_and(|path| apps::is_sync_app(&file_name(path)))
        .then_some(Hint::Sync);
    let is_own = is_own(place, &raw.name, &raw.command, own);

    Entry {
        id: EntryId::new(place, &raw.name),
        place,
        raw_name: raw.name,
        name,
        alias,
        publisher,
        state: approval.state,
        approval_present: approval.present,
        file_time: approval.file_time,
        disabled_ms: approval.disabled_ms,
        mark: None,
        target,
        host: hosted,
        hint,
        own: is_own,
        adjustable: false,
        name_exact: raw.name_exact,
    }
}

/// Lit ces places. Les entrées sont triées par portée, puis par nom.
fn collect(host: &dyn StartupHost, own: Option<&str>, places: &[Place]) -> Listing {
    let mut listing = Listing::default();
    for &place in places {
        match host.entries(place) {
            Ok(raws) => {
                if !raws.complete || raws.entries.len() > MAX_ENTRIES {
                    listing.incomplete.push(place);
                }
                listing.entries.extend(
                    raws.entries
                        .into_iter()
                        .take(MAX_ENTRIES)
                        .map(|raw| build(host, own, place, raw)),
                );
            }
            Err(_) => listing.unreadable.push(place),
        }
    }
    let mut counts: HashMap<EntryId, usize> = HashMap::new();
    for entry in &listing.entries {
        *counts.entry(entry.id).or_default() += 1;
    }
    for entry in &mut listing.entries {
        entry.adjustable = entry.scope() == Scope::User
            && matches!(entry.state, State::Enabled | State::Disabled)
            && entry.name_exact
            && !entry.own
            && counts.get(&entry.id) == Some(&1);
    }
    // Compte d'abord, PC ensuite ; dans chaque portée, par nom sans casse, puis
    // par alias, puis par identifiant. Le tri est stable : les places gardent
    // leur ordre à égalité parfaite.
    listing.entries.sort_by_cached_key(|entry| {
        (
            entry.scope() != Scope::User,
            entry.name.to_lowercase(),
            entry.alias.as_ref().map(|alias| alias.to_lowercase()),
            entry.id.to_string(),
        )
    });
    listing
}

/// Les six places, avec ce que `startup.json` sait de ce que Shinku a coupé.
///
/// `own` est la commande exacte que le mode jeu écrit pour cette installation
/// dans la valeur `Run` de Shinku : une entrée qui la porte est celle de Shinku
/// lui-même. Le fichier n'est réécrit que si quelque chose a changé.
pub fn enumerate(
    host: &dyn StartupHost,
    own: Option<&str>,
    store: Option<&StartupStore>,
) -> Listing {
    let mut listing = collect(host, own, &Place::ALL);
    if let Some(store) = store {
        let mut file = store.load();
        // Une place lue en partie ne prouve pas qu'une entrée a disparu.
        let unseen: Vec<Place> =
            listing.unreadable.iter().chain(&listing.incomplete).copied().collect();
        let marks = file.reconcile(&listing.entries, &unseen, host.now().unix_millis());
        if marks.changed {
            // Un fichier qui ne s'écrit pas ne doit rien changer à ce que la
            // page voit : les marques viennent de la relecture ci-dessus.
            let _ = store.save(&file);
        }
        for entry in &mut listing.entries {
            entry.mark = marks.of.get(&entry.id).copied();
        }
    }
    listing
}

/// Le nombre d'entrées actives du compte et du PC, sans celle de Shinku. `None`
/// si une place n'a pas pu être lue **en entier**, ou si la machine ne sait pas
/// compter : on ne donne pas un chiffre faux.
pub fn count_enabled(host: &dyn StartupHost, own: Option<&str>) -> Option<u64> {
    if !host.measures() {
        return None;
    }
    let mut count = 0u64;
    for place in Place::ALL {
        let raws = host.entries(place).ok()?;
        if !raws.complete || raws.entries.len() > MAX_ENTRIES {
            return None;
        }
        for raw in raws.entries {
            // Un nom approximatif : état inconnu, pas compté (voir `build`).
            if !raw.name_exact {
                continue;
            }
            let state =
                approval::read(&host.approval(place, &raw.name), host.file_time_now()).state;
            if state == State::Enabled && !is_own(place, &raw.name, &raw.command, own) {
                count += 1;
            }
        }
    }
    Some(count)
}

/// Ce qu'il faut pour compter les apps inscrites à la fin d'une analyse.
#[derive(Clone, Debug)]
pub struct Probe {
    host: Arc<dyn StartupHost>,
    own: Option<String>,
}

impl Probe {
    /// Une sonde sur cette machine ; `own` est la commande de Shinku (voir [`enumerate`]).
    pub fn new(host: Arc<dyn StartupHost>, own: Option<String>) -> Self {
        Self { host, own }
    }

    /// Le nombre d'entrées actives hors celle de Shinku, ou `None`.
    pub fn count(&self) -> Option<u64> {
        count_enabled(self.host.as_ref(), self.own.as_deref())
    }
}

/// Change l'état d'une entrée **du compte**, puis relit l'approbation pour
/// vérifier que Windows la lit comme voulu.
///
/// Les deux seules écritures sont [`approval::cut_bytes`] et
/// [`approval::restore_bytes`], et seulement pour une entrée lue active ou
/// coupée. Ne fait rien si l'état est déjà le bon.
pub fn toggle(
    host: &dyn StartupHost,
    own: Option<&str>,
    id: EntryId,
    enabled: bool,
) -> Result<Change, ToggleError> {
    // On relit la place de l'entrée, pas la dernière vue de la page : la
    // machine a pu changer entre-temps.
    let listing = collect(host, own, &[id.place()]);
    if !listing.unreadable.is_empty() {
        return Err(ToggleError::Failed);
    }
    let mut matching = listing.entries.iter().filter(|entry| entry.id == id);
    let (Some(entry), None) = (matching.next(), matching.next()) else {
        return Err(ToggleError::UnknownEntry);
    };
    if !entry.adjustable {
        return Err(ToggleError::NotAdjustable);
    }
    let wanted = if enabled { State::Enabled } else { State::Disabled };
    if entry.state == wanted {
        return Ok(Change { name: entry.name.clone(), file_time: entry.file_time, changed: false });
    }
    let (bytes, file_time) = if enabled {
        (approval::restore_bytes(), 0)
    } else {
        let now = host.file_time_now();
        (approval::cut_bytes(now), now)
    };
    host.set_approval(entry.place, &entry.raw_name, bytes).map_err(|_| ToggleError::Failed)?;
    let read = approval::read(&host.approval(entry.place, &entry.raw_name), host.file_time_now());
    if read.state != wanted {
        return Err(ToggleError::Failed);
    }
    Ok(Change { name: entry.name.clone(), file_time, changed: true })
}

#[cfg(test)]
mod tests;
