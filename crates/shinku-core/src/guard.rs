//! Liste d'interdits : ce qu'aucune règle générale ne peut toucher.
//!
//! Windows, Program Files, les racines du profil et des bibliothèques, les
//! fichiers d'échange et d'hibernation : supprimer là-dedans peut empêcher le
//! PC de démarrer ou détruire des fichiers personnels. La liste est compilée
//! dans le cœur, et tout emplacement qu'une règle veut nettoyer passe d'abord
//! devant elle.
//!
//! L'analyse s'en sert pour ne jamais proposer ce qu'il serait interdit de
//! supprimer. L'exécution s'en servira une seconde fois, élément par élément.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::fs::fold_path;
use crate::platform::{Environment, KnownFolder};
use crate::scope::{ScopeTree, Segment, path_segments};

/// Comment un emplacement est protégé.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protection {
    /// Rien de ce qu'il contient ne peut être proposé (Windows, Program Files…).
    Subtree,
    /// Le dossier lui-même est intouchable et ne peut pas servir de racine à
    /// une règle, mais son contenu se décide ailleurs (profil, Documents…).
    Itself,
}

/// Un emplacement protégé.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedPlace {
    /// Chemin, sous forme interne.
    pub path: PathBuf,
    /// Nature de la protection.
    pub protection: Protection,
}

/// Pourquoi un emplacement ne peut pas servir de racine à une règle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootRefusal {
    /// Le chemin n'est pas absolu, ou contient `.` ou `..`.
    NotAbsolute,
    /// C'est la racine d'un disque.
    VolumeRoot,
    /// L'emplacement est protégé, ou se trouve dans un dossier protégé.
    Protected,
    /// L'emplacement contient un dossier protégé : il est trop large.
    ContainsProtected,
}

impl fmt::Display for RootRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAbsolute => "le chemin n'est pas absolu",
            Self::VolumeRoot => "c'est la racine d'un disque",
            Self::Protected => "l'emplacement est protégé",
            Self::ContainsProtected => "l'emplacement contient un dossier protégé",
        })
    }
}

impl std::error::Error for RootRefusal {}

/// Dossiers système que l'on trouve à la racine d'un disque. Ils sont
/// protégés en entier, sur chaque disque.
const SYSTEM_ROOT_ENTRIES: [&str; 17] = [
    "$Recycle.Bin",
    "System Volume Information",
    "Recovery",
    "$WinREAgent",
    "$Windows.~BT",
    "$Windows.~WS",
    "$SysReset",
    "$GetCurrent",
    "Windows.old",
    "Config.Msi",
    "PerfLogs",
    "Documents and Settings",
    // Fichiers d'échange, d'hibernation et de démarrage.
    "pagefile.sys",
    "hiberfil.sys",
    "swapfile.sys",
    "DumpStack.log.tmp",
    "bootmgr",
];

/// La liste d'interdits d'une machine.
#[derive(Debug)]
pub struct Guard {
    places: Vec<ProtectedPlace>,
    tree: ScopeTree<Protection>,
    /// Pour chaque position de l'arbre : y a-t-il un emplacement protégé ici ou plus bas ?
    guarded_below: Vec<bool>,
}

impl Guard {
    /// Construit la liste pour une machine.
    pub fn for_machine(environment: &dyn Environment) -> Self {
        let mut places: Vec<ProtectedPlace> = Vec::new();
        let mut add = |path: PathBuf, protection: Protection| {
            let known = fold_path(&path);
            if known.is_some() && !places.iter().any(|place| fold_path(&place.path) == known) {
                places.push(ProtectedPlace { path, protection });
            }
        };

        // Windows et les programmes installés : rien à y proposer.
        for folder in [
            KnownFolder::Windows,
            KnownFolder::ProgramFiles,
            KnownFolder::ProgramFilesX86,
            KnownFolder::ProgramData,
        ] {
            if let Some(path) = environment.known_folder(folder) {
                add(path, Protection::Subtree);
            }
        }

        // Chaque disque : sa racine, et les dossiers système qu'elle porte.
        for volume in environment.volumes() {
            for name in SYSTEM_ROOT_ENTRIES {
                add(volume.root.join(name), Protection::Subtree);
            }
            add(volume.root, Protection::Itself);
        }

        // Le profil, son dossier parent, et les bibliothèques : leur contenu
        // est analysé, mais aucun de ces dossiers n'est jamais « à nettoyer ».
        if let Some(profile) = environment.known_folder(KnownFolder::Profile) {
            if let Some(users) = profile.parent() {
                add(users.to_path_buf(), Protection::Itself);
            }
            add(profile, Protection::Itself);
        }
        for folder in KnownFolder::LIBRARIES {
            if let Some(path) = environment.known_folder(folder) {
                add(path, Protection::Itself);
            }
        }
        for folder in
            [KnownFolder::LocalAppData, KnownFolder::LocalAppDataLow, KnownFolder::RoamingAppData]
        {
            if let Some(path) = environment.known_folder(folder) {
                // `AppData` lui-même, parent des trois.
                if let Some(parent) = path.parent() {
                    add(parent.to_path_buf(), Protection::Itself);
                }
                add(path, Protection::Itself);
            }
        }
        for root in environment.sync_roots() {
            add(root, Protection::Itself);
        }

        let mut tree = ScopeTree::new();
        for place in &places {
            if let Some(segments) = path_segments(&place.path) {
                tree.insert(&segments, place.protection);
            }
        }
        let guarded_below = tree.reaches(|_| true);
        Self { places, tree, guarded_below }
    }

    /// Les emplacements protégés.
    pub fn places(&self) -> &[ProtectedPlace] {
        &self.places
    }

    /// Une règle peut-elle nettoyer le contenu de `path` ?
    ///
    /// Non si `path` est protégé, s'il se trouve dans un dossier protégé, ou
    /// s'il est assez large pour contenir un dossier protégé.
    pub fn check_rule_root(&self, path: &Path) -> Result<(), RootRefusal> {
        let components = fold_path(path).ok_or(RootRefusal::NotAbsolute)?;
        if components.len() <= 1 {
            return Err(RootRefusal::VolumeRoot);
        }
        match self.examine(&components) {
            Verdict::Free => Ok(()),
            Verdict::Inside | Verdict::Exact => Err(RootRefusal::Protected),
            Verdict::Contains => Err(RootRefusal::ContainsProtected),
        }
    }

    /// Vrai si supprimer `path` (fichier ou dossier) est interdit : il est
    /// protégé, dans un dossier protégé, ou il contient un dossier protégé.
    /// Un chemin qu'on ne sait pas lire est interdit par défaut.
    pub fn forbids(&self, path: &Path) -> bool {
        match fold_path(path) {
            Some(components) => self.examine(&components) != Verdict::Free,
            None => true,
        }
    }

    fn examine(&self, components: &[String]) -> Verdict {
        let mut current = vec![ScopeTree::<Protection>::ROOT];
        let mut next = Vec::new();
        for (index, component) in components.iter().enumerate() {
            next.clear();
            self.tree.step(&current, component, &mut next);
            std::mem::swap(&mut current, &mut next);
            if current.is_empty() {
                // On a quitté tous les emplacements protégés sans en traverser un.
                return Verdict::Free;
            }
            let last = index + 1 == components.len();
            let marks = current.iter().flat_map(|&node| self.tree.marks(node));
            for mark in marks {
                match mark {
                    Protection::Subtree => {
                        return if last { Verdict::Exact } else { Verdict::Inside };
                    }
                    Protection::Itself if last => return Verdict::Exact,
                    Protection::Itself => {}
                }
            }
        }
        if current.iter().any(|&node| self.guarded_below[node as usize]) {
            Verdict::Contains
        } else {
            Verdict::Free
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Free,
    /// Dans un dossier protégé en entier.
    Inside,
    /// Exactement un emplacement protégé.
    Exact,
    /// Contient un emplacement protégé.
    Contains,
}

/// Ajoute au chemin les segments écrits dans une règle.
pub(crate) fn join_segments(base: &Path, parts: &[&str]) -> Option<Vec<Segment>> {
    let mut segments = path_segments(base)?;
    segments.extend(parts.iter().map(|part| Segment::parse(part)));
    Some(segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{StaticEnvironment, Volume};

    /// Un PC typique, décrit avec des chemins du système de test.
    fn pc() -> (StaticEnvironment, PathBuf) {
        #[cfg(windows)]
        let c = PathBuf::from(r"\\?\C:\");
        #[cfg(unix)]
        let c = PathBuf::from("/c");
        let profile = c.join("Users").join("Léa");
        let environment = StaticEnvironment::new()
            .with_volume(Volume {
                root: c.clone(),
                label: String::new(),
                file_system: "NTFS".to_owned(),
                total: 0,
                free: 0,
                fixed: true,
                seek_penalty: Some(false),
                system: true,
            })
            .with_folder(KnownFolder::Windows, c.join("Windows"))
            .with_folder(KnownFolder::ProgramFiles, c.join("Program Files"))
            .with_folder(KnownFolder::ProgramFilesX86, c.join("Program Files (x86)"))
            .with_folder(KnownFolder::ProgramData, c.join("ProgramData"))
            .with_folder(KnownFolder::Profile, profile.clone())
            .with_folder(KnownFolder::Documents, profile.join("Documents"))
            .with_folder(KnownFolder::Pictures, profile.join("Pictures"))
            .with_folder(KnownFolder::Downloads, profile.join("Downloads"))
            .with_folder(KnownFolder::Desktop, profile.join("Desktop"))
            .with_folder(KnownFolder::LocalAppData, profile.join("AppData").join("Local"))
            .with_folder(KnownFolder::RoamingAppData, profile.join("AppData").join("Roaming"))
            .with_sync_root(profile.join("OneDrive"));
        (environment, c)
    }

    #[test]
    fn caches_inside_the_profile_can_be_cleaned() {
        let (environment, c) = pc();
        let guard = Guard::for_machine(&environment);
        let local = c.join("Users").join("Léa").join("AppData").join("Local");
        assert_eq!(guard.check_rule_root(&local.join("Temp")), Ok(()));
        assert_eq!(guard.check_rule_root(&local.join("Google").join("Chrome")), Ok(()));
        assert!(!guard.forbids(&local.join("Temp").join("a.tmp")));
        // Un %TEMP% déplacé sur le disque reste acceptable pour la liste d'interdits.
        assert_eq!(guard.check_rule_root(&c.join("Temp")), Ok(()));
    }

    #[test]
    fn protected_places_cannot_be_rule_roots() {
        let (environment, c) = pc();
        let guard = Guard::for_machine(&environment);
        let profile = c.join("Users").join("Léa");
        let refused = Err(RootRefusal::Protected);
        assert_eq!(guard.check_rule_root(&c), Err(RootRefusal::VolumeRoot));
        assert_eq!(guard.check_rule_root(&c.join("Users")), refused);
        assert_eq!(guard.check_rule_root(&profile), refused);
        assert_eq!(guard.check_rule_root(&profile.join("Documents")), refused);
        assert_eq!(guard.check_rule_root(&profile.join("AppData")), refused);
        assert_eq!(guard.check_rule_root(&profile.join("AppData").join("Local")), refused);
        assert_eq!(guard.check_rule_root(&profile.join("OneDrive")), refused);
        assert_eq!(guard.check_rule_root(&c.join("Windows")), refused);
        assert_eq!(guard.check_rule_root(&c.join("Windows").join("Temp")), refused);
        assert_eq!(guard.check_rule_root(&c.join("Windows").join("WinSxS").join("x")), refused);
        assert_eq!(guard.check_rule_root(&c.join("Program Files").join("App")), refused);
        assert_eq!(guard.check_rule_root(&c.join("$Recycle.Bin")), refused);
    }

    #[test]
    fn a_root_that_swallows_a_protected_place_is_too_wide() {
        // Un dossier connu rangé à un endroit inhabituel.
        let (environment, c) = pc();
        let odd = c.join("Données").join("Archives").join("Documents");
        let environment = environment.with_folder(KnownFolder::Documents, odd);
        let guard = Guard::for_machine(&environment);
        assert_eq!(
            guard.check_rule_root(&c.join("Données").join("Archives")),
            Err(RootRefusal::ContainsProtected)
        );
        assert_eq!(guard.check_rule_root(&c.join("Données")), Err(RootRefusal::ContainsProtected));
        assert_eq!(guard.check_rule_root(&c.join("Données").join("Autre")), Ok(()));
    }

    #[test]
    fn relative_paths_are_refused() {
        let (environment, _) = pc();
        let guard = Guard::for_machine(&environment);
        assert_eq!(guard.check_rule_root(Path::new("Temp")), Err(RootRefusal::NotAbsolute));
        assert!(guard.forbids(Path::new("Temp")));
    }

    #[test]
    fn deletion_is_forbidden_in_and_on_protected_places() {
        let (environment, c) = pc();
        let guard = Guard::for_machine(&environment);
        let profile = c.join("Users").join("Léa");
        // Dans un dossier protégé en entier.
        assert!(guard.forbids(&c.join("Windows").join("System32").join("ntoskrnl.exe")));
        assert!(guard.forbids(&c.join("pagefile.sys")));
        assert!(guard.forbids(&c.join("hiberfil.sys")));
        // Les dossiers eux-mêmes, et ce qui les contient.
        assert!(guard.forbids(&profile.join("Documents")));
        assert!(guard.forbids(&profile));
        assert!(guard.forbids(&c));
        // Leur contenu, lui, se décide ailleurs.
        assert!(!guard.forbids(&profile.join("Documents").join("copie.pdf")));
        assert!(!guard.forbids(&profile.join("Downloads").join("vieux").join("setup.exe")));
        assert!(!guard.forbids(&c.join("Jeux").join("gros.iso")));
    }

    #[cfg(windows)]
    #[test]
    fn case_and_path_form_do_not_matter() {
        let (environment, _) = pc();
        let guard = Guard::for_machine(&environment);
        assert!(guard.forbids(Path::new(r"c:\WINDOWS\system32\drivers\etc\hosts")));
        assert!(guard.forbids(Path::new(r"C:\PROGRAM FILES\App\app.exe")));
        assert_eq!(
            guard.check_rule_root(Path::new(r"C:\users\LÉA\documents")),
            Err(RootRefusal::Protected)
        );
    }

    #[test]
    fn every_place_is_listed_once() {
        let (environment, _) = pc();
        let guard = Guard::for_machine(&environment);
        let mut keys: Vec<_> = guard.places().iter().map(|place| fold_path(&place.path)).collect();
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), before);
        assert!(guard.places().len() > 20);
    }
}
