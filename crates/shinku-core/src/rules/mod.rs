//! Les règles : ce qu'on a le droit de chercher, et où.
//!
//! Une règle de fichiers inutiles désigne un emplacement, dit ce qui s'y
//! nettoie (tout le contenu, ou certains fichiers), à partir de quel âge, avec
//! quels droits, et quelle app il vaut mieux fermer avant.
//!
//! Les règles sont compilées dans le binaire. Aucune n'est téléchargée, et
//! aucune ne vient de l'interface : c'est ce qui permet de les relire, de les
//! tester, et de garantir qu'un nettoyage ne sort jamais de ces emplacements.

use serde::{Deserialize, Serialize};

use crate::model::Rights;
use crate::platform::KnownFolder;

mod catalog;

pub use catalog::CATALOG;

/// Les catégories de fichiers inutiles, telles que l'écran Résultats les montre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JunkCategory {
    /// Caches des apps.
    AppCaches,
    /// Fichiers temporaires.
    TempFiles,
    /// Restes de mises à jour.
    UpdateLeftovers,
    /// Journaux et rapports d'erreur.
    LogsAndReports,
}

impl JunkCategory {
    /// Toutes les catégories, dans l'ordre de l'écran.
    pub const ALL: [Self; 4] =
        [Self::AppCaches, Self::TempFiles, Self::UpdateLeftovers, Self::LogsAndReports];

    /// Nom affiché.
    pub const fn label(self) -> &'static str {
        match self {
            Self::AppCaches => "Caches des apps",
            Self::TempFiles => "Fichiers temporaires",
            Self::UpdateLeftovers => "Restes de mises à jour",
            Self::LogsAndReports => "Journaux et rapports d'erreur",
        }
    }
}

/// Ce qu'une règle nettoie à son emplacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Tout le contenu du dossier, sous-dossiers compris. Le dossier lui-même reste.
    Everything,
    /// Seulement les fichiers posés directement dans le dossier dont le nom
    /// correspond à l'un de ces motifs (`*` remplace n'importe quoi).
    FilesHere(&'static [&'static str]),
}

/// Contrôle supplémentaire sur l'emplacement d'une règle, une fois résolu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootCheck {
    /// Aucun : l'emplacement est figé par la règle.
    None,
    /// L'emplacement vient d'une variable d'environnement (`%TEMP%`), que
    /// n'importe quel programme peut avoir changée. On ne l'accepte que s'il
    /// est à sa place habituelle ou s'il s'appelle bien `Temp` ou `Tmp`.
    TempFolder,
}

/// Une règle de fichiers inutiles.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    /// Identifiant stable, écrit dans le journal.
    pub id: &'static str,
    /// Catégorie affichée.
    pub category: JunkCategory,
    /// Nom affiché : l'app ou la nature des fichiers.
    pub label: &'static str,
    /// Dossier connu d'où part l'emplacement.
    pub base: KnownFolder,
    /// Chemin sous ce dossier, composant par composant. `*` remplace une
    /// partie de nom (profils de navigateur, apps du Store…).
    pub path: &'static [&'static str],
    /// Ce qui se nettoie à cet emplacement.
    pub target: Target,
    /// Âge minimal, en jours. Ce qui a été créé ou modifié plus récemment est
    /// épargné : une app en a peut-être besoin. 0 : pas de condition d'âge.
    pub min_age_days: u32,
    /// Avec un âge minimal : chaque élément posé directement dans le dossier
    /// est jugé en bloc. Un sous-dossier n'est nettoyé que si rien n'y a bougé
    /// depuis l'âge minimal. Sinon, chaque fichier est jugé seul.
    pub whole_entries: bool,
    /// Droits nécessaires au nettoyage.
    pub rights: Rights,
    /// Apps à fermer avant de nettoyer (noms de processus).
    pub close_first: &'static [&'static str],
    /// Contrôle supplémentaire sur l'emplacement résolu.
    pub root_check: RootCheck,
}

impl Rule {
    /// Modèle de règle : tout le contenu d'un dossier du profil, sans
    /// condition. Les règles du catalogue partent de là.
    pub(crate) const fn new(
        id: &'static str,
        category: JunkCategory,
        label: &'static str,
        base: KnownFolder,
        path: &'static [&'static str],
    ) -> Self {
        Self {
            id,
            category,
            label,
            base,
            path,
            target: Target::Everything,
            min_age_days: 0,
            whole_entries: false,
            rights: Rights::User,
            close_first: &[],
            root_check: RootCheck::None,
        }
    }

    /// Ne garde que les fichiers posés directement dans le dossier et dont le
    /// nom correspond à l'un des motifs.
    pub(crate) const fn files(mut self, patterns: &'static [&'static str]) -> Self {
        self.target = Target::FilesHere(patterns);
        self
    }

    /// Épargne ce qui a moins de `days` jours, fichier par fichier.
    pub(crate) const fn older_than(mut self, days: u32) -> Self {
        self.min_age_days = days;
        self
    }

    /// Épargne ce qui a moins de `days` jours, en jugeant en bloc chaque
    /// élément posé directement dans le dossier.
    pub(crate) const fn entries_older_than(mut self, days: u32) -> Self {
        self.min_age_days = days;
        self.whole_entries = true;
        self
    }

    /// Apps à fermer avant de nettoyer.
    pub(crate) const fn close(mut self, processes: &'static [&'static str]) -> Self {
        self.close_first = processes;
        self
    }

    /// Ajoute un contrôle sur l'emplacement résolu.
    pub(crate) const fn check(mut self, root_check: RootCheck) -> Self {
        self.root_check = root_check;
        self
    }
}

/// Âge minimal des fichiers temporaires, des journaux et des rapports.
/// C'est aussi celui que l'outil de nettoyage de Windows applique à `%TEMP%`.
pub const SPARE_RECENT_DAYS: u32 = 7;

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::scope::glob_match;

    #[test]
    fn identifiers_are_unique_and_tidy() {
        let mut seen = HashSet::new();
        for rule in CATALOG {
            assert!(seen.insert(rule.id), "identifiant en double : {}", rule.id);
            assert!(
                rule.id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-'),
                "identifiant mal formé : {}",
                rule.id
            );
            assert!(!rule.label.trim().is_empty(), "{} : nom vide", rule.id);
        }
        assert!(CATALOG.len() >= 30, "le catalogue paraît bien court");
    }

    #[test]
    fn every_location_is_a_plain_chain_of_names() {
        for rule in CATALOG {
            for part in rule.path {
                assert!(!part.is_empty(), "{} : composant vide", rule.id);
                assert!(*part != "." && *part != "..", "{} : composant relatif", rule.id);
                assert!(
                    !part.contains(['\\', '/', ':', '?', '"', '<', '>', '|']),
                    "{} : caractère interdit dans « {part} »",
                    rule.id
                );
                assert!(!part.ends_with(['.', ' ']), "{} : « {part} » finit mal", rule.id);
            }
        }
    }

    #[test]
    fn no_rule_can_cover_a_whole_known_folder() {
        for rule in CATALOG {
            if rule.base == KnownFolder::Temp {
                // `%TEMP%` est fait pour ça, et il a son propre contrôle.
                assert_eq!(rule.root_check, RootCheck::TempFolder, "{}", rule.id);
                continue;
            }
            assert!(!rule.path.is_empty(), "{} : nettoierait tout un dossier connu", rule.id);
            // Le premier composant est un nom précis : un joker juste sous
            // AppData désignerait toutes les apps à la fois.
            assert!(!rule.path[0].contains('*'), "{} : joker en tête", rule.id);
            // Et un joker n'est jamais le dernier mot d'une règle « tout le contenu ».
            if rule.target == Target::Everything {
                let last = rule.path[rule.path.len() - 1];
                assert!(!last.contains('*'), "{} : joker en fin de chemin", rule.id);
            }
        }
    }

    #[test]
    fn rules_stay_inside_the_profile_and_need_no_elevation() {
        // Tant que le helper n'existe pas, le catalogue ne contient que ce
        // que l'utilisateur peut nettoyer lui-même.
        for rule in CATALOG {
            assert_eq!(rule.rights, Rights::User, "{}", rule.id);
            assert!(
                matches!(
                    rule.base,
                    KnownFolder::LocalAppData | KnownFolder::RoamingAppData | KnownFolder::Temp
                ),
                "{} : hors du profil",
                rule.id
            );
        }
    }

    #[test]
    fn age_and_patterns_are_coherent() {
        for rule in CATALOG {
            if rule.whole_entries {
                assert!(rule.min_age_days > 0, "{} : jugement en bloc sans âge", rule.id);
                assert_eq!(rule.target, Target::Everything, "{}", rule.id);
            }
            if let Target::FilesHere(patterns) = rule.target {
                assert!(!patterns.is_empty(), "{} : aucun motif", rule.id);
                for pattern in patterns {
                    assert!(
                        !pattern.is_empty() && *pattern != "*",
                        "{} : motif trop large",
                        rule.id
                    );
                    assert_eq!(
                        pattern.to_lowercase(),
                        *pattern,
                        "{} : motif en minuscules",
                        rule.id
                    );
                }
            }
            // Temporaires, journaux et rapports épargnent toujours le récent.
            if matches!(rule.category, JunkCategory::TempFiles | JunkCategory::LogsAndReports) {
                assert!(rule.min_age_days >= SPARE_RECENT_DAYS, "{} : âge minimal absent", rule.id);
            }
        }
    }

    #[test]
    fn dangerous_places_are_never_named() {
        // Dossiers d'apps qui ressemblent à des caches mais contiennent des
        // données : ils ne doivent apparaître dans aucune règle.
        let forbidden = ["localcache", "localstate", "roamingstate", "settings", "user data"];
        for rule in CATALOG {
            let last = rule.path.last().map(|part| part.to_lowercase()).unwrap_or_default();
            assert!(!forbidden.contains(&last.as_str()), "{} vise « {last} »", rule.id);
        }
    }

    #[test]
    fn patterns_match_what_they_are_meant_for() {
        let thumbnails = CATALOG.iter().find(|rule| rule.id == "windows.thumbnails").unwrap();
        let Target::FilesHere(patterns) = thumbnails.target else { panic!("motifs attendus") };
        let hit = |name: &str| patterns.iter().any(|pattern| glob_match(pattern, name));
        assert!(hit("thumbcache_256.db"));
        assert!(hit("iconcache_48.db"));
        assert!(!hit("explorer.exe"));
        assert!(!hit("thumbcache_256.db.bak"));
    }

    #[test]
    fn every_category_has_a_label() {
        for category in JunkCategory::ALL {
            assert!(!category.label().is_empty());
        }
        assert_eq!(JunkCategory::LogsAndReports.label(), "Journaux et rapports d'erreur");
    }
}
