//! Empreintes de fichiers pour la recherche de doublons.
//!
//! Deux empreintes BLAKE3 par fichier, calculées seulement quand il le faut :
//!
//! - celle d'un extrait (le début et la fin du fichier), qui coûte deux
//!   lectures et écarte presque tous les faux candidats de même taille ;
//! - celle du fichier entier, pour les groupes qui restent.
//!
//! Un petit fichier tient tout entier dans son extrait : ses deux empreintes
//! sont alors la même, et il n'est lu qu'une fois.
//!
//! Un fichier n'est lu qu'à travers [`VerifiedFile`] : on sait donc qu'il n'a
//! pas changé depuis l'analyse, qu'il n'est pas un lien et que son contenu est
//! bien sur le disque. Les empreintes sont gardées dans un cache, pour ne pas
//! relire un fichier inchangé à l'analyse suivante.

use std::collections::HashMap;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::fs::VerifiedFile;
use crate::model::{FileId, Timestamp};

/// Taille de chaque morceau de l'extrait : 64 Ko au début, 64 Ko à la fin.
pub const EXTRACT_BYTES: u64 = 64 * 1024;

/// Taille des lectures pour l'empreinte complète.
const READ_BYTES: usize = 1024 * 1024;

/// Contexte de dérivation de l'empreinte d'un extrait partiel. Elle ne peut
/// ainsi jamais être confondue avec l'empreinte complète d'un autre fichier.
const EXTRACT_CONTEXT: &str = "Shinku 2026-11 empreinte d'extrait v1";

/// Une empreinte BLAKE3 (256 bits).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest(pub [u8; 32]);

impl Digest {
    /// Écriture hexadécimale, comme l'affiche `b3sum`.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({}…)", &self.to_hex()[..12])
    }
}

/// Vrai si l'extrait couvre tout le fichier : l'empreinte d'extrait suffit
/// alors à l'identifier, sans seconde lecture.
pub const fn extract_covers(size: u64) -> bool {
    size <= 2 * EXTRACT_BYTES
}

/// Empreinte de l'extrait d'un fichier de `size` octets : sa taille, son
/// début et sa fin.
///
/// Si l'extrait couvre tout le fichier ([`extract_covers`]), c'est l'empreinte
/// BLAKE3 ordinaire de son contenu, la même que [`full_digest`].
pub fn extract_digest(file: &mut VerifiedFile, size: u64) -> io::Result<Digest> {
    let reader = file.file_mut();
    reader.seek(SeekFrom::Start(0))?;

    if extract_covers(size) {
        let mut content = vec![0u8; size as usize];
        reader.read_exact(&mut content)?;
        return Ok(Digest(*blake3::hash(&content).as_bytes()));
    }

    let mut hasher = blake3::Hasher::new_derive_key(EXTRACT_CONTEXT);
    hasher.update(&size.to_le_bytes());
    let mut buffer = vec![0u8; EXTRACT_BYTES as usize];
    reader.read_exact(&mut buffer)?;
    hasher.update(&buffer);
    reader.seek(SeekFrom::Start(size - EXTRACT_BYTES))?;
    reader.read_exact(&mut buffer)?;
    hasher.update(&buffer);
    Ok(Digest(*hasher.finalize().as_bytes()))
}

/// Empreinte BLAKE3 du fichier entier.
///
/// `progress` reçoit le nombre d'octets lus à chaque lecture. Rend `None` si
/// `cancel` passe à vrai en cours de route.
pub fn full_digest(
    file: &mut VerifiedFile,
    progress: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> io::Result<Option<Digest>> {
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; READ_BYTES];
    let reader = file.file_mut();
    reader.seek(SeekFrom::Start(0))?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                hasher.update(&buffer[..read]);
                progress(read as u64);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(Some(Digest(*hasher.finalize().as_bytes())))
}

/// Ce qui désigne un fichier dans le cache : s'il change de taille ou de
/// date, ses empreintes ne valent plus rien.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey {
    /// Numéro de série du volume.
    pub volume: u64,
    /// Identifiant du fichier sur ce volume.
    pub file_id: FileId,
    /// Taille logique.
    pub size: u64,
    /// Date de dernière modification.
    pub modified: Timestamp,
}

/// Empreintes connues d'un fichier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CachedDigests {
    /// Empreinte de l'extrait.
    pub extract: Option<Digest>,
    /// Empreinte du fichier entier.
    pub full: Option<Digest>,
}

/// Cache d'empreintes, partagé entre les fils d'exécution et gardé d'une
/// analyse à l'autre.
pub trait DigestCache: Send + Sync + fmt::Debug {
    /// Empreintes déjà connues pour ce fichier.
    fn get(&self, key: &CacheKey) -> Option<CachedDigests>;
    /// Complète les empreintes connues pour ce fichier.
    fn put(&self, key: CacheKey, digests: CachedDigests);
}

/// Cache en mémoire : il vit le temps du processus.
#[derive(Debug, Default)]
pub struct MemoryCache {
    entries: Mutex<HashMap<CacheKey, CachedDigests>>,
}

impl MemoryCache {
    /// Crée un cache vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Nombre de fichiers connus.
    pub fn len(&self) -> usize {
        self.entries.lock().map_or(0, |entries| entries.len())
    }

    /// Vrai si le cache est vide.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl DigestCache for MemoryCache {
    fn get(&self, key: &CacheKey) -> Option<CachedDigests> {
        self.entries.lock().ok()?.get(key).copied()
    }

    fn put(&self, key: CacheKey, digests: CachedDigests) {
        if let Ok(mut entries) = self.entries.lock() {
            let known = entries.entry(key).or_default();
            known.extract = digests.extract.or(known.extract);
            known.full = digests.full.or(known.full);
        }
    }
}

/// Pas de cache : chaque analyse relit tout.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoCache;

impl DigestCache for NoCache {
    fn get(&self, _key: &CacheKey) -> Option<CachedDigests> {
        None
    }

    fn put(&self, _key: CacheKey, _digests: CachedDigests) {}
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::fs::{DirReader, Fingerprint, to_internal_path};

    fn workspace() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = to_internal_path(&std::fs::canonicalize(temp.path()).unwrap()).unwrap();
        (temp, root)
    }

    fn open(dir: &Path, name: &str) -> (VerifiedFile, u64) {
        let mut found: Option<Fingerprint> = None;
        DirReader::new()
            .read(dir, None, &mut |entry, meta| {
                if entry.to_string_lossy() == name {
                    found = Some(meta.fingerprint());
                }
            })
            .unwrap();
        let fingerprint = found.expect("fichier listé");
        (VerifiedFile::open(&dir.join(name), fingerprint).unwrap(), fingerprint.size)
    }

    fn extract_of(dir: &Path, name: &str) -> Digest {
        let (mut file, size) = open(dir, name);
        extract_digest(&mut file, size).unwrap()
    }

    fn full_of(dir: &Path, name: &str) -> Digest {
        let (mut file, _) = open(dir, name);
        full_digest(&mut file, &mut |_| {}, &AtomicBool::new(false)).unwrap().unwrap()
    }

    /// Contenu pseudo-aléatoire reproductible.
    fn noise(length: usize, seed: u64) -> Vec<u8> {
        let mut state = seed | 1;
        (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    }

    #[test]
    fn the_full_digest_is_plain_blake3() {
        let (_temp, root) = workspace();
        std::fs::write(root.join("vide"), b"").unwrap();
        std::fs::write(root.join("abc"), b"abc").unwrap();
        // Valeurs de référence de BLAKE3.
        assert_eq!(
            full_of(&root, "vide").to_hex(),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        assert_eq!(
            full_of(&root, "abc").to_hex(),
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
    }

    #[test]
    fn identical_files_share_both_digests() {
        let (_temp, root) = workspace();
        let content = noise(700_000, 7);
        std::fs::write(root.join("a.bin"), &content).unwrap();
        std::fs::write(root.join("b.bin"), &content).unwrap();
        assert_eq!(extract_of(&root, "a.bin"), extract_of(&root, "b.bin"));
        assert_eq!(full_of(&root, "a.bin"), full_of(&root, "b.bin"));
    }

    #[test]
    fn a_difference_in_the_middle_escapes_the_extract_but_not_the_full_digest() {
        let (_temp, root) = workspace();
        let content = noise(700_000, 7);
        let mut altered = content.clone();
        altered[350_000] ^= 0xFF;
        std::fs::write(root.join("a.bin"), &content).unwrap();
        std::fs::write(root.join("b.bin"), &altered).unwrap();
        assert_eq!(extract_of(&root, "a.bin"), extract_of(&root, "b.bin"));
        assert_ne!(full_of(&root, "a.bin"), full_of(&root, "b.bin"));
    }

    #[test]
    fn the_extract_sees_the_start_and_the_end() {
        let (_temp, root) = workspace();
        let content = noise(700_000, 7);
        let mut head = content.clone();
        head[10] ^= 1;
        let mut tail = content.clone();
        tail[699_990] ^= 1;
        std::fs::write(root.join("a.bin"), &content).unwrap();
        std::fs::write(root.join("tête.bin"), &head).unwrap();
        std::fs::write(root.join("queue.bin"), &tail).unwrap();
        assert_ne!(extract_of(&root, "a.bin"), extract_of(&root, "tête.bin"));
        assert_ne!(extract_of(&root, "a.bin"), extract_of(&root, "queue.bin"));
    }

    #[test]
    fn small_files_are_covered_entirely_by_the_extract() {
        let (_temp, root) = workspace();
        assert!(extract_covers(2 * EXTRACT_BYTES));
        assert!(!extract_covers(2 * EXTRACT_BYTES + 1));
        // Juste à la limite : une différence au milieu est vue.
        let content = noise(2 * EXTRACT_BYTES as usize, 3);
        let mut altered = content.clone();
        altered[EXTRACT_BYTES as usize] ^= 1;
        std::fs::write(root.join("a.bin"), &content).unwrap();
        std::fs::write(root.join("b.bin"), &altered).unwrap();
        std::fs::write(root.join("vide.bin"), b"").unwrap();
        assert_ne!(extract_of(&root, "a.bin"), extract_of(&root, "b.bin"));
        // L'extrait est alors le fichier : une seule empreinte, celle de BLAKE3.
        for name in ["a.bin", "b.bin", "vide.bin"] {
            assert_eq!(extract_of(&root, name), full_of(&root, name), "{name}");
        }
        // Un octet de plus, et l'extrait n'est plus qu'un extrait.
        std::fs::write(root.join("c.bin"), noise(2 * EXTRACT_BYTES as usize + 1, 3)).unwrap();
        assert_ne!(extract_of(&root, "c.bin"), full_of(&root, "c.bin"));
    }

    #[test]
    fn the_extract_depends_on_the_size() {
        // Même début, même fin, tailles différentes : empreintes différentes.
        let (_temp, root) = workspace();
        std::fs::write(root.join("court.bin"), vec![0u8; 300_000]).unwrap();
        std::fs::write(root.join("long.bin"), vec![0u8; 400_000]).unwrap();
        assert_ne!(extract_of(&root, "court.bin"), extract_of(&root, "long.bin"));
    }

    #[test]
    fn hashing_reports_progress_and_can_be_cancelled() {
        let (_temp, root) = workspace();
        std::fs::write(root.join("gros.bin"), noise(3 * READ_BYTES + 5, 9)).unwrap();
        let (mut file, size) = open(&root, "gros.bin");
        let mut read = 0;
        let digest =
            full_digest(&mut file, &mut |bytes| read += bytes, &AtomicBool::new(false)).unwrap();
        assert!(digest.is_some());
        assert_eq!(read, size);

        let (mut file, _) = open(&root, "gros.bin");
        let cancelled = full_digest(&mut file, &mut |_| {}, &AtomicBool::new(true)).unwrap();
        assert_eq!(cancelled, None);
    }

    #[test]
    fn the_cache_completes_what_it_knows() {
        let cache = MemoryCache::new();
        let key = CacheKey {
            volume: 1,
            file_id: FileId(42),
            size: 10,
            modified: Timestamp::from_unix(5, 0),
        };
        assert_eq!(cache.get(&key), None);
        let extract = Digest([1; 32]);
        let full = Digest([2; 32]);
        cache.put(key, CachedDigests { extract: Some(extract), full: None });
        cache.put(key, CachedDigests { extract: None, full: Some(full) });
        assert_eq!(
            cache.get(&key),
            Some(CachedDigests { extract: Some(extract), full: Some(full) })
        );
        assert_eq!(cache.len(), 1);
        // Une autre date : un autre fichier, du point de vue du cache.
        let later = CacheKey { modified: Timestamp::from_unix(6, 0), ..key };
        assert_eq!(cache.get(&later), None);
        assert_eq!(NoCache.get(&key), None);
    }
}
