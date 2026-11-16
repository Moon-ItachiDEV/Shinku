//! Une machine fabriquée de toutes pièces pour le module « Désinstallation » :
//! des inscriptions, un petit disque (fichiers, dossiers, liens), des
//! raccourcis, une corbeille et un lanceur qui notent tout ce qu'on leur
//! demande. Elle sert aux tests du cœur et au contrat avec l'interface.
//!
//! Les chemins sont ceux de Windows (`C:\…`), comparés sans tenir compte de la
//! casse. Le « profil » de cette machine est `C:\Users\Léa`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::apps::host::{
    AccountFolder, AppsHost, Child, Existence, Launch, Measure, RawRegistration, RawSource,
    Shortcut, SystemDirs, Watch,
};
use crate::apps::paths::{is_inside, is_same_or_inside, norm, parent};
use crate::apps::{Place, Source};
use crate::platform::RecycleError;

/// Ce que la machine fabriquée répond à un lancement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchPlan {
    /// Lancé, sans que le processus soit suivi.
    Started,
    /// La personne dit non à la fenêtre de Windows.
    Cancelled,
    /// Fichier introuvable.
    Missing,
    /// Échec, avec ce code.
    Failed(u32),
}

/// Un lancement noté par la machine fabriquée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launched {
    /// Le programme, chemin complet.
    pub program: String,
    /// Les paramètres, tels que passés.
    pub parameters: String,
    /// Avec la demande de droits de Windows.
    pub elevated: bool,
    /// Avec une garde sur le processus.
    pub watched: bool,
}

/// Comment la corbeille fabriquée répond pour un chemin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FakeRecycle {
    /// Un programme l'utilise.
    Locked,
    /// La corbeille ne peut pas le recevoir.
    Refused,
    /// Accès refusé.
    Denied,
    /// Échec d'une autre sorte.
    Other,
    /// Elle dit oui, mais une partie reste sur le disque.
    LeavesSome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Node {
    File(u64),
    Dir,
    Link,
}

#[derive(Debug)]
struct Fake {
    registrations: HashMap<Place, Vec<RawRegistration>>,
    unreadable_places: HashSet<Place>,
    incomplete_places: HashSet<Place>,
    unreadable_keys: HashSet<(Source, String)>,
    variables: HashMap<String, String>,
    nodes: BTreeMap<String, (String, Node)>,
    denied: HashSet<String>,
    remote_drives: HashSet<char>,
    unmeasurable: HashSet<String>,
    unreadable_dirs: HashSet<String>,
    shortcuts: Vec<Shortcut>,
    unreadable_shortcuts: HashSet<String>,
    folders: HashMap<AccountFolder, String>,
    own_dir: Option<String>,
    subtrees: Vec<String>,
    itself: Vec<String>,
    sync_roots: Vec<String>,
    launches: Vec<Launched>,
    launch_plan: LaunchPlan,
    gate: Option<(Sender<()>, Receiver<()>)>,
    recycle_failures: HashMap<String, FakeRecycle>,
    recycled: Vec<String>,
    uptime: Duration,
    registration_reads: usize,
    long_paths: HashMap<String, Option<String>>,
}

/// La machine fabriquée.
#[derive(Debug)]
pub struct FakeAppsHost {
    inner: Mutex<Fake>,
    ended: Arc<AtomicBool>,
}

impl Default for FakeAppsHost {
    fn default() -> Self {
        Self::new()
    }
}

/// La garde sur un processus de la machine fabriquée.
#[derive(Debug)]
struct FakeWatch(Arc<AtomicBool>);

impl Watch for FakeWatch {
    fn ended(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Le profil de la machine fabriquée.
pub const PROFILE: &str = r"C:\Users\Léa";
/// Son `%APPDATA%`.
pub const ROAMING: &str = r"C:\Users\Léa\AppData\Roaming";
/// Son `%LOCALAPPDATA%`.
pub const LOCAL: &str = r"C:\Users\Léa\AppData\Local";
/// Son `LocalLow`.
pub const LOCAL_LOW: &str = r"C:\Users\Léa\AppData\LocalLow";
/// Son menu Démarrer.
pub const START_MENU: &str = r"C:\Users\Léa\AppData\Roaming\Microsoft\Windows\Start Menu";
/// Son Bureau.
pub const DESKTOP: &str = r"C:\Users\Léa\Desktop";

impl FakeAppsHost {
    /// Un PC vide : Windows dans `C:\Windows`, un profil `C:\Users\Léa`, les
    /// dossiers protégés habituels, Shinku dans `C:\Program Files\Shinku`.
    pub fn new() -> Self {
        let mut fake = Fake {
            registrations: HashMap::new(),
            unreadable_places: HashSet::new(),
            incomplete_places: HashSet::new(),
            unreadable_keys: HashSet::new(),
            variables: HashMap::new(),
            nodes: BTreeMap::new(),
            denied: HashSet::new(),
            remote_drives: HashSet::new(),
            unmeasurable: HashSet::new(),
            unreadable_dirs: HashSet::new(),
            shortcuts: Vec::new(),
            unreadable_shortcuts: HashSet::new(),
            folders: HashMap::new(),
            own_dir: Some(r"C:\Program Files\Shinku".to_owned()),
            subtrees: [
                r"C:\Windows",
                r"C:\Program Files",
                r"C:\Program Files (x86)",
                r"C:\ProgramData",
            ]
            .map(norm)
            .to_vec(),
            itself: [
                r"C:\Users",
                PROFILE,
                r"C:\Users\Léa\AppData",
                LOCAL,
                LOCAL_LOW,
                ROAMING,
                DESKTOP,
                r"C:\Users\Léa\Documents",
                r"C:\Users\Léa\Downloads",
            ]
            .map(norm)
            .to_vec(),
            sync_roots: Vec::new(),
            launches: Vec::new(),
            launch_plan: LaunchPlan::Started,
            gate: None,
            recycle_failures: HashMap::new(),
            recycled: Vec::new(),
            uptime: Duration::from_secs(10_000),
            registration_reads: 0,
            long_paths: HashMap::new(),
        };
        for (name, value) in [
            ("APPDATA", ROAMING),
            ("LOCALAPPDATA", LOCAL),
            ("USERPROFILE", PROFILE),
            ("SYSTEMROOT", r"C:\Windows"),
            ("WINDIR", r"C:\Windows"),
            ("PROGRAMFILES", r"C:\Program Files"),
            ("PROGRAMFILES(X86)", r"C:\Program Files (x86)"),
            ("PROGRAMDATA", r"C:\ProgramData"),
        ] {
            fake.variables.insert(name.to_owned(), value.to_owned());
        }
        for (folder, path) in [
            (AccountFolder::Roaming, ROAMING),
            (AccountFolder::Local, LOCAL),
            (AccountFolder::LocalLow, LOCAL_LOW),
            (AccountFolder::StartMenu, START_MENU),
            (AccountFolder::Desktop, DESKTOP),
        ] {
            fake.folders.insert(folder, path.to_owned());
        }
        let host = Self { inner: Mutex::new(fake), ended: Arc::new(AtomicBool::new(false)) };
        for dir in [
            r"C:\Windows\System32",
            ROAMING,
            LOCAL,
            LOCAL_LOW,
            START_MENU,
            DESKTOP,
            r"C:\Program Files\Shinku",
        ] {
            host.add_dir(dir);
        }
        host.add_file(r"C:\Windows\System32\msiexec.exe", 100);
        host.add_file(r"C:\Windows\System32\rundll32.exe", 100);
        host.add_file(r"C:\Windows\System32\cmd.exe", 100);
        host.add_file(r"C:\Program Files\Shinku\shinku.exe", 100);
        host
    }

    fn state(&self) -> MutexGuard<'_, Fake> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // --- Registre ---

    /// Ajoute une inscription à une place.
    pub fn add_registration(&self, place: Place, registration: RawRegistration) {
        self.state().registrations.entry(place).or_default().push(registration);
    }

    /// Retire l'inscription de cette sous-clé (sans tenir compte de la casse).
    pub fn remove_registration(&self, place: Place, key: &str) {
        if let Some(list) = self.state().registrations.get_mut(&place) {
            list.retain(|raw| !raw.key.eq_ignore_ascii_case(key));
        }
    }

    /// Remplace une inscription de la place du PC ou du compte (même sous-clé).
    pub fn replace_registration(&self, place: Place, registration: RawRegistration) {
        self.remove_registration(place, &registration.key);
        self.add_registration(place, registration);
    }

    /// Cette place ne se lit pas.
    pub fn make_unreadable(&self, place: Place) {
        self.state().unreadable_places.insert(place);
    }

    /// Cette place se lit de nouveau.
    pub fn make_readable(&self, place: Place) {
        self.state().unreadable_places.remove(&place);
        self.state().incomplete_places.remove(&place);
    }

    /// L'hôte a dû laisser quelque chose de côté dans cette place.
    pub fn make_incomplete(&self, place: Place) {
        self.state().incomplete_places.insert(place);
    }

    /// La relecture de cette inscription seule est refusée (la place, elle, se lit).
    pub fn make_registration_unreadable(&self, source: Source, key: &str) {
        self.state().unreadable_keys.insert((source, key.to_lowercase()));
    }

    /// Combien de fois une inscription a été relue seule.
    pub fn registration_reads(&self) -> usize {
        self.state().registration_reads
    }

    /// Définit une variable d'environnement.
    pub fn set_variable(&self, name: &str, value: &str) {
        self.state().variables.insert(name.to_uppercase(), value.to_owned());
    }

    // --- Disque ---

    fn insert_node(&self, path: &str, node: Node) {
        let original = path.trim_end_matches(['\\', '/']).replace('/', "\\");
        let mut ancestors = Vec::new();
        let mut cursor = original.clone();
        while let Some(up) = parent(&cursor) {
            if up.len() <= 3 {
                break;
            }
            ancestors.push(up.to_owned());
            cursor = up.to_owned();
        }
        let mut state = self.state();
        for ancestor in ancestors {
            state.nodes.entry(norm(&ancestor)).or_insert((ancestor, Node::Dir));
        }
        state.nodes.insert(norm(&original), (original, node));
    }

    /// Ce chemin est un fichier de cette taille (ses dossiers existent aussi).
    pub fn add_file(&self, path: &str, bytes: u64) {
        self.insert_node(path, Node::File(bytes));
    }

    /// Ce chemin est un dossier (ses parents aussi).
    pub fn add_dir(&self, path: &str) {
        self.insert_node(path, Node::Dir);
    }

    /// Ce chemin est un lien (jonction) : jamais suivi.
    pub fn add_link(&self, path: &str) {
        self.insert_node(path, Node::Link);
    }

    /// Retire ce chemin et tout ce qu'il contient.
    pub fn remove_path(&self, path: &str) {
        let key = norm(path);
        self.state().nodes.retain(|name, _| *name != key && !is_inside(name, &key));
    }

    /// La machine ne sait pas dire ce qu'est ce chemin (accès refusé…).
    pub fn deny_access(&self, path: &str) {
        self.state().denied.insert(norm(path));
    }

    /// Ce lecteur (`'Z'`) n'est pas un lecteur fixe.
    pub fn add_remote_drive(&self, letter: char) {
        self.state().remote_drives.insert(letter.to_ascii_uppercase());
    }

    /// Ce dossier ne se compte pas en entier.
    pub fn make_unmeasurable(&self, path: &str) {
        self.state().unmeasurable.insert(norm(path));
    }

    /// Le contenu de ce dossier ne se lit pas.
    pub fn make_dir_unreadable(&self, path: &str) {
        self.state().unreadable_dirs.insert(norm(path));
    }

    /// Ce raccourci existe, avec cette cible brute.
    pub fn add_shortcut(&self, path: &str, target: Option<&str>) {
        self.add_file(path, 1024);
        self.state()
            .shortcuts
            .push(Shortcut { path: path.to_owned(), target: target.map(str::to_owned) });
    }

    // --- Dossiers, garde, Shinku ---

    /// Change un dossier du compte.
    pub fn set_account_folder(&self, folder: AccountFolder, path: Option<&str>) {
        let mut state = self.state();
        match path {
            Some(path) => state.folders.insert(folder, path.to_owned()),
            None => state.folders.remove(&folder),
        };
    }

    /// Change le dossier de `shinku.exe`.
    pub fn set_own_dir(&self, dir: Option<&str>) {
        self.state().own_dir = dir.map(str::to_owned);
    }

    /// Ce dossier et tout ce qu'il contient sont protégés par le garde.
    pub fn protect_tree(&self, path: &str) {
        self.state().subtrees.push(norm(path));
    }

    /// Ce dossier lui-même est protégé par le garde (son contenu ne l'est pas).
    pub fn protect_itself(&self, path: &str) {
        self.state().itself.push(norm(path));
    }

    /// Ce dossier est synchronisé avec un service en ligne.
    pub fn add_sync_root(&self, path: &str) {
        self.state().sync_roots.push(path.to_owned());
    }

    // --- Lancement ---

    /// Ce que le prochain lancement répond.
    pub fn set_launch(&self, plan: LaunchPlan) {
        self.state().launch_plan = plan;
    }

    /// Les lancements demandés, dans l'ordre.
    pub fn launches(&self) -> Vec<Launched> {
        self.state().launches.clone()
    }

    /// Le processus lancé avec une garde est sorti.
    pub fn end_process(&self) {
        self.ended.store(true, Ordering::SeqCst);
    }

    /// Le prochain lancement attend qu'on le libère, comme Windows qui attend
    /// la réponse à sa fenêtre. Rend le signal « le lancement est en cours » et
    /// la clé pour le libérer.
    pub fn block_next_launch(&self) -> (Receiver<()>, Sender<()>) {
        let (entered_tx, entered_rx) = channel();
        let (release_tx, release_rx) = channel();
        self.state().gate = Some((entered_tx, release_rx));
        (entered_rx, release_tx)
    }

    // --- Corbeille ---

    /// La corbeille répond ainsi pour ce chemin.
    pub fn fail_recycle(&self, path: &str, how: FakeRecycle) {
        self.state().recycle_failures.insert(norm(path), how);
    }

    /// Les chemins envoyés à la corbeille, dans l'ordre.
    pub fn recycled(&self) -> Vec<String> {
        self.state().recycled.clone()
    }

    // --- Formes longues ---

    /// La forme longue de ce chemin (`None` : la machine n'y arrive pas). Sans
    /// table, un chemin propre est rendu tel quel, tout autre n'y arrive pas.
    pub fn set_long_path(&self, path: &str, long: Option<&str>) {
        self.state().long_paths.insert(norm(path), long.map(str::to_owned));
    }

    // --- Horloge ---

    /// Avance l'horloge.
    pub fn advance(&self, duration: Duration) {
        let mut state = self.state();
        state.uptime += duration;
    }
}

impl AppsHost for FakeAppsHost {
    fn registrations(&self, place: Place) -> io::Result<RawSource> {
        let state = self.state();
        if state.unreadable_places.contains(&place) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "place illisible"));
        }
        Ok(RawSource {
            entries: state.registrations.get(&place).cloned().unwrap_or_default(),
            complete: !state.incomplete_places.contains(&place),
        })
    }

    fn registration(&self, source: Source, key: &str) -> io::Result<Option<RawRegistration>> {
        let mut state = self.state();
        state.registration_reads += 1;
        if state.unreadable_keys.contains(&(source, key.to_lowercase()))
            || state.unreadable_places.contains(&source.place())
        {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "lecture refusée"));
        }
        Ok(state
            .registrations
            .get(&source.place())
            .and_then(|list| list.iter().find(|raw| raw.key.eq_ignore_ascii_case(key)))
            .cloned())
    }

    fn expand(&self, text: &str) -> String {
        let state = self.state();
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find('%') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            match after.find('%') {
                Some(end) => match state.variables.get(&after[..end].to_uppercase()) {
                    Some(value) => {
                        out.push_str(value);
                        rest = &after[end + 1..];
                    }
                    None => {
                        out.push('%');
                        rest = after;
                    }
                },
                None => {
                    out.push('%');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }

    fn is_local_fixed(&self, path: &str) -> bool {
        let letter = path.chars().next().map(|letter| letter.to_ascii_uppercase());
        crate::apps::paths::is_absolute(path)
            && !letter.is_some_and(|letter| self.state().remote_drives.contains(&letter))
    }

    fn existence(&self, path: &str) -> Existence {
        let state = self.state();
        let key = norm(path);
        if state.denied.contains(&key) {
            return Existence::Unsure;
        }
        match state.nodes.get(&key) {
            Some((_, Node::File(_))) => Existence::File,
            Some((_, Node::Dir)) => Existence::Folder,
            Some((_, Node::Link)) => Existence::Unsure,
            None => Existence::Absent,
        }
    }

    fn system_dirs(&self) -> Option<SystemDirs> {
        Some(SystemDirs {
            system: r"C:\Windows\System32".to_owned(),
            windows: r"C:\Windows".to_owned(),
        })
    }

    fn launch(&self, program: &str, parameters: &str, elevated: bool, watch: bool) -> Launch {
        let (plan, gate) = {
            let mut state = self.state();
            state.launches.push(Launched {
                program: program.to_owned(),
                parameters: parameters.to_owned(),
                elevated,
                watched: watch,
            });
            (state.launch_plan, state.gate.take())
        };
        if let Some((entered, release)) = gate {
            let _ = entered.send(());
            let _ = release.recv();
        }
        match plan {
            LaunchPlan::Started => Launch::Started(
                watch.then(|| Box::new(FakeWatch(Arc::clone(&self.ended))) as Box<dyn Watch>),
            ),
            LaunchPlan::Cancelled => Launch::Cancelled,
            LaunchPlan::Missing => Launch::Missing,
            LaunchPlan::Failed(code) => Launch::Failed(code),
        }
    }

    fn account_folder(&self, folder: AccountFolder) -> Option<String> {
        self.state().folders.get(&folder).cloned()
    }

    fn plain_chain(&self, path: &str) -> bool {
        let state = self.state();
        let full = norm(path);
        let mut current = String::new();
        for (index, part) in full.split('\\').enumerate() {
            if index > 0 {
                current.push('\\');
            }
            current.push_str(part);
            // Seul compte ce qui existe : un lien ou un fichier au milieu du
            // chemin ne se traverse pas.
            if matches!(state.nodes.get(&current), Some((_, Node::Link | Node::File(_)))) {
                return false;
            }
        }
        true
    }

    fn children(&self, path: &str) -> io::Result<Vec<Child>> {
        let state = self.state();
        let key = norm(path);
        if state.unreadable_dirs.contains(&key) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "dossier illisible"));
        }
        match state.nodes.get(&key) {
            Some((_, Node::Dir)) => {}
            Some(_) => return Err(io::Error::new(io::ErrorKind::NotADirectory, "pas un dossier")),
            None => return Err(io::Error::new(io::ErrorKind::NotFound, "introuvable")),
        }
        Ok(state
            .nodes
            .iter()
            .filter(|(name, _)| is_inside(name, &key) && !name[key.len() + 1..].contains('\\'))
            .map(|(_, (original, node))| Child {
                name: original.rsplit('\\').next().unwrap_or(original).to_owned(),
                is_dir: *node == Node::Dir,
                is_link: *node == Node::Link,
            })
            .collect())
    }

    fn shortcuts(&self, root: &str) -> io::Result<Vec<Shortcut>> {
        let state = self.state();
        let key = norm(root);
        if state.unreadable_shortcuts.contains(&key) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "dossier illisible"));
        }
        Ok(state
            .shortcuts
            .iter()
            .filter(|shortcut| {
                state.nodes.contains_key(&norm(&shortcut.path))
                    && is_inside(&shortcut.path, root)
                    && norm(&shortcut.path)[key.len() + 1..].split('\\').count() <= 3
            })
            .cloned()
            .collect())
    }

    fn measure(&self, path: &str) -> Option<Measure> {
        let state = self.state();
        let key = norm(path);
        if state.unmeasurable.contains(&key) {
            return None;
        }
        match state.nodes.get(&key)?.1 {
            Node::File(bytes) => Some(Measure { bytes, files: 1, has_link: false }),
            Node::Link => None,
            Node::Dir => {
                let mut total = Measure { bytes: 0, files: 0, has_link: false };
                for (name, (_, node)) in &state.nodes {
                    if !is_inside(name, &key) {
                        continue;
                    }
                    match node {
                        Node::File(bytes) => {
                            total.bytes += bytes;
                            total.files += 1;
                        }
                        Node::Link => total.has_link = true,
                        Node::Dir => {}
                    }
                }
                Some(total)
            }
        }
    }

    fn recycle(&self, path: &str) -> Result<(), RecycleError> {
        let mut state = self.state();
        let key = norm(path);
        if !state.nodes.contains_key(&key) {
            return Err(RecycleError::Failed("introuvable".to_owned()));
        }
        match state.recycle_failures.get(&key) {
            Some(FakeRecycle::Locked) => return Err(RecycleError::Locked),
            Some(FakeRecycle::Refused) => return Err(RecycleError::Refused),
            Some(FakeRecycle::Denied) => return Err(RecycleError::AccessDenied),
            Some(FakeRecycle::Other) => return Err(RecycleError::Failed("autre".to_owned())),
            Some(FakeRecycle::LeavesSome) => {
                state.recycled.push(path.to_owned());
                return Ok(());
            }
            None => {}
        }
        state.nodes.retain(|name, _| *name != key && !is_inside(name, &key));
        state.recycled.push(path.to_owned());
        Ok(())
    }

    fn uptime(&self) -> Duration {
        self.state().uptime
    }

    fn own_dir(&self) -> Option<String> {
        self.state().own_dir.clone()
    }

    fn root_allowed(&self, path: &str) -> bool {
        let state = self.state();
        let key = norm(path);
        if key.len() <= 3 {
            return false;
        }
        let hits = |list: &Vec<String>, inside_counts: bool| {
            list.iter().any(|protected| {
                key == *protected
                    || (inside_counts && is_inside(&key, protected))
                    // Un dossier qui en contient un de protégé est trop large.
                    || is_inside(protected, &key)
            })
        };
        !(hits(&state.itself, false) || hits(&state.subtrees, true))
    }

    fn in_sync_root(&self, path: &str) -> bool {
        self.state().sync_roots.iter().any(|root| is_same_or_inside(path, root))
    }

    fn long_path(&self, path: &str) -> Option<String> {
        if let Some(known) = self.state().long_paths.get(&norm(path)) {
            return known.clone();
        }
        crate::apps::paths::is_plain(path).then(|| path.trim_end_matches(['\\', '/']).to_owned())
    }
}
