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
use shinku_core::clean::journal::{JournalStore, Run, RunState};
use shinku_core::clean::report::{CleanOutcome, DuplicatesStop, StepState, StopReason};
use shinku_core::clean::{CleanPhase, CleanRefusal, CleanRequest};
use shinku_core::fs::to_internal_path;
use shinku_core::game::PowerOutcome;
use shinku_core::game::control::FakeAgent;
use shinku_core::game::detect::Source;
use shinku_core::game::host::{FakeHost, PowerPlan};
use shinku_core::game::store::{GameFile, GameRef, GameStore, Last, PowerRecord, Quieted, Session};
use shinku_core::game::view::{GameOption, GameRefusal, GameStatus, PowerSupport};
use shinku_core::hash::MemoryCache;
use shinku_core::model::{Lot, ModuleId, RiskLevel, Timestamp, Trigger};
use shinku_core::platform::agent::{AgentState, OpenProgram, WindowsPage};
use shinku_core::platform::{Environment, KnownFolder, RecycleBin, StaticEnvironment, Volume};
use shinku_core::rules::JunkCategory;
use shinku_core::scan::report::{CategoryState, DuplicatesState, KeepChoice};
use shinku_core::scan::{Phase, ScanOptions};
use shinku_core::session::SessionState;
use shinku_core::settings::{AutoScan, Language, LoadIssue, SettingsStore, Theme};
use shinku_core::startup::host::{FakeStartupHost, StartupHost};
use shinku_core::startup::store::{StartupFile, StartupStore};
use shinku_core::startup::view::StartupRefusal;
use shinku_core::startup::{EntryId, Hint, Mark, Origin, Place, Scope, State, TargetKind};

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
    // Un petit PC : un fichier temporaire ancien, le cache d'un navigateur
    // resté ouvert, deux photos identiques, une vidéo et une corbeille.
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
        root.join("$Recycle.Bin"),
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
        .with_recycle_folder(root.join("$Recycle.Bin"))
        // Le navigateur tourne : son cache ne sera pas touché, et le grand
        // vide le dira (`openApps`, `leftOpen`).
        .with_process("chrome.exe")
        .with_folder(KnownFolder::Profile, profile.clone())
        .with_folder(KnownFolder::Documents, profile.join("Documents"))
        .with_folder(KnownFolder::Pictures, profile.join("Pictures"))
        .with_folder(KnownFolder::Videos, profile.join("Videos"))
        .with_folder(KnownFolder::LocalAppData, local.clone())
        .with_folder(KnownFolder::RoamingAppData, roaming)
        .with_folder(KnownFolder::Temp, local.join("Temp"));
    let store = SettingsStore::for_machine(&environment).expect("un emplacement pour les réglages");
    // La dernière fois, un grand vide a été coupé en route : l'accueil le dira.
    let journal = JournalStore::for_machine(&environment).expect("un emplacement pour le journal");
    journal.record(&Run::starting(environment.now())).expect("journal écrit");

    let game_store = GameStore::for_machine(&environment).expect("un emplacement pour game.json");
    let agent = FakeAgent::new(AgentState::Running);
    let open_program =
        |name: &str, path: &str| OpenProgram { name: name.to_owned(), path: path.to_owned() };
    agent.set_open_apps(vec![
        open_program("mycloud.exe", r"C:\Program Files\MyCloud\mycloud.exe"),
        open_program("foo.exe", r"C:\Games\Foo\foo.exe"),
        open_program("bar.exe", r"C:\Games\Bar\bar.exe"),
    ]);

    // Le démarrage : des entrées de chaque sorte, pour que la page n'ait ni
    // champ vide ni liste vide dans ses échantillons.
    let startup_host = Arc::new(FakeStartupHost::new());
    let own_command = r#""C:\Program Files\Shinku\shinku-agent.exe" --demarrage"#;
    startup_host.add_program(r"C:\Apps\Discord\app.exe", Some("Update"), Some("Discord Inc."));
    startup_host.add_program(r"C:\Apps\Steam\steam.exe", Some("Steam"), Some("Valve Corporation"));
    startup_host.add_program(
        r"C:\Apps\OneDrive\onedrive.exe",
        Some("Microsoft OneDrive"),
        Some("Microsoft Corporation"),
    );
    startup_host.add_program(r"C:\Apps\Spotify\spotify.exe", Some("Spotify"), None);
    startup_host.add_file(r"C:\Windows\System32\rundll32.exe");
    startup_host.add_file(r"C:\Windows\System32\SecurityHealthSystray.exe");
    for (place, name, command) in [
        (Place::UserRun, "Discord", r#""C:\Apps\Discord\app.exe" --start-minimized"#),
        (Place::UserRun, "Steam", r#""C:\Apps\Steam\steam.exe" -silent"#),
        (Place::UserRun, "OneDrive", r#""C:\Apps\OneDrive\onedrive.exe" /background"#),
        (Place::UserRun, "Spotify", r#""C:\Apps\Spotify\spotify.exe""#),
        (Place::UserRun, "Lancement rapide", "rundll32.exe shell32.dll,Control_RunDLL"),
        (Place::UserRun, "Shinku", own_command),
        (Place::UserRun, "Ancien outil", r"C:\Ancien\outil.exe"),
        (Place::UserRun32, "Outil 32 bits", r#""C:\Apps\Spotify\spotify.exe""#),
        (Place::UserFolder, "Mise à jour Adobe.lnk", r#""C:\Adobe\parti.exe""#),
        (Place::MachineRun, "SecurityHealth", r"%windir%\System32\SecurityHealthSystray.exe"),
        (Place::MachineRun32, "Mise à jour Java", r#""C:\Apps\Spotify\spotify.exe""#),
        (Place::MachineFolder, "Commun.lnk", r#""C:\Apps\Spotify\spotify.exe""#),
    ] {
        startup_host.add(place, name, command);
    }
    startup_host.set_variable("windir", r"C:\Windows");
    // Spotify s'est remise seule ; « Ancien outil » a un réglage que Shinku ne lit pas ;
    // « Sécurité Windows » est coupée dans Windows.
    startup_host.set_bytes(Place::UserRun, "Ancien outil", &[6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let stamp = startup_host.file_time_now();
    let mut cut = vec![3, 0, 0, 0];
    cut.extend_from_slice(&stamp.to_le_bytes());
    startup_host.set_bytes(Place::MachineRun, "SecurityHealth", &cut);
    startup_host.make_unreadable(Place::MachineFolder);
    let startup_store =
        StartupStore::for_machine(&environment).expect("un emplacement pour startup.json");
    let mut cut_file = StartupFile::default();
    cut_file.remember(EntryId::new(Place::UserRun, "Spotify"), "Spotify", 1_794_000_000_000, stamp);
    startup_store.save(&cut_file).expect("startup.json écrit");

    let mut options = ScanOptions::new(Trigger::Button);
    options.duplicate_min_size = 4096;
    options.large_threshold = 32 * 1024;
    let backend = Backend::new(Arc::new(environment), Arc::new(MemoryCache::new()), Some(store))
        .with_scan_defaults(options)
        .with_journal(journal)
        .with_game_parts(
            Arc::new(FakeHost::new()),
            Arc::new(agent.clone()),
            Some(GameStore::at(game_store.path())),
        )
        .with_startup_parts(
            startup_host.clone(),
            Some(StartupStore::at(startup_store.path())),
            Some(own_command.to_owned()),
        );

    let mut samples: BTreeMap<&str, Value> = BTreeMap::new();
    samples.insert("sessionView.idle", to_json(&backend.view()));

    // Le démarrage : Steam est coupée d'un geste, puis la vue est celle d'après.
    let before = to_json(&backend.startup_view());
    let steam = before["entries"]
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["name"] == "Steam"))
        .and_then(|entry| entry["id"].as_str())
        .expect("l'entrée Steam")
        .to_owned();
    let view = backend.set_startup_entry(&steam, false).expect("Steam est coupée");
    samples.insert("startupView", to_json(&view));

    // Le mode jeu : des listes pleines, pour que ni les réglages ni la page
    // n'aient de liste vide dans leurs échantillons.
    backend.set_game_option(GameOption::Auto, true).expect("mode automatique");
    backend.set_game_option(GameOption::Power, true).expect("alimentation");
    backend.set_game_option(GameOption::QuietApps, true).expect("veilleuse");
    backend.set_quiet_app("onedrive.exe", false).expect("app décochée");
    let open = backend.open_apps();
    samples.insert("openApps", to_json(&open));
    backend.add_quiet_app(open[0].id, &open[0].path).expect("app ajoutée");
    backend.add_game(open[1].id, &open[1].path).expect("jeu ajouté");
    backend.add_game(open[2].id, &open[2].path).expect("jeu ajouté");
    backend.set_game_status(&open[2].path, GameStatus::NotGame).expect("jeu écarté");
    let mut file = GameFile::default();
    file.remember(r"D:\SteamLibrary\steamapps\common\Baz\baz.exe", "Baz", 1_700_000_000_000);
    file.remember(&open[1].path, "Foo", 1_700_000_100_000);
    game_store.save(&file).expect("game.json écrit");
    samples.insert("gameView.idle", to_json(&backend.game_view()));

    // Une partie en cours, et le bilan de la précédente.
    file.session = Some(Session {
        game: GameRef { path: open[1].path.clone(), name: "Foo".to_owned(), source: Source::Added },
        since: 1_700_000_200_000,
        released: false,
        power: PowerRecord::applied(PowerPlan::BALANCED),
        quieted: vec![Quieted {
            pid: 4242,
            started: 133_000_000_000_000_000,
            exe: "onedrive.exe".to_owned(),
            priority: 0x20,
            throttle_control: 0,
            throttle_state: 0,
        }],
        restore_pending: false,
    });
    file.last = Some(Last {
        name: "Bar".to_owned(),
        path: open[2].path.clone(),
        started: 1_700_000_000_000,
        ended: 1_700_000_100_000,
        power_restored: Some(true),
        apps_restored: 2,
        complete: true,
    });
    game_store.save(&file).expect("game.json écrit");
    samples.insert("gameView.playing", to_json(&backend.game_view()));
    file.session = None;
    game_store.save(&file).expect("game.json écrit");

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
    let duplicates = backend.duplicates(0, 10).expect("doublons");
    samples.insert("duplicates", to_json(&duplicates));

    // Le choix de l'exemplaire gardé : l'autre demande que la fenêtre envoie.
    let group = duplicates.groups.first().expect("un groupe de doublons");
    let keep_choice =
        KeepChoice { digest: group.digest.clone(), size: group.size, index: 0, whole_folder: true };
    samples.insert("keepChoice", to_json(&keep_choice));
    assert!(backend.keep_duplicate(&keep_choice), "le choix désigne un exemplaire du rapport");

    // Le grand vide : l'écran de confirmation, puis le bilan.
    let preview = backend.clean_preview().expect("écran de confirmation");
    samples.insert("cleanPreview", to_json(&preview));
    let request = CleanRequest {
        analysis: preview.analysis,
        recycle_bin: true,
        junk_files: true,
        duplicates: true,
    };
    // La demande est la seule chose que la fenêtre envoie au cœur : sa forme
    // fait partie du contrat, elle aussi.
    samples.insert("cleanRequest", to_json(&request));
    backend.start_clean(request).expect("grand vide lancé");
    let cleaned = loop {
        let view = backend.view();
        if view.state == SessionState::Cleaned {
            break view;
        }
        assert!(Instant::now() < deadline, "pas de bilan : {:?}", view.state);
        std::thread::sleep(Duration::from_millis(5));
    };
    samples.insert("sessionView.cleaned", to_json(&cleaned));
    // L'arrêt de sécurité de l'étape « Doublons », absent d'un grand vide
    // qui se passe bien.
    samples.insert(
        "cleanStop",
        to_json(&DuplicatesStop {
            reason: StopReason::KeptCopyChanged,
            file: profile.join("Pictures").join("photo.jpg"),
        }),
    );

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
    samples.insert("error.cleanRunning", to_json(&BackendError::CleanRunning));
    samples.insert(
        "error.cleanRefused",
        to_json(&BackendError::CleanRefused { reason: CleanRefusal::StaleResults }),
    );
    samples.insert(
        "error.cleanNotStarted",
        to_json(&BackendError::CleanNotStarted { reason: "raison".to_owned() }),
    );

    samples.insert(
        "error.gameRefused",
        to_json(&BackendError::GameRefused { reason: GameRefusal::UnknownGame }),
    );

    samples.insert(
        "error.startupRefused",
        to_json(&BackendError::StartupRefused { reason: StartupRefusal::NotAdjustable }),
    );

    // Les valeurs de chaque énumération.
    let mut enums: BTreeMap<&str, Value> = BTreeMap::new();
    enums.insert(
        "phase",
        variants!(Phase: Preparing, Walking, DuplicatesQuick, DuplicatesFull, Done, Cancelled),
    );
    enums.insert(
        "sessionState",
        variants!(SessionState: Idle, Scanning, Results, Failed, Cleaning, Cleaned),
    );
    enums.insert(
        "moduleId",
        variants!(ModuleId: JunkFiles, Duplicates, LargeFiles, RecycleBin, Startup, GameMode, Uninstall, Privacy),
    );
    enums.insert("lot", variants!(Lot: Auto, Optional, Review));
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
    enums.insert(
        "cleanPhase",
        variants!(CleanPhase: Preparing, RecycleBin, JunkFiles, Duplicates, Done),
    );
    enums.insert(
        "cleanRefusal",
        variants!(CleanRefusal: NoResults, StaleResults, OtherAnalysis, NothingToClean),
    );
    enums.insert("cleanOutcome", variants!(CleanOutcome: Completed, Cancelled));
    enums.insert("stepState", variants!(StepState: NotRequested, Done, Stopped, Failed));
    enums.insert("stopReason", variants!(StopReason: NotInRecycleBin, KeptCopyChanged));
    enums.insert("runState", variants!(RunState: Running, Completed, Cancelled, Interrupted));
    enums.insert("agentState", variants!(AgentState: Running, Stopped, Missing));
    enums.insert(
        "powerOutcome",
        variants!(PowerOutcome: Applied, AlreadyFast, OnBattery, Unavailable, Unsuited, Custom, Refused, Off),
    );
    enums.insert("powerSupport", variants!(PowerSupport: Available, Unavailable, Unsuited));
    enums.insert("gameSource", variants!(Source: Added, Library));
    enums.insert("windowsPage", variants!(WindowsPage: GameMode, Power, StartupApps));
    enums.insert("startupScope", variants!(Scope: User, Machine));
    enums.insert("startupState", variants!(State: Enabled, Disabled, Unknown));
    enums.insert("startupTarget", variants!(TargetKind: Found, Missing, Hosted, Unknown));
    enums.insert("startupOrigin", variants!(Origin: Registry, Folder));
    enums.insert("startupHint", variants!(Hint: Sync));
    enums.insert(
        "startupRefusal",
        variants!(StartupRefusal: UnknownEntry, NotAdjustable, Busy, Failed),
    );
    // Les marques portent des champs : le `match` veille à ce qu'il n'en manque pas.
    fn every_mark_has_a_name(mark: Mark) {
        match mark {
            Mark::CutByShinku { .. } | Mark::CameBack { .. } => {}
        }
    }
    let _ = every_mark_has_a_name;
    enums.insert(
        "startupMark",
        Value::Array(vec![
            name(&Mark::CutByShinku { at_ms: 0 }),
            name(&Mark::CameBack { cut_ms: 0 }),
        ]),
    );
    enums.insert("gameStatus", variants!(GameStatus: NotGame, Forgotten));
    enums.insert("gameOption", variants!(GameOption: Auto, Power, QuietApps));
    enums.insert(
        "gameRefusal",
        variants!(GameRefusal: UnknownApp, UnknownGame, NotAGame, ProtectedApp, ListFull),
    );
    // Les énumérations dont les variantes portent des champs : la liste vient
    // des échantillons ci-dessus, et le `match` veille à ce qu'il n'en manque pas.
    fn every_error_has_a_sample(error: BackendError) {
        match error {
            BackendError::SettingsUnchecked
            | BackendError::ScanNotStarted { .. }
            | BackendError::SettingsNotSaved { .. }
            | BackendError::FolderRefused { .. }
            | BackendError::CleanRunning
            | BackendError::CleanRefused { .. }
            | BackendError::CleanNotStarted { .. }
            | BackendError::GameRefused { .. }
            | BackendError::StartupRefused { .. } => {}
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
