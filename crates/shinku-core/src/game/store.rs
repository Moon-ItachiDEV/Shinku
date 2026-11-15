//! `game.json` : le journal du mode jeu, gardé dans `%LOCALAPPDATA%\Shinku\`.
//!
//! Il contient ce qu'il faut pour tout remettre en place après un plantage ou
//! une coupure de courant (la partie en cours, avec l'état d'avant de chaque
//! chose changée), le bilan de la dernière partie, et les jeux déjà joués.
//! Il contient des numéros de processus : il ne doit pas suivre un profil
//! itinérant, d'où `AppData\Local`.
//!
//! Un seul écrivain : l'agent. Il écrit à chaque transition, jamais
//! périodiquement, d'un seul geste (voir [`crate::settings`]). La fenêtre ne
//! fait que le lire.
//!
//! La partie en cours garde deux dates : `opened`, l'ouverture, qui ne change
//! jamais pendant la partie (c'est la borne de tout ce qui touche à la remise
//! en place), et `since`, le début du jeu en cours, qui change quand la partie
//! passe d'un jeu à l'autre (c'est ce que la page affiche). Elle garde aussi
//! ses **racines** : tout processus qui est passé par le journal pendant la
//! partie, même s'il en est sorti depuis, pour retrouver ceux qui ont hérité de
//! sa priorité basse.
//!
//! **Lecture méfiante** : n'importe quel programme du compte peut écrire ce
//! fichier. Absent, illisible ou de plus de 1 Mo, c'est un état vide. Chaque
//! champ est lu pour lui-même, borné et contrôlé : une classe de priorité
//! inconnue (ou « temps réel ») devient « normale », un masque est borné à 0
//! ou 1, un GUID mal formé fait ignorer la partie alimentation, les listes et
//! les chaînes ont une taille maximale.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::game::PowerOutcome;
use crate::game::detect::{Source, key};
use crate::game::host::{PowerPlan, priority};
use crate::platform::{Environment, KnownFolder};
use crate::settings::write_replacing;

/// Version du format de `game.json`.
pub const GAME_VERSION: u32 = 1;
/// Nom du fichier.
const FILE_NAME: &str = "game.json";
/// Dossier de Shinku dans `AppData\Local`.
const APP_DIR: &str = "Shinku";
/// Nombre de jeux de bibliothèque gardés dans « déjà joués ».
pub const SEEN_KEPT: usize = 20;
/// Nombre maximal de processus en veilleuse dans le journal.
pub const QUIETED_KEPT: usize = 512;
/// Nombre maximal de racines dans le journal.
pub const ROOTS_KEPT: usize = 512;
/// Taille maximale du fichier : au-delà, il est traité comme illisible.
pub const MAX_FILE_BYTES: u64 = 1 << 20;
/// Longueur maximale d'une chaîne lue dans le fichier, en caractères.
const TEXT_LIMIT: usize = 260;

/// Un jeu reconnu.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRef {
    /// Chemin de son exécutable.
    pub path: String,
    /// Son nom.
    pub name: String,
    /// Comment il a été reconnu.
    pub source: Source,
}

/// Ce que la partie a fait de l'alimentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerRecord {
    /// Le résultat.
    pub outcome: PowerOutcome,
    /// Le mode d'avant. Présent pour `applied`, tant que Shinku ne l'a pas remis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<PowerPlan>,
    /// Le mode posé par Shinku. Présent pour `applied`, tant que Shinku ne l'a pas remis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied: Option<PowerPlan>,
}

impl PowerRecord {
    /// Un résultat sans changement d'alimentation.
    pub const fn of(outcome: PowerOutcome) -> Self {
        Self { outcome, previous: None, applied: None }
    }

    /// « Performances élevées » posé par Shinku, à la place de `previous`.
    pub const fn applied(previous: PowerPlan) -> Self {
        Self {
            outcome: PowerOutcome::Applied,
            previous: Some(previous),
            applied: Some(PowerPlan::HIGH_PERFORMANCE),
        }
    }

    /// Vrai si Shinku a posé un mode d'alimentation qu'il n'a pas encore remis.
    pub fn is_held(&self) -> bool {
        self.outcome == PowerOutcome::Applied && self.previous.is_some() && self.applied.is_some()
    }
}

/// Un processus mis en veilleuse, avec son état d'avant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quieted {
    /// Son numéro.
    pub pid: u32,
    /// Sa date de création (`FILETIME`) : un pid se recycle, pas cette date.
    pub started: i64,
    /// Le nom de son exécutable.
    pub exe: String,
    /// Sa classe de priorité d'avant.
    pub priority: u32,
    /// Le masque de contrôle du mode efficacité, d'avant (0 ou 1).
    pub throttle_control: u32,
    /// Le masque d'état du mode efficacité, d'avant (0 ou 1).
    pub throttle_state: u32,
}

/// Un processus passé par le journal pendant la partie : la racine d'une
/// famille de processus qui ont pu hériter de sa priorité basse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    /// Son numéro.
    pub pid: u32,
    /// Sa date de création (`FILETIME`).
    pub started: i64,
    /// Date, en millisecondes depuis 1970, où un relevé l'a vu mort. Absente
    /// tant qu'il vit.
    #[serde(rename = "goneMs", skip_serializing_if = "Option::is_none")]
    pub gone: Option<i64>,
}

/// La partie en cours.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// Le jeu.
    pub game: GameRef,
    /// Ouverture de la partie, en millisecondes depuis 1970. Ne change jamais
    /// pendant la partie : tout ce qui est né avant n'est pas d'elle.
    pub opened: i64,
    /// Début du jeu en cours, en millisecondes depuis 1970 (c'est l'affichage).
    pub since: i64,
    /// La personne a demandé de tout remettre en place : rien n'est plus appliqué.
    pub released: bool,
    /// L'alimentation.
    pub power: PowerRecord,
    /// Les processus en veilleuse.
    pub quieted: Vec<Quieted>,
    /// Tous les processus passés par le journal pendant la partie.
    pub roots: Vec<Root>,
    /// Quelque chose n'a pas pu être remis en place : Shinku réessaie.
    pub restore_pending: bool,
}

impl Session {
    /// Note un processus comme racine. Si la liste est pleine, la plus ancienne
    /// racine morte lui laisse la place ; s'il n'y en a pas, il n'est pas noté
    /// (il reste dans `quieted`, qui sert aussi de racines).
    pub fn add_root(&mut self, pid: u32, started: i64) {
        if self.roots.iter().any(|root| root.pid == pid && root.started == started) {
            return;
        }
        if self.roots.len() >= ROOTS_KEPT {
            let Some(dead) = self.roots.iter().position(|root| root.gone.is_some()) else {
                return;
            };
            self.roots.remove(dead);
        }
        self.roots.push(Root { pid, started, gone: None });
    }

    /// Les racines de la partie : celles qui sont notées, plus les processus
    /// en veilleuse (qui en sont toujours).
    pub fn all_roots(&self) -> Vec<Root> {
        let mut roots = self.roots.clone();
        for entry in &self.quieted {
            if !roots.iter().any(|root| root.pid == entry.pid && root.started == entry.started) {
                roots.push(Root { pid: entry.pid, started: entry.started, gone: None });
            }
        }
        roots
    }
}

/// Le bilan de la dernière partie terminée.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Last {
    /// Le nom du jeu.
    pub name: String,
    /// Le chemin de son exécutable.
    pub path: String,
    /// Début, en millisecondes depuis 1970.
    pub started: i64,
    /// Fin, en millisecondes depuis 1970.
    pub ended: i64,
    /// L'alimentation est revenue : `Some(true)` ; elle n'a pas pu l'être :
    /// `Some(false)` ; `None` si Shinku ne l'avait pas changée.
    pub power_restored: Option<bool>,
    /// Nombre d'apps distinctes remises à leur vitesse.
    pub apps_restored: u32,
    /// Faux si quelque chose n'a pas pu être remis en place.
    pub complete: bool,
}

/// Un jeu de bibliothèque déjà joué.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Seen {
    /// Le chemin de son exécutable.
    pub path: String,
    /// Son nom.
    pub name: String,
    /// La dernière partie, en millisecondes depuis 1970.
    pub last_played: i64,
}

/// Le contenu de `game.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameFile {
    /// Version du format.
    pub version: u32,
    /// La partie en cours, s'il y en a une.
    pub session: Option<Session>,
    /// Le bilan de la dernière partie.
    pub last: Option<Last>,
    /// Les jeux de bibliothèque déjà joués, le plus récent d'abord.
    pub seen: Vec<Seen>,
}

impl Default for GameFile {
    fn default() -> Self {
        Self { version: GAME_VERSION, session: None, last: None, seen: Vec::new() }
    }
}

impl GameFile {
    /// Note un jeu de bibliothèque comme joué à cette date : il passe en tête,
    /// une seule fois par chemin, et la liste reste bornée.
    pub fn remember(&mut self, path: &str, name: &str, played: i64) {
        let wanted = key(path);
        self.seen.retain(|seen| key(&seen.path) != wanted);
        self.seen
            .insert(0, Seen { path: path.to_owned(), name: name.to_owned(), last_played: played });
        self.seen.truncate(SEEN_KEPT);
    }
}

/// Le fichier `game.json`.
#[derive(Clone, Debug)]
pub struct GameStore {
    path: PathBuf,
}

impl GameStore {
    /// Le fichier de cette machine : `%LOCALAPPDATA%\Shinku\game.json`. `None`
    /// si Windows ne donne pas le dossier `AppData\Local`.
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

    /// Lit le fichier. Absent ou illisible : un état vide.
    pub fn load(&self) -> GameFile {
        let Some(text) = read_bounded(&self.path) else { return GameFile::default() };
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(document)) => parse(&document),
            _ => GameFile::default(),
        }
    }

    /// Écrit le fichier d'un seul geste : l'ancien ou le nouveau, jamais un mélange.
    pub fn save(&self, file: &GameFile) -> io::Result<()> {
        let json = serde_json::to_string_pretty(file).map_err(io::Error::other)?;
        let dir = self.path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "game.json n'a pas de dossier")
        })?;
        fs::create_dir_all(dir)?;
        write_replacing(&self.path, json.as_bytes())
    }
}

/// Lit le fichier en entier, à condition qu'il tienne dans
/// [`MAX_FILE_BYTES`] : un fichier plus gros n'est pas le nôtre.
fn read_bounded(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut text = String::new();
    file.take(MAX_FILE_BYTES + 1).read_to_string(&mut text).ok()?;
    (u64::try_from(text.len()).ok()? <= MAX_FILE_BYTES).then_some(text)
}

/// Une chaîne du fichier, bornée.
fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(|text| text.chars().take(TEXT_LIMIT).collect())
}

/// Un entier du fichier.
fn integer(value: Option<&Value>) -> Option<i64> {
    let number = value?;
    number.as_i64().or_else(|| number.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
}

/// Un masque du mode efficacité : 0 ou 1, et 0 pour tout le reste.
fn mask(value: Option<&Value>) -> u32 {
    u32::from(value.and_then(Value::as_u64) == Some(1))
}

fn source(value: Option<&Value>) -> Source {
    match value.and_then(Value::as_str) {
        Some("added") => Source::Added,
        _ => Source::Library,
    }
}

fn plan(value: Option<&Value>) -> Option<PowerPlan> {
    value.and_then(Value::as_str)?.parse().ok()
}

fn parse_power(value: Option<&Value>) -> PowerRecord {
    let Some(Value::Object(power)) = value else { return PowerRecord::of(PowerOutcome::Off) };
    let outcome: Option<PowerOutcome> =
        power.get("outcome").and_then(|outcome| serde_json::from_value(outcome.clone()).ok());
    match outcome {
        Some(PowerOutcome::Applied) => {
            match (plan(power.get("previous")), plan(power.get("applied"))) {
                (Some(previous), Some(applied)) => PowerRecord {
                    outcome: PowerOutcome::Applied,
                    previous: Some(previous),
                    applied: Some(applied),
                },
                // Ni l'un ni l'autre : c'est ainsi que Shinku écrit une
                // alimentation déjà remise (« appliquée, puis remise »).
                _ if !power.contains_key("previous") && !power.contains_key("applied") => {
                    PowerRecord { outcome: PowerOutcome::Applied, previous: None, applied: None }
                }
                // Un GUID abîmé : la partie alimentation est ignorée. Rien ne
                // sera remis, plutôt que de remettre un mode au hasard.
                _ => PowerRecord::of(PowerOutcome::Off),
            }
        }
        Some(other) => PowerRecord::of(other),
        None => PowerRecord::of(PowerOutcome::Off),
    }
}

fn parse_quieted(value: &Value) -> Option<Quieted> {
    let entry = value.as_object()?;
    let pid = u32::try_from(integer(entry.get("pid"))?).ok()?;
    let started = integer(entry.get("started"))?;
    let class = integer(entry.get("priority"))
        .and_then(|class| u32::try_from(class).ok())
        .filter(|&class| priority::is_known(class))
        .unwrap_or(priority::NORMAL);
    Some(Quieted {
        pid,
        started,
        exe: text(entry.get("exe")).unwrap_or_default(),
        priority: class,
        throttle_control: mask(entry.get("throttleControl")),
        throttle_state: mask(entry.get("throttleState")),
    })
}

fn parse_root(value: &Value) -> Option<Root> {
    let root = value.as_object()?;
    Some(Root {
        pid: u32::try_from(integer(root.get("pid"))?).ok()?,
        started: integer(root.get("started"))?,
        gone: integer(root.get("goneMs")),
    })
}

fn parse_session(value: &Value) -> Option<Session> {
    let session = value.as_object()?;
    let game = session.get("game").and_then(Value::as_object);
    let field = |name: &str| game.and_then(|game| text(game.get(name))).unwrap_or_default();
    let quieted = match session.get("quieted") {
        Some(Value::Array(entries)) => {
            entries.iter().filter_map(parse_quieted).take(QUIETED_KEPT).collect()
        }
        _ => Vec::new(),
    };
    let roots = match session.get("roots") {
        Some(Value::Array(entries)) => {
            entries.iter().filter_map(parse_root).take(ROOTS_KEPT).collect()
        }
        _ => Vec::new(),
    };
    let since = integer(session.get("since")).unwrap_or(0);
    Some(Session {
        game: GameRef {
            path: field("path"),
            name: field("name"),
            source: source(game.and_then(|game| game.get("source"))),
        },
        // Un journal d'avant `opened` se relit avec `opened = since`.
        opened: integer(session.get("opened")).unwrap_or(since),
        since,
        released: session.get("released").and_then(Value::as_bool).unwrap_or(false),
        power: parse_power(session.get("power")),
        quieted,
        roots,
        restore_pending: session.get("restorePending").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn parse_last(value: &Value) -> Option<Last> {
    let last = value.as_object()?;
    Some(Last {
        name: text(last.get("name"))?,
        path: text(last.get("path")).unwrap_or_default(),
        started: integer(last.get("started")).unwrap_or(0),
        ended: integer(last.get("ended")).unwrap_or(0),
        power_restored: last.get("powerRestored").and_then(Value::as_bool),
        apps_restored: integer(last.get("appsRestored"))
            .and_then(|count| u32::try_from(count).ok())
            .unwrap_or(0),
        complete: last.get("complete").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn parse(document: &Map<String, Value>) -> GameFile {
    // Une partie illisible n'est pas une raison de laisser des processus ralentis :
    // le journal garde tout ce qui se lit, champ par champ.
    let mut file = GameFile {
        session: document.get("session").and_then(parse_session),
        last: document.get("last").and_then(parse_last),
        ..GameFile::default()
    };
    if let Some(Value::Array(entries)) = document.get("seen") {
        for entry in entries {
            // Assez de jeux : le reste du fichier n'est pas lu (il peut être énorme).
            if file.seen.len() >= SEEN_KEPT {
                break;
            }
            let Some(entry) = entry.as_object() else { continue };
            let Some(path) = text(entry.get("path")).filter(|path| !path.is_empty()) else {
                continue;
            };
            if file.seen.iter().any(|known| key(&known.path) == key(&path)) {
                continue;
            }
            let name = text(entry.get("name")).unwrap_or_default();
            let last_played = integer(entry.get("lastPlayed")).unwrap_or(0);
            file.seen.push(Seen { path, name, last_played });
        }
    }
    file
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::StaticEnvironment;
    use serde_json::json;

    fn store() -> (tempfile::TempDir, GameStore) {
        let temp = tempfile::tempdir().unwrap();
        let store = GameStore::at(temp.path().join("Shinku").join("game.json"));
        (temp, store)
    }

    fn quieted(pid: u32) -> Quieted {
        Quieted {
            pid,
            started: 133_800_000_000_000_000,
            exe: "onedrive.exe".to_owned(),
            priority: priority::NORMAL,
            throttle_control: 0,
            throttle_state: 0,
        }
    }

    fn full_file() -> GameFile {
        GameFile {
            version: GAME_VERSION,
            session: Some(Session {
                game: GameRef {
                    path: r"D:\SteamLibrary\steamapps\common\Hollow Peaks\hp.exe".to_owned(),
                    name: "Hollow Peaks".to_owned(),
                    source: Source::Library,
                },
                opened: 1_794_765_000_000,
                since: 1_794_765_291_613,
                released: false,
                power: PowerRecord::applied(PowerPlan::BALANCED),
                quieted: vec![quieted(111), Quieted { priority: priority::HIGH, ..quieted(112) }],
                roots: vec![
                    Root { pid: 111, started: 133_800_000_000_000_000, gone: None },
                    Root {
                        pid: 90,
                        started: 133_799_000_000_000_000,
                        gone: Some(1_794_765_200_000),
                    },
                ],
                restore_pending: false,
            }),
            last: Some(Last {
                name: "Hollow Peaks".to_owned(),
                path: r"D:\x\hp.exe".to_owned(),
                started: 1_794_765_291_613,
                ended: 1_794_772_491_613,
                power_restored: Some(true),
                apps_restored: 2,
                complete: true,
            }),
            seen: vec![Seen {
                path: r"D:\x\hp.exe".to_owned(),
                name: "Hollow Peaks".to_owned(),
                last_played: 1_794_772_491_613,
            }],
        }
    }

    #[test]
    fn the_file_lives_in_local_app_data() {
        let local = Path::new(if cfg!(windows) { r"C:\Users\a\AppData\Local" } else { "/a/Local" });
        let environment = StaticEnvironment::new().with_folder(KnownFolder::LocalAppData, local);
        let store = GameStore::for_machine(&environment).unwrap();
        assert_eq!(store.path(), local.join("Shinku").join("game.json"));
        assert!(GameStore::for_machine(&StaticEnvironment::new()).is_none());
    }

    #[test]
    fn a_file_survives_a_round_trip_and_uses_the_documented_names() {
        let (_temp, store) = store();
        assert_eq!(store.load(), GameFile::default(), "absent : un état vide");
        let file = full_file();
        store.save(&file).unwrap();
        assert_eq!(store.load(), file);
        // Rien ne traîne à côté du fichier.
        let names: Vec<_> = fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["game.json"]);

        let document: Value =
            serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
        assert_eq!(document["version"], 1);
        assert_eq!(document["session"]["game"]["source"], "library");
        assert_eq!(document["session"]["power"]["outcome"], "applied");
        assert_eq!(
            document["session"]["power"]["previous"],
            "381b4222-f694-41f0-9685-ff5bb260df2e"
        );
        assert_eq!(document["session"]["power"]["applied"], "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c");
        assert_eq!(document["session"]["quieted"][0]["throttleControl"], 0);
        assert_eq!(document["session"]["restorePending"], false);
        assert_eq!(document["last"]["powerRestored"], true);
        assert_eq!(document["last"]["appsRestored"], 2);
        assert_eq!(document["seen"][0]["lastPlayed"], 1_794_772_491_613_i64);

        // Une partie sans changement d'alimentation n'écrit ni `previous` ni `applied`.
        let mut plain = file;
        plain.session.as_mut().unwrap().power = PowerRecord::of(PowerOutcome::OnBattery);
        store.save(&plain).unwrap();
        let document: Value =
            serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
        assert_eq!(document["session"]["power"], json!({ "outcome": "onBattery" }));
    }

    #[test]
    fn an_unreadable_file_is_an_empty_state() {
        let (_temp, store) = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        for text in ["", "{", "[1, 2]", "null", "\"texte\"", "{ \"session\": 12, \"seen\": {} }"] {
            fs::write(store.path(), text).unwrap();
            assert_eq!(store.load(), GameFile::default(), "{text}");
        }
        // Une marque d'ordre des octets est acceptée.
        fs::write(store.path(), "\u{feff}{ \"last\": { \"name\": \"Hades\" } }").unwrap();
        assert_eq!(store.load().last.unwrap().name, "Hades");
    }

    #[test]
    fn a_hostile_file_is_read_with_suspicion() {
        let (_temp, store) = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let many: Vec<Value> = (0..700)
            .map(|pid| json!({ "pid": pid, "started": 1, "exe": "a.exe", "priority": 32 }))
            .collect();
        let long = "x".repeat(1000);
        let document = json!({
            "version": 99,
            "session": {
                "game": { "path": long, "name": 5, "source": "n'importe quoi" },
                "since": "hier",
                "power": {
                    "outcome": "applied",
                    "previous": "{381b4222-f694-41f0-9685-ff5bb260df2e}",
                    "applied": "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"
                },
                "quieted": [
                    { "pid": 1, "started": 5, "exe": "a.exe", "priority": 256,
                      "throttleControl": 7, "throttleState": -1 },
                    { "pid": 2, "started": 5, "exe": "b.exe", "priority": 99999 },
                    { "pid": 3, "started": 5, "exe": "c.exe", "priority": 64,
                      "throttleControl": 1, "throttleState": 1 },
                    { "pid": -4, "started": 5 },
                    { "pid": 5 },
                    { "pid": 6, "started": 5, "priority": 16384 },
                    "pas un objet",
                    12
                ]
            }
        });
        fs::write(store.path(), document.to_string()).unwrap();
        let file = store.load();
        assert_eq!(file.version, GAME_VERSION, "la version du fichier ne s'impose pas");
        let session = file.session.expect("une partie abîmée garde ce qui se lit");
        assert_eq!(session.game.path.chars().count(), 260);
        assert_eq!(session.game.name, "");
        assert_eq!(session.game.source, Source::Library);
        assert_eq!(session.since, 0);
        assert_eq!(
            session.power,
            PowerRecord::of(PowerOutcome::Off),
            "un GUID mal formé : l'alimentation est ignorée"
        );
        let pids: Vec<u32> = session.quieted.iter().map(|q| q.pid).collect();
        assert_eq!(pids, [1, 2, 3, 6]);
        // Temps réel ou inconnu : remis à « normale ». Masques bornés à 0 ou 1.
        assert_eq!(session.quieted[0].priority, priority::NORMAL);
        assert_eq!(
            (session.quieted[0].throttle_control, session.quieted[0].throttle_state),
            (0, 0)
        );
        assert_eq!(session.quieted[1].priority, priority::NORMAL);
        assert_eq!(session.quieted[2].priority, priority::IDLE);
        assert_eq!(
            (session.quieted[2].throttle_control, session.quieted[2].throttle_state),
            (1, 1)
        );
        assert_eq!(session.quieted[3].priority, priority::BELOW_NORMAL);
        assert_eq!(session.quieted[3].exe, "");

        // Au plus 512 processus.
        let document = json!({ "session": { "quieted": many } });
        fs::write(store.path(), document.to_string()).unwrap();
        assert_eq!(store.load().session.unwrap().quieted.len(), QUIETED_KEPT);
    }

    #[test]
    fn seen_games_are_bounded_unique_and_most_recent_first() {
        let (_temp, store) = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let entries: Vec<Value> = (0..30)
            .map(|n| json!({ "path": format!(r"D:\Games\g{n}\g.exe"), "name": "g", "lastPlayed": 100 - n }))
            .chain([
                json!({ "path": r"d:\GAMES\g0\G.EXE", "lastPlayed": 5 }),
                json!({ "path": "", "lastPlayed": 5 }),
                json!({ "lastPlayed": 5 }),
                json!(7),
            ])
            .collect();
        fs::write(store.path(), json!({ "seen": entries }).to_string()).unwrap();
        let seen = store.load().seen;
        assert_eq!(seen.len(), SEEN_KEPT);
        assert_eq!(seen[0].path, r"D:\Games\g0\g.exe");
        assert_eq!(seen[0].last_played, 100, "le doublon plus bas est écarté");

        let mut file = GameFile::default();
        file.remember(r"D:\Games\a\a.exe", "A", 1);
        file.remember(r"D:\Games\b\b.exe", "B", 2);
        file.remember(r"d:\games\a\A.EXE", "A", 3);
        let paths: Vec<&str> = file.seen.iter().map(|seen| seen.path.as_str()).collect();
        assert_eq!(paths, [r"d:\games\a\A.EXE", r"D:\Games\b\b.exe"]);
        for n in 0..40 {
            file.remember(&format!(r"D:\Games\{n}\g.exe"), "g", n);
        }
        assert_eq!(file.seen.len(), SEEN_KEPT);
        assert_eq!(file.seen[0].last_played, 39);
    }

    #[test]
    fn a_failed_write_is_reported_and_leaves_the_old_file() {
        let (_temp, store) = store();
        let file = full_file();
        store.save(&file).unwrap();
        // Le « dossier » du fichier devient un fichier : plus rien ne s'y écrit.
        let blocked = GameStore::at(store.path().join("sous").join("game.json"));
        assert!(blocked.save(&file).is_err());
        assert_eq!(store.load(), file);
    }

    fn write_raw(store: &GameStore, text: &str) {
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), text).unwrap();
    }

    #[test]
    fn a_journal_without_opened_takes_the_start_of_the_game_as_its_opening() {
        let (_temp, store) = store();
        write_raw(
            &store,
            &json!({"session": {"game": {"path": r"D:\x\hp.exe", "name": "x", "source": "library"},
                "since": 42, "quieted": []}})
            .to_string(),
        );
        let session = store.load().session.expect("une partie");
        assert_eq!((session.opened, session.since), (42, 42));
        assert!(session.roots.is_empty());
    }

    #[test]
    fn roots_are_read_with_their_death_date_and_bounded() {
        let (_temp, store) = store();
        let roots: Vec<Value> =
            (0..(ROOTS_KEPT + 100)).map(|n| json!({"pid": n, "started": 5, "goneMs": 7})).collect();
        write_raw(
            &store,
            &json!({"session": {"game": {"path": r"D:\x\hp.exe", "name": "x", "source": "library"},
                "opened": 1, "since": 2, "quieted": [], "roots": roots}})
            .to_string(),
        );
        let session = store.load().session.expect("une partie");
        assert_eq!(session.roots.len(), ROOTS_KEPT);
        assert_eq!(session.roots[0].gone, Some(7));
        assert_eq!(session.opened, 1);
    }

    #[test]
    fn a_released_power_record_survives_a_round_trip() {
        let (_temp, store) = store();
        let mut file = full_file();
        if let Some(session) = file.session.as_mut() {
            session.power =
                PowerRecord { outcome: PowerOutcome::Applied, previous: None, applied: None };
        }
        store.save(&file).unwrap();
        let session = store.load().session.expect("une partie");
        assert_eq!(session.power.outcome, PowerOutcome::Applied);
        assert!(!session.power.is_held(), "rien à remettre : déjà remise");
    }

    #[test]
    fn a_huge_seen_list_loads_quickly_and_is_cut_to_the_limit() {
        let (_temp, store) = store();
        let seen: Vec<Value> = (0..12_000)
            .map(|n| json!({"path": format!(r"D:\Games\g{n}\g.exe"), "name": "g", "lastPlayed": n}))
            .collect();
        write_raw(&store, &json!({ "seen": seen }).to_string());
        let started = std::time::Instant::now();
        let file = store.load();
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "lecture trop lente");
        assert_eq!(file.seen.len(), SEEN_KEPT);
    }

    #[test]
    fn a_journal_over_one_mebibyte_is_treated_as_unreadable() {
        let (_temp, store) = store();
        let padding = " ".repeat(usize::try_from(MAX_FILE_BYTES).unwrap() + 10);
        write_raw(&store, &format!("{{\"seen\": []{padding}}}"));
        assert_eq!(store.load(), GameFile::default());
    }
}
