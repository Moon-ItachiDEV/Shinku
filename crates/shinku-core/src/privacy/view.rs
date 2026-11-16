//! Ce que la page « Confidentialité » reçoit du cœur, et ce qu'elle peut se
//! faire refuser.
//!
//! La page ne connaît que des identifiants ([`TraceId`]) : elle n'envoie jamais
//! un chemin, un nom de fichier ni un nom de valeur. Elle ne reçoit jamais le
//! **contenu** d'une trace : seulement qu'elle existe, son poids ou son nombre
//! d'éléments.

use std::fmt;

use serde::Serialize;

use crate::privacy::id::TraceId;
use crate::privacy::rules::Browser;

/// Ce que Shinku sait effacer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TraceKind {
    /// L'historique d'un profil de navigateur.
    History,
    /// Fichiers et dossiers récents de Windows.
    Recent,
    /// Ce qui a été tapé dans Windows (Exécuter, barre d'adresse, recherche).
    Typed,
    /// Le presse-papiers.
    Clipboard,
}

/// Ce que Shinku voit d'une trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TraceState {
    /// Elle existe : on peut la cocher.
    Present,
    /// Rien à effacer : pas de case.
    Empty,
    /// Shinku ne sait pas la lire (ou refuse d'y toucher : un lien) : inerte.
    Unreadable,
}

/// Ce qui est arrivé à une trace que la personne avait cochée.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TraceOutcome {
    /// Effacée.
    Erased,
    /// Il n'y avait rien à effacer.
    Nothing,
    /// Une app la tient (ou le navigateur est ouvert) : rien n'est touché.
    InUse,
    /// Ce n'est plus ce que la page avait montré : elle doit regarder de nouveau.
    Changed,
    /// Une partie reste, ou est revenue (le registre seulement).
    Partial,
    /// Pas pu être effacée : rien n'est touché (fichiers).
    Failed,
}

/// Un bloc de navigateur, ou le bloc « Windows ».
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupKind {
    /// Un profil d'un navigateur.
    Browser,
    /// Les traces du compte Windows.
    Windows,
}

/// Une trace, telle que la page l'affiche.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceItem {
    /// Fabriqué par le cœur, opaque pour la page.
    pub id: TraceId,
    /// Ce que c'est.
    pub kind: TraceKind,
    /// Présente, vide ou illisible.
    pub state: TraceState,
    /// Le poids des fichiers, sans rien ouvrir ; sinon `null`.
    pub bytes: Option<u64>,
    /// Le nombre de valeurs ou de raccourcis ; sinon `null`.
    pub count: Option<u32>,
}

/// Un navigateur et son profil, ou le bloc « Windows ».
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyGroup {
    /// Une clé de liste, stable d'une vue à l'autre (la page n'a pas à la lire).
    pub key: String,
    /// Un navigateur, ou Windows.
    pub kind: GroupKind,
    /// Le navigateur ; `null` pour Windows.
    pub app: Option<Browser>,
    /// Le nom du profil lu par le cœur, déjà nettoyé ; `null` s'il n'y en a pas de lisible.
    pub profile: Option<String>,
    /// 0 : `Default`, N : `Profile N`, `null` : le bloc Windows. Ajout de la
    /// conception : la page fabrique « Profil principal » ou « Profil N » sans
    /// lire `key`.
    pub slot: Option<u32>,
    /// Le navigateur est ouvert : ses traces ne s'effacent pas.
    pub running: bool,
    /// Les traces du bloc.
    pub items: Vec<TraceItem>,
}

/// Tout ce que la page affiche.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyView {
    /// Un bloc par navigateur et par profil, puis « Windows ».
    pub groups: Vec<PrivacyGroup>,
    /// Un dossier de profils Firefox existe : une ligne d'aide.
    pub firefox: bool,
    /// Chrome et Edge sont tous deux là : Edge peut réimporter l'historique de Chrome.
    pub edge_imports_chrome: bool,
}

/// Ce qui est arrivé à une trace demandée.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceResult {
    /// L'identifiant que la page a envoyé.
    pub id: String,
    /// Ce qui s'est passé.
    pub outcome: TraceOutcome,
}

/// Ce que rend `erase_traces` : la vue relue, et ce qui est arrivé à chaque trace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErasedTraces {
    /// La vue, relue après l'effacement.
    pub view: PrivacyView,
    /// Un résultat par identifiant demandé (une fois chacun, dans l'ordre).
    pub results: Vec<TraceResult>,
}

/// Pourquoi une demande d'effacement est refusée : rien n'a été touché.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PrivacyRefusal {
    /// Un grand vide, une mise à la corbeille de restes ou un autre effacement est en cours.
    Busy,
    /// Plus de 64 identifiants dans la demande.
    TooMany,
}

impl fmt::Display for PrivacyRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Busy => "un autre geste est en cours",
            Self::TooMany => "trop de traces demandées à la fois",
        })
    }
}
