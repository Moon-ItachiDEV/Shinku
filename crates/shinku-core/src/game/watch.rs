//! La machine à états du mode jeu : un pas toutes les deux secondes.
//!
//! Le [`Watcher`] regarde la fenêtre au premier plan. Quand le même jeu reconnu
//! y reste deux pas de suite, il ouvre une partie : il note dans `game.json`
//! tout ce qu'il va changer (avec l'état d'avant), puis le change. À la fin de
//! la partie, il remet chaque chose en place, et note le bilan.
//!
//! Quatre règles guident tout ce qui suit :
//!
//! - **l'intention d'abord** : rien n'est changé sur le PC avant que le journal
//!   ne le dise. Si le journal ne s'écrit pas (disque plein), rien n'est modifié ;
//! - **ne remettre que ce qu'on a posé** : une priorité, un masque d'efficacité
//!   ou un mode d'alimentation que la personne (ou un autre programme) a changé
//!   entre-temps n'est pas rétabli ;
//! - **le doute profite à la machine** : un processus qu'on ne peut pas lire ou
//!   modifier est laissé de côté, un mode d'alimentation inconnu n'est pas touché ;
//! - **le jeu n'est jamais ralenti** : un processus du jeu qui a hérité d'une
//!   priorité basse est remonté, et un processus qui a lancé le jeu n'est pas
//!   mis en veilleuse.
//!
//! Les délais se comptent avec l'horloge monotone de la machine
//! ([`GameHost::uptime_ms`]), jamais avec l'heure murale, qui peut reculer ou
//! sauter. L'heure murale ne sert qu'aux dates écrites dans le journal.
//!
//! **Priorité héritée.** Un processus créé par un processus en veilleuse hérite
//! de sa classe de priorité. La partie garde donc les **racines** de ce qu'elle
//! a ralenti (même celles qui sont mortes depuis) et retrouve leurs
//! descendants à partir d'elles : un processus est un descendant si son parent
//! est une racine, s'il est né après cette racine et après l'ouverture de la
//! partie, et, quand la racine est morte, pas après la date où un relevé l'a vue
//! morte (à un relevé près).

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::Arc;
use std::time::Duration;

use crate::game::apps;
use crate::game::detect::{self, Lists, Source, file_name, key};
use crate::game::host::{
    Foreground, GameHost, PowerPlan, ProcessRef, ProcessState, RunningProcess, priority,
};
use crate::game::store::{
    GameFile, GameRef, GameStore, Last, PowerRecord, QUIETED_KEPT, Quieted, Root, Session,
};
use crate::game::{PowerOutcome, unsuited_processor};
use crate::model::Timestamp;
use crate::settings::Settings;

/// Délai entre deux relevés des processus en veilleuse.
const SCAN_EVERY: Duration = Duration::from_secs(30);
/// Délai entre deux nouvelles tentatives de remise en place.
const RETRY_EVERY: Duration = Duration::from_secs(30);
/// Nombre de tentatives de remise en place avant d'abandonner.
const RESTORE_ATTEMPTS: u32 = 20;
/// Pas consécutifs pour qu'un jeu soit reconnu comme le jeu en cours.
const CONFIRM_STEPS: u32 = 2;
/// Temps hors du premier plan au bout duquel la partie est finie : un lanceur
/// resté ouvert en arrière-plan ne garde pas le PC en mode jeu.
const BACKGROUND_LIMIT: Duration = Duration::from_secs(600);
/// Délai avant de réessayer d'ouvrir une partie quand le journal ne s'est pas écrit.
const WRITE_RETRY: Duration = Duration::from_secs(60);
/// Marge sur la date où un relevé a vu une racine morte : un relevé.
const ROOT_GRACE: Duration = SCAN_EVERY;

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

/// Ce que le watcher retient d'une partie ouverte, en plus du journal. Tous les
/// instants sont ceux de l'horloge monotone de la machine, en millisecondes.
#[derive(Debug)]
struct Live {
    /// Dernier instant où le jeu était au premier plan.
    last_foreground: u64,
    /// Instant du dernier relevé des processus.
    last_scan: u64,
    /// La partie est finie, mais tout n'a pas pu être remis en place.
    ending: bool,
    /// Tentatives de remise en place de la cause d'échec en cours (la première
    /// comprise). Elle repart de zéro quand une remise réussit.
    attempts: u32,
    /// Instant de la dernière tentative.
    last_retry: u64,
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
    /// Pas de nouvelle tentative d'alimentation avant cet instant.
    power_retry_until: u64,
}

impl Live {
    fn new(uptime: u64) -> Self {
        Self {
            last_foreground: uptime,
            last_scan: uptime,
            ending: false,
            attempts: 0,
            last_retry: uptime,
            incomplete: false,
            apps_restored: Vec::new(),
            switch: None,
            dropped: Vec::new(),
            unmodifiable: HashSet::new(),
            watched: None,
            quiet_names: None,
            power_retry_until: 0,
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

/// Un processus à mettre en veilleuse.
#[derive(Debug)]
struct Planned {
    process: RunningProcess,
    /// L'état d'avant, tel qu'il sera noté au journal.
    noted: ProcessState,
    /// L'état lu sur le processus.
    read: ProcessState,
    /// L'état à lui donner.
    target: ProcessState,
}

/// Ce qui décide quels processus sont mis en veilleuse.
struct Wish<'a> {
    /// Le chemin du jeu en cours.
    game: &'a str,
    /// Les noms des apps à mettre en veilleuse.
    names: &'a [String],
    /// L'app que la personne regarde.
    watched: Option<&'a str>,
    /// Ce qui est déjà au journal.
    current: &'a [Quieted],
    /// Ce qu'on n'a pas pu modifier.
    unmodifiable: &'a HashSet<ProcessRef>,
    /// Les descendants des racines de la partie : s'ils sont trouvés en
    /// « sous la normale », c'est qu'ils ont hérité de cette classe.
    descendants: &'a HashSet<ProcessRef>,
    /// Les processus qui ont lancé le jeu.
    ancestors: &'a HashSet<ProcessRef>,
}

/// La machine à états du mode jeu.
#[derive(Debug)]
pub struct Watcher {
    host: Arc<dyn GameHost>,
    store: GameStore,
    /// Le contenu de `game.json`, tel que Shinku le connaît.
    file: GameFile,
    /// Le journal en mémoire est en avance sur le disque : une écriture a échoué.
    dirty: bool,
    live: Option<Live>,
    candidate: Option<Candidate>,
    /// Pas d'ouverture de partie avant cet instant (le journal ne s'écrivait pas).
    write_retry_at: Option<u64>,
    /// Windows a redémarré depuis l'ouverture de la partie reprise : ses
    /// numéros de processus sont ceux d'un autre démarrage, seule l'alimentation
    /// se remet.
    rebooted: bool,
    /// Processus dont on a vérifié que c'est le jeu.
    verified: HashSet<ProcessRef>,
    /// Processus de même nom que le jeu, mais d'un autre chemin.
    rejected: HashSet<ProcessRef>,
    unsuited: Option<bool>,
}

/// Une durée en millisecondes.
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
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

/// Les processus qui ont lancé un processus du jeu, de parent en parent. Un
/// parent doit avoir été créé avant son enfant : un pid repris par un autre
/// processus ne fait pas un parent.
fn ancestors_of(game: &HashSet<ProcessRef>, processes: &[RunningProcess]) -> HashSet<ProcessRef> {
    let mut found = HashSet::new();
    for process in processes.iter().filter(|p| game.contains(&p.process)) {
        let mut current = process;
        // Une chaîne plus longue que la liste est une boucle : on s'arrête.
        for _ in 0..processes.len() {
            let Some(parent) = processes.iter().find(|p| {
                p.process.pid == current.parent && p.process.started < current.process.started
            }) else {
                break;
            };
            if !found.insert(parent.process) {
                break;
            }
            current = parent;
        }
    }
    found
}

impl Watcher {
    /// Une machine à états sur cette machine et ce journal.
    pub fn new(host: Arc<dyn GameHost>, store: GameStore) -> Self {
        let file = store.load();
        Self {
            host,
            store,
            file,
            dirty: false,
            live: None,
            candidate: None,
            write_retry_at: None,
            rebooted: false,
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
    /// si Windows a redémarré depuis, seule l'alimentation est remise (les
    /// numéros de processus du journal sont ceux d'un autre démarrage) ; si le
    /// jeu vit encore, elle reprend telle quelle ; sinon elle se termine
    /// normalement, et tout est remis en place (l'alimentation aussi, si le mode
    /// actif est encore celui que Shinku avait posé). Sans effet si une partie
    /// est déjà suivie, et sans danger si on l'appelle deux fois.
    pub fn recover(&mut self) {
        if self.live.is_some() {
            return;
        }
        self.file = self.store.load();
        self.dirty = false;
        let Some(session) = self.file.session.clone() else { return };
        let uptime = self.host.uptime_ms();
        // Windows a démarré à cet instant (à l'heure murale) : si c'est après
        // l'ouverture de la partie, le PC a redémarré depuis.
        let boot =
            self.host.now().unix_millis().saturating_sub(i64::try_from(uptime).unwrap_or(i64::MAX));
        self.rebooted = boot > session.opened;
        self.live = Some(Live::new(uptime));
        if self.rebooted {
            self.finish();
            return;
        }
        let alive = self.game_alive(
            &session.game.path,
            &self.host.processes(),
            self.host.foreground().as_ref(),
        );
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
        self.flush();
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

    /// La source d'alimentation a changé : seule l'alimentation est réévaluée.
    /// Ce n'est pas un pas : les comptes de pas, de relevés et de tentatives ne
    /// bougent pas (Windows envoie cet événement à chaque variation de la
    /// charge de la batterie).
    pub fn power_changed(&mut self, settings: &Settings) {
        let Some(live) = self.live.as_ref() else { return };
        let released = self.file.session.as_ref().is_none_or(|session| session.released);
        if live.ending || released {
            return;
        }
        self.power_step(settings);
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
        let uptime = self.host.uptime_ms();
        if let Some(live) = self.live.as_mut() {
            live.apps_restored.extend(done.apps.iter().cloned());
            live.ending = false;
            // Une tentative de plus pour la même cause d'échec, ou la fin du compte.
            live.attempts = if pending { live.attempts + 1 } else { 0 };
            live.last_retry = uptime;
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

    /// Retente tout de suite une remise en place restée à moitié, sans attendre
    /// le prochain essai. Pour un arrêt (la désinstallation) qui ne peut pas
    /// attendre.
    pub fn settle(&mut self) {
        if self.live.as_ref().is_some_and(|live| live.ending) {
            self.attempt_restore();
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
        self.dirty = false;
        Ok(())
    }

    /// Applique un changement au journal, même si l'écriture échoue : le geste
    /// qu'il décrit est déjà fait. Une écriture ratée est réessayée à chaque pas
    /// (voir [`Watcher::flush`]) : le journal ne doit pas mentir longtemps.
    fn change_anyway(&mut self, edit: impl FnOnce(&mut GameFile)) {
        edit(&mut self.file);
        self.dirty = self.store.save(&self.file).is_err();
    }

    /// Réécrit le journal s'il est en retard sur la mémoire.
    fn flush(&mut self) {
        if self.dirty && self.store.save(&self.file).is_ok() {
            self.dirty = false;
        }
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
        let waiting = self.write_retry_at.is_some_and(|at| self.host.uptime_ms() < at);
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
        let uptime = self.host.uptime_ms();
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
        let ancestors = ancestors_of(&HashSet::from([foreground.process]), &processes);
        let planned = self.plan_quiet(
            &processes,
            &Wish {
                game: &game.path,
                names: &names,
                watched: None,
                current: &[],
                unmodifiable: &HashSet::new(),
                descendants: &HashSet::new(),
                ancestors: &ancestors,
            },
        );
        let mut session = Session {
            game,
            opened: now.unix_millis(),
            since: now.unix_millis(),
            released: false,
            power,
            quieted: planned.iter().map(|plan| entry_of(&plan.process, plan.noted)).collect(),
            roots: Vec::new(),
            restore_pending: false,
        };
        for plan in &planned {
            session.add_root(plan.process.process.pid, plan.process.process.started);
        }

        // b. Écrire la session complète, d'une seule écriture : l'intention
        // d'abord. Si le journal ne s'écrit pas, rien n'est modifié.
        if self.change(|file| file.session = Some(session)).is_err() {
            self.write_retry_at = Some(uptime.saturating_add(millis(WRITE_RETRY)));
            return;
        }
        self.write_retry_at = None;
        self.candidate = None;
        let mut live = Live::new(uptime);
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
    /// notés pour être retirés du journal au pas suivant, sauf si l'essai les a
    /// quand même changés (un échec à moitié) : ceux-là restent au journal, et
    /// seront remis.
    fn act_on(&mut self, planned: Vec<Planned>) {
        for plan in planned {
            let process = plan.process.process;
            if self.host.apply(process, plan.target).is_ok() {
                continue;
            }
            let changed = self.host.inspect(process).is_some_and(|state| state != plan.read);
            if let Some(live) = self.live.as_mut() {
                live.unmodifiable.insert(process);
                if !changed {
                    live.dropped.push(process);
                }
            }
        }
    }

    /// Les processus à mettre en veilleuse : ceux dont le nom est dans la liste,
    /// qui ne sont ni protégés, ni le jeu, ni un de ses ancêtres, ni l'app que
    /// la personne regarde, ni déjà dans le journal, et qu'on peut lire.
    ///
    /// Un processus trouvé en « sous la normale » qui descend d'une racine de
    /// la partie a hérité de cette classe : son état d'avant est « normale ».
    fn plan_quiet(&mut self, processes: &[RunningProcess], wish: &Wish<'_>) -> Vec<Planned> {
        let mut planned = Vec::new();
        for process in processes {
            if !wish.names.contains(&process.exe_name)
                || apps::protected(&process.exe_name)
                || wish.watched == Some(process.exe_name.as_str())
                || wish.unmodifiable.contains(&process.process)
                || wish.ancestors.contains(&process.process)
                || wish.current.iter().any(|entry| process_of(entry) == process.process)
                || self.is_game_process(process, wish.game)
            {
                continue;
            }
            // Un processus qu'on ne peut pas lire est laissé de côté.
            let Some(read) = self.host.inspect(process.process) else { continue };
            if !priority::is_known(read.priority) {
                continue;
            }
            let mut noted = read;
            if read.priority == priority::BELOW_NORMAL
                && wish.descendants.contains(&process.process)
            {
                noted.priority = priority::NORMAL;
            }
            if let Some(target) = quiet_target(noted) {
                planned.push(Planned { process: process.clone(), noted, read, target });
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
                let uptime = self.host.uptime_ms();
                if self.live.as_ref().is_some_and(|live| live.power_retry_until > uptime) {
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
                        live.power_retry_until = uptime.saturating_add(millis(RETRY_EVERY));
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

    /// Les descendants des racines de la partie dans cette liste de processus :
    /// ceux qui ont pu hériter d'une priorité basse.
    ///
    /// Un processus est un descendant si son parent est une racine ou un
    /// descendant, s'il est né après ce parent, et s'il est né après
    /// l'ouverture de la partie. Si la racine est morte, il ne doit pas être né
    /// après la date où un relevé l'a vue morte (à un relevé près), ni après
    /// qu'un autre processus a repris son numéro : un pid repris n'est pas un
    /// parent.
    fn descendants(
        &self,
        opened: i64,
        roots: &[Root],
        processes: &[RunningProcess],
    ) -> HashSet<ProcessRef> {
        let mut found = HashSet::new();
        if roots.is_empty() || self.rebooted {
            return found;
        }
        let opened = from_millis(opened);
        let grace = i64::try_from(ROOT_GRACE.as_millis()).unwrap_or(i64::MAX);
        let born = |started: i64| Timestamp::from_filetime(started);
        let mut children: HashMap<u32, Vec<&RunningProcess>> = HashMap::new();
        for process in processes {
            children.entry(process.parent).or_default().push(process);
        }
        // Chaque entrée : le numéro du parent, sa date de création, et la
        // dernière date de naissance qu'un enfant peut avoir.
        let mut queue: Vec<(u32, i64, Option<Timestamp>)> = Vec::new();
        for root in roots {
            let alive = processes
                .iter()
                .any(|p| p.process.pid == root.pid && p.process.started == root.started);
            let mut limit = None;
            if !alive {
                limit = root.gone.map(|gone| from_millis(gone.saturating_add(grace)));
                let taken = processes
                    .iter()
                    .find(|p| p.process.pid == root.pid && p.process.started != root.started);
                if let Some(taken) = taken {
                    let at = born(taken.process.started);
                    limit = Some(limit.map_or(at, |known| known.min(at)));
                }
            }
            queue.push((root.pid, root.started, limit));
        }
        let mut index = 0;
        while let Some(&(parent, parent_started, limit)) = queue.get(index) {
            index += 1;
            for child in children.get(&parent).map_or(&[][..], Vec::as_slice) {
                let at = born(child.process.started);
                if child.process.started <= parent_started
                    || at <= opened
                    || limit.is_some_and(|limit| at > limit)
                    || !found.insert(child.process)
                {
                    continue;
                }
                queue.push((child.process.pid, child.process.started, None));
            }
        }
        found
    }

    /// Remet un ensemble de processus : chacun à son état d'avant, sous les
    /// conditions de [`restoring_state`], puis les descendants qui ont hérité de
    /// la priorité basse. Rend ceux qu'on n'a pas pu remettre et le nom des apps
    /// remises (une entrée par processus).
    ///
    /// `full` : c'est la fin de la partie (ou son relâchement), les descendants
    /// se cherchent à partir de toutes ses racines. Sinon, seules ces entrées
    /// retrouvent leurs descendants (`staying` reste en veilleuse).
    ///
    /// Un processus qui n'existe plus, ou qu'on ne peut plus lire, n'a rien à
    /// recevoir : il n'est pas rendu. Après un redémarrage de Windows, rien
    /// n'est remis : ces numéros de processus sont ceux d'un autre démarrage.
    fn restore_entries(
        &self,
        session: &Session,
        entries: &[Quieted],
        staying: &[Quieted],
        full: bool,
    ) -> (Vec<Quieted>, Vec<String>) {
        let mut failed = Vec::new();
        let mut apps = Vec::new();
        if self.rebooted {
            return (failed, apps);
        }
        for entry in entries {
            let process = process_of(entry);
            let Some(current) = self.host.inspect(process) else { continue };
            let Some(target) = restoring_state(entry, current) else { continue };
            match self.host.apply(process, target) {
                Ok(()) => apps.push(entry.exe.clone()),
                Err(_) => failed.push(entry.clone()),
            }
        }
        let roots = if full {
            session.all_roots()
        } else {
            entries
                .iter()
                .map(|entry| Root { pid: entry.pid, started: entry.started, gone: None })
                .collect()
        };
        self.restore_descendants(session.opened, &roots, entries, staying);
        (failed, apps)
    }

    /// Un processus créé par un processus en « sous la normale » hérite de cette
    /// classe. Tout descendant d'une racine (voir [`Watcher::descendants`]) trouvé
    /// en « sous la normale » est remis en « normale », quel que soit son nom.
    /// Les processus du journal ne sont pas touchés ici : ils ont leur état d'avant.
    fn restore_descendants(
        &self,
        opened: i64,
        roots: &[Root],
        entries: &[Quieted],
        staying: &[Quieted],
    ) {
        if roots.is_empty() {
            return;
        }
        let processes = self.host.processes();
        let descendants = self.descendants(opened, roots, &processes);
        let skip: HashSet<ProcessRef> = roots
            .iter()
            .map(|root| ProcessRef { pid: root.pid, started: root.started })
            .chain(entries.iter().chain(staying).map(process_of))
            .collect();
        for process in processes.iter().filter(|p| descendants.contains(&p.process)) {
            if skip.contains(&process.process) {
                continue;
            }
            if let Some(state) = self.host.inspect(process.process)
                && state.priority == priority::BELOW_NORMAL
            {
                // Un échec n'est pas retenté : ce processus n'est pas au journal.
                let _ = self
                    .host
                    .apply(process.process, ProcessState { priority: priority::NORMAL, ..state });
            }
        }
    }

    /// Remet tout ce que cette partie a changé.
    fn restore_session(&self, session: &Session) -> Restoration {
        let (power, power_restored) = self.restore_power(&session.power);
        let (left, apps) = self.restore_entries(session, &session.quieted, &[], true);
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
        self.rebooted = false;
        self.verified.clear();
        self.rejected.clear();
    }

    /// Quelque chose n'a pas pu être remis : la partie reste, avec ce qui reste
    /// à remettre. C'est une tentative de plus pour la cause d'échec en cours.
    fn mark_pending(&mut self, done: Restoration, ending: bool) {
        let uptime = self.host.uptime_ms();
        if let Some(live) = self.live.as_mut() {
            live.ending = ending;
            live.attempts += 1;
            live.last_retry = uptime;
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

    /// Retente une remise en place restée à moitié, tous les 30 secondes, jusqu'à
    /// 20 essais : au-delà, on abandonne, et le bilan dit que tout n'est pas revenu.
    fn retry_pending(&mut self) {
        let uptime = self.host.uptime_ms();
        let Some(live) = self.live.as_ref() else { return };
        if uptime.saturating_sub(live.last_retry) < millis(RETRY_EVERY) {
            return;
        }
        self.attempt_restore();
    }

    /// Une tentative de remise en place d'une partie à moitié remise.
    fn attempt_restore(&mut self) {
        let uptime = self.host.uptime_ms();
        let Some(live) = self.live.as_mut() else { return };
        live.last_retry = uptime;
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
            if done.complete {
                live.attempts = 0;
            }
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
        let uptime = self.host.uptime_ms();
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
                live.last_foreground = uptime;
                live.switch = None;
            }
        } else {
            // Fin : le jeu n'est pas au premier plan depuis trop longtemps.
            let away = self.live.as_ref().map(|live| uptime.saturating_sub(live.last_foreground));
            if away.is_some_and(|away| away >= millis(BACKGROUND_LIMIT)) {
                self.finish();
                return;
            }
            if detect && let Some((next, source)) = self.watch_switch(foreground.as_ref(), &lists) {
                self.switch_to(&next, source, &processes);
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
    /// et son début changent (l'ouverture de la partie, elle, ne bouge pas :
    /// c'est elle qui borne la remise en place). L'ancien jeu passe dans le
    /// bilan et les jeux vus. Le nouveau jeu n'est jamais ralenti.
    ///
    /// Une partie relâchée n'a rien à transmettre : elle se termine, et la
    /// détection en ouvrira une neuve pour ce jeu.
    fn switch_to(&mut self, foreground: &Foreground, source: Source, processes: &[RunningProcess]) {
        let Some(old) = self.file.session.clone() else { return };
        if old.released {
            self.finish();
            return;
        }
        let uptime = self.host.uptime_ms();
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
            live.last_foreground = uptime;
            live.switch = None;
        }
        // Le nouveau jeu a pu naître d'une app en veilleuse : il hérite de sa
        // priorité basse, ou il est lui-même au journal.
        let Some(session) = self.file.session.clone() else { return };
        let descendants = self.descendants(session.opened, &session.all_roots(), processes);
        let game = self.guard_game(&session, processes, &descendants);
        self.put_back_game(&session, &game);
    }

    /// Ce qui se passe à chaque pas d'une partie : l'alimentation, le jeu, puis
    /// les apps en veilleuse.
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
        let Some(session) = self.file.session.clone() else { return };
        let descendants = self.descendants(session.opened, &session.all_roots(), processes);
        let game = self.guard_game(&session, processes, &descendants);
        self.quiet_step(settings, processes, foreground, &descendants, &game);
    }

    /// Le jeu n'est jamais ralenti : tout processus du jeu trouvé en « sous la
    /// normale », né après l'ouverture de la partie et descendant d'une racine,
    /// a hérité de cette classe, et il est remonté en « normale ». Rend les
    /// processus du jeu.
    fn guard_game(
        &mut self,
        session: &Session,
        processes: &[RunningProcess],
        descendants: &HashSet<ProcessRef>,
    ) -> HashSet<ProcessRef> {
        let name = file_name(&session.game.path);
        let mut game = HashSet::new();
        for process in processes {
            // Un processus du jeu qu'on n'a pas encore vu au premier plan n'est
            // ouvert que s'il descend d'une racine : c'est le seul qui puisse
            // être ralenti par héritage.
            let known = self.verified.contains(&process.process)
                || (process.exe_name == name
                    && descendants.contains(&process.process)
                    && self.is_game_process(process, &session.game.path));
            if known {
                game.insert(process.process);
            }
        }
        for process in processes
            .iter()
            .filter(|p| game.contains(&p.process) && descendants.contains(&p.process))
        {
            if self.live.as_ref().is_some_and(|live| live.unmodifiable.contains(&process.process)) {
                continue;
            }
            let Some(state) = self.host.inspect(process.process) else { continue };
            if state.priority == priority::BELOW_NORMAL
                && self
                    .host
                    .apply(process.process, ProcessState { priority: priority::NORMAL, ..state })
                    .is_err()
                && let Some(live) = self.live.as_mut()
            {
                live.unmodifiable.insert(process.process);
            }
        }
        game
    }

    /// Remet à leur état d'avant les entrées du journal qui sont devenues le
    /// jeu (la personne avait mis en veilleuse un exécutable qu'elle joue
    /// maintenant) : le jeu n'est jamais ralenti.
    fn put_back_game(&mut self, session: &Session, game: &HashSet<ProcessRef>) {
        let (back, staying): (Vec<Quieted>, Vec<Quieted>) =
            session.quieted.iter().cloned().partition(|entry| game.contains(&process_of(entry)));
        if !back.is_empty() {
            self.put_back(session, back, staying);
        }
    }

    /// Remet ces entrées à leur état d'avant et les retire du journal. Celles
    /// qu'on n'a pas pu remettre y restent.
    fn put_back(&mut self, session: &Session, back: Vec<Quieted>, staying: Vec<Quieted>) {
        let (failed, apps) = self.restore_entries(session, &back, &staying, false);
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
    /// éteint, app décochée, app que la personne regarde, app devenue le jeu),
    /// et, tous les 30 secondes ou dès que la liste ou l'app regardée change, met
    /// les nouvelles en veilleuse.
    fn quiet_step(
        &mut self,
        settings: &Settings,
        processes: &[RunningProcess],
        foreground: Option<&Foreground>,
        descendants: &HashSet<ProcessRef>,
        game: &HashSet<ProcessRef>,
    ) {
        let Some(session) = self.file.session.clone() else { return };
        let names = quiet_names(settings);
        // L'app que la personne regarde n'est jamais ralentie.
        let watched = foreground.map(|f| file_name(&f.exe)).filter(|name| names.contains(name));

        let (staying, back) = split_quieted(&session, &names, watched.as_deref(), game);
        if !back.is_empty() {
            self.put_back(&session, back, staying);
        }

        let uptime = self.host.uptime_ms();
        let Some(live) = self.live.as_mut() else { return };
        let changed = live.watched != watched || live.quiet_names.as_ref() != Some(&names);
        let due = uptime.saturating_sub(live.last_scan) >= millis(SCAN_EVERY);
        live.watched.clone_from(&watched);
        live.quiet_names = Some(names.clone());
        if !(changed || due) {
            return;
        }
        live.last_scan = uptime;
        if !names.is_empty() {
            self.scan(processes, &names, watched.as_deref(), descendants, game);
        }
    }

    /// Relevé des processus : retire du journal ceux qui sont morts (en notant
    /// la date de leur mort sur leur racine), met les nouveaux en veilleuse. Le
    /// journal n'est réécrit que si quelque chose a changé, et avant d'agir sur
    /// les nouveaux.
    fn scan(
        &mut self,
        processes: &[RunningProcess],
        names: &[String],
        watched: Option<&str>,
        descendants: &HashSet<ProcessRef>,
        game: &HashSet<ProcessRef>,
    ) {
        let Some(session) = self.file.session.clone() else { return };
        let now = self.host.now().unix_millis();
        let alive: HashSet<ProcessRef> = processes.iter().map(|p| p.process).collect();
        let mut next = session.clone();
        // Les processus en veilleuse sont toujours des racines (un journal
        // d'avant les racines n'en note pas).
        for entry in &session.quieted {
            next.add_root(entry.pid, entry.started);
        }
        for root in &mut next.roots {
            if root.gone.is_none()
                && !alive.contains(&ProcessRef { pid: root.pid, started: root.started })
            {
                root.gone = Some(now);
            }
        }
        let mut quieted: Vec<Quieted> =
            session.quieted.iter().filter(|e| alive.contains(&process_of(e))).cloned().collect();
        let removed = quieted.len() != session.quieted.len();
        let unmodifiable =
            self.live.as_ref().map(|live| live.unmodifiable.clone()).unwrap_or_default();
        let ancestors = ancestors_of(game, processes);
        let mut planned = self.plan_quiet(
            processes,
            &Wish {
                game: &session.game.path,
                names,
                watched,
                current: &quieted,
                unmodifiable: &unmodifiable,
                descendants,
                ancestors: &ancestors,
            },
        );
        planned.truncate(QUIETED_KEPT.saturating_sub(quieted.len()));
        if !removed && planned.is_empty() && next.roots == session.roots {
            return;
        }
        for plan in &planned {
            next.add_root(plan.process.process.pid, plan.process.process.started);
        }
        quieted.extend(planned.iter().map(|plan| entry_of(&plan.process, plan.noted)));
        next.quieted = quieted;
        let written = self.change(|file| file.session = Some(next));
        // Journal non écrit : rien n'est modifié, on réessaiera au prochain relevé.
        if written.is_ok() {
            self.act_on(planned);
        }
    }
}

/// Sépare les entrées du journal : celles qui restent en veilleuse, et celles
/// qui doivent retrouver leur état d'avant (app décochée, app regardée, ou app
/// devenue le jeu).
fn split_quieted(
    session: &Session,
    names: &[String],
    watched: Option<&str>,
    game: &HashSet<ProcessRef>,
) -> (Vec<Quieted>, Vec<Quieted>) {
    session.quieted.iter().cloned().partition(|entry| {
        names.contains(&entry.exe)
            && watched != Some(entry.exe.as_str())
            && !game.contains(&process_of(entry))
    })
}

#[cfg(test)]
mod tests;
