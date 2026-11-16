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

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};

use crate::apps::follow::{Follow, Step, Tracked};
use crate::apps::host::{AppsHost, RawRegistration};
use crate::apps::launch::{Plan, plan};
use crate::apps::leftovers::{self, RemoveOutcome, RemoveResult, Snapshot};
use crate::apps::notes::{UninstallNote, UninstallStore};
use crate::apps::view::{AppRow, AppsView, FollowView, Outcome, RemoveLeftovers, UninstallRefusal};
use crate::apps::{AppId, MAX_LEFTOVERS, Removal, app_from, list};

/// L'heure murale, en millisecondes depuis 1970. Les essais la remplacent.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// Les notes de désinstallation : le fichier, et la clé (l'heure) de la note
/// de la désinstallation en cours, tant qu'il y en a une.
struct Notes {
    store: Option<UninstallStore>,
    current: Option<i64>,
}

/// Le module « Désinstallation », vu du guichet.
pub struct AppsDesk {
    host: Arc<dyn AppsHost>,
    follow: Mutex<Follow>,
    /// `uninstalls.json`. Seul ce guichet l'écrit, et jamais sous le verrou du
    /// suivi : l'ordre est toujours suivi **puis plus rien**, ou notes seules.
    notes: Mutex<Notes>,
    clock: Clock,
    /// Pris (sans attendre) par une mise à la corbeille, un `rescan`, ou le
    /// démarrage d'un grand vide. La première recherche des restes, lancée par
    /// [`AppsDesk::view`], ne le prend pas : elle ne fait que lire.
    exclusive: Mutex<()>,
}

impl fmt::Debug for AppsDesk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppsDesk").finish_non_exhaustive()
    }
}

impl AppsDesk {
    /// Un guichet sur cette machine. `notes` est `uninstalls.json` (sans lui,
    /// rien n'est noté) et `clock` l'heure murale, qui date les notes.
    pub fn new(host: Arc<dyn AppsHost>, notes: Option<UninstallStore>, clock: Clock) -> Self {
        Self {
            host,
            follow: Mutex::new(Follow::new()),
            notes: Mutex::new(Notes { store: notes, current: None }),
            clock,
            exclusive: Mutex::new(()),
        }
    }

    /// Un guichet sans notes, pour les essais qui n'en parlent pas.
    #[cfg(test)]
    pub(crate) fn bare(host: Arc<dyn AppsHost>) -> Self {
        Self::new(host, None, Arc::new(|| 0))
    }

    fn notes_lock(&self) -> MutexGuard<'_, Notes> {
        self.notes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Les désinstallations notées, de la plus ancienne à la plus récente. Lit
    /// le fichier ; n'écrit rien.
    pub fn notes(&self) -> Vec<UninstallNote> {
        let notes = self.notes_lock();
        notes.store.as_ref().map(|store| store.load().notes).unwrap_or_default()
    }

    /// Oublie toutes les désinstallations notées : le fichier est vidé, et la
    /// note de la désinstallation en cours (si elle a une clé) ne sera pas
    /// recréée par une mise à la corbeille qui suit. Rien d'autre n'est touché.
    pub fn forget_notes(&self) {
        let mut notes = self.notes_lock();
        notes.current = None;
        if let Some(store) = &notes.store {
            // Une écriture qui échoue laisse les notes : la vue d'après le dira.
            let _ = store.save(&crate::apps::notes::NotesFile::default());
        }
    }

    /// Écrit la note d'une app vue partie, et en garde la clé.
    fn write_note(&self, name: &str) {
        let at_ms = (self.clock)();
        let mut notes = self.notes_lock();
        let Some(store) = &notes.store else { return };
        let mut file = store.load();
        file.push(UninstallNote::new(at_ms, name));
        // Une écriture qui échoue n'empêche rien : la clé n'est gardée que si la
        // note est partie.
        if store.save(&file).is_ok() {
            notes.current = Some(at_ms);
        }
    }

    /// Ajoute des restes mis à la corbeille à la note courante, s'il y en a une.
    fn add_leftovers(&self, count: u32, bytes: u64) {
        if count == 0 {
            return;
        }
        let notes = self.notes_lock();
        let Some(key) = notes.current else { return };
        let Some(store) = &notes.store else { return };
        let mut file = store.load();
        let Some(note) = file.notes.iter_mut().rev().find(|note| note.at_ms == key) else {
            return;
        };
        note.leftovers = note.leftovers.saturating_add(count);
        note.leftover_bytes = note.leftover_bytes.saturating_add(bytes);
        let _ = store.save(&file);
    }

    /// Oublie la clé de la note courante (le suivi est revenu à `Idle`, ou une
    /// autre désinstallation commence).
    fn end_note(&self) {
        self.notes_lock().current = None;
    }

    /// Relève le drapeau « l'app est partie » et écrit la note, hors du verrou
    /// du suivi.
    fn settle_note(&self) {
        let gone = self.follow().take_gone();
        if let Some(name) = gone {
            self.write_note(&name);
        }
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
        // Le drapeau est relevé avant de regarder si le suivi est revenu à `Idle` :
        // un « Revenir à la liste » après le passage garde sa note, et en oublie la clé.
        self.settle_note();
        if matches!(follow, FollowView::Idle) {
            self.end_note();
        }
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
        self.end_note();
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
        // La note existe avant que les restes partent, même si personne n'a relu la
        // vue depuis la recherche.
        self.settle_note();
        let input = if keys.len() > MAX_LEFTOVERS { None } else { self.follow().removal_input() };
        let (results, bytes) = match input {
            Some((snapshot, found)) => {
                let (results, bytes) =
                    leftovers::remove(self.host.as_ref(), &snapshot, &found, keys);
                self.follow().apply_removal(&results);
                // Seuls les restes réellement mis à la corbeille comptent : ni
                // `AlreadyGone`, ni `Refused`.
                let removed = results
                    .iter()
                    .filter(|result| result.outcome == RemoveOutcome::Removed)
                    .count();
                self.add_leftovers(u32::try_from(removed).unwrap_or(u32::MAX), bytes);
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
