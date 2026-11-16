//! L'étape « Doublons » du grand vide : les copies partent à la corbeille.
//!
//! Ce sont des fichiers personnels. L'étape ne les supprime jamais : elle les
//! confie à la corbeille de Windows, d'où ils se restaurent. Si la corbeille
//! ne peut pas recevoir un fichier, il reste à sa place.
//!
//! La liste vient du rapport de l'analyse, gardé dans le cœur. Dans chaque
//! groupe, un exemplaire et un seul y est marqué gardé (le plus ancien, ou
//! celui que la personne a choisi) : l'étape le laisse toujours en place, et
//! laisse entier un groupe qui n'en marquerait pas exactement un. Avant
//! d'envoyer une copie :
//!
//! - elle doit toujours se trouver là où Shinku cherche des doublons, hors de
//!   tout dossier exclu ou interdit, au bout d'un chemin sans aucun lien ;
//! - l'exemplaire gardé et la copie sont ouverts et revérifiés (taille, date,
//!   identifiant) : ce sont les fichiers que l'analyse a vus, et ce sont bien
//!   deux fichiers distincts ;
//! - leurs contenus sont comparés octet par octet. Une empreinte identique ne
//!   suffit pas pour se séparer d'un fichier personnel.
//!
//! Après l'envoi, l'étape ne croit pas Windows sur parole : elle regarde ce
//! que la copie est devenue, et si l'exemplaire gardé est toujours à sa
//! place. Au moindre doute, elle s'arrête et dit sur quel fichier.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::report::{DuplicatesStep, DuplicatesStop, Left, SkippedFiles, StepState, StopReason};
use super::{CleanControl, CleanPhase, CleanProgress};
use crate::fs::{
    self, Fingerprint, Identity, OpenError, PathState, VerifiedFile, inspect_dir_chain,
    same_file_id,
};
use crate::guard::Guard;
use crate::model::{Timestamp, Trigger};
use crate::platform::{Environment, RecycleError};
use crate::scan::plan::Plan;
use crate::scan::report::{DuplicateFile, DuplicateGroup, Report};
use crate::scan::{Modules, ScanOptions};

/// Taille des lectures pendant la comparaison.
const CHUNK: usize = 4 * 1024 * 1024;

/// Ce que l'étape a fait.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    pub step: DuplicatesStep,
    pub skipped: SkippedFiles,
    /// Taille des fichiers partis à la corbeille. Windows mesure sa corbeille
    /// ainsi, et non en place occupée sur le disque.
    pub sent_logical: u64,
}

/// Le plan de l'étape : là où l'analyse commencée à `analysis_started` a
/// cherché des doublons, et rien d'autre.
fn plan(environment: &dyn Environment, analysis_started: Timestamp, excluded: &[PathBuf]) -> Plan {
    let options = ScanOptions {
        excluded: excluded.to_vec(),
        modules: Modules { junk: false, duplicates: true, large_files: false, recycle_bin: false },
        background: false,
        ..ScanOptions::new(Trigger::Button)
    };
    Plan::build(environment, &options, analysis_started)
}

/// L'identité que l'analyse a relevée pour un exemplaire.
fn recorded_identity(file: &DuplicateFile) -> Option<Identity> {
    file.file_id.map(|file_id| Identity { volume: file.volume, file_id })
}

fn same_identity(left: Identity, right: Identity) -> bool {
    left.volume == right.volume && same_file_id(left.file_id, right.file_id)
}

fn fingerprint(group: &DuplicateGroup, file: &DuplicateFile) -> Fingerprint {
    Fingerprint { size: group.size, modified: file.modified, file_id: file.file_id }
}

/// Où se trouve un fichier par rapport au chemin où l'analyse l'a vu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// Ce chemin le désigne toujours.
    Here,
    /// Ce chemin ne désigne plus rien, ou désigne un autre fichier.
    Elsewhere,
    /// Impossible de le savoir.
    Unknown,
}

fn place_of(path: &Path, identity: Identity) -> Place {
    match fs::identity(path) {
        Ok(Some(found)) if same_identity(found, identity) => Place::Here,
        Ok(Some(_)) => Place::Elsewhere,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Place::Elsewhere,
        Ok(None) | Err(_) => Place::Unknown,
    }
}

/// Range un refus d'ouverture. `None` : le fichier a disparu.
fn reason(error: &OpenError) -> Option<Left> {
    match error {
        OpenError::NotFound => None,
        OpenError::Locked => Some(Left::InUse),
        OpenError::Changed | OpenError::NotRegular => Some(Left::Changed),
        OpenError::AccessDenied => Some(Left::Denied),
        OpenError::OnlineOnly | OpenError::Io(_) => Some(Left::Other),
    }
}

/// Range un refus de la corbeille.
fn bin_reason(error: &RecycleError) -> Left {
    match error {
        RecycleError::Refused | RecycleError::Unavailable => Left::Unrecyclable,
        RecycleError::Locked => Left::InUse,
        RecycleError::AccessDenied => Left::Denied,
        RecycleError::Failed(_) => Left::Other,
    }
}

/// Résultat d'une comparaison de contenus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Compared {
    Same,
    Different,
    /// L'arrêt a été demandé avant la fin.
    Cancelled,
}

/// Remplit `buffer` en entier. Faux si le fichier s'arrête avant.
fn fill(file: &mut File, buffer: &mut [u8]) -> io::Result<bool> {
    match file.read_exact(buffer) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(error),
    }
}

/// Vrai s'il ne reste rien à lire.
fn at_end(file: &mut File) -> io::Result<bool> {
    let mut extra = [0u8; 1];
    loop {
        match file.read(&mut extra) {
            Ok(read) => return Ok(read == 0),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

/// Compare deux fichiers de `size` octets, du premier au dernier octet.
/// `on_read` reçoit le nombre d'octets comparés à chaque lecture.
fn same_content(
    kept: &mut File,
    copy: &mut File,
    size: u64,
    cancel: &AtomicBool,
    buffers: &mut (Vec<u8>, Vec<u8>),
    on_read: &mut dyn FnMut(u64),
) -> io::Result<Compared> {
    let (left, right) = buffers;
    kept.seek(SeekFrom::Start(0))?;
    copy.seek(SeekFrom::Start(0))?;
    let mut remaining = size;
    while remaining > 0 {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Compared::Cancelled);
        }
        let length = remaining.min(left.len() as u64) as usize;
        // Plus court que prévu : ce n'est plus le fichier de l'analyse.
        if !fill(kept, &mut left[..length])? || !fill(copy, &mut right[..length])? {
            return Ok(Compared::Different);
        }
        if left[..length] != right[..length] {
            return Ok(Compared::Different);
        }
        remaining -= length as u64;
        on_read(length as u64);
    }
    // Et aucun des deux ne doit continuer au-delà.
    Ok(if at_end(kept)? && at_end(copy)? { Compared::Same } else { Compared::Different })
}

/// Ce qu'une copie est devenue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sent {
    /// Elle est dans la corbeille. `kept_intact` : l'exemplaire gardé est
    /// toujours à sa place, tel que l'analyse l'a vu.
    Recycled { kept_intact: bool },
    /// Elle avait déjà disparu : il n'y avait plus rien à faire.
    Vanished,
    /// Elle est restée à sa place.
    Left(Left),
    /// L'exemplaire gardé n'est plus celui de l'analyse : tout le groupe reste.
    KeptChanged,
    /// L'arrêt a été demandé pendant la comparaison : elle est restée.
    Cancelled,
    /// Elle a quitté sa place et Shinku ne la retrouve pas.
    Lost,
}

/// Ce qu'il faut faire après un groupe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Continue,
    Halt,
}

struct Step<'a> {
    environment: &'a dyn Environment,
    plan: Plan,
    guard: Guard,
    control: &'a CleanControl,
    progress: &'a CleanProgress,
    /// Dossiers déjà contrôlés : vrai si l'on peut y prendre une copie.
    dirs: HashMap<PathBuf, bool>,
    buffers: (Vec<u8>, Vec<u8>),
}

impl Step<'_> {
    /// Vrai si `dir` est un vrai dossier, atteint sans passer par un lien ni
    /// par un « dossier jamais touché », même renommé ou déplacé depuis.
    fn dir_is_safe(&mut self, dir: &Path) -> bool {
        if let Some(&known) = self.dirs.get(dir) {
            return known;
        }
        let excluded = &self.plan.excluded;
        let leads_into_an_exclusion = || {
            !excluded.is_empty()
                && dir.ancestors().any(|ancestor| match fs::identity(ancestor) {
                    Ok(Some(identity)) => excluded.contains(&identity),
                    Ok(None) => false,
                    // Dans le doute, on n'y touche pas.
                    Err(_) => true,
                })
        };
        let safe =
            matches!(inspect_dir_chain(dir), Ok(PathState::RealDir)) && !leads_into_an_exclusion();
        self.dirs.insert(dir.to_path_buf(), safe);
        safe
    }

    /// Ouvre l'exemplaire gardé, s'il est toujours celui de l'analyse.
    fn open_kept(
        &self,
        group: &DuplicateGroup,
        kept: &DuplicateFile,
    ) -> Result<(VerifiedFile, Identity), Left> {
        let identity = recorded_identity(kept).ok_or(Left::Other)?;
        if !self.plan.covers_duplicate(&kept.path) {
            return Err(Left::Other);
        }
        let file = VerifiedFile::open(&kept.path, fingerprint(group, kept))
            .map_err(|error| reason(&error).unwrap_or(Left::Changed))?;
        // Le fichier ouvert est le bon. Reste à savoir que c'est bien lui,
        // sur ce disque, que son chemin désigne.
        match place_of(&kept.path, identity) {
            Place::Here => Ok((file, identity)),
            Place::Elsewhere | Place::Unknown => Err(Left::Changed),
        }
    }

    /// Envoie une copie à la corbeille, si tout confirme que c'en est une.
    /// `read` reçoit le nombre d'octets comparés.
    fn send(
        &mut self,
        group: &DuplicateGroup,
        kept: (&DuplicateFile, &mut VerifiedFile, Identity),
        copy: &DuplicateFile,
        read: &mut u64,
    ) -> Sent {
        let (kept, kept_file, kept_identity) = kept;
        let Some(identity) = recorded_identity(copy) else { return Sent::Left(Left::Other) };
        // Deux noms d'un même fichier ne sont pas deux exemplaires : envoyer
        // l'un à la corbeille y enverrait l'autre.
        if same_identity(identity, kept_identity) {
            return Sent::Left(Left::Other);
        }
        // Derniers contrôles avant d'écrire : la liste d'interdits et les
        // dossiers exclus, relus pour ce chemin.
        if self.guard.forbids(&copy.path) || !self.plan.covers_duplicate(&copy.path) {
            return Sent::Left(Left::Other);
        }
        let Some(parent) = copy.path.parent() else { return Sent::Left(Left::Other) };
        if !self.dir_is_safe(parent) {
            return Sent::Left(Left::Other);
        }

        let mut copy_file = match VerifiedFile::open(&copy.path, fingerprint(group, copy)) {
            Ok(file) => file,
            Err(error) => return reason(&error).map_or(Sent::Vanished, Sent::Left),
        };
        if place_of(&copy.path, identity) != Place::Here {
            return Sent::Left(Left::Changed);
        }

        let progress = self.progress;
        let compared = same_content(
            kept_file.file_mut(),
            copy_file.file_mut(),
            group.size,
            self.control.cancel_flag(),
            &mut self.buffers,
            &mut |bytes| {
                *read += bytes;
                progress.advance(bytes);
            },
        );
        match compared {
            Ok(Compared::Same) => {}
            Ok(Compared::Different) => return Sent::Left(Left::Changed),
            Ok(Compared::Cancelled) => return Sent::Cancelled,
            Err(_) => return Sent::Left(Left::Other),
        }
        // Modifié pendant la lecture : ce qui a été comparé ne vaut plus rien.
        if kept_file.recheck().is_err() {
            return Sent::KeptChanged;
        }
        if let Err(error) = copy_file.recheck() {
            return reason(&error).map_or(Sent::Vanished, Sent::Left);
        }

        let recycled = self.environment.recycle(&copy.path);
        // Windows a supprimé pour de bon : la copie n'est pas dans la corbeille,
        // quoi que le disque dise ensuite. Jamais comptée comme envoyée.
        if matches!(recycled, Ok(crate::platform::Sent::Destroyed)) {
            return Sent::Lost;
        }

        // Les faits, pas la réponse de Windows. La copie est toujours ouverte :
        // on sait si elle existe encore, et si elle est toujours à sa place.
        let alive = matches!(copy_file.recheck(), Ok(()) | Err(OpenError::Changed));
        match (place_of(&copy.path, identity), alive) {
            // Elle a quitté sa place et existe toujours : elle est dans la corbeille.
            (Place::Elsewhere, true) => {}
            // Elle a quitté sa place, et rien ne dit qu'elle existe encore.
            (Place::Elsewhere, false) => return Sent::Lost,
            // Elle n'a pas bougé, à ce qu'on voit. Mais un fichier supprimé
            // pour de bon ne disparaît qu'une fois que plus personne ne le
            // tient : on le lâche, et on regarde à nouveau.
            (Place::Here | Place::Unknown, _) => {
                drop(copy_file);
                return match place_of(&copy.path, identity) {
                    // Elle est restée : Windows a refusé, ou n'a rien fait.
                    Place::Here => {
                        Sent::Left(recycled.as_ref().err().map_or(Left::Other, bin_reason))
                    }
                    Place::Elsewhere | Place::Unknown => Sent::Lost,
                };
            }
        }
        let kept_intact =
            kept_file.recheck().is_ok() && place_of(&kept.path, kept_identity) == Place::Here;
        Sent::Recycled { kept_intact }
    }

    /// Laisse `files` copies de ce groupe en place, pour cette raison.
    fn leave(&self, outcome: &mut Outcome, group: &DuplicateGroup, reason: Left, files: u64) {
        outcome.skipped.add(reason, files);
        self.progress.skipped(files);
        self.progress.advance(group.size.saturating_mul(files));
    }

    /// Traite un groupe : chaque copie est comparée à l'exemplaire gardé,
    /// puis envoyée à la corbeille.
    fn group(
        &mut self,
        group: &DuplicateGroup,
        outcome: &mut Outcome,
        tick: &mut dyn FnMut(),
    ) -> Flow {
        let copies: Vec<&DuplicateFile> = group.files.iter().filter(|file| !file.keep).collect();
        let mut keepers = group.files.iter().filter(|file| file.keep);
        // Un groupe sans exemplaire gardé, ou avec plusieurs, n'est pas un
        // groupe que l'analyse a pu bâtir : on n'y touche pas.
        let (Some(kept), None) = (keepers.next(), keepers.next()) else {
            self.leave(outcome, group, Left::Other, copies.len() as u64);
            return Flow::Continue;
        };
        // L'exemplaire gardé reste ouvert tant que ses copies partent.
        let (mut kept_file, kept_identity) = match self.open_kept(group, kept) {
            Ok(opened) => opened,
            Err(reason) => {
                self.leave(outcome, group, reason, copies.len() as u64);
                return Flow::Continue;
            }
        };

        for (position, copy) in copies.iter().enumerate() {
            if self.control.is_cancelled() {
                outcome.step.state = StepState::Stopped;
                return Flow::Halt;
            }
            let mut read = 0u64;
            let sent = self.send(group, (kept, &mut kept_file, kept_identity), copy, &mut read);
            // L'avancement compte la copie en entier, quoi qu'elle soit devenue.
            self.progress.advance(group.size.saturating_sub(read));
            let stop = |outcome: &mut Outcome, reason: StopReason| {
                outcome.step.state = StepState::Stopped;
                outcome.step.stop = Some(DuplicatesStop { reason, file: copy.path.clone() });
            };
            match sent {
                Sent::Recycled { kept_intact } => {
                    outcome.step.copies += 1;
                    outcome.step.bytes += copy.on_disk;
                    outcome.sent_logical += group.size;
                    self.progress.recycled(copy.on_disk, 1);
                    if !kept_intact {
                        stop(outcome, StopReason::KeptCopyChanged);
                        return Flow::Halt;
                    }
                }
                Sent::Vanished => {}
                Sent::Left(reason) => {
                    outcome.skipped.add(reason, 1);
                    self.progress.skipped(1);
                }
                Sent::KeptChanged => {
                    // Cette copie et les suivantes n'ont plus de modèle sûr.
                    let rest = (copies.len() - position) as u64;
                    outcome.skipped.add(Left::Changed, rest);
                    self.progress.skipped(rest);
                    self.progress.advance(group.size.saturating_mul(rest - 1));
                    return Flow::Continue;
                }
                Sent::Cancelled => {
                    outcome.step.state = StepState::Stopped;
                    return Flow::Halt;
                }
                Sent::Lost => {
                    stop(outcome, StopReason::NotInRecycleBin);
                    return Flow::Halt;
                }
            }
            tick();
        }
        Flow::Continue
    }
}

/// Mène l'étape. `tick` est appelé régulièrement : il sert au journal.
pub(crate) fn clean(
    environment: &dyn Environment,
    report: &Report,
    excluded: &[PathBuf],
    control: &CleanControl,
    tick: &mut dyn FnMut(),
) -> Outcome {
    let mut outcome = Outcome::default();
    let plan = plan(environment, report.started, excluded);
    // Un « dossier jamais touché » que Shinku ne sait pas appliquer ne
    // protège rien : dans le doute, rien ne part.
    if plan.misses_an_exclusion() {
        outcome.step.state = StepState::Failed;
        return outcome;
    }
    let groups = &report.duplicates.groups;
    let progress = control.progress_counters();
    let work = |group: &DuplicateGroup| {
        let copies = group.files.iter().filter(|file| !file.keep).count() as u64;
        group.size.saturating_mul(copies)
    };
    progress.begin(CleanPhase::Duplicates, groups.iter().map(work).sum());

    let mut step = Step {
        guard: Guard::for_machine(environment),
        environment,
        plan,
        control,
        progress,
        dirs: HashMap::new(),
        buffers: (vec![0u8; CHUNK], vec![0u8; CHUNK]),
    };
    outcome.step.state = StepState::Done;
    for group in groups {
        if control.is_cancelled() {
            outcome.step.state = StepState::Stopped;
            break;
        }
        if step.group(group, &mut outcome, tick) == Flow::Halt {
            break;
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn pair(left: &[u8], right: &[u8]) -> (tempfile::TempDir, File, File) {
        let temp = tempfile::tempdir().unwrap();
        let write = |name: &str, content: &[u8]| {
            let path = temp.path().join(name);
            File::create(&path).unwrap().write_all(content).unwrap();
            File::open(&path).unwrap()
        };
        let (a, b) = (write("a", left), write("b", right));
        (temp, a, b)
    }

    fn compare(left: &[u8], right: &[u8], size: u64, chunk: usize) -> (Compared, u64) {
        let (_temp, mut a, mut b) = pair(left, right);
        let mut read = 0;
        let compared = same_content(
            &mut a,
            &mut b,
            size,
            &AtomicBool::new(false),
            &mut (vec![0u8; chunk], vec![0u8; chunk]),
            &mut |bytes| read += bytes,
        )
        .unwrap();
        (compared, read)
    }

    #[test]
    fn identical_files_are_the_same_whatever_the_chunk_size() {
        let content: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        for chunk in [1, 7, 4096, 10_000, 65_536] {
            assert_eq!(
                compare(&content, &content, 10_000, chunk),
                (Compared::Same, 10_000),
                "lectures de {chunk} octets"
            );
        }
        assert_eq!(compare(b"", b"", 0, 16), (Compared::Same, 0));
    }

    #[test]
    fn a_single_different_byte_is_seen_wherever_it_is() {
        let content: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        for position in [0, 1, 4095, 4096, 9_999] {
            let mut other = content.clone();
            other[position] ^= 1;
            let (compared, _) = compare(&content, &other, 10_000, 4096);
            assert_eq!(compared, Compared::Different, "octet {position}");
        }
    }

    #[test]
    fn a_file_that_is_shorter_or_longer_than_announced_is_different() {
        let content = vec![7u8; 5000];
        // L'un des deux s'arrête avant la taille annoncée.
        assert_eq!(compare(&content, &content[..4999], 5000, 4096).0, Compared::Different);
        assert_eq!(compare(&content[..100], &content, 5000, 4096).0, Compared::Different);
        // L'un des deux continue au-delà.
        let mut longer = content.clone();
        longer.push(7);
        assert_eq!(compare(&content, &longer, 5000, 4096).0, Compared::Different);
        assert_eq!(compare(&longer, &content, 5000, 4096).0, Compared::Different);
        // Les deux continuent au-delà : ce ne sont plus les fichiers de l'analyse.
        assert_eq!(compare(&longer, &longer, 5000, 4096).0, Compared::Different);
    }

    #[test]
    fn a_comparison_can_be_stopped_between_two_reads() {
        let content = vec![1u8; 9000];
        let (_temp, mut a, mut b) = pair(&content, &content);
        let cancel = AtomicBool::new(false);
        let mut read = 0;
        let compared = same_content(
            &mut a,
            &mut b,
            9000,
            &cancel,
            &mut (vec![0u8; 4096], vec![0u8; 4096]),
            &mut |bytes| {
                read += bytes;
                cancel.store(true, Ordering::Relaxed);
            },
        )
        .unwrap();
        assert_eq!((compared, read), (Compared::Cancelled, 4096));
    }

    #[test]
    fn refusals_are_sorted_by_reason() {
        assert_eq!(reason(&OpenError::NotFound), None);
        assert_eq!(reason(&OpenError::Locked), Some(Left::InUse));
        assert_eq!(reason(&OpenError::Changed), Some(Left::Changed));
        assert_eq!(reason(&OpenError::NotRegular), Some(Left::Changed));
        assert_eq!(reason(&OpenError::AccessDenied), Some(Left::Denied));
        assert_eq!(reason(&OpenError::OnlineOnly), Some(Left::Other));
        assert_eq!(bin_reason(&RecycleError::Refused), Left::Unrecyclable);
        assert_eq!(bin_reason(&RecycleError::Unavailable), Left::Unrecyclable);
        assert_eq!(bin_reason(&RecycleError::Locked), Left::InUse);
        assert_eq!(bin_reason(&RecycleError::AccessDenied), Left::Denied);
        assert_eq!(bin_reason(&RecycleError::Failed("x".to_owned())), Left::Other);
    }
}
