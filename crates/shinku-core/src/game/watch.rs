//! La machine à états du mode jeu : un pas toutes les deux secondes.
//!
//! Le [`Watcher`] regarde la fenêtre au premier plan. Quand le même jeu reconnu
//! y reste deux pas de suite, il ouvre une partie : il note dans `game.json`
//! tout ce qu'il va changer (avec l'état d'avant), puis le change. À la fin de
//! la partie, il remet chaque chose en place, et note le bilan.
//!
//! Trois règles guident tout ce qui suit :
//!
//! - **l'intention d'abord** : rien n'est changé sur le PC avant que le journal
//!   ne le dise. Si le journal ne s'écrit pas (disque plein), rien n'est modifié ;
//! - **ne remettre que ce qu'on a posé** : une priorité, un masque d'efficacité
//!   ou un mode d'alimentation que la personne (ou un autre programme) a changé
//!   entre-temps n'est pas rétabli ;
//! - **le doute profite à la machine** : un processus qu'on ne peut pas lire ou
//!   modifier est laissé de côté, un mode d'alimentation inconnu n'est pas touché.

use std::collections::HashSet;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use crate::game::apps;
use crate::game::detect::{self, Lists, Source, file_name, key};
use crate::game::host::{
    Foreground, GameHost, PowerPlan, ProcessRef, ProcessState, RunningProcess, priority,
};
use crate::game::store::{
    GameFile, GameRef, GameStore, Last, PowerRecord, QUIETED_KEPT, Quieted, Session,
};
use crate::game::{PowerOutcome, unsuited_processor};
use crate::model::Timestamp;
use crate::settings::Settings;

/// Pas entre deux relevés des processus en veilleuse (≈ 30 s).
const STEPS_PER_SCAN: u32 = 15;
/// Pas entre deux nouvelles tentatives de remise en place (≈ 30 s).
const STEPS_PER_RETRY: u32 = 15;
/// Nombre de tentatives de remise en place avant d'abandonner.
const RESTORE_ATTEMPTS: u32 = 20;
/// Pas consécutifs pour qu'un jeu soit reconnu comme le jeu en cours.
const CONFIRM_STEPS: u32 = 2;
/// Temps hors du premier plan au bout duquel la partie est finie : un lanceur
/// resté ouvert en arrière-plan ne garde pas le PC en mode jeu.
const BACKGROUND_LIMIT: Duration = Duration::from_secs(600);
/// Délai avant de réessayer d'ouvrir une partie quand le journal ne s'est pas écrit.
const WRITE_RETRY: Duration = Duration::from_secs(60);

/// Ce que le mode jeu fait en ce moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// Aucun jeu.
    Idle,
    /// Un jeu reconnu est au premier plan, mais pas depuis assez longtemps.
    Candidate,
    /// Une partie est ouverte.
    Playing,
}

/// Un jeu reconnu au premier plan, qui attend d'y rester assez longtemps.
#[derive(Debug)]
struct Candidate {
    key: String,
    steps: u32,
}

/// Ce que le watcher retient d'une partie ouverte, en plus du journal.
#[derive(Debug)]
struct Live {
    /// Dernier instant où le jeu était au premier plan.
    last_foreground: Timestamp,
    /// Pas depuis le dernier relevé des processus.
    steps_since_scan: u32,
    /// La partie est finie, mais tout n'a pas pu être remis en place.
    ending: bool,
    /// Tentatives de remise en place (la première comprise).
    attempts: u32,
    /// Pas depuis la dernière tentative.
    steps_since_retry: u32,
    /// Une remise en place a été abandonnée : le bilan le dira.
    incomplete: bool,
    /// Noms des apps déjà remises à leur vitesse pendant cette partie.
    apps_restored: Vec<String>,
    /// Un autre jeu reconnu au premier plan, et depuis combien de pas.
    switch: Option<(String, u32)>,
    /// Processus qu'on n'a pas pu modifier : à retirer du journal au pas suivant.
    dropped: Vec<ProcessRef>,
    /// Processus qu'on n'a pas pu modifier : on n'y revient pas.
    unmodifiable: HashSet<ProcessRef>,
    /// L'app en veilleuse que la personne regarde, si c'en est une.
    watched: Option<String>,
    /// La liste des apps en veilleuse au dernier pas.
    quiet_names: Option<Vec<String>>,
    /// Pas à attendre avant de réessayer le changement d'alimentation.
    power_retry: u32,
}

impl Live {
    fn new(now: Timestamp) -> Self {
        Self {
            last_foreground: now,
            steps_since_scan: 0,
            ending: false,
            attempts: 0,
            steps_since_retry: 0,
            incomplete: false,
            apps_restored: Vec::new(),
            switch: None,
            dropped: Vec::new(),
            unmodifiable: HashSet::new(),
            watched: None,
            quiet_names: None,
            power_retry: 0,
        }
    }
}

/// Ce que le mode jeu veut faire de l'alimentation à cet instant.
enum PowerWish {
    /// Pas de changement : voici pourquoi.
    Outcome(PowerOutcome),
    /// Passer en « Performances élevées » ; `previous` est le mode d'avant.
    Apply(PowerPlan),
    /// Le mode posé par Shinku est en place : on n'y touche pas.
    Keep,
}

/// Ce qui a pu être remis en place.
#[derive(Debug)]
struct Restoration {
    /// L'alimentation après la remise.
    power: PowerRecord,
    /// `Some(true)` remise, `Some(false)` échec, `None` jamais changée.
    power_restored: Option<bool>,
    /// Les processus qu'on n'a pas pu remettre.
    left: Vec<Quieted>,
    /// Les apps remises, une entrée par processus.
    apps: Vec<String>,
    /// Tout est revenu.
    complete: bool,
}

/// La machine à états du mode jeu.
#[derive(Debug)]
pub struct Watcher {
    host: Arc<dyn GameHost>,
    store: GameStore,
    /// Le contenu de `game.json`, tel que Shinku le connaît.
    file: GameFile,
    live: Option<Live>,
    candidate: Option<Candidate>,
    /// Pas d'ouverture de partie avant cet instant (le journal ne s'écrivait pas).
    write_retry_at: Option<Timestamp>,
    /// Processus dont on a vérifié que c'est le jeu.
    verified: HashSet<ProcessRef>,
    /// Processus de même nom que le jeu, mais d'un autre chemin.
    rejected: HashSet<ProcessRef>,
    unsuited: Option<bool>,
}

/// L'instant `delay` après `from`.
fn later(from: Timestamp, delay: Duration) -> Timestamp {
    let ticks = i64::try_from(delay.as_nanos() / 100).unwrap_or(i64::MAX);
    Timestamp::from_ticks(from.ticks().saturating_add(ticks))
}

/// Un instant exprimé en millisecondes depuis 1970 (c'est ainsi que `game.json`
/// note ses dates).
fn from_millis(millis: i64) -> Timestamp {
    Timestamp::from_ticks(millis.saturating_mul(10_000))
}

/// Les noms des apps à mettre en veilleuse avec ces réglages.
fn quiet_names(settings: &Settings) -> Vec<String> {
    if settings.game_mode_quiet_apps { apps::effective(settings) } else { Vec::new() }
}

/// L'état dans lequel mettre un processus pour le mettre en veilleuse, ou
/// `None` s'il n'y a rien à baisser. On ne fait que baisser : une priorité déjà
/// basse reste ce qu'elle est.
fn quiet_target(before: ProcessState) -> Option<ProcessState> {
    let mut target = before;
    if matches!(before.priority, priority::NORMAL | priority::ABOVE_NORMAL | priority::HIGH) {
        target.priority = priority::BELOW_NORMAL;
    }
    target.throttle_control |= 1;
    target.throttle_state |= 1;
    (target != before).then_some(target)
}

/// L'état dans lequel remettre un processus, ou `None` s'il n'y a rien à
/// remettre : on ne rend la priorité que si elle vaut encore « sous la
/// normale », et les masques que s'ils valent encore ce que Shinku a posé.
fn restoring_state(entry: &Quieted, current: ProcessState) -> Option<ProcessState> {
    let mut target = current;
    let lowered = !matches!(entry.priority, priority::IDLE | priority::BELOW_NORMAL);
    if lowered && current.priority == priority::BELOW_NORMAL {
        target.priority = entry.priority;
    }
    let posed = (entry.throttle_control | 1, entry.throttle_state | 1);
    let before = (entry.throttle_control, entry.throttle_state);
    if posed != before && (current.throttle_control, current.throttle_state) == posed {
        target.throttle_control = entry.throttle_control;
        target.throttle_state = entry.throttle_state;
    }
    (target != current).then_some(target)
}

fn entry_of(process: &RunningProcess, before: ProcessState) -> Quieted {
    Quieted {
        pid: process.process.pid,
        started: process.process.started,
        exe: process.exe_name.clone(),
        priority: before.priority,
        throttle_control: before.throttle_control,
        throttle_state: before.throttle_state,
    }
}

fn process_of(entry: &Quieted) -> ProcessRef {
    ProcessRef { pid: entry.pid, started: entry.started }
}

impl Watcher {
    /// Une machine à états sur cette machine et ce journal.
    pub fn new(host: Arc<dyn GameHost>, store: GameStore) -> Self {
        let file = store.load();
        Self {
            host,
            store,
            file,
            live: None,
            candidate: None,
            write_retry_at: None,
            verified: HashSet::new(),
            rejected: HashSet::new(),
            unsuited: None,
        }
    }

    /// Vrai si une partie est ouverte (ou à moitié remise en place).
    pub fn has_session(&self) -> bool {
        self.live.is_some()
    }

    /// À appeler au démarrage de l'agent. Si le journal contient une partie :
    /// si le jeu vit encore, elle reprend telle quelle ; sinon elle se termine
    /// normalement, et tout est remis en place (l'alimentation aussi, si le mode
    /// actif est encore celui que Shinku avait posé). Sans effet si une partie
    /// est déjà suivie, et sans danger si on l'appelle deux fois.
    pub fn recover(&mut self) {
        if self.live.is_some() {
            return;
        }
        self.file = self.store.load();
        let Some(session) = self.file.session.clone() else { return };
        let alive = self.game_alive(
            &session.game.path,
            &self.host.processes(),
            self.host.foreground().as_ref(),
        );
        self.live = Some(Live::new(self.host.now()));
        // Une remise en place à moitié faite d'une partie finie, elle, reprend.
        if alive && (!session.restore_pending || session.released) {
            if session.restore_pending
                && let Some(live) = self.live.as_mut()
            {
                live.attempts = 1;
            }
            return;
        }
        self.finish();
    }

    /// Un pas : à appeler toutes les deux secondes avec les réglages en vigueur.
    pub fn tick(&mut self, settings: &Settings) -> Activity {
        self.tick_detecting(settings, true)
    }

    /// Un pas, avec ou sans détection de nouvelles parties. Sans détection, une
    /// partie en cours continue d'être suivie et se termine normalement ; seule
    /// l'ouverture d'une partie (et le changement de jeu) est suspendue.
    pub fn tick_detecting(&mut self, settings: &Settings, detect: bool) -> Activity {
        if self.live.is_some() {
            self.tick_session(settings, detect);
        } else if settings.game_mode_auto && detect {
            self.tick_candidate(settings);
        } else {
            self.candidate = None;
        }
        if self.live.is_some() {
            Activity::Playing
        } else if self.candidate.is_some() {
            Activity::Candidate
        } else {
            Activity::Idle
        }
    }

    /// « Tout remettre maintenant » : tout est remis en place tout de suite. La
    /// partie reste ouverte, sans rien appliquer, jusqu'à sa fin normale : le
    /// mode jeu ne se réactive pas aussitôt.
    pub fn release(&mut self) {
        let Some(session) = self.file.session.clone() else { return };
        if session.released || self.live.is_none() {
            return;
        }
        let done = self.restore_session(&session);
        let pending = !done.complete;
        if let Some(live) = self.live.as_mut() {
            live.apps_restored.extend(done.apps.iter().cloned());
            live.ending = false;
            live.attempts = 1;
            live.steps_since_retry = 0;
        }
        self.change_anyway(|file| {
            if let Some(session) = file.session.as_mut() {
                session.released = true;
                session.power = done.power;
                session.quieted = done.left;
                session.restore_pending = pending;
            }
        });
    }

    /// Fermeture ou déconnexion de la session Windows, arrêt de l'agent,
    /// désinstallation : tout est remis en place et la partie se termine tout de
    /// suite. La détection normale reprendra ensuite. Si quelque chose n'a pas
    /// pu être remis, le journal le garde : la prochaine reprise réessaie.
    pub fn suspend(&mut self) {
        self.candidate = None;
        if self.live.is_some() {
            self.finish();
        }
    }

    // ---------------------------------------------------------------- journal

    /// Applique un changement au journal et l'écrit. Si l'écriture échoue, rien
    /// ne change, ni en mémoire ni sur le disque.
    fn change(&mut self, edit: impl FnOnce(&mut GameFile)) -> io::Result<()> {
        let mut file = self.file.clone();
        edit(&mut file);
        self.store.save(&file)?;
        self.file = file;
        Ok(())
    }

    /// Applique un changement au journal, même si l'écriture échoue : le geste
    /// qu'il décrit est déjà fait.
    fn change_anyway(&mut self, edit: impl FnOnce(&mut GameFile)) {
        edit(&mut self.file);
        // Rien de plus à faire si le disque refuse : la prochaine transition réessaiera.
        let _ = self.store.save(&self.file);
    }

    // -------------------------------------------------------------- détection

    /// Compte les pas d'un jeu reconnu au premier plan, et ouvre une partie au
    /// deuxième.
    fn tick_candidate(&mut self, settings: &Settings) {
        let Some(foreground) = self.host.foreground() else {
            self.candidate = None;
            return;
        };
        let Some(source) = detect::recognise(&foreground.exe, &Lists::of(settings)) else {
            self.candidate = None;
            return;
        };
        let wanted = key(&foreground.exe);
        let steps = match &self.candidate {
            Some(known) if known.key == wanted => known.steps + 1,
            _ => 1,
        };
        self.candidate = Some(Candidate { key: wanted, steps });
        let waiting = self.write_retry_at.is_some_and(|at| self.host.now() < at);
        if steps >= CONFIRM_STEPS && !waiting {
            self.activate(settings, &foreground, source);
        }
    }

    /// Nom à montrer pour ce jeu.
    fn game_name(&self, exe: &str, source: Source) -> String {
        match source {
            Source::Library => {
                detect::library_title(exe).unwrap_or_else(|| self.host.display_name(exe))
            }
            Source::Added => self.host.display_name(exe),
        }
    }

    /// Vrai si un processus de la session est le jeu : même chemin d'exécutable.
    ///
    /// On compare d'abord le nom, on n'ouvre que les processus de ce nom, et on
    /// garde en mémoire ceux qu'on a déjà vérifiés pour ne pas rouvrir le jeu à
    /// chaque pas.
    fn game_alive(
        &mut self,
        game: &str,
        processes: &[RunningProcess],
        foreground: Option<&Foreground>,
    ) -> bool {
        let present: HashSet<ProcessRef> = processes.iter().map(|p| p.process).collect();
        self.verified.retain(|process| present.contains(process));
        self.rejected.retain(|process| present.contains(process));
        let wanted = key(game);
        if let Some(foreground) = foreground
            && key(&foreground.exe) == wanted
        {
            self.verified.insert(foreground.process);
        }
        if !self.verified.is_empty() {
            return true;
        }
        let name = file_name(game);
        for process in processes.iter().filter(|p| p.exe_name == name) {
            if self.rejected.contains(&process.process) {
                continue;
            }
            match self.host.exe_path(process.process) {
                Some(path) if key(&path) == wanted => {
                    self.verified.insert(process.process);
                    return true;
                }
                // Un autre programme du même nom, ou un processus qui ne s'ouvre
                // pas : on ne le rouvrira pas à chaque pas.
                _ => {
                    self.rejected.insert(process.process);
                }
            }
        }
        false
    }

    /// Vrai si ce processus est celui du jeu en cours.
    fn is_game_process(&mut self, process: &RunningProcess, game: &str) -> bool {
        if self.verified.contains(&process.process) {
            return true;
        }
        if self.rejected.contains(&process.process) || process.exe_name != file_name(game) {
            return false;
        }
        match self.host.exe_path(process.process) {
            Some(path) if key(&path) == key(game) => {
                self.verified.insert(process.process);
                true
            }
            _ => {
                self.rejected.insert(process.process);
                false
            }
        }
    }

    // ------------------------------------------------------------- activation

    /// Ouvre une partie : préparer, écrire le journal, puis agir.
    fn activate(&mut self, settings: &Settings, foreground: &Foreground, source: Source) {
        let now = self.host.now();
        self.verified.clear();
        self.rejected.clear();
        self.verified.insert(foreground.process);
        let game = GameRef {
            path: foreground.exe.clone(),
            name: self.game_name(&foreground.exe, source),
            source,
        };

        // a. Préparer : l'état voulu de l'alimentation, et l'état d'avant de
        // chaque processus à mettre en veilleuse.
        let power = match self.wanted_power(settings, false) {
            PowerWish::Apply(previous) => PowerRecord::applied(previous),
            PowerWish::Outcome(outcome) => PowerRecord::of(outcome),
            PowerWish::Keep => PowerRecord::of(PowerOutcome::Off),
        };
        let names = quiet_names(settings);
        let processes = self.host.processes();
        let planned = self.plan_quiet(&processes, &game.path, &names, None, &[], &HashSet::new());
        let session = Session {
            game,
            since: now.unix_millis(),
            released: false,
            power,
            quieted: planned
                .iter()
                .map(|(process, before, _)| entry_of(process, *before))
                .collect(),
            restore_pending: false,
        };

        // b. Écrire la session complète, d'une seule écriture : l'intention
        // d'abord. Si le journal ne s'écrit pas, rien n'est modifié.
        if self.change(|file| file.session = Some(session)).is_err() {
            self.write_retry_at = Some(later(now, WRITE_RETRY));
            return;
        }
        self.write_retry_at = None;
        self.candidate = None;
        let mut live = Live::new(now);
        live.quiet_names = Some(names);
        self.live = Some(live);

        // c. Agir : l'alimentation, puis chaque processus. Un processus qu'on ne
        // peut pas modifier est laissé de côté en silence.
        if power.is_held() && self.host.set_power_plan(PowerPlan::HIGH_PERFORMANCE).is_err() {
            self.change_anyway(|file| {
                if let Some(session) = file.session.as_mut() {
                    session.power = PowerRecord::of(PowerOutcome::Refused);
                }
            });
        }
        self.act_on(planned);
    }

    /// Met ces processus en veilleuse. Ceux qu'on ne peut pas modifier sont
    /// notés pour être retirés du journal au pas suivant.
    fn act_on(&mut self, planned: Vec<(RunningProcess, ProcessState, ProcessState)>) {
        for (process, _, target) in planned {
            if self.host.apply(process.process, target).is_err()
                && let Some(live) = self.live.as_mut()
            {
                live.dropped.push(process.process);
                live.unmodifiable.insert(process.process);
            }
        }
    }

    /// Les processus à mettre en veilleuse : ceux dont le nom est dans la liste,
    /// qui ne sont ni protégés, ni le jeu, ni l'app que la personne regarde, ni
    /// déjà dans le journal, et qu'on peut lire. Rend le processus, son état
    /// d'avant et l'état voulu.
    fn plan_quiet(
        &mut self,
        processes: &[RunningProcess],
        game: &str,
        names: &[String],
        watched: Option<&str>,
        current: &[Quieted],
        unmodifiable: &HashSet<ProcessRef>,
    ) -> Vec<(RunningProcess, ProcessState, ProcessState)> {
        let mut planned = Vec::new();
        for process in processes {
            if !names.contains(&process.exe_name)
                || apps::protected(&process.exe_name)
                || watched == Some(process.exe_name.as_str())
                || unmodifiable.contains(&process.process)
                || current.iter().any(|entry| process_of(entry) == process.process)
                || self.is_game_process(process, game)
            {
                continue;
            }
            // Un processus qu'on ne peut pas lire est laissé de côté.
            let Some(before) = self.host.inspect(process.process) else { continue };
            if !priority::is_known(before.priority) {
                continue;
            }
            if let Some(target) = quiet_target(before) {
                planned.push((process.clone(), before, target));
            }
        }
        planned
    }

    // --------------------------------------------------------- alimentation

    fn unsuited(&mut self) -> bool {
        *self.unsuited.get_or_insert_with(|| unsuited_processor(&self.host.processor_name()))
    }

    /// Ce que le réglage et l'état du PC demandent pour l'alimentation à cet
    /// instant. `held` : Shinku a déjà posé « Performances élevées ».
    fn wanted_power(&mut self, settings: &Settings, held: bool) -> PowerWish {
        if !settings.game_mode_power {
            return PowerWish::Outcome(PowerOutcome::Off);
        }
        if self.unsuited() {
            return PowerWish::Outcome(PowerOutcome::Unsuited);
        }
        if !self.host.has_power_plan(PowerPlan::HIGH_PERFORMANCE) {
            return PowerWish::Outcome(PowerOutcome::Unavailable);
        }
        if self.host.on_battery() {
            return PowerWish::Outcome(PowerOutcome::OnBattery);
        }
        if held {
            return PowerWish::Keep;
        }
        match self.host.power_plan() {
            Some(PowerPlan::HIGH_PERFORMANCE) => PowerWish::Outcome(PowerOutcome::AlreadyFast),
            Some(active) if active == PowerPlan::BALANCED || active == PowerPlan::POWER_SAVER => {
                PowerWish::Apply(active)
            }
            // Un mode du constructeur, personnalisé, ou illisible : on n'y touche pas.
            _ => PowerWish::Outcome(PowerOutcome::Custom),
        }
    }

    /// Remet le mode d'alimentation d'avant, mais seulement si le mode actif est
    /// encore celui que Shinku a posé : si la personne l'a changé entre-temps,
    /// son choix reste. Rend l'alimentation après la remise, et `Some(true)` si
    /// elle est remise, `Some(false)` si elle n'a pas pu l'être, `None` si Shinku
    /// ne l'avait pas changée.
    fn restore_power(&self, record: &PowerRecord) -> (PowerRecord, Option<bool>) {
        if record.outcome != PowerOutcome::Applied {
            return (*record, None);
        }
        let (Some(previous), Some(applied)) = (record.previous, record.applied) else {
            // Déjà remise (relâchée plus tôt).
            return (*record, Some(true));
        };
        let restored = PowerRecord { previous: None, applied: None, ..*record };
        match self.host.power_plan() {
            Some(active) if active == applied => match self.host.set_power_plan(previous) {
                Ok(()) => (restored, Some(true)),
                Err(_) => (*record, Some(false)),
            },
            // La personne a choisi un autre mode : il reste.
            Some(_) => (restored, Some(true)),
            // Impossible de savoir : on réessaiera.
            None => (*record, Some(false)),
        }
    }

    /// Réévalue l'alimentation, à chaque pas.
    fn power_step(&mut self, settings: &Settings) {
        let Some(record) = self.file.session.as_ref().map(|session| session.power) else { return };
        let wish = self.wanted_power(settings, record.is_held());
        match wish {
            PowerWish::Keep => {}
            PowerWish::Outcome(outcome) => {
                if record.is_held() {
                    let (_, restored) = self.restore_power(&record);
                    if restored == Some(false) {
                        // Réessayé au pas suivant.
                        return;
                    }
                } else if record.outcome == outcome {
                    return;
                }
                self.change_anyway(|file| {
                    if let Some(session) = file.session.as_mut() {
                        session.power = PowerRecord::of(outcome);
                    }
                });
            }
            PowerWish::Apply(previous) => {
                // Windows a refusé : on n'insiste pas tant que la situation ne change pas.
                if record.outcome == PowerOutcome::Refused {
                    return;
                }
                if let Some(live) = self.live.as_mut()
                    && live.power_retry > 0
                {
                    live.power_retry -= 1;
                    return;
                }
                // L'intention d'abord.
                let intention = PowerRecord::applied(previous);
                let written = self.change(|file| {
                    if let Some(session) = file.session.as_mut() {
                        session.power = intention;
                    }
                });
                if written.is_err() {
                    if let Some(live) = self.live.as_mut() {
                        live.power_retry = STEPS_PER_RETRY;
                    }
                    return;
                }
                if self.host.set_power_plan(PowerPlan::HIGH_PERFORMANCE).is_err() {
                    self.change_anyway(|file| {
                        if let Some(session) = file.session.as_mut() {
                            session.power = PowerRecord::of(PowerOutcome::Refused);
                        }
                    });
                }
            }
        }
    }

    // ------------------------------------------------------------ remise en place

    /// Remet un ensemble de processus : chacun à son état d'avant, sous les
    /// conditions de [`restoring_state`], puis les descendants des processus du
    /// journal qui ont hérité de la priorité basse. Rend ceux qu'on n'a pas pu
    /// remettre et le nom des apps remises (une entrée par processus).
    ///
    /// Un processus qui n'existe plus, ou qu'on ne peut plus lire, n'a rien à
    /// recevoir : il n'est pas rendu.
    fn restore_entries(
        &self,
        since: i64,
        entries: &[Quieted],
        staying: &[Quieted],
    ) -> (Vec<Quieted>, Vec<String>) {
        let mut failed = Vec::new();
        let mut apps = Vec::new();
        for entry in entries {
            let process = process_of(entry);
            let Some(current) = self.host.inspect(process) else { continue };
            let Some(target) = restoring_state(entry, current) else { continue };
            match self.host.apply(process, target) {
                Ok(()) => apps.push(entry.exe.clone()),
                Err(_) => failed.push(entry.clone()),
            }
        }
        self.restore_descendants(from_millis(since), entries, staying);
        (failed, apps)
    }

    /// Un processus créé par un processus en « sous la normale » hérite de cette
    /// classe. Tout descendant d'un processus du journal, créé après le début
    /// de la partie et trouvé en « sous la normale », est remis en « normale »,
    /// quel que soit son nom. Le parent doit avoir été créé avant l'enfant : un
    /// pid repris par un autre processus ne fait pas un parent.
    fn restore_descendants(&self, since: Timestamp, roots: &[Quieted], staying: &[Quieted]) {
        if roots.is_empty() {
            return;
        }
        let processes = self.host.processes();
        let mut seen: HashSet<ProcessRef> = roots.iter().chain(staying).map(process_of).collect();
        // Un pid repris par un autre processus n'est plus celui du journal : ses
        // « enfants » sont ceux de l'autre.
        let mut queue: Vec<(u32, i64)> = roots
            .iter()
            .filter(|root| {
                !processes
                    .iter()
                    .any(|p| p.process.pid == root.pid && p.process.started != root.started)
            })
            .map(|root| (root.pid, root.started))
            .collect();
        let mut index = 0;
        while let Some(&(parent, parent_started)) = queue.get(index) {
            index += 1;
            for child in &processes {
                if child.parent != parent
                    || child.process.started <= parent_started
                    || Timestamp::from_filetime(child.process.started) <= since
                    || !seen.insert(child.process)
                {
                    continue;
                }
                queue.push((child.process.pid, child.process.started));
                if let Some(state) = self.host.inspect(child.process)
                    && state.priority == priority::BELOW_NORMAL
                {
                    // Un échec n'est pas retenté : ce processus n'est pas au journal.
                    let _ = self
                        .host
                        .apply(child.process, ProcessState { priority: priority::NORMAL, ..state });
                }
            }
        }
    }

    /// Remet tout ce que cette partie a changé.
    fn restore_session(&self, session: &Session) -> Restoration {
        let (power, power_restored) = self.restore_power(&session.power);
        let (left, apps) = self.restore_entries(session.since, &session.quieted, &[]);
        let complete = power_restored != Some(false) && left.is_empty();
        Restoration { power, power_restored, left, apps, complete }
    }

    /// Termine la partie : tout est remis en place, puis le bilan est noté. Si
    /// quelque chose n'a pas pu être remis, la partie reste au journal avec
    /// `restorePending`, et la remise est retentée.
    fn finish(&mut self) {
        let Some(session) = self.file.session.clone() else {
            self.live = None;
            return;
        };
        let done = self.restore_session(&session);
        if done.complete {
            self.close(&session, &done);
        } else {
            self.mark_pending(done, true);
        }
    }

    /// La partie est remise en place : bilan, jeux vus, journal vide.
    fn close(&mut self, session: &Session, done: &Restoration) {
        let now = self.host.now();
        let (mut apps, incomplete) =
            self.live.take().map(|live| (live.apps_restored, live.incomplete)).unwrap_or_default();
        apps.extend(done.apps.iter().cloned());
        apps.sort_unstable();
        apps.dedup();
        let power_restored = match session.power.outcome {
            PowerOutcome::Applied if session.power.is_held() => done.power_restored,
            PowerOutcome::Applied => Some(true),
            _ => None,
        };
        let last = Last {
            name: session.game.name.clone(),
            path: session.game.path.clone(),
            started: session.since,
            ended: now.unix_millis(),
            power_restored,
            apps_restored: u32::try_from(apps.len()).unwrap_or(u32::MAX),
            complete: done.complete && !incomplete,
        };
        self.change_anyway(|file| {
            if session.game.source == Source::Library {
                file.remember(&session.game.path, &session.game.name, now.unix_millis());
            }
            file.last = Some(last);
            file.session = None;
        });
        self.candidate = None;
        self.verified.clear();
        self.rejected.clear();
    }

    /// Quelque chose n'a pas pu être remis : la partie reste, avec ce qui reste à remettre.
    fn mark_pending(&mut self, done: Restoration, ending: bool) {
        if let Some(live) = self.live.as_mut() {
            live.ending = ending;
            live.attempts = 1;
            live.steps_since_retry = 0;
            live.apps_restored.extend(done.apps);
        }
        self.change_anyway(|file| {
            if let Some(session) = file.session.as_mut() {
                session.power = done.power;
                session.quieted = done.left;
                session.restore_pending = true;
            }
        });
    }

    /// Retente une remise en place restée à moitié, tous les 15 pas, jusqu'à 20
    /// essais : au-delà, on abandonne, et le bilan dit que tout n'est pas revenu.
    fn retry_pending(&mut self) {
        let Some(live) = self.live.as_mut() else { return };
        live.steps_since_retry += 1;
        if live.steps_since_retry < STEPS_PER_RETRY {
            return;
        }
        live.steps_since_retry = 0;
        live.attempts += 1;
        let (attempts, ending) = (live.attempts, live.ending);
        let Some(session) = self.file.session.clone() else { return };
        let done = self.restore_session(&session);
        let give_up = !done.complete && attempts >= RESTORE_ATTEMPTS;
        if ending && (done.complete || give_up) {
            self.close(&session, &done);
            return;
        }
        if let Some(live) = self.live.as_mut() {
            live.apps_restored.extend(done.apps.iter().cloned());
            live.incomplete |= give_up;
        }
        self.change_anyway(|file| {
            if let Some(session) = file.session.as_mut() {
                session.power = done.power;
                session.quieted = done.left;
                session.restore_pending = !done.complete && !give_up;
            }
        });
    }

    // --------------------------------------------------------- partie en cours

    /// Un pas pendant une partie.
    fn tick_session(&mut self, settings: &Settings, detect: bool) {
        let Some(session) = self.file.session.clone() else {
            self.live = None;
            return;
        };
        if self.live.as_ref().is_some_and(|live| live.ending) {
            self.retry_pending();
            return;
        }
        let now = self.host.now();
        let lists = Lists::of(settings);
        // Fin : le mode automatique est éteint, ou le jeu n'est plus reconnu
        // (« Ce n'est pas un jeu »).
        if !settings.game_mode_auto || detect::recognise(&session.game.path, &lists).is_none() {
            self.finish();
            return;
        }
        let foreground = self.host.foreground();
        let processes = self.host.processes();
        // Fin : plus aucun processus du jeu ne vit.
        if !self.game_alive(&session.game.path, &processes, foreground.as_ref()) {
            self.finish();
            return;
        }
        let on_game = foreground.as_ref().is_some_and(|f| key(&f.exe) == key(&session.game.path));
        if on_game {
            if let Some(live) = self.live.as_mut() {
                live.last_foreground = now;
                live.switch = None;
            }
        } else {
            // Fin : le jeu n'est pas au premier plan depuis trop longtemps.
            let away = self.live.as_ref().map(|live| now.since(live.last_foreground));
            if away.is_some_and(|away| away >= BACKGROUND_LIMIT) {
                self.finish();
                return;
            }
            if detect && let Some((next, source)) = self.watch_switch(foreground.as_ref(), &lists) {
                self.switch_to(&next, source);
                return;
            }
        }
        self.maintain(settings, &processes, foreground.as_ref());
    }

    /// Un autre jeu reconnu est-il au premier plan depuis deux pas ?
    fn watch_switch(
        &mut self,
        foreground: Option<&Foreground>,
        lists: &Lists<'_>,
    ) -> Option<(Foreground, Source)> {
        let live = self.live.as_mut()?;
        let Some(source) = foreground.and_then(|f| detect::recognise(&f.exe, lists)) else {
            live.switch = None;
            return None;
        };
        let foreground = foreground?;
        let wanted = key(&foreground.exe);
        let steps = match &live.switch {
            Some((known, steps)) if *known == wanted => steps + 1,
            _ => 1,
        };
        live.switch = Some((wanted, steps));
        (steps >= CONFIRM_STEPS).then(|| (foreground.clone(), source))
    }

    /// La partie change de jeu, sans rien remettre ni réappliquer : seuls le jeu
    /// et le début changent. L'ancien jeu passe dans le bilan et les jeux vus.
    fn switch_to(&mut self, foreground: &Foreground, source: Source) {
        let Some(old) = self.file.session.clone() else { return };
        let now = self.host.now();
        let game = GameRef {
            path: foreground.exe.clone(),
            name: self.game_name(&foreground.exe, source),
            source,
        };
        self.change_anyway(|file| {
            if old.game.source == Source::Library {
                file.remember(&old.game.path, &old.game.name, now.unix_millis());
            }
            file.last = Some(Last {
                name: old.game.name.clone(),
                path: old.game.path.clone(),
                started: old.since,
                ended: now.unix_millis(),
                power_restored: None,
                apps_restored: 0,
                complete: true,
            });
            if let Some(session) = file.session.as_mut() {
                session.game = game;
                session.since = now.unix_millis();
            }
        });
        self.verified.clear();
        self.rejected.clear();
        self.verified.insert(foreground.process);
        if let Some(live) = self.live.as_mut() {
            live.last_foreground = now;
            live.switch = None;
        }
    }

    /// Ce qui se passe à chaque pas d'une partie : l'alimentation, puis les apps
    /// en veilleuse.
    fn maintain(
        &mut self,
        settings: &Settings,
        processes: &[RunningProcess],
        foreground: Option<&Foreground>,
    ) {
        let Some(session) = self.file.session.as_ref() else { return };
        if session.released {
            if session.restore_pending {
                self.retry_pending();
            }
            return;
        }
        self.drop_unmodifiable();
        self.power_step(settings);
        self.quiet_step(settings, processes, foreground);
    }

    /// Retire du journal les processus qu'on n'a pas pu modifier.
    fn drop_unmodifiable(&mut self) {
        let Some(live) = self.live.as_mut() else { return };
        if live.dropped.is_empty() {
            return;
        }
        let dropped = std::mem::take(&mut live.dropped);
        let written = self.change(|file| {
            if let Some(session) = file.session.as_mut() {
                session.quieted.retain(|entry| !dropped.contains(&process_of(entry)));
            }
        });
        if written.is_err()
            && let Some(live) = self.live.as_mut()
        {
            live.dropped = dropped;
        }
    }

    /// Les apps en veilleuse : remet celles qui ne doivent plus l'être (réglage
    /// éteint, app décochée, app que la personne regarde), et, tous les 15 pas
    /// ou dès que la liste ou l'app regardée change, met les nouvelles en veilleuse.
    fn quiet_step(
        &mut self,
        settings: &Settings,
        processes: &[RunningProcess],
        foreground: Option<&Foreground>,
    ) {
        let Some(session) = self.file.session.clone() else { return };
        let names = quiet_names(settings);
        // L'app que la personne regarde n'est jamais ralentie.
        let watched = foreground.map(|f| file_name(&f.exe)).filter(|name| names.contains(name));

        let (staying, back): (Vec<Quieted>, Vec<Quieted>) =
            session.quieted.iter().cloned().partition(|entry| {
                names.contains(&entry.exe) && watched.as_deref() != Some(entry.exe.as_str())
            });
        if !back.is_empty() {
            let (failed, apps) = self.restore_entries(session.since, &back, &staying);
            if let Some(live) = self.live.as_mut() {
                live.apps_restored.extend(apps);
            }
            let mut quieted = staying;
            quieted.extend(failed);
            self.change_anyway(|file| {
                if let Some(session) = file.session.as_mut() {
                    session.quieted = quieted;
                }
            });
        }

        let Some(live) = self.live.as_mut() else { return };
        live.steps_since_scan += 1;
        let changed = live.watched != watched || live.quiet_names.as_ref() != Some(&names);
        let due = live.steps_since_scan >= STEPS_PER_SCAN;
        live.watched.clone_from(&watched);
        live.quiet_names = Some(names.clone());
        if !(changed || due) {
            return;
        }
        live.steps_since_scan = 0;
        if !names.is_empty() {
            self.scan(processes, &names, watched.as_deref());
        }
    }

    /// Relevé des processus : retire du journal ceux qui sont morts, met les
    /// nouveaux en veilleuse. Le journal n'est réécrit que si la liste a
    /// changé, et avant d'agir sur les nouveaux.
    fn scan(&mut self, processes: &[RunningProcess], names: &[String], watched: Option<&str>) {
        let Some(session) = self.file.session.clone() else { return };
        let alive: HashSet<ProcessRef> = processes.iter().map(|p| p.process).collect();
        let mut quieted: Vec<Quieted> =
            session.quieted.iter().filter(|e| alive.contains(&process_of(e))).cloned().collect();
        let removed = quieted.len() != session.quieted.len();
        let unmodifiable =
            self.live.as_ref().map(|live| live.unmodifiable.clone()).unwrap_or_default();
        let mut planned =
            self.plan_quiet(processes, &session.game.path, names, watched, &quieted, &unmodifiable);
        planned.truncate(QUIETED_KEPT.saturating_sub(quieted.len()));
        if !removed && planned.is_empty() {
            return;
        }
        quieted.extend(planned.iter().map(|(process, before, _)| entry_of(process, *before)));
        let written = self.change(|file| {
            if let Some(session) = file.session.as_mut() {
                session.quieted = quieted;
            }
        });
        // Journal non écrit : rien n'est modifié, on réessaiera au prochain relevé.
        if written.is_ok() {
            self.act_on(planned);
        }
    }
}

#[cfg(test)]
mod tests;
