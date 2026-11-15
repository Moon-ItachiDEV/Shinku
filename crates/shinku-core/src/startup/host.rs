//! La machine, vue par le module « Démarrage ».
//!
//! Comme [`crate::game::host::GameHost`] pour le mode jeu, le trait
//! [`StartupHost`] est tout ce que le module sait de Windows : les entrées des
//! six places, les valeurs d'approbation, les variables, les fichiers et leurs
//! ressources de version. La vraie implémentation est dans
//! `platform/windows/startup.rs`. Les tests utilisent [`FakeStartupHost`], une
//! machine fabriquée de toutes pièces qui note chaque écriture.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::model::Timestamp;
use crate::startup::{Place, Scope};

/// Une entrée telle que la place la donne, avant toute interprétation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntry {
    /// Nom de la valeur, ou nom du fichier avec son extension.
    pub name: String,
    /// Faux si le nom n'était pas de l'UTF-16 valide (le nom ci-dessus est
    /// alors approximatif).
    pub name_exact: bool,
    /// `Run` : la commande telle qu'écrite au registre. Dossier : la cible du
    /// raccourci si elle a pu être lue, sinon le chemin du fichier lui-même
    /// (dans les deux cas entre guillemets : c'est un chemin, pas une ligne de
    /// commande).
    pub command: String,
}

impl RawEntry {
    /// Une entrée au nom exact.
    pub fn new(name: &str, command: &str) -> Self {
        Self { name: name.to_owned(), name_exact: true, command: command.to_owned() }
    }
}

/// Ce qu'une place contient, avec ce que l'hôte n'a pas pu lire.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawPlace {
    /// Les entrées lues.
    pub entries: Vec<RawEntry>,
    /// Faux si l'hôte a dû laisser quelque chose de côté : une borne atteinte
    /// (trop d'entrées, un nom trop long, une valeur démesurée). La liste est
    /// alors **incomplète**, et la page doit le dire.
    pub complete: bool,
}

impl RawPlace {
    /// Une place lue en entier.
    pub fn complete(entries: Vec<RawEntry>) -> Self {
        Self { entries, complete: true }
    }

    /// Une place où quelque chose a été laissé de côté.
    pub fn incomplete(entries: Vec<RawEntry>) -> Self {
        Self { entries, complete: false }
    }
}

/// Ce que la machine répond quand on lui demande ce qu'est un chemin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Existence {
    /// Un fichier.
    File,
    /// Un dossier : ce n'est pas un programme, mais ce n'est pas « absent ».
    Folder,
    /// Rien, avec certitude (seulement « introuvable »).
    Absent,
    /// Impossible de le dire : accès refusé, nom invalide, lien, lecteur qui
    /// ne répond pas, forme inconnue. Jamais pris pour « absent ».
    Unsure,
}

/// Une valeur d'approbation, telle que le registre la donne.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawApproval {
    /// Vrai si la valeur est de type `REG_BINARY`.
    pub is_binary: bool,
    /// Ses octets.
    pub bytes: Vec<u8>,
}

/// Ce que les ressources de version d'un fichier disent de lui.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileIdentity {
    /// `FileDescription`.
    pub description: Option<String>,
    /// `CompanyName`.
    pub company: Option<String>,
}

/// La machine, vue par le module « Démarrage ». Ne modifie qu'une chose : les
/// valeurs d'approbation du compte.
pub trait StartupHost: Send + Sync + fmt::Debug {
    /// L'heure murale, pour les dates qu'on écrit et qu'on affiche.
    fn now(&self) -> Timestamp;

    /// `FILETIME` de maintenant (100 ns depuis 1601), pour la valeur d'approbation.
    fn file_time_now(&self) -> u64;

    /// Les entrées brutes d'une place. Une place illisible rend une erreur : la
    /// page le dira. Une place qui n'existe pas est une place vide. Une place
    /// dont l'hôte a dû laisser quelque chose de côté le dit
    /// ([`RawPlace::complete`]).
    fn entries(&self, place: Place) -> io::Result<RawPlace>;

    /// La valeur d'approbation telle quelle (`None` : absente).
    fn approval(&self, place: Place, name: &str) -> io::Result<Option<RawApproval>>;

    /// Écrit l'approbation. Refuse (`PermissionDenied`) toute place du PC : ce
    /// lot n'y écrit pas.
    fn set_approval(&self, place: Place, name: &str, bytes: [u8; 12]) -> io::Result<()>;

    /// Développe `%VAR%`.
    fn expand(&self, text: &str) -> String;

    /// Vrai si ce chemin absolu est sur un lecteur **fixe** de cette machine.
    /// Ni un chemin réseau, ni un lecteur réseau, amovible ou absent : Shinku
    /// n'y cherche rien et n'y lit rien, car cela pourrait déclencher une
    /// connexion ou attendre un délai.
    fn is_local_fixed(&self, path: &str) -> bool;

    /// Ce qu'est ce chemin. À n'appeler que pour un chemin où
    /// [`is_local_fixed`](Self::is_local_fixed) est vrai ; sinon la réponse est
    /// [`Existence::Unsure`], sans aucun accès.
    fn existence(&self, path: &str) -> Existence;

    /// Vrai si cette machine sait compter les entrées de démarrage. Faux pour
    /// la machine inerte : un « 0 » y serait un chiffre inventé.
    fn measures(&self) -> bool {
        true
    }

    /// Description et éditeur lus dans les ressources de version du fichier.
    /// Rien sur un lecteur qui n'est pas fixe.
    fn identity(&self, path: &str) -> FileIdentity;

    /// Dossier de Windows (`GetSystemWindowsDirectoryW`, jamais
    /// l'environnement), sans barre finale.
    fn windows_dir(&self) -> String;
}

/// Une machine qui ne voit rien : aucune entrée, aucun fichier. C'est celle des
/// plateformes sans démarrage de Windows.
#[derive(Clone, Copy, Debug, Default)]
pub struct InertStartupHost;

impl StartupHost for InertStartupHost {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn file_time_now(&self) -> u64 {
        u64::try_from(self.now().ticks()).unwrap_or(0).saturating_add(FILETIME_OF_UNIX_EPOCH)
    }

    fn entries(&self, _place: Place) -> io::Result<RawPlace> {
        Ok(RawPlace::complete(Vec::new()))
    }

    fn approval(&self, _place: Place, _name: &str) -> io::Result<Option<RawApproval>> {
        Ok(None)
    }

    fn set_approval(&self, _place: Place, _name: &str, _bytes: [u8; 12]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "pas de démarrage de Windows ici"))
    }

    fn expand(&self, text: &str) -> String {
        text.to_owned()
    }

    fn is_local_fixed(&self, _path: &str) -> bool {
        false
    }

    fn existence(&self, _path: &str) -> Existence {
        Existence::Unsure
    }

    fn measures(&self) -> bool {
        false
    }

    fn identity(&self, _path: &str) -> FileIdentity {
        FileIdentity::default()
    }

    fn windows_dir(&self) -> String {
        String::new()
    }
}

/// Le `FILETIME` de 1970 : ce qu'il faut ajouter à un nombre de ticks depuis
/// 1970 pour avoir un `FILETIME`.
const FILETIME_OF_UNIX_EPOCH: u64 = 116_444_736_000_000_000;

/// Une écriture de Shinku sur la machine fabriquée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    /// La place.
    pub place: Place,
    /// Le nom de la valeur.
    pub name: String,
    /// Les douze octets écrits.
    pub bytes: [u8; 12],
}

#[derive(Debug)]
struct Fake {
    now: Timestamp,
    entries: HashMap<Place, Vec<RawEntry>>,
    unreadable: HashSet<Place>,
    approvals: HashMap<(Place, String), RawApproval>,
    unreadable_approvals: HashSet<(Place, String)>,
    incomplete: HashSet<Place>,
    files: HashSet<String>,
    folders: HashSet<String>,
    denied: HashSet<String>,
    remote_drives: HashSet<char>,
    identities: HashMap<String, FileIdentity>,
    variables: HashMap<String, String>,
    windows_dir: String,
    writes: Vec<Written>,
    write_refused: bool,
    write_ignored: bool,
    checked: Vec<String>,
}

/// Une machine fabriquée de toutes pièces : des entrées, des approbations, des
/// fichiers. Elle note chaque écriture et chaque chemin dont on a demandé s'il
/// est un fichier.
#[derive(Debug)]
pub struct FakeStartupHost {
    inner: Mutex<Fake>,
}

impl Default for FakeStartupHost {
    fn default() -> Self {
        Self::new()
    }
}

/// Le nom d'une valeur de registre se compare sans tenir compte de la casse.
fn fold(name: &str) -> String {
    name.to_lowercase()
}

impl FakeStartupHost {
    /// Une machine vide, le 15 novembre 2026, avec Windows dans `C:\Windows`.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Fake {
                now: Timestamp::from_unix(1_794_700_000, 0),
                entries: HashMap::new(),
                unreadable: HashSet::new(),
                approvals: HashMap::new(),
                unreadable_approvals: HashSet::new(),
                incomplete: HashSet::new(),
                files: HashSet::new(),
                folders: HashSet::new(),
                denied: HashSet::new(),
                remote_drives: HashSet::new(),
                identities: HashMap::new(),
                variables: HashMap::new(),
                windows_dir: r"C:\Windows".to_owned(),
                writes: Vec::new(),
                write_refused: false,
                write_ignored: false,
                checked: Vec::new(),
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, Fake> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ajoute une entrée à une place.
    pub fn add_entry(&self, place: Place, entry: RawEntry) {
        self.state().entries.entry(place).or_default().push(entry);
    }

    /// Ajoute une entrée au nom exact.
    pub fn add(&self, place: Place, name: &str, command: &str) {
        self.add_entry(place, RawEntry::new(name, command));
    }

    /// Retire une entrée.
    pub fn remove(&self, place: Place, name: &str) {
        if let Some(list) = self.state().entries.get_mut(&place) {
            list.retain(|entry| entry.name != name);
        }
    }

    /// Cette place ne se lit pas.
    pub fn make_unreadable(&self, place: Place) {
        self.state().unreadable.insert(place);
    }

    /// L'hôte a dû laisser quelque chose de côté dans cette place.
    pub fn make_incomplete(&self, place: Place) {
        self.state().incomplete.insert(place);
    }

    /// Pose une valeur d'approbation telle quelle.
    pub fn set_raw_approval(&self, place: Place, name: &str, approval: RawApproval) {
        self.state().approvals.insert((place, fold(name)), approval);
    }

    /// Pose une valeur d'approbation binaire.
    pub fn set_bytes(&self, place: Place, name: &str, bytes: &[u8]) {
        self.set_raw_approval(place, name, RawApproval { is_binary: true, bytes: bytes.to_vec() });
    }

    /// Efface une valeur d'approbation (l'app l'a effacée en se réinscrivant).
    pub fn clear_approval(&self, place: Place, name: &str) {
        self.state().approvals.remove(&(place, fold(name)));
    }

    /// La lecture de cette valeur d'approbation est refusée.
    pub fn make_approval_unreadable(&self, place: Place, name: &str) {
        self.state().unreadable_approvals.insert((place, fold(name)));
    }

    /// La valeur d'approbation telle qu'elle est maintenant.
    pub fn approval_of(&self, place: Place, name: &str) -> Option<RawApproval> {
        self.state().approvals.get(&(place, fold(name))).cloned()
    }

    /// Ce chemin est un fichier (comparé sans tenir compte de la casse).
    pub fn add_file(&self, path: &str) {
        self.state().files.insert(fold(path));
    }

    /// Ce chemin est un dossier.
    pub fn add_folder(&self, path: &str) {
        self.state().folders.insert(fold(path));
    }

    /// La machine ne sait pas dire ce qu'est ce chemin (accès refusé…).
    pub fn deny_access(&self, path: &str) {
        self.state().denied.insert(fold(path));
    }

    /// Ce lecteur (`'Z'`) n'est pas un lecteur fixe : réseau, amovible…
    pub fn add_remote_drive(&self, letter: char) {
        self.state().remote_drives.insert(letter.to_ascii_uppercase());
    }

    /// Ce fichier existe, et ses ressources de version le décrivent ainsi.
    pub fn add_program(&self, path: &str, description: Option<&str>, company: Option<&str>) {
        self.add_file(path);
        self.state().identities.insert(
            fold(path),
            FileIdentity {
                description: description.map(str::to_owned),
                company: company.map(str::to_owned),
            },
        );
    }

    /// Définit une variable d'environnement (`%NOM%`).
    pub fn set_variable(&self, name: &str, value: &str) {
        self.state().variables.insert(name.to_uppercase(), value.to_owned());
    }

    /// Change le dossier de Windows.
    pub fn set_windows_dir(&self, dir: &str) {
        self.state().windows_dir = dir.to_owned();
    }

    /// Change l'heure.
    pub fn set_now(&self, now: Timestamp) {
        self.state().now = now;
    }

    /// Avance l'heure.
    pub fn advance(&self, duration: std::time::Duration) {
        let mut state = self.state();
        let ticks = i64::try_from(duration.as_nanos() / 100).unwrap_or(i64::MAX);
        state.now = Timestamp::from_ticks(state.now.ticks().saturating_add(ticks));
    }

    /// Windows refuse toute écriture.
    pub fn refuse_writes(&self) {
        self.state().write_refused = true;
    }

    /// Windows dit oui, mais ne garde rien : la relecture ne donne pas ce qui a
    /// été écrit.
    pub fn ignore_writes(&self) {
        self.state().write_ignored = true;
    }

    /// Les écritures demandées, dans l'ordre.
    pub fn writes(&self) -> Vec<Written> {
        self.state().writes.clone()
    }

    /// Les chemins dont on a demandé s'ils étaient des fichiers, dans l'ordre.
    pub fn checked_paths(&self) -> Vec<String> {
        self.state().checked.clone()
    }
}

impl StartupHost for FakeStartupHost {
    fn now(&self) -> Timestamp {
        self.state().now
    }

    fn file_time_now(&self) -> u64 {
        let ticks = u64::try_from(self.state().now.ticks()).unwrap_or(0);
        ticks.saturating_add(FILETIME_OF_UNIX_EPOCH)
    }

    fn entries(&self, place: Place) -> io::Result<RawPlace> {
        let state = self.state();
        if state.unreadable.contains(&place) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "place illisible"));
        }
        Ok(RawPlace {
            entries: state.entries.get(&place).cloned().unwrap_or_default(),
            complete: !state.incomplete.contains(&place),
        })
    }

    fn approval(&self, place: Place, name: &str) -> io::Result<Option<RawApproval>> {
        let state = self.state();
        let key = (place, fold(name));
        if state.unreadable_approvals.contains(&key) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "lecture refusée"));
        }
        Ok(state.approvals.get(&key).cloned())
    }

    fn set_approval(&self, place: Place, name: &str, bytes: [u8; 12]) -> io::Result<()> {
        let mut state = self.state();
        state.writes.push(Written { place, name: name.to_owned(), bytes });
        if place.scope() == Scope::Machine {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "place du PC"));
        }
        if state.write_refused {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "écriture refusée"));
        }
        if !state.write_ignored {
            state.approvals.insert(
                (place, fold(name)),
                RawApproval { is_binary: true, bytes: bytes.to_vec() },
            );
        }
        Ok(())
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
                    // Variable inconnue : Windows la laisse telle quelle.
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
        !letter.is_some_and(|letter| self.state().remote_drives.contains(&letter))
    }

    fn existence(&self, path: &str) -> Existence {
        let mut state = self.state();
        state.checked.push(path.to_owned());
        let key = fold(path);
        if state.denied.contains(&key) {
            Existence::Unsure
        } else if state.files.contains(&key) {
            Existence::File
        } else if state.folders.contains(&key) {
            Existence::Folder
        } else {
            Existence::Absent
        }
    }

    fn identity(&self, path: &str) -> FileIdentity {
        self.state().identities.get(&fold(path)).cloned().unwrap_or_default()
    }

    fn windows_dir(&self) -> String {
        self.state().windows_dir.clone()
    }
}
