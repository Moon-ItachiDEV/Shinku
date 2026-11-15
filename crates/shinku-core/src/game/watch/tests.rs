//! Les comportements de la machine à états, un par un, avec une fausse machine.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use super::*;
use crate::game::STEP;
use crate::game::host::{FakeHost, Gesture};

const GAME: &str = r"D:\SteamLibrary\steamapps\common\Hollow Peaks\bin\hp.exe";
const OTHER_GAME: &str = r"E:\Games\Hades\Hades.exe";
const TWIN: &str = r"C:\Autre\hp.exe";
const NOTEPAD: &str = r"C:\Windows\System32\notepad.exe";
const ONEDRIVE: &str = r"C:\Users\a\AppData\Local\Microsoft\OneDrive\OneDrive.exe";
const DROPBOX: &str = r"C:\Program Files\Dropbox\Client\Dropbox.exe";
const GOOGLE_DRIVE: &str = r"C:\Program Files\Google\Drive File Stream\GoogleDriveFS.exe";
const CHROME: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";

fn normal() -> ProcessState {
    ProcessState { priority: priority::NORMAL, throttle_control: 0, throttle_state: 0 }
}

fn low() -> ProcessState {
    ProcessState { priority: priority::BELOW_NORMAL, throttle_control: 1, throttle_state: 1 }
}

/// Un PC, un journal, une machine à états, et les réglages d'une personne qui
/// a allumé le mode jeu automatique.
struct Bench {
    dir: tempfile::TempDir,
    host: FakeHost,
    store: GameStore,
    watcher: Watcher,
    settings: Settings,
}

impl Bench {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = GameStore::at(dir.path().join("Shinku").join("game.json"));
        Self::on(dir, store)
    }

    fn on(dir: tempfile::TempDir, store: GameStore) -> Self {
        let host = FakeHost::new();
        let watcher = Watcher::new(Arc::new(host.clone()), store.clone());
        let settings = Settings { game_mode_auto: true, ..Settings::default() };
        Self { dir, host, store, watcher, settings }
    }

    /// Un journal qui ne peut pas s'écrire : son dossier est un fichier.
    fn blocked() -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Shinku"), b"x").unwrap();
        let store = GameStore::at(dir.path().join("Shinku").join("game.json"));
        Self::on(dir, store)
    }

    fn unblock(&self) {
        fs::remove_file(self.dir.path().join("Shinku")).unwrap();
    }

    /// Un pas : deux secondes passent, puis la machine à états regarde.
    fn tick(&mut self) -> Activity {
        self.host.advance(STEP);
        self.watcher.tick(&self.settings)
    }

    fn ticks(&mut self, count: u32) {
        for _ in 0..count {
            self.tick();
        }
    }

    fn file(&self) -> GameFile {
        self.store.load()
    }

    fn session(&self) -> Session {
        self.file().session.expect("une partie est ouverte")
    }

    /// Lance un programme (le parent est un processus quelconque).
    fn app(&self, pid: u32, path: &str) -> ProcessRef {
        self.host.spawn(pid, 1, path)
    }

    /// Lance le jeu et le met au premier plan.
    fn launch(&self) -> ProcessRef {
        let game = self.app(100, GAME);
        self.host.set_foreground(Some(game));
        game
    }

    /// Lance le jeu et attend que la partie soit ouverte.
    fn play(&mut self) -> ProcessRef {
        let game = self.launch();
        assert_eq!(self.tick(), Activity::Candidate);
        assert_eq!(self.tick(), Activity::Playing);
        game
    }

    fn state(&self, process: ProcessRef) -> ProcessState {
        self.host.state_of(process).expect("le processus vit")
    }

    fn power(&self) -> PowerOutcome {
        self.session().power.outcome
    }

    /// Ferme le jeu et laisse la machine à états s'en apercevoir.
    fn quit(&mut self, game: ProcessRef) -> Activity {
        self.host.kill(game);
        self.tick()
    }
}

fn exes(session: &Session) -> Vec<String> {
    let mut names: Vec<String> = session.quieted.iter().map(|entry| entry.exe.clone()).collect();
    names.sort();
    names
}

// ---------------------------------------------------------------- 1. éteint

#[test]
fn nothing_is_detected_while_the_mode_is_off() {
    let mut bench = Bench::new();
    bench.settings.game_mode_auto = false;
    bench.launch();
    for _ in 0..6 {
        assert_eq!(bench.tick(), Activity::Idle);
    }
    assert!(!bench.watcher.has_session());
    assert!(bench.host.gestures().is_empty());
    assert!(!bench.store.path().exists(), "rien n'est écrit");
}

#[test]
fn switching_the_mode_off_ends_the_session_and_puts_everything_back() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    bench.play();
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));
    assert_eq!(bench.state(onedrive), low());

    bench.settings.game_mode_auto = false;
    assert_eq!(bench.tick(), Activity::Idle);
    assert!(!bench.watcher.has_session());
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    assert_eq!(bench.state(onedrive), normal());
    let file = bench.file();
    assert_eq!(file.session, None);
    assert!(file.last.expect("un bilan").complete);
}

// -------------------------------------------------------------- 2. candidat

#[test]
fn a_game_must_stay_in_front_for_two_steps() {
    let mut bench = Bench::new();
    let game = bench.launch();
    assert_eq!(bench.tick(), Activity::Candidate);
    assert!(!bench.store.path().exists(), "un candidat ne change rien");
    assert!(bench.host.gestures().is_empty());

    // Un autre programme prend le premier plan : on repart de zéro.
    let notepad = bench.app(300, NOTEPAD);
    bench.host.set_foreground(Some(notepad));
    assert_eq!(bench.tick(), Activity::Idle);
    bench.host.set_foreground(Some(game));
    assert_eq!(bench.tick(), Activity::Candidate);
    assert!(!bench.watcher.has_session());
    assert_eq!(bench.tick(), Activity::Playing);
    assert!(bench.watcher.has_session());
}

#[test]
fn two_different_games_in_turn_never_make_a_candidate() {
    let mut bench = Bench::new();
    let first = bench.app(100, GAME);
    let second = bench.app(101, OTHER_GAME);
    for _ in 0..4 {
        bench.host.set_foreground(Some(first));
        assert_eq!(bench.tick(), Activity::Candidate);
        bench.host.set_foreground(Some(second));
        assert_eq!(bench.tick(), Activity::Candidate);
    }
    assert!(!bench.watcher.has_session());
}

#[test]
fn a_program_that_is_not_a_game_is_never_a_candidate() {
    let mut bench = Bench::new();
    let chrome = bench.app(100, CHROME);
    bench.host.set_foreground(Some(chrome));
    assert_eq!(bench.tick(), Activity::Idle);
    assert_eq!(bench.tick(), Activity::Idle);
    // Sans rien au premier plan non plus.
    bench.host.set_foreground(None);
    assert_eq!(bench.tick(), Activity::Idle);
}

#[test]
fn a_game_added_by_the_person_is_recognised_anywhere() {
    let mut bench = Bench::new();
    let javaw = r"C:\Program Files\Java\bin\javaw.exe";
    bench.settings.game_mode_games = vec![PathBuf::from(javaw)];
    let process = bench.app(100, javaw);
    bench.host.set_foreground(Some(process));
    bench.ticks(2);
    let session = bench.session();
    assert_eq!(session.game.source, Source::Added);
    assert_eq!(session.game.name, "javaw", "le nom d'un jeu ajouté vient de son fichier");
    // Un jeu ajouté n'est pas noté dans les jeux vus : il est dans les réglages.
    bench.host.kill(process);
    bench.tick();
    assert!(bench.file().seen.is_empty());
}

// ----------------------------------------------------- 3. identité par chemin

#[test]
fn the_game_is_its_executable_path_not_a_pid() {
    let mut bench = Bench::new();
    let first = bench.play();
    let since = bench.session().since;

    // Le jeu se relance sous un autre pid : c'est toujours le même jeu.
    bench.host.kill(first);
    let second = bench.app(200, GAME);
    bench.host.set_foreground(Some(second));
    assert_eq!(bench.tick(), Activity::Playing);
    assert_eq!(bench.session().since, since);
    assert!(bench.host.opened().is_empty(), "le jeu au premier plan est connu sans l'ouvrir");

    // Un programme du même nom, d'un autre dossier, n'est pas le jeu.
    let twin = bench.app(300, TWIN);
    let notepad = bench.app(301, NOTEPAD);
    bench.host.set_foreground(Some(notepad));
    bench.ticks(5);
    assert!(bench.watcher.has_session());
    assert!(
        bench.host.opened().is_empty(),
        "un jeu déjà vérifié n'est pas rouvert à chaque pas : {:?}",
        bench.host.opened()
    );

    bench.host.kill(second);
    assert_eq!(bench.tick(), Activity::Idle, "il ne reste que le programme de même nom");
    bench.ticks(3);
    assert_eq!(
        bench.host.opened(),
        [twin.pid],
        "seul le processus de même nom est ouvert, et une seule fois"
    );
}

#[test]
fn only_processes_with_the_name_of_the_game_are_opened() {
    let mut bench = Bench::new();
    // Pas d'autre jeu au premier plan : le jeu n'est vérifié qu'en l'ouvrant.
    for pid in 10..40 {
        bench.app(pid, NOTEPAD);
    }
    let game = bench.play();
    bench.host.set_foreground(None);
    // Un nouveau suivi du même journal, sans mémoire des pid déjà vérifiés.
    let mut again = Watcher::new(Arc::new(bench.host.clone()), bench.store.clone());
    again.recover();
    assert!(again.has_session());
    assert_eq!(bench.host.opened(), [game.pid]);
}

// ------------------------------------------------------------ 4. activation

#[test]
fn the_journal_is_written_before_anything_is_changed() {
    let mut bench = Bench::new();
    bench.host.spy_on(bench.store.path());
    bench.app(200, ONEDRIVE);
    bench.app(201, DROPBOX);
    bench.play();

    let gestures = bench.host.gestures();
    assert_eq!(gestures.len(), 3, "l'alimentation, puis chaque processus : {gestures:?}");
    assert_eq!(gestures[0], Gesture::SetPowerPlan(PowerPlan::HIGH_PERFORMANCE));
    assert!(gestures[1..].iter().all(|gesture| matches!(gesture, Gesture::Apply { .. })));
    let spied = bench.host.spied();
    assert_eq!(spied.len(), 3);
    for text in spied {
        // À chaque geste, la session complète est déjà au journal.
        let document: Value = serde_json::from_str(&text.expect("le journal existe")).unwrap();
        assert_eq!(document["session"]["power"]["outcome"], "applied");
        assert_eq!(document["session"]["power"]["previous"], PowerPlan::BALANCED.to_string());
        assert_eq!(document["session"]["quieted"].as_array().unwrap().len(), 2);
        assert_eq!(document["session"]["game"]["name"], "Hollow Peaks");
    }
    let session = bench.session();
    assert_eq!(session.quieted[0].priority, priority::NORMAL, "l'état d'avant est noté");
    assert!(!session.released && !session.restore_pending);
}

#[test]
fn if_the_journal_cannot_be_written_nothing_is_changed() {
    let mut bench = Bench::blocked();
    let onedrive = bench.app(200, ONEDRIVE);
    bench.launch();
    assert_eq!(bench.tick(), Activity::Candidate);
    assert_eq!(bench.tick(), Activity::Candidate, "le journal refuse : pas de partie");
    assert!(bench.host.gestures().is_empty());
    assert_eq!(bench.state(onedrive), normal());
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));

    // Même si le disque redevient bon, on n'insiste pas avant 60 secondes.
    let failed = bench.host.now();
    bench.unblock();
    while bench.tick() != Activity::Playing {
        let waited = bench.host.now().since(failed);
        assert!(waited < Duration::from_secs(70), "la partie ne s'ouvre jamais");
    }
    let waited = bench.host.now().since(failed);
    assert!(waited >= WRITE_RETRY, "réessayé trop tôt : {waited:?}");
    assert_eq!(bench.state(onedrive), low());
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));
}

#[test]
fn a_process_that_cannot_be_read_or_changed_is_left_aside() {
    let mut bench = Bench::new();
    let fine = bench.app(200, ONEDRIVE);
    let locked = bench.app(201, DROPBOX);
    bench.host.set_writable(locked, false);
    let hidden = bench.app(202, GOOGLE_DRIVE);
    bench.host.set_readable(hidden, false);
    bench.play();

    // Le processus illisible n'est pas listé ; celui qu'on ne peut pas
    // modifier l'est le temps d'un pas, puis il est retiré en silence.
    assert_eq!(exes(&bench.session()), ["dropbox.exe", "onedrive.exe"]);
    assert_eq!(bench.state(fine), low());
    assert_eq!(bench.state(locked), normal());
    bench.tick();
    assert_eq!(exes(&bench.session()), ["onedrive.exe"]);
    assert!(bench.host.gestures().iter().all(|gesture| match gesture {
        Gesture::Apply { process, .. } => *process != locked && *process != hidden,
        Gesture::SetPowerPlan(_) => true,
    }));
    // Et on n'y revient pas aux relevés suivants.
    bench.ticks(40);
    assert_eq!(exes(&bench.session()), ["onedrive.exe"]);
}

// ------------------------------------------------------------ 5. alimentation

#[test]
fn high_performance_is_applied_then_given_back() {
    let mut bench = Bench::new();
    let game = bench.play();
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));
    assert_eq!(bench.session().power, PowerRecord::applied(PowerPlan::BALANCED));

    assert_eq!(bench.quit(game), Activity::Idle);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    let last = bench.file().last.unwrap();
    assert_eq!(last.power_restored, Some(true));
    assert!(last.complete);
}

#[test]
fn the_mode_that_comes_back_is_the_one_that_was_there() {
    let mut bench = Bench::new();
    bench.host.set_active_plan(Some(PowerPlan::POWER_SAVER));
    let game = bench.play();
    assert_eq!(bench.session().power.previous, Some(PowerPlan::POWER_SAVER));
    bench.quit(game);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::POWER_SAVER));
}

#[test]
fn some_processors_keep_the_normal_power_mode() {
    for name in [
        "AMD Ryzen 9 7900X3D 12-Core Processor",
        "AMD Ryzen 9 7950X3D 16-Core Processor",
        "AMD Ryzen 9 9900X3D 12-Core Processor",
        "AMD Ryzen 9 9950X3D 16-Core Processor",
    ] {
        let mut bench = Bench::new();
        bench.host.set_processor(name);
        let game = bench.play();
        assert_eq!(bench.power(), PowerOutcome::Unsuited, "{name}");
        assert!(bench.host.gestures().is_empty(), "{name} : rien n'est touché");
        bench.quit(game);
        assert_eq!(bench.file().last.unwrap().power_restored, None);
    }
}

#[test]
fn a_pc_without_high_performance_is_left_alone() {
    let mut bench = Bench::new();
    bench.host.remove_plan(PowerPlan::HIGH_PERFORMANCE);
    bench.play();
    assert_eq!(bench.power(), PowerOutcome::Unavailable);
    assert!(bench.host.gestures().is_empty());
}

#[test]
fn a_pc_already_in_high_performance_is_left_alone() {
    let mut bench = Bench::new();
    bench.host.set_active_plan(Some(PowerPlan::HIGH_PERFORMANCE));
    let game = bench.play();
    assert_eq!(bench.power(), PowerOutcome::AlreadyFast);
    // Les pas suivants ne la prennent pas pour une alimentation posée par Shinku.
    bench.ticks(5);
    assert_eq!(bench.power(), PowerOutcome::AlreadyFast);
    bench.quit(game);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));
    assert!(bench.host.gestures().is_empty());
}

#[test]
fn a_custom_power_mode_is_left_alone() {
    let mut bench = Bench::new();
    let makers = PowerPlan::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);
    bench.host.set_active_plan(Some(makers));
    let game = bench.play();
    assert_eq!(bench.power(), PowerOutcome::Custom);
    bench.quit(game);
    assert_eq!(bench.host.active_plan(), Some(makers));
    assert!(bench.host.gestures().is_empty());
}

#[test]
fn a_refused_power_change_is_reported_and_not_forced() {
    let mut bench = Bench::new();
    bench.host.refuse_power_changes(true);
    let game = bench.play();
    assert_eq!(bench.power(), PowerOutcome::Refused);
    assert_eq!(bench.session().power, PowerRecord::of(PowerOutcome::Refused));
    bench.ticks(10);
    assert_eq!(bench.power(), PowerOutcome::Refused);
    bench.quit(game);
    let last = bench.file().last.unwrap();
    assert_eq!(last.power_restored, None, "rien n'avait été changé");
    assert!(last.complete);
}

#[test]
fn on_battery_the_power_mode_is_given_back_at_once_and_taken_again_on_mains() {
    let mut bench = Bench::new();
    bench.host.set_on_battery(true);
    bench.play();
    assert_eq!(bench.power(), PowerOutcome::OnBattery);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));

    bench.host.set_on_battery(false);
    bench.tick();
    assert_eq!(bench.power(), PowerOutcome::Applied);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));

    // Au pas qui suit le débranchement, tout est remis.
    bench.host.set_on_battery(true);
    bench.tick();
    assert_eq!(bench.power(), PowerOutcome::OnBattery);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    assert_eq!(bench.session().power, PowerRecord::of(PowerOutcome::OnBattery));

    bench.host.set_on_battery(false);
    bench.tick();
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));
}

#[test]
fn the_power_setting_applies_at_the_next_step() {
    let mut bench = Bench::new();
    bench.play();
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));

    bench.settings.game_mode_power = false;
    bench.tick();
    assert_eq!(bench.power(), PowerOutcome::Off);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    bench.ticks(3);
    assert_eq!(bench.power(), PowerOutcome::Off);

    bench.settings.game_mode_power = true;
    bench.tick();
    assert_eq!(bench.power(), PowerOutcome::Applied);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));
}

#[test]
fn a_power_mode_chosen_by_the_person_during_the_game_stays() {
    let mut bench = Bench::new();
    let game = bench.play();
    // La personne change le mode elle-même.
    bench.host.set_active_plan(Some(PowerPlan::POWER_SAVER));
    bench.ticks(3);
    bench.host.clear_gestures();
    bench.quit(game);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::POWER_SAVER));
    assert!(bench.host.gestures().is_empty(), "Shinku ne remet que ce qu'il a posé");
    assert!(bench.file().last.unwrap().complete);
}

// ------------------------------------------------------------- 6. veilleuse

#[test]
fn only_listed_apps_that_are_not_protected_are_quieted() {
    let mut bench = Bench::new();
    bench.settings.game_mode_quiet_added = vec!["explorer.exe".to_owned(), "hp.exe".to_owned()];
    let onedrive = bench.app(200, ONEDRIVE);
    let dropbox = bench.app(201, DROPBOX);
    let notepad = bench.app(202, NOTEPAD);
    let explorer = bench.app(203, r"C:\Windows\explorer.exe");
    let twin = bench.app(204, TWIN);
    let game = bench.play();

    assert_eq!(bench.state(onedrive), low());
    assert_eq!(bench.state(dropbox), low());
    assert_eq!(bench.state(notepad), normal(), "pas dans la liste");
    assert_eq!(bench.state(explorer), normal(), "protégé, même ajouté à la main");
    assert_eq!(bench.state(game), normal(), "le jeu lui-même n'est jamais ralenti");
    assert_eq!(bench.state(twin), low(), "un autre programme de même nom, d'un autre dossier");
}

#[test]
fn an_app_is_only_ever_lowered() {
    let mut bench = Bench::new();
    bench.settings.game_mode_quiet_added = vec!["a.exe".to_owned(), "b.exe".to_owned()];
    let idle = bench.app(200, ONEDRIVE);
    bench.host.set_state(idle, ProcessState { priority: priority::IDLE, ..normal() });
    let below = bench.app(201, DROPBOX);
    bench.host.set_state(below, low());
    let high = bench.app(202, GOOGLE_DRIVE);
    bench.host.set_state(high, ProcessState { priority: priority::HIGH, ..normal() });
    let realtime = bench.app(203, r"C:\x\a.exe");
    bench.host.set_state(realtime, ProcessState { priority: 0x100, ..normal() });
    let forced = bench.app(204, r"C:\x\b.exe");
    bench.host.set_state(forced, ProcessState { throttle_control: 1, ..normal() });
    let game = bench.play();

    // Déjà en priorité basse : elle est gardée, seul le mode efficacité s'ajoute.
    assert_eq!(
        bench.state(idle),
        ProcessState { priority: priority::IDLE, throttle_control: 1, throttle_state: 1 }
    );
    assert_eq!(bench.state(below), low(), "déjà tout en place : rien à faire");
    assert_eq!(bench.state(high), low());
    assert_eq!(bench.state(realtime).priority, 0x100, "jamais touché");
    assert_eq!(bench.state(forced), low());
    // Le processus déjà en place, le temps réel et rien d'autre ne sont pas listés.
    assert_eq!(exes(&bench.session()), ["b.exe", "googledrivefs.exe", "onedrive.exe"]);

    bench.quit(game);
    assert_eq!(bench.state(idle), ProcessState { priority: priority::IDLE, ..normal() });
    assert_eq!(bench.state(below), low(), "elle n'avait rien reçu : elle ne change pas");
    assert_eq!(bench.state(high), ProcessState { priority: priority::HIGH, ..normal() });
    assert_eq!(bench.state(realtime).priority, 0x100);
    assert_eq!(bench.state(forced), ProcessState { throttle_control: 1, ..normal() });
}

#[test]
fn only_what_shinku_set_is_put_back() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let dropbox = bench.app(201, DROPBOX);
    let drive = bench.app(202, GOOGLE_DRIVE);
    let game = bench.play();
    assert_eq!(bench.state(onedrive), low());

    // La personne (ou un autre programme) change des choses entre-temps.
    bench.host.set_state(onedrive, ProcessState { priority: priority::ABOVE_NORMAL, ..low() });
    bench.host.set_state(dropbox, ProcessState { throttle_control: 1, throttle_state: 0, ..low() });
    bench.quit(game);

    // Sa priorité choisie reste ; les masques que Shinku avait posés reviennent.
    assert_eq!(
        bench.state(onedrive),
        ProcessState { priority: priority::ABOVE_NORMAL, ..normal() }
    );
    // Ses masques choisis restent ; la priorité que Shinku avait posée revient.
    assert_eq!(bench.state(dropbox), ProcessState { throttle_control: 1, ..normal() });
    assert_eq!(bench.state(drive), normal());
}

#[test]
fn the_app_being_watched_regains_its_speed_and_goes_back_when_left() {
    let mut bench = Bench::new();
    bench.settings.game_mode_quiet_added = vec!["chrome.exe".to_owned()];
    let main = bench.app(210, CHROME);
    let renderer = bench.host.spawn(211, 210, CHROME);
    let onedrive = bench.app(200, ONEDRIVE);
    let game = bench.play();
    assert_eq!(bench.state(main), low());
    assert_eq!(bench.state(renderer), low());

    // La personne regarde Chrome : tous ses processus retrouvent leur état d'avant.
    bench.host.set_foreground(Some(main));
    bench.tick();
    assert_eq!(bench.state(main), normal());
    assert_eq!(bench.state(renderer), normal());
    assert_eq!(bench.state(onedrive), low(), "les autres apps restent en veilleuse");
    assert_eq!(exes(&bench.session()), ["onedrive.exe"]);
    bench.ticks(20);
    assert_eq!(bench.state(main), normal(), "tant qu'elle est regardée, rien ne la ralentit");
    assert!(bench.watcher.has_session());

    // Elle quitte le premier plan et la partie dure : ils y retournent.
    bench.host.set_foreground(Some(game));
    bench.tick();
    assert_eq!(bench.state(main), low());
    assert_eq!(bench.state(renderer), low());
    assert_eq!(exes(&bench.session()), ["chrome.exe", "chrome.exe", "onedrive.exe"]);
}

#[test]
fn new_processes_are_quieted_every_fifteen_steps_and_the_journal_is_written_only_on_change() {
    let mut bench = Bench::new();
    let first = bench.app(200, ONEDRIVE);
    let dropbox = bench.app(201, DROPBOX);
    bench.play();
    // Le journal est effacé : il ne doit réapparaître que si la liste change.
    fs::remove_file(bench.store.path()).unwrap();
    let newcomer = bench.app(220, ONEDRIVE);

    bench.ticks(14);
    assert_eq!(bench.state(newcomer), normal(), "pas avant le quinzième pas");
    assert!(!bench.store.path().exists(), "rien n'a changé : rien n'est écrit");
    bench.tick();
    assert_eq!(bench.state(newcomer), low());
    assert_eq!(bench.session().quieted.len(), 3);

    // Un processus mort est retiré au relevé suivant (et le journal réécrit).
    bench.host.kill(dropbox);
    fs::remove_file(bench.store.path()).unwrap();
    bench.ticks(14);
    assert!(!bench.store.path().exists());
    bench.tick();
    let pids: Vec<u32> = bench.session().quieted.iter().map(|entry| entry.pid).collect();
    assert_eq!(pids, [first.pid, newcomer.pid]);
}

#[test]
fn a_process_started_from_a_lowered_one_is_put_back_too() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    // Créé avant la partie : ce n'est pas un enfant de la partie.
    let before = bench.host.spawn(232, 200, r"C:\x\vieux.exe");
    bench.host.set_state(before, low());
    let game = bench.play();

    let child = bench.host.spawn(230, 200, r"C:\x\setup.exe");
    bench.host.set_state(child, low());
    let grandchild = bench.host.spawn(231, 230, r"C:\x\helper.exe");
    bench.host.set_state(grandchild, low());
    let calm = bench.host.spawn(233, 200, r"C:\x\calme.exe");
    let stranger = bench.host.spawn(240, 1, r"C:\x\autre.exe");
    bench.host.set_state(stranger, low());

    bench.quit(game);
    assert_eq!(bench.state(onedrive), normal());
    assert_eq!(bench.state(child), ProcessState { priority: priority::NORMAL, ..low() });
    assert_eq!(bench.state(grandchild).priority, priority::NORMAL);
    assert_eq!(bench.state(calm), normal());
    assert_eq!(bench.state(stranger), low(), "pas un descendant : on n'y touche pas");
    assert_eq!(bench.state(before), low(), "créé avant la partie : on n'y touche pas");
}

#[test]
fn an_orphan_of_a_closed_app_is_still_a_descendant() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let game = bench.play();
    let orphan = bench.host.spawn(302, 200, r"C:\x\orphelin.exe");
    bench.host.set_state(orphan, low());
    bench.host.kill(onedrive);
    bench.quit(game);
    assert_eq!(bench.state(orphan).priority, priority::NORMAL);
}

#[test]
fn a_recycled_pid_is_not_a_parent() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let game = bench.play();
    bench.host.kill(onedrive);
    // Le pid de OneDrive est repris par un programme sans rapport, dont un
    // enfant est resté en priorité basse pour une autre raison.
    bench.host.spawn(200, 1, r"C:\x\autre.exe");
    let stranger_child = bench.host.spawn(301, 200, r"C:\x\enfant.exe");
    bench.host.set_state(stranger_child, low());
    bench.quit(game);
    assert_eq!(bench.state(stranger_child), low(), "son parent n'est pas celui du journal");
}

#[test]
fn the_quiet_setting_applies_at_the_next_step() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    bench.play();
    assert_eq!(bench.state(onedrive), low());

    bench.settings.game_mode_quiet_apps = false;
    bench.tick();
    assert_eq!(bench.state(onedrive), normal());
    assert!(bench.session().quieted.is_empty());
    bench.ticks(20);
    assert_eq!(bench.state(onedrive), normal(), "éteint : rien n'est ralenti");

    bench.settings.game_mode_quiet_apps = true;
    bench.tick();
    assert_eq!(bench.state(onedrive), low());
}

#[test]
fn ticking_or_unticking_an_app_applies_at_the_next_step() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let chrome = bench.app(210, CHROME);
    bench.play();
    assert_eq!(bench.state(chrome), normal());

    bench.settings.game_mode_quiet_added = vec!["chrome.exe".to_owned()];
    bench.settings.game_mode_quiet_removed = vec!["onedrive.exe".to_owned()];
    bench.tick();
    assert_eq!(bench.state(chrome), low());
    assert_eq!(bench.state(onedrive), normal());
    assert_eq!(exes(&bench.session()), ["chrome.exe"]);
}

// ------------------------------------------------------------ 7. fin de partie

#[test]
fn the_session_ends_when_no_process_of_the_game_lives() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let dropbox = bench.app(201, DROPBOX);
    let game = bench.play();
    let since = bench.session().since;
    bench.ticks(10);

    assert_eq!(bench.quit(game), Activity::Idle);
    assert!(!bench.watcher.has_session());
    assert_eq!(bench.state(onedrive), normal());
    assert_eq!(bench.state(dropbox), normal());
    let file = bench.file();
    assert_eq!(file.session, None);
    let last = file.last.expect("un bilan");
    assert_eq!(last.name, "Hollow Peaks");
    assert_eq!(last.path, GAME);
    assert_eq!(last.started, since);
    assert!(last.ended >= since + 20_000, "la partie a duré au moins vingt secondes");
    assert_eq!(last.power_restored, Some(true));
    assert_eq!(last.apps_restored, 2);
    assert!(last.complete);
    // Un jeu de bibliothèque est noté parmi les jeux vus.
    assert_eq!(file.seen.len(), 1);
    assert_eq!(file.seen[0].path, GAME);
    assert_eq!(file.seen[0].name, "Hollow Peaks");
    assert_eq!(file.seen[0].last_played, last.ended);
}

#[test]
fn a_launcher_left_open_in_the_background_does_not_keep_the_pc_in_game_mode() {
    let mut bench = Bench::new();
    let game = bench.play();
    let notepad = bench.app(300, NOTEPAD);
    bench.host.set_foreground(Some(notepad));
    let started = bench.host.now();

    let mut elapsed = Duration::ZERO;
    while bench.watcher.has_session() {
        bench.tick();
        elapsed = bench.host.now().since(started);
        assert!(elapsed <= Duration::from_secs(700), "la partie ne se termine jamais");
    }
    assert!(
        elapsed >= Duration::from_secs(590) && elapsed <= Duration::from_secs(610),
        "fin au bout de dix minutes, pas de {elapsed:?}"
    );
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    assert!(bench.file().last.unwrap().complete);

    // Le jeu revient au premier plan : la détection normale relance une partie.
    bench.host.set_foreground(Some(game));
    assert_eq!(bench.tick(), Activity::Candidate);
    assert_eq!(bench.tick(), Activity::Playing);
}

#[test]
fn a_game_back_in_front_resets_the_ten_minutes() {
    let mut bench = Bench::new();
    let game = bench.play();
    let notepad = bench.app(300, NOTEPAD);
    for _ in 0..3 {
        bench.host.set_foreground(Some(notepad));
        bench.ticks(250);
        assert!(bench.watcher.has_session());
        bench.host.set_foreground(Some(game));
        bench.tick();
    }
    assert!(bench.watcher.has_session());
}

#[test]
fn a_game_that_is_no_longer_recognised_ends_the_session() {
    let mut bench = Bench::new();
    bench.app(200, ONEDRIVE);
    bench.play();
    // « Ce n'est pas un jeu ».
    bench.settings.game_mode_not_games = vec![PathBuf::from(GAME.to_lowercase())];
    assert_eq!(bench.tick(), Activity::Idle);
    assert!(!bench.watcher.has_session());
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    // Et il ne redevient pas un candidat.
    assert_eq!(bench.tick(), Activity::Idle);
    assert_eq!(bench.tick(), Activity::Idle);
}

#[test]
fn another_game_in_front_for_two_steps_changes_the_game_without_touching_anything() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let first = bench.play();
    let before = bench.session();
    bench.host.clear_gestures();

    let second = bench.app(101, OTHER_GAME);
    bench.host.set_foreground(Some(second));
    bench.tick();
    assert_eq!(bench.session().game.path, GAME, "un seul pas ne suffit pas");
    // Un retour au premier jeu repart de zéro.
    bench.host.set_foreground(Some(first));
    bench.tick();
    bench.host.set_foreground(Some(second));
    bench.tick();
    assert_eq!(bench.session().game.path, GAME);
    assert_eq!(bench.tick(), Activity::Playing);

    let after = bench.session();
    assert_eq!(after.game.path, OTHER_GAME);
    assert_eq!(after.game.name, "Hades");
    assert!(after.since > before.since);
    assert_eq!(after.power, before.power, "l'alimentation n'est ni remise ni réappliquée");
    assert_eq!(after.quieted, before.quieted);
    assert!(bench.host.gestures().is_empty(), "rien n'est remis, rien n'est réappliqué");
    assert_eq!(bench.state(onedrive), low());
    // L'ancien jeu passe dans les jeux vus et dans le bilan.
    let file = bench.file();
    assert_eq!(file.seen[0].path, GAME);
    assert_eq!(file.last.unwrap().name, "Hollow Peaks");
}

#[test]
fn a_restore_that_fails_is_retried_every_fifteen_steps_until_it_works() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let game = bench.play();
    bench.host.refuse_power_changes(true);
    assert_eq!(
        bench.quit(game),
        Activity::Playing,
        "la partie reste tant que tout n'est pas remis"
    );
    let session = bench.session();
    assert!(session.restore_pending);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));
    assert_eq!(bench.state(onedrive), normal(), "ce qui a pu être remis l'est");
    assert!(session.quieted.is_empty());
    assert!(bench.watcher.has_session());

    // Rien ne se passe pendant quatorze pas.
    bench.host.refuse_power_changes(false);
    bench.ticks(14);
    assert!(bench.watcher.has_session());
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));
    // Au quinzième, c'est remis, et le bilan est complet.
    assert_eq!(bench.tick(), Activity::Idle);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    let last = bench.file().last.unwrap();
    assert!(last.complete);
    assert_eq!(last.power_restored, Some(true));
    assert_eq!(bench.file().session, None);
}

#[test]
fn after_twenty_attempts_the_restore_is_given_up() {
    let mut bench = Bench::new();
    let game = bench.play();
    bench.host.refuse_power_changes(true);
    bench.quit(game);
    // Première tentative à la fin, puis dix-neuf, tous les quinze pas.
    bench.ticks(15 * 19 - 1);
    assert!(bench.watcher.has_session(), "dix-neuf tentatives : on réessaie encore");
    assert!(bench.session().restore_pending);
    assert_eq!(bench.tick(), Activity::Idle, "vingtième tentative : on abandonne");
    let file = bench.file();
    assert_eq!(file.session, None);
    let last = file.last.unwrap();
    assert!(!last.complete);
    assert_eq!(last.power_restored, Some(false));
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));
}

// ------------------------------------------------------------ 8. relâcher

#[test]
fn releasing_puts_everything_back_at_once_and_applies_nothing_more() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let game = bench.play();
    assert_eq!(bench.state(onedrive), low());

    bench.watcher.release();
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    assert_eq!(bench.state(onedrive), normal());
    let session = bench.session();
    assert!(session.released);
    assert!(session.quieted.is_empty());
    assert!(!session.restore_pending);
    assert!(bench.watcher.has_session(), "la partie reste ouverte");

    // Les pas suivants n'appliquent rien, même avec un nouveau processus ou un
    // changement de réglage : le mode jeu ne se réactive pas aussitôt.
    bench.host.clear_gestures();
    let newcomer = bench.app(220, ONEDRIVE);
    bench.settings.game_mode_power = false;
    bench.settings.game_mode_power = true;
    for _ in 0..40 {
        assert_eq!(bench.tick(), Activity::Playing);
    }
    assert!(bench.host.gestures().is_empty());
    assert_eq!(bench.state(newcomer), normal());
    assert!(bench.session().released);

    // La fin normale du jeu clôt la partie, puis une nouvelle partie reprend.
    assert_eq!(bench.quit(game), Activity::Idle);
    let last = bench.file().last.unwrap();
    assert!(last.complete);
    assert_eq!(last.power_restored, Some(true));
    let again = bench.launch();
    bench.ticks(2);
    assert!(!bench.session().released);
    assert_eq!(bench.state(again), normal());
}

#[test]
fn releasing_without_a_session_does_nothing() {
    let mut bench = Bench::new();
    bench.watcher.release();
    assert!(bench.host.gestures().is_empty());
    assert!(!bench.store.path().exists());
}

#[test]
fn a_release_that_fails_is_retried() {
    let mut bench = Bench::new();
    bench.play();
    bench.host.refuse_power_changes(true);
    bench.watcher.release();
    let session = bench.session();
    assert!(session.released && session.restore_pending);
    bench.host.refuse_power_changes(false);
    bench.ticks(15);
    let session = bench.session();
    assert!(session.released && !session.restore_pending);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
}

// ------------------------------------------------------------ 9. suspendre

#[test]
fn suspending_puts_everything_back_and_closes_the_session_at_once() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    bench.play();
    bench.watcher.suspend();
    assert!(!bench.watcher.has_session());
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    assert_eq!(bench.state(onedrive), normal());
    let file = bench.file();
    assert_eq!(file.session, None);
    assert!(file.last.unwrap().complete);

    // La détection normale reprend ensuite.
    assert_eq!(bench.tick(), Activity::Candidate);
    assert_eq!(bench.tick(), Activity::Playing);
}

#[test]
fn a_suspend_that_cannot_restore_everything_keeps_the_journal() {
    let mut bench = Bench::new();
    bench.play();
    bench.host.refuse_power_changes(true);
    bench.watcher.suspend();
    // La reprise suivante (au prochain démarrage de l'agent) réessaiera.
    let session = bench.session();
    assert!(session.restore_pending);
    assert!(session.power.is_held());
    bench.host.refuse_power_changes(false);
    let mut next = Watcher::new(Arc::new(bench.host.clone()), bench.store.clone());
    next.recover();
    assert!(!next.has_session());
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
}

// ------------------------------------------------------------ 10. reprise

fn restart(bench: &Bench) -> Watcher {
    Watcher::new(Arc::new(bench.host.clone()), bench.store.clone())
}

#[test]
fn recovery_resumes_a_session_whose_game_still_runs() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let game = bench.play();
    let journal = bench.file();
    let gestures = bench.host.gestures();

    // L'agent s'arrête sans prévenir, puis redémarre.
    let mut next = restart(&bench);
    next.recover();
    assert!(next.has_session());
    assert_eq!(bench.host.gestures(), gestures, "rien n'est refait");
    assert_eq!(bench.file(), journal, "la partie reprend telle quelle");

    // Les pas suivants la suivent, et sa fin remet les mêmes choses en place.
    assert_eq!(next.tick(&bench.settings), Activity::Playing);
    bench.host.kill(game);
    assert_eq!(next.tick(&bench.settings), Activity::Idle);
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    assert_eq!(bench.state(onedrive), normal());
    assert!(bench.file().last.unwrap().complete);
}

#[test]
fn recovery_ends_a_session_whose_game_is_gone() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let game = bench.play();
    // Le PC s'est éteint en pleine partie : le jeu n'est plus là.
    bench.host.kill(game);

    let mut next = restart(&bench);
    next.recover();
    assert!(!next.has_session());
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    assert_eq!(bench.state(onedrive), normal());
    let file = bench.file();
    assert_eq!(file.session, None);
    assert!(file.last.unwrap().complete);

    // Idempotent : une seconde reprise ne refait rien.
    let gestures = bench.host.gestures();
    next.recover();
    restart(&bench).recover();
    assert_eq!(bench.host.gestures(), gestures);
}

#[test]
fn recovery_leaves_a_power_mode_the_person_chose() {
    let mut bench = Bench::new();
    let game = bench.play();
    bench.host.kill(game);
    bench.host.set_active_plan(Some(PowerPlan::POWER_SAVER));
    bench.host.clear_gestures();
    restart(&bench).recover();
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::POWER_SAVER));
    assert!(bench.host.gestures().is_empty());
}

#[test]
fn recovery_does_not_reopen_the_game_at_every_step() {
    let mut bench = Bench::new();
    let twin = bench.app(50, TWIN);
    let game = bench.play();
    bench.host.set_foreground(None);
    let mut next = restart(&bench);
    next.recover();
    assert!(next.has_session());
    for _ in 0..5 {
        bench.host.advance(STEP);
        next.tick(&bench.settings);
    }
    assert_eq!(bench.host.opened(), [twin.pid, game.pid], "chaque processus est ouvert une fois");
}

#[test]
fn recovery_retries_a_restore_left_half_done() {
    let mut bench = Bench::new();
    let game = bench.play();
    bench.host.refuse_power_changes(true);
    bench.quit(game);
    assert!(bench.session().restore_pending);

    // L'agent redémarre avec le même refus : la session reste, à moitié remise.
    let mut next = restart(&bench);
    next.recover();
    assert!(next.has_session());
    assert!(bench.session().restore_pending);
    bench.host.refuse_power_changes(false);
    for _ in 0..15 {
        bench.host.advance(STEP);
        next.tick(&bench.settings);
    }
    assert!(!next.has_session());
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
}

#[test]
fn recovery_keeps_a_released_session_released() {
    let mut bench = Bench::new();
    bench.play();
    bench.watcher.release();
    let mut next = restart(&bench);
    next.recover();
    assert!(next.has_session());
    bench.host.clear_gestures();
    for _ in 0..10 {
        bench.host.advance(STEP);
        next.tick(&bench.settings);
    }
    assert!(bench.host.gestures().is_empty());
    assert!(bench.session().released);
}

#[test]
fn a_hostile_journal_cannot_make_shinku_change_a_process_it_does_not_own() {
    let bench = Bench::new();
    let victim = bench.app(500, NOTEPAD);
    bench.host.set_state(victim, ProcessState { priority: priority::ABOVE_NORMAL, ..normal() });
    // Un programme écrit un journal qui désigne ce processus, avec un pid qui
    // existe mais une date de création qui n'est pas la sienne.
    let text = serde_json::json!({
        "session": {
            "game": { "path": GAME, "name": "x", "source": "library" },
            "since": 1,
            "quieted": [{ "pid": 500, "started": 5, "exe": "notepad.exe", "priority": 128 }]
        }
    });
    fs::create_dir_all(bench.store.path().parent().unwrap()).unwrap();
    fs::write(bench.store.path(), text.to_string()).unwrap();
    let mut next = restart(&bench);
    next.recover();
    assert!(!next.has_session(), "le jeu n'est pas là : la partie est close");
    assert_eq!(
        bench.state(victim),
        ProcessState { priority: priority::ABOVE_NORMAL, ..normal() },
        "un pid dont la date de création ne correspond pas n'est pas touché"
    );
}

// --------------------------------------------------------------- l'agent seul

#[test]
fn ticking_without_detection_still_follows_a_running_session() {
    let mut bench = Bench::new();
    let game = bench.play();
    bench.host.advance(STEP);
    assert_eq!(bench.watcher.tick_detecting(&bench.settings, false), Activity::Playing);
    bench.host.kill(game);
    bench.host.advance(STEP);
    assert_eq!(bench.watcher.tick_detecting(&bench.settings, false), Activity::Idle);
    // Sans détection, un jeu au premier plan ne devient pas un candidat.
    bench.launch();
    bench.host.advance(STEP);
    assert_eq!(bench.watcher.tick_detecting(&bench.settings, false), Activity::Idle);
    bench.host.advance(STEP);
    assert_eq!(bench.watcher.tick_detecting(&bench.settings, false), Activity::Idle);
    assert!(!bench.watcher.has_session());
}

// ------------------------------------------- priorité héritée (bloquants 1 à 4)

/// L'état d'un processus né d'un processus en veilleuse : il a hérité de la
/// classe de son parent, mais pas du mode efficacité.
fn inherited() -> ProcessState {
    ProcessState { priority: priority::BELOW_NORMAL, throttle_control: 0, throttle_state: 0 }
}

#[test]
fn a_listed_child_born_lowered_goes_back_to_normal() {
    let mut bench = Bench::new();
    bench.settings.game_mode_quiet_added = vec!["chrome.exe".to_owned()];
    let main = bench.app(210, CHROME);
    let game = bench.play();
    assert_eq!(bench.state(main), low());
    // Un nouvel onglet naît de Chrome en veilleuse : il hérite de la classe.
    bench.host.advance(STEP);
    let tab = bench.host.spawn(211, 210, CHROME);
    bench.host.set_state(tab, inherited());
    bench.ticks(16);
    let entry = bench.session().quieted.into_iter().find(|entry| entry.pid == 211);
    assert_eq!(
        entry.map(|entry| entry.priority),
        Some(priority::NORMAL),
        "l'état d'avant de l'onglet est « normale », pas la classe héritée"
    );
    bench.quit(game);
    assert_eq!(bench.state(tab).priority, priority::NORMAL, "l'onglet revient en « normale »");
    assert_eq!(bench.state(main), normal());
}

#[test]
fn an_orphan_whose_parent_left_the_journal_goes_back_to_normal() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let game = bench.play();
    bench.host.advance(STEP);
    let child = bench.host.spawn(230, 200, r"C:\x\FileCoAuth.exe");
    bench.host.set_state(child, inherited());
    bench.host.kill(onedrive);
    // Un relevé retire OneDrive du journal.
    bench.ticks(16);
    assert!(bench.session().quieted.is_empty());
    bench.quit(game);
    assert_eq!(bench.state(child).priority, priority::NORMAL, "l'orphelin hérité revient");
}

#[test]
fn a_change_of_game_does_not_forget_children_born_before_it() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    let first = bench.play();
    bench.host.advance(STEP);
    let child = bench.host.spawn(230, 200, r"C:\x\FileCoAuth.exe");
    bench.host.set_state(child, inherited());
    let second = bench.app(101, OTHER_GAME);
    bench.host.set_foreground(Some(second));
    bench.ticks(3);
    assert_eq!(bench.session().game.path, OTHER_GAME);
    bench.host.kill(second);
    bench.host.kill(first);
    assert_eq!(bench.tick(), Activity::Idle);
    assert_eq!(bench.state(onedrive), normal());
    assert_eq!(bench.state(child).priority, priority::NORMAL, "l'enfant hérité revient");
}

#[test]
fn a_game_started_by_a_quieted_app_is_never_lowered() {
    let mut bench = Bench::new();
    // Une app ajoutée par la personne, qui sert aussi de lanceur à un jeu.
    bench.settings.game_mode_quiet_added = vec!["itch.exe".to_owned()];
    let itch = bench.app(50, r"C:\Users\a\AppData\Local\itch\itch.exe");
    let launcher = bench.app(100, r"D:\SteamLibrary\steamapps\common\X\launcher.exe");
    bench.host.set_foreground(Some(launcher));
    bench.ticks(2);
    assert_eq!(bench.state(itch), low());
    // Le lanceur demande à l'app de démarrer le vrai jeu : il hérite de la classe.
    bench.host.advance(STEP);
    let real = bench.host.spawn(101, 50, r"D:\SteamLibrary\steamapps\common\X\x64\X.exe");
    bench.host.set_state(real, inherited());
    bench.host.set_foreground(Some(real));
    bench.ticks(3);
    assert!(bench.session().game.path.ends_with("X.exe"), "la partie a suivi le jeu");
    assert_eq!(bench.state(real).priority, priority::NORMAL, "le jeu n'est pas ralenti");
    bench.ticks(40);
    assert_eq!(bench.state(real).priority, priority::NORMAL);
}

// ------------------------------------------------ redémarrage et horloges

#[test]
fn after_a_reboot_only_the_power_plan_is_put_back() {
    let mut bench = Bench::new();
    bench.app(200, ONEDRIVE);
    bench.play();
    // Une minute passe, puis Windows redémarre : tous les pid sont neufs.
    bench.host.advance(Duration::from_secs(60));
    bench.host.reboot();
    let newcomer = bench.app(200, ONEDRIVE);
    bench.host.set_state(newcomer, low());
    // Un jeu lancé après le redémarrage, de même nom que l'ancien.
    let again = bench.launch();
    let mut next = restart(&bench);
    next.recover();
    assert!(!next.has_session(), "la partie d'avant le redémarrage est close");
    assert_eq!(bench.host.active_plan(), Some(PowerPlan::BALANCED));
    assert_eq!(
        bench.state(newcomer),
        low(),
        "un processus d'après le redémarrage n'est pas touché"
    );
    assert_eq!(bench.state(again), normal());
    assert_eq!(bench.file().session, None);
}

#[test]
fn a_clock_jumping_back_does_not_extend_the_ten_minutes() {
    let mut bench = Bench::new();
    bench.play();
    let notepad = bench.app(300, NOTEPAD);
    bench.host.set_foreground(Some(notepad));
    bench.host.jump_clock(-7_200_000);
    // Les délais suivent l'horloge monotone : dix minutes de temps réel.
    bench.ticks(290);
    assert!(bench.watcher.has_session());
    bench.ticks(15);
    assert!(!bench.watcher.has_session(), "la partie se termine malgré l'heure reculée");
}

#[test]
fn a_clock_jumping_forward_does_not_end_the_session() {
    let mut bench = Bench::new();
    bench.play();
    let notepad = bench.app(300, NOTEPAD);
    bench.host.set_foreground(Some(notepad));
    bench.host.jump_clock(7_200_000);
    bench.ticks(100);
    assert!(bench.watcher.has_session(), "deux heures d'horloge murale ne sont pas dix minutes");
}

#[test]
fn a_write_that_failed_is_tried_again_after_a_minute_of_uptime() {
    let mut bench = Bench::blocked();
    bench.launch();
    bench.tick();
    bench.tick();
    assert!(!bench.watcher.has_session(), "journal impossible : rien n'est ouvert");
    bench.unblock();
    bench.host.jump_clock(-3_600_000);
    // Moins d'une minute de temps réel : pas de nouvel essai, même si
    // l'heure murale est revenue en arrière.
    bench.ticks(10);
    assert!(!bench.watcher.has_session());
    bench.ticks(25);
    assert!(bench.watcher.has_session(), "l'essai a lieu après soixante secondes réelles");
}

// ----------------------------------------------------------- racines et gone

#[test]
fn a_child_born_long_after_its_root_died_is_not_raised() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    bench.play();
    bench.host.kill(onedrive);
    // Le relevé suivant marque la racine comme morte.
    bench.ticks(16);
    // Bien plus tard, un autre programme prend le même pid et lance un enfant.
    bench.ticks(30);
    let recycled = bench.host.spawn(200, 1, r"C:\Autre\thing.exe");
    bench.host.advance(STEP);
    let child = bench.host.spawn(231, 200, r"C:\Autre\child.exe");
    bench.host.set_state(child, low());
    let _ = recycled;
    bench.ticks(32);
    assert_eq!(bench.state(child), low(), "un enfant d'un pid recyclé n'est pas à Shinku");
}

#[test]
fn a_game_switched_to_that_was_quieted_is_put_back() {
    let mut bench = Bench::new();
    bench.settings.game_mode_quiet_added = vec!["hades.exe".to_owned()];
    let other = bench.app(101, OTHER_GAME);
    bench.play();
    assert_eq!(bench.state(other), low());
    bench.host.set_foreground(Some(other));
    bench.ticks(5);
    assert_eq!(bench.session().game.path, OTHER_GAME);
    assert_eq!(bench.state(other).priority, priority::NORMAL, "le nouveau jeu n'est pas ralenti");
}

// ----------------------------------------------------------- erreurs d'écriture

#[test]
fn a_throttle_that_cannot_change_keeps_the_entry_and_puts_it_back_at_the_end() {
    let mut bench = Bench::new();
    let onedrive = bench.app(200, ONEDRIVE);
    bench.host.break_throttle(onedrive, true);
    let game = bench.play();
    // La priorité a changé, puis le mode efficacité a refusé.
    assert_eq!(bench.state(onedrive).priority, priority::BELOW_NORMAL);
    assert!(
        bench.session().quieted.iter().any(|entry| entry.pid == 200),
        "l'entrée reste au journal"
    );
    bench.host.break_throttle(onedrive, false);
    bench.quit(game);
    assert_eq!(bench.state(onedrive).priority, priority::NORMAL);
}

#[test]
fn a_journal_that_cannot_be_written_is_flushed_as_soon_as_it_can() {
    let mut bench = Bench::new();
    bench.app(200, ONEDRIVE);
    let game = bench.play();
    // Le fichier temporaire est un dossier : plus aucune écriture ne réussit.
    let tmp = bench.store.path().with_extension("json.tmp");
    let _ = fs::remove_file(&tmp);
    fs::create_dir(&tmp).unwrap();
    bench.host.kill(game);
    let activity = bench.tick();
    assert_eq!(activity, Activity::Idle);
    assert!(bench.file().session.is_some(), "le disque garde encore la partie");
    fs::remove_dir(&tmp).unwrap();
    bench.tick();
    let file = bench.file();
    assert_eq!(file.session, None, "dès que l'écriture passe, le disque dit la vérité");
    assert!(file.last.is_some());
}

// ------------------------------------------------- relâcher puis changer de jeu

#[test]
fn a_released_game_then_another_game_closes_the_session_and_opens_a_new_one() {
    let mut bench = Bench::new();
    bench.play();
    bench.watcher.release();
    let other = bench.app(101, OTHER_GAME);
    bench.host.set_foreground(Some(other));
    bench.ticks(4);
    let file = bench.file();
    assert_eq!(file.last.as_ref().map(|last| last.path.as_str()), Some(GAME));
    let session = file.session.expect("une nouvelle partie est ouverte");
    assert_eq!(session.game.path, OTHER_GAME);
    assert!(!session.released, "la nouvelle partie n'est pas relâchée");
}

#[test]
fn a_release_then_a_recovery_reports_the_power_as_restored() {
    let mut bench = Bench::new();
    let game = bench.play();
    bench.watcher.release();
    bench.host.kill(game);
    let mut next = restart(&bench);
    next.recover();
    let last = bench.file().last.expect("un bilan");
    assert_eq!(last.power_restored, Some(true));
}

#[test]
fn restore_attempts_are_counted_across_a_release_and_the_end_of_the_game() {
    let mut bench = Bench::new();
    let game = bench.play();
    bench.host.refuse_power_changes(true);
    bench.watcher.release();
    assert!(bench.session().restore_pending);
    bench.host.kill(game);
    // La remise du relâchement a compté pour une tentative : dix-neuf restent.
    bench.ticks(15 * 18);
    assert!(bench.watcher.has_session());
    bench.ticks(15);
    assert!(!bench.watcher.has_session(), "abandon à la vingtième tentative cumulée");
    assert_eq!(bench.file().last.unwrap().power_restored, Some(false));
}
