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
use std::time::{Duration, SystemTime};

use crate::game::STEP;
use crate::game::host::GameHost;
use crate::game::store::GameStore;
use crate::game::watch::Watcher;
use crate::model::Timestamp;
use crate::settings::{LoadIssue, Settings, SettingsStore};

/// Temps pendant lequel un agent lancé par Windows ne détecte rien : Windows
/// ouvre sa session, le PC est chargé, personne ne joue encore.
pub const START_DELAY: Duration = Duration::from_secs(120);

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
    /// Pas de détection avant cet instant.
    quiet_until: Option<Timestamp>,
    /// La session Windows est déconnectée ou se ferme : pas de détection.
    away: bool,
    /// Efface la valeur `Run` de Windows (ordre « retirer »).
    forget_autostart: Box<dyn Fn() + Send>,
}

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
        let now = host.now();
        let mut agent = Self {
            host,
            watcher,
            settings_store,
            settings: Settings::default(),
            stamp: None,
            quiet_until: delayed.then(|| {
                let ticks = i64::try_from(START_DELAY.as_nanos() / 100).unwrap_or(i64::MAX);
                Timestamp::from_ticks(now.ticks().saturating_add(ticks))
            }),
            away: false,
            forget_autostart: Box::new(crate::platform::agent::forget_autostart),
        };
        agent.reload_settings(true);
        let flow = agent.flow();
        (agent, flow)
    }

    /// Remplace ce que fait l'ordre « retirer » pour effacer la valeur `Run`
    /// de Windows. Pour les tests : le vrai ne doit pas toucher au registre.
    pub fn with_autostart_removal(mut self, removal: impl Fn() + Send + 'static) -> Self {
        self.forget_autostart = Box::new(removal);
        self
    }

    /// Traite un événement.
    pub fn handle(&mut self, event: AgentEvent) -> Flow {
        match event {
            AgentEvent::Tick | AgentEvent::PowerChanged => {
                self.reload_settings(false);
                let detect = !self.away && !self.in_start_delay();
                self.watcher.tick_detecting(&self.settings, detect);
                self.flow()
            }
            AgentEvent::Release => {
                self.watcher.release();
                Flow::Continue
            }
            AgentEvent::Disconnected | AgentEvent::SessionEnding => {
                self.watcher.suspend();
                self.away = true;
                Flow::Continue
            }
            AgentEvent::Reconnected => {
                self.away = false;
                Flow::Continue
            }
            AgentEvent::Quit => {
                self.watcher.suspend();
                Flow::Stop
            }
            AgentEvent::Remove => {
                self.watcher.suspend();
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
        self.quiet_until.is_some_and(|until| self.host.now() < until)
    }

    /// Continuer tant que le mode automatique est allumé ou qu'une partie est ouverte.
    fn flow(&self) -> Flow {
        if self.settings.game_mode_auto || self.watcher.has_session() {
            Flow::Continue
        } else {
            Flow::Stop
        }
    }

    /// Relit `settings.json` si sa date de modification a changé. Un fichier
    /// illisible ne remplace pas les derniers réglages valides.
    fn reload_settings(&mut self, first: bool) {
        let stamp = fs::metadata(self.settings_store.path())
            .ok()
            .map(|metadata| (metadata.modified().ok(), metadata.len()));
        if !first && stamp == self.stamp {
            return;
        }
        self.stamp = stamp;
        let loaded = self.settings_store.load();
        if matches!(loaded.issue, Some(LoadIssue::Unreadable { .. })) {
            return;
        }
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
        fn play(&self, agent: &mut Agent) {
            let game = self.host.spawn(100, 1, GAME);
            self.host.set_foreground(Some(game));
            for _ in 0..2 {
                assert_eq!(self.tick(agent), Flow::Continue);
            }
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
    fn a_closing_session_restores_before_returning() {
        let bench = Bench::new(true);
        let onedrive = bench.host.spawn(200, 1, ONEDRIVE);
        let (mut agent, _) = bench.start(false, false);
        bench.play(&mut agent);
        assert_eq!(agent.handle(AgentEvent::SessionEnding), Flow::Continue);
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
}
