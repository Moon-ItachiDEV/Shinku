//! Ce que la page « Mode jeu » reçoit du cœur, et les demandes qu'elle lui fait.
//!
//! La page ne fournit jamais un chemin ni un nom libre : elle envoie des
//! verbes bornés ([`GameOption`], [`GameStatus`], [`WindowsPage`]), un numéro
//! pris dans la dernière liste que le cœur lui a rendue, ou un texte que le
//! cœur ne fait que **comparer** à ce qu'il connaît déjà.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::game::PowerOutcome;
use crate::game::detect::Source;
use crate::platform::agent::AgentState;

/// Un interrupteur du mode jeu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GameOption {
    /// Le mode jeu automatique.
    Auto,
    /// L'alimentation en « Performances élevées ».
    Power,
    /// Les apps d'arrière-plan en veilleuse.
    QuietApps,
}

/// Ce que la personne dit d'un jeu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GameStatus {
    /// « Ce n'est pas un jeu » : Shinku ne le traite plus jamais comme tel.
    NotGame,
    /// Oublié : ni ajouté, ni écarté. La reconnaissance automatique reprend.
    Forgotten,
}

/// Ce que l'alimentation permet sur ce PC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerSupport {
    /// Le mode « Performances élevées » existe et convient.
    Available,
    /// Ce PC n'a pas le mode « Performances élevées ».
    Unavailable,
    /// Ce processeur est mieux servi par le mode normal de Windows.
    Unsuited,
}

/// Pourquoi une demande du mode jeu est refusée. Rien n'a changé.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GameRefusal {
    /// Cette app n'est pas connue (elle n'est plus ouverte, ou pas dans la liste).
    UnknownApp,
    /// Ce jeu n'est pas dans les listes.
    UnknownGame,
    /// Ce programme n'est jamais un jeu (navigateur, lanceur, outil…).
    NotAGame,
    /// Cette app ne peut pas être mise en veilleuse.
    ProtectedApp,
    /// La liste est pleine.
    ListFull,
}

impl fmt::Display for GameRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownApp => "cette app n'est pas connue",
            Self::UnknownGame => "ce jeu n'est pas dans la liste",
            Self::NotAGame => "ce programme n'est jamais un jeu",
            Self::ProtectedApp => "cette app ne peut pas être mise en veilleuse",
            Self::ListFull => "la liste est pleine",
        })
    }
}

/// La partie en cours.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSession {
    /// Le nom du jeu.
    pub name: String,
    /// Le chemin de son exécutable.
    pub path: String,
    /// Début, en millisecondes depuis 1970.
    pub since: i64,
    /// Comment il a été reconnu.
    pub source: Source,
    /// Ce qui a été fait de l'alimentation.
    pub power: PowerOutcome,
    /// Nombre d'apps distinctes en veilleuse en ce moment.
    pub quieted_apps: u32,
    /// La personne a tout remis en place pour cette partie.
    pub released: bool,
    /// Quelque chose n'a pas encore pu être remis en place.
    pub restore_pending: bool,
}

/// Le bilan de la dernière partie.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastGame {
    /// Le nom du jeu.
    pub name: String,
    /// Début, en millisecondes depuis 1970.
    pub started: i64,
    /// Fin, en millisecondes depuis 1970.
    pub ended: i64,
    /// L'alimentation est revenue (`true`), n'a pas pu l'être (`false`), ou
    /// n'avait pas été changée (`null`).
    pub power_restored: Option<bool>,
    /// Nombre d'apps remises à leur vitesse.
    pub apps_restored: u32,
    /// Tout a été remis en place.
    pub complete: bool,
}

/// Une app qu'on peut mettre en veilleuse.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuietApp {
    /// Le nom de son exécutable, en minuscules.
    pub exe: String,
    /// Son nom affiché.
    pub label: String,
    /// Elle sera mise en veilleuse.
    pub on: bool,
    /// Elle tourne en ce moment.
    pub running: bool,
    /// Elle fait partie de la liste de départ de Shinku.
    pub built_in: bool,
}

/// Un jeu connu.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownGame {
    /// Le chemin de son exécutable.
    pub path: String,
    /// Son nom.
    pub name: String,
    /// Ajouté par la personne, ou reconnu tout seul.
    pub source: Source,
    /// La dernière partie, en millisecondes depuis 1970, si elle est connue.
    pub last_played: Option<i64>,
}

/// Un programme que la personne a écarté.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotGame {
    /// Le chemin de son exécutable.
    pub path: String,
    /// Son nom.
    pub name: String,
}

/// Tout ce que la page « Mode jeu » affiche.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    /// Le mode jeu automatique est allumé.
    pub auto: bool,
    /// Alimentation en « Performances élevées » pendant la partie.
    pub power: bool,
    /// Apps d'arrière-plan en veilleuse pendant la partie.
    pub quiet_apps: bool,
    /// L'agent.
    pub agent: AgentState,
    /// La personne a désactivé Shinku dans les apps de démarrage de Windows.
    pub startup_disabled: bool,
    /// La partie en cours.
    pub session: Option<GameSession>,
    /// La dernière partie.
    pub last: Option<LastGame>,
    /// Ce que l'alimentation permet sur ce PC.
    pub power_support: PowerSupport,
    /// Le PC fonctionne sur batterie.
    pub on_battery: bool,
    /// Les apps qu'on peut mettre en veilleuse : intégrées, puis ajoutées.
    pub apps: Vec<QuietApp>,
    /// Les jeux : ajoutés (le dernier ajouté d'abord), puis reconnus (le plus
    /// récent d'abord).
    pub games: Vec<KnownGame>,
    /// Les programmes écartés.
    pub not_games: Vec<NotGame>,
}

/// Une app ouverte, telle que la page la propose dans un sélecteur. Le numéro
/// ne vaut que pour la dernière liste rendue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApp {
    /// Numéro dans la dernière liste.
    pub id: u32,
    /// Le nom de son exécutable.
    pub name: String,
    /// Le chemin de son exécutable.
    pub path: String,
}
