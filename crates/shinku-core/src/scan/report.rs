//! Le rapport d'une analyse : ce que l'écran Résultats affiche.
//!
//! Le cœur garde le rapport. L'interface n'en reçoit que des totaux et des
//! pages : jamais la liste entière d'un disque.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::{FileId, Lot, ModuleId, RiskLevel, Timestamp, Trigger};
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
                // Jamais d'office : la vider est sans retour, et elle garde
                // ce que la personne a jeté comme les copies qu'un grand vide
                // précédent y a envoyées « récupérables ».
                lot: Lot::Optional,
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
            // Des fichiers personnels : jamais cochés d'office. Une photo ou
            // une vidéo en double ne part que si l'utilisateur l'a choisi.
            CategorySummary {
                module: ModuleId::Duplicates,
                lot: Lot::Optional,
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
        Summary {
            complete: self.complete,
            started: self.started,
            finished: self.finished,
            auto_bytes,
            categories,
        }
    }

    /// Une page de la liste des gros fichiers.
    pub fn large_files_page(&self, offset: usize, limit: usize) -> &[LargeFile] {
        page(&self.large_files.files, offset, limit)
    }

    /// Une page de la liste des groupes de doublons.
    pub fn duplicates_page(&self, offset: usize, limit: usize) -> &[DuplicateGroup] {
        page(&self.duplicates.groups, offset, limit)
    }

    /// Le même rapport, où l'utilisateur a choisi l'exemplaire à garder.
    ///
    /// Le choix désigne un exemplaire que ce rapport connaît déjà : le groupe
    /// par son empreinte et sa taille, l'exemplaire par son rang. Aucun chemin
    /// ne vient de l'extérieur. Avec `whole_folder`, le choix vaut pour tous
    /// les groupes qui ont un exemplaire dans le même dossier que celui-là :
    /// un groupe qui y garde déjà un exemplaire ne bouge pas, les autres y
    /// gardent leur plus ancien.
    ///
    /// Chaque groupe garde toujours un exemplaire et un seul. `None` si le
    /// rapport n'est pas complet ou si le choix ne désigne rien.
    pub fn keeping(&self, choice: &KeepChoice) -> Option<Self> {
        if !self.complete || self.duplicates.state != DuplicatesState::Done {
            return None;
        }
        let groups = &self.duplicates.groups;
        let chosen = groups
            .iter()
            .find(|group| group.digest == choice.digest && group.size == choice.size)?
            .files
            .get(choice.index as usize)?;
        let folder = chosen.path.parent()?;

        let mut revised = self.clone();
        for group in &mut revised.duplicates.groups {
            let same_group = group.digest == choice.digest && group.size == choice.size;
            let kept = if same_group {
                Some(choice.index as usize)
            } else if choice.whole_folder {
                let here = |file: &DuplicateFile| file.path.parent() == Some(folder);
                // Un exemplaire déjà gardé dans ce dossier a pu être choisi à
                // la main : il n'est pas remplacé par son voisin.
                if group.files.iter().any(|file| file.keep && here(file)) {
                    None
                } else {
                    group.files.iter().position(here)
                }
            } else {
                None
            };
            if let Some(kept) = kept {
                group.keep_only(kept);
            }
        }
        let duplicates = &mut revised.duplicates;
        duplicates.bytes = duplicates.groups.iter().map(|group| group.reclaimable).sum();
        Some(revised)
    }
}

/// L'exemplaire qu'un utilisateur veut garder dans un groupe de doublons.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeepChoice {
    /// Empreinte du groupe.
    pub digest: String,
    /// Taille de chaque exemplaire du groupe.
    pub size: u64,
    /// Rang de l'exemplaire à garder dans le groupe.
    pub index: u32,
    /// Garder aussi, dans les autres groupes, l'exemplaire rangé dans le
    /// même dossier.
    pub whole_folder: bool,
}

impl DuplicateGroup {
    /// Garde l'exemplaire de rang `kept` et lui seul.
    fn keep_only(&mut self, kept: usize) {
        if kept >= self.files.len() {
            return;
        }
        for (rank, file) in self.files.iter_mut().enumerate() {
            file.keep = rank == kept;
        }
        self.reclaimable =
            self.files.iter().filter(|file| !file.keep).map(|file| file.on_disk).sum();
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
    /// Début de l'analyse. Le rapport partiel et le rapport complet d'une
    /// même analyse portent le même : c'est à lui que l'interface reconnaît
    /// qu'une nouvelle analyse a remplacé la précédente.
    pub started: Timestamp,
    /// Moment où ces résultats ont été établis.
    pub finished: Timestamp,
    /// Total du lot automatique : ce que « Faire le vide » libère si
    /// l'utilisateur ne touche à aucune case.
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
    /// Lot automatique, au choix ou « À vérifier ».
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
    /// Les exemplaires, du plus ancien au plus récent. Un seul porte `keep` :
    /// le premier, ou celui que l'utilisateur a choisi.
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
    /// Identifiant du fichier sur son volume, tel que l'analyse l'a vu. Il
    /// reste dans le cœur : le grand vide s'en sert pour revérifier le
    /// fichier avant d'y toucher.
    #[serde(skip)]
    pub file_id: Option<FileId>,
    /// Numéro de série du volume où se trouve le fichier.
    #[serde(skip)]
    pub volume: u64,
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
