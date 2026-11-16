//! `startup.json` : ce que Shinku a coupé, gardé dans `%LOCALAPPDATA%\Shinku\`
//! (à côté de `game.json`).
//!
//! Windows ne dit pas **qui** a coupé une entrée. Ce fichier permet de dire
//! « coupée par Shinku le 12 nov. », et de remarquer qu'une app s'est remise
//! toute seule (certaines, celles bâties sur Electron notamment, effacent
//! l'approbation quand elles se réinscrivent).
//!
//! ```json
//! { "version": 1,
//!   "cut": [ { "id": "ur-…", "name": "Discord", "atMs": 1763230000000,
//!              "fileTime": 133760000000000000, "backMs": null } ] }
//! ```
//!
//! **Lecture méfiante** : n'importe quel programme du compte peut écrire ce
//! fichier. Absent, illisible ou de plus de 1 Mo, c'est un fichier vide (jamais
//! une erreur montrée). Une ligne mal formée est ignorée, et il y a 200 lignes
//! au plus (les plus anciennes sortent). Il ne contient que des dates et des
//! noms à montrer : rien de ce qu'il dit ne sert à écrire dans le registre.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::game::store::{APP_DIR, read_bounded};
use crate::platform::{Environment, KnownFolder};
use crate::settings::write_replacing;
use crate::startup::{DISPLAY_LIMIT, Entry, EntryId, Mark, Place, State, clean_text};

/// Version du format de `startup.json`.
pub const STARTUP_VERSION: u32 = 1;
/// Nom du fichier.
pub(crate) const FILE_NAME: &str = "startup.json";
/// Nombre de lignes gardées, au plus.
pub const CUT_KEPT: usize = 200;
/// Combien de temps on dit qu'une app « s'est remise toute seule », en
/// millisecondes depuis qu'on l'a vue pour la première fois.
pub const CAME_BACK_MS: i64 = 14 * 24 * 3600 * 1000;

/// Une entrée que Shinku a coupée.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CutLine {
    /// L'identifiant de l'entrée (du compte, ou du PC depuis que l'assistant
    /// élevé les règle).
    pub id: EntryId,
    /// Son nom affiché à ce moment-là (pour qui lirait le fichier).
    pub name: String,
    /// Date de la coupure, en millisecondes depuis 1970.
    #[serde(rename = "atMs")]
    pub at_ms: i64,
    /// Le `FILETIME` écrit dans l'approbation.
    pub file_time: u64,
    /// Première fois qu'on l'a vue active après la coupure.
    #[serde(rename = "backMs")]
    pub back_ms: Option<i64>,
}

/// Le contenu de `startup.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupFile {
    /// Version du format.
    pub version: u32,
    /// Ce que Shinku a coupé.
    pub cut: Vec<CutLine>,
}

impl Default for StartupFile {
    fn default() -> Self {
        Self { version: STARTUP_VERSION, cut: Vec::new() }
    }
}

/// Ce que [`StartupFile::reconcile`] conclut.
#[derive(Debug, Default)]
pub struct Marks {
    /// La marque de chaque entrée concernée.
    pub of: HashMap<EntryId, Mark>,
    /// Vrai si le fichier a changé : il faut le réécrire.
    pub changed: bool,
}

impl StartupFile {
    /// Note une coupure : la ligne de cette entrée est ajoutée ou remplacée, et
    /// la liste reste bornée.
    pub fn remember(&mut self, id: EntryId, name: &str, at_ms: i64, file_time: u64) {
        self.cut.retain(|line| line.id != id);
        self.cut.push(CutLine { id, name: clean_text(name), at_ms, file_time, back_ms: None });
        self.bound();
    }

    /// Oublie une entrée : Shinku l'a remise.
    pub fn forget(&mut self, id: EntryId) {
        self.cut.retain(|line| line.id != id);
    }

    /// Garde les 200 lignes les plus récentes.
    fn bound(&mut self) {
        while self.cut.len() > CUT_KEPT {
            let oldest = self
                .cut
                .iter()
                .enumerate()
                .min_by_key(|(_, line)| line.at_ms)
                .map(|(index, _)| index);
            match oldest {
                Some(index) => self.cut.remove(index),
                None => break,
            };
        }
    }

    /// Confronte le fichier à ce que les places contiennent, et dit quelle
    /// marque porte chaque entrée. Retire les lignes devenues sans objet :
    ///
    /// - l'entrée est coupée **avec le même `FILETIME`** : coupée par Shinku ;
    /// - coupée avec un autre : recoupée ailleurs, la ligne n'est plus vraie ;
    /// - active **sans valeur d'approbation** : l'app l'a effacée en se
    ///   réinscrivant, elle s'est remise sans Shinku. On note quand on l'a vue
    ///   pour la première fois ainsi, on le dit pendant 14 jours, puis la
    ///   ligne part ;
    /// - active **avec** une valeur (`02…`) : quelqu'un l'a rallumée dans
    ///   Windows, la ligne part en silence, sans marque ;
    /// - elle n'existe plus : la ligne part.
    ///
    /// Une ligne dont on ne peut rien conclure est gardée telle quelle : place
    /// illisible (l'entrée n'a pas disparu, on ne la voit pas), réglage
    /// illisible, ou identifiant porté par plusieurs entrées.
    pub fn reconcile(&mut self, entries: &[Entry], unreadable: &[Place], now_ms: i64) -> Marks {
        let mut marks = Marks::default();
        let mut by_id: HashMap<EntryId, Vec<&Entry>> = HashMap::new();
        for entry in entries {
            by_id.entry(entry.id).or_default().push(entry);
        }
        let mut kept = Vec::with_capacity(self.cut.len());
        for line in std::mem::take(&mut self.cut) {
            let mut line = line;
            if unreadable.contains(&line.id.place()) {
                kept.push(line);
                continue;
            }
            let found = by_id.get(&line.id).map(Vec::as_slice).unwrap_or_default();
            let entry = match found {
                [] => {
                    marks.changed = true;
                    continue;
                }
                [entry] => *entry,
                _ => {
                    kept.push(line);
                    continue;
                }
            };
            match entry.state {
                State::Disabled if entry.file_time == line.file_time => {
                    if line.back_ms.take().is_some() {
                        marks.changed = true;
                    }
                    marks.of.insert(line.id, Mark::CutByShinku { at_ms: line.at_ms });
                }
                State::Disabled => {
                    marks.changed = true;
                    continue;
                }
                // Une valeur `02…` : quelqu'un a rallumé l'app dans Windows (le
                // Gestionnaire des tâches l'écrit). Shinku ne peut pas dire
                // qu'elle « s'est remise toute seule » : la coupure est oubliée,
                // sans marque.
                State::Enabled if entry.approval_present => {
                    marks.changed = true;
                    continue;
                }
                // Valeur absente : l'app l'a effacée en se réinscrivant.
                State::Enabled => {
                    // Une date dans le futur (fichier trafiqué) vaut maintenant.
                    if line.back_ms.is_some_and(|back| back > now_ms) {
                        line.back_ms = Some(now_ms);
                        marks.changed = true;
                    }
                    let back = match line.back_ms {
                        Some(back) => back,
                        None => {
                            line.back_ms = Some(now_ms);
                            marks.changed = true;
                            now_ms
                        }
                    };
                    if now_ms.saturating_sub(back) >= CAME_BACK_MS {
                        marks.changed = true;
                        continue;
                    }
                    marks.of.insert(line.id, Mark::CameBack { cut_ms: line.at_ms });
                }
                State::Unknown => {}
            }
            kept.push(line);
        }
        self.cut = kept;
        marks
    }
}

/// Le fichier `startup.json`.
#[derive(Clone, Debug)]
pub struct StartupStore {
    path: PathBuf,
}

impl StartupStore {
    /// Le fichier de cette machine : `%LOCALAPPDATA%\Shinku\startup.json`.
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

    /// Lit le fichier. Absent ou illisible : un fichier vide.
    pub fn load(&self) -> StartupFile {
        let Some(text) = read_bounded(&self.path) else { return StartupFile::default() };
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let Ok(Value::Object(document)) = serde_json::from_str::<Value>(text) else {
            return StartupFile::default();
        };
        let mut file = StartupFile::default();
        if let Some(Value::Array(lines)) = document.get("cut") {
            // Au plus 200 lignes lues : le reste du fichier n'est pas regardé.
            for line in lines.iter().filter_map(parse_line) {
                if file.cut.iter().any(|known| known.id == line.id) {
                    continue;
                }
                file.cut.push(line);
                if file.cut.len() >= CUT_KEPT {
                    break;
                }
            }
        }
        file
    }

    /// Écrit le fichier d'un seul geste : l'ancien ou le nouveau, jamais un mélange.
    pub fn save(&self, file: &StartupFile) -> io::Result<()> {
        let json = serde_json::to_string_pretty(file).map_err(io::Error::other)?;
        let dir = self.path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "startup.json n'a pas de dossier")
        })?;
        fs::create_dir_all(dir)?;
        write_replacing(&self.path, json.as_bytes())
    }
}

/// Un entier du fichier. Les décimaux sont refusés : un `FILETIME` ne tient
/// pas dans un nombre flottant sans perdre ses derniers chiffres.
fn integer(value: Option<&Value>) -> Option<i64> {
    value?.as_i64()
}

/// Une ligne du fichier, si elle est bien formée : un identifiant d'une entrée,
/// une date, un `FILETIME` non nul.
fn parse_line(value: &Value) -> Option<CutLine> {
    let line = value.as_object()?;
    let id = EntryId::parse(line.get("id")?.as_str()?)?;
    let at_ms = integer(line.get("atMs"))?;
    let file_time = line.get("fileTime")?.as_u64().filter(|&time| time != 0)?;
    let name = line
        .get("name")
        .and_then(Value::as_str)
        .map(|name| name.chars().take(DISPLAY_LIMIT).collect())
        .unwrap_or_default();
    Some(CutLine { id, name, at_ms, file_time, back_ms: integer(line.get("backMs")) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::store::MAX_FILE_BYTES;
    use crate::platform::StaticEnvironment;
    use crate::startup::approval::{cut_bytes, restore_bytes};
    use crate::startup::host::{FakeStartupHost, StartupHost};
    use crate::startup::{self, Place};
    use serde_json::json;

    fn store() -> (tempfile::TempDir, StartupStore) {
        let temp = tempfile::tempdir().unwrap();
        let store = StartupStore::at(temp.path().join("Shinku").join("startup.json"));
        (temp, store)
    }

    fn write_raw(store: &StartupStore, text: &str) {
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), text).unwrap();
    }

    fn id(name: &str) -> EntryId {
        EntryId::new(Place::UserRun, name)
    }

    #[test]
    fn the_file_lives_next_to_game_json() {
        let local = Path::new(if cfg!(windows) { r"C:\Users\a\AppData\Local" } else { "/a/Local" });
        let environment = StaticEnvironment::new().with_folder(KnownFolder::LocalAppData, local);
        let store = StartupStore::for_machine(&environment).unwrap();
        assert_eq!(store.path(), local.join("Shinku").join("startup.json"));
        assert!(StartupStore::for_machine(&StaticEnvironment::new()).is_none());
    }

    #[test]
    fn a_file_survives_a_round_trip_and_uses_the_documented_names() {
        let (_temp, store) = store();
        assert_eq!(store.load(), StartupFile::default(), "absent : un fichier vide");
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 1_763_230_000_000, 133_760_000_000_000_000);
        file.cut[0].back_ms = Some(1_763_300_000_000);
        file.remember(id("Steam"), "Steam", 1_763_231_000_000, 133_760_010_000_000_000);
        store.save(&file).unwrap();
        assert_eq!(store.load(), file);
        // Rien ne traîne à côté du fichier.
        let names: Vec<_> = fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["startup.json"]);

        let document: Value =
            serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
        assert_eq!(document["version"], 1);
        assert_eq!(document["cut"][0]["id"], id("Discord").to_string());
        assert_eq!(document["cut"][0]["name"], "Discord");
        assert_eq!(document["cut"][0]["atMs"], 1_763_230_000_000_i64);
        assert_eq!(document["cut"][0]["fileTime"], 133_760_000_000_000_000_u64);
        assert_eq!(document["cut"][0]["backMs"], 1_763_300_000_000_i64);
        assert_eq!(document["cut"][1]["backMs"], Value::Null);
    }

    #[test]
    fn an_unreadable_file_is_an_empty_file() {
        let (_temp, store) = store();
        for text in ["", "{", "[1, 2]", "null", "\"texte\"", "{ \"cut\": 12 }", "{ \"cut\": {} }"] {
            write_raw(&store, text);
            assert_eq!(store.load(), StartupFile::default(), "{text}");
        }
        // Une marque d'ordre des octets est acceptée.
        let line = json!({"id": id("A").to_string(), "atMs": 1, "fileTime": 5});
        write_raw(&store, &format!("\u{feff}{}", json!({ "cut": [line] })));
        assert_eq!(store.load().cut.len(), 1);
        // Des octets qui ne sont pas de l'UTF-8.
        fs::write(store.path(), [0xff, 0xfe, 0x00, 0x7b]).unwrap();
        assert_eq!(store.load(), StartupFile::default());
    }

    #[test]
    fn a_malformed_line_is_ignored_and_the_others_are_kept() {
        let (_temp, store) = store();
        let good = |name: &str| json!({"id": id(name).to_string(), "name": name, "atMs": 10, "fileTime": 20});
        let document = json!({ "version": 99, "cut": [
            good("A"),
            "pas un objet",
            12,
            null,
            { "atMs": 1, "fileTime": 2 },
            { "id": "xx-0123456789abcdef", "atMs": 1, "fileTime": 2 },
            { "id": "ur-0123456789ABCDEF", "atMs": 1, "fileTime": 2 },
            { "id": "ur-0123456789abcde", "atMs": 1, "fileTime": 2 },
            { "id": id("B").to_string(), "fileTime": 2 },
            { "id": id("C").to_string(), "atMs": "hier", "fileTime": 2 },
            { "id": id("D").to_string(), "atMs": 1 },
            { "id": id("E").to_string(), "atMs": 1, "fileTime": "beaucoup" },
            { "id": id("F").to_string(), "atMs": 1, "fileTime": 0 },
            { "id": id("G").to_string(), "atMs": 1, "fileTime": -4 },
            { "id": id("H").to_string(), "atMs": 1.5, "fileTime": 2 },
            { "id": id("I").to_string(), "atMs": 1, "fileTime": 2.5 },
            good("A"),
            good("Z"),
        ]});
        write_raw(&store, &document.to_string());
        let names: Vec<String> = store.load().cut.into_iter().map(|line| line.name).collect();
        assert_eq!(names, ["A", "Z"], "la ligne en double aussi est écartée");
    }

    #[test]
    fn a_line_of_the_pc_is_kept_since_the_helper_cuts_those_too() {
        let (_temp, store) = store();
        let document = json!({ "cut": [
            { "id": EntryId::new(Place::MachineRun, "M").to_string(), "name": "M", "atMs": 1, "fileTime": 2 },
            { "id": EntryId::new(Place::MachineFolder, "F.lnk").to_string(), "name": "F", "atMs": 1, "fileTime": 2 },
        ]});
        write_raw(&store, &document.to_string());
        let names: Vec<String> = store.load().cut.into_iter().map(|line| line.name).collect();
        assert_eq!(names, ["M", "F"]);
    }

    #[test]
    fn a_hostile_field_is_bounded_and_a_bad_optional_field_is_forgotten() {
        let (_temp, store) = store();
        let long = "x".repeat(5000);
        let document = json!({ "cut": [
            { "id": id("A").to_string(), "name": long, "atMs": 1, "fileTime": 2, "backMs": "demain" },
            { "id": id("B").to_string(), "name": 7, "atMs": 1, "fileTime": 2 },
        ]});
        write_raw(&store, &document.to_string());
        let file = store.load();
        assert_eq!(file.cut[0].name.chars().count(), DISPLAY_LIMIT);
        assert_eq!(file.cut[0].back_ms, None);
        assert_eq!(file.cut[1].name, "");
    }

    #[test]
    fn at_most_two_hundred_lines_and_the_oldest_leave_first() {
        let (_temp, store) = store();
        let lines: Vec<Value> = (0..300)
            .map(|n| json!({"id": id(&format!("app{n}")).to_string(), "atMs": n, "fileTime": 9}))
            .collect();
        write_raw(&store, &json!({ "cut": lines }).to_string());
        assert_eq!(store.load().cut.len(), CUT_KEPT, "le reste du fichier n'est pas lu");

        let mut file = StartupFile::default();
        for n in 0..250 {
            file.remember(id(&format!("app{n}")), "x", i64::from(n), 9);
        }
        assert_eq!(file.cut.len(), CUT_KEPT);
        let oldest = file.cut.iter().map(|line| line.at_ms).min();
        assert_eq!(oldest, Some(50), "les 50 plus anciennes sont sorties");
        // Une ligne pour une même entrée : l'ancienne est remplacée.
        file.remember(id("app249"), "x", 999, 7);
        assert_eq!(file.cut.len(), CUT_KEPT);
        let line = file.cut.iter().find(|line| line.id == id("app249")).unwrap();
        assert_eq!((line.at_ms, line.file_time), (999, 7));
    }

    #[test]
    fn a_file_over_one_mebibyte_is_treated_as_unreadable() {
        let (_temp, store) = store();
        let padding = " ".repeat(usize::try_from(MAX_FILE_BYTES).unwrap() + 10);
        write_raw(&store, &format!("{{\"cut\": []{padding}}}"));
        assert_eq!(store.load(), StartupFile::default());
    }

    #[test]
    fn a_failed_write_is_reported_and_leaves_the_old_file() {
        let (_temp, store) = store();
        let mut file = StartupFile::default();
        file.remember(id("A"), "A", 1, 2);
        store.save(&file).unwrap();
        let blocked = StartupStore::at(store.path().join("sous").join("startup.json"));
        assert!(blocked.save(&file).is_err());
        assert_eq!(store.load(), file);
    }

    #[test]
    fn forgetting_removes_the_line() {
        let mut file = StartupFile::default();
        file.remember(id("A"), "A", 1, 2);
        file.remember(id("B"), "B", 1, 2);
        file.forget(id("A"));
        assert_eq!(file.cut.len(), 1);
        assert_eq!(file.cut[0].id, id("B"));
    }

    // --- Ce que les places disent des lignes ---

    const DAY: i64 = 24 * 3600 * 1000;

    /// Un PC où « Discord » est inscrit pour le compte.
    fn pc() -> FakeStartupHost {
        let host = FakeStartupHost::new();
        host.add(Place::UserRun, "Discord", r"C:\Apps\discord.exe");
        host
    }

    fn cut_at(file_time: u64) -> [u8; 12] {
        cut_bytes(file_time)
    }

    fn listing(host: &FakeStartupHost, store: &StartupStore) -> startup::Listing {
        startup::enumerate(host, None, Some(store), startup::MachineRules::CLOSED)
    }

    fn discord(listing: &startup::Listing) -> &Entry {
        listing.entries.iter().find(|entry| entry.raw_name == "Discord").unwrap()
    }

    #[test]
    fn disabled_with_the_same_file_time_is_cut_by_shinku() {
        let (_temp, store) = store();
        let host = pc();
        let stamp = host.file_time_now();
        host.set_bytes(Place::UserRun, "Discord", &cut_at(stamp));
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 1_000, stamp);
        store.save(&file).unwrap();

        let view = listing(&host, &store);
        assert_eq!(discord(&view).mark, Some(Mark::CutByShinku { at_ms: 1_000 }));
        assert_eq!(store.load(), file, "rien n'a changé : le fichier est le même");
    }

    #[test]
    fn disabled_with_another_file_time_was_cut_again_elsewhere() {
        let (_temp, store) = store();
        let host = pc();
        let stamp = host.file_time_now();
        host.set_bytes(Place::UserRun, "Discord", &cut_at(stamp + 10_000_000));
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 1_000, stamp);
        store.save(&file).unwrap();

        let view = listing(&host, &store);
        assert_eq!(discord(&view).mark, None);
        assert_eq!(discord(&view).state, State::Disabled);
        assert!(store.load().cut.is_empty(), "la ligne n'est plus vraie : elle part");
    }

    #[test]
    fn an_entry_that_came_back_is_noted_once_then_forgotten_after_fourteen_days() {
        let (_temp, store) = store();
        let host = pc();
        let stamp = host.file_time_now();
        // L'app a effacé l'approbation en se réinscrivant.
        host.clear_approval(Place::UserRun, "Discord");
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 1_000, stamp);
        store.save(&file).unwrap();

        let first_seen = host.now().unix_millis();
        let view = listing(&host, &store);
        assert_eq!(discord(&view).mark, Some(Mark::CameBack { cut_ms: 1_000 }));
        assert_eq!(store.load().cut[0].back_ms, Some(first_seen));

        // Treize jours plus tard : toujours dit, et la date de première
        // observation n'a pas bougé.
        host.advance(std::time::Duration::from_secs(13 * 24 * 3600));
        let view = listing(&host, &store);
        assert_eq!(discord(&view).mark, Some(Mark::CameBack { cut_ms: 1_000 }));
        assert_eq!(store.load().cut[0].back_ms, Some(first_seen));

        // Quatorze jours après la première observation : la ligne part.
        host.advance(std::time::Duration::from_secs(24 * 3600));
        let view = listing(&host, &store);
        assert_eq!(discord(&view).mark, None);
        assert!(store.load().cut.is_empty());
        assert_eq!(14 * DAY, CAME_BACK_MS);
    }

    #[test]
    fn an_entry_switched_back_on_in_windows_is_not_said_to_have_come_back_alone() {
        let (_temp, store) = store();
        let host = pc();
        let stamp = host.file_time_now();
        // La personne l'a rallumée dans le Gestionnaire des tâches : Windows écrit
        // `02 00 00 00` et huit zéros. Ce n'est pas « toute seule ».
        host.set_bytes(Place::UserRun, "Discord", &restore_bytes());
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 1_000, stamp);
        store.save(&file).unwrap();

        let view = listing(&host, &store);
        assert_eq!(discord(&view).state, State::Enabled);
        assert_eq!(discord(&view).mark, None, "aucune marque : Shinku ne peut pas le dire");
        assert!(store.load().cut.is_empty(), "la coupure notée est oubliée");
        // Et le jour suivant non plus : rien ne revient.
        host.advance(std::time::Duration::from_secs(24 * 3600));
        assert_eq!(discord(&listing(&host, &store)).mark, None);
    }

    #[test]
    fn a_forged_future_date_of_return_does_not_keep_the_mark_forever() {
        let (_temp, store) = store();
        let host = pc();
        let stamp = host.file_time_now();
        host.clear_approval(Place::UserRun, "Discord");
        let now = host.now().unix_millis();
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 1_000, stamp);
        file.cut[0].back_ms = Some(now + 400 * DAY);
        store.save(&file).unwrap();

        let view = listing(&host, &store);
        assert_eq!(discord(&view).mark, Some(Mark::CameBack { cut_ms: 1_000 }));
        assert_eq!(store.load().cut[0].back_ms, Some(now), "bornée par maintenant");
        host.advance(std::time::Duration::from_secs(14 * 24 * 3600));
        let view = listing(&host, &store);
        assert_eq!(discord(&view).mark, None, "elle expire quand même");
        assert!(store.load().cut.is_empty());
    }

    #[test]
    fn a_place_read_in_part_keeps_its_lines() {
        let (_temp, store) = store();
        let host = pc();
        // L'entrée n'est pas dans ce qu'on a lu : on ne sait pas si elle a disparu.
        host.make_incomplete(Place::UserRun);
        host.remove(Place::UserRun, "Discord");
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 1, 2);
        store.save(&file).unwrap();
        let view = listing(&host, &store);
        assert_eq!(view.incomplete, [Place::UserRun]);
        assert_eq!(store.load(), file);
    }

    #[test]
    fn an_entry_that_is_gone_loses_its_line() {
        let (_temp, store) = store();
        let host = pc();
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 1, 2);
        file.remember(id("Parti"), "Parti", 1, 2);
        store.save(&file).unwrap();
        host.set_bytes(Place::UserRun, "Discord", &cut_at(2));
        listing(&host, &store);
        let names: Vec<String> = store.load().cut.into_iter().map(|line| line.name).collect();
        assert_eq!(names, ["Discord"]);
    }

    #[test]
    fn an_unreadable_place_keeps_its_lines() {
        let (_temp, store) = store();
        let host = pc();
        host.make_unreadable(Place::UserRun);
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 1, 2);
        store.save(&file).unwrap();
        let view = listing(&host, &store);
        assert_eq!(view.unreadable, [Place::UserRun]);
        assert_eq!(
            store.load(),
            file,
            "on ne voit pas l'entrée : elle n'a pas disparu pour autant"
        );
    }

    #[test]
    fn an_unreadable_setting_keeps_its_line_without_a_mark() {
        let (_temp, store) = store();
        let host = pc();
        host.set_bytes(Place::UserRun, "Discord", &[6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 1, 2);
        store.save(&file).unwrap();
        let view = listing(&host, &store);
        assert_eq!(discord(&view).state, State::Unknown);
        assert_eq!(discord(&view).mark, None);
        assert_eq!(store.load(), file);
    }

    #[test]
    fn two_entries_with_the_same_id_leave_their_line_alone() {
        let (_temp, store) = store();
        let host = pc();
        // Deux fois le même nom dans la même place : impossible au registre,
        // mais une place qui répondrait ainsi ne doit rien faire effacer.
        host.add(Place::UserRun, "Discord", r"C:\Autre\discord.exe");
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 1, 2);
        store.save(&file).unwrap();
        let view = listing(&host, &store);
        assert!(view.entries.iter().all(|entry| entry.mark.is_none()));
        assert_eq!(store.load(), file);
    }

    #[test]
    fn the_file_is_rewritten_only_when_something_changed() {
        let (_temp, store) = store();
        let host = pc();
        let stamp = host.file_time_now();
        host.set_bytes(Place::UserRun, "Discord", &cut_at(stamp));
        // Un fichier écrit autrement que Shinku ne l'écrit : s'il était réécrit,
        // sa forme changerait.
        let compact = format!(
            "{{\"version\":1,\"cut\":[{{\"id\":\"{}\",\"name\":\"Discord\",\"atMs\":5,\"fileTime\":{stamp},\"backMs\":null}}]}}",
            id("Discord")
        );
        write_raw(&store, &compact);
        listing(&host, &store);
        listing(&host, &store);
        assert_eq!(fs::read_to_string(store.path()).unwrap(), compact);

        // Dès qu'une ligne part, il est réécrit.
        host.remove(Place::UserRun, "Discord");
        listing(&host, &store);
        assert!(store.load().cut.is_empty());
        assert_ne!(fs::read_to_string(store.path()).unwrap(), compact);
    }

    #[test]
    fn without_a_file_to_write_the_view_still_has_its_marks() {
        let host = pc();
        let view = startup::enumerate(&host, None, None, startup::MachineRules::CLOSED);
        assert_eq!(discord(&view).mark, None);
        assert_eq!(view.entries.len(), 1);
        // Un dossier qui n'existe pas et ne peut pas se créer : les marques se lisent quand même.
        let (_temp, good) = store();
        let stamp = host.file_time_now();
        host.set_bytes(Place::UserRun, "Discord", &cut_at(stamp));
        let mut file = StartupFile::default();
        file.remember(id("Discord"), "Discord", 5, stamp);
        good.save(&file).unwrap();
        let blocked = StartupStore::at(good.path().join("sous").join("startup.json"));
        assert_eq!(
            discord(&startup::enumerate(
                &host,
                None,
                Some(&blocked),
                startup::MachineRules::CLOSED
            ))
            .mark,
            None
        );
        assert_eq!(
            discord(&startup::enumerate(&host, None, Some(&good), startup::MachineRules::CLOSED))
                .mark,
            Some(Mark::CutByShinku { at_ms: 5 })
        );
    }
}
