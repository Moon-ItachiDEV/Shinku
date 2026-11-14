//! Le plan d'une analyse : où aller, avec quelles règles, et où ne pas aller.
//!
//! Avant de lire quoi que ce soit, tout est rangé dans un seul arbre
//! d'emplacements : les interdits, les dossiers que l'utilisateur ne veut
//! jamais voir touchés, les racines des règles de fichiers inutiles, les
//! dossiers où chercher des doublons, les disques à indexer pour les gros
//! fichiers. Le parcours n'a plus qu'à suivre cet arbre.
//!
//! C'est aussi ici que chaque emplacement venu de la machine (dossier connu,
//! `%TEMP%`) est contrôlé : liste d'interdits, et chemin sans aucun lien.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::ScanOptions;
use super::report::RefusedRoot;
use crate::fs::{self, Identity, PathState, fold_path, inspect_dir_chain, to_internal_path};
use crate::guard::{Guard, Protection, join_segments};
use crate::model::{Rights, Timestamp};
use crate::platform::{Environment, KnownFolder, Volume};
use crate::rules::{CATALOG, RootCheck, Rule};
use crate::scope::{ScopeTree, path_segments};

/// Ce qu'un emplacement de l'arbre signifie pour l'analyse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    /// « Dossiers jamais touchés » : l'analyse n'y entre pas.
    Excluded,
    /// Interdit en entier (Windows, Program Files…) : l'analyse n'y entre pas.
    Forbidden,
    /// Dossier protégé en lui-même (profil, bibliothèques…) : jamais un
    /// fichier inutile, mais son contenu s'analyse.
    Keep,
    /// Racine d'une règle de fichiers inutiles (indice dans `Plan::rules`).
    Junk(u16),
    /// Les doublons se cherchent à partir d'ici.
    Dups,
    /// Les gros fichiers se cherchent à partir d'ici.
    Large,
    /// Dossier synchronisé avec un service en ligne.
    Sync,
}

/// Une règle retenue pour cette analyse.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlannedRule {
    pub rule: &'static Rule,
    /// Ce qui a été créé ou modifié après cet instant est épargné.
    pub cutoff: Timestamp,
}

/// Un point de départ du parcours.
#[derive(Clone, Debug)]
pub(crate) struct PlannedRoot {
    pub path: PathBuf,
    /// Composants du chemin, pliés : pour retrouver sa place dans l'arbre.
    pub components: Vec<String>,
    /// Identité du dossier, si le système de fichiers en fournit une.
    pub identity: Option<Identity>,
    /// Nombre de lectures en parallèle adapté à son disque.
    pub threads: usize,
    /// Regroupe les points de départ d'un même disque.
    pub volume_key: u64,
}

#[derive(Debug)]
pub(crate) struct Plan {
    pub tree: ScopeTree<Mark>,
    /// Pour chaque position de l'arbre : un emplacement à préserver (protégé,
    /// interdit ou exclu) se trouve-t-il ici ou plus bas ?
    pub kept_below: Vec<bool>,
    pub rules: Vec<PlannedRule>,
    pub roots: Vec<PlannedRoot>,
    /// Dossiers exclus par l'utilisateur, reconnus aussi par leur identité :
    /// un autre chemin vers le même dossier ne les fait pas réapparaître.
    pub excluded: Vec<Identity>,
    pub large_threshold: u64,
    pub duplicate_min_size: u64,
    pub refused: Vec<RefusedRoot>,
    pub volumes: Vec<Volume>,
}

impl Plan {
    pub(crate) fn build(environment: &dyn Environment, options: &ScanOptions) -> Self {
        let now = environment.now();
        let guard = Guard::for_machine(environment);
        let volumes = environment.volumes();
        let mut tree = ScopeTree::new();
        let mut refused = Vec::new();
        // Dossiers d'où l'analyse doit pouvoir partir.
        let mut anchors: Vec<PathBuf> = Vec::new();

        for place in guard.places() {
            if let Some(segments) = path_segments(&place.path) {
                let mark = match place.protection {
                    Protection::Subtree => Mark::Forbidden,
                    Protection::Itself => Mark::Keep,
                };
                tree.insert(&segments, mark);
            }
        }

        let mut excluded = Vec::new();
        for path in &options.excluded {
            let internal = to_internal_path(path).ok();
            let segments = internal.as_deref().and_then(path_segments);
            match (internal, segments) {
                (Some(internal), Some(segments)) => {
                    tree.insert(&segments, Mark::Excluded);
                    if let Ok(Some(identity)) = fs::identity(&internal) {
                        excluded.push(identity);
                    }
                    // Donnée à travers un lien (« Mes documents »), l'exclusion
                    // vaut aussi pour le dossier réel : c'est par lui que le
                    // parcours arrivera.
                    let real = std::fs::canonicalize(&internal)
                        .ok()
                        .and_then(|real| to_internal_path(&real).ok())
                        .filter(|real| *real != internal);
                    if let Some(real) = real {
                        if let Some(segments) = path_segments(&real) {
                            tree.insert(&segments, Mark::Excluded);
                        }
                        if let Ok(Some(identity)) = fs::identity(&real) {
                            excluded.push(identity);
                        }
                    }
                }
                // Une exclusion qu'on ne sait pas appliquer doit se voir : elle
                // ne protège rien.
                _ => refused.push(refusal("exclusion", path, "le chemin n'est pas absolu")),
            }
        }

        let sync_roots = environment.sync_roots();
        for root in &sync_roots {
            if let Some(segments) = path_segments(root) {
                tree.insert(&segments, Mark::Sync);
            }
        }

        let mut rules = Vec::new();
        if options.modules.junk {
            let local = environment.known_folder(KnownFolder::LocalAppData);
            for rule in CATALOG {
                // Ce qui demande les droits administrateur est l'affaire du helper.
                if rule.rights != Rights::User {
                    continue;
                }
                let Some(base) = environment.known_folder(rule.base) else { continue };
                match plan_rule(rule, &base, local.as_deref(), &guard) {
                    Ok(Some(prefix)) => {
                        let Some(segments) = join_segments(&base, rule.path) else { continue };
                        tree.insert(&segments, Mark::Junk(rules.len() as u16));
                        let age = Duration::from_secs(u64::from(rule.min_age_days) * 24 * 3600);
                        rules.push(PlannedRule { rule, cutoff: now.saturating_sub(age) });
                        anchors.push(prefix);
                    }
                    // L'app n'est pas installée : rien à dire.
                    Ok(None) => {}
                    Err(reason) => refused.push(refusal(rule.id, &base, &reason)),
                }
            }
        }

        if options.modules.duplicates {
            let libraries =
                KnownFolder::LIBRARIES.into_iter().filter_map(|f| environment.known_folder(f));
            for root in libraries.chain(sync_roots.iter().cloned()) {
                match inspect_dir_chain(&root) {
                    Ok(PathState::RealDir) => {
                        if let Some(segments) = path_segments(&root) {
                            tree.insert(&segments, Mark::Dups);
                            anchors.push(root);
                        }
                    }
                    Ok(PathState::Missing) => {}
                    Ok(PathState::NotPlain) => {
                        refused.push(refusal("doublons", &root, "le chemin passe par un lien"));
                    }
                    Err(error) => refused.push(refusal("doublons", &root, &error.to_string())),
                }
            }
        }

        if options.modules.large_files {
            for volume in volumes.iter().filter(|volume| volume.fixed && volume.system) {
                if let Some(segments) = path_segments(&volume.root) {
                    tree.insert(&segments, Mark::Large);
                    anchors.push(volume.root.clone());
                }
            }
        }

        let parallelism = environment.parallelism();
        let roots = walk_roots(anchors, &volumes, parallelism);
        let kept_below =
            tree.reaches(|mark| matches!(mark, Mark::Keep | Mark::Forbidden | Mark::Excluded));

        Self {
            tree,
            kept_below,
            rules,
            roots,
            excluded,
            large_threshold: options.large_threshold,
            duplicate_min_size: options.duplicate_min_size,
            refused,
            volumes,
        }
    }

    /// Nombre de lectures en parallèle pour le disque désigné par `volume_key`.
    pub(crate) fn threads_for(&self, volume_key: u64) -> usize {
        self.roots.iter().find(|root| root.volume_key == volume_key).map_or(1, |root| root.threads)
    }
}

fn refusal(purpose: &str, path: &Path, reason: &str) -> RefusedRoot {
    RefusedRoot { purpose: purpose.to_owned(), path: path.to_path_buf(), reason: reason.to_owned() }
}

/// Contrôle l'emplacement d'une règle. Rend le dossier d'où partir pour
/// l'atteindre, `None` s'il n'existe pas sur cette machine, ou la raison du refus.
fn plan_rule(
    rule: &Rule,
    base: &Path,
    local_app_data: Option<&Path>,
    guard: &Guard,
) -> Result<Option<PathBuf>, String> {
    if rule.root_check == RootCheck::TempFolder && !temp_is_plausible(base, local_app_data) {
        return Err("ce dossier temporaire n'est ni à sa place habituelle ni nommé Temp".to_owned());
    }
    // La partie du chemin sans joker : c'est elle qu'on peut vérifier d'avance.
    // Le reste est vérifié dossier par dossier pendant le parcours.
    let mut prefix = base.to_path_buf();
    for part in rule.path.iter().take_while(|part| !part.contains('*')) {
        prefix.push(part);
    }
    guard.check_rule_root(&prefix).map_err(|refusal| refusal.to_string())?;
    match inspect_dir_chain(&prefix) {
        Ok(PathState::RealDir) => Ok(Some(prefix)),
        Ok(PathState::Missing) => Ok(None),
        Ok(PathState::NotPlain) => Err("le chemin passe par un lien".to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// `%TEMP%` vient d'une variable d'environnement. On ne l'accepte que sous
/// `AppData\Local` (sa place habituelle) ou s'il s'appelle `Temp` ou `Tmp`.
fn temp_is_plausible(temp: &Path, local_app_data: Option<&Path>) -> bool {
    let Some(components) = fold_path(temp) else { return false };
    if let Some(local) = local_app_data.and_then(fold_path)
        && components.len() > local.len()
        && components.starts_with(&local)
    {
        return true;
    }
    components
        .last()
        .is_some_and(|name| name.eq_ignore_ascii_case("temp") || name.eq_ignore_ascii_case("tmp"))
}

/// Réduit les dossiers de départ à ceux qui ne sont contenus dans aucun
/// autre : chaque dossier du disque n'est ainsi lu qu'une fois.
fn walk_roots(anchors: Vec<PathBuf>, volumes: &[Volume], parallelism: usize) -> Vec<PlannedRoot> {
    let mut keyed: Vec<(Vec<String>, PathBuf)> =
        anchors.into_iter().filter_map(|path| Some((fold_path(&path)?, path))).collect();
    // Les plus courts d'abord : un dossier parent passe avant ses descendants.
    keyed.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then_with(|| a.0.cmp(&b.0)));
    let mut kept: Vec<(Vec<String>, PathBuf)> = Vec::new();
    for (components, path) in keyed {
        if !kept.iter().any(|(parent, _)| components.starts_with(parent)) {
            kept.push((components, path));
        }
    }

    let volume_roots: Vec<(Vec<String>, &Volume)> =
        volumes.iter().filter_map(|volume| Some((fold_path(&volume.root)?, volume))).collect();
    kept.into_iter()
        .map(|(components, path)| {
            let volume = volume_roots
                .iter()
                .filter(|(root, _)| components.starts_with(root))
                .max_by_key(|(root, _)| root.len());
            let identity = fs::identity(&path).ok().flatten();
            // À défaut d'identité, la position du disque dans la liste sert de clé.
            let position = volume
                .and_then(|(_, found)| volumes.iter().position(|v| std::ptr::eq(v, *found)))
                .map_or(u64::MAX, |index| index as u64);
            PlannedRoot {
                threads: volume.map_or(2, |(_, volume)| volume.read_threads(parallelism)),
                volume_key: identity.map_or(position, |identity| identity.volume),
                identity,
                components,
                path,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moved_temp_folder_must_look_like_one() {
        #[cfg(unix)]
        let (local, elsewhere) = (Path::new("/u/AppData/Local"), Path::new("/d"));
        #[cfg(windows)]
        let (local, elsewhere) = (Path::new(r"C:\u\AppData\Local"), Path::new(r"D:\"));
        assert!(temp_is_plausible(&local.join("Temp"), Some(local)));
        assert!(temp_is_plausible(&local.join("Temp").join("2"), Some(local)));
        assert!(temp_is_plausible(&elsewhere.join("TEMP"), Some(local)));
        assert!(temp_is_plausible(&elsewhere.join("Travail").join("tmp"), None));
        // Une variable d'environnement détournée vers des fichiers personnels.
        assert!(!temp_is_plausible(&elsewhere.join("Projets"), Some(local)));
        assert!(!temp_is_plausible(local, Some(local)));
        assert!(!temp_is_plausible(Path::new("Temp"), Some(local)));
    }

    #[test]
    fn nested_starting_points_are_merged() {
        #[cfg(unix)]
        let root = PathBuf::from("/");
        #[cfg(windows)]
        let root = PathBuf::from(r"\\?\C:\");
        let anchors = vec![
            root.join("Users").join("a").join("Documents"),
            root.join("Users").join("a"),
            root.join("Users").join("a").join("AppData").join("Local").join("Temp"),
            root.join("Données"),
            root.join("Données"),
        ];
        let roots = walk_roots(anchors, &[], 8);
        let paths: Vec<&Path> = roots.iter().map(|root| root.path.as_path()).collect();
        assert_eq!(paths, [root.join("Données"), root.join("Users").join("a")]);
        assert!(roots.iter().all(|root| root.threads == 2));
    }
}
