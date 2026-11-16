//! Le lanceur de l'assistant élevé, vu du cœur : un trait, un hôte inerte, et un
//! faux pour les essais du guichet.
//!
//! `shinku.exe` ne s'élève jamais lui-même : il lance `shinku-helper.exe` par
//! `ShellExecuteExW` (verbe `runas`) avec la demande sur la ligne de commande,
//! attend sa fin, puis **relit lui-même le registre**. Le code de sortie ne sert
//! qu'à choisir le message.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use crate::startup::access::PcAccess;

/// Ce qui s'est passé quand on a lancé l'assistant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelperRun {
    /// Il est sorti avec ce code.
    Exited(u32),
    /// Windows a dit que la personne a refusé (`ERROR_CANCELLED`).
    Cancelled,
    /// Il n'a pas fini dans le délai : on ne sait pas.
    TimedOut,
    /// Il n'a pas pu être lancé.
    NotStarted,
}

/// L'assistant élevé, vu de `shinku.exe`.
pub trait ElevatedHelper: Send + Sync + fmt::Debug {
    /// Qui peut régler les entrées du PC, et comment.
    fn access(&self) -> PcAccess;

    /// Le dossier Démarrage commun est-il sûr (propriétaire et droits) ?
    fn folder_safe(&self) -> bool;

    /// Lance l'assistant avec ces quatre mots et attend sa fin. `owner` est la
    /// fenêtre de Shinku (0 : aucune), pour que l'accord de Windows s'ouvre
    /// devant elle. Ne tient aucun verrou du guichet.
    fn run(&self, words: &[String; 4], owner: isize) -> HelperRun;
}

/// Un lanceur qui ne fait rien : aucune entrée du PC ne se règle.
#[derive(Clone, Copy, Debug, Default)]
pub struct InertHelper;

impl ElevatedHelper for InertHelper {
    fn access(&self) -> PcAccess {
        PcAccess::Unavailable
    }

    fn folder_safe(&self) -> bool {
        false
    }

    fn run(&self, _words: &[String; 4], _owner: isize) -> HelperRun {
        HelperRun::NotStarted
    }
}

type Effect = Arc<dyn Fn(&[String; 4]) -> HelperRun + Send + Sync>;

/// Un lanceur fabriqué : il note chaque demande, et rend ce qu'on lui dit (par
/// défaut, `Exited(0)` sans rien changer).
pub struct FakeHelper {
    access: Mutex<PcAccess>,
    folder_safe: Mutex<bool>,
    effect: Mutex<Option<Effect>>,
    runs: Mutex<Vec<([String; 4], isize)>>,
}

impl fmt::Debug for FakeHelper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FakeHelper")
    }
}

impl FakeHelper {
    /// Un lanceur qui dit `consent`, dossier commun sûr.
    pub fn new() -> Self {
        Self {
            access: Mutex::new(PcAccess::Consent),
            folder_safe: Mutex::new(true),
            effect: Mutex::new(None),
            runs: Mutex::new(Vec::new()),
        }
    }

    /// Change l'accès annoncé.
    pub fn set_access(&self, access: PcAccess) {
        *self.access.lock().unwrap_or_else(PoisonError::into_inner) = access;
    }

    /// Dit si le dossier commun est sûr.
    pub fn set_folder_safe(&self, safe: bool) {
        *self.folder_safe.lock().unwrap_or_else(PoisonError::into_inner) = safe;
    }

    /// Ce qui se passe à chaque lancement (par exemple : écrire dans la machine
    /// fabriquée, attendre un signal, rendre un code).
    pub fn on_run(&self, effect: impl Fn(&[String; 4]) -> HelperRun + Send + Sync + 'static) {
        *self.effect.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(effect));
    }

    /// Les lancements faits, dans l'ordre : les quatre mots et la fenêtre.
    pub fn runs(&self) -> Vec<([String; 4], isize)> {
        self.runs.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Default for FakeHelper {
    fn default() -> Self {
        Self::new()
    }
}

impl ElevatedHelper for FakeHelper {
    fn access(&self) -> PcAccess {
        *self.access.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn folder_safe(&self) -> bool {
        *self.folder_safe.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn run(&self, words: &[String; 4], owner: isize) -> HelperRun {
        self.runs.lock().unwrap_or_else(PoisonError::into_inner).push((words.clone(), owner));
        // Le verrou de l'effet n'est pas tenu pendant qu'il tourne : un effet
        // qui attend ne bloque rien d'autre.
        let effect = self.effect.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match effect {
            Some(effect) => effect(words),
            None => HelperRun::Exited(0),
        }
    }
}
