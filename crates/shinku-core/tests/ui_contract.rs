//! Le contrat entre le cœur et l'interface : la forme du JSON échangé.
//!
//! L'interface est écrite en TypeScript. Rien ne relie ses types à ceux du
//! cœur au moment de compiler : si un champ change de nom ici, l'interface
//! afficherait du vide sans que personne ne le voie avant de lancer Shinku.
//!
//! Ce test note donc, dans `apps/shinku/tests/contract/contract.json`, la forme
//! de tout ce que le guichet peut rendre et les valeurs de chaque énumération.
//! Les tests de l'interface relisent ce même fichier et le comparent à ce
//! qu'elle attend. Quand le contrat change volontairement :
//!
//! ```text
//! SHINKU_UPDATE_CONTRACT=1 cargo test -p shinku-core --test ui_contract
//! ```
//!
//! puis on adapte l'interface jusqu'à ce que `npm test` repasse.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};
use shinku_core::backend::{Backend, BackendError, FolderRefusal};
use shinku_core::fs::to_internal_path;
use shinku_core::hash::MemoryCache;
use shinku_core::model::{Lot, ModuleId, RiskLevel, Timestamp, Trigger};
use shinku_core::platform::{KnownFolder, RecycleBin, StaticEnvironment, Volume};
use shinku_core::rules::JunkCategory;
use shinku_core::scan::report::{CategoryState, DuplicatesState};
use shinku_core::scan::{Phase, ScanOptions};
use shinku_core::session::SessionState;
use shinku_core::settings::{AutoScan, Language, LoadIssue, SettingsStore, Theme};

/// Le contrat connu, lu à la compilation : le test ne dépend d'aucun chemin
/// au moment où il tourne.
const KNOWN_CONTRACT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/shinku/tests/contract/contract.json"
));

/// Le nom que porte une variante dans le JSON.
fn name<T: serde::Serialize>(variant: &T) -> Value {
    match to_json(variant) {
        Value::String(name) => Value::String(name),
        // Une variante qui porte des champs se reconnaît à son champ `kind`.
        Value::Object(fields) => fields["kind"].clone(),
        other => panic!("une variante se sérialise en texte ou en objet, pas en {other}"),
    }
}

/// Toutes les variantes d'une énumération, telles que le JSON les nomme.
///
/// Le `match` cesse de compiler dès qu'une variante est ajoutée au cœur : il
/// rappelle de la déclarer ici, donc de la faire entrer dans le contrat.
macro_rules! variants {
    ($type:ty: $($variant:ident),+ $(,)?) => {{
        fn complete(value: $type) {
            match value {
                $(<$type>::$variant => {})+
            }
        }
        let _ = complete;
        Value::Array(vec![$(name(&<$type>::$variant)),+])
    }};
}

/// La forme d'une valeur JSON : ses champs et leur nature, sans leur contenu.
/// Une liste est décrite par la forme commune à tous ses éléments.
fn shape(value: &Value) -> Value {
    match value {
        Value::Null => json!("null"),
        Value::Bool(_) => json!("boolean"),
        Value::Number(_) => json!("number"),
        Value::String(_) => json!("string"),
        Value::Array(items) => {
            Value::Array(items.iter().map(shape).reduce(merge).into_iter().collect())
        }
        Value::Object(fields) => {
            Value::Object(fields.iter().map(|(name, field)| (name.clone(), shape(field))).collect())
        }
    }
}

/// Réunit deux formes observées pour la même valeur : une valeur absente ou
/// une liste vide n'apprend rien, l'autre forme l'emporte.
fn merge(left: Value, right: Value) -> Value {
    match (left, right) {
        (left, right) if left == right => left,
        (Value::String(kind), other) | (other, Value::String(kind)) if kind == "null" => other,
        (Value::Array(left), Value::Array(right)) => {
            Value::Array(left.into_iter().chain(right).reduce(merge).into_iter().collect())
        }
        (Value::Object(mut left), Value::Object(right)) => {
            for (name, field) in right {
                let merged = match left.remove(&name) {
                    Some(known) => merge(known, field),
                    None => field,
                };
                left.insert(name, merged);
            }
            Value::Object(left)
        }
        _ => json!("mixed"),
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("une valeur du guichet se sérialise toujours")
}

#[test]
fn the_json_sent_to_the_window_keeps_its_shape() {
    // Un petit PC : un fichier temporaire ancien, un cache de navigateur,
    // deux photos identiques et une vidéo.
    let workspace = tempfile::tempdir().expect("dossier temporaire");
    let root = to_internal_path(&fs::canonicalize(workspace.path()).expect("chemin réel"))
        .expect("chemin interne");
    let profile = root.join("Users").join("a");
    let local = profile.join("AppData").join("Local");
    let roaming = profile.join("AppData").join("Roaming");
    let chrome_cache =
        local.join("Google").join("Chrome").join("User Data").join("Default").join("Cache");
    for folder in [
        local.join("Temp"),
        chrome_cache.clone(),
        roaming.clone(),
        profile.join("Documents"),
        profile.join("Pictures"),
        profile.join("Videos"),
    ] {
        fs::create_dir_all(folder).expect("dossier du faux PC");
    }
    fs::write(local.join("Temp").join("vieux.tmp"), [7u8; 9000]).expect("fichier temporaire");
    // Une règle qui demande de fermer une app d'abord : `closeFirst` n'est pas vide.
    fs::write(chrome_cache.join("f_000001"), [5u8; 5000]).expect("cache du navigateur");
    fs::write(profile.join("Documents").join("photo.jpg"), [3u8; 8000]).expect("photo");
    fs::write(profile.join("Pictures").join("photo.jpg"), [3u8; 8000]).expect("photo");
    fs::write(profile.join("Videos").join("film.mkv"), [9u8; 70_000]).expect("vidéo");

    let month = Duration::from_secs(30 * 24 * 3600);
    let environment = StaticEnvironment::new()
        .with_now(Timestamp::from_system_time(SystemTime::now() + month))
        .with_parallelism(2)
        .with_volume(Volume {
            root: root.clone(),
            label: "Système".to_owned(),
            file_system: "NTFS".to_owned(),
            total: 1_000_000,
            free: 400_000,
            fixed: true,
            seek_penalty: Some(false),
            system: true,
        })
        .with_recycle_bin(RecycleBin { bytes: 4096, items: 3 })
        .with_folder(KnownFolder::Profile, profile.clone())
        .with_folder(KnownFolder::Documents, profile.join("Documents"))
        .with_folder(KnownFolder::Pictures, profile.join("Pictures"))
        .with_folder(KnownFolder::Videos, profile.join("Videos"))
        .with_folder(KnownFolder::LocalAppData, local.clone())
        .with_folder(KnownFolder::RoamingAppData, roaming)
        .with_folder(KnownFolder::Temp, local.join("Temp"));
    let store = SettingsStore::for_machine(&environment).expect("un emplacement pour les réglages");

    let mut options = ScanOptions::new(Trigger::Button);
    options.duplicate_min_size = 4096;
    options.large_threshold = 32 * 1024;
    let backend = Backend::new(Arc::new(environment), Arc::new(MemoryCache::new()), Some(store))
        .with_scan_defaults(options);

    let mut samples: BTreeMap<&str, Value> = BTreeMap::new();
    samples.insert("sessionView.idle", to_json(&backend.view()));

    // Un dossier jamais touché, pour que la liste des réglages ne soit pas vide.
    let settings = backend.exclude_folder(&profile.join("Documents")).expect("dossier exclu");
    samples.insert("settings", to_json(&settings));
    samples.insert("overview", to_json(&backend.overview()));
    // Comme la fenêtre : on rend le dossier tel que la liste des réglages
    // l'écrit, ce qui sous Windows n'est pas l'écriture interne du chemin.
    let given_back = backend.include_folder(&settings.excluded_folders[0]).expect("dossier rendu");
    assert!(given_back.excluded_folders.is_empty(), "le dossier est resté exclu");

    backend.start_scan().expect("analyse lancée");
    let deadline = Instant::now() + Duration::from_secs(60);
    let results = loop {
        let view = backend.view();
        if view.state == SessionState::Results {
            break view;
        }
        assert!(Instant::now() < deadline, "pas de résultats : {:?}", view.state);
        std::thread::sleep(Duration::from_millis(5));
    };
    samples.insert("sessionView.results", to_json(&results));
    samples.insert("junk", to_json(&backend.junk().expect("fichiers inutiles")));
    samples.insert("largeFiles", to_json(&backend.large_files(0, 10).expect("gros fichiers")));
    samples.insert("duplicates", to_json(&backend.duplicates(0, 10).expect("doublons")));

    // Les deux façons dont un fichier de réglages peut être abîmé.
    samples.insert(
        "loadIssue.unreadable",
        to_json(&LoadIssue::Unreadable { reason: "illisible".to_owned() }),
    );
    samples.insert("loadIssue.partial", to_json(&LoadIssue::Partial { fields: vec!["theme"] }));

    // Tous les refus possibles.
    samples.insert("error.settingsUnchecked", to_json(&BackendError::SettingsUnchecked));
    samples.insert(
        "error.scanNotStarted",
        to_json(&BackendError::ScanNotStarted { reason: "raison".to_owned() }),
    );
    samples.insert(
        "error.settingsNotSaved",
        to_json(&BackendError::SettingsNotSaved { reason: "raison".to_owned() }),
    );
    samples.insert(
        "error.folderRefused",
        to_json(&BackendError::FolderRefused { reason: FolderRefusal::NotAFolder }),
    );

    // Les valeurs de chaque énumération.
    let mut enums: BTreeMap<&str, Value> = BTreeMap::new();
    enums.insert(
        "phase",
        variants!(Phase: Preparing, Walking, DuplicatesQuick, DuplicatesFull, Done, Cancelled),
    );
    enums.insert("sessionState", variants!(SessionState: Idle, Scanning, Results, Failed));
    enums.insert(
        "moduleId",
        variants!(ModuleId: JunkFiles, Duplicates, LargeFiles, RecycleBin, Startup, GameMode, Uninstall, Privacy),
    );
    enums.insert("lot", variants!(Lot: Auto, Review));
    enums.insert("risk", variants!(RiskLevel: A, B, C, D));
    enums.insert("categoryState", variants!(CategoryState: Ready, Pending, Unavailable));
    enums.insert(
        "junkCategory",
        variants!(JunkCategory: AppCaches, TempFiles, UpdateLeftovers, LogsAndReports),
    );
    enums.insert("duplicatesState", variants!(DuplicatesState: Done, Pending, Off));
    enums.insert("theme", variants!(Theme: System, Light, Dark));
    enums.insert("autoScan", variants!(AutoScan: Never, Weekly, Monthly));
    enums.insert("language", variants!(Language: Fr, En));
    enums.insert("folderRefusal", variants!(FolderRefusal: NotAbsolute, NotAFolder, NotUnicode));
    // Les énumérations dont les variantes portent des champs : la liste vient
    // des échantillons ci-dessus, et le `match` veille à ce qu'il n'en manque pas.
    fn every_error_has_a_sample(error: BackendError) {
        match error {
            BackendError::SettingsUnchecked
            | BackendError::ScanNotStarted { .. }
            | BackendError::SettingsNotSaved { .. }
            | BackendError::FolderRefused { .. } => {}
        }
    }
    fn every_issue_has_a_sample(issue: LoadIssue) {
        match issue {
            LoadIssue::Unreadable { .. } | LoadIssue::Partial { .. } => {}
        }
    }
    let _ = (every_error_has_a_sample, every_issue_has_a_sample);
    let kinds = |prefix: &str| -> Value {
        samples
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .map(|(_, sample)| sample["kind"].clone())
            .collect()
    };
    enums.insert("backendError", kinds("error."));
    enums.insert("loadIssue", kinds("loadIssue."));

    // Les échantillons doivent montrer des listes pleines : une liste vide ne dit rien de sa forme.
    let shapes: BTreeMap<&str, Value> =
        samples.iter().map(|(name, value)| (*name, shape(value))).collect();
    let contract = json!({ "enums": enums, "shapes": shapes });
    let text = format!("{}\n", serde_json::to_string_pretty(&contract).expect("texte JSON"));

    // Les échantillons doivent montrer des listes pleines : une liste vide ne
    // dit rien de sa forme, et une forme « mixed » trahit un échantillon mal choisi.
    assert!(!text.contains("[]"), "une liste est restée vide dans les échantillons :\n{text}");
    assert!(
        !text.contains("\"mixed\""),
        "deux éléments d'une même liste n'ont pas la même forme :\n{text}"
    );

    if std::env::var_os("SHINKU_UPDATE_CONTRACT").is_some() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/shinku/tests/contract/contract.json");
        fs::write(&path, &text).expect("écriture du contrat");
        return;
    }
    assert_eq!(
        KNOWN_CONTRACT.replace("\r\n", "\n"),
        text,
        "Le JSON rendu à l'interface a changé. Si c'est voulu : \
         SHINKU_UPDATE_CONTRACT=1 cargo test -p shinku-core --test ui_contract, \
         puis adapter l'interface (apps/shinku/src/backend/types.ts)."
    );
}
