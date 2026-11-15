//! Le rapport d'une analyse : ce que l'écran Résultats affiche.
//!
//! Le cœur garde le rapport. L'interface n'en reçoit que des totaux et des
//! pages : jamais la liste entière d'un disque.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use crate::model::{Lot, ModuleId, RiskLevel, Timestamp, Trigger};
use crate::platform::{RecycleBin, Volume, serialize_display_path};
use crate::rules::JunkCategory;

/// Au-delà de cet âge, des résultats ne servent plus à nettoyer : le disque a
/// pu changer, et une nouvelle analyse précède toute écriture.
pub const RESULTS_MAX_AGE: Duration = Duration::from_secs(10 * 60);

/// Résultat d'une analyse.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Ce qui a déclenché l'analyse.
    pub trigger: Trigger,
    /// Début de l'analyse.
    pub started: Timestamp,
    /// Moment où ce rapport a été établi.
    pub finished: Timestamp,
    /// Faux tant que la recherche de doublons continue : le reste est déjà là.
    pub complete: bool,
    /// Les disques, pour la jauge.
    pub volumes: Vec<Volume>,
    /// Fichiers inutiles.
    pub junk: JunkReport,
    /// Gros fichiers.
    pub large_files: LargeFilesReport,
    /// Doublons.
    pub duplicates: DuplicatesReport,
    /// Corbeille, si elle a pu être mesurée.
    pub recycle_bin: Option<RecycleBin>,
    /// Ce qui n'a pas pu être analysé.
    pub skipped: Skipped,
    /// Chiffres du parcours.
    pub stats: Stats,
}

impl Report {
    /// Vrai si ces résultats sont trop vieux pour nettoyer sans réanalyser.
    pub fn is_stale(&self, now: Timestamp) -> bool {
        now.since(self.finished) > RESULTS_MAX_AGE
    }

    /// Les cinq catégories du grand vide, avec leurs totaux.
    pub fn summary(&self) -> Summary {
        let duplicates_state = match self.duplicates.state {
            DuplicatesState::Done => CategoryState::Ready,
            DuplicatesState::Pending => CategoryState::Pending,
            DuplicatesState::Off => CategoryState::Unavailable,
        };
        let categories = vec![
            // L'ordre est celui du nettoyage : la corbeille d'abord, pour que
            // les doublons qu'on y enverra ensuite restent récupérables.
            CategorySummary {
                module: ModuleId::RecycleBin,
                lot: Lot::Auto,
                risk: RiskLevel::D,
                bytes: self.recycle_bin.map_or(0, |bin| bin.bytes),
                items: self.recycle_bin.map_or(0, |bin| bin.items),
                state: if self.recycle_bin.is_some() {
                    CategoryState::Ready
                } else {
                    CategoryState::Unavailable
                },
            },
            CategorySummary {
                module: ModuleId::JunkFiles,
                lot: Lot::Auto,
                risk: RiskLevel::A,
                bytes: self.junk.bytes,
                items: self.junk.files,
                state: CategoryState::Ready,
            },
            CategorySummary {
                module: ModuleId::Duplicates,
                lot: Lot::Auto,
                risk: RiskLevel::B,
                bytes: self.duplicates.bytes,
                items: self.duplicates.copies,
                state: duplicates_state,
            },
            CategorySummary {
                module: ModuleId::LargeFiles,
                lot: Lot::Review,
                risk: RiskLevel::B,
                bytes: self.large_files.bytes,
                items: self.large_files.count,
                state: CategoryState::Ready,
            },
            // Le module Démarrage n'existe pas encore : sa place est réservée.
            CategorySummary {
                module: ModuleId::Startup,
                lot: Lot::Review,
                risk: RiskLevel::C,
                bytes: 0,
                items: 0,
                state: CategoryState::Unavailable,
            },
        ];
        let auto_bytes = categories
            .iter()
            .filter(|category| category.lot == Lot::Auto)
            .map(|category| category.bytes)
            .sum();
        Summary { complete: self.complete, finished: self.finished, auto_bytes, categories }
    }

    /// Une page de la liste des gros fichiers.
    pub fn large_files_page(&self, offset: usize, limit: usize) -> &[LargeFile] {
        page(&self.large_files.files, offset, limit)
    }

    /// Une page de la liste des groupes de doublons.
    pub fn duplicates_page(&self, offset: usize, limit: usize) -> &[DuplicateGroup] {
        page(&self.duplicates.groups, offset, limit)
    }
}

fn page<T>(items: &[T], offset: usize, limit: usize) -> &[T] {
    let start = offset.min(items.len());
    let end = start.saturating_add(limit).min(items.len());
    &items[start..end]
}

/// Vue d'ensemble du grand vide.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// Faux tant que la recherche de doublons continue.
    pub complete: bool,
    /// Moment où ces résultats ont été établis.
    pub finished: Timestamp,
    /// Total du lot automatique : ce que « Faire le vide » libérerait.
    pub auto_bytes: u64,
    /// Les catégories, dans l'ordre du nettoyage puis de la vérification.
    pub categories: Vec<CategorySummary>,
}

/// Une catégorie du grand vide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySummary {
    /// Module concerné.
    pub module: ModuleId,
    /// Lot automatique ou « À vérifier ».
    pub lot: Lot,
    /// Niveau de suppression.
    pub risk: RiskLevel,
    /// Place récupérable, en octets.
    pub bytes: u64,
    /// Nombre d'éléments.
    pub items: u64,
    /// Où en est cette catégorie.
    pub state: CategoryState,
}

/// État d'une catégorie dans un rapport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CategoryState {
    /// Le chiffre est définitif.
    Ready,
    /// Le calcul continue.
    Pending,
    /// Non mesuré dans cette analyse.
    Unavailable,
}

/// Fichiers inutiles trouvés.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkReport {
    /// Place récupérable, en octets sur le disque.
    pub bytes: u64,
    /// Nombre de fichiers.
    pub files: u64,
    /// Place occupée par ce qui est trop récent pour être nettoyé.
    pub spared_bytes: u64,
    /// Nombre de fichiers trop récents.
    pub spared_files: u64,
    /// Détail par catégorie. Les catégories vides restent listées.
    pub categories: Vec<JunkCategoryReport>,
}

/// Une catégorie de fichiers inutiles.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkCategoryReport {
    /// La catégorie.
    pub category: JunkCategory,
    /// Son nom affiché.
    pub label: &'static str,
    /// Place récupérable.
    pub bytes: u64,
    /// Nombre de fichiers.
    pub files: u64,
    /// Détail par app ou par nature, du plus gros au plus petit.
    pub lines: Vec<JunkLine>,
}

/// Une ligne du détail : une app, ou une nature de fichiers.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkLine {
    /// Nom affiché.
    pub label: &'static str,
    /// Place récupérable.
    pub bytes: u64,
    /// Nombre de fichiers.
    pub files: u64,
    /// Règles qui ont contribué à cette ligne.
    pub rules: Vec<&'static str>,
    /// Apps qu'il vaut mieux fermer avant de nettoyer.
    pub close_first: Vec<&'static str>,
}

/// Gros fichiers trouvés.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFilesReport {
    /// Seuil appliqué, en octets sur le disque.
    pub threshold: u64,
    /// Nombre de fichiers au-dessus du seuil.
    pub count: u64,
    /// Place qu'ils occupent.
    pub bytes: u64,
    /// Les plus gros, par taille décroissante. La liste est bornée.
    pub files: Vec<LargeFile>,
}

/// Un gros fichier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFile {
    /// Chemin du fichier.
    #[serde(serialize_with = "serialize_display_path")]
    pub path: PathBuf,
    /// Taille logique.
    pub size: u64,
    /// Place réellement occupée sur le disque : c'est elle qui est affichée.
    pub on_disk: u64,
    /// Date de dernière modification.
    pub modified: Timestamp,
    /// Dans un dossier synchronisé : le supprimer le supprime aussi dans le cloud.
    pub synced: bool,
}

/// Doublons trouvés.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatesReport {
    /// Où en est la recherche.
    pub state: DuplicatesState,
    /// Nombre de fichiers examinés.
    pub candidates: u64,
    /// Nombre de copies en trop.
    pub copies: u64,
    /// Place que ces copies occupent sur le disque.
    pub bytes: u64,
    /// Fichiers écartés en cours de route (verrouillés, modifiés, illisibles).
    pub skipped_files: u64,
    /// Liens physiques rencontrés : comptés une fois, jamais proposés.
    pub hard_links: u64,
    /// Les groupes, du plus gros gain au plus petit.
    pub groups: Vec<DuplicateGroup>,
}

impl DuplicatesReport {
    /// Rapport vide dans l'état donné.
    pub fn empty(state: DuplicatesState) -> Self {
        Self {
            state,
            candidates: 0,
            copies: 0,
            bytes: 0,
            skipped_files: 0,
            hard_links: 0,
            groups: Vec::new(),
        }
    }
}

/// État de la recherche de doublons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicatesState {
    /// Terminée.
    Done,
    /// En cours : les empreintes se calculent encore.
    Pending,
    /// Non demandée.
    Off,
}

/// Un groupe de fichiers au contenu identique.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    /// Taille de chaque exemplaire.
    pub size: u64,
    /// Empreinte BLAKE3 du contenu, commune à tous les exemplaires : celle
    /// qu'afficherait `b3sum` pour chacun d'eux.
    pub digest: String,
    /// Place récupérable en ne gardant que l'exemplaire conservé.
    pub reclaimable: u64,
    /// Les exemplaires. Le premier est le plus ancien : c'est celui qu'on garde.
    pub files: Vec<DuplicateFile>,
}

/// Un exemplaire dans un groupe de doublons.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFile {
    /// Chemin du fichier.
    #[serde(serialize_with = "serialize_display_path")]
    pub path: PathBuf,
    /// Place occupée sur le disque.
    pub on_disk: u64,
    /// Date de création à cet emplacement.
    pub created: Timestamp,
    /// Date de dernière modification.
    pub modified: Timestamp,
    /// Exemplaire conservé : il n'est jamais proposé à la suppression.
    pub keep: bool,
    /// Dans un dossier synchronisé : le supprimer le supprime aussi dans le cloud.
    pub synced: bool,
}

/// Ce que l'analyse n'a pas pu ou pas voulu lire.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    /// Dossiers interdits au compte : « 3 dossiers non analysés ».
    pub denied_folders: u64,
    /// Quelques-uns de ces dossiers, pour le détail.
    #[serde(serialize_with = "serialize_display_paths")]
    pub denied_examples: Vec<PathBuf>,
    /// Dossiers illisibles pour une autre raison.
    pub unreadable_folders: u64,
    /// Dossiers remplacés par autre chose pendant l'analyse.
    pub replaced_folders: u64,
    /// Emplacements écartés avant même de commencer, et pourquoi.
    pub refused_roots: Vec<RefusedRoot>,
}

fn serialize_display_paths<S: serde::Serializer>(
    paths: &[PathBuf],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(
        paths.iter().map(|path| crate::fs::to_display_path(path).to_string_lossy().into_owned()),
    )
}

/// Un emplacement que le plan d'analyse a refusé.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefusedRoot {
    /// À quoi il devait servir (identifiant de règle, « doublons »…).
    pub purpose: String,
    /// L'emplacement.
    #[serde(serialize_with = "serialize_display_path")]
    pub path: PathBuf,
    /// La raison du refus.
    pub reason: String,
}

/// Chiffres du parcours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// Dossiers lus.
    pub folders: u64,
    /// Fichiers vus.
    pub files: u64,
    /// Durée du parcours, en millisecondes.
    pub walk_ms: u64,
    /// Durée totale de l'analyse, en millisecondes.
    pub total_ms: u64,
}
