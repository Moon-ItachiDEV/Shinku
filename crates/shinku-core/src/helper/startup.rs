//! Ce que fait l'assistant élevé pour une demande de démarrage, dans l'ordre.
//!
//! La suite est écrite contre le trait [`MachineStartup`] : « ai-je les
//! droits », « mon emplacement est-il sûr », les noms d'une place, l'approbation,
//! l'heure, la question. L'hôte de Windows est dans `platform/windows/` et
//! **n'emprunte rien** à [`crate::startup::host::StartupHost`] (qui développe
//! des variables et sonde des fichiers). Aucune méthode du trait ne touche au
//! compte (`HKCU`) : les seules places sont celles du PC ([`StartupSlot`]).
//!
//! Codes de sortie ([`Code`]) : 0 fait, 2 demande refusée, 3 annulé (ou
//! échéance), 4 entrée introuvable (ou ambiguë), 5 pas de droits, 6 emplacement
//! non sûr, 7 réglage illisible, 8 échec, 9 déjà dans cet état. Le code 1 n'est
//! jamais rendu : c'est celui d'un processus tué (`taskkill /F`), et le guichet
//! le range avec les codes inconnus.
//!
//! L'assistant n'écrit rien d'autre que la valeur d'approbation : ni journal,
//! ni fichier, ni autre clé.

use std::ffi::OsString;
use std::io;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use crate::helper::request::{StartupAction, StartupRequest, StartupSlot};
use crate::startup::State;
use crate::startup::approval;
use crate::startup::host::RawApproval;
use crate::startup::shown::shown_name;

/// Le temps laissé à la personne pour répondre, à compter de l'ouverture de la boîte.
pub const QUESTION_DEADLINE: Duration = Duration::from_secs(120);

/// Les codes de sortie de l'assistant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Code {
    /// Fait.
    Done = 0,
    /// Demande refusée.
    Refused = 2,
    /// Annulé, ou échéance passée.
    Cancelled = 3,
    /// Entrée introuvable.
    NotFound = 4,
    /// Pas de droits d'administrateur dans le jeton.
    NoRights = 5,
    /// Emplacement non sûr.
    UnsafeLocation = 6,
    /// Réglage illisible.
    Unreadable = 7,
    /// Échec.
    Failed = 8,
    /// Déjà dans cet état : rien n'est écrit. (Pas 1 : voir l'en-tête.)
    Already = 9,
}

/// Les noms d'une place, tels que l'hôte les énumère.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Names {
    /// Les noms qui sont des entrées (UTF-16 valide ; un nom qui ne l'est pas
    /// ne peut pas égaler celui d'une demande, l'hôte peut l'omettre).
    pub names: Vec<String>,
    /// Faux si une borne a laissé quelque chose de côté. N'empêche rien si le
    /// nom est trouvé.
    pub complete: bool,
}

/// Pourquoi les noms d'une place ne sont pas rendus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamesError {
    /// Le dossier Démarrage commun n'est pas sûr (propriétaire ou droits).
    UnsafeFolder,
    /// La place ne se lit pas.
    Unreadable,
}

/// La machine, vue par l'assistant.
pub trait MachineStartup {
    /// Le groupe Administrateurs est-il **activé** dans mon jeton ?
    fn has_rights(&self) -> bool;

    /// Mon emplacement est-il sûr ?
    fn location_safe(&self) -> bool;

    /// Les noms d'une place, avec les règles du lot 1 pour dire ce qui est une
    /// entrée. Le dossier commun passe d'abord par la décision « objet sûr ».
    fn names(&self, place: StartupSlot) -> Result<Names, NamesError>;

    /// La valeur d'approbation d'une entrée (`None` : absente).
    fn approval(&self, place: StartupSlot, name: &str) -> io::Result<Option<RawApproval>>;

    /// Écrit la valeur d'approbation (crée la clé si elle manque).
    fn set_approval(&self, place: StartupSlot, name: &str, bytes: [u8; 12]) -> io::Result<()>;

    /// `FILETIME` de maintenant.
    fn file_time_now(&self) -> u64;

    /// Pose la question à la personne ; `true` si elle accepte. L'hôte applique
    /// l'échéance ([`Deadline`]) : passée, il sort avec le code 3 sans rien écrire.
    fn ask(&self, text: &str) -> bool;
}

/// L'état de la question, **atomique** : posée, réponse prise, échéance passée.
/// De la réponse et de la minuterie, celle qui change l'état la première gagne.
#[derive(Debug)]
pub struct Deadline {
    state: AtomicU8,
}

const ASKED: u8 = 0;
const ANSWERED: u8 = 1;
const EXPIRED: u8 = 2;

impl Default for Deadline {
    fn default() -> Self {
        Self::new()
    }
}

impl Deadline {
    /// Une question posée.
    pub const fn new() -> Self {
        Self { state: AtomicU8::new(ASKED) }
    }

    fn change(&self, to: u8) -> bool {
        self.state.compare_exchange(ASKED, to, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }

    /// La réponse arrive : vrai si elle gagne (l'échéance n'est pas passée).
    pub fn answer(&self) -> bool {
        self.change(ANSWERED)
    }

    /// La minuterie sonne : vrai si elle gagne (rien n'a été répondu).
    pub fn expire(&self) -> bool {
        self.change(EXPIRED)
    }

    /// Vrai si l'échéance est passée.
    pub fn expired(&self) -> bool {
        self.state.load(Ordering::SeqCst) == EXPIRED
    }
}

/// Le lieu, en texte fixe compilé.
const fn place_text(place: StartupSlot) -> &'static str {
    match place {
        StartupSlot::Run => "registre de Windows, Run (64 bits)",
        StartupSlot::Run32 => "registre de Windows, Run (32 bits)",
        StartupSlot::Folder => "dossier Démarrage commun",
    }
}

/// Le texte de la boîte. Le nom passe par la même fonction d'affichage que la
/// page ; la place est un texte fixe.
pub fn question_text(action: StartupAction, place: StartupSlot, name: &str) -> String {
    let shown = shown_name(name);
    let (first, tail) = match action {
        StartupAction::Cut => (
            format!(
                "Couper « {} » au démarrage de Windows, pour tous les comptes de ce PC ?",
                shown.text
            ),
            "\n\nL'app n'est pas désinstallée. Tu pourras la remettre depuis Shinku ou depuis le Gestionnaire des tâches.",
        ),
        StartupAction::Restore => (
            format!(
                "Remettre « {} » au démarrage de Windows, pour tous les comptes de ce PC ?",
                shown.text
            ),
            "",
        ),
    };
    let mut text = format!("{first}\n\nOù elle est inscrite : {}.{tail}", place_text(place));
    if shown.altered {
        text.push_str(
            "\n\nLe nom contient des caractères particuliers, montrés entre ⟨ ⟩, ou il est trop long et raccourci.",
        );
    }
    text.push_str(
        "\n\nSi tu n'as pas touché à cet interrupteur dans Shinku à l'instant, choisis Annuler.",
    );
    text
}

/// Cherche l'entrée par son nom : exactement un nom égal (égalité exacte des
/// unités UTF-16), sinon introuvable.
fn find(host: &dyn MachineStartup, request: &StartupRequest) -> Result<(), Code> {
    let names = host.names(request.place).map_err(|error| match error {
        NamesError::UnsafeFolder => Code::UnsafeLocation,
        NamesError::Unreadable => Code::Failed,
    })?;
    let equal = names
        .names
        .iter()
        .filter(|name| name.encode_utf16().eq(request.name.encode_utf16()))
        .count();
    if equal != 1 {
        return Err(Code::NotFound);
    }
    // Un autre nom qui ne diffère que par la casse : la valeur d'approbation est
    // un nom de valeur de registre, qui ne tient pas compte de la casse. Couper
    // l'un couperait l'autre sans que la boîte le dise. Comparaison large exprès.
    let wanted = &request.name;
    let confusable = names.names.iter().any(|name| {
        name != wanted
            && (name.to_lowercase() == wanted.to_lowercase()
                || name.to_uppercase() == wanted.to_uppercase())
    });
    if confusable { Err(Code::NotFound) } else { Ok(()) }
}

/// Ce que vaut la réponse de la boîte : `None` si l'échéance a gagné (la
/// minuterie fait sortir le processus, la réponse tardive ne vaut rien),
/// sinon `Some(accepté)`. C'est ici, et nulle part ailleurs, que l'hôte décide.
pub fn settle(deadline: &Deadline, accepted: bool) -> Option<bool> {
    deadline.answer().then_some(accepted)
}

/// L'état actuel, lu strictement ; `Err` si illisible ou déjà dans l'état voulu.
fn current(host: &dyn MachineStartup, request: &StartupRequest) -> Result<(), Code> {
    let read = approval::read(&host.approval(request.place, &request.name), host.file_time_now());
    let wanted = match request.action {
        StartupAction::Cut => State::Disabled,
        StartupAction::Restore => State::Enabled,
    };
    match read.state {
        State::Unknown => Err(Code::Unreadable),
        state if state == wanted => Err(Code::Already),
        _ => Ok(()),
    }
}

/// La suite complète, étapes 1 à 7. Rend le code de sortie.
pub fn run(host: &dyn MachineStartup, arguments: &[OsString]) -> u8 {
    execute(host, arguments) as u8
}

fn execute(host: &dyn MachineStartup, arguments: &[OsString]) -> Code {
    // 1. La demande.
    let Some(request) = StartupRequest::parse(arguments) else { return Code::Refused };
    // 2. Les droits.
    if !host.has_rights() {
        return Code::NoRights;
    }
    // 3. L'emplacement.
    if !host.location_safe() {
        return Code::UnsafeLocation;
    }
    // 4 et 5. L'entrée, son réglage.
    if let Err(code) = find(host, &request).and_then(|()| current(host, &request)) {
        return code;
    }
    // 6. La question.
    if !host.ask(&question_text(request.action, request.place, &request.name)) {
        return Code::Cancelled;
    }
    // 7. Les mêmes vérifications, puis l'écriture, puis la relecture.
    if let Err(code) = find(host, &request).and_then(|()| current(host, &request)) {
        return code;
    }
    let bytes = match request.action {
        StartupAction::Cut => approval::cut_bytes(host.file_time_now()),
        StartupAction::Restore => approval::restore_bytes(),
    };
    if host.set_approval(request.place, &request.name, bytes).is_err() {
        return Code::Failed;
    }
    match host.approval(request.place, &request.name) {
        Ok(Some(RawApproval { is_binary: true, bytes: read })) if read == bytes => Code::Done,
        _ => Code::Failed,
    }
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;
