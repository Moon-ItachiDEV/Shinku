//! Ce que la page « Démarrage » reçoit du cœur.
//!
//! La page ne connaît que des identifiants ([`EntryId`]) : elle n'envoie
//! jamais un chemin, un nom de valeur ni une commande. Les noms, les éditeurs
//! et les textes qu'elle affiche sont ceux que le cœur a nettoyés.

use std::fmt;

use serde::Serialize;

use crate::startup::{EntryId, Hint, Listing, Mark, Origin, Scope, State, TargetKind};

/// Une entrée de démarrage, telle que la page l'affiche.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupEntry {
    /// Son identifiant : c'est lui que la page renvoie pour la régler.
    pub id: EntryId,
    /// Le nom affiché en premier.
    pub name: String,
    /// Le nom sous lequel elle est inscrite, s'il diffère.
    pub alias: Option<String>,
    /// La société que son fichier déclare (pas une identité vérifiée).
    pub publisher: Option<String>,
    /// Le compte, ou le PC.
    pub scope: Scope,
    /// Une valeur de registre, ou un raccourci du dossier Démarrage.
    pub origin: Origin,
    /// Active, coupée, ou illisible.
    pub state: State,
    /// Date de la coupure lue dans Windows, en millisecondes depuis 1970.
    pub disabled_at: Option<i64>,
    /// Ce que Shinku sait de son histoire.
    pub mark: Option<Mark>,
    /// Ce que sa commande désigne.
    pub target: TargetKind,
    /// Le programme hôte, si sa commande en passe par un.
    pub host: Option<String>,
    /// Une remarque.
    pub hint: Option<Hint>,
    /// L'entrée de Shinku lui-même.
    pub own: bool,
    /// Vrai si Shinku sait la régler.
    pub adjustable: bool,
}

/// Tout ce que la page affiche.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupView {
    /// Les entrées : le compte d'abord, puis le PC, chaque portée triée.
    pub entries: Vec<StartupEntry>,
    /// Les portées dont une place n'a pas pu être lue, ou seulement en partie.
    pub unreadable: Vec<Scope>,
}

impl From<&Listing> for StartupView {
    fn from(listing: &Listing) -> Self {
        let entries = listing
            .entries
            .iter()
            .map(|entry| StartupEntry {
                id: entry.id,
                name: entry.name.clone(),
                alias: entry.alias.clone(),
                publisher: entry.publisher.clone(),
                scope: entry.scope(),
                origin: entry.place.origin(),
                state: entry.state,
                disabled_at: entry.disabled_ms,
                mark: entry.mark,
                target: entry.target,
                host: entry.host.clone(),
                hint: entry.hint,
                own: entry.own,
                adjustable: entry.adjustable,
            })
            .collect();
        let mut unreadable: Vec<Scope> = Vec::new();
        // Une place lue en partie est dite comme une place illisible : la liste
        // n'est pas complète, la page ne doit pas la présenter comme telle.
        for place in listing.unreadable.iter().chain(&listing.incomplete) {
            if !unreadable.contains(&place.scope()) {
                unreadable.push(place.scope());
            }
        }
        // Le compte d'abord, comme les entrées.
        unreadable.sort_by_key(|scope| *scope != Scope::User);
        Self { entries, unreadable }
    }
}

/// Pourquoi une demande de la page « Démarrage » est refusée.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupRefusal {
    /// Aucune entrée ne porte cet identifiant (ou plusieurs).
    UnknownEntry,
    /// Cette entrée ne se règle pas ici.
    NotAdjustable,
    /// Un autre changement est en cours.
    Busy,
    /// Windows n'a pas gardé le changement.
    Failed,
}

impl fmt::Display for StartupRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownEntry => "cette entrée n'existe plus",
            Self::NotAdjustable => "cette entrée ne se règle pas ici",
            Self::Busy => "un autre changement est en cours",
            Self::Failed => "le changement n'a pas pu être fait",
        })
    }
}
