//! Le guichet du mode jeu : ce que la fenêtre peut lire et demander.
//!
//! [`GameDesk`] garde ce qui coûte à lire (noms de fichiers, prise en charge
//! de l'alimentation, dernière liste d'apps ouvertes) et le compte des
//! relances de l'agent. Les modifications de réglages sont des fonctions pures
//! sur [`Settings`] : le guichet ([`crate::backend::Backend`]) les applique
//! sous son verrou et les enregistre.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::game::apps::{self, BUILT_IN};
use crate::game::control::AgentControl;
use crate::game::detect::{self, Source, key};
use crate::game::host::{GameHost, PowerPlan, stem};
use crate::game::store::{GameFile, GameStore, Last};
use crate::game::unsuited_processor;
use crate::game::view::{
    GameRefusal, GameSession, GameStatus, GameView, KnownGame, LastGame, NotGame, OpenApp,
    PowerSupport, QuietApp,
};
use crate::platform::Environment;
use crate::platform::agent::{AgentState, Autostart, OpenProgram, Order, WindowsPage};
use crate::settings::{MAX_EXE_NAMES, MAX_GAME_PATHS, Settings, clean_exe_name};

/// Nombre maximal de relances de l'agent par ouverture de Shinku.
const RELAUNCHES: u32 = 3;
/// Délai minimal entre deux relances.
const RELAUNCH_GAP: Duration = Duration::from_secs(10);
/// Délai avant de revérifier qu'un PC sans « Performances élevées » n'en a toujours pas.
const SUPPORT_RECHECK: Duration = Duration::from_secs(30);
/// Délai avant de chercher à nouveau le nom d'une app ajoutée qu'on n'a pas su lire.
const LABEL_RECHECK: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
struct Cache {
    /// Nom à montrer, par chemin (clé de [`detect::key`]).
    names: HashMap<String, String>,
    /// Ce que l'alimentation permet, et quand on l'a su.
    support: Option<(PowerSupport, Instant)>,
    /// Le nom à montrer d'une app ajoutée, par nom d'exécutable (en minuscules).
    labels: HashMap<String, String>,
    /// Quand on a cherché pour la dernière fois le nom d'une app ajoutée sans
    /// le trouver : on ne rouvre pas tous les processus à chaque sondage.
    label_misses: HashMap<String, Instant>,
    /// La dernière liste d'apps ouvertes rendue à la page.
    open: Vec<OpenProgram>,
    /// Relances de l'agent depuis l'ouverture de Shinku.
    relaunches: u32,
    last_relaunch: Option<Instant>,
}

/// Le mode jeu, vu du guichet.
#[derive(Debug)]
pub struct GameDesk {
    environment: Arc<dyn Environment>,
    host: Arc<dyn GameHost>,
    control: Arc<dyn AgentControl>,
    store: Option<GameStore>,
    relaunch_gap: Duration,
    cache: Mutex<Cache>,
}

impl GameDesk {
    /// Un guichet sur cette machine, cet agent et ce journal.
    pub fn new(
        environment: Arc<dyn Environment>,
        host: Arc<dyn GameHost>,
        control: Arc<dyn AgentControl>,
        store: Option<GameStore>,
    ) -> Self {
        Self {
            environment,
            host,
            control,
            store,
            relaunch_gap: RELAUNCH_GAP,
            cache: Mutex::new(Cache::default()),
        }
    }

    /// Change le délai minimal entre deux relances de l'agent (pour les tests).
    pub fn with_relaunch_gap(mut self, gap: Duration) -> Self {
        self.relaunch_gap = gap;
        self
    }

    fn cache(&self) -> MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Le journal du mode jeu, tel que l'agent l'a écrit.
    pub fn file(&self) -> GameFile {
        self.store.as_ref().map(GameStore::load).unwrap_or_default()
    }

    /// Le bilan de la dernière partie, tel que `game.json` le dit. **Lecture
    /// seule** : ni l'agent, ni les processus, ni l'alimentation ne sont
    /// regardés. Sans chemin.
    pub fn last(&self) -> Option<LastGame> {
        self.file().last.as_ref().map(last_game)
    }

    /// Le nom à montrer pour un exécutable : demandé une fois à la machine.
    fn name_of(&self, path: &str) -> String {
        let wanted = key(path);
        if let Some(known) = self.cache().names.get(&wanted) {
            return known.clone();
        }
        // Lu hors du verrou : lire les ressources d'un fichier peut être long.
        let name = self.host.display_name(path);
        self.cache().names.insert(wanted, name.clone());
        name
    }

    /// Le nom à montrer d'une app ajoutée par la personne : la description de
    /// son fichier, quand un processus de ce nom tourne ou que le nom est déjà
    /// en cache ; sinon le nom du fichier sans `.exe`.
    fn quiet_label(&self, exe: &str, running: &HashSet<String>) -> String {
        {
            let cache = self.cache();
            if let Some(label) = cache.labels.get(exe) {
                return label.clone();
            }
            if cache.label_misses.get(exe).is_some_and(|miss| miss.elapsed() < LABEL_RECHECK) {
                return stem(exe);
            }
        }
        if running.contains(exe) {
            // Lu hors du verrou : lister les processus et lire leurs ressources est long.
            let path = self
                .host
                .processes()
                .iter()
                .filter(|process| process.exe_name == exe)
                .find_map(|process| self.host.exe_path(process.process));
            if let Some(path) = path {
                let label = self.name_of(&path);
                self.cache().labels.insert(exe.to_owned(), label.clone());
                return label;
            }
            self.cache().label_misses.insert(exe.to_owned(), Instant::now());
        }
        stem(exe)
    }

    /// Note le nom à montrer d'une app ouverte que la personne ajoute : elle
    /// tourne, on peut lire son fichier. Le nom reste quand elle se ferme.
    pub fn remember_label(&self, program: &OpenProgram) {
        let exe = detect::file_name(&program.path);
        let label = self.name_of(&program.path);
        self.cache().labels.insert(exe, label);
    }

    fn power_support(&self) -> PowerSupport {
        if let Some((support, since)) = self.cache().support
            && (support != PowerSupport::Unavailable || since.elapsed() < SUPPORT_RECHECK)
        {
            return support;
        }
        let support = if unsuited_processor(&self.host.processor_name()) {
            PowerSupport::Unsuited
        } else if self.host.has_power_plan(PowerPlan::HIGH_PERFORMANCE) {
            PowerSupport::Available
        } else {
            PowerSupport::Unavailable
        };
        self.cache().support = Some((support, Instant::now()));
        support
    }

    /// Relance l'agent s'il est arrêté : au plus une fois toutes les 10
    /// secondes, et trois fois au plus par ouverture de Shinku.
    fn relaunch_if_stopped(&self) -> AgentState {
        let state = self.control.state();
        if state != AgentState::Stopped {
            return state;
        }
        {
            let mut cache = self.cache();
            let waited = cache.last_relaunch.is_none_or(|last| last.elapsed() >= self.relaunch_gap);
            if cache.relaunches >= RELAUNCHES || !waited {
                return state;
            }
            cache.relaunches += 1;
            cache.last_relaunch = Some(Instant::now());
        }
        self.control.spawn(false)
    }

    /// Tout ce que la page affiche.
    pub fn view(&self, settings: &Settings) -> GameView {
        let file = self.file();
        let agent =
            if settings.game_mode_auto { self.relaunch_if_stopped() } else { self.control.state() };
        let running: HashSet<String> = self
            .environment
            .running_processes()
            .unwrap_or_default()
            .into_iter()
            .map(|name| name.to_lowercase())
            .collect();
        let effective = apps::effective(settings);

        let mut quiet_apps: Vec<QuietApp> = BUILT_IN
            .iter()
            .map(|app| QuietApp {
                exe: app.exe.to_owned(),
                label: app.label.to_owned(),
                on: effective.iter().any(|name| name == app.exe),
                running: running.contains(app.exe),
                built_in: true,
            })
            .collect();
        for added in &settings.game_mode_quiet_added {
            if apps::built_in(added).is_none() {
                quiet_apps.push(QuietApp {
                    exe: added.clone(),
                    label: self.quiet_label(added, &running),
                    on: effective.contains(added),
                    running: running.contains(added),
                    built_in: false,
                });
            }
        }

        GameView {
            auto: settings.game_mode_auto,
            power: settings.game_mode_power,
            quiet_apps: settings.game_mode_quiet_apps,
            agent,
            startup_disabled: self.control.autostart() == Autostart::DisabledInWindows,
            session: file.session.as_ref().map(|session| {
                let mut apps: Vec<&str> = session.quieted.iter().map(|q| q.exe.as_str()).collect();
                apps.sort_unstable();
                apps.dedup();
                GameSession {
                    name: session.game.name.clone(),
                    path: session.game.path.clone(),
                    since: session.since,
                    source: session.game.source,
                    power: session.power.outcome,
                    quieted_apps: u32::try_from(apps.len()).unwrap_or(u32::MAX),
                    released: session.released,
                    restore_pending: session.restore_pending,
                }
            }),
            last: file.last.as_ref().map(last_game),
            power_support: self.power_support(),
            on_battery: self.host.on_battery(),
            apps: quiet_apps,
            games: self.games(settings, &file),
            not_games: settings
                .game_mode_not_games
                .iter()
                .map(|path| {
                    let path = path.to_string_lossy().into_owned();
                    NotGame { name: self.name_of(&path), path }
                })
                .collect(),
        }
    }

    /// Les jeux ajoutés (le dernier ajouté d'abord), puis les jeux reconnus déjà
    /// joués (le plus récent d'abord), sans ceux que la personne a écartés ni
    /// ceux qu'elle a déjà ajoutés.
    fn games(&self, settings: &Settings, file: &GameFile) -> Vec<KnownGame> {
        let played = |path: &str| {
            file.seen.iter().find(|seen| key(&seen.path) == key(path)).map(|seen| seen.last_played)
        };
        let mut games: Vec<KnownGame> = settings
            .game_mode_games
            .iter()
            .rev()
            .map(|path| {
                let path = path.to_string_lossy().into_owned();
                KnownGame {
                    name: self.name_of(&path),
                    source: Source::Added,
                    last_played: played(&path),
                    path,
                }
            })
            .collect();
        let hidden: HashSet<String> = settings
            .game_mode_games
            .iter()
            .chain(&settings.game_mode_not_games)
            .map(|path| key(&path.to_string_lossy()))
            .collect();
        for seen in &file.seen {
            if hidden.contains(&key(&seen.path)) {
                continue;
            }
            let name =
                if seen.name.is_empty() { self.name_of(&seen.path) } else { seen.name.clone() };
            games.push(KnownGame {
                path: seen.path.clone(),
                name,
                source: Source::Library,
                last_played: Some(seen.last_played),
            });
        }
        games
    }

    /// Les apps ouvertes en ce moment. Le cœur garde cette liste : les numéros
    /// rendus ne valent que pour elle.
    pub fn open_apps(&self) -> Vec<OpenApp> {
        let open = self.control.open_apps();
        let apps = open
            .iter()
            .enumerate()
            .map(|(id, app)| OpenApp {
                id: u32::try_from(id).unwrap_or(u32::MAX),
                name: app.name.clone(),
                path: app.path.clone(),
            })
            .collect();
        self.cache().open = open;
        apps
    }

    /// L'app de ce numéro, dans la dernière liste rendue, à condition que
    /// `path` soit exactement son chemin. Le chemin n'est que **comparé** : la
    /// page ne désigne jamais autre chose que ce que le cœur lui a donné, et une
    /// liste qui a changé entre-temps (le même numéro pour une autre app) est
    /// refusée.
    pub fn open_app(&self, id: u32, path: &str) -> Result<OpenProgram, GameRefusal> {
        let index = usize::try_from(id).map_err(|_| GameRefusal::UnknownApp)?;
        let program = self.cache().open.get(index).cloned().ok_or(GameRefusal::UnknownApp)?;
        if program.path == path { Ok(program) } else { Err(GameRefusal::UnknownApp) }
    }

    /// Les chemins que la page peut désigner : jeux ajoutés, jeux vus, jeux
    /// écartés, et le jeu de la partie en cours.
    pub fn known_paths(&self, settings: &Settings) -> Vec<String> {
        let file = self.file();
        settings
            .game_mode_games
            .iter()
            .chain(&settings.game_mode_not_games)
            .map(|path| path.to_string_lossy().into_owned())
            .chain(file.seen.iter().map(|seen| seen.path.clone()))
            .chain(file.session.iter().map(|session| session.game.path.clone()))
            .collect()
    }

    /// Met l'agent et la valeur `Run` d'accord avec les réglages.
    ///
    /// - la valeur `Run` est présente si le mode automatique est allumé et
    ///   que Shinku doit se lancer avec Windows, absente sinon ;
    /// - si le mode automatique est allumé, ou si le journal contient une
    ///   partie, et que l'agent ne tourne pas : il est lancé.
    pub fn sync(&self, settings: &Settings) {
        let run = settings.game_mode_auto && settings.launch_with_windows;
        // Un échec de registre ne doit pas empêcher de lancer l'agent.
        let _ = self.control.set_autostart(run);
        let wanted = settings.game_mode_auto || self.file().session.is_some();
        if wanted && self.control.state() == AgentState::Stopped {
            self.control.spawn(false);
        }
    }

    /// « Tout remettre maintenant » : un ordre à l'agent qui tourne ; sans
    /// agent mais avec une partie au journal, un agent est lancé pour la relâcher.
    pub fn release(&self) {
        let ordered =
            self.control.state() == AgentState::Running && self.control.post(Order::Release);
        if !ordered && self.file().session.is_some() {
            self.control.spawn(true);
        }
    }

    /// Ouvre une page des réglages de Windows.
    pub fn open_settings(&self, page: WindowsPage) {
        // Rien à dire à la personne si Windows ne l'ouvre pas : elle le voit.
        let _ = self.control.open_settings(page);
    }
}

fn contains(list: &[String], exe: &str) -> bool {
    list.iter().any(|name| name.eq_ignore_ascii_case(exe))
}

/// Le bilan de la dernière partie, sans le chemin de l'exécutable.
fn last_game(last: &Last) -> LastGame {
    LastGame {
        name: last.name.clone(),
        started: last.started,
        ended: last.ended,
        power_restored: last.power_restored,
        apps_restored: last.apps_restored,
        complete: last.complete,
    }
}

/// Allume ou éteint une app. `exe` doit déjà être connu (intégrée ou ajoutée)
/// sauf si `new_app` est vrai. Allumer une app protégée (lanceur de jeux,
/// programme de Windows ou de Shinku) est refusé.
///
/// Allumer : une app cochée d'office est retirée de ceux qu'on a décochés ; sinon
/// elle est ajoutée aux cochés. Éteindre : une app ajoutée est retirée ; une
/// app cochée d'office est ajoutée aux décochés. La liste est restée telle
/// quelle si la demande est refusée.
pub fn set_quiet(
    settings: &mut Settings,
    exe: &str,
    on: bool,
    new_app: bool,
) -> Result<(), GameRefusal> {
    let exe = exe.to_lowercase();
    if !new_app
        && apps::built_in(&exe).is_none()
        && !contains(&settings.game_mode_quiet_added, &exe)
    {
        return Err(GameRefusal::UnknownApp);
    }
    // Un lanceur de jeux, un programme de Windows : jamais mis en veilleuse.
    // (Éteindre reste permis : c'est la façon de retirer un reste de la liste.)
    if on && apps::protected(&exe) {
        return Err(GameRefusal::ProtectedApp);
    }
    let mut next = settings.clone();
    if on {
        next.game_mode_quiet_removed.retain(|name| !name.eq_ignore_ascii_case(&exe));
        if !apps::effective(&next).contains(&exe) && !contains(&next.game_mode_quiet_added, &exe) {
            next.game_mode_quiet_added.push(exe);
        }
    } else {
        next.game_mode_quiet_added.retain(|name| !name.eq_ignore_ascii_case(&exe));
        if apps::effective(&next).contains(&exe) && !contains(&next.game_mode_quiet_removed, &exe) {
            next.game_mode_quiet_removed.push(exe);
        }
    }
    if next.game_mode_quiet_added.len() > MAX_EXE_NAMES
        || next.game_mode_quiet_removed.len() > MAX_EXE_NAMES
    {
        return Err(GameRefusal::ListFull);
    }
    *settings = next;
    Ok(())
}

/// Ajoute une app ouverte aux apps en veilleuse. Refuse un nom protégé.
pub fn add_quiet_app(settings: &mut Settings, program: &OpenProgram) -> Result<(), GameRefusal> {
    let name = detect::file_name(&program.path);
    let Some(exe) = clean_exe_name(&name) else { return Err(GameRefusal::UnknownApp) };
    if apps::protected(&exe) {
        return Err(GameRefusal::ProtectedApp);
    }
    set_quiet(settings, &exe, true, true)
}

/// Ajoute un jeu ouvert. Refuse un programme qui n'est jamais un jeu. Le
/// retire de ceux que la personne avait écartés.
pub fn add_game(settings: &mut Settings, program: &OpenProgram) -> Result<(), GameRefusal> {
    let wanted = key(&program.path);
    if wanted.is_empty() {
        return Err(GameRefusal::UnknownApp);
    }
    if detect::never_a_game(&program.path) {
        return Err(GameRefusal::NotAGame);
    }
    let listed = |list: &[PathBuf]| list.iter().any(|path| key(&path.to_string_lossy()) == wanted);
    if !listed(&settings.game_mode_games) && settings.game_mode_games.len() >= MAX_GAME_PATHS {
        return Err(GameRefusal::ListFull);
    }
    settings.game_mode_not_games.retain(|path| key(&path.to_string_lossy()) != wanted);
    if !listed(&settings.game_mode_games) {
        settings.game_mode_games.push(PathBuf::from(&program.path));
    }
    Ok(())
}

/// Dit ce que la personne pense d'un jeu. `path` n'est que **comparé** aux
/// chemins connus ; c'est l'écriture du chemin connu qui est gardée.
pub fn set_status(
    settings: &mut Settings,
    known: &[String],
    path: &str,
    status: GameStatus,
) -> Result<(), GameRefusal> {
    let wanted = key(path);
    let Some(known) = known.iter().find(|candidate| key(candidate) == wanted && !wanted.is_empty())
    else {
        return Err(GameRefusal::UnknownGame);
    };
    let same = |other: &PathBuf| key(&other.to_string_lossy()) == wanted;
    match status {
        GameStatus::NotGame => {
            let listed = settings.game_mode_not_games.iter().any(same);
            if !listed && settings.game_mode_not_games.len() >= MAX_GAME_PATHS {
                return Err(GameRefusal::ListFull);
            }
            settings.game_mode_games.retain(|other| !same(other));
            if !listed {
                settings.game_mode_not_games.push(PathBuf::from(known));
            }
        }
        GameStatus::Forgotten => {
            settings.game_mode_games.retain(|other| !same(other));
            settings.game_mode_not_games.retain(|other| !same(other));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(path: &str) -> OpenProgram {
        OpenProgram { name: detect::file_name(path), path: path.to_owned() }
    }

    #[test]
    fn a_default_app_is_unchecked_then_checked_again() {
        let mut settings = Settings::default();
        set_quiet(&mut settings, "OneDrive.exe", false, false).unwrap();
        assert_eq!(settings.game_mode_quiet_removed, ["onedrive.exe"]);
        // Éteindre deux fois ne double rien.
        set_quiet(&mut settings, "onedrive.exe", false, false).unwrap();
        assert_eq!(settings.game_mode_quiet_removed, ["onedrive.exe"]);
        set_quiet(&mut settings, "onedrive.exe", true, false).unwrap();
        assert!(settings.game_mode_quiet_removed.is_empty());
        assert!(settings.game_mode_quiet_added.is_empty(), "déjà cochée d'office : pas d'ajout");
    }

    #[test]
    fn an_unknown_exe_is_refused_unless_it_comes_from_the_open_list() {
        let mut settings = Settings::default();
        assert_eq!(set_quiet(&mut settings, "x.exe", true, false), Err(GameRefusal::UnknownApp));
        assert_eq!(settings, Settings::default());
        set_quiet(&mut settings, "x.exe", true, true).unwrap();
        assert_eq!(settings.game_mode_quiet_added, ["x.exe"]);
    }

    #[test]
    fn the_quiet_list_is_bounded_and_a_refusal_leaves_it_untouched() {
        let mut settings = Settings::default();
        for index in 0..MAX_EXE_NAMES {
            settings.game_mode_quiet_added.push(format!("app{index}.exe"));
        }
        let before = settings.clone();
        assert_eq!(set_quiet(&mut settings, "encore.exe", true, true), Err(GameRefusal::ListFull));
        assert_eq!(settings, before);
        // Une app déjà dans la liste n'a pas besoin de place.
        set_quiet(&mut settings, "app3.exe", true, false).unwrap();
    }

    #[test]
    fn the_game_lists_are_bounded() {
        let mut settings = Settings::default();
        for index in 0..MAX_GAME_PATHS {
            settings.game_mode_games.push(PathBuf::from(format!(r"D:\Jeux\g{index}\g.exe")));
        }
        assert_eq!(
            add_game(&mut settings, &program(r"D:\Jeux\nouveau\n.exe")),
            Err(GameRefusal::ListFull)
        );
        assert_eq!(settings.game_mode_games.len(), MAX_GAME_PATHS);
        // Un jeu déjà là n'est pas un nouveau venu.
        add_game(&mut settings, &program(r"D:\Jeux\g1\g.exe")).unwrap();
    }

    #[test]
    fn a_browser_or_a_windows_tool_is_never_a_game() {
        let mut settings = Settings::default();
        for path in [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Windows\System32\cmd.exe",
        ] {
            assert_eq!(
                add_game(&mut settings, &program(path)),
                Err(GameRefusal::NotAGame),
                "{path}"
            );
        }
        assert_eq!(add_game(&mut settings, &program("")), Err(GameRefusal::UnknownApp));
    }

    #[test]
    fn a_protected_app_never_joins_the_quiet_list() {
        let mut settings = Settings::default();
        assert_eq!(
            add_quiet_app(&mut settings, &program(r"C:\Windows\explorer.exe")),
            Err(GameRefusal::ProtectedApp)
        );
        assert_eq!(
            add_quiet_app(&mut settings, &program(r"C:\Windows\System32\svchost.exe")),
            Err(GameRefusal::ProtectedApp)
        );
        add_quiet_app(&mut settings, &program(r"C:\Outils\Sync.exe")).unwrap();
        assert_eq!(settings.game_mode_quiet_added, ["sync.exe"]);
    }

    #[test]
    fn a_status_keeps_the_known_spelling_of_the_path() {
        let mut settings = Settings::default();
        let known = vec![r"D:\Jeux\Hollow Peaks\hp.exe".to_owned()];
        set_status(&mut settings, &known, "d:/jeux/hollow peaks/HP.EXE", GameStatus::NotGame)
            .unwrap();
        assert_eq!(settings.game_mode_not_games, [PathBuf::from(&known[0])]);
        assert_eq!(
            set_status(&mut settings, &known, "", GameStatus::NotGame),
            Err(GameRefusal::UnknownGame)
        );
    }

    #[test]
    fn a_protected_app_cannot_be_ticked_even_from_the_open_list() {
        let mut settings = Settings::default();
        assert_eq!(
            set_quiet(&mut settings, "steam.exe", true, true),
            Err(GameRefusal::ProtectedApp)
        );
        assert_eq!(settings, Settings::default());
    }
}
