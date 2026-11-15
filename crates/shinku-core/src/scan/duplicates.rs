//! Recherche de doublons, en cascade.
//!
//! Lire un fichier coûte cher : on ne le fait que lorsqu'il le faut.
//!
//! 1. Les liens physiques d'un même fichier sont ramenés à un seul candidat :
//!    ce ne sont pas des copies, ils ne prennent la place qu'une fois.
//! 2. Seuls les fichiers de même taille peuvent être identiques.
//! 3. Parmi eux, on compare l'empreinte d'un extrait (début et fin).
//! 4. Pour ceux qui restent, l'empreinte BLAKE3 du fichier entier.
//!
//! La comparaison octet par octet n'a lieu qu'au moment de supprimer, contre
//! l'exemplaire gardé. Dans chaque groupe, l'exemplaire le plus ancien est
//! gardé, tant que la personne n'en choisit pas un autre (`Report::keeping`) :
//! l'exemplaire gardé n'est jamais proposé à la suppression.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::index::Candidate;
use super::report::{DuplicateFile, DuplicateGroup, DuplicatesReport, DuplicatesState};
use super::{Phase, Progress};
use crate::fs::{Fingerprint, VerifiedFile};
use crate::hash::{
    CacheKey, CachedDigests, Digest, DigestCache, EXTRACT_BYTES, extract_covers, extract_digest,
    full_digest,
};

/// Au plus ce nombre de fichiers lus en même temps : au-delà, c'est le disque
/// qui limite, pas le processeur.
const MAX_READERS: usize = 4;

pub(crate) struct Finder<'a> {
    pub cache: &'a dyn DigestCache,
    pub progress: &'a Progress,
    pub cancel: &'a AtomicBool,
    /// Nombre de lectures en parallèle permis sur un volume.
    pub threads_for: &'a (dyn Fn(u64) -> usize + Sync),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Extract,
    Full,
}

impl Finder<'_> {
    /// Cherche les doublons parmi `candidates`. `None` si l'analyse est annulée.
    pub(crate) fn find(&self, mut candidates: Vec<Candidate>) -> Option<DuplicatesReport> {
        let mut report = DuplicatesReport::empty(DuplicatesState::Done);
        report.candidates = candidates.len() as u64;

        // 1. Liens physiques : même volume, même identifiant.
        candidates
            .sort_by(|a, b| (a.volume, a.file_id, &a.path).cmp(&(b.volume, b.file_id, &b.path)));
        let before = candidates.len();
        candidates.dedup_by(|later, first| {
            later.file_id.is_some()
                && (later.volume, later.file_id) == (first.volume, first.file_id)
        });
        report.hard_links = (before - candidates.len()) as u64;

        // 2. Même taille.
        let by_size = group_by(0..candidates.len(), |&index| candidates[index].size);
        let sized: Vec<usize> = by_size.into_iter().flatten().collect();

        // 3. Même extrait.
        let total = sized.iter().map(|&i| candidates[i].size.min(2 * EXTRACT_BYTES)).sum();
        self.progress.begin(Phase::DuplicatesQuick, total);
        let extracts = self.digest_all(&candidates, &sized, Stage::Extract)?;
        report.skipped_files += extracts.values().filter(|digest| digest.is_none()).count() as u64;
        let by_extract =
            group_by(sized.iter().copied().filter(|index| extracts[index].is_some()), |&index| {
                (candidates[index].size, extracts[&index])
            });
        let matched: Vec<usize> = by_extract.into_iter().flatten().collect();

        // 4. Même contenu. Un petit fichier est déjà couvert en entier par son extrait.
        let to_read: Vec<usize> =
            matched.iter().copied().filter(|&i| !extract_covers(candidates[i].size)).collect();
        self.progress
            .begin(Phase::DuplicatesFull, to_read.iter().map(|&i| candidates[i].size).sum());
        let fulls = self.digest_all(&candidates, &to_read, Stage::Full)?;
        report.skipped_files += fulls.values().filter(|digest| digest.is_none()).count() as u64;
        let identity = |index: usize| -> Option<Digest> {
            if extract_covers(candidates[index].size) {
                extracts[&index]
            } else {
                fulls.get(&index).copied().flatten()
            }
        };
        let groups = group_by(
            matched.iter().copied().filter(|&index| identity(index).is_some()),
            |&index| (candidates[index].size, identity(index)),
        );

        for members in groups {
            let digest = identity(members[0]).map(|digest| digest.to_hex()).unwrap_or_default();
            report.groups.push(build_group(&candidates, members, digest));
        }
        // Le plus gros gain d'abord ; à gain égal, un ordre stable.
        report.groups.sort_by(|a, b| {
            b.reclaimable
                .cmp(&a.reclaimable)
                .then_with(|| b.size.cmp(&a.size))
                .then_with(|| a.files[0].path.cmp(&b.files[0].path))
        });
        report.copies = report.groups.iter().map(|group| group.files.len() as u64 - 1).sum();
        report.bytes = report.groups.iter().map(|group| group.reclaimable).sum();
        Some(report)
    }

    /// Calcule une empreinte pour chaque fichier de `wanted`, volume par
    /// volume. `None` à la place d'une empreinte : le fichier est écarté.
    /// Rend `None` si l'analyse est annulée.
    fn digest_all(
        &self,
        candidates: &[Candidate],
        wanted: &[usize],
        stage: Stage,
    ) -> Option<HashMap<usize, Option<Digest>>> {
        let mut results = HashMap::with_capacity(wanted.len());
        let mut by_volume: HashMap<u64, Vec<usize>> = HashMap::new();
        for &index in wanted {
            by_volume.entry(candidates[index].volume).or_default().push(index);
        }
        for (volume, mut tasks) in by_volume {
            // Dans l'ordre des chemins : sur un disque dur, les fichiers d'un
            // même dossier sont souvent voisins.
            tasks.sort_by(|&a, &b| candidates[a].path.cmp(&candidates[b].path));
            let threads = (self.threads_for)(volume).clamp(1, MAX_READERS).min(tasks.len().max(1));
            let cursor = AtomicUsize::new(0);
            let parts: Vec<Vec<(usize, Option<Digest>)>> = std::thread::scope(|scope| {
                let workers: Vec<_> = (0..threads)
                    .map(|_| {
                        scope.spawn(|| {
                            let mut done = Vec::new();
                            loop {
                                let next = cursor.fetch_add(1, Ordering::Relaxed);
                                let Some(&index) = tasks.get(next) else { break };
                                if self.cancel.load(Ordering::Relaxed) {
                                    break;
                                }
                                done.push((index, self.digest_one(&candidates[index], stage)));
                            }
                            done
                        })
                    })
                    .collect();
                workers
                    .into_iter()
                    .map(|worker| match worker.join() {
                        Ok(done) => done,
                        Err(panic) => std::panic::resume_unwind(panic),
                    })
                    .collect()
            });
            results.extend(parts.into_iter().flatten());
        }
        (!self.cancel.load(Ordering::Relaxed)).then_some(results)
    }

    /// Empreinte d'un fichier, depuis le cache ou en le lisant. `None` s'il
    /// ne peut pas être lu tel que l'analyse l'a vu.
    fn digest_one(&self, candidate: &Candidate, stage: Stage) -> Option<Digest> {
        let key = candidate.file_id.map(|file_id| CacheKey {
            volume: candidate.volume,
            file_id,
            size: candidate.size,
            modified: candidate.modified,
        });
        let known = key.and_then(|key| self.cache.get(&key)).unwrap_or_default();
        let cached = match stage {
            Stage::Extract => known.extract,
            Stage::Full => known.full,
        };
        let weight = match stage {
            Stage::Extract => candidate.size.min(2 * EXTRACT_BYTES),
            Stage::Full => candidate.size,
        };
        if let Some(digest) = cached {
            self.progress.advance(weight);
            return Some(digest);
        }

        let fingerprint = Fingerprint {
            size: candidate.size,
            modified: candidate.modified,
            file_id: candidate.file_id,
        };
        let mut read = 0u64;
        let outcome = VerifiedFile::open(&candidate.path, fingerprint).ok().and_then(|mut file| {
            let digest = match stage {
                Stage::Extract => extract_digest(&mut file, candidate.size).ok()?,
                Stage::Full => {
                    let mut count = |bytes: u64| {
                        read += bytes;
                        self.progress.advance(bytes);
                    };
                    full_digest(&mut file, &mut count, self.cancel).ok()??
                }
            };
            // Modifié pendant la lecture : ce qu'on a lu ne vaut rien.
            file.recheck().ok()?;
            Some(digest)
        });
        // La progression avance même pour un fichier écarté : le total reste juste.
        self.progress.advance(weight.saturating_sub(read));

        let digest = outcome?;
        if let Some(key) = key {
            let digests = match stage {
                // L'extrait d'un petit fichier est le fichier entier.
                Stage::Extract if extract_covers(candidate.size) => {
                    CachedDigests { extract: Some(digest), full: Some(digest) }
                }
                Stage::Extract => CachedDigests { extract: Some(digest), full: None },
                Stage::Full => CachedDigests { extract: None, full: Some(digest) },
            };
            self.cache.put(key, digests);
        }
        Some(digest)
    }
}

/// Regroupe des indices par clé et ne garde que les groupes d'au moins deux,
/// dans un ordre stable.
fn group_by<K: Ord>(
    indices: impl Iterator<Item = usize>,
    key: impl Fn(&usize) -> K,
) -> Vec<Vec<usize>> {
    let mut keyed: Vec<(K, usize)> = indices.map(|index| (key(&index), index)).collect();
    keyed.sort();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut previous: Option<K> = None;
    for (key, index) in keyed {
        match (&previous, groups.last_mut()) {
            (Some(same), Some(group)) if *same == key => group.push(index),
            _ => {
                groups.push(vec![index]);
                previous = Some(key);
            }
        }
    }
    groups.retain(|group| group.len() > 1);
    groups
}

fn build_group(
    candidates: &[Candidate],
    mut members: Vec<usize>,
    digest: String,
) -> DuplicateGroup {
    // Le plus ancien d'abord : c'est l'exemplaire gardé.
    members.sort_by(|&a, &b| {
        let (a, b) = (&candidates[a], &candidates[b]);
        (a.created, a.modified, &a.path).cmp(&(b.created, b.modified, &b.path))
    });
    let files: Vec<DuplicateFile> = members
        .iter()
        .enumerate()
        .map(|(position, &index)| {
            let candidate = &candidates[index];
            DuplicateFile {
                path: candidate.path.clone(),
                on_disk: candidate.on_disk,
                created: candidate.created,
                modified: candidate.modified,
                keep: position == 0,
                synced: candidate.synced,
                file_id: candidate.file_id,
                volume: candidate.volume,
            }
        })
        .collect();
    DuplicateGroup {
        size: candidates[members[0]].size,
        digest,
        reclaimable: files.iter().filter(|file| !file.keep).map(|file| file.on_disk).sum(),
        files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping_keeps_only_real_groups_in_a_stable_order() {
        let sizes = [5u64, 3, 5, 9, 3, 5];
        let groups = group_by(0..sizes.len(), |&index| sizes[index]);
        assert_eq!(groups, [vec![1, 4], vec![0, 2, 5]]);
        assert!(group_by(0..0, |&index| index).is_empty());
        assert!(group_by(0..3, |&index| index).is_empty());
    }
}
