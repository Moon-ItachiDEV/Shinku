//! Ce que la fenêtre peut demander à l'agent et à Windows.
//!
//! Le guichet ([`crate::backend::Backend`]) ne parle pas directement à
//! Windows : il passe par [`AgentControl`], remplacé dans les tests par une
//! version fabriquée ([`FakeAgent`]).

use std::fmt;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::platform::agent::{self, AgentState, Autostart, OpenProgram, Order, WindowsPage};

/// Le contrôle de l'agent et des réglages de Windows.
pub trait AgentControl: Send + Sync + fmt::Debug {
    /// L'agent tourne-t-il, est-il arrêté, ou est-il absent de cette installation ?
    fn state(&self) -> AgentState;
    /// Lance l'agent (avec `--relacher` si demandé).
    fn spawn(&self, release_first: bool) -> AgentState;
    /// Poste un ordre à l'agent en cours. Faux s'il n'y en a pas.
    fn post(&self, order: Order) -> bool;
    /// Où en est la valeur `Run` de Windows.
    fn autostart(&self) -> Autostart;
    /// Pose ou retire la valeur `Run`.
    fn set_autostart(&self, on: bool) -> io::Result<()>;
    /// Les apps ouvertes en ce moment, une par exécutable.
    fn open_apps(&self) -> Vec<OpenProgram>;
    /// Ouvre une page des réglages de Windows.
    fn open_settings(&self, page: WindowsPage) -> io::Result<()>;
}

/// La vraie machine : l'agent installé à côté de `shinku.exe`, et Windows.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemAgent;

impl AgentControl for SystemAgent {
    fn state(&self) -> AgentState {
        agent::agent_state()
    }

    fn spawn(&self, release_first: bool) -> AgentState {
        agent::spawn_agent(release_first)
    }

    fn post(&self, order: Order) -> bool {
        agent::post_agent(order)
    }

    fn autostart(&self) -> Autostart {
        agent::autostart()
    }

    fn set_autostart(&self, on: bool) -> io::Result<()> {
        agent::set_autostart(on)
    }

    fn open_apps(&self) -> Vec<OpenProgram> {
        agent::open_apps()
    }

    fn open_settings(&self, page: WindowsPage) -> io::Result<()> {
        agent::open_windows_settings(page)
    }
}

/// Une installation sans agent : rien ne tourne, rien ne se lance.
#[derive(Clone, Copy, Debug, Default)]
pub struct InertAgent;

impl AgentControl for InertAgent {
    fn state(&self) -> AgentState {
        AgentState::Missing
    }

    fn spawn(&self, _release_first: bool) -> AgentState {
        AgentState::Missing
    }

    fn post(&self, _order: Order) -> bool {
        false
    }

    fn autostart(&self) -> Autostart {
        Autostart::Absent
    }

    fn set_autostart(&self, _on: bool) -> io::Result<()> {
        Ok(())
    }

    fn open_apps(&self) -> Vec<OpenProgram> {
        Vec::new()
    }

    fn open_settings(&self, _page: WindowsPage) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct Fake {
    state: AgentState,
    autostart: Autostart,
    apps: Vec<OpenProgram>,
    spawns: Vec<bool>,
    orders: Vec<Order>,
    autostart_calls: Vec<bool>,
    opened: Vec<WindowsPage>,
    /// Faire démarrer l'agent quand on le lance.
    spawn_works: bool,
    /// Durée de l'écriture de la valeur `Run` quand on l'allume (hors verrou).
    autostart_delay: std::time::Duration,
}

/// Un agent fabriqué, pour les tests : on lui dit dans quel état il est, et il
/// note ce qu'on lui demande. Ses copies partagent le même état.
#[derive(Clone, Debug)]
pub struct FakeAgent {
    inner: Arc<Mutex<Fake>>,
}

impl Default for FakeAgent {
    fn default() -> Self {
        Self::new(AgentState::Stopped)
    }
}

impl FakeAgent {
    /// Un agent dans cet état.
    pub fn new(state: AgentState) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Fake {
                state,
                autostart: Autostart::Absent,
                apps: Vec::new(),
                spawns: Vec::new(),
                orders: Vec::new(),
                autostart_calls: Vec::new(),
                opened: Vec::new(),
                spawn_works: true,
                autostart_delay: std::time::Duration::ZERO,
            })),
        }
    }

    /// L'écriture de la valeur `Run`, quand on l'allume, dure ce temps : un
    /// changement plus récent peut alors la rattraper, si rien ne les met en file.
    pub fn set_autostart_delay(&self, delay: std::time::Duration) {
        self.fake().autostart_delay = delay;
    }

    fn fake(&self) -> MutexGuard<'_, Fake> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Change l'état de l'agent.
    pub fn set_state(&self, state: AgentState) {
        self.fake().state = state;
    }

    /// Fixe l'état de la valeur `Run`.
    pub fn set_autostart_state(&self, autostart: Autostart) {
        self.fake().autostart = autostart;
    }

    /// Fixe la liste des apps ouvertes.
    pub fn set_open_apps(&self, apps: Vec<OpenProgram>) {
        self.fake().apps = apps;
    }

    /// Le lancement de l'agent échoue (ou réussit à nouveau).
    pub fn set_spawn_works(&self, works: bool) {
        self.fake().spawn_works = works;
    }

    /// Les lancements demandés (`true` : avec `--relacher`).
    pub fn spawns(&self) -> Vec<bool> {
        self.fake().spawns.clone()
    }

    /// Les ordres postés.
    pub fn orders(&self) -> Vec<Order> {
        self.fake().orders.clone()
    }

    /// Les appels à `set_autostart`.
    pub fn autostart_calls(&self) -> Vec<bool> {
        self.fake().autostart_calls.clone()
    }

    /// Les pages de Windows ouvertes.
    pub fn opened(&self) -> Vec<WindowsPage> {
        self.fake().opened.clone()
    }
}

impl AgentControl for FakeAgent {
    fn state(&self) -> AgentState {
        self.fake().state
    }

    fn spawn(&self, release_first: bool) -> AgentState {
        let mut fake = self.fake();
        fake.spawns.push(release_first);
        if fake.state != AgentState::Missing && fake.spawn_works {
            fake.state = AgentState::Running;
        }
        fake.state
    }

    fn post(&self, order: Order) -> bool {
        let mut fake = self.fake();
        if fake.state != AgentState::Running {
            return false;
        }
        fake.orders.push(order);
        true
    }

    fn autostart(&self) -> Autostart {
        self.fake().autostart
    }

    fn set_autostart(&self, on: bool) -> io::Result<()> {
        let delay = self.fake().autostart_delay;
        if on && !delay.is_zero() {
            std::thread::sleep(delay);
        }
        let mut fake = self.fake();
        fake.autostart_calls.push(on);
        fake.autostart = if on { Autostart::Enabled } else { Autostart::Absent };
        Ok(())
    }

    fn open_apps(&self) -> Vec<OpenProgram> {
        self.fake().apps.clone()
    }

    fn open_settings(&self, page: WindowsPage) -> io::Result<()> {
        self.fake().opened.push(page);
        Ok(())
    }
}
