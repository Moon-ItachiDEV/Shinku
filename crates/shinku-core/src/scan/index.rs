//! L'indexeur : un seul parcours du disque pour tous les modules.
//!
//! À chaque dossier, un petit contexte dit ce qui s'y applique : on est dans
//! le territoire d'une règle de fichiers inutiles, on cherche des doublons, on
//! relève les gros fichiers, on est sous un dossier synchronisé. Le contexte
//! d'un sous-dossier se calcule à partir de celui de son parent et de l'arbre
//! des emplacements ([`super::plan`]).
//!
//! Rien n'est lu ici à part les listes de dossiers. Un fichier inutile est
//! compté, un gros fichier est relevé, un candidat aux doublons est noté :
//! les contenus ne sont ouverts que plus tard, et seulement pour les doublons.
//!
//! Le grand vide refait ce même parcours avant de supprimer, en demandant en
//! plus la liste de ce qu'il a compté ([`Index::recording`]) : ce qui part est
//! décidé par le code qui a produit les chiffres affichés, pas par une copie.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use super::Progress;
use super::plan::{Mark, Plan};
use super::report::LargeFile;
use crate::fs::{
    EntryFlags, EntryKind, EntryMeta, Expected, ReadDirError, fold_name, same_file_id,
};
use crate::model::{FileId, Timestamp};
use crate::rules::Target;
use crate::scope::{NodeId, ScopeTree, glob_match};
use crate::walk::{Child, Dir, Visitor};

/// On cherche des doublons dans ce dossier.
const DUPS: u8 = 1 << 0;
/// On relève les gros fichiers dans ce dossier.
const LARGE: u8 = 1 << 1;
/// Ce dossier est synchronisé avec un service en ligne.
const SYNC: u8 = 1 << 2;
/// Ce dossier est lui-même une racine de recherche de doublons.
const DUP_ROOT: u8 = 1 << 3;
/// Ce qui se transmet d'un dossier à ses sous-dossiers.
const INHERITED: u8 = DUPS | LARGE | SYNC;

/// Nombre de gros fichiers gardés pour l'affichage.
pub(crate) const LARGE_FILES_KEPT: usize = 500;
/// Nombre de dossiers interdits donnés en exemple.
const DENIED_EXAMPLES: usize = 5;

/// Dossiers gérés par un outil : y chercher des doublons casserait un projet.
const TOOL_FOLDERS: [&str; 6] =
    ["node_modules", "__pycache__", "site-packages", "venv", "$recycle.bin", "bower_components"];
/// Leur présence fait d'un dossier un projet suivi par un gestionnaire de versions.
const PROJECT_MARKERS: [&[u8]; 3] = [b".git", b".svn", b".hg"];

/// Position dans le territoire d'une règle de fichiers inutiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JunkCtx {
    /// Indice de la règle dans le plan.
    rule: u16,
    /// Élément de premier niveau auquel ce dossier appartient, pour les
    /// règles qui jugent en bloc. 0 : aucun.
    unit: u32,
}

/// Ce qui s'applique à un dossier.
#[derive(Clone, Debug, Default)]
pub(crate) struct Ctx {
    /// Positions dans l'arbre des emplacements, tant que le chemin en suit un.
    nodes: Vec<NodeId>,
    junk: Option<JunkCtx>,
    flags: u8,
}

impl Ctx {
    /// Vrai s'il n'y a rien à faire dans ce dossier ni plus bas.
    fn is_idle(&self) -> bool {
        self.nodes.is_empty() && self.junk.is_none() && self.flags & (DUPS | LARGE) == 0
    }
}

/// Totaux d'une règle de fichiers inutiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RuleTotals {
    pub bytes: u64,
    pub files: u64,
    pub spared_bytes: u64,
    pub spared_files: u64,
}

/// Totaux d'un élément jugé en bloc : il n'est nettoyé que si rien n'y est
/// récent, et s'il peut partir en entier.
#[derive(Clone, Copy, Debug)]
struct UnitTotals {
    rule: u16,
    bytes: u64,
    files: u64,
    /// Date la plus récente rencontrée dans l'élément, dossiers compris.
    newest: Timestamp,
    /// L'élément contient quelque chose que Shinku ne supprimera pas (dossier
    /// exclu ou illisible, fichier géré par un service de synchronisation…).
    /// En retirer le reste n'en laisserait qu'un morceau : il est épargné.
    kept_whole: bool,
}

impl UnitTotals {
    fn new(rule: u16) -> Self {
        Self {
            rule,
            bytes: 0,
            files: 0,
            newest: Timestamp::from_ticks(i64::MIN),
            kept_whole: false,
        }
    }
}

/// Un fichier (ou un lien) que le parcours a compté « à nettoyer ».
#[derive(Clone, Debug)]
pub(crate) struct SweptFile {
    /// Indice de la règle dans le plan.
    pub rule: u16,
    /// Élément jugé en bloc auquel il appartient. 0 : aucun, il part seul.
    pub unit: u32,
    pub path: PathBuf,
    /// Ce qu'il faudra retrouver sur le fichier ouvert pour le supprimer.
    pub expected: Expected,
    /// Place qu'il occupe sur le disque.
    pub bytes: u64,
}

/// Un dossier d'un élément jugé en bloc : il part avec lui, une fois vide.
#[derive(Clone, Debug)]
pub(crate) struct SweptDir {
    /// Indice de la règle dans le plan.
    pub rule: u16,
    pub unit: u32,
    pub path: PathBuf,
    pub expected: Expected,
    /// Profondeur sous le point de départ du parcours : les plus profonds
    /// se retirent d'abord.
    pub depth: u32,
}

/// Un fichier dont on comparera peut-être le contenu.
#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub path: PathBuf,
    pub size: u64,
    pub on_disk: u64,
    pub created: Timestamp,
    pub modified: Timestamp,
    pub file_id: Option<FileId>,
    pub volume: u64,
    pub synced: bool,
}

/// Les plus gros fichiers vus, en nombre borné.
#[derive(Debug, Default)]
pub(crate) struct LargeTop {
    heap: BinaryHeap<Reverse<Ranked>>,
    pub count: u64,
    pub bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct Ranked(LargeFile);

impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // À taille égale, l'ordre des chemins rend le résultat reproductible.
        self.0.on_disk.cmp(&other.0.on_disk).then_with(|| other.0.path.cmp(&self.0.path))
    }
}

impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl LargeTop {
    /// Vrai si un fichier de cette taille entrerait dans la liste.
    fn would_keep(&self, on_disk: u64) -> bool {
        self.heap.len() < LARGE_FILES_KEPT
            || self.heap.peek().is_some_and(|smallest| on_disk >= smallest.0.0.on_disk)
    }

    fn count(&mut self, on_disk: u64) {
        self.count += 1;
        self.bytes = self.bytes.saturating_add(on_disk);
    }

    fn keep(&mut self, file: LargeFile) {
        self.heap.push(Reverse(Ranked(file)));
        if self.heap.len() > LARGE_FILES_KEPT {
            self.heap.pop();
        }
    }

    fn merge(&mut self, other: Self) {
        self.count += other.count;
        self.bytes = self.bytes.saturating_add(other.bytes);
        for Reverse(Ranked(file)) in other.heap {
            self.keep(file);
        }
    }

    /// Les fichiers gardés, du plus gros au plus petit.
    pub(crate) fn into_sorted(self) -> Vec<LargeFile> {
        let mut files: Vec<Ranked> = self.heap.into_iter().map(|Reverse(ranked)| ranked).collect();
        files.sort_by(|a, b| b.cmp(a));
        files.into_iter().map(|Ranked(file)| file).collect()
    }
}

/// Ce qu'un fil d'exécution a relevé.
#[derive(Debug, Default)]
pub(crate) struct Sink {
    junk: Vec<RuleTotals>,
    units: HashMap<u32, UnitTotals>,
    large: LargeTop,
    candidates: Vec<Candidate>,
    folders: u64,
    files: u64,
    denied: u64,
    denied_examples: Vec<PathBuf>,
    unreadable: u64,
    replaced: u64,
    /// Ce qui a été compté « à nettoyer », si le parcours le relève.
    swept_files: Vec<SweptFile>,
    swept_dirs: Vec<SweptDir>,
    // État du dossier en cours de lecture.
    /// Candidats aux doublons du dossier, en attente de savoir s'il est un projet.
    pending: Vec<Candidate>,
    /// Le dossier contient la marque d'un projet ou d'une app.
    managed: bool,
    dir_files: u64,
    dir_bytes: u64,
    /// Tampon pour avancer dans l'arbre des emplacements.
    step: Vec<NodeId>,
}

/// Résultat du parcours, tous fils confondus.
#[derive(Debug, Default)]
pub(crate) struct Indexed {
    /// Totaux par règle, dans l'ordre du plan.
    pub junk: Vec<RuleTotals>,
    pub large: LargeTop,
    pub candidates: Vec<Candidate>,
    pub folders: u64,
    pub files: u64,
    pub denied: u64,
    pub denied_examples: Vec<PathBuf>,
    pub unreadable: u64,
    pub replaced: u64,
    /// Ce que le parcours a relevé pour le grand vide. Un fichier qui
    /// appartient à un élément jugé en bloc ne part que si cet élément est
    /// dans `clean_units`.
    pub swept_files: Vec<SweptFile>,
    pub swept_dirs: Vec<SweptDir>,
    /// Les éléments jugés en bloc qui sont à nettoyer.
    pub clean_units: HashSet<u32>,
}

impl Indexed {
    pub(crate) fn new(plan: &Plan) -> Self {
        Self { junk: vec![RuleTotals::default(); plan.rules.len()], ..Self::default() }
    }

    /// Ajoute ce que les fils d'un parcours ont relevé.
    pub(crate) fn absorb(&mut self, plan: &Plan, sinks: Vec<Sink>) {
        let mut units: HashMap<u32, UnitTotals> = HashMap::new();
        for sink in sinks {
            for (total, part) in self.junk.iter_mut().zip(&sink.junk) {
                total.bytes += part.bytes;
                total.files += part.files;
                total.spared_bytes += part.spared_bytes;
                total.spared_files += part.spared_files;
            }
            for (id, part) in sink.units {
                let unit = units.entry(id).or_insert(UnitTotals::new(part.rule));
                unit.bytes += part.bytes;
                unit.files += part.files;
                unit.newest = unit.newest.max(part.newest);
                unit.kept_whole |= part.kept_whole;
            }
            self.large.merge(sink.large);
            self.candidates.extend(sink.candidates);
            self.folders += sink.folders;
            self.files += sink.files;
            self.denied += sink.denied;
            for example in sink.denied_examples {
                if self.denied_examples.len() < DENIED_EXAMPLES {
                    self.denied_examples.push(example);
                }
            }
            self.unreadable += sink.unreadable;
            self.replaced += sink.replaced;
            self.swept_files.extend(sink.swept_files);
            self.swept_dirs.extend(sink.swept_dirs);
        }
        // Un élément jugé en bloc n'est compté « à nettoyer » que si tout ce
        // qu'il contient est plus vieux que l'âge minimal de sa règle, et
        // s'il peut partir en entier.
        for (id, unit) in units {
            let rule = plan.rules[unit.rule as usize];
            let total = &mut self.junk[unit.rule as usize];
            if !unit.kept_whole && unit.newest <= rule.cutoff {
                total.bytes += unit.bytes;
                total.files += unit.files;
                self.clean_units.insert(id);
            } else {
                total.spared_bytes += unit.bytes;
                total.spared_files += unit.files;
            }
        }
    }
}

/// Le visiteur du parcours.
pub(crate) struct Index<'a> {
    plan: &'a Plan,
    progress: &'a Progress,
    /// Numéro de série du volume parcouru.
    volume: u64,
    /// Compteur des éléments jugés en bloc, commun à tous les parcours d'une analyse.
    units: &'a AtomicU32,
    /// Relever ce qui est compté « à nettoyer », pour le grand vide.
    record: bool,
}

/// Instant qui date un élément : sa création ou sa modification, au plus
/// récent. Un fichier sorti d'une archive garde une vieille date de
/// modification alors qu'il vient d'arriver : sa création le trahit.
fn stamp(meta: &EntryMeta) -> Timestamp {
    meta.created.max(meta.modified)
}

fn is_hidden(meta: &EntryMeta) -> bool {
    meta.flags.contains(EntryFlags::HIDDEN) || meta.flags.contains(EntryFlags::SYSTEM)
}

/// Vrai pour ce qu'un service de synchronisation gère : le supprimer ici le
/// supprimerait aussi en ligne. Ce n'est jamais un fichier inutile.
fn is_cloud_managed(meta: &EntryMeta) -> bool {
    meta.flags.contains(EntryFlags::CLOUD) || meta.flags.contains(EntryFlags::ONLINE_ONLY)
}

impl Sink {
    /// Les totaux de l'élément jugé en bloc où se trouve `inside`.
    fn unit(&mut self, inside: JunkCtx) -> &mut UnitTotals {
        self.units.entry(inside.unit).or_insert(UnitTotals::new(inside.rule))
    }

    /// L'élément jugé en bloc où se trouve `inside`, s'il y en a un, contient
    /// quelque chose qui restera : il est épargné en entier.
    fn keep_whole(&mut self, inside: Option<JunkCtx>) {
        if let Some(inside) = inside.filter(|inside| inside.unit != 0) {
            self.unit(inside).kept_whole = true;
        }
    }
}

fn ends_with_ignoring_case(name: &OsStr, suffix: &[u8]) -> bool {
    let bytes = name.as_encoded_bytes();
    bytes.len() > suffix.len() && bytes[bytes.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

fn is_project_marker(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    PROJECT_MARKERS.iter().any(|marker| bytes.eq_ignore_ascii_case(marker))
}

impl<'a> Index<'a> {
    pub(crate) fn new(
        plan: &'a Plan,
        progress: &'a Progress,
        volume: u64,
        units: &'a AtomicU32,
    ) -> Self {
        Self { plan, progress, volume, units, record: false }
    }

    /// Le même parcours, qui relève en plus ce qu'il compte « à nettoyer ».
    pub(crate) fn recording(mut self) -> Self {
        self.record = true;
        self
    }

    /// Contexte d'un point de départ, comme si on était descendu jusqu'à lui
    /// depuis la racine du disque. `None` si l'analyse n'a rien à y faire :
    /// il se trouve, par exemple, dans un dossier exclu.
    pub(crate) fn root_ctx(&self, components: &[String]) -> Option<Ctx> {
        // Sous Windows, un chemin commence par son disque et la racine de
        // l'arbre ne désigne rien. Ailleurs, elle désigne `/` : ce qui la
        // marque s'applique dès le départ.
        let mut flags = 0;
        for mark in self.plan.tree.marks(ScopeTree::<Mark>::ROOT) {
            match *mark {
                Mark::Excluded | Mark::Forbidden => return None,
                Mark::Keep | Mark::Junk(_) => {}
                Mark::Dups => flags |= DUPS | DUP_ROOT,
                Mark::Large => flags |= LARGE,
                Mark::Sync => flags |= SYNC,
            }
        }
        let mut ctx = Ctx { nodes: vec![ScopeTree::<Mark>::ROOT], junk: None, flags };
        let mut step = Vec::new();
        for (index, component) in components.iter().enumerate() {
            let last = index + 1 == components.len();
            let (next, needs_unit) =
                self.descend(&ctx, component, false, false, None, &mut step)?;
            // Un point de départ n'est jamais à l'intérieur d'une règle.
            debug_assert!(!needs_unit);
            // En chemin vers le point de départ, on ne garde que la position
            // dans l'arbre : rien n'est analysé au-dessus de lui.
            ctx = if last || !next.nodes.is_empty() { next } else { return None };
        }
        (!ctx.is_idle()).then_some(ctx)
    }

    /// Calcule le contexte d'un sous-dossier. `None` : on n'y va pas.
    /// Le booléen demande un nouvel élément « jugé en bloc » pour ce dossier.
    fn descend(
        &self,
        parent: &Ctx,
        folded: &str,
        hidden: bool,
        cloud: bool,
        file_id: Option<FileId>,
        step: &mut Vec<NodeId>,
    ) -> Option<(Ctx, bool)> {
        let plan = self.plan;
        step.clear();
        if !parent.nodes.is_empty() {
            plan.tree.step(&parent.nodes, folded, step);
        }

        let mut junk_root: Option<u16> = None;
        let mut flags = parent.flags & INHERITED;
        let mut explicit = 0u8;
        let mut kept = false;
        for &node in step.iter() {
            kept |= plan.kept_below[node as usize];
            for mark in plan.tree.marks(node) {
                match *mark {
                    Mark::Excluded | Mark::Forbidden => return None,
                    Mark::Keep => {}
                    // Si deux règles désignent le même dossier, la première l'emporte.
                    Mark::Junk(rule) => {
                        junk_root = Some(junk_root.map_or(rule, |known| known.min(rule)));
                    }
                    Mark::Dups => explicit |= DUPS | DUP_ROOT,
                    Mark::Large => explicit |= LARGE,
                    Mark::Sync => explicit |= SYNC,
                }
            }
        }
        // Un dossier exclu par l'utilisateur, atteint par un autre chemin.
        if let Some(id) = file_id {
            let excluded = plan
                .excluded
                .iter()
                .any(|known| known.volume == self.volume && same_file_id(known.file_id, id));
            if excluded {
                return None;
            }
        }

        // Ce que l'Explorateur ne montre pas n'est pas proposé à la suppression.
        if hidden {
            flags &= !(DUPS | LARGE);
        }
        let tool_folder = || TOOL_FOLDERS.iter().any(|name| folded.eq_ignore_ascii_case(name));
        if flags & DUPS != 0 && (folded.starts_with('.') || tool_folder()) {
            flags &= !DUPS;
        }
        if cloud {
            flags |= SYNC;
        }
        // Un emplacement demandé explicitement l'emporte sur ces déductions.
        flags |= explicit;

        let mut needs_unit = false;
        let junk = match parent.junk {
            Some(inside) => {
                let rule = plan.rules[inside.rule as usize].rule;
                match rule.target {
                    // Dans le territoire d'une règle, un dossier qui contient
                    // un endroit protégé ou exclu est laissé tel quel, avec
                    // tout ce qu'il contient : on ne nettoie pas autour de ce
                    // que l'utilisateur a demandé de ne jamais toucher.
                    _ if kept => None,
                    Target::FilesHere(_) => None,
                    Target::Everything => {
                        needs_unit = inside.unit == 0 && rule.whole_entries;
                        Some(inside)
                    }
                }
            }
            // Le dossier que désigne une règle : elle s'y applique, même si un
            // dossier exclu se trouve plus bas. C'est ce dernier, et les
            // dossiers qui y mènent, qui seront laissés.
            None => junk_root.map(|rule| JunkCtx { rule, unit: 0 }),
        };
        if junk.is_some() {
            // Le territoire d'une règle n'est analysé que par elle.
            flags &= !(DUPS | LARGE | DUP_ROOT);
        }

        step.retain(|&node| plan.tree.has_children(node));
        let ctx = Ctx { nodes: step.clone(), junk, flags };
        (!ctx.is_idle()).then_some((ctx, needs_unit))
    }

    fn junk_entry(
        &self,
        sink: &mut Sink,
        parent: &Dir<'_, Ctx>,
        inside: JunkCtx,
        name: &OsStr,
        meta: &EntryMeta,
    ) {
        let planned = self.plan.rules[inside.rule as usize];
        let rule = planned.rule;
        match meta.kind {
            // Un lien est supprimé en tant que lien, sans toucher à sa cible.
            EntryKind::File | EntryKind::Link => {}
            // Un dossier où le parcours n'entre pas, ou autre chose qu'un
            // fichier : il restera en place.
            EntryKind::Dir | EntryKind::Other => {
                sink.keep_whole(Some(inside));
                return;
            }
        }
        if let Target::FilesHere(patterns) = rule.target {
            let folded = fold_name(name);
            let wanted = meta.kind == EntryKind::File
                && patterns.iter().any(|pattern| glob_match(pattern, &folded));
            if !wanted {
                return;
            }
        }
        if is_cloud_managed(meta) || meta.flags.contains(EntryFlags::SPECIAL) {
            sink.keep_whole(Some(inside));
            return;
        }

        let bytes = meta.allocated;
        let swept = |unit: u32| SweptFile {
            rule: inside.rule,
            unit,
            path: parent.path.join(name),
            expected: meta.expected(),
            bytes,
        };
        if inside.unit != 0 {
            let unit = sink.unit(inside);
            unit.bytes += bytes;
            unit.files += 1;
            unit.newest = unit.newest.max(stamp(meta));
            if self.record {
                sink.swept_files.push(swept(inside.unit));
            }
            return;
        }
        let totals = &mut sink.junk[inside.rule as usize];
        // Rien de plus récent que le début de l'analyse n'est compté, même
        // pour une règle sans âge minimal : ce qui est proposé existait déjà,
        // tel quel, quand l'analyse a commencé.
        if stamp(meta) <= planned.cutoff {
            totals.bytes += bytes;
            totals.files += 1;
            sink.dir_bytes += bytes;
            if self.record {
                sink.swept_files.push(swept(0));
            }
        } else {
            totals.spared_bytes += bytes;
            totals.spared_files += 1;
        }
    }
}

impl Visitor for Index<'_> {
    type Ctx = Ctx;
    type Sink = Sink;

    fn sink(&self) -> Sink {
        Sink { junk: vec![RuleTotals::default(); self.plan.rules.len()], ..Sink::default() }
    }

    fn directory(
        &self,
        sink: &mut Sink,
        parent: &Dir<'_, Ctx>,
        name: &OsStr,
        meta: &EntryMeta,
    ) -> Option<Ctx> {
        if parent.ctx.flags & DUPS != 0 && is_project_marker(name) {
            sink.managed = true;
        }
        let folded = fold_name(name);
        let mut step = std::mem::take(&mut sink.step);
        // Dans le territoire d'une règle, un dossier qu'un service de
        // synchronisation gère n'est pas un dossier de fichiers inutiles.
        let outcome = if parent.ctx.junk.is_some() && is_cloud_managed(meta) {
            None
        } else {
            self.descend(
                parent.ctx,
                &folded,
                is_hidden(meta),
                meta.flags.contains(EntryFlags::CLOUD),
                meta.file_id,
                &mut step,
            )
        };
        sink.step = step;

        let (mut ctx, needs_unit) = match outcome {
            Some(found) => found,
            None => {
                // On n'y entre pas : c'est par exemple un dossier que
                // l'utilisateur a exclu, atteint ici par un autre chemin.
                // L'élément jugé en bloc qui le contient ne sera pas nettoyé.
                sink.keep_whole(parent.ctx.junk);
                return None;
            }
        };
        match ctx.junk.as_mut() {
            Some(inside) => {
                if needs_unit {
                    // Les identifiants commencent à 1 : 0 veut dire « aucun ».
                    inside.unit = self.units.fetch_add(1, Ordering::Relaxed) + 1;
                }
                if inside.unit != 0 {
                    // Les dates du dossier lui-même comptent : y ajouter ou en
                    // retirer un fichier change sa date de modification.
                    let unit = sink.unit(*inside);
                    unit.newest = unit.newest.max(stamp(meta));
                    if self.record {
                        sink.swept_dirs.push(SweptDir {
                            rule: inside.rule,
                            unit: inside.unit,
                            path: parent.path.join(name),
                            expected: meta.expected(),
                            depth: parent.depth + 1,
                        });
                    }
                }
            }
            // Le parcours continue ici sans la règle : ce dossier mène à un
            // endroit protégé, et restera en place.
            None => sink.keep_whole(parent.ctx.junk),
        }
        Some(ctx)
    }

    fn entry(&self, sink: &mut Sink, parent: &Dir<'_, Ctx>, name: &OsStr, meta: &EntryMeta) {
        if meta.kind == EntryKind::File {
            sink.dir_files += 1;
        }
        // Un fichier peut lui aussi être exclu ou interdit par son nom
        // (`pagefile.sys`, un fichier ajouté aux « jamais touchés »). On ne le
        // vérifie que là où l'arbre des emplacements a encore quelque chose à dire.
        if !parent.ctx.nodes.is_empty() {
            let folded = fold_name(name);
            sink.step.clear();
            self.plan.tree.step(&parent.ctx.nodes, &folded, &mut sink.step);
            let off_limits = sink.step.iter().any(|&node| {
                let marks = self.plan.tree.marks(node);
                marks.iter().any(|mark| matches!(mark, Mark::Excluded | Mark::Forbidden))
            });
            if off_limits {
                sink.keep_whole(parent.ctx.junk);
                return;
            }
        }
        if let Some(inside) = parent.ctx.junk {
            self.junk_entry(sink, parent, inside, name, meta);
            return;
        }
        let flags = parent.ctx.flags;
        if flags & DUPS != 0 {
            // Un dépôt de code, ou un dossier d'app (il contient une
            // bibliothèque de programme) : les fichiers identiques y sont
            // voulus. La racine d'une bibliothèque de l'utilisateur fait
            // exception : un `.dll` téléchargé n'en fait pas une app.
            let app_folder = flags & DUP_ROOT == 0
                && meta.kind == EntryKind::File
                && ends_with_ignoring_case(name, b".dll");
            if is_project_marker(name) || app_folder {
                sink.managed = true;
            }
        }
        if meta.kind != EntryKind::File || is_hidden(meta) {
            return;
        }
        let synced = flags & SYNC != 0 || meta.flags.contains(EntryFlags::CLOUD);

        if flags & LARGE != 0 && meta.allocated >= self.plan.large_threshold {
            sink.large.count(meta.allocated);
            if sink.large.would_keep(meta.allocated) {
                sink.large.keep(LargeFile {
                    path: parent.path.join(name),
                    size: meta.size,
                    on_disk: meta.allocated,
                    modified: meta.modified,
                    synced,
                });
            }
        }
        if flags & DUPS != 0
            && !sink.managed
            && meta.is_readable_file()
            && meta.size >= self.plan.duplicate_min_size
        {
            sink.pending.push(Candidate {
                path: parent.path.join(name),
                size: meta.size,
                on_disk: meta.allocated,
                created: meta.created,
                modified: meta.modified,
                file_id: meta.file_id,
                volume: self.volume,
                synced,
            });
        }
    }

    fn leave(
        &self,
        sink: &mut Sink,
        dir: &Dir<'_, Ctx>,
        children: &mut Vec<Child<Ctx>>,
        outcome: &Result<(), ReadDirError>,
    ) {
        sink.folders += 1;
        match outcome {
            Ok(()) | Err(ReadDirError::NotFound) => {}
            Err(ReadDirError::AccessDenied) => {
                sink.denied += 1;
                if sink.denied_examples.len() < DENIED_EXAMPLES {
                    sink.denied_examples.push(dir.path.to_path_buf());
                }
            }
            Err(ReadDirError::NotADirectory | ReadDirError::Changed) => sink.replaced += 1,
            Err(ReadDirError::Io(_)) => sink.unreadable += 1,
        }
        // Un dossier qu'on n'a pas pu lire garde ce qu'il contient : l'élément
        // jugé en bloc auquel il appartient ne peut plus partir en entier.
        if !matches!(outcome, Ok(()) | Err(ReadDirError::NotFound)) {
            sink.keep_whole(dir.ctx.junk);
        }

        if sink.managed {
            // Projet ou dossier d'app : pas de doublons ici ni plus bas.
            sink.pending.clear();
            children.retain_mut(|child| {
                child.ctx.flags &= !(DUPS | DUP_ROOT);
                !child.ctx.is_idle()
            });
        } else {
            sink.candidates.append(&mut sink.pending);
        }
        sink.managed = false;

        sink.files += sink.dir_files;
        self.progress.folders.fetch_add(1, Ordering::Relaxed);
        self.progress.files.fetch_add(sink.dir_files, Ordering::Relaxed);
        if sink.dir_bytes > 0 {
            self.progress.found_bytes.fetch_add(sink.dir_bytes, Ordering::Relaxed);
        }
        sink.dir_files = 0;
        sink.dir_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(on_disk: u64, name: &str) -> LargeFile {
        LargeFile {
            path: PathBuf::from(name),
            size: on_disk,
            on_disk,
            modified: Timestamp::UNIX_EPOCH,
            synced: false,
        }
    }

    #[test]
    fn only_the_largest_files_are_kept_but_all_are_counted() {
        let mut top = LargeTop::default();
        for size in 1..=(LARGE_FILES_KEPT as u64 + 100) {
            top.count(size);
            if top.would_keep(size) {
                top.keep(file(size, &format!("f{size}")));
            }
        }
        assert_eq!(top.count, LARGE_FILES_KEPT as u64 + 100);
        let sorted = top.into_sorted();
        assert_eq!(sorted.len(), LARGE_FILES_KEPT);
        assert_eq!(sorted[0].on_disk, LARGE_FILES_KEPT as u64 + 100);
        assert_eq!(sorted[LARGE_FILES_KEPT - 1].on_disk, 101);
    }

    #[test]
    fn merging_keeps_the_overall_largest() {
        let mut left = LargeTop::default();
        let mut right = LargeTop::default();
        for size in [5, 1, 9] {
            left.count(size);
            left.keep(file(size, &format!("l{size}")));
        }
        for size in [7, 3] {
            right.count(size);
            right.keep(file(size, &format!("r{size}")));
        }
        left.merge(right);
        assert_eq!((left.count, left.bytes), (5, 25));
        let sizes: Vec<u64> = left.into_sorted().iter().map(|file| file.on_disk).collect();
        assert_eq!(sizes, [9, 7, 5, 3, 1]);
    }

    #[test]
    fn equal_sizes_are_ordered_by_path() {
        let mut top = LargeTop::default();
        for name in ["b", "a", "c"] {
            top.keep(file(4, name));
        }
        let names: Vec<_> = top.into_sorted().into_iter().map(|file| file.path).collect();
        assert_eq!(names, [PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c")]);
    }

    #[test]
    fn markers_are_recognised_whatever_the_case() {
        assert!(is_project_marker(OsStr::new(".git")));
        assert!(is_project_marker(OsStr::new(".GIT")));
        assert!(!is_project_marker(OsStr::new(".gitignore")));
        assert!(ends_with_ignoring_case(OsStr::new("moteur.DLL"), b".dll"));
        assert!(!ends_with_ignoring_case(OsStr::new(".dll"), b".dll"));
        assert!(!ends_with_ignoring_case(OsStr::new("dll"), b".dll"));
    }
}
