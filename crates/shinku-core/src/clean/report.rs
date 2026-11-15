//! Le bilan d'un grand vide : ce que l'écran « Terminé » affiche.
//!
//! Deux chiffres ne se mélangent jamais : ce qui est libéré pour de bon
//! (corbeille vidée, fichiers inutiles) et ce qui attend dans la corbeille,
//! encore récupérable et pas encore libéré.

use std::path::PathBuf;

use serde::Serialize;

use crate::model::Timestamp;
use crate::platform::serialize_display_path;

/// Comment un grand vide s'est terminé.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanOutcome {
    /// Le grand vide est allé au bout de ce qu'il pouvait faire. Chaque
    /// étape dit ce qu'elle est devenue, et des fichiers ont pu être laissés.
    Completed,
    /// L'utilisateur a arrêté le grand vide : le bilan est partiel.
    Cancelled,
}

/// Ce qu'est devenue une étape du grand vide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepState {
    /// L'utilisateur ne l'avait pas cochée.
    #[default]
    NotRequested,
    /// Menée jusqu'au bout.
    Done,
    /// Arrêtée en route, ou jamais commencée parce que le grand vide s'est
    /// arrêté avant. Ce qu'elle avait déjà fait reste fait.
    Stopped,
    /// Elle n'a pas pu se faire, ou pas en entier. Ses chiffres disent ce qui
    /// est tout de même parti.
    Failed,
}

/// Fichiers que le grand vide a laissés en place, par raison.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedFiles {
    /// Une app s'en sert.
    pub in_use: u64,
    /// Ils ont changé depuis l'analyse.
    pub changed: u64,
    /// Windows refuse l'accès.
    pub denied: u64,
    /// La corbeille ne peut pas les recevoir : Shinku ne les supprime pas
    /// définitivement à la place.
    pub unrecyclable: u64,
    /// Autre raison.
    pub other: u64,
}

/// Pourquoi un fichier a été laissé.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Left {
    InUse,
    Changed,
    Denied,
    Unrecyclable,
    Other,
}

impl SkippedFiles {
    /// Nombre total de fichiers laissés.
    pub fn total(&self) -> u64 {
        self.in_use + self.changed + self.denied + self.unrecyclable + self.other
    }

    pub(crate) fn add(&mut self, reason: Left, files: u64) {
        let counter = match reason {
            Left::InUse => &mut self.in_use,
            Left::Changed => &mut self.changed,
            Left::Denied => &mut self.denied,
            Left::Unrecyclable => &mut self.unrecyclable,
            Left::Other => &mut self.other,
        };
        *counter += files;
    }

    pub(crate) fn merge(&mut self, other: Self) {
        self.in_use += other.in_use;
        self.changed += other.changed;
        self.denied += other.denied;
        self.unrecyclable += other.unrecyclable;
        self.other += other.other;
    }
}

/// L'étape « Corbeille ».
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinStep {
    /// Ce qu'elle est devenue.
    pub state: StepState,
    /// Place libérée en la vidant.
    pub bytes: u64,
    /// Nombre d'éléments qui en sont sortis.
    pub items: u64,
}

/// Une app restée ouverte : ses fichiers inutiles n'ont pas été touchés.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftOpen {
    /// Nom affiché de l'app.
    pub label: &'static str,
    /// Place qu'elle aurait rendue.
    pub bytes: u64,
}

/// L'étape « Fichiers inutiles ».
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkStep {
    /// Ce qu'elle est devenue.
    pub state: StepState,
    /// Place occupée par les fichiers supprimés.
    pub bytes: u64,
    /// Nombre de fichiers supprimés.
    pub files: u64,
    /// Apps restées ouvertes pendant le grand vide.
    pub left_open: Vec<LeftOpen>,
}

/// Pourquoi l'étape « Doublons » s'est arrêtée d'elle-même.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    /// Shinku n'a pas retrouvé un fichier après l'avoir envoyé à la corbeille.
    NotInRecycleBin,
    /// L'exemplaire gardé a changé pendant qu'une copie partait à la
    /// corbeille. La copie y est toujours.
    KeptCopyChanged,
}

/// L'arrêt de sécurité de l'étape « Doublons ».
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatesStop {
    /// La raison.
    pub reason: StopReason,
    /// La copie concernée, à son emplacement d'origine.
    #[serde(serialize_with = "serialize_display_path")]
    pub file: PathBuf,
}

/// L'étape « Doublons ».
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatesStep {
    /// Ce qu'elle est devenue.
    pub state: StepState,
    /// Place occupée par les copies envoyées à la corbeille.
    pub bytes: u64,
    /// Nombre de copies envoyées à la corbeille.
    pub copies: u64,
    /// L'arrêt de sécurité, s'il y en a eu un.
    pub stop: Option<DuplicatesStop>,
}

/// Le bilan d'un grand vide.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanReport {
    /// Début du grand vide.
    pub started: Timestamp,
    /// Fin du grand vide.
    pub finished: Timestamp,
    /// Comment il s'est terminé.
    pub outcome: CleanOutcome,
    /// Place libérée pour de bon : corbeille vidée et fichiers inutiles.
    /// Quand le disque a pu être mesuré avant et après, ce chiffre ne dépasse
    /// pas ce qu'il a réellement gagné.
    pub freed_bytes: u64,
    /// Place occupée par ce qui attend dans la corbeille : récupérable, et pas
    /// encore libérée. Inférieure à ce qui y a été envoyé si la corbeille,
    /// pleine, a déjà abandonné une partie de son contenu.
    pub recoverable_bytes: u64,
    /// La corbeille.
    pub recycle_bin: BinStep,
    /// Les fichiers inutiles.
    pub junk: JunkStep,
    /// Les doublons.
    pub duplicates: DuplicatesStep,
    /// Les fichiers laissés en place.
    pub skipped: SkippedFiles,
    /// Durée, en millisecondes.
    pub duration_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipped_files_add_up() {
        let mut skipped = SkippedFiles::default();
        skipped.add(Left::InUse, 3);
        skipped.add(Left::Changed, 1);
        skipped.add(Left::Unrecyclable, 2);
        let mut all = SkippedFiles { denied: 4, other: 1, ..SkippedFiles::default() };
        all.merge(skipped);
        assert_eq!(all.total(), 11);
        assert_eq!(
            (all.in_use, all.changed, all.denied, all.unrecyclable, all.other),
            (3, 1, 4, 2, 1)
        );
    }
}
