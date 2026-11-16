//! Le guichet du module « Désinstallation » : ce que la fenêtre peut lire et
//! demander.
//!
//! [`AppsDesk`] garde la machine et l'état de la désinstallation en cours (en
//! mémoire seulement). La liste des apps se refait à chaque demande : rien n'en
//! est gardé, la machine peut avoir changé. L'état de la désinstallation, lui,
//! ne se tient que sous un verrou **court** : ni le lancement du désinstalleur
//! (qui attend la réponse à la fenêtre de Windows), ni la recherche des restes
//! (un travail de disque), ni la mise à la corbeille ne le tiennent. `apps_view`
//! répond donc à tout moment.
//!
//! Un seul geste qui **écrit** à la fois : une mise à la corbeille en refuse une
//! autre, et refuse de partir pendant un grand vide ([`UninstallRefusal::Busy`]).
//! Le guichet principal s'en sert pour que le grand vide ne démarre pas en même
//! temps ([`AppsDesk::try_exclusive`]). Les recherches de restes (la première,
//! lancée par la page qui attend, et « Chercher à nouveau ») ne **lisent** que :
//! la première n'est pas exclusive, et `rescan` ne l'est que pour ne pas se
//! chevaucher avec une mise à la corbeille.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};

use crate::apps::follow::{Follow, Step, Tracked};
use crate::apps::host::{AppsHost, RawRegistration};
use crate::apps::launch::{Plan, plan};
use crate::apps::leftovers::{self, RemoveOutcome, RemoveResult, Snapshot};
use crate::apps::view::{AppRow, AppsView, Outcome, RemoveLeftovers, UninstallRefusal};
use crate::apps::{AppId, MAX_LEFTOVERS, Removal, app_from, list};

/// Le module « Désinstallation », vu du guichet.
#[derive(Debug)]
pub struct AppsDesk {
    host: Arc<dyn AppsHost>,
    follow: Mutex<Follow>,
    /// Pris (sans attendre) par une mise à la corbeille, un `rescan`, ou le
    /// démarrage d'un grand vide. La première recherche des restes, lancée par
    /// [`AppsDesk::view`], ne le prend pas : elle ne fait que lire.
    exclusive: Mutex<()>,
}

impl AppsDesk {
    /// Un guichet sur cette machine.
    pub fn new(host: Arc<dyn AppsHost>) -> Self {
        Self { host, follow: Mutex::new(Follow::new()), exclusive: Mutex::new(()) }
    }

    fn follow(&self) -> MutexGuard<'_, Follow> {
        self.follow.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Prend, sans attendre, le droit d'être seul : `None` si une mise à la
    /// corbeille ou une nouvelle recherche des restes (`rescan`) est en cours.
    pub fn try_exclusive(&self) -> Option<MutexGuard<'_, ()>> {
        match self.exclusive.try_lock() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    /// Tout ce que la page affiche. Relit aussi où en est la désinstallation en
    /// cours (la page l'appelle toutes les deux secondes).
    pub fn view(&self) -> AppsView {
        self.poll();
        self.build()
    }

    /// Un pas de suivi, puis, s'il le faut, la recherche des restes (hors verrou).
    fn poll(&self) {
        let step = self.follow().poll(self.host.as_ref());
        if let Step::Search { generation, snapshot, mut next_key } = step {
            let found = leftovers::search(self.host.as_ref(), &snapshot, &mut next_key);
            self.follow().apply_search(generation, found, next_key);
        }
    }

    /// La vue d'à présent, sans relire le suivi.
    fn build(&self) -> AppsView {
        let listing = list(self.host.as_ref());
        let (follow, kept) = {
            let mut follow = self.follow();
            (follow.view(self.host.uptime()), follow.take_kept())
        };
        AppsView {
            apps: listing.apps.iter().map(AppRow::from).collect(),
            complete: listing.complete,
            follow,
            outcome: kept.then_some(Outcome::Kept),
        }
    }

    /// Lance le désinstalleur de l'app de cet identifiant.
    ///
    /// L'inscription est **relue à l'instant** : la commande ne vient jamais de
    /// la page. Ne rend la main qu'après la réponse à la fenêtre de Windows ;
    /// pendant ce temps, [`view`](Self::view) répond (phase `launching`).
    pub fn start(&self, id: &str) -> Result<AppsView, UninstallRefusal> {
        let id = AppId::parse(id).ok_or(UninstallRefusal::NotOffered)?;
        if !self.follow().is_idle() {
            return Err(UninstallRefusal::Busy);
        }
        let host = self.host.as_ref();
        let raw: RawRegistration = match host.registration(id.source(), id.key()) {
            Ok(Some(raw)) => raw,
            Ok(None) => return Err(UninstallRefusal::Vanished),
            Err(_) => return Err(UninstallRefusal::Unreadable),
        };
        let own_dir = host.own_dir();
        let app = app_from(&raw, id.source(), own_dir.as_deref(), host)
            .filter(|app| app.removal == Removal::Offered)
            .ok_or(UninstallRefusal::NotOffered)?;
        let Plan::Run { program, parameters, elevated, installer } =
            plan(&raw, id.source(), id.key(), host)
        else {
            return Err(UninstallRefusal::Broken);
        };
        // La photo, prise avant de lancer : ensuite, l'inscription n'existe plus.
        let snapshot = Snapshot::take(
            &raw,
            &app.name,
            app.publisher.as_deref(),
            app.scope,
            &Plan::Run {
                program: program.clone(),
                parameters: parameters.clone(),
                elevated,
                installer,
            },
            host,
        );
        let tracked = Tracked { id, name: app.name, snapshot, installer, since: host.uptime() };
        // `Launching` avant de lancer : un second geste est refusé pendant que
        // la fenêtre de Windows attend la personne.
        let generation = self.follow().begin(tracked).ok_or(UninstallRefusal::Busy)?;
        let launched = host.launch(&program, &parameters, elevated, installer);
        self.follow().launched(generation, launched, host.uptime())?;
        Ok(self.view())
    }

    /// Revient à la liste. Ne tue rien, ne touche à rien.
    pub fn stop(&self) -> AppsView {
        self.follow().stop();
        self.build()
    }

    /// Cherche de nouveau les restes (phase `removed` seulement) : les cases
    /// de la page repartent vides.
    pub fn rescan(&self) -> Result<AppsView, UninstallRefusal> {
        let _exclusive = self.try_exclusive().ok_or(UninstallRefusal::Busy)?;
        let input = self.follow().rescan_input();
        if let Some((generation, snapshot, mut next_key)) = input {
            let found = leftovers::search(self.host.as_ref(), &snapshot, &mut next_key);
            self.follow().apply_search(generation, found, next_key);
        }
        Ok(self.build())
    }

    /// Met à la corbeille les restes de ces clés, jamais plus loin (phase
    /// `removed` seulement ; sinon tout est refusé, comme au-delà de 64 clés).
    /// `cleaning` dit si un grand vide est en cours : il est lu **une fois
    /// l'exclusivité prise**, car `start_clean` ne la rend qu'après le départ
    /// du grand vide ; lu avant, il pouvait être faux au moment de partir.
    pub fn remove(
        &self,
        keys: &[u32],
        cleaning: &dyn Fn() -> bool,
    ) -> Result<RemoveLeftovers, UninstallRefusal> {
        let _exclusive = self.try_exclusive().ok_or(UninstallRefusal::Busy)?;
        if cleaning() {
            return Err(UninstallRefusal::Busy);
        }
        let input = if keys.len() > MAX_LEFTOVERS { None } else { self.follow().removal_input() };
        let (results, bytes) = match input {
            Some((generation, snapshot, found)) => {
                let (results, bytes) =
                    leftovers::remove(self.host.as_ref(), &snapshot, &found, keys);
                self.follow().apply_removal(generation, &results);
                (results, bytes)
            }
            None => {
                let mut seen: Vec<u32> = Vec::new();
                let refused = keys.iter().take(MAX_LEFTOVERS).filter(|key| {
                    let new = !seen.contains(key);
                    seen.push(**key);
                    new
                });
                (
                    refused
                        .map(|&key| RemoveResult {
                            key,
                            outcome: RemoveOutcome::Refused,
                            reason: None,
                        })
                        .collect(),
                    0,
                )
            }
        };
        Ok(RemoveLeftovers { view: self.build(), results, bytes })
    }
}
