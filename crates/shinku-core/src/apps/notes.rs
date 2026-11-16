//! `uninstalls.json` : les désinstallations que Shinku a vues, gardées dans
//! `%LOCALAPPDATA%\Shinku\` (à côté de `startup.json`, jamais dans le profil
//! itinérant : ce sont des noms d'apps).
//!
//! Le Journal les montre. Le fichier ne dit que ce que Shinku a **vu** : l'app
//! était inscrite quand il a lancé son désinstalleur, et son inscription a
//! disparu ensuite. Il ne dit pas dans quel état est le PC maintenant.
//!
//! ```json
//! { "version": 1,
//!   "notes": [ { "atMs": 1763230000000, "name": "Essai",
//!                "leftovers": 3, "leftoverBytes": 340000000 } ] }
//! ```
//!
//! **Un seul écrivain** : [`crate::apps::desk::AppsDesk`], sous son verrou. La
//! lecture est **méfiante**, champ par champ : n'importe quel programme du
//! compte peut écrire ce fichier. Absent, illisible ou de plus de 1 Mio, c'est
//! un fichier vide ; une note mal formée est écartée, pas tout le fichier ; il y
//! a 100 notes au plus (les plus anciennes sortent). Il ne contient aucun
//! chemin : un nom, une date, deux totaux.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::game::store::{APP_DIR, read_bounded};
use crate::platform::{Environment, KnownFolder};
use crate::settings::write_replacing;

/// Version du format de `uninstalls.json`.
pub const NOTES_VERSION: u32 = 1;
/// Nom du fichier.
pub(crate) const FILE_NAME: &str = "uninstalls.json";
/// Nombre de notes gardées, au plus.
pub const NOTES_KEPT: usize = 100;
/// Nombre de caractères d'un nom, au plus.
pub const NAME_LIMIT: usize = 200;

/// Une désinstallation que Shinku a vue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallNote {
    /// L'heure murale où Shinku a vu l'app partie, en millisecondes depuis 1970.
    pub at_ms: i64,
    /// Le nom affiché de l'app, 200 caractères au plus.
    pub name: String,
    /// Nombre de restes que Shinku a mis à la corbeille ensuite.
    pub leftovers: u32,
    /// Leur poids mesuré (0 si aucun n'a pu l'être).
    pub leftover_bytes: u64,
}

impl UninstallNote {
    /// Une note sans reste. Le nom est borné au caractère.
    pub fn new(at_ms: i64, name: &str) -> Self {
        Self { at_ms, name: bounded_name(name), leftovers: 0, leftover_bytes: 0 }
    }
}

/// Le contenu de `uninstalls.json` : les notes, de la plus ancienne à la plus
/// récente.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesFile {
    /// Version du format.
    pub version: u32,
    /// Les notes.
    pub notes: Vec<UninstallNote>,
}

impl Default for NotesFile {
    fn default() -> Self {
        Self { version: NOTES_VERSION, notes: Vec::new() }
    }
}

impl NotesFile {
    /// Ajoute une note ; les plus anciennes sortent au-delà de 100.
    pub fn push(&mut self, note: UninstallNote) {
        self.notes.push(note);
        if self.notes.len() > NOTES_KEPT {
            let extra = self.notes.len() - NOTES_KEPT;
            self.notes.drain(..extra);
        }
    }
}

/// Le fichier `uninstalls.json`.
#[derive(Clone, Debug)]
pub struct UninstallStore {
    path: PathBuf,
}

impl UninstallStore {
    /// Le fichier de cette machine : `%LOCALAPPDATA%\Shinku\uninstalls.json`.
    /// `None` si Windows ne donne pas le dossier `AppData\Local`.
    pub fn for_machine(environment: &dyn Environment) -> Option<Self> {
        let local = environment.known_folder(KnownFolder::LocalAppData)?;
        Some(Self::at(local.join(APP_DIR).join(FILE_NAME)))
    }

    /// Un fichier à cet emplacement.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Emplacement du fichier.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Lit le fichier. Absent, illisible ou trop gros : un fichier vide.
    pub fn load(&self) -> NotesFile {
        let Some(text) = read_bounded(&self.path) else { return NotesFile::default() };
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let Ok(Value::Object(document)) = serde_json::from_str::<Value>(text) else {
            return NotesFile::default();
        };
        let mut file = NotesFile::default();
        if let Some(Value::Array(notes)) = document.get("notes") {
            for note in notes.iter().filter_map(parse_note) {
                file.push(note);
            }
        }
        file
    }

    /// Écrit le fichier d'un seul geste : l'ancien ou le nouveau, jamais un mélange.
    pub fn save(&self, file: &NotesFile) -> io::Result<()> {
        let json = serde_json::to_string_pretty(file).map_err(io::Error::other)?;
        let dir = self.path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "uninstalls.json n'a pas de dossier")
        })?;
        fs::create_dir_all(dir)?;
        write_replacing(&self.path, json.as_bytes())
    }
}

/// Un nom borné à 200 caractères (jamais coupé au milieu d'un caractère), sans
/// caractère de commande.
fn bounded_name(name: &str) -> String {
    name.chars().filter(|character| !character.is_control()).take(NAME_LIMIT).collect()
}

/// Une note du fichier, si elle est bien formée : une date entière et un nom
/// non vide. Les totaux absents ou mal formés valent 0.
fn parse_note(value: &Value) -> Option<UninstallNote> {
    let note = value.as_object()?;
    let at_ms = note.get("atMs")?.as_i64()?;
    let name = bounded_name(note.get("name")?.as_str()?);
    if name.trim().is_empty() {
        return None;
    }
    let leftovers = note
        .get("leftovers")
        .and_then(Value::as_u64)
        .map_or(0, |count| u32::try_from(count).unwrap_or(u32::MAX));
    let leftover_bytes = note.get("leftoverBytes").and_then(Value::as_u64).unwrap_or(0);
    Some(UninstallNote { at_ms, name, leftovers, leftover_bytes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::store::MAX_FILE_BYTES;
    use crate::platform::StaticEnvironment;
    use serde_json::json;

    fn store() -> (tempfile::TempDir, UninstallStore) {
        let temp = tempfile::tempdir().unwrap();
        let store = UninstallStore::at(temp.path().join("Shinku").join("uninstalls.json"));
        (temp, store)
    }

    fn write_raw(store: &UninstallStore, text: &str) {
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), text).unwrap();
    }

    #[test]
    fn the_file_lives_next_to_startup_json() {
        let local = Path::new(if cfg!(windows) { r"C:\Users\a\AppData\Local" } else { "/a/Local" });
        let environment = StaticEnvironment::new().with_folder(KnownFolder::LocalAppData, local);
        let store = UninstallStore::for_machine(&environment).unwrap();
        assert_eq!(store.path(), local.join("Shinku").join("uninstalls.json"));
        assert!(UninstallStore::for_machine(&StaticEnvironment::new()).is_none());
    }

    #[test]
    fn a_missing_file_is_an_empty_one_and_a_round_trip_keeps_the_names() {
        let (_temp, store) = store();
        assert_eq!(store.load(), NotesFile::default());
        let mut file = NotesFile::default();
        let mut note = UninstallNote::new(1_763_230_000_000, "Essai");
        note.leftovers = 3;
        note.leftover_bytes = 340_000_000;
        file.push(note);
        store.save(&file).unwrap();
        assert_eq!(store.load(), file);
        let text = fs::read_to_string(store.path()).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["notes"][0]["atMs"], 1_763_230_000_000_i64);
        assert_eq!(parsed["notes"][0]["leftoverBytes"], 340_000_000_u64);
    }

    #[test]
    fn a_damaged_file_is_an_empty_one() {
        let (_temp, store) = store();
        for text in ["", "{ \"notes\": [", "[]", "42", "\"x\"", "{\"notes\": 7}"] {
            write_raw(&store, text);
            assert_eq!(store.load(), NotesFile::default(), "{text:?}");
        }
    }

    #[test]
    fn a_file_over_one_mebibyte_is_not_ours() {
        let (_temp, store) = store();
        let filler = "x".repeat(usize::try_from(MAX_FILE_BYTES).unwrap());
        write_raw(
            &store,
            &format!(
                "{{\"notes\":[{{\"atMs\":1,\"name\":\"Bonne\"}}],\"remplissage\":\"{filler}\"}}"
            ),
        );
        assert_eq!(store.load(), NotesFile::default());
    }

    #[test]
    fn one_damaged_note_is_dropped_and_the_good_ones_stay() {
        let (_temp, store) = store();
        let document = json!({
            "version": 1,
            "notes": [
                { "atMs": 1, "name": "Avant", "leftovers": 2, "leftoverBytes": 10 },
                { "name": "Sans date" },
                { "atMs": "hier", "name": "Date en texte" },
                { "atMs": 1.5, "name": "Date décimale" },
                { "atMs": 2 },
                { "atMs": 3, "name": 12 },
                { "atMs": 4, "name": "   " },
                "n'importe quoi",
                { "atMs": 5, "name": "Après", "leftovers": -1, "leftoverBytes": "beaucoup" },
            ]
        });
        write_raw(&store, &document.to_string());
        let file = store.load();
        let names: Vec<&str> = file.notes.iter().map(|note| note.name.as_str()).collect();
        assert_eq!(names, ["Avant", "Après"]);
        assert_eq!(file.notes[0].leftovers, 2);
        assert_eq!(file.notes[1].leftovers, 0, "un total mal formé vaut zéro");
        assert_eq!(file.notes[1].leftover_bytes, 0);
    }

    #[test]
    fn a_huge_count_is_bounded() {
        let (_temp, store) = store();
        write_raw(&store, r#"{"notes":[{"atMs":1,"name":"A","leftovers":99999999999}]}"#);
        assert_eq!(store.load().notes[0].leftovers, u32::MAX);
    }

    #[test]
    fn at_most_one_hundred_notes_stay_and_the_oldest_leave() {
        let (_temp, store) = store();
        let mut file = NotesFile::default();
        for index in 0..130_i64 {
            file.push(UninstallNote::new(index + 1, &format!("App {index}")));
        }
        assert_eq!(file.notes.len(), NOTES_KEPT);
        assert_eq!(file.notes[0].name, "App 30");
        store.save(&file).unwrap();
        assert_eq!(store.load().notes.len(), NOTES_KEPT);
        // Un fichier écrit à la main avec 150 notes est lu à 100.
        let many: Vec<Value> = (0..150)
            .map(|index| json!({ "atMs": index + 1, "name": format!("N{index}") }))
            .collect();
        write_raw(&store, &json!({ "notes": many }).to_string());
        let read = store.load();
        assert_eq!(read.notes.len(), NOTES_KEPT);
        assert_eq!(read.notes[99].name, "N149");
    }

    #[test]
    fn a_name_is_cut_to_two_hundred_characters_never_inside_one() {
        let long = "é".repeat(500);
        let note = UninstallNote::new(1, &long);
        assert_eq!(note.name.chars().count(), NAME_LIMIT);
        assert!(note.name.chars().all(|character| character == 'é'));
        let (_temp, store) = store();
        write_raw(&store, &json!({ "notes": [{ "atMs": 1, "name": long }] }).to_string());
        assert_eq!(store.load().notes[0].name.chars().count(), NAME_LIMIT);
        // Les caractères de commande n'entrent pas dans un nom.
        assert_eq!(UninstallNote::new(1, "A\u{0}B\nC").name, "ABC");
    }
}
