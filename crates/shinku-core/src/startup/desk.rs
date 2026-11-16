//! Le guichet du module « Démarrage » : ce que la fenêtre peut lire et demander.
//!
//! [`StartupDesk`] garde la machine et `startup.json`. La vue se refait à
//! chaque demande : aucune liste n'est gardée entre deux appels, la machine
//! peut avoir changé (un autre programme, le Gestionnaire des tâches). Un seul
//! changement à la fois : le second, s'il arrive pendant le premier, est refusé
//! ([`StartupRefusal::Busy`]).
//!
//! Une entrée **du PC** se règle par l'assistant élevé ([`ElevatedHelper`]) : le
//! guichet pose `pending`, **relâche tout verrou de lecture** (la vue répond
//! pendant l'attente), lance l'assistant et attend sa fin, puis relit la machine
//! et met `startup.json` à jour d'après la valeur **relue**. Le code de sortie de
//! l'assistant ne sert qu'à choisir le message.
//!
//! Si Shinku se ferme pendant l'attente, l'assistant finit seul : le changement
//! se fait sans la marque « par Shinku ».

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};

use crate::helper::request::{StartupAction, StartupRequest, StartupSlot};
use crate::platform::agent::run_command;
use crate::startup::access::PcAccess;
use crate::startup::host::StartupHost;
use crate::startup::launcher::{ElevatedHelper, HelperRun, InertHelper};
use crate::startup::store::StartupStore;
use crate::startup::view::{StartupPending, StartupRefusal, StartupView};
use crate::startup::{self, EntryId, MachineRules, Probe, Scope, State, ToggleError};

/// Le module « Démarrage », vu du guichet.
#[derive(Debug)]
pub struct StartupDesk {
    host: Arc<dyn StartupHost>,
    helper: Arc<dyn ElevatedHelper>,
    store: Option<StartupStore>,
    /// La commande que le mode jeu écrit pour cette installation.
    own: Option<String>,
    /// Pris (sans attendre) par un changement en cours, **pendant toute
    /// l'attente de l'assistant** : c'est lui qui refuse un second changement.
    busy: Mutex<()>,
    /// Met en file la lecture de la machine et l'écriture de `startup.json` :
    /// une vue et un changement ne s'écrasent pas. Jamais tenu pendant l'attente.
    turn: Mutex<()>,
    /// Le changement du PC dont on attend la réponse.
    pending: Mutex<Option<StartupPending>>,
}

/// Retire `pending` à la fin de l'attente, quoi qu'il arrive.
struct PendingGuard<'a>(&'a Mutex<Option<StartupPending>>);

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

fn refusal(error: ToggleError) -> StartupRefusal {
    match error {
        ToggleError::UnknownEntry => StartupRefusal::UnknownEntry,
        ToggleError::NotAdjustable => StartupRefusal::NotAdjustable,
        ToggleError::Failed => StartupRefusal::Failed,
    }
}

impl StartupDesk {
    /// Un guichet sur cette machine et ce fichier, sans assistant élevé : les
    /// entrées du PC ne se règlent pas.
    pub fn new(host: Arc<dyn StartupHost>, store: Option<StartupStore>) -> Self {
        Self {
            host,
            helper: Arc::new(InertHelper),
            store,
            own: run_command(),
            busy: Mutex::new(()),
            turn: Mutex::new(()),
            pending: Mutex::new(None),
        }
    }

    /// Change la commande qu'on prend pour celle de Shinku (pour les tests).
    pub fn with_own_command(mut self, own: Option<String>) -> Self {
        self.own = own;
        self
    }

    /// Branche l'assistant élevé : les entrées du PC se règlent par lui.
    pub fn with_helper(mut self, helper: Arc<dyn ElevatedHelper>) -> Self {
        self.helper = helper;
        self
    }

    /// De quoi compter les apps inscrites à la fin d'une analyse.
    pub fn probe(&self) -> Probe {
        Probe::new(Arc::clone(&self.host), self.own.clone())
    }

    fn turn(&self) -> MutexGuard<'_, ()> {
        self.turn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn pending(&self) -> Option<StartupPending> {
        *self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ce que l'assistant permet, et ce qui s'en déduit pour les entrées du PC.
    fn rules(&self) -> (PcAccess, MachineRules) {
        let pc = self.helper.access();
        let allowed = pc != PcAccess::Unavailable;
        (pc, MachineRules { allowed, folder_safe: allowed && self.helper.folder_safe() })
    }

    /// Tout ce que la page affiche. Met `startup.json` à jour. Répond tout de
    /// suite pendant l'attente de l'assistant, avec `pending`.
    pub fn view(&self) -> StartupView {
        let _turn = self.turn();
        self.read()
    }

    fn read(&self) -> StartupView {
        let (pc, rules) = self.rules();
        let listing =
            startup::enumerate(self.host.as_ref(), self.own.as_deref(), self.store.as_ref(), rules);
        StartupView::from(&listing).with_access(pc, self.pending())
    }

    /// Coupe ou remet une entrée, puis rend la vue d'après. Sans fenêtre
    /// propriétaire pour l'accord de Windows : voir [`StartupDesk::set_for`].
    pub fn set(&self, id: &str, enabled: bool) -> Result<StartupView, StartupRefusal> {
        self.set_for(id, enabled, 0)
    }

    /// Coupe ou remet une entrée. `id` est celui que la page a reçu : mal formé,
    /// il désigne une entrée inconnue. Un changement à la fois : s'il y en a un
    /// en cours (même en attente d'une réponse), `Busy`.
    ///
    /// Pour une entrée du PC, `owner` est la fenêtre de Shinku (0 : aucune) :
    /// l'accord de Windows s'ouvre devant elle. Rend la main quand l'assistant a
    /// fini.
    pub fn set_for(
        &self,
        id: &str,
        enabled: bool,
        owner: isize,
    ) -> Result<StartupView, StartupRefusal> {
        let id = EntryId::parse(id).ok_or(StartupRefusal::UnknownEntry)?;
        let _busy = match self.busy.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(StartupRefusal::Busy),
        };
        if id.place().scope() == Scope::Machine {
            return self.set_machine(id, enabled, owner);
        }
        let _turn = self.turn();
        let change = startup::toggle(self.host.as_ref(), self.own.as_deref(), id, enabled)
            .map_err(refusal)?;
        if change.changed {
            self.remember(id, &change.name, enabled, change.file_time);
        }
        Ok(self.read())
    }

    /// Met `startup.json` à jour : une coupure retenue, une remise oubliée. Un
    /// fichier qui ne s'écrit pas n'annule pas le changement : la page dira
    /// « Coupée le … » d'après Windows, sans « par Shinku ».
    fn remember(&self, id: EntryId, name: &str, enabled: bool, file_time: u64) {
        let Some(store) = &self.store else { return };
        let mut file = store.load();
        if enabled {
            file.forget(id);
        } else {
            file.remember(id, name, self.host.now().unix_millis(), file_time);
        }
        let _ = store.save(&file);
    }

    /// Une entrée du PC : par l'assistant élevé, **sans verrou de lecture**.
    fn set_machine(
        &self,
        id: EntryId,
        enabled: bool,
        owner: isize,
    ) -> Result<StartupView, StartupRefusal> {
        // 1. Ce qu'on va demander : relu à l'instant, une seule fois, réglable.
        let request = {
            let _turn = self.turn();
            let (_, rules) = self.rules();
            let entry = startup::locate(self.host.as_ref(), self.own.as_deref(), id, rules)
                .map_err(refusal)?;
            let wanted = if enabled { State::Enabled } else { State::Disabled };
            if entry.state == wanted {
                // Déjà dans l'état voulu : aucun accord à demander.
                return Ok(self.read());
            }
            let slot = StartupSlot::of_place(entry.place).ok_or(StartupRefusal::NotAdjustable)?;
            let action = if enabled { StartupAction::Restore } else { StartupAction::Cut };
            StartupRequest::new(action, slot, &entry.raw_name)
                .ok_or(StartupRefusal::NotAdjustable)?
        };

        // 2. L'attente : `pending` posé, aucun verrou de lecture tenu.
        *self.pending.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(StartupPending { id, enabled });
        let pending = PendingGuard(&self.pending);
        let outcome = self.helper.run(&request.to_words(), owner);

        // 3. La relecture dit l'état ; `startup.json` suit la valeur relue.
        // `pending` reste posé jusqu'à la fin de cette étape (et il est retiré
        // quoi qu'il arrive, même sur panique) : la page ne cesse pas de relire
        // avant que la vue dise la vérité.
        let _turn = self.turn();
        let (_, rules) = self.rules();
        let again = startup::reread(self.host.as_ref(), self.own.as_deref(), id, rules);
        if let Some(entry) = again {
            let cut = !enabled && outcome == HelperRun::Exited(0) && entry.state == State::Disabled;
            let restored = enabled && entry.state == State::Enabled;
            if cut || restored {
                self.remember(id, &entry.name, enabled, entry.file_time);
            }
        }
        drop(pending);
        match outcome {
            HelperRun::Cancelled | HelperRun::Exited(3) => Err(StartupRefusal::Cancelled),
            HelperRun::Exited(5) => Err(StartupRefusal::NeedsAdmin),
            HelperRun::Exited(0 | 9) => Ok(self.read()),
            HelperRun::Exited(4) => Err(StartupRefusal::UnknownEntry),
            // Dont le code 1, celui d'un assistant tué : jamais rendu par lui.
            HelperRun::Exited(_) | HelperRun::TimedOut | HelperRun::NotStarted => {
                Err(StartupRefusal::Failed)
            }
        }
    }
}

#[cfg(test)]
#[path = "desk_pc_tests.rs"]
mod pc_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::approval::cut_bytes;
    use crate::startup::host::FakeStartupHost;
    use crate::startup::view::StartupEntry;
    use crate::startup::{Mark, Place, Scope, State};

    const OWN: &str = r#""C:\Program Files\Shinku\shinku-agent.exe" --demarrage"#;

    struct Fixture {
        _temp: tempfile::TempDir,
        host: Arc<FakeStartupHost>,
        store: StartupStore,
        desk: StartupDesk,
    }

    fn fixture() -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let store = StartupStore::at(temp.path().join("Shinku").join("startup.json"));
        let host = Arc::new(FakeStartupHost::new());
        host.add_program(r"C:\Apps\Discord\app.exe", Some("Update"), Some("Discord Inc."));
        host.add(Place::UserRun, "Discord", r#""C:\Apps\Discord\app.exe""#);
        host.add(Place::UserRun, "Shinku", OWN);
        host.add(Place::MachineRun, "SecurityHealth", r"C:\Windows\SecurityHealth.exe");
        let desk = StartupDesk::new(host.clone(), Some(store.clone()))
            .with_own_command(Some(OWN.to_owned()));
        Fixture { _temp: temp, host, store, desk }
    }

    fn find<'a>(entries: &'a [StartupEntry], name: &str) -> &'a StartupEntry {
        entries.iter().find(|entry| entry.name == name).unwrap_or_else(|| panic!("{name}"))
    }

    #[test]
    fn the_view_has_the_account_first_and_keeps_only_ids() {
        let Fixture { desk, .. } = fixture();
        let view = desk.view();
        let names: Vec<&str> = view.entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["Discord", "Shinku", "SecurityHealth"]);
        let discord = find(&view.entries, "Discord");
        assert_eq!(discord.scope, Scope::User);
        assert_eq!(discord.publisher.as_deref(), Some("Discord Inc."));
        assert!(discord.adjustable && !discord.own);
        assert!(find(&view.entries, "Shinku").own);
        assert_eq!(find(&view.entries, "SecurityHealth").scope, Scope::Machine);
        assert!(view.unreadable.is_empty());
        // Ce que la page reçoit ne contient ni chemin ni commande ni nom de valeur brut.
        let json = serde_json::to_string(&view).unwrap();
        for forbidden in ["C:\\\\", "app.exe", "--demarrage", "rawName", "command"] {
            assert!(!json.contains(forbidden), "{forbidden} dans {json}");
        }
    }

    #[test]
    fn the_unreadable_scopes_are_listed_account_first_without_duplicates() {
        let Fixture { desk, host, .. } = fixture();
        for place in [Place::MachineFolder, Place::UserRun32, Place::MachineRun, Place::UserFolder]
        {
            host.make_unreadable(place);
        }
        assert_eq!(desk.view().unreadable, [Scope::User, Scope::Machine]);
    }

    #[test]
    fn cutting_then_restoring_goes_through_the_file_and_the_marks() {
        let Fixture { desk, host, store, .. } = fixture();
        let id = find(&desk.view().entries, "Discord").id.to_string();

        let view = desk.set(&id, false).unwrap();
        let discord = find(&view.entries, "Discord");
        assert_eq!(discord.state, State::Disabled);
        let now = host.now().unix_millis();
        assert_eq!(discord.mark, Some(Mark::CutByShinku { at_ms: now }));
        assert_eq!(discord.disabled_at, Some(now));
        let file = store.load();
        assert_eq!(file.cut.len(), 1);
        assert_eq!(file.cut[0].name, "Discord");
        assert_eq!(file.cut[0].file_time, host.file_time_now());

        let view = desk.set(&id, true).unwrap();
        let discord = find(&view.entries, "Discord");
        assert_eq!((discord.state, discord.mark), (State::Enabled, None));
        assert!(store.load().cut.is_empty(), "remettre par Shinku retire la ligne");
    }

    #[test]
    fn a_cut_made_in_windows_has_no_shinku_mark() {
        let Fixture { desk, host, store, .. } = fixture();
        host.set_bytes(Place::UserRun, "Discord", &cut_bytes(host.file_time_now()));
        let view = desk.view();
        let discord = find(&view.entries, "Discord");
        assert_eq!(discord.state, State::Disabled);
        assert_eq!(discord.mark, None);
        assert!(discord.disabled_at.is_some());
        assert!(!store.path().exists(), "rien à écrire");
    }

    #[test]
    fn a_cut_by_shinku_that_comes_back_alone_is_reported() {
        let Fixture { desk, host, .. } = fixture();
        let id = find(&desk.view().entries, "Discord").id.to_string();
        desk.set(&id, false).unwrap();
        // L'app se réinscrit et efface l'approbation.
        host.clear_approval(Place::UserRun, "Discord");
        let view = desk.view();
        let discord = find(&view.entries, "Discord");
        assert_eq!(discord.state, State::Enabled);
        assert!(matches!(discord.mark, Some(Mark::CameBack { .. })));
    }

    #[test]
    fn a_bad_id_is_an_unknown_entry_and_nothing_is_written() {
        let Fixture { desk, host, .. } = fixture();
        for id in [
            "",
            "n'importe quoi",
            "UR-8D8E4FCF12C5DAF9",
            "ur-8d8e4fcf12c5daf9 ",
            "ur-0000000000000000",
        ] {
            assert_eq!(desk.set(id, false).unwrap_err(), StartupRefusal::UnknownEntry, "{id:?}");
        }
        assert!(host.writes().is_empty());
    }

    #[test]
    fn the_pc_and_shinku_do_not_adjust() {
        let Fixture { desk, host, .. } = fixture();
        let view = desk.view();
        for name in ["SecurityHealth", "Shinku"] {
            let id = find(&view.entries, name).id.to_string();
            assert_eq!(desk.set(&id, false).unwrap_err(), StartupRefusal::NotAdjustable, "{name}");
        }
        assert!(host.writes().is_empty());
    }

    #[test]
    fn a_failure_is_reported_and_leaves_no_line() {
        let Fixture { desk, host, store, .. } = fixture();
        let id = find(&desk.view().entries, "Discord").id.to_string();
        host.refuse_writes();
        assert_eq!(desk.set(&id, false).unwrap_err(), StartupRefusal::Failed);
        assert!(store.load().cut.is_empty());
        host.ignore_writes();
        assert_eq!(desk.set(&id, false).unwrap_err(), StartupRefusal::Failed);
        assert!(store.load().cut.is_empty());
    }

    #[test]
    fn the_view_after_a_failure_shows_the_real_state() {
        let Fixture { desk, host, .. } = fixture();
        let before = desk.view();
        let id = find(&before.entries, "Discord").id.to_string();
        // Windows dit oui mais ne garde rien : la page recharge et voit ce qui est.
        host.ignore_writes();
        assert_eq!(desk.set(&id, false).unwrap_err(), StartupRefusal::Failed);
        assert_eq!(desk.view(), before, "rien n'a changé, et la vue le dit");
        // Quelqu'un coupe l'entrée dans Windows entre-temps : la vue d'après le montre.
        host.set_bytes(Place::UserRun, "Discord", &cut_bytes(host.file_time_now()));
        assert_eq!(desk.set(&id, true).unwrap_err(), StartupRefusal::Failed);
        let view = desk.view();
        let discord = find(&view.entries, "Discord");
        assert_eq!(discord.state, State::Disabled, "l'état réel, pas celui qu'on voulait");
        assert_eq!(discord.mark, None, "Shinku n'a rien coupé");
    }

    #[test]
    fn a_second_change_at_the_same_time_is_refused_as_busy() {
        let Fixture { desk, host, .. } = fixture();
        let id = find(&desk.view().entries, "Discord").id.to_string();
        let hold = desk.busy.lock().unwrap();
        assert_eq!(desk.set(&id, false).unwrap_err(), StartupRefusal::Busy);
        // Un identifiant mal formé est refusé avant, sans dépendre du verrou.
        assert_eq!(desk.set("x", false).unwrap_err(), StartupRefusal::UnknownEntry);
        // La lecture, elle, passe.
        assert_eq!(desk.view().entries.len(), 3);
        assert!(host.writes().is_empty());
        drop(hold);
        assert!(desk.set(&id, false).is_ok());
    }

    #[test]
    fn a_poisoned_lock_does_not_block_the_page_for_good() {
        let Fixture { desk, .. } = fixture();
        let id = find(&desk.view().entries, "Discord").id.to_string();
        let desk = Arc::new(desk);
        let other = Arc::clone(&desk);
        let _ = std::thread::spawn(move || {
            let _hold = other.busy.lock().unwrap();
            let _turn = other.turn.lock().unwrap();
            panic!("un fil qui meurt en tenant les verrous");
        })
        .join();
        assert!(desk.set(&id, false).is_ok());
        assert_eq!(desk.view().entries.len(), 3);
    }

    #[test]
    fn a_file_that_cannot_be_written_does_not_undo_the_change() {
        let temp = tempfile::tempdir().unwrap();
        let blocker = temp.path().join("fichier");
        std::fs::write(&blocker, "x").unwrap();
        // Le « dossier » de startup.json est un fichier : rien ne s'y écrit.
        let store = StartupStore::at(blocker.join("startup.json"));
        let host = Arc::new(FakeStartupHost::new());
        host.add(Place::UserRun, "Discord", r"C:\x.exe");
        let desk = StartupDesk::new(host, Some(store)).with_own_command(None);
        let id = find(&desk.view().entries, "Discord").id.to_string();
        let view = desk.set(&id, false).unwrap();
        let discord = find(&view.entries, "Discord");
        assert_eq!((discord.state, discord.mark), (State::Disabled, None));
    }

    #[test]
    fn without_a_file_the_desk_still_works() {
        let host = Arc::new(FakeStartupHost::new());
        host.add(Place::UserRun, "Discord", r"C:\x.exe");
        let desk = StartupDesk::new(host, None);
        let id = find(&desk.view().entries, "Discord").id.to_string();
        assert!(desk.set(&id, false).is_ok());
    }

    #[test]
    fn the_probe_counts_what_the_desk_would_list() {
        let Fixture { desk, host, .. } = fixture();
        // Discord et SecurityHealth : Shinku ne se compte pas.
        assert_eq!(desk.probe().count(), Some(2));
        host.make_unreadable(Place::UserRun32);
        assert_eq!(desk.probe().count(), None);
    }

    #[test]
    fn the_view_serializes_with_the_names_the_page_expects() {
        let Fixture { desk, host, .. } = fixture();
        host.set_bytes(Place::UserRun, "Discord", &cut_bytes(host.file_time_now()));
        let json = serde_json::to_value(desk.view()).unwrap();
        let discord = &json["entries"][0];
        assert_eq!(discord["scope"], "user");
        assert_eq!(discord["origin"], "registry");
        assert_eq!(discord["state"], "disabled");
        assert_eq!(discord["target"], "found");
        assert_eq!(discord["alias"], serde_json::Value::Null);
        assert_eq!(discord["host"], serde_json::Value::Null);
        assert_eq!(discord["hint"], serde_json::Value::Null);
        assert!(discord["disabledAt"].is_i64());
        assert!(discord["id"].as_str().unwrap().starts_with("ur-"));
        assert_eq!(json["unreadable"], serde_json::json!([]));
        assert_eq!(
            serde_json::to_value(Mark::CutByShinku { at_ms: 5 }).unwrap(),
            serde_json::json!({ "kind": "cutByShinku", "at": 5 })
        );
        assert_eq!(
            serde_json::to_value(Mark::CameBack { cut_ms: 6 }).unwrap(),
            serde_json::json!({ "kind": "cameBack", "cut": 6 })
        );
        assert_eq!(
            serde_json::to_value(crate::backend::BackendError::StartupRefused {
                reason: StartupRefusal::UnknownEntry
            })
            .unwrap(),
            serde_json::json!({ "kind": "startupRefused", "reason": "unknownEntry" })
        );
    }
}
