//! Parcours des dossiers, en parallèle.
//!
//! Le parcours ne sait rien des règles : il lit les dossiers, présente chaque
//! entrée à un [`Visitor`], et descend là où le visiteur le demande. Il ne
//! suit jamais un lien (c'est [`crate::fs`] qui le garantit) et ne lit aucun
//! fichier.
//!
//! Plusieurs fils d'exécution se partagent une pile de dossiers à lire. Chaque
//! fil garde son propre collecteur ([`Visitor::Sink`]) : rien n'est partagé
//! pendant la lecture, et les collecteurs sont fusionnés à la fin. Sur un
//! disque dur, on demande un seul fil.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use crate::fs::{DirReader, EntryMeta, ReadDirError};
use crate::model::FileId;
use crate::platform;

/// Profondeur au-delà de laquelle on ne descend plus. Aucun dossier réel
/// n'approche cette limite ; elle borne seulement le pire cas.
pub const MAX_DEPTH: u32 = 256;

/// Le dossier en cours de lecture.
#[derive(Debug)]
pub struct Dir<'a, C> {
    /// Chemin interne du dossier.
    pub path: &'a Path,
    /// Contexte que le visiteur lui a attaché.
    pub ctx: &'a C,
    /// Nombre de niveaux sous la racine du parcours.
    pub depth: u32,
}

/// Un sous-dossier dans lequel le visiteur a demandé à descendre.
#[derive(Debug)]
pub struct Child<C> {
    /// Nom du sous-dossier.
    pub name: OsString,
    /// Identifiant annoncé par la liste du parent, revérifié à l'ouverture.
    pub file_id: Option<FileId>,
    /// Contexte avec lequel le parcourir.
    pub ctx: C,
}

/// Un point de départ du parcours.
#[derive(Debug)]
pub struct Root<C> {
    /// Chemin interne du dossier de départ.
    pub path: PathBuf,
    /// Identifiant attendu, s'il est connu.
    pub file_id: Option<FileId>,
    /// Contexte de départ.
    pub ctx: C,
}

/// Ce que le parcours montre, et ce qu'on lui répond.
pub trait Visitor: Sync {
    /// Contexte attaché à chaque dossier, calculé à partir de celui du parent.
    type Ctx: Send;
    /// Collecteur propre à chaque fil d'exécution.
    type Sink: Send;

    /// Crée le collecteur d'un fil d'exécution.
    fn sink(&self) -> Self::Sink;

    /// Un sous-dossier parcourable vient d'être listé. Rendre son contexte
    /// pour y descendre, ou `None` pour ne pas y aller.
    fn directory(
        &self,
        sink: &mut Self::Sink,
        parent: &Dir<'_, Self::Ctx>,
        name: &OsStr,
        meta: &EntryMeta,
    ) -> Option<Self::Ctx>;

    /// Tout le reste vient d'être listé : fichier, lien, ou dossier dans
    /// lequel on ne peut pas descendre.
    fn entry(
        &self,
        sink: &mut Self::Sink,
        parent: &Dir<'_, Self::Ctx>,
        name: &OsStr,
        meta: &EntryMeta,
    );

    /// Le dossier a été lu en entier, ou n'a pas pu l'être (`outcome`).
    /// Dernière occasion de corriger ou de retirer des sous-dossiers à
    /// parcourir : certaines décisions demandent d'avoir tout vu.
    fn leave(
        &self,
        sink: &mut Self::Sink,
        dir: &Dir<'_, Self::Ctx>,
        children: &mut Vec<Child<Self::Ctx>>,
        outcome: &Result<(), ReadDirError>,
    );
}

/// Réglages d'un parcours.
#[derive(Clone, Copy, Debug)]
pub struct WalkOptions {
    /// Nombre de fils d'exécution (au moins un).
    pub threads: usize,
    /// Lire en priorité basse, pour passer après l'utilisateur.
    pub background: bool,
}

struct Job<C> {
    path: PathBuf,
    file_id: Option<FileId>,
    ctx: C,
    depth: u32,
}

struct Queue<C> {
    jobs: Vec<Job<C>>,
    /// Nombre de fils en train de lire un dossier (ils peuvent encore ajouter du travail).
    busy: usize,
    /// Plus rien à faire : tout le monde s'arrête.
    closed: bool,
}

struct Shared<C> {
    queue: Mutex<Queue<C>>,
    wake: Condvar,
}

impl<C> Shared<C> {
    fn lock(&self) -> MutexGuard<'_, Queue<C>> {
        // Un fil qui a paniqué ne laisse pas la pile dans un état incohérent :
        // chaque modification tient en une instruction.
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Prend un dossier à lire. `None` quand le parcours est terminé ou annulé.
    fn take(&self, cancel: &AtomicBool) -> Option<Job<C>> {
        let mut queue = self.lock();
        loop {
            if queue.closed {
                return None;
            }
            if cancel.load(Ordering::Relaxed) {
                queue.closed = true;
                self.wake.notify_all();
                return None;
            }
            if let Some(job) = queue.jobs.pop() {
                queue.busy += 1;
                return Some(job);
            }
            if queue.busy == 0 {
                queue.closed = true;
                self.wake.notify_all();
                return None;
            }
            queue = self.wake.wait(queue).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Rend la main après un dossier : partage les sous-dossiers trouvés.
    /// `keeps_working` : le fil enchaîne sur un sous-dossier qu'il garde pour lui.
    fn give(&self, jobs: impl Iterator<Item = Job<C>>, keeps_working: bool) {
        let mut queue = self.lock();
        let before = queue.jobs.len();
        queue.jobs.extend(jobs);
        let added = queue.jobs.len() - before;
        if !keeps_working {
            queue.busy -= 1;
        }
        if added > 0 || (queue.busy == 0 && queue.jobs.is_empty()) {
            // Du travail pour les fils en attente, ou la fin pour tous.
            self.wake.notify_all();
        }
    }

    /// Un fil s'arrête sur une panique : on libère tous les autres.
    fn abort(&self) {
        let mut queue = self.lock();
        queue.closed = true;
        self.wake.notify_all();
    }
}

/// Ferme le parcours si le fil qui la porte panique, pour que les autres
/// fils ne restent pas à attendre un travail qui ne viendra plus.
struct AbortOnPanic<'a, C>(&'a Shared<C>);

impl<C> Drop for AbortOnPanic<'_, C> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.abort();
        }
    }
}

/// Parcourt les dossiers à partir de `roots` et rend un collecteur par fil.
///
/// Quand `cancel` passe à vrai, le parcours s'arrête dès que les lectures en
/// cours se terminent ; les collecteurs rendus sont alors partiels.
pub fn walk<V: Visitor>(
    roots: Vec<Root<V::Ctx>>,
    visitor: &V,
    options: WalkOptions,
    cancel: &AtomicBool,
) -> Vec<V::Sink> {
    if roots.is_empty() {
        return Vec::new();
    }
    let jobs = roots
        .into_iter()
        .map(|root| Job { path: root.path, file_id: root.file_id, ctx: root.ctx, depth: 0 })
        .collect();
    let shared =
        Shared { queue: Mutex::new(Queue { jobs, busy: 0, closed: false }), wake: Condvar::new() };

    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..options.threads.max(1))
            .map(|_| scope.spawn(|| work(&shared, visitor, options.background, cancel)))
            .collect();
        workers
            .into_iter()
            .map(|worker| match worker.join() {
                Ok(sink) => sink,
                Err(panic) => std::panic::resume_unwind(panic),
            })
            .collect()
    })
}

fn work<V: Visitor>(
    shared: &Shared<V::Ctx>,
    visitor: &V,
    background: bool,
    cancel: &AtomicBool,
) -> V::Sink {
    let _abort = AbortOnPanic(shared);
    let _priority = background.then(platform::background_priority);
    let mut sink = visitor.sink();
    let mut reader = DirReader::new();
    let mut children: Vec<Child<V::Ctx>> = Vec::new();
    // Sous-dossier que ce fil garde pour lui, pour avancer en profondeur sans
    // repasser par la pile commune.
    let mut next: Option<Job<V::Ctx>> = None;

    loop {
        let job = match next.take() {
            Some(job) if !cancel.load(Ordering::Relaxed) => job,
            Some(_) => {
                shared.give(std::iter::empty(), false);
                continue;
            }
            None => match shared.take(cancel) {
                Some(job) => job,
                None => break,
            },
        };

        let dir = Dir { path: &job.path, ctx: &job.ctx, depth: job.depth };
        let can_descend = job.depth < MAX_DEPTH;
        let outcome = reader.read(&job.path, job.file_id, &mut |name, meta| {
            if can_descend && meta.is_walkable_dir() {
                if let Some(ctx) = visitor.directory(&mut sink, &dir, name, meta) {
                    children.push(Child { name: name.to_os_string(), file_id: meta.file_id, ctx });
                }
            } else {
                visitor.entry(&mut sink, &dir, name, meta);
            }
        });
        visitor.leave(&mut sink, &dir, &mut children, &outcome);

        let depth = job.depth + 1;
        let mut jobs = children.drain(..).map(|child| Job {
            path: job.path.join(&child.name),
            file_id: child.file_id,
            ctx: child.ctx,
            depth,
        });
        next = jobs.next();
        shared.give(jobs, next.is_some());
    }
    sink
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::fs::{EntryKind, to_internal_path};

    /// Visiteur de test : note tout ce qu'il voit, n'entre pas dans les
    /// dossiers nommés `interdit`, et retire au dernier moment ceux nommés `tardif`.
    struct Recorder {
        directories_seen: AtomicUsize,
        stop_after: Option<(usize, &'static AtomicBool)>,
    }

    #[derive(Default)]
    struct Seen {
        files: BTreeSet<String>,
        links: BTreeSet<String>,
        failures: Vec<String>,
        max_depth: u32,
    }

    impl Visitor for Recorder {
        type Ctx = String;
        type Sink = Seen;

        fn sink(&self) -> Seen {
            Seen::default()
        }

        fn directory(
            &self,
            _sink: &mut Seen,
            parent: &Dir<'_, String>,
            name: &OsStr,
            _meta: &EntryMeta,
        ) -> Option<String> {
            let name = name.to_string_lossy();
            (name != "interdit").then(|| format!("{}/{name}", parent.ctx))
        }

        fn entry(&self, sink: &mut Seen, parent: &Dir<'_, String>, name: &OsStr, meta: &EntryMeta) {
            let label = format!("{}/{}", parent.ctx, name.to_string_lossy());
            match meta.kind {
                EntryKind::Link => sink.links.insert(label),
                _ => sink.files.insert(label),
            };
            sink.max_depth = sink.max_depth.max(parent.depth);
        }

        fn leave(
            &self,
            sink: &mut Seen,
            dir: &Dir<'_, String>,
            children: &mut Vec<Child<String>>,
            outcome: &Result<(), ReadDirError>,
        ) {
            if let Err(error) = outcome {
                sink.failures.push(format!("{} : {error}", dir.ctx));
            }
            children.retain(|child| child.name != "tardif");
            let seen = self.directories_seen.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some((limit, cancel)) = self.stop_after
                && seen >= limit
            {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    fn recorder() -> Recorder {
        Recorder { directories_seen: AtomicUsize::new(0), stop_after: None }
    }

    fn merged(sinks: Vec<Seen>) -> Seen {
        let mut all = Seen::default();
        for sink in sinks {
            all.files.extend(sink.files);
            all.links.extend(sink.links);
            all.failures.extend(sink.failures);
            all.max_depth = all.max_depth.max(sink.max_depth);
        }
        all
    }

    fn tree() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = to_internal_path(&fs::canonicalize(temp.path()).unwrap()).unwrap();
        for dir in ["a/b/c", "a/interdit/x", "a/tardif/y", "d"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["1.txt", "a/2.txt", "a/b/3.txt", "a/b/c/4.txt", "d/5.txt"] {
            fs::write(root.join(file), b"x").unwrap();
        }
        fs::write(root.join("a/interdit/x/secret.txt"), b"x").unwrap();
        fs::write(root.join("a/tardif/y/secret.txt"), b"x").unwrap();
        (temp, root)
    }

    fn roots(root: &Path) -> Vec<Root<String>> {
        vec![Root { path: root.to_path_buf(), file_id: None, ctx: String::new() }]
    }

    #[test]
    fn every_file_is_seen_once_whatever_the_thread_count() {
        let (_temp, root) = tree();
        let expected: BTreeSet<String> =
            ["/1.txt", "/a/2.txt", "/a/b/3.txt", "/a/b/c/4.txt", "/d/5.txt"]
                .into_iter()
                .map(str::to_owned)
                .collect();
        for threads in [1, 2, 8] {
            let cancel = AtomicBool::new(false);
            let options = WalkOptions { threads, background: false };
            let seen = merged(walk(roots(&root), &recorder(), options, &cancel));
            assert_eq!(seen.files, expected, "{threads} fil(s)");
            assert!(seen.failures.is_empty());
            assert_eq!(seen.max_depth, 3);
        }
    }

    #[test]
    fn refused_and_withdrawn_folders_are_not_entered() {
        let (_temp, root) = tree();
        let cancel = AtomicBool::new(false);
        let options = WalkOptions { threads: 4, background: false };
        let seen = merged(walk(roots(&root), &recorder(), options, &cancel));
        assert!(!seen.files.iter().any(|file| file.contains("secret")), "{:?}", seen.files);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_shown_but_not_followed() {
        let (_temp, root) = tree();
        std::os::unix::fs::symlink(root.join("a"), root.join("d").join("boucle")).unwrap();
        let cancel = AtomicBool::new(false);
        let options = WalkOptions { threads: 2, background: false };
        let seen = merged(walk(roots(&root), &recorder(), options, &cancel));
        assert_eq!(seen.links.iter().collect::<Vec<_>>(), ["/d/boucle"]);
        assert_eq!(seen.files.len(), 5, "le contenu du lien n'est pas compté une seconde fois");
    }

    #[test]
    fn a_missing_root_is_reported_to_the_visitor() {
        let (_temp, root) = tree();
        let cancel = AtomicBool::new(false);
        let options = WalkOptions { threads: 2, background: false };
        let seen = merged(walk(roots(&root.join("absent")), &recorder(), options, &cancel));
        assert_eq!(seen.failures.len(), 1);
        assert!(seen.files.is_empty());
    }

    #[test]
    fn no_roots_means_no_work() {
        let cancel = AtomicBool::new(false);
        let options = WalkOptions { threads: 4, background: false };
        assert!(walk(Vec::new(), &recorder(), options, &cancel).is_empty());
    }

    #[test]
    fn cancelling_stops_the_walk_early() {
        // Une longue chaîne de dossiers : sans annulation, 60 lectures.
        let temp = tempfile::tempdir().unwrap();
        let root = to_internal_path(&fs::canonicalize(temp.path()).unwrap()).unwrap();
        let mut deep = root.clone();
        for level in 0..60 {
            deep.push(format!("n{level}"));
        }
        fs::create_dir_all(&deep).unwrap();

        static CANCEL: AtomicBool = AtomicBool::new(false);
        let visitor =
            Recorder { directories_seen: AtomicUsize::new(0), stop_after: Some((5, &CANCEL)) };
        let options = WalkOptions { threads: 3, background: false };
        walk(roots(&root), &visitor, options, &CANCEL);
        let seen = visitor.directories_seen.load(Ordering::Relaxed);
        assert!((5..20).contains(&seen), "{seen} dossiers lus après l'annulation");
    }

    #[test]
    fn a_cancelled_walk_does_nothing() {
        let (_temp, root) = tree();
        let cancel = AtomicBool::new(true);
        let options = WalkOptions { threads: 4, background: true };
        let seen = merged(walk(roots(&root), &recorder(), options, &cancel));
        assert!(seen.files.is_empty());
    }

    #[test]
    fn depth_is_bounded() {
        // Au-delà de la limite, un dossier est montré comme une entrée simple.
        let temp = tempfile::tempdir().unwrap();
        let root = to_internal_path(&fs::canonicalize(temp.path()).unwrap()).unwrap();
        fs::create_dir(root.join("x")).unwrap();
        struct Shallow;
        impl Visitor for Shallow {
            type Ctx = ();
            type Sink = Vec<String>;
            fn sink(&self) -> Vec<String> {
                Vec::new()
            }
            fn directory(
                &self,
                _: &mut Vec<String>,
                _: &Dir<'_, ()>,
                _: &OsStr,
                _: &EntryMeta,
            ) -> Option<()> {
                Some(())
            }
            fn entry(&self, sink: &mut Vec<String>, _: &Dir<'_, ()>, name: &OsStr, _: &EntryMeta) {
                sink.push(name.to_string_lossy().into_owned());
            }
            fn leave(
                &self,
                _: &mut Vec<String>,
                _: &Dir<'_, ()>,
                _: &mut Vec<Child<()>>,
                _: &Result<(), ReadDirError>,
            ) {
            }
        }
        // On part directement de la profondeur limite.
        let shared = Shared {
            queue: Mutex::new(Queue {
                jobs: vec![Job { path: root, file_id: None, ctx: (), depth: MAX_DEPTH }],
                busy: 0,
                closed: false,
            }),
            wake: Condvar::new(),
        };
        let cancel = AtomicBool::new(false);
        let seen = work(&shared, &Shallow, false, &cancel);
        assert_eq!(seen, ["x"]);
    }
}
