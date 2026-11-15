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
//! **Dans le doute, on remet.** Relever par erreur un processus est un défaut
//! bénin ; laisser une app en priorité basse est un défaut grave.
//!
//! Les délais se comptent avec l'horloge monotone de la machine
//! ([`GameHost::uptime_ms`]). L'heure murale ne sert qu'aux dates écrites dans
//! le journal : aucune décision ne s'appuie sur elle, et aucune ne compare deux
//! dates.
//!
//! **Priorité héritée.** Sous Windows, un processus créé par un processus en
//! veilleuse hérite de sa classe de priorité au moment de sa création, et
//! seulement à ce moment. L'invariant : les seuls processus que Shinku laisse
//! en priorité basse sont ceux du journal (des apps listées et cochées, chacune
//! avec son identité exacte et son état d'avant). Tout autre processus trouvé
//! « sous la normale » par héritage est **relevé dès qu'il est vu**. La
//! filiation n'est pas déduite après coup : elle est constatée, à chaque pas.
//!
//! - un **émetteur** est une entrée dont Shinku a baissé la classe ;
//! - la **famille** (en mémoire seulement) est faite des processus hors journal
//!   vus naître, depuis le relevé précédent, d'un émetteur ou d'un membre ;
//! - un membre trouvé exactement « sous la normale » est relevé, une seule
//!   fois ; s'il est lui-même listé et coché, il entre au journal avec « normale »
//!   pour état d'avant ;
//! - la remise en place est une seule fonction, qui rend leur état aux entrées
//!   par identité, puis relève ce qui est né pendant ce temps.

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::Arc;
use std::time::Duration;

use crate::game::apps;
use crate::game::detect::{self, Lists, Source, file_name, key};
use crate::game::host::{
    Foreground, GameHost, Liveness, PowerPlan, ProcessRef, ProcessState, RunningProcess, priority,
};
use crate::game::store::{
    GameFile, GameRef, GameStore, Last, PowerRecord, QUIETED_KEPT, Quieted, Session,
};
use crate::game::{PowerOutcome, unsuited_processor};
use crate::settings::Settings;

/// Délai entre deux relevés des processus en veilleuse.
const SCAN_EVERY: Duration = Duration::from_secs(30);
/// Délai entre deux nouvelles tentatives de remise en place.
pub(crate) const RETRY_EVERY: Duration = Duration::from_secs(30);
/// Nombre de tentatives de remise en place avant d'abandonner.
const RESTORE_ATTEMPTS: u32 = 20;
/// Pas consécutifs pour qu'un jeu soit reconnu comme le jeu en cours.
const CONFIRM_STEPS: u32 = 2;
/// Temps hors du premier plan au bout duquel la partie est finie : un lanceur
/// resté ouvert en arrière-plan ne garde pas le PC en mode jeu.
const BACKGROUND_LIMIT: Duration = Duration::from_secs(600);
/// Délai avant de réessayer d'ouvrir une partie quand le journal ne s'est pas écrit.
const WRITE_RETRY: Duration = Duration::from_secs(60);
/// Taille maximale de la famille suivie en mémoire : au-delà, on n'ajoute plus
/// (limite écrite dans la documentation).
const FAMILY_KEPT: usize = 2048;
/// Lectures d'un membre dont la priorité ne se lit pas, avant d'y renoncer.
const EVAL_TRIES: u8 = 3;
/// Tours de remise au plus, quand la passe de famille trouve encore à relever.
const RESTORE_ROUNDS: usize = 3;

/// Le mode efficacité « par défaut » : Windows le gère, rien n'est imposé.
const DEFAULT_THROTTLE: ProcessState =
    ProcessState { priority: priority::NORMAL, throttle_control: 0, throttle_state: 0 };

/// Ce que le relevé des processus permet de conclure sur le jeu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    /// Relevé vide, ou sans le jeu alors que son processus n'est pas prouvé mort.
    Blind,
    /// Le jeu est dans le relevé.
    Playing,
    /// Le jeu est mort.
    Over,
}

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

/// Où en est une remise (ou un relèvement) qui a échoué pour un processus : on
/// ne le retente qu'à [`RETRY_EVERY`], et au plus [`RESTORE_ATTEMPTS`] fois ;
/// au-delà, il reste au journal pour la remise de fin de partie.
#[derive(Clone, Copy, Debug)]
struct Retry {
    /// Pas de nouvel essai avant cet instant de l'horloge monotone.
    next: u64,
    /// Essais ratés.
    tries: u32,
}

/// Vrai si ce processus peut être retenté à cet instant.
fn may_try(failures: &HashMap<ProcessRef, Retry>, process: ProcessRef, uptime: u64) -> bool {
    failures
        .get(&process)
        .is_none_or(|retry| retry.tries < RESTORE_ATTEMPTS && uptime >= retry.next)
}

/// Note un échec de plus pour ce processus.
fn note_failure(failures: &mut HashMap<ProcessRef, Retry>, process: ProcessRef, uptime: u64) {
    let retry = failures.entry(process).or_insert(Retry { next: 0, tries: 0 });
    retry.tries += 1;
    retry.next = uptime.saturating_add(millis(RETRY_EVERY));
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
    /// Les remises ratées de processus du journal (app décochée, regardée,
    /// devenue le jeu).
    back_failed: HashMap<ProcessRef, Retry>,
    /// Les relèvements ratés d'un processus du jeu (ou d'un de ses ancêtres).
    guard_failed: HashMap<ProcessRef, Retry>,
    /// Les identités du dernier relevé lisible. Absent juste après une reprise :
    /// on ne sait alors pas ce qui est nouveau.
    previous: Option<HashSet<ProcessRef>>,
    /// La famille des émetteurs : processus hors journal vus naître d'eux.
    family: HashSet<ProcessRef>,
    /// Membres dont la priorité ne s'est pas lue : lectures déjà faites.
    eval_retry: HashMap<ProcessRef, u8>,
    /// Héritiers listés et cochés, vus au premier relevé : leur état d'avant est
    /// « normale ». Consommés par le prochain relevé des apps listées.
    heirs: HashSet<ProcessRef>,
    /// Un héritier listé attend : le relevé des apps listées n'attend pas son tour.
    force_scan: bool,
    /// Les processus du jeu connus, même ceux qui ne sont plus dans le relevé.
    game_ids: HashSet<ProcessRef>,
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
            back_failed: HashMap::new(),
            guard_failed: HashMap::new(),
            previous: None,
            family: HashSet::new(),
            eval_retry: HashMap::new(),
            heirs: HashSet::new(),
            force_scan: false,
            game_ids: HashSet::new(),
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
    /// Les héritiers vus au premier relevé : trouvés « sous la normale », leur
    /// état d'avant est « normale ».
    heirs: &'a HashSet<ProcessRef>,
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

/// Une entrée dont Shinku a baissé la classe de priorité : ses enfants en
/// héritent. Une entrée déjà basse avant Shinku n'en est pas une.
fn is_emitter(entry: &Quieted) -> bool {
    priority::lowers(entry.priority, priority::BELOW_NORMAL)
}

/// Les processus qui ont lancé un processus du jeu, de parent en parent, en
/// suivant les numéros de parent (sans comparer de dates).
fn ancestors_of(game: &HashSet<ProcessRef>, processes: &[RunningProcess]) -> HashSet<ProcessRef> {
    let by_pid: HashMap<u32, &RunningProcess> =
        processes.iter().map(|p| (p.process.pid, p)).collect();
    let mut found = HashSet::new();
    for process in processes.iter().filter(|p| game.contains(&p.process)) {
        let mut current = process;
        // Une chaîne plus longue que la liste est une boucle : on s'arrête.
        for _ in 0..processes.len() {
            let Some(parent) = by_pid.get(&current.parent) else { break };
            if parent.process == current.process || !found.insert(parent.process) {
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
    /// si le jeu vit encore (ou si le relevé des processus est illisible), elle
    /// reprend telle quelle ; sinon elle se termine normalement, et tout est
    /// remis en place par identité (l'alimentation aussi, si le mode actif est
    /// encore celui que Shinku avait posé). Après un redémarrage du PC, aucune
    /// identité ne correspond : rien n'est touché, sans avoir à le détecter.
    /// Sans effet si une partie est déjà suivie, et sans danger si on l'appelle
    /// deux fois.
    pub fn recover(&mut self) {
        if self.live.is_some() {
            return;
        }
        self.file = self.store.load();
        self.dirty = false;
        let Some(session) = self.file.session.clone() else { return };
        let uptime = self.host.uptime_ms();
        self.live = Some(Live::new(uptime));
        let processes = self.host.processes();
        let view = self.view_of(&session.game.path, &processes, self.host.foreground().as_ref());
        // Une remise en place à moitié faite d'une partie finie, elle, reprend.
        if view != View::Over && (!session.restore_pending || session.released) {
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
        let done = self.restore_all(&session);
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
    ///
    /// Le journal n'est écrit que si son contenu change : un geste qui ne change
    /// rien (une remise qui échoue pas après pas) ne réécrit pas le fichier.
    fn change_anyway(&mut self, edit: impl FnOnce(&mut GameFile)) {
        let before = self.file.clone();
        edit(&mut self.file);
        if self.file != before {
            self.dirty = self.store.save(&self.file).is_err();
        }
    }

    /// Le journal en mémoire est en avance sur le disque.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Réécrit le journal s'il est en retard sur la mémoire.
    pub fn flush(&mut self) {
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
                heirs: &HashSet::new(),
                ancestors: &ancestors,
            },
        );
        let session = Session {
            game,
            since: now.unix_millis(),
            released: false,
            power,
            quieted: planned.iter().map(|plan| entry_of(&plan.process, plan.noted)).collect(),
            restore_pending: false,
        };

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
        live.previous = Some(processes.iter().map(|p| p.process).collect());
        live.game_ids = self.verified.clone();
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

    /// Met ces processus en veilleuse. Un processus dont le geste échoue ne
    /// quitte le journal (au pas suivant) que si son état lu est égal à l'état
    /// d'avant noté. Sinon (un héritier déjà bas dont seul le mode efficacité a
    /// échoué, un échec à moitié) il reste : il sera relevé à la remise.
    fn act_on(&mut self, planned: Vec<Planned>) {
        for plan in planned {
            let process = plan.process.process;
            if self.host.apply(process, plan.target).is_ok() {
                continue;
            }
            let untouched = self.host.inspect(process).is_some_and(|state| state == plan.noted);
            if let Some(live) = self.live.as_mut() {
                live.unmodifiable.insert(process);
                if untouched {
                    live.dropped.push(process);
                }
            }
        }
    }

    /// Les processus à mettre en veilleuse : ceux dont le nom est dans la liste,
    /// qui ne sont ni protégés, ni le jeu, ni un de ses ancêtres, ni l'app que
    /// la personne regarde, ni déjà dans le journal, et qu'on peut lire.
    ///
    /// Un héritier (vu naître d'un émetteur, ou orphelin nouveau) trouvé en
    /// « sous la normale » a hérité de cette classe : son état d'avant est
    /// « normale », mode efficacité par défaut.
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
            if read.priority == priority::BELOW_NORMAL && wish.heirs.contains(&process.process) {
                noted = DEFAULT_THROTTLE;
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

    /// Rend leur état d'avant à ces entrées, par identité exacte (pid et date de
    /// création). Une entrée dont le processus n'existe plus (`Dead`) sort ; une
    /// entrée qu'on ne peut ni lire ni modifier reste, et sera retentée.
    fn give_back(&self, entries: &[Quieted]) -> GiveBack {
        let mut given = GiveBack::default();
        for entry in entries {
            let process = process_of(entry);
            match self.host.inspect(process) {
                None if self.host.liveness(process) == Liveness::Dead => {
                    if is_emitter(entry) {
                        given.dead_emitters.push(entry.pid);
                    }
                }
                None => given.failed.push(entry.clone()),
                Some(current) => {
                    let Some(target) = restoring_state(entry, current) else { continue };
                    match self.host.apply(process, target) {
                        Ok(()) => given.apps.push(entry.exe.clone()),
                        Err(_) => given.failed.push(entry.clone()),
                    }
                }
            }
        }
        given
    }

    /// La remise en place, une seule pour la fin de partie, « Tout remettre »,
    /// chaque nouvel essai, la reprise après plantage et les ordres `--quitter`
    /// et `--retirer`. Par tours, trois au plus : (a) chaque entrée du journal
    /// retrouve son état d'avant ; (b) un relevé frais, puis une passe de famille
    /// qui relève tout membre trouvé « sous la normale », listé ou non ; (c) si
    /// elle a relevé quelque chose, un tour de plus. Le journal n'est écrit
    /// qu'après (par l'appelant).
    ///
    /// Avec mémoire (l'agent a suivi la partie), seuls les nouveaux processus
    /// sont candidats et les émetteurs morts comptent. Sans mémoire (reprise après
    /// plantage), tout le relevé est candidat, mais seuls les émetteurs trouvés
    /// vivants sont des points de départ : après un redémarrage du PC aucune
    /// identité ne correspond, et rien n'est touché.
    fn restore_all(&mut self, session: &Session) -> Restoration {
        let (power, power_restored) = self.restore_power(&session.power);
        let memory = self.live.as_ref().is_some_and(|live| live.previous.is_some());
        let mut left = session.quieted.clone();
        let mut apps = Vec::new();
        let first: HashSet<ProcessRef> =
            self.host.processes().iter().map(|process| process.process).collect();
        let mut seeds: HashSet<u32> = left
            .iter()
            .filter(|entry| is_emitter(entry) && first.contains(&process_of(entry)))
            .map(|entry| entry.pid)
            .collect();
        for _ in 0..RESTORE_ROUNDS {
            let given = self.give_back(&left);
            apps.extend(given.apps);
            if memory {
                seeds.extend(given.dead_emitters);
            }
            left = given.failed;
            let fresh = self.host.processes();
            let excluded: HashSet<ProcessRef> = left.iter().map(process_of).collect();
            let pass = Pass { names: &[], watched: None, excluded: &excluded, remise: true };
            if self.family_pass(&fresh, &seeds, &pass) == 0 {
                break;
            }
        }
        let complete = power_restored != Some(false) && left.is_empty();
        Restoration { power, power_restored, left, apps, complete }
    }

    /// La passe de famille. `seeds` : les numéros de processus dont les enfants
    /// ont pu hériter (émetteurs vivants, émetteurs et membres morts à ce pas).
    /// Tout processus nouveau, hors journal, ni jeu ni ancêtre du jeu, dont le
    /// parent est un de ces numéros (ou un membre), entre dans la famille quelle
    /// que soit sa priorité, jusqu'au point fixe. Chaque membre est évalué une
    /// seule fois, à son entrée : trouvé exactement « sous la normale », il est
    /// relevé, sauf s'il est listé et coché (il passe alors par le journal, avec
    /// « normale » pour état d'avant). Rend le nombre de processus relevés.
    fn family_pass(
        &mut self,
        processes: &[RunningProcess],
        seeds: &HashSet<u32>,
        pass: &Pass<'_>,
    ) -> usize {
        let Some(live) = self.live.as_mut() else { return 0 };
        if seeds.is_empty() && live.family.is_empty() {
            return 0;
        }
        let mut family = std::mem::take(&mut live.family);
        let mut retry = std::mem::take(&mut live.eval_retry);
        let previous = live.previous.take();
        let present: HashSet<ProcessRef> = processes.iter().map(|p| p.process).collect();
        // Un membre absent du relevé est mort : ses enfants ont pu en hériter.
        let mut parents = seeds.clone();
        family.retain(|member| {
            let here = present.contains(member);
            if !here {
                parents.insert(member.pid);
            }
            here
        });
        retry.retain(|member, _| present.contains(member));
        parents.extend(family.iter().map(|member| member.pid));
        let new = |id: &ProcessRef| previous.as_ref().is_none_or(|before| !before.contains(id));
        let mut to_read: Vec<&RunningProcess> =
            processes.iter().filter(|p| retry.contains_key(&p.process)).collect();
        loop {
            let mut grew = false;
            for process in processes {
                if family.len() >= FAMILY_KEPT {
                    break;
                }
                if parents.contains(&process.parent)
                    && new(&process.process)
                    && !pass.excluded.contains(&process.process)
                    && family.insert(process.process)
                {
                    parents.insert(process.process.pid);
                    to_read.push(process);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        let room = self.file.session.as_ref().is_some_and(|s| s.quieted.len() < QUIETED_KEPT);
        let (mut relieved, mut heirs) = (0, Vec::new());
        for process in to_read {
            let Some(state) = self.host.inspect(process.process) else {
                let tries = retry.entry(process.process).or_insert(0);
                *tries += 1;
                if *tries >= EVAL_TRIES {
                    retry.remove(&process.process);
                }
                continue;
            };
            retry.remove(&process.process);
            if state.priority != priority::BELOW_NORMAL {
                continue;
            }
            let listed = !pass.remise
                && room
                && pass.names.contains(&process.exe_name)
                && !apps::protected(&process.exe_name)
                && pass.watched != Some(process.exe_name.as_str());
            if listed {
                heirs.push(process.process);
            } else if self.host.apply(process.process, DEFAULT_THROTTLE).is_ok() {
                relieved += 1;
            }
        }
        if let Some(live) = self.live.as_mut() {
            live.family = family;
            live.eval_retry = retry;
            live.previous = previous;
            if !heirs.is_empty() {
                live.heirs.extend(heirs);
                live.force_scan = true;
            }
        }
        relieved
    }

    /// Un processus nouveau, listé et coché, trouvé « sous la normale » et dont le
    /// parent n'est dans aucun processus du relevé (l'intermédiaire a vécu moins
    /// d'un pas : `Update.exe` qui relance une app) est lui aussi un héritier.
    fn note_orphans(&mut self, processes: &[RunningProcess], pass: &Pass<'_>) {
        if pass.names.is_empty() {
            return;
        }
        let Some(previous) = self.live.as_ref().and_then(|live| live.previous.as_ref()) else {
            return;
        };
        let pids: HashSet<u32> = processes.iter().map(|p| p.process.pid).collect();
        let found: Vec<ProcessRef> = processes
            .iter()
            .filter(|p| {
                !previous.contains(&p.process)
                    && !pids.contains(&p.parent)
                    && pass.names.contains(&p.exe_name)
                    && !apps::protected(&p.exe_name)
                    && pass.watched != Some(p.exe_name.as_str())
                    && !pass.excluded.contains(&p.process)
            })
            .filter(|p| {
                self.host.inspect(p.process).is_some_and(|s| s.priority == priority::BELOW_NORMAL)
            })
            .map(|p| p.process)
            .collect();
        if let Some(live) = self.live.as_mut()
            && !found.is_empty()
        {
            live.heirs.extend(found);
            live.force_scan = true;
        }
    }

    /// Ce que le relevé permet de conclure sur le jeu : `Blind` (relevé vide, ou
    /// sans le jeu alors que son processus n'est pas prouvé mort : on ne conclut
    /// rien), `Playing`, ou `Over` (le jeu est mort).
    fn view_of(
        &mut self,
        game: &str,
        processes: &[RunningProcess],
        foreground: Option<&Foreground>,
    ) -> View {
        if !processes.is_empty() && self.game_alive(game, processes, foreground) {
            let seen = self.verified.clone();
            if let Some(live) = self.live.as_mut() {
                live.game_ids.extend(seen);
            }
            return View::Playing;
        }
        let known: Vec<ProcessRef> = self
            .live
            .as_ref()
            .map(|live| live.game_ids.iter().copied().collect())
            .unwrap_or_default();
        let all_dead = known.iter().all(|id| self.host.liveness(*id) == Liveness::Dead);
        if all_dead && !(known.is_empty() && processes.is_empty()) {
            View::Over
        } else {
            View::Blind
        }
    }

    /// Les processus du jeu déjà vérifiés qui sont dans ce relevé : on ne rouvre
    /// pas les autres.
    fn game_set(&self, processes: &[RunningProcess]) -> HashSet<ProcessRef> {
        processes.iter().map(|p| p.process).filter(|id| self.verified.contains(id)).collect()
    }

    /// Les morts et la famille, à chaque pas tant que le journal n'est pas vide :
    /// une entrée absente du relevé n'en sort que si `liveness` dit `Dead`, puis
    /// la passe de famille relève les héritiers.
    fn track(
        &mut self,
        processes: &[RunningProcess],
        names: &[String],
        watched: Option<&str>,
        game: &HashSet<ProcessRef>,
        ancestors: &HashSet<ProcessRef>,
    ) {
        let Some(session) = self.file.session.as_ref() else { return };
        let entries = session.quieted.clone();
        let present: HashSet<ProcessRef> = processes.iter().map(|p| p.process).collect();
        let (mut seeds, mut dead) = (HashSet::new(), HashSet::new());
        for entry in &entries {
            let id = process_of(entry);
            let gone = !present.contains(&id) && self.host.liveness(id) == Liveness::Dead;
            if gone {
                dead.insert(id);
            }
            if is_emitter(entry) && (gone || present.contains(&id)) {
                seeds.insert(entry.pid);
            }
        }
        let mut excluded: HashSet<ProcessRef> = entries.iter().map(process_of).collect();
        excluded.extend(game);
        excluded.extend(ancestors);
        let pass = Pass { names, watched, excluded: &excluded, remise: false };
        self.note_orphans(processes, &pass);
        self.family_pass(processes, &seeds, &pass);
        if !dead.is_empty() {
            self.change_anyway(|file| {
                if let Some(session) = file.session.as_mut() {
                    session.quieted.retain(|entry| !dead.contains(&process_of(entry)));
                }
            });
        }
    }

    /// Termine la partie : tout est remis en place, puis le bilan est noté. Si
    /// quelque chose n'a pas pu être remis, la partie reste au journal avec
    /// `restorePending`, et la remise est retentée.
    fn finish(&mut self) {
        let Some(session) = self.file.session.clone() else {
            self.live = None;
            return;
        };
        let done = self.restore_all(&session);
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
        let done = self.restore_all(&session);
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

    // --------------------------------------------------------- partie en cours

    /// Un pas pendant une partie.
    fn tick_session(&mut self, settings: &Settings, detect: bool) {
        let Some(session) = self.file.session.clone() else {
            self.live = None;
            return;
        };
        let processes = self.host.processes();
        let foreground = self.host.foreground();
        let view = self.view_of(&session.game.path, &processes, foreground.as_ref());
        self.session_step(settings, detect, &session, &processes, foreground.as_ref(), view);
        // Le relevé précédent n'est remplacé que par un relevé lisible.
        if view != View::Blind
            && let Some(live) = self.live.as_mut()
        {
            live.previous = Some(processes.iter().map(|p| p.process).collect());
        }
    }

    fn session_step(
        &mut self,
        settings: &Settings,
        detect: bool,
        session: &Session,
        processes: &[RunningProcess],
        foreground: Option<&Foreground>,
        view: View,
    ) {
        let uptime = self.host.uptime_ms();
        let lists = Lists::of(settings);
        let ending = self.live.as_ref().is_some_and(|live| live.ending);
        // Une app listée n'entre au journal ni pendant une remise en attente, ni
        // quand la partie est relâchée : ses héritiers sont alors relevés.
        let names = if ending || session.released { Vec::new() } else { quiet_names(settings) };
        let watched = foreground.map(|f| file_name(&f.exe)).filter(|name| names.contains(name));
        let (game, ancestors) = if view == View::Playing {
            let game = self.game_set(processes);
            let ancestors = ancestors_of(&game, processes);
            (game, ancestors)
        } else {
            (HashSet::new(), HashSet::new())
        };
        if ending {
            if view != View::Blind {
                self.track(processes, &names, watched.as_deref(), &game, &ancestors);
            }
            self.retry_pending();
            return;
        }
        // Fin : le mode automatique est éteint, le jeu n'est plus reconnu
        // (« Ce n'est pas un jeu »), ou plus aucun processus du jeu ne vit. Un
        // relevé illisible ne fait pas dire que le jeu est fini.
        if !settings.game_mode_auto
            || detect::recognise(&session.game.path, &lists).is_none()
            || view == View::Over
        {
            self.finish();
            return;
        }
        if view != View::Blind {
            self.track(processes, &names, watched.as_deref(), &game, &ancestors);
        }
        let on_game = foreground.is_some_and(|f| key(&f.exe) == key(&session.game.path));
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
            if detect
                && view != View::Blind
                && let Some((next, source)) = self.watch_switch(foreground, &lists)
            {
                self.switch_to(&next, source, processes);
                return;
            }
        }
        self.maintain(settings, processes, foreground, view, &game, &ancestors);
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
    /// et son début changent. L'ancien jeu passe dans le bilan et les jeux vus.
    /// Le nouveau jeu n'est jamais ralenti : il a pu naître d'une app en
    /// veilleuse, ou être lui-même au journal.
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
        let Some(session) = self.file.session.clone() else { return };
        let game = self.game_set(processes);
        let ancestors = ancestors_of(&game, processes);
        self.guard_game(&session, processes, &game, &ancestors);
        self.put_back_game(&session, &game, &ancestors);
    }

    /// Ce qui se passe à chaque pas d'une partie : l'alimentation, le jeu, puis
    /// les apps en veilleuse. Un relevé illisible ne laisse que l'alimentation.
    fn maintain(
        &mut self,
        settings: &Settings,
        processes: &[RunningProcess],
        foreground: Option<&Foreground>,
        view: View,
        game: &HashSet<ProcessRef>,
        ancestors: &HashSet<ProcessRef>,
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
        if view == View::Blind {
            return;
        }
        let Some(session) = self.file.session.clone() else { return };
        self.guard_game(&session, processes, game, ancestors);
        self.quiet_step(settings, processes, foreground, game, ancestors);
    }

    /// Le jeu n'est jamais ralenti : tout processus du jeu, et tout processus qui
    /// l'a lancé quand il n'est pas au journal (lui retrouve son état d'avant),
    /// trouvé exactement « sous la normale » est relevé. Un relèvement qui échoue
    /// est retenté toutes les 30 secondes, vingt fois au plus.
    fn guard_game(
        &mut self,
        session: &Session,
        processes: &[RunningProcess],
        game: &HashSet<ProcessRef>,
        ancestors: &HashSet<ProcessRef>,
    ) {
        let journal: HashSet<ProcessRef> = session.quieted.iter().map(process_of).collect();
        let mut raise = game.clone();
        raise.extend(ancestors.difference(&journal));
        let uptime = self.host.uptime_ms();
        for process in processes.iter().filter(|p| raise.contains(&p.process)) {
            let due = self
                .live
                .as_ref()
                .is_none_or(|live| may_try(&live.guard_failed, process.process, uptime));
            if !due {
                continue;
            }
            let Some(state) = self.host.inspect(process.process) else { continue };
            if state.priority != priority::BELOW_NORMAL {
                continue;
            }
            let raised = self
                .host
                .apply(process.process, ProcessState { priority: priority::NORMAL, ..state });
            if let Some(live) = self.live.as_mut() {
                match raised {
                    Ok(()) => {
                        live.guard_failed.remove(&process.process);
                    }
                    Err(_) => note_failure(&mut live.guard_failed, process.process, uptime),
                }
            }
        }
    }

    /// Remet à leur état d'avant les entrées du journal qui sont devenues le
    /// jeu, ou qui l'ont lancé (la personne avait mis en veilleuse un
    /// exécutable qu'elle joue maintenant) : le jeu n'est jamais ralenti.
    fn put_back_game(
        &mut self,
        session: &Session,
        game: &HashSet<ProcessRef>,
        ancestors: &HashSet<ProcessRef>,
    ) {
        let (back, staying): (Vec<Quieted>, Vec<Quieted>) =
            session.quieted.iter().cloned().partition(|entry| {
                let process = process_of(entry);
                game.contains(&process) || ancestors.contains(&process)
            });
        if !back.is_empty() {
            self.put_back(back, staying);
        }
    }

    /// Remet ces entrées à leur état d'avant, par identité, et les retire du
    /// journal. Celles qu'on n'a pas pu remettre y restent, à leur place, et sont
    /// retentées toutes les 30 secondes (vingt fois au plus : la remise de fin de
    /// partie s'en chargera ensuite). Pas de cascade : leurs héritiers non listés
    /// sont déjà relevés, et les listés ont leur propre entrée.
    fn put_back(&mut self, back: Vec<Quieted>, _staying: Vec<Quieted>) {
        let given = self.give_back(&back);
        let uptime = self.host.uptime_ms();
        let failed: HashSet<ProcessRef> = given.failed.iter().map(process_of).collect();
        if let Some(live) = self.live.as_mut() {
            live.apps_restored.extend(given.apps);
            for entry in &back {
                let process = process_of(entry);
                if failed.contains(&process) {
                    note_failure(&mut live.back_failed, process, uptime);
                } else {
                    live.back_failed.remove(&process);
                }
            }
        }
        let done: HashSet<ProcessRef> =
            back.iter().map(process_of).filter(|process| !failed.contains(process)).collect();
        self.change_anyway(|file| {
            if let Some(session) = file.session.as_mut() {
                session.quieted.retain(|entry| !done.contains(&process_of(entry)));
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
    /// éteint, app décochée, app que la personne regarde, app devenue le jeu ou
    /// qui l'a lancé), et, tous les 30 secondes, dès que la liste ou l'app
    /// regardée change, ou dès qu'un héritier listé attend, met les nouvelles
    /// en veilleuse.
    fn quiet_step(
        &mut self,
        settings: &Settings,
        processes: &[RunningProcess],
        foreground: Option<&Foreground>,
        game: &HashSet<ProcessRef>,
        ancestors: &HashSet<ProcessRef>,
    ) {
        let Some(session) = self.file.session.clone() else { return };
        let names = quiet_names(settings);
        // L'app que la personne regarde n'est jamais ralentie.
        let watched = foreground.map(|f| file_name(&f.exe)).filter(|name| names.contains(name));

        let (mut staying, back) =
            split_quieted(&session, &names, watched.as_deref(), game, ancestors);
        let uptime = self.host.uptime_ms();
        // Une remise qui a échoué attend son tour : elle reste au journal.
        let (back, waiting): (Vec<Quieted>, Vec<Quieted>) = back.into_iter().partition(|entry| {
            self.live
                .as_ref()
                .is_none_or(|live| may_try(&live.back_failed, process_of(entry), uptime))
        });
        staying.extend(waiting);
        if !back.is_empty() {
            self.put_back(back, staying);
        }

        let Some(live) = self.live.as_mut() else { return };
        let changed = live.watched != watched || live.quiet_names.as_ref() != Some(&names);
        let due = uptime.saturating_sub(live.last_scan) >= millis(SCAN_EVERY);
        let forced = std::mem::take(&mut live.force_scan);
        live.watched.clone_from(&watched);
        live.quiet_names = Some(names.clone());
        if !(changed || due || forced) {
            return;
        }
        live.last_scan = uptime;
        if !names.is_empty() {
            self.scan(processes, &names, watched.as_deref(), game, ancestors);
        }
    }

    /// Relevé des apps listées : met les nouvelles en veilleuse. Le journal n'est
    /// réécrit que si quelque chose change, et avant d'agir. Un héritier qu'on ne
    /// peut pas journaliser (journal plein, écriture en échec) est relevé tout de
    /// suite au lieu d'être laissé bas.
    fn scan(
        &mut self,
        processes: &[RunningProcess],
        names: &[String],
        watched: Option<&str>,
        game: &HashSet<ProcessRef>,
        ancestors: &HashSet<ProcessRef>,
    ) {
        let Some(session) = self.file.session.clone() else { return };
        let heirs =
            self.live.as_mut().map(|live| std::mem::take(&mut live.heirs)).unwrap_or_default();
        let unmodifiable =
            self.live.as_ref().map(|live| live.unmodifiable.clone()).unwrap_or_default();
        let _ = game;
        let mut planned = self.plan_quiet(
            processes,
            &Wish {
                game: &session.game.path,
                names,
                watched,
                current: &session.quieted,
                unmodifiable: &unmodifiable,
                heirs: &heirs,
                ancestors,
            },
        );
        let room = QUIETED_KEPT.saturating_sub(session.quieted.len());
        let overflow = planned.split_off(room.min(planned.len()));
        self.relieve_heirs(&overflow);
        if planned.is_empty() {
            return;
        }
        let mut next = session;
        next.quieted.extend(planned.iter().map(|plan| entry_of(&plan.process, plan.noted)));
        // Journal non écrit : rien n'est abaissé, on réessaiera au prochain relevé.
        if self.change(|file| file.session = Some(next)).is_ok() {
            self.act_on(planned);
        } else {
            self.relieve_heirs(&planned);
        }
    }

    /// Relève ceux de ces processus qui ont hérité de la classe basse.
    fn relieve_heirs(&self, planned: &[Planned]) {
        for plan in planned.iter().filter(|plan| plan.noted != plan.read) {
            let _ = self.host.apply(plan.process.process, DEFAULT_THROTTLE);
        }
    }
}

/// Ce que `give_back` a fait.
#[derive(Debug, Default)]
struct GiveBack {
    /// Les entrées qu'on n'a pas pu remettre (elles restent au journal).
    failed: Vec<Quieted>,
    /// Les apps remises, une entrée par processus.
    apps: Vec<String>,
    /// Les numéros des émetteurs dont le processus est mort.
    dead_emitters: Vec<u32>,
}

/// Les réglages d'une passe de famille.
struct Pass<'a> {
    /// Les noms d'apps qu'on peut encore mettre au journal (vide : aucune).
    names: &'a [String],
    /// L'app que la personne regarde.
    watched: Option<&'a str>,
    /// Ce qui ne peut pas entrer dans la famille : le journal, le jeu et ses
    /// ancêtres.
    excluded: &'a HashSet<ProcessRef>,
    /// C'est une remise en place : tout héritier est relevé, listé ou non.
    remise: bool,
}

/// Sépare les entrées du journal : celles qui restent en veilleuse, et celles
/// qui doivent retrouver leur état d'avant (app décochée, app regardée, app
/// devenue le jeu, ou app qui l'a lancé).
fn split_quieted(
    session: &Session,
    names: &[String],
    watched: Option<&str>,
    game: &HashSet<ProcessRef>,
    ancestors: &HashSet<ProcessRef>,
) -> (Vec<Quieted>, Vec<Quieted>) {
    session.quieted.iter().cloned().partition(|entry| {
        let process = process_of(entry);
        names.contains(&entry.exe)
            && watched != Some(entry.exe.as_str())
            && !game.contains(&process)
            && !ancestors.contains(&process)
    })
}

#[cfg(test)]
mod tests;
