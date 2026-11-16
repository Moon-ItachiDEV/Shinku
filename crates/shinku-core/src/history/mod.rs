//! Le Journal : ce que Shinku a noté de ce qu'il a fait, rassemblé en une seule
//! liste, la plus récente en haut.
//!
//! Il ne fait **rien** : il lit quatre traces que d'autres modules gardent déjà
//! (les grands vides, les entrées de Démarrage coupées, la dernière partie du
//! mode jeu, les désinstallations vues) et les assemble. Aucune ne passe ici
//! avec un chemin ni un identifiant : des noms d'apps, des dates, des totaux.
//!
//! [`assemble`] est une fonction **pure** : elle ne lit aucun fichier. Trois des
//! quatre traces peuvent avoir été écrites par n'importe quel programme du
//! compte : toute date hors de `[1, maintenant + 24 h]` devient `None`, pour
//! qu'un nombre que JavaScript ne tient pas n'arrive jamais à la page et qu'une
//! date du futur ne se colle pas en tête pour toujours.

use serde::Serialize;

use crate::apps::notes::UninstallNote;
use crate::clean::journal::{Run, RunState};
use crate::game::view::LastGame;
use crate::startup::store::CutLine;

/// Nombre de lignes rendues, au plus.
pub const LINES_KEPT: usize = 200;
/// Ce qu'on tolère d'avance sur l'heure d'à présent : une journée.
const FUTURE_SLACK_MS: i64 = 24 * 3600 * 1000;

/// Une ligne du Journal. Aucun champ ne porte un chemin ni un identifiant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum HistoryLine {
    /// Un grand vide.
    Clean {
        /// Début.
        started_ms: Option<i64>,
        /// Fin, si elle a pu être notée.
        finished_ms: Option<i64>,
        /// Où il en est.
        state: RunState,
        /// Place libérée pour de bon, au dernier pointage.
        freed_bytes: u64,
        /// Place occupée par ce qui est parti à la corbeille.
        recoverable_bytes: u64,
        /// Fichiers laissés en place.
        skipped_files: u64,
    },
    /// Une entrée de Démarrage que Shinku a coupée.
    StartupCut {
        /// Date de la coupure.
        at_ms: Option<i64>,
        /// Son nom, tel que Shinku l'a noté.
        name: String,
        /// La première fois qu'on l'a vue active après la coupure.
        back_ms: Option<i64>,
    },
    /// Une partie du mode jeu : la dernière seulement.
    Game {
        /// Début.
        started_ms: Option<i64>,
        /// Fin.
        ended_ms: Option<i64>,
        /// Le nom du jeu.
        name: String,
        /// L'alimentation est revenue, n'a pas pu l'être, ou n'avait pas été changée.
        power_restored: Option<bool>,
        /// Nombre d'apps remises à leur vitesse.
        apps_restored: u32,
        /// Tout a été remis en place.
        complete: bool,
    },
    /// Une app que Shinku a vue désinstallée.
    Uninstall {
        /// L'heure où Shinku a vu l'app partie.
        at_ms: Option<i64>,
        /// Son nom.
        name: String,
        /// Restes mis à la corbeille ensuite.
        leftovers: u32,
        /// Leur poids mesuré (0 si aucun n'a pu l'être).
        leftover_bytes: u64,
    },
}

/// Le Journal tel que la page le reçoit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryView {
    /// Les lignes, la plus récente d'abord ; celles sans date à la fin.
    pub lines: Vec<HistoryLine>,
    /// Il y en avait davantage : les plus anciennes ne sont pas montrées.
    pub more: bool,
}

impl HistoryLine {
    /// La date qui range la ligne : la fin d'un grand vide ou d'une partie, à
    /// défaut son début ; pour les autres, leur date.
    fn sort_date(&self) -> Option<i64> {
        match self {
            Self::Clean { started_ms, finished_ms, .. } => finished_ms.or(*started_ms),
            Self::Game { started_ms, ended_ms, .. } => ended_ms.or(*started_ms),
            Self::StartupCut { at_ms, .. } | Self::Uninstall { at_ms, .. } => *at_ms,
        }
    }

    /// Le genre et le nom, pour départager deux lignes de même date.
    fn tie_break(&self) -> (u8, &str) {
        match self {
            Self::Clean { .. } => (0, ""),
            Self::StartupCut { name, .. } => (1, name),
            Self::Game { name, .. } => (2, name),
            Self::Uninstall { name, .. } => (3, name),
        }
    }
}

/// Une date de fichier, ou `None` si elle n'a pas de sens : avant 1970, nulle,
/// ou plus d'une journée après maintenant.
fn sane(date: i64, now_ms: i64) -> Option<i64> {
    (date >= 1 && date <= now_ms.saturating_add(FUTURE_SLACK_MS)).then_some(date)
}

/// Rassemble les quatre traces en un seul Journal.
///
/// Tri : la date de chaque ligne (voir [`HistoryLine`]), la plus récente
/// d'abord, les lignes sans date à la fin ; à égalité, le genre puis le nom.
/// 200 lignes au plus : `more` dit qu'il y en avait davantage. Un grand vide en
/// cours ou coupé est montré comme les autres.
pub fn assemble(
    now_ms: i64,
    runs: &[Run],
    cuts: &[CutLine],
    last_game: Option<&LastGame>,
    uninstalls: &[UninstallNote],
) -> HistoryView {
    let date = |value: i64| sane(value, now_ms);
    let mut lines: Vec<HistoryLine> = Vec::new();
    for run in runs {
        lines.push(HistoryLine::Clean {
            started_ms: date(run.started_ms),
            finished_ms: run.finished_ms.and_then(date),
            state: run.state,
            freed_bytes: run.freed_bytes,
            recoverable_bytes: run.recoverable_bytes,
            skipped_files: run.skipped_files,
        });
    }
    for cut in cuts {
        lines.push(HistoryLine::StartupCut {
            at_ms: date(cut.at_ms),
            name: cut.name.clone(),
            back_ms: cut.back_ms.and_then(date),
        });
    }
    if let Some(game) = last_game {
        lines.push(HistoryLine::Game {
            started_ms: date(game.started),
            ended_ms: date(game.ended),
            name: game.name.clone(),
            power_restored: game.power_restored,
            apps_restored: game.apps_restored,
            complete: game.complete,
        });
    }
    for note in uninstalls {
        lines.push(HistoryLine::Uninstall {
            at_ms: date(note.at_ms),
            name: note.name.clone(),
            leftovers: note.leftovers,
            leftover_bytes: note.leftover_bytes,
        });
    }
    lines.sort_by(|left, right| {
        // Décroissant ; `None` après toute date.
        right
            .sort_date()
            .cmp(&left.sort_date())
            .then_with(|| left.tie_break().cmp(&right.tie_break()))
    });
    let more = lines.len() > LINES_KEPT;
    lines.truncate(LINES_KEPT);
    HistoryView { lines, more }
}

#[cfg(test)]
mod tests;
