//! Ce que la page « Désinstallation » reçoit du cœur.
//!
//! La page ne connaît que des identifiants ([`AppId`]) et des clés de restes :
//! elle n'envoie jamais un chemin ni une commande. Les noms, les éditeurs et les
//! chemins qu'elle affiche sont ceux que le cœur a nettoyés. Les champs absents
//! ne sont pas écrits (ils n'existent pas, ils ne valent pas `null`).

use std::fmt;

use serde::Serialize;

use crate::apps::leftovers::{Leftover, LeftoverKind, RemoveResult};
use crate::apps::{App, AppId, Date, Kind, Removal, Scope};

/// Une app installée, telle que la page l'affiche.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRow {
    /// Son identifiant : c'est lui que la page renvoie pour la désinstaller.
    pub id: AppId,
    /// Son nom.
    pub name: String,
    /// Sa version, si elle en déclare une.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Son éditeur (pas une identité vérifiée).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// La taille qu'elle déclare, en octets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// La date d'installation, `AAAA-MM-JJ`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed: Option<Date>,
    /// Le compte, ou le PC.
    pub scope: Scope,
    /// Un indice par le nom : une ressemblance, jamais une certitude.
    pub kind: Kind,
    /// Ce que Shinku propose.
    pub removal: Removal,
}

impl From<&App> for AppRow {
    fn from(app: &App) -> Self {
        Self {
            id: app.id.clone(),
            name: app.name.clone(),
            version: app.version.clone(),
            publisher: app.publisher.clone(),
            bytes: app.bytes,
            installed: app.installed,
            scope: app.scope,
            kind: app.kind,
            removal: app.removal,
        }
    }
}

/// Un reste proposé, tel que la page l'affiche.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverRow {
    /// Sa clé, opaque : c'est tout ce que la page renvoie pour le retirer.
    pub key: u32,
    /// Sa famille.
    pub kind: LeftoverKind,
    /// Son chemin, à montrer et jamais à renvoyer.
    pub path: String,
    /// Sa taille, en octets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// Son nombre de fichiers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<u64>,
}

impl From<&Leftover> for LeftoverRow {
    fn from(leftover: &Leftover) -> Self {
        Self {
            key: leftover.key,
            kind: leftover.kind,
            path: leftover.path.clone(),
            bytes: leftover.bytes,
            files: leftover.files,
        }
    }
}

/// Où en est la désinstallation en cours.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "phase")]
pub enum FollowView {
    /// Rien en cours.
    Idle,
    /// Le désinstalleur est en train d'être lancé (la fenêtre de Windows est
    /// peut-être ouverte).
    Launching {
        /// L'app.
        id: AppId,
        /// Son nom.
        name: String,
    },
    /// Le désinstalleur est lancé, l'inscription est encore là.
    Waiting {
        /// L'app.
        id: AppId,
        /// Son nom.
        name: String,
        /// Depuis combien de secondes.
        seconds: u64,
    },
    /// L'inscription a disparu : on laisse le désinstalleur finir.
    Settling {
        /// L'app.
        id: AppId,
        /// Son nom.
        name: String,
    },
    /// L'app est désinstallée ; ce qu'elle a laissé a été cherché.
    Removed {
        /// L'app.
        id: AppId,
        /// Son nom.
        name: String,
        /// Les restes proposés (aucun n'est coché d'avance).
        leftovers: Vec<LeftoverRow>,
        /// Une app du même nom est encore installée : rien n'est proposé.
        family: bool,
        /// Faux si les inscriptions n'ont pas pu être relues en entier : rien
        /// n'est proposé.
        complete: bool,
    },
}

/// Ce que Shinku dit une seule fois.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// L'installeur de Windows est sorti, et l'app est restée : la personne a
    /// annulé.
    Kept,
}

/// Tout ce que la page affiche.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppsView {
    /// Les apps installées, triées par nom (sans casse ni accents).
    pub apps: Vec<AppRow>,
    /// Faux si une source n'a pas pu être lue en entier.
    pub complete: bool,
    /// La désinstallation en cours.
    pub follow: FollowView,
    /// Dit une seule fois.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
}

/// Ce que rend `remove_leftovers`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveLeftovers {
    /// La vue d'après.
    pub view: AppsView,
    /// Ce qui est arrivé à chaque reste demandé.
    pub results: Vec<RemoveResult>,
    /// Les octets mis à la corbeille.
    pub bytes: u64,
}

/// Pourquoi une demande de la page « Désinstallation » est refusée. Rien n'a
/// été lancé.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UninstallRefusal {
    /// Un autre geste est en cours : une désinstallation, une mise à la
    /// corbeille, un grand vide.
    Busy,
    /// L'app n'est plus installée : la page recharge la liste.
    Vanished,
    /// L'inscription ne se lit pas.
    Unreadable,
    /// Shinku ne propose pas de retirer cette app.
    NotOffered,
    /// Le désinstalleur est introuvable ou inutilisable.
    Broken,
    /// La personne a dit non à la fenêtre de Windows.
    Cancelled,
    /// Windows n'a pas pu lancer le désinstalleur.
    Failed,
}

impl fmt::Display for UninstallRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Busy => "un autre geste est en cours",
            Self::Vanished => "cette app n'est plus installée",
            Self::Unreadable => "l'inscription de cette app ne se lit pas",
            Self::NotOffered => "Shinku ne propose pas de retirer cette app",
            Self::Broken => "le désinstalleur est introuvable",
            Self::Cancelled => "la demande a été refusée dans la fenêtre de Windows",
            Self::Failed => "Windows n'a pas pu lancer le désinstalleur",
        })
    }
}
