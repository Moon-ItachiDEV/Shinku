//! Le nom affiché d'un profil, lu dans `Local State`.
//!
//! `Local State` porte la clé de chiffrement des mots de passe : Shinku ne
//! l'**écrit** ni ne l'**efface** jamais (il n'est dans aucune règle, un essai
//! l'interdit). Il n'est lu que pour une chose : `profile.info_cache.<dossier>.name`,
//! pour un `<dossier>` que Shinku a déjà trouvé par sa propre liste. Rien d'autre
//! n'en est gardé : ni `user_name` (qui porte l'adresse électronique), ni
//! `gaia_name`, ni `profiles_order`. Aucun texte lu ici ne sert à fabriquer un
//! chemin.
//!
//! Le fichier est ouvert sans suivre de lien, vérifié comme tout fichier lu par
//! le moteur ([`VerifiedFile`]), et **borné à 4 Mio** : au-delà, il n'est pas
//! lu et le profil prend son nom par défaut. Chromium l'écrit par remplacement
//! atomique : le lire ne le gêne pas.
//!
//! **Pourquoi `clean_text` et pas `shown_name`** (les deux sont dans `startup`) :
//! `shown_name` est faite pour une *confirmation*, elle montre entre ⟨ ⟩ tout
//! ce qui n'est pas une lettre, un chiffre ou de l'ASCII, donc un nom de profil
//! avec un emoji (ce que Chrome propose pour ses avatars) s'afficherait
//! `⟨U+1F3E0⟩`. `clean_text` est celle de la liste de Démarrage : elle retire
//! les caractères de contrôle et de mise en forme, dont ceux du sens d'écriture
//! (U+202A à U+202E, U+2066 à U+2069), réduit les blancs et borne à 80 caractères.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use crate::fs::{EntryMeta, VerifiedFile};
use crate::startup::clean_text;

/// Taille maximale de `Local State` que Shinku lit, en octets.
pub const MAX_LOCAL_STATE: u64 = 4 * 1024 * 1024;

/// Une entrée de `info_cache` : seul `name` est lu, quelle que soit la forme du reste.
#[derive(Deserialize)]
#[serde(untagged)]
enum Entry {
    Named {
        name: String,
    },
    #[allow(dead_code)]
    Other(serde::de::IgnoredAny),
}

#[derive(Deserialize)]
struct InfoCache {
    #[serde(default)]
    info_cache: HashMap<String, Entry>,
}

#[derive(Deserialize)]
struct LocalState {
    #[serde(default)]
    profile: Option<InfoCache>,
}

/// Les noms affichés des dossiers demandés (`folders`, comparés **exactement** à
/// la clé de `info_cache`). Un dossier sans nom lisible, ou dont le nom est vide
/// une fois nettoyé, est absent du résultat. Un texte qui n'est pas du JSON de
/// la forme attendue ne donne rien.
pub fn parse_display_names(bytes: &[u8], folders: &[String]) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let Ok(state) = serde_json::from_slice::<LocalState>(bytes) else { return names };
    let Some(cache) = state.profile else { return names };
    for (folder, entry) in cache.info_cache {
        if !folders.contains(&folder) {
            continue;
        }
        if let Entry::Named { name } = entry {
            let cleaned = clean_text(&name);
            if !cleaned.is_empty() {
                names.insert(folder, cleaned);
            }
        }
    }
    names
}

/// Lit `Local State` (`path`, que la liste de `User Data` a donné avec `meta`) et
/// en tire le nom des dossiers demandés. Tout échec (fichier absent, trop gros,
/// changé pendant la lecture, illisible) donne une table vide : jamais une erreur de vue.
pub fn read_display_names(
    path: &Path,
    meta: &EntryMeta,
    folders: &[String],
) -> HashMap<String, String> {
    if !meta.is_readable_file() || meta.size > MAX_LOCAL_STATE {
        return HashMap::new();
    }
    let Ok(mut file) = VerifiedFile::open(path, meta.fingerprint()) else { return HashMap::new() };
    let mut bytes = Vec::new();
    // Une lecture bornée : un fichier qui grossit pendant qu'on le lit n'ouvre pas la mémoire.
    if file.file_mut().take(MAX_LOCAL_STATE + 1).read_to_end(&mut bytes).is_err()
        || bytes.len() as u64 > MAX_LOCAL_STATE
        || file.recheck().is_err()
    {
        return HashMap::new();
    }
    parse_display_names(&bytes, folders)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    const STATE: &str = r#"{
        "profile": {
            "info_cache": {
                "Default": { "name": "Léa", "gaia_name": "Léa Martin", "user_name": "lea@example.org" },
                "Profile 1": { "name": "Travail", "user_name": "lea@travail.example" },
                "Profile 2": { "user_name": "secret@example.org", "gaia_name": "Secret" },
                "Profile 3": { "name": 12 },
                "Profile 4": 5,
                "Profile 5": { "name": "   " },
                "Profile 9": { "name": "Pas trouvé sur le disque" }
            },
            "profiles_order": ["Default", "Profile 1"]
        }
    }"#;

    #[test]
    fn only_the_name_of_a_folder_found_on_the_disk_is_read() {
        let found =
            folders(&["Default", "Profile 1", "Profile 2", "Profile 3", "Profile 4", "Profile 5"]);
        let names = parse_display_names(STATE.as_bytes(), &found);
        assert_eq!(names.get("Default").map(String::as_str), Some("Léa"));
        assert_eq!(names.get("Profile 1").map(String::as_str), Some("Travail"));
        // Pas de nom : jamais l'adresse ni le nom du compte en ligne à la place.
        assert_eq!(names.get("Profile 2"), None);
        assert_eq!(names.get("Profile 3"), None);
        assert_eq!(names.get("Profile 4"), None);
        assert_eq!(names.get("Profile 5"), None);
        // Un dossier que la liste du disque n'a pas trouvé n'entre pas.
        assert_eq!(names.get("Profile 9"), None);
        assert_eq!(names.len(), 2);
        let everything: String = names.values().cloned().collect();
        for never in ["lea@", "Martin", "secret", "Secret", "gaia", "user_name"] {
            assert!(!everything.contains(never), "{never}");
        }
    }

    #[test]
    fn the_folder_is_compared_exactly() {
        let names = parse_display_names(
            br#"{"profile":{"info_cache":{"profile 1":{"name":"x"},"Profile 01":{"name":"y"}}}}"#,
            &folders(&["Profile 1"]),
        );
        assert!(names.is_empty());
    }

    #[test]
    fn a_name_is_cleaned_as_the_startup_list_cleans_its_own() {
        let json = r#"{"profile":{"info_cache":{"Default":{"name":"  Pro\u202Efil\u200B\t perso\u0007 "}}}}"#;
        let names = parse_display_names(json.as_bytes(), &folders(&["Default"]));
        assert_eq!(names["Default"], "Profil perso");
        let long = format!(
            r#"{{"profile":{{"info_cache":{{"Default":{{"name":"{}"}}}}}}}}"#,
            "é".repeat(200)
        );
        let names = parse_display_names(long.as_bytes(), &folders(&["Default"]));
        assert_eq!(names["Default"].chars().count(), 80);
        // Un emoji est un nom comme un autre.
        let names = parse_display_names(
            r#"{"profile":{"info_cache":{"Default":{"name":"🏠 Maison"}}}}"#.as_bytes(),
            &folders(&["Default"]),
        );
        assert_eq!(names["Default"], "🏠 Maison");
    }

    #[test]
    fn what_is_not_the_expected_json_gives_nothing() {
        let f = folders(&["Default"]);
        for text in [
            "",
            "pas du json",
            "[]",
            "null",
            r#"{"profile":5}"#,
            r#"{"profile":{"info_cache":[]}}"#,
            r#"{}"#,
        ] {
            assert!(parse_display_names(text.as_bytes(), &f).is_empty(), "{text}");
        }
        assert!(parse_display_names(&[0xff, 0xfe, 0x00], &f).is_empty());
    }

    #[cfg(unix)]
    mod on_disk {
        use super::*;
        use crate::fs::{DirReader, to_internal_path};
        use std::ffi::OsStr;
        use std::fs;

        fn names_of(dir: &Path, folders: &[String]) -> HashMap<String, String> {
            let dir = to_internal_path(dir).unwrap();
            let mut found = None;
            DirReader::new()
                .read(&dir, None, &mut |name: &OsStr, meta: &EntryMeta| {
                    if name == "Local State" {
                        found = Some(*meta);
                    }
                })
                .unwrap();
            match found {
                Some(meta) => read_display_names(&dir.join("Local State"), &meta, folders),
                None => HashMap::new(),
            }
        }

        #[test]
        fn the_file_is_read_through_the_verified_open() {
            let temp = tempfile::tempdir().unwrap();
            fs::write(temp.path().join("Local State"), STATE).unwrap();
            let names = names_of(temp.path(), &folders(&["Default"]));
            assert_eq!(names["Default"], "Léa");
            // Jamais modifié.
            assert_eq!(fs::read_to_string(temp.path().join("Local State")).unwrap(), STATE);
        }

        #[test]
        fn a_missing_file_or_one_over_four_mebibytes_gives_the_default_name() {
            let temp = tempfile::tempdir().unwrap();
            assert!(names_of(temp.path(), &folders(&["Default"])).is_empty());
            let filler = " ".repeat(usize::try_from(MAX_LOCAL_STATE).unwrap());
            fs::write(
                temp.path().join("Local State"),
                format!(r#"{{"profile":{{"info_cache":{{"Default":{{"name":"Léa"}}}}}}}}{filler}"#),
            )
            .unwrap();
            assert!(names_of(temp.path(), &folders(&["Default"])).is_empty());
            // Pile à la borne : lu.
            let exact = r#"{"profile":{"info_cache":{"Default":{"name":"Léa"}}}}"#;
            let pad = usize::try_from(MAX_LOCAL_STATE).unwrap() - exact.len();
            fs::write(temp.path().join("Local State"), format!("{exact}{}", " ".repeat(pad)))
                .unwrap();
            assert_eq!(names_of(temp.path(), &folders(&["Default"]))["Default"], "Léa");
        }

        #[test]
        fn a_link_in_place_of_the_file_is_not_followed() {
            let temp = tempfile::tempdir().unwrap();
            let elsewhere = tempfile::tempdir().unwrap();
            fs::write(elsewhere.path().join("x"), STATE).unwrap();
            std::os::unix::fs::symlink(elsewhere.path().join("x"), temp.path().join("Local State"))
                .unwrap();
            assert!(names_of(temp.path(), &folders(&["Default"])).is_empty());
        }
    }
}
