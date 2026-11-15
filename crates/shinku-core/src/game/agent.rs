//! La boucle de l'agent, sans Windows.
//!
//! `shinku-agent.exe` ne fait que brancher les messages de Windows sur cette
//! boucle ([`AgentEvent`]) : une minuterie de deux secondes, un changement de
//! source d'alimentation, la fermeture de la session, et les ordres de la
//! fenêtre ou de l'installeur. Toute la décision se prend ici, et se teste avec
//! une fausse machine.

use std::fmt;
use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

use crate::game::STEP;
use crate::game::host::GameHost;
use crate::game::store::GameStore;
use crate::game::watch::{RETRY_EVERY, Watcher};
use crate::settings::{LoadIssue, Settings, SettingsStore};

/// Temps pendant lequel un agent lancé par Windows ne détecte rien : Windows
/// ouvre sa session, le PC est chargé, personne ne joue encore. Il se compte
/// avec l'horloge monotone de la machine, pas avec l'heure murale.
pub const START_DELAY: Duration = Duration::from_secs(120);
/// Essais d'une remise en place restée à moitié avant de s'arrêter pour de bon
/// (ordre « retirer » : après lui, plus rien ne la reprendra).
pub const REMOVE_ATTEMPTS: u32 = 5;
/// Pause entre deux de ces essais.
pub const REMOVE_PAUSE: Duration = Duration::from_millis(200);

/// Reprise unique, dans le processus courant, quand aucun agent ne répond à
/// l'ordre « quitter » ou « retirer » (désinstallation, mise à jour) : ce que
/// l'agent aurait fait en s'arrêtant. Une partie restée au journal est remise
/// en place ; si quelque chose résiste et que `patient`, quelques essais
/// rapprochés suivent, car après la désinstallation plus rien ne le reprendra.
///
/// Ce programme peut tourner élevé (c'est l'installeur qui l'appelle) : il ne
/// détecte rien, il ne remet que ce que le journal dit.
pub fn restore_without_agent(host: Arc<dyn GameHost>, game_store: GameStore, patient: bool) {
    let mut watcher = Watcher::new(host, game_store);
    watcher.recover();
    watcher.suspend();
    if patient {
        settle_patiently(&mut watcher, REMOVE_PAUSE);
    }
}

/// Quelques essais rapprochés d'une remise en place inachevée.
fn settle_patiently(watcher: &mut Watcher, pause: Duration) {
    for _ in 0..REMOVE_ATTEMPTS {
        if !watcher.has_session() {
            return;
        }
        thread::sleep(pause);
        watcher.settle();
    }
}

/// Ce qui arrive à l'agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentEvent {
    /// La minuterie de deux secondes.
    Tick,
    /// La source d'alimentation a changé (secteur, batterie).
    PowerChanged,
    /// Ordre : tout remettre en place maintenant.
    Release,
    /// Ordre : s'arrêter (tout est remis en place d'abord).
    Quit,
    /// Ordre : s'arrêter, et ne plus se lancer avec Windows.
    Remove,
    /// La session Windows se ferme.
    SessionEnding,
    /// La session est déconnectée (verrouillage à distance, changement d'utilisateur).
    Disconnected,
    /// La session est reconnectée.
    Reconnected,
}

/// Ce que l'agent fait ensuite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Continuer à veiller.
    Continue,
    /// S'arrêter.
    Stop,
}

/// Date et taille d'un fichier : si l'une change, le fichier a changé.
type Stamp = (Option<SystemTime>, u64);

/// La boucle de l'agent.
pub struct Agent {
    host: Arc<dyn GameHost>,
    watcher: Watcher,
    settings_store: SettingsStore,
    /// Les derniers réglages valides.
    settings: Settings,
    /// La date du fichier des réglages à la dernière lecture.
    stamp: Option<Stamp>,
    /// Pas de détection avant cet instant de l'horloge monotone, en millisecondes.
    quiet_until: Option<u64>,
    /// La session Windows est déconnectée ou se ferme : pas de détection.
    away: bool,
    /// Efface la valeur `Run` de Windows (ordre « retirer »).
    forget_autostart: Box<dyn Fn() + Send>,
    /// Pause entre les essais d'une remise inachevée à l'ordre « retirer ».
    remove_pause: Duration,
    /// Le dernier fichier de réglages illisible (sa date) et l'instant avant
    /// lequel on ne le relit pas, à moins que sa date change.
    unreadable: Option<(Option<Stamp>, u64)>,
    /// Pas où l'agent a attendu pour écrire un journal en retard avant de s'arrêter.
    waited: u32,
}

/// Pas qu'un agent qui voudrait s'arrêter attend, au plus, pour écrire un
/// journal en retard sur le disque.
const LATE_JOURNAL_STEPS: u32 = 5;

impl fmt::Debug for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Agent")
            .field("watcher", &self.watcher)
            .field("away", &self.away)
            .finish_non_exhaustive()
    }
}

impl Agent {
    /// Démarre l'agent : reprend la partie du journal s'il y en a une, la
    /// relâche si `release_first`, lit les réglages. Rend `Flow::Stop` s'il n'y
    /// a rien à faire (mode automatique éteint et aucune partie).
    ///
    /// `delayed` : l'agent a été lancé par Windows. Il ne détecte aucun jeu
    /// pendant [`START_DELAY`] ; la reprise d'une partie du journal, elle, est
    /// immédiate.
    pub fn start(
        host: Arc<dyn GameHost>,
        settings_store: SettingsStore,
        game_store: GameStore,
        delayed: bool,
        release_first: bool,
    ) -> (Self, Flow) {
        let mut watcher = Watcher::new(Arc::clone(&host), game_store);
        watcher.recover();
        if release_first {
            watcher.release();
        }
        let now = host.uptime_ms();
        let mut agent = Self {
            host,
            watcher,
            settings_store,
            settings: Settings::default(),
            stamp: None,
            quiet_until: delayed
                .then(|| now.saturating_add(u64::try_from(START_DELAY.as_millis()).unwrap_or(0))),
            away: false,
            forget_autostart: Box::new(crate::platform::agent::forget_autostart),
            remove_pause: REMOVE_PAUSE,
            unreadable: None,
            waited: 0,
        };
        agent.reload_settings(true);
        let flow = agent.flow(true);
        (agent, flow)
    }

    /// Remplace ce que fait l'ordre « retirer » pour effacer la valeur `Run`
    /// de Windows. Pour les tests : le vrai ne doit pas toucher au registre.
    pub fn with_autostart_removal(mut self, removal: impl Fn() + Send + 'static) -> Self {
        self.forget_autostart = Box::new(removal);
        self
    }

    /// Change la pause entre les essais d'une remise inachevée à l'ordre
    /// « retirer ». Pour les tests : le vrai attend.
    pub fn with_remove_pause(mut self, pause: Duration) -> Self {
        self.remove_pause = pause;
        self
    }

    /// Traite un événement.
    pub fn handle(&mut self, event: AgentEvent) -> Flow {
        match event {
            AgentEvent::Tick => {
                self.reload_settings(false);
                let detect = !self.away && !self.in_start_delay();
                self.watcher.tick_detecting(&self.settings, detect);
                self.flow(true)
            }
            // Windows envoie cet événement à chaque variation de la batterie :
            // ce n'est pas un pas, seule l'alimentation est réévaluée.
            AgentEvent::PowerChanged => {
                self.reload_settings(false);
                self.watcher.power_changed(&self.settings);
                self.flow(false)
            }
            AgentEvent::Release => {
                self.watcher.release();
                Flow::Continue
            }
            AgentEvent::Disconnected => {
                self.watcher.suspend();
                self.away = true;
                Flow::Continue
            }
            // Tout est remis avant de rendre la main à Windows. Le gestionnaire
            // de redémarrage envoie aussi cet événement pour fermer les apps
            // pendant une mise à jour, sans fermer la session : l'agent s'arrête
            // dans tous les cas (il serait resté sourd, sans `Reconnected`).
            AgentEvent::SessionEnding => {
                self.watcher.suspend();
                self.watcher.flush();
                self.away = true;
                Flow::Stop
            }
            AgentEvent::Reconnected => {
                self.away = false;
                Flow::Continue
            }
            AgentEvent::Quit => {
                self.watcher.suspend();
                self.watcher.flush();
                Flow::Stop
            }
            AgentEvent::Remove => {
                self.watcher.suspend();
                settle_patiently(&mut self.watcher, self.remove_pause);
                self.watcher.flush();
                (self.forget_autostart)();
                Flow::Stop
            }
        }
    }

    /// La durée entre deux événements `Tick` que l'agent attend.
    pub const fn step() -> Duration {
        STEP
    }

    fn in_start_delay(&self) -> bool {
        self.quiet_until.is_some_and(|until| self.host.uptime_ms() < until)
    }

    /// Continuer tant que le mode automatique est allumé ou qu'une partie est
    /// ouverte. Un agent qui s'arrête tente d'abord d'écrire un journal en
    /// retard ; s'il n'y arrive pas, il attend quelques pas (le disque était
    /// plein, un antivirus tenait le fichier) avant de s'arrêter quand même.
    ///
    /// Seuls les vrais pas (`step`) comptent dans l'attente : les variations de
    /// la batterie arrivent par rafales et ne la raccourciraient pas de 10 s.
    fn flow(&mut self, step: bool) -> Flow {
        if self.settings.game_mode_auto || self.watcher.has_session() {
            self.waited = 0;
            return Flow::Continue;
        }
        self.watcher.flush();
        if self.watcher.is_dirty() && self.waited < LATE_JOURNAL_STEPS {
            self.waited += u32::from(step);
            return Flow::Continue;
        }
        Flow::Stop
    }

    /// Relit `settings.json` si sa date de modification a changé. Un fichier
    /// illisible ne remplace pas les derniers réglages valides, et sera relu au
    /// pas suivant.
    fn reload_settings(&mut self, first: bool) {
        let stamp = fs::metadata(self.settings_store.path())
            .ok()
            .map(|metadata| (metadata.modified().ok(), metadata.len()));
        if !first && stamp == self.stamp {
            return;
        }
        let uptime = self.host.uptime_ms();
        // Un fichier qui reste illisible n'est pas relu à chaque pas : une fois
        // tous les 30 secondes, ou dès que sa date change.
        if !first
            && let Some((failed, until)) = self.unreadable
            && failed == stamp
            && uptime < until
        {
            return;
        }
        let loaded = self.settings_store.load();
        if matches!(loaded.issue, Some(LoadIssue::Unreadable { .. })) {
            // Une lecture ratée (fichier tenu par un antivirus, écriture en
            // cours) ne doit pas faire oublier le changement : la date n'est pas
            // notée, le fichier sera relu plus tard.
            self.unreadable = Some((
                stamp,
                uptime.saturating_add(u64::try_from(RETRY_EVERY.as_millis()).unwrap_or(0)),
            ));
            return;
        }
        self.unreadable = None;
        self.stamp = stamp;
        self.settings = loaded.settings;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::game::host::FakeHost;

    const GAME: &str = r"D:\Jeux\Hollow Peaks\hp.exe";
    const ONEDRIVE: &str = r"C:\Users\a\AppData\Local\Microsoft\OneDrive\OneDrive.exe";

    struct Bench {
        _dir: tempfile::TempDir,
        host: FakeHost,
        settings: SettingsStore,
        game: GameStore,
        removals: Arc<AtomicU32>,
    }

    impl Bench {
        fn new(auto: bool) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let bench = Self {
                settings: SettingsStore::at(dir.path().join("roaming").join("settings.json")),
                game: GameStore::at(dir.path().join("local").join("game.json")),
                host: FakeHost::new(),
                removals: Arc::new(AtomicU32::new(0)),
                _dir: dir,
            };
            bench.write(auto);
            bench
        }

        /// Écrit les réglages : le jeu est ajouté à la main, le mode jeu est allumé ou non.
        fn write(&self, auto: bool) {
            let settings = Settings {
                game_mode_auto: auto,
                game_mode_games: vec![PathBuf::from(GAME)],
                ..Settings::default()
            };
            self.settings.save(&settings).unwrap();
        }

        fn start(&self, delayed: bool, release_first: bool) -> (Agent, Flow) {
            let (agent, flow) = Agent::start(
                Arc::new(self.host.clone()),
                self.settings.clone(),
                self.game.clone(),
                delayed,
                release_first,
            );
            let removals = Arc::clone(&self.removals);
            (
                agent.with_autostart_removal(move || {
                    let _ = removals.fetch_add(1, Ordering::SeqCst);
                }),
                flow,
            )
        }

        fn tick(&self, agent: &mut Agent) -> Flow {
            self.host.advance(STEP);
            agent.handle(AgentEvent::Tick)
        }

        /// Lance le jeu au premier plan et laisse l'agent le reconnaître.
        fn play(&self, agent: &mut Agent) -> crate::game::host::ProcessRef {
            let game = self.host.spawn(100, 1, GAME);
            self.host.set_foreground(Some(game));
            for _ in 0..2 {
                assert_eq!(self.tick(agent), Flow::Continue);
            }
            game
        }

        fn session(&self) -> Option<crate::game::store::Session> {
            self.game.load().session
        }
    }

    #[test]
    fn an_agent_with_nothing_to_watch_stops_at_once() {
        let bench = Bench::new(false);
        let (_agent, flow) = bench.start(false, false);
        assert_eq!(flow, Flow::Stop);
    }

    #[test]
    fn an_agent_watches_while_the_mode_is_on_and_stops_when_it_is_turned_off() {
        let bench = Bench::new(true);
        let (mut agent, flow) = bench.start(false, false);
        assert_eq!(flow, Flow::Continue);
        assert_eq!(bench.tick(&mut agent), Flow::Continue);
        // Le fichier change de taille : sa date ou sa taille a bougé, l'agent le relit.
        bench.write(false);
        assert_eq!(bench.tick(&mut agent), Flow::Stop);
    }

    #[test]
    fn an_unreadable_settings_file_keeps_the_last_valid_settings() {
        let bench = Bench::new(true);
        let (mut agent, _) = bench.start(false, false);
        std::fs::write(bench.settings.path(), b"{ pas du json").unwrap();
        bench.play(&mut agent);
        assert!(bench.session().is_some(), "les derniers réglages valides servent encore");
    }

    #[test]
    fn a_game_is_recognised_and_a_quit_puts_everything_back() {
        let bench = Bench::new(true);
        let onedrive = bench.host.spawn(200, 1, ONEDRIVE);
        let (mut agent, _) = bench.start(false, false);
        bench.play(&mut agent);
        let low = bench.host.state_of(onedrive).unwrap();
        assert_ne!(low.priority, crate::game::host::priority::NORMAL, "OneDrive est en veilleuse");
        assert_eq!(agent.handle(AgentEvent::Quit), Flow::Stop);
        assert_eq!(
            bench.host.state_of(onedrive).unwrap().priority,
            crate::game::host::priority::NORMAL
        );
        assert_eq!(bench.removals.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_remove_order_puts_everything_back_and_forgets_the_autostart() {
        let bench = Bench::new(true);
        let onedrive = bench.host.spawn(200, 1, ONEDRIVE);
        let (mut agent, _) = bench.start(false, false);
        bench.play(&mut agent);
        assert_eq!(agent.handle(AgentEvent::Remove), Flow::Stop);
        assert_eq!(
            bench.host.state_of(onedrive).unwrap().priority,
            crate::game::host::priority::NORMAL
        );
        assert_eq!(bench.removals.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_release_order_keeps_the_agent_alive_but_stops_applying() {
        let bench = Bench::new(true);
        let onedrive = bench.host.spawn(200, 1, ONEDRIVE);
        let (mut agent, _) = bench.start(false, false);
        bench.play(&mut agent);
        assert_eq!(agent.handle(AgentEvent::Release), Flow::Continue);
        assert!(bench.session().is_some_and(|session| session.released));
        assert_eq!(
            bench.host.state_of(onedrive).unwrap().priority,
            crate::game::host::priority::NORMAL
        );
    }

    #[test]
    fn a_disconnected_session_restores_and_detects_nothing_until_it_is_back() {
        let bench = Bench::new(true);
        let onedrive = bench.host.spawn(200, 1, ONEDRIVE);
        let (mut agent, _) = bench.start(false, false);
        bench.play(&mut agent);
        assert_eq!(agent.handle(AgentEvent::Disconnected), Flow::Continue);
        assert_eq!(
            bench.host.state_of(onedrive).unwrap().priority,
            crate::game::host::priority::NORMAL
        );
        assert!(bench.session().is_none(), "la partie est close");
        for _ in 0..4 {
            bench.tick(&mut agent);
        }
        assert!(bench.session().is_none(), "rien n'est détecté pendant l'absence");
        assert_eq!(agent.handle(AgentEvent::Reconnected), Flow::Continue);
        for _ in 0..3 {
            bench.tick(&mut agent);
        }
        assert!(bench.session().is_some(), "la détection reprend au retour");
    }

    #[test]
    fn a_closing_session_restores_before_returning_and_stops_the_agent() {
        let bench = Bench::new(true);
        let onedrive = bench.host.spawn(200, 1, ONEDRIVE);
        let (mut agent, _) = bench.start(false, false);
        bench.play(&mut agent);
        // Le gestionnaire de redémarrage envoie aussi cet événement sans fermer
        // la session : l'agent s'arrête, il ne reste pas sourd.
        assert_eq!(agent.handle(AgentEvent::SessionEnding), Flow::Stop);
        assert_eq!(
            bench.host.state_of(onedrive).unwrap().priority,
            crate::game::host::priority::NORMAL
        );
    }

    #[test]
    fn windows_gives_two_minutes_before_any_detection() {
        let bench = Bench::new(true);
        let (mut agent, _) = bench.start(true, false);
        bench.play(&mut agent);
        assert!(bench.session().is_none(), "trop tôt");
        bench.host.advance(START_DELAY);
        for _ in 0..3 {
            bench.tick(&mut agent);
        }
        assert!(bench.session().is_some(), "après le délai, le jeu est reconnu");
    }

    #[test]
    fn a_game_left_in_the_journal_is_taken_back_at_once_even_during_the_delay() {
        let bench = Bench::new(true);
        let onedrive = bench.host.spawn(200, 1, ONEDRIVE);
        {
            let (mut first, _) = bench.start(false, false);
            bench.play(&mut first);
            // L'agent est tué net : le journal reste.
        }
        let (mut second, flow) = bench.start(true, false);
        assert_eq!(flow, Flow::Continue);
        assert!(bench.session().is_some());
        assert_eq!(bench.tick(&mut second), Flow::Continue);
        // Le jeu vit toujours : la partie continue, sans attendre.
        assert!(bench.session().is_some());
        assert_ne!(
            bench.host.state_of(onedrive).unwrap().priority,
            crate::game::host::priority::NORMAL
        );
    }

    #[test]
    fn starting_with_release_puts_back_a_forgotten_game_first() {
        let bench = Bench::new(true);
        let onedrive = bench.host.spawn(200, 1, ONEDRIVE);
        {
            let (mut first, _) = bench.start(false, false);
            bench.play(&mut first);
        }
        let (_second, _) = bench.start(false, true);
        assert_eq!(
            bench.host.state_of(onedrive).unwrap().priority,
            crate::game::host::priority::NORMAL
        );
        assert!(bench.session().is_some_and(|session| session.released));
    }

    #[test]
    fn a_stopped_agent_with_a_journal_session_keeps_running_to_restore_it() {
        let bench = Bench::new(true);
        bench.host.spawn(200, 1, ONEDRIVE);
        {
            let (mut first, _) = bench.start(false, false);
            bench.play(&mut first);
        }
        // Le mode automatique est éteint pendant que la partie court.
        bench.write(false);
        let (_second, flow) = bench.start(false, false);
        assert_eq!(flow, Flow::Continue, "une partie ouverte garde l'agent en vie");
    }
    #[test]
    fn two_power_events_without_a_step_never_open_a_session() {
        let bench = Bench::new(true);
        let (mut agent, _) = bench.start(false, false);
        let game = bench.host.spawn(100, 1, GAME);
        bench.host.set_foreground(Some(game));
        assert_eq!(agent.handle(AgentEvent::PowerChanged), Flow::Continue);
        assert_eq!(agent.handle(AgentEvent::PowerChanged), Flow::Continue);
        assert!(bench.session().is_none(), "un événement n'est pas un pas");
    }

    #[test]
    fn going_on_battery_gives_the_power_mode_back_at_once() {
        let bench = Bench::new(true);
        let (mut agent, _) = bench.start(false, false);
        bench.play(&mut agent);
        assert_eq!(bench.host.active_plan(), Some(crate::game::host::PowerPlan::HIGH_PERFORMANCE));
        bench.host.set_on_battery(true);
        agent.handle(AgentEvent::PowerChanged);
        assert_eq!(bench.host.active_plan(), Some(crate::game::host::PowerPlan::BALANCED));
    }

    #[test]
    fn the_start_delay_follows_the_monotonic_clock() {
        let bench = Bench::new(true);
        let (mut agent, _) = bench.start(true, false);
        // L'heure murale recule de deux heures : le délai, lui, court toujours.
        bench.host.jump_clock(-7_200_000);
        bench.play(&mut agent);
        assert!(bench.session().is_none(), "trop tôt");
        bench.host.advance(START_DELAY);
        for _ in 0..3 {
            bench.tick(&mut agent);
        }
        assert!(bench.session().is_some());
    }

    #[test]
    fn an_unreadable_file_is_read_again_only_every_thirty_seconds_even_with_the_same_stamp() {
        let bench = Bench::new(true);
        let (mut agent, _) = bench.start(false, false);
        let path = bench.settings.path().to_path_buf();
        // Le fichier est remplacé par un dossier : la lecture échoue.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let (modified, len) = (metadata.modified().unwrap(), metadata.len());
        assert_eq!(bench.tick(&mut agent), Flow::Continue, "les derniers réglages restent");
        // Un vrai fichier de même taille et de même date prend sa place : si la
        // date de l'échec avait été retenue, il ne serait jamais relu.
        fs::remove_dir(&path).unwrap();
        let off = Settings { game_mode_auto: false, ..Settings::default() };
        let mut text = serde_json::to_vec(&off).unwrap();
        // (Sous Windows un dossier mesure 0 : la taille ne peut alors pas être égalée,
        // le test ne prouve plus que la relecture simple.)
        text.resize(usize::try_from(len).unwrap().max(text.len()), b' ');
        fs::write(&path, text).unwrap();
        fs::OpenOptions::new().write(true).open(&path).unwrap().set_modified(modified).unwrap();
        // Un fichier illisible n'est pas relu à chaque pas... (si la date et la
        // taille diffèrent, comme sous Windows, il est relu tout de suite : on ne
        // peut alors prouver que la relecture au bout de 30 secondes)
        let new = fs::metadata(&path).unwrap();
        if (new.modified().unwrap(), new.len()) == (modified, len) {
            assert_eq!(bench.tick(&mut agent), Flow::Continue, "pas relu avant 30 secondes");
        }
        // ... mais il l'est ensuite, même s'il garde la même date.
        bench.host.advance(Duration::from_secs(30));
        assert_eq!(
            bench.tick(&mut agent),
            Flow::Stop,
            "le fichier est relu au bout de 30 secondes"
        );
    }

    #[test]
    fn the_remove_order_retries_a_refused_restore() {
        let bench = Bench::new(true);
        let (agent, _) = bench.start(false, false);
        let mut agent = agent.with_remove_pause(Duration::ZERO);
        bench.play(&mut agent);
        bench.host.refuse_power_changes_for(3);
        assert_eq!(agent.handle(AgentEvent::Remove), Flow::Stop);
        assert!(bench.session().is_none(), "la partie est close après quelques essais");
        assert_eq!(bench.host.active_plan(), Some(crate::game::host::PowerPlan::BALANCED));
        assert_eq!(bench.removals.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn restoring_without_an_agent_closes_a_session_left_in_the_journal() {
        let bench = Bench::new(true);
        let onedrive = bench.host.spawn(200, 1, ONEDRIVE);
        {
            let (mut agent, _) = bench.start(false, false);
            bench.play(&mut agent);
        }
        assert!(bench.session().is_some());
        bench.host.refuse_power_changes_for(2);
        restore_without_agent(Arc::new(bench.host.clone()), bench.game.clone(), true);
        assert!(bench.session().is_none());
        assert_eq!(bench.host.active_plan(), Some(crate::game::host::PowerPlan::BALANCED));
        assert_eq!(
            bench.host.state_of(onedrive).unwrap().priority,
            crate::game::host::priority::NORMAL
        );
    }

    #[test]
    fn restoring_without_an_agent_after_a_clock_jump_puts_the_apps_back() {
        let bench = Bench::new(true);
        let onedrive = bench.host.spawn(200, 1, ONEDRIVE);
        {
            let (mut agent, _) = bench.start(false, false);
            bench.play(&mut agent);
        }
        assert_eq!(
            bench.host.state_of(onedrive).unwrap().priority,
            crate::game::host::priority::BELOW_NORMAL
        );
        // L'heure murale avance de deux heures : ce n'est pas un redémarrage.
        bench.host.jump_clock(2 * 3_600_000);
        restore_without_agent(Arc::new(bench.host.clone()), bench.game.clone(), false);
        assert!(bench.session().is_none());
        assert_eq!(
            bench.host.state_of(onedrive).unwrap().priority,
            crate::game::host::priority::NORMAL
        );
    }

    #[test]
    fn an_agent_that_wants_to_stop_waits_a_few_steps_for_a_late_journal() {
        let bench = Bench::new(true);
        let (mut agent, _) = bench.start(false, false);
        let game = bench.play(&mut agent);
        // La personne éteint le mode, et le journal ne peut pas s'écrire.
        bench.write(false);
        let tmp = bench.game.path().with_extension("json.tmp");
        fs::create_dir(&tmp).unwrap();
        bench.host.kill(game);
        assert_eq!(bench.tick(&mut agent), Flow::Continue, "journal en retard : l'agent attend");
        assert!(bench.session().is_some(), "le disque garde la partie");
        fs::remove_dir(&tmp).unwrap();
        assert_eq!(bench.tick(&mut agent), Flow::Stop, "le journal est écrit, l'agent s'arrête");
        assert!(bench.session().is_none(), "le disque dit la vérité");
    }

    #[test]
    fn power_events_do_not_shorten_the_wait_for_a_late_journal() {
        let bench = Bench::new(true);
        let (mut agent, _) = bench.start(false, false);
        let game = bench.play(&mut agent);
        bench.write(false);
        let tmp = bench.game.path().with_extension("json.tmp");
        fs::create_dir(&tmp).unwrap();
        bench.host.kill(game);
        assert_eq!(bench.tick(&mut agent), Flow::Continue);
        // Une rafale de variations de batterie n'use pas l'attente.
        for _ in 0..20 {
            assert_eq!(agent.handle(AgentEvent::PowerChanged), Flow::Continue);
        }
        fs::remove_dir(&tmp).unwrap();
        assert_eq!(bench.tick(&mut agent), Flow::Stop, "l'attente n'était pas épuisée");
        assert!(bench.session().is_none());
    }

    #[test]
    fn an_agent_that_cannot_write_its_journal_stops_anyway_after_a_few_steps() {
        let bench = Bench::new(true);
        let (mut agent, _) = bench.start(false, false);
        let game = bench.play(&mut agent);
        bench.write(false);
        let tmp = bench.game.path().with_extension("json.tmp");
        fs::create_dir(&tmp).unwrap();
        bench.host.kill(game);
        let mut last = Flow::Continue;
        for _ in 0..10 {
            last = bench.tick(&mut agent);
        }
        assert_eq!(last, Flow::Stop);
    }
}
