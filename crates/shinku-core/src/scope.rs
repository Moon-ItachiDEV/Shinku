//! Arbre des emplacements : où chercher, où ne jamais aller.
//!
//! L'analyse lit le disque une seule fois pour tous les modules. Pour savoir,
//! à chaque dossier, ce qui s'y applique (une règle de fichiers inutiles
//! commence ici, ce dossier est exclu par l'utilisateur, celui-là est
//! protégé…), tous les emplacements sont rangés dans un même arbre, composant
//! par composant. Le parcours avance dans l'arbre en même temps que sur le
//! disque : il n'y a jamais de chemin complet à comparer.
//!
//! Un composant peut contenir le joker `*` (`Profile *`, `*`), pour les
//! emplacements dont une partie change d'un PC à l'autre.

use std::collections::HashMap;
use std::path::Path;

use crate::fs::{fold_name, fold_path};

/// Un composant de chemin dans un emplacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    /// Nom exact, déjà plié pour la comparaison.
    Exact(String),
    /// Motif avec un ou plusieurs `*`, déjà plié.
    Glob(String),
}

impl Segment {
    /// Lit un composant tel qu'on l'écrit dans une règle.
    pub fn parse(text: &str) -> Self {
        let folded = fold_name(text.as_ref());
        if folded.contains('*') { Self::Glob(folded) } else { Self::Exact(folded) }
    }

    /// Vrai si ce composant accepte le nom (déjà plié) `name`.
    pub fn matches(&self, name: &str) -> bool {
        match self {
            Self::Exact(exact) => exact == name,
            Self::Glob(pattern) => glob_match(pattern, name),
        }
    }
}

/// Composants d'un chemin absolu, sous forme de segments exacts.
/// `None` si le chemin est relatif ou contient `.` ou `..`.
pub fn path_segments(path: &Path) -> Option<Vec<Segment>> {
    Some(fold_path(path)?.into_iter().map(Segment::Exact).collect())
}

/// Compare `text` au motif `pattern`, où `*` remplace n'importe quelle suite
/// de caractères (même vide). Les deux doivent déjà être pliés.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let (pattern, text) = (pattern.as_bytes(), text.as_bytes());
    let (mut p, mut t) = (0, 0);
    // Position du dernier `*` rencontré et du texte qu'il avait alors absorbé.
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some((star_p, star_t)) = star {
            // Le dernier `*` absorbe un caractère de plus, et on reprend.
            p = star_p + 1;
            t = star_t + 1;
            star = Some((star_p, star_t + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&byte| byte == b'*')
}

/// Position dans l'arbre.
pub type NodeId = u32;

#[derive(Debug)]
struct Node<M> {
    exact: HashMap<String, NodeId>,
    globs: Vec<(String, NodeId)>,
    marks: Vec<M>,
}

impl<M> Default for Node<M> {
    fn default() -> Self {
        Self { exact: HashMap::new(), globs: Vec::new(), marks: Vec::new() }
    }
}

/// Arbre d'emplacements portant des marques de type `M`.
#[derive(Debug)]
pub struct ScopeTree<M> {
    nodes: Vec<Node<M>>,
}

impl<M> Default for ScopeTree<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M> ScopeTree<M> {
    /// La racine de l'arbre : le point de départ de tout chemin absolu.
    pub const ROOT: NodeId = 0;

    /// Crée un arbre vide.
    pub fn new() -> Self {
        Self { nodes: vec![Node::default()] }
    }

    /// Pose la marque `mark` sur l'emplacement décrit par `segments`.
    pub fn insert(&mut self, segments: &[Segment], mark: M) -> NodeId {
        let mut node = Self::ROOT;
        for segment in segments {
            let existing = match segment {
                Segment::Exact(name) => self.nodes[node as usize].exact.get(name).copied(),
                Segment::Glob(pattern) => self.nodes[node as usize]
                    .globs
                    .iter()
                    .find(|(known, _)| known == pattern)
                    .map(|&(_, child)| child),
            };
            node = match existing {
                Some(child) => child,
                None => {
                    let child = self.nodes.len() as NodeId;
                    self.nodes.push(Node::default());
                    match segment {
                        Segment::Exact(name) => {
                            self.nodes[node as usize].exact.insert(name.clone(), child);
                        }
                        Segment::Glob(pattern) => {
                            self.nodes[node as usize].globs.push((pattern.clone(), child));
                        }
                    }
                    child
                }
            };
        }
        self.nodes[node as usize].marks.push(mark);
        node
    }

    /// Marques posées exactement sur `node`.
    pub fn marks(&self, node: NodeId) -> &[M] {
        &self.nodes[node as usize].marks
    }

    /// Vrai si des emplacements plus profonds passent par `node`.
    pub fn has_children(&self, node: NodeId) -> bool {
        let node = &self.nodes[node as usize];
        !node.exact.is_empty() || !node.globs.is_empty()
    }

    /// Avance d'un composant : depuis chaque position de `from`, suit le nom
    /// (déjà plié) `name` et ajoute les positions atteintes à `out`.
    pub fn step(&self, from: &[NodeId], name: &str, out: &mut Vec<NodeId>) {
        for &node in from {
            let node = &self.nodes[node as usize];
            if let Some(&child) = node.exact.get(name) {
                push_unique(out, child);
            }
            for (pattern, child) in &node.globs {
                if glob_match(pattern, name) {
                    push_unique(out, *child);
                }
            }
        }
    }

    /// Positions atteintes en suivant tout un chemin depuis la racine.
    pub fn locate(&self, components: &[String]) -> Vec<NodeId> {
        let mut current = vec![Self::ROOT];
        let mut next = Vec::new();
        for component in components {
            next.clear();
            self.step(&current, component, &mut next);
            std::mem::swap(&mut current, &mut next);
            if current.is_empty() {
                break;
            }
        }
        current
    }

    /// Calcule, pour chaque position, si `wanted` est vrai pour une marque
    /// posée dessus ou plus bas. Sert à savoir si un dossier « contient »
    /// un emplacement d'un certain type.
    pub fn reaches(&self, wanted: impl Fn(&M) -> bool) -> Vec<bool> {
        let mut result = vec![false; self.nodes.len()];
        // Un enfant est toujours créé après son parent : en remontant la
        // liste, chaque nœud est traité après tous ses descendants.
        for index in (0..self.nodes.len()).rev() {
            let node = &self.nodes[index];
            result[index] = node.marks.iter().any(&wanted)
                || node.exact.values().any(|&child| result[child as usize])
                || node.globs.iter().any(|&(_, child)| result[child as usize]);
        }
        result
    }
}

fn push_unique(list: &mut Vec<NodeId>, node: NodeId) {
    if !list.contains(&node) {
        list.push(node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(parts: &[&str]) -> Vec<Segment> {
        parts.iter().map(|part| Segment::parse(part)).collect()
    }

    fn folded(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| fold_name(part.as_ref())).collect()
    }

    #[test]
    fn globs() {
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "default"));
        assert!(glob_match("profile *", "profile 12"));
        assert!(!glob_match("profile *", "profile"));
        assert!(glob_match("thumbcache_*.db", "thumbcache_1280.db"));
        assert!(!glob_match("thumbcache_*.db", "thumbcache_1280.db.tmp"));
        assert!(glob_match("*.dmp", "chrome.exe.1234.dmp"));
        assert!(glob_match("a*b*c", "a-b-b-c"));
        assert!(!glob_match("a*b*c", "a-b-b"));
        assert!(glob_match("cache", "cache"));
        assert!(!glob_match("cache", "cache2"));
        assert!(!glob_match("", "x"));
        assert!(glob_match("", ""));
        assert!(glob_match("**", "x"));
        assert!(glob_match("é*", "été"));
    }

    #[test]
    fn exact_paths_are_found_component_by_component() {
        let mut tree = ScopeTree::new();
        tree.insert(&segments(&["c:", "Users", "Léa", "Documents"]), "documents");
        tree.insert(&segments(&["c:", "Users", "Léa"]), "profil");

        let profile = tree.locate(&folded(&["c:", "Users", "Léa"]));
        assert_eq!(profile.len(), 1);
        assert_eq!(tree.marks(profile[0]), ["profil"]);
        assert!(tree.has_children(profile[0]));

        let documents = tree.locate(&folded(&["c:", "Users", "Léa", "Documents"]));
        assert_eq!(tree.marks(documents[0]), ["documents"]);
        assert!(!tree.has_children(documents[0]));

        assert!(tree.locate(&folded(&["c:", "Users", "Paul"])).is_empty());
        assert!(tree.locate(&folded(&["c:", "Users", "Léa", "Documents", "x"])).is_empty());
    }

    #[test]
    fn a_name_can_follow_several_branches() {
        let mut tree = ScopeTree::new();
        tree.insert(&segments(&["base", "*", "Cache"]), "joker");
        tree.insert(&segments(&["base", "Default", "Cache"]), "exact");
        tree.insert(&segments(&["base", "Profile *", "Cache"]), "profil");

        let marks = |path: &[&str]| -> Vec<&str> {
            let mut found: Vec<&str> = tree
                .locate(&folded(path))
                .into_iter()
                .flat_map(|node| tree.marks(node).iter().copied())
                .collect();
            found.sort_unstable();
            found
        };
        assert_eq!(marks(&["base", "Default", "Cache"]), ["exact", "joker"]);
        assert_eq!(marks(&["base", "Profile 3", "Cache"]), ["joker", "profil"]);
        assert_eq!(marks(&["base", "Guest", "Cache"]), ["joker"]);
        assert!(marks(&["base", "Guest", "Autre"]).is_empty());
        // Un joker ne traverse pas deux niveaux.
        assert!(marks(&["base", "a", "b", "Cache"]).is_empty());
    }

    #[test]
    fn the_same_location_is_stored_once() {
        let mut tree = ScopeTree::new();
        let first = tree.insert(&segments(&["a", "*", "b"]), 1);
        let second = tree.insert(&segments(&["a", "*", "b"]), 2);
        assert_eq!(first, second);
        assert_eq!(tree.marks(first), [1, 2]);
    }

    #[test]
    fn reaches_tells_what_lies_below() {
        let mut tree = ScopeTree::new();
        let deep = tree.insert(&segments(&["c:", "a", "b"]), "protégé");
        let other = tree.insert(&segments(&["c:", "x"]), "libre");
        let reaches = tree.reaches(|mark| *mark == "protégé");

        let a = tree.locate(&folded(&["c:", "a"]))[0];
        assert!(reaches[ScopeTree::<&str>::ROOT as usize]);
        assert!(reaches[a as usize]);
        assert!(reaches[deep as usize]);
        assert!(!reaches[other as usize]);
    }

    #[cfg(unix)]
    #[test]
    fn real_paths_become_segments() {
        let path = Path::new("/home/léa/Images");
        let expected = segments(&["home", "léa", "Images"]);
        assert_eq!(path_segments(path), Some(expected));
        assert_eq!(path_segments(Path::new("relatif")), None);
    }
}
