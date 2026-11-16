//! `shinku.exe --oublier` : effacer les réglages de Shinku, avec les droits
//! ordinaires de la personne devant le bureau.
//!
//! Seul le désinstalleur l'appelle, quand la personne a coché « Supprimer aussi
//! mes réglages », **après** avoir fermé la fenêtre et l'agent. Le mot de ligne
//! de commande ne sait rien de la fenêtre ni de l'agent : s'ils tournent encore,
//! il efface ce qu'il peut (un fichier qu'ils tiennent ouvert est « occupé », et
//! reste après les dix secondes d'essais).
//!
//! # Ce qu'il fait, dans l'ordre
//!
//! 1. refuse s'il est élevé par rapport au bureau ([`Outcome::Refused`]) : il
//!    ne touche à rien, il ne regarde même pas les dossiers ;
//! 2. efface les fichiers de la liste fermée ([`list`]), chacun par les règles de
//!    [`crate::fs`] ;
//! 3. retire les deux dossiers `Shinku`, **seulement s'ils sont vides** ;
//! 4. supprime les deux arborescences du modèle de Tauri,
//!    `%APPDATA%\fr.shinku.app` et `%LOCALAPPDATA%\fr.shinku.app` (le cache de
//!    WebView2), sans jamais traverser un lien ([`crate::fs::remove_tree`]) ;
//! 5. dans l'échéance totale ([`TOTAL_DEADLINE`], vingt secondes), si quelque chose était occupé, recommence le tout, de court en court,
//!    pendant dix secondes **en tout** ([`RETRY_BUDGET`]) ;
//! 6. ne se termine pas avant une seconde ([`MIN_LIFETIME`]).
//!
//! # La seconde de vie minimale
//!
//! C'est un garde-fou d'attente, pas une fonction. Le désinstalleur ne reçoit du
//! processus ni poignée ni code de sortie (`RunAsUser` n'en rend pas) : il ne le
//! voit qu'en interrogeant Windows sur le chemin de `shinku.exe` (le Gestionnaire
//! de redémarrage ; voir la conception). Un processus qui n'aurait rien à faire disparaîtrait avant
//! que le désinstalleur ait pu le voir exister, et il croirait qu'il ne s'est pas
//! lancé. Le processus dort donc le reste de la seconde, même quand il refuse.
//!
//! # Décisions et machine
//!
//! Tout ce qui décide (la liste, l'ordre, le réessai, le code) est ici, pur :
//! [`run`] ne parle à la machine que par [`ForgetHost`] (élévation, dossiers,
//! horloge, attente, suppression). Les essais lui donnent un faux hôte qui ne
//! dort pas ; la vraie machine est [`crate::platform::forget_host`]. Rien n'est
//! écrit sur le disque, rien n'est affiché.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::fs::{Removed, TreeReport};
use crate::platform::KnownFolder;

pub mod list;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

/// L'unique mot de la ligne de commande.
pub const COMMAND: &str = "--oublier";

/// Le nom des dossiers que le modèle de Tauri crée dans `%APPDATA%` et
/// `%LOCALAPPDATA%` : l'`identifier` de `tauri.conf.json`. Un essai lit ce
/// fichier et échoue si les deux divergent : le nom n'a qu'une source.
pub const TAURI_IDENTIFIER: &str = "fr.shinku.app";

/// Tout est parti, ou il n'y avait rien.
pub const EXIT_DONE: u8 = 0;
/// Il reste quelque chose.
pub const EXIT_LEFT: u8 = 1;
/// Refusé : le processus est élevé par rapport au bureau.
pub const EXIT_REFUSED: u8 = 2;

/// Le temps minimal que le processus vit, pour que le désinstalleur puisse le
/// voir (voir l'en-tête du module).
pub const MIN_LIFETIME: Duration = Duration::from_secs(1);
/// Le temps qu'on laisse, en tout, aux fichiers occupés pour se libérer.
pub const RETRY_BUDGET: Duration = Duration::from_secs(10);
/// L'échéance de **tout** le passage, réessais compris : le désinstalleur attend
/// trente secondes au plus avant d'arrêter le processus. Elle est regardée entre
/// deux gestes, et dans l'arborescence entre deux entrées ; un geste commencé
/// (une suppression, la lecture d'un dossier) n'est pas interrompu.
pub const TOTAL_DEADLINE: Duration = Duration::from_secs(20);
/// La pause entre deux passes quand quelque chose est occupé.
pub const RETRY_PAUSE: Duration = Duration::from_millis(250);

/// Ce que la ligne de commande demande : vrai seulement si, après le nom du
/// programme, il y a **un seul** argument et que c'est exactement `--oublier`.
/// Toute autre forme (un mot de plus, une autre casse, `--oublier=1`) démarre
/// Shinku comme d'habitude.
pub fn asked<I>(arguments: I) -> bool
where
    I: IntoIterator,
    I::Item: AsRef<OsStr>,
{
    let mut arguments = arguments.into_iter().skip(1);
    matches!((arguments.next(), arguments.next()), (Some(only), None) if only.as_ref() == COMMAND)
}

/// Le résultat de `--oublier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Tout est parti, ou il n'y avait rien.
    Done,
    /// Il reste quelque chose : un fichier occupé au bout des dix secondes, un
    /// lien laissé en place, un fichier qui n'est pas dans la liste, un droit
    /// refusé.
    Left,
    /// Refusé parce que le processus est élevé par rapport au bureau.
    Refused,
}

impl Outcome {
    /// Le code de sortie du processus.
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Done => EXIT_DONE,
            Self::Left => EXIT_LEFT,
            Self::Refused => EXIT_REFUSED,
        }
    }
}

/// Ce que `--oublier` demande à la machine.
pub trait ForgetHost {
    /// Vrai si ce processus est élevé par rapport au bureau (la décision de
    /// [`crate::platform::runs_elevated`], la même que pour la fenêtre).
    fn elevated(&self) -> bool;

    /// Un dossier connu, s'il existe sur cette machine.
    fn folder(&self, folder: KnownFolder) -> Option<PathBuf>;

    /// Le temps écoulé depuis un instant d'origine quelconque, qui ne recule
    /// jamais.
    fn elapsed(&self) -> Duration;

    /// Attend. Ni l'horloge ni l'attente ne sont celles du système dans les
    /// essais : ils ne dorment pas.
    fn sleep(&self, duration: Duration);

    /// Supprime le fichier `name` de `dir` (voir [`crate::fs::remove_plain_file`]).
    fn remove_file(&self, dir: &Path, name: &OsStr) -> Removed;

    /// Supprime `dir` s'il est vide (voir [`crate::fs::remove_empty_dir`]).
    fn remove_empty_dir(&self, dir: &Path) -> Removed;

    /// Supprime l'arborescence de `root` (voir [`crate::fs::remove_tree`]), en
    /// s'arrêtant quand l'horloge de l'hôte ([`ForgetHost::elapsed`]) atteint
    /// `deadline`.
    fn remove_tree(&self, root: &Path, deadline: Duration) -> TreeReport;
}

/// Ce qu'une passe a vu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Pass {
    /// Quelque chose est occupé : une autre passe peut réussir.
    busy: bool,
    /// Quelque chose reste, qu'une autre passe ne changera pas.
    kept: bool,
}

impl Pass {
    fn file(&mut self, outcome: Removed) {
        match outcome {
            Removed::Gone | Removed::Missing => {}
            Removed::Busy => self.busy = true,
            Removed::Kept => self.kept = true,
        }
    }

    fn tree(&mut self, report: TreeReport) {
        self.busy |= report.busy > 0;
        self.kept |= report.kept > 0;
    }
}

/// Un dossier de réglages, tel que la liste fermée le décrit, résolu.
struct Resolved {
    dir: PathBuf,
    files: Vec<OsString>,
}

/// Une passe : les fichiers, puis les dossiers vides, puis les deux
/// arborescences.
fn pass(
    host: &dyn ForgetHost,
    settings: &[Resolved],
    trees: &[PathBuf],
    deadline: Duration,
) -> Pass {
    let mut seen = Pass::default();
    // Passée l'échéance, on ne commence plus rien : ce qui reste est « gardé ».
    let late = |seen: &mut Pass| {
        let late = host.elapsed() >= deadline;
        seen.kept |= late;
        late
    };
    for place in settings {
        for name in &place.files {
            if late(&mut seen) {
                return seen;
            }
            seen.file(host.remove_file(&place.dir, name));
        }
    }
    for place in settings {
        if late(&mut seen) {
            return seen;
        }
        seen.file(host.remove_empty_dir(&place.dir));
    }
    for root in trees {
        if late(&mut seen) {
            return seen;
        }
        seen.tree(host.remove_tree(root, deadline));
    }
    seen
}

/// Fait ce que `--oublier` fait, et rend le résultat. Le code de sortie est
/// [`Outcome::exit_code`].
pub fn run(host: &dyn ForgetHost) -> Outcome {
    let started = host.elapsed();
    let outcome = forget(host);
    // La seconde de vie minimale, même quand on refuse ou qu'il n'y avait rien.
    let lived = host.elapsed().saturating_sub(started);
    if lived < MIN_LIFETIME {
        host.sleep(MIN_LIFETIME - lived);
    }
    outcome
}

fn forget(host: &dyn ForgetHost) -> Outcome {
    // Avant tout : élevé, on ne touche à rien (pas même un dossier à regarder).
    if host.elevated() {
        return Outcome::Refused;
    }
    let started = host.elapsed();
    let deadline = started + TOTAL_DEADLINE;
    // Un dossier connu que l'hôte ne sait pas donner : on n'a rien regardé de ce
    // côté, et on ne dit pas « tout est parti ».
    let mut unresolved = false;
    let mut settings: Vec<Resolved> = Vec::new();
    for group in list::closed_list() {
        match host.folder(group.folder) {
            Some(base) => settings.push(Resolved { dir: base.join(group.dir), files: group.files }),
            None => unresolved = true,
        }
    }
    let mut trees: Vec<PathBuf> = Vec::new();
    for folder in [KnownFolder::RoamingAppData, KnownFolder::LocalAppData] {
        match host.folder(folder) {
            Some(base) => trees.push(base.join(TAURI_IDENTIFIER)),
            None => unresolved = true,
        }
    }

    loop {
        let seen = pass(host, &settings, &trees, deadline);
        let spent = host.elapsed().saturating_sub(started);
        if !seen.busy {
            return if seen.kept || unresolved { Outcome::Left } else { Outcome::Done };
        }
        // Cinq secondes de réessai en tout, pas par fichier : une seule horloge
        // pour la série de passes, à l'intérieur de l'échéance totale.
        if spent >= RETRY_BUDGET || spent >= TOTAL_DEADLINE {
            return Outcome::Left;
        }
        host.sleep(RETRY_PAUSE.min(RETRY_BUDGET - spent));
    }
}
