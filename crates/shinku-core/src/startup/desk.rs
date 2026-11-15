//! Le guichet du module « Démarrage » : ce que la fenêtre peut lire et demander.
//!
//! [`StartupDesk`] garde la machine et `startup.json`. La vue se refait à
//! chaque demande : aucune liste n'est gardée entre deux appels, la machine
//! peut avoir changé (un autre programme, le Gestionnaire des tâches). Un seul
//! changement à la fois : le second, s'il arrive pendant le premier, est refusé
//! ([`StartupRefusal::Busy`]).

use std::sync::{Arc, Mutex, TryLockError};

use crate::platform::agent::run_command;
use crate::startup::host::StartupHost;
use crate::startup::store::StartupStore;
use crate::startup::view::{StartupRefusal, StartupView};
use crate::startup::{self, EntryId, Probe, ToggleError};

/// Le module « Démarrage », vu du guichet.
#[derive(Debug)]
pub struct StartupDesk {
    host: Arc<dyn StartupHost>,
    store: Option<StartupStore>,
    /// La commande que le mode jeu écrit pour cette installation.
    own: Option<String>,
    /// Pris (sans attendre) par un changement en cours.
    busy: Mutex<()>,
    /// Met en file la lecture de la machine et l'écriture de `startup.json` :
    /// une vue et un changement ne s'écrasent pas.
    turn: Mutex<()>,
}

impl StartupDesk {
    /// Un guichet sur cette machine et ce fichier.
    pub fn new(host: Arc<dyn StartupHost>, store: Option<StartupStore>) -> Self {
        Self { host, store, own: run_command(), busy: Mutex::new(()), turn: Mutex::new(()) }
    }

    /// Change la commande qu'on prend pour celle de Shinku (pour les tests).
    pub fn with_own_command(mut self, own: Option<String>) -> Self {
        self.own = own;
        self
    }

    /// De quoi compter les apps inscrites à la fin d'une analyse.
    pub fn probe(&self) -> Probe {
        Probe::new(Arc::clone(&self.host), self.own.clone())
    }

    /// Tout ce que la page affiche. Met `startup.json` à jour.
    pub fn view(&self) -> StartupView {
        let _turn = self.turn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.read()
    }

    fn read(&self) -> StartupView {
        let listing =
            startup::enumerate(self.host.as_ref(), self.own.as_deref(), self.store.as_ref());
        StartupView::from(&listing)
    }

    /// Coupe ou remet une entrée du compte, puis rend la vue d'après.
    ///
    /// `id` est celui que la page a reçu : mal formé, il désigne une entrée
    /// inconnue. Un changement à la fois : s'il y en a un en cours, `Busy`.
    pub fn set(&self, id: &str, enabled: bool) -> Result<StartupView, StartupRefusal> {
        let id = EntryId::parse(id).ok_or(StartupRefusal::UnknownEntry)?;
        let _busy = match self.busy.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(StartupRefusal::Busy),
        };
        let _turn = self.turn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let change = startup::toggle(self.host.as_ref(), self.own.as_deref(), id, enabled)
            .map_err(|error| match error {
                ToggleError::UnknownEntry => StartupRefusal::UnknownEntry,
                ToggleError::NotAdjustable => StartupRefusal::NotAdjustable,
                ToggleError::Failed => StartupRefusal::Failed,
            })?;
        if change.changed
            && let Some(store) = &self.store
        {
            let mut file = store.load();
            if enabled {
                file.forget(id);
            } else {
                file.remember(id, &change.name, self.host.now().unix_millis(), change.file_time);
            }
            // Un fichier qui ne s'écrit pas n'annule pas le changement : la
            // page dira « Coupée le … » d'après Windows, sans « par Shinku ».
            let _ = store.save(&file);
        }
        Ok(self.read())
    }
}

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
