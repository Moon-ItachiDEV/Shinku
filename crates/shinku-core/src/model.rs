//! Types de base partagés par tout le cœur.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize, Serializer};

/// Instant, en centaines de nanosecondes depuis le 1er janvier 1970 (UTC).
///
/// C'est la précision des dates de fichiers de Windows. La conversion depuis
/// un `FILETIME` est donc sans perte : deux lectures de la même date donnent
/// exactement la même valeur, ce qui permet de vérifier qu'un fichier n'a pas
/// bougé entre l'analyse et le nettoyage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Le 1er janvier 1970.
    pub const UNIX_EPOCH: Self = Self(0);

    const TICKS_PER_SECOND: i64 = 10_000_000;
    const TICKS_PER_MILLI: i64 = 10_000;
    /// Écart entre le 1er janvier 1601 (origine des `FILETIME`) et 1970.
    const FILETIME_TO_UNIX: i64 = 116_444_736_000_000_000;

    /// Construit un instant à partir d'un nombre de « ticks » de 100 ns depuis 1970.
    pub const fn from_ticks(ticks: i64) -> Self {
        Self(ticks)
    }

    /// Nombre de « ticks » de 100 ns depuis 1970.
    pub const fn ticks(self) -> i64 {
        self.0
    }

    /// Convertit un `FILETIME` de Windows (100 ns depuis 1601).
    pub const fn from_filetime(filetime: i64) -> Self {
        Self(filetime.saturating_sub(Self::FILETIME_TO_UNIX))
    }

    /// Convertit une date exprimée en secondes et nanosecondes depuis 1970.
    pub const fn from_unix(seconds: i64, nanos: u32) -> Self {
        Self(seconds.saturating_mul(Self::TICKS_PER_SECOND).saturating_add(nanos as i64 / 100))
    }

    /// Convertit une date de la bibliothèque standard.
    pub fn from_system_time(time: SystemTime) -> Self {
        match time.duration_since(UNIX_EPOCH) {
            Ok(after) => Self(duration_to_ticks(after)),
            Err(before) => Self(-duration_to_ticks(before.duration())),
        }
    }

    /// L'instant présent.
    pub fn now() -> Self {
        Self::from_system_time(SystemTime::now())
    }

    /// Millisecondes depuis 1970, pour l'affichage.
    pub const fn unix_millis(self) -> i64 {
        self.0.div_euclid(Self::TICKS_PER_MILLI)
    }

    /// Recule d'une durée, sans jamais déborder.
    pub fn saturating_sub(self, duration: Duration) -> Self {
        Self(self.0.saturating_sub(duration_to_ticks(duration)))
    }

    /// Durée écoulée depuis `earlier`. Nulle si `earlier` est dans le futur.
    pub fn since(self, earlier: Self) -> Duration {
        let ticks = self.0.saturating_sub(earlier.0).max(0) as u64;
        Duration::new(ticks / 10_000_000, ((ticks % 10_000_000) * 100) as u32)
    }
}

fn duration_to_ticks(duration: Duration) -> i64 {
    i64::try_from(duration.as_nanos() / 100).unwrap_or(i64::MAX)
}

/// Côté interface, une date voyage en millisecondes depuis 1970 : c'est ce
/// qu'attend `Date` en JavaScript, et la valeur reste exacte dans un `number`.
impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(self.unix_millis())
    }
}

/// Identifiant d'un fichier sur son volume (numéro de fichier NTFS ou ReFS).
///
/// Il suit le fichier quand on le renomme ou qu'on le déplace sur le même
/// volume, et il est le même pour tous les liens physiques d'un fichier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u128);

/// Les modules de Shinku, tels qu'ils apparaissent dans le menu latéral.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModuleId {
    /// Fichiers inutiles : caches, temporaires, journaux, restes de mises à jour.
    JunkFiles,
    /// Doublons.
    Duplicates,
    /// Gros fichiers.
    LargeFiles,
    /// Corbeille.
    RecycleBin,
    /// Apps au démarrage.
    Startup,
    /// Mode jeu.
    GameMode,
    /// Désinstallation.
    Uninstall,
    /// Confidentialité.
    Privacy,
}

/// Dans quel lot du grand vide tombe une catégorie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lot {
    /// Lot automatique : coché d'office, part avec « Faire le vide ».
    Auto,
    /// Lot au choix : proposé à côté du lot automatique, jamais coché
    /// d'office. Il ne part avec « Faire le vide » que si l'utilisateur l'a
    /// coché lui-même.
    Optional,
    /// Lot « À vérifier » : jamais coché d'office, ouvre le module concerné.
    Review,
}

/// Niveau de suppression. Il fixe la méthode et le retour arrière possible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Régénérable : caches, temporaires, journaux. Suppression directe.
    A,
    /// Fichiers personnels : doublons, gros fichiers. Passe par la corbeille.
    B,
    /// Réglages : apps au démarrage, mode jeu. Journalisé, annulable.
    C,
    /// Irréversible : vidage de la corbeille, traces. Confirmation explicite.
    D,
}

/// Droits nécessaires pour une action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Rights {
    /// Droits de l'utilisateur : traité dans `shinku.exe`, sans élévation.
    User,
    /// Droits administrateur : traité par `shinku-helper.exe`, sur un verbe fixe.
    Admin,
}

/// Ce qui a déclenché une analyse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Trigger {
    /// Le bouton « Analyser » de l'Accueil.
    Button,
    /// L'analyse automatique (chaque semaine ou chaque mois).
    Automatic,
    /// « Lancer un grand vide » à la fin d'un module.
    Module,
}

/// Un nombre d'octets, avec un affichage lisible pour les journaux et les tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteSize(pub u64);

impl fmt::Display for ByteSize {
    /// Unités binaires étiquetées comme le fait l'Explorateur de Windows
    /// (1 Go = 1024 Mo), avec la virgule française.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNITS: [&str; 5] = ["octets", "Ko", "Mo", "Go", "To"];
        let mut value = self.0 as f64;
        let mut unit = 0;
        while value >= 1024.0 && unit < UNITS.len() - 1 {
            value /= 1024.0;
            unit += 1;
        }
        if unit == 0 {
            // En français, zéro et un prennent le singulier.
            return write!(f, "{} {}", self.0, if self.0 < 2 { "octet" } else { UNITS[0] });
        }
        let text = if value >= 100.0 { format!("{value:.0}") } else { format!("{value:.1}") };
        write!(f, "{} {}", text.replace('.', ","), UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_conversion_is_lossless() {
        // 1er janvier 1970 en FILETIME.
        assert_eq!(Timestamp::from_filetime(116_444_736_000_000_000), Timestamp::UNIX_EPOCH);
        // Un tick après : la précision de 100 ns est conservée.
        assert_eq!(Timestamp::from_filetime(116_444_736_000_000_001).ticks(), 1);
    }

    #[test]
    fn system_time_round_trip() {
        let time = UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_700);
        let stamp = Timestamp::from_system_time(time);
        assert_eq!(stamp, Timestamp::from_unix(1_700_000_000, 123_456_700));
        assert_eq!(stamp.unix_millis(), 1_700_000_000_123);
    }

    #[test]
    fn dates_before_1970_are_negative() {
        let time = UNIX_EPOCH - Duration::from_secs(10);
        assert_eq!(Timestamp::from_system_time(time).ticks(), -100_000_000);
        assert_eq!(Timestamp::from_system_time(time).unix_millis(), -10_000);
    }

    #[test]
    fn since_never_goes_negative() {
        let earlier = Timestamp::from_unix(100, 0);
        let later = Timestamp::from_unix(160, 500);
        assert_eq!(later.since(earlier), Duration::new(60, 500));
        assert_eq!(earlier.since(later), Duration::ZERO);
    }

    #[test]
    fn saturating_sub_does_not_overflow() {
        let stamp = Timestamp::from_ticks(i64::MIN + 5);
        assert_eq!(stamp.saturating_sub(Duration::from_secs(1)).ticks(), i64::MIN);
    }

    #[test]
    fn timestamps_serialize_as_milliseconds() {
        let stamp = Timestamp::from_unix(1_700_000_000, 999_999_999);
        assert_eq!(serde_json::to_string(&stamp).unwrap(), "1700000000999");
    }

    #[test]
    fn byte_sizes_read_like_explorer() {
        assert_eq!(ByteSize(0).to_string(), "0 octet");
        assert_eq!(ByteSize(1).to_string(), "1 octet");
        assert_eq!(ByteSize(1023).to_string(), "1023 octets");
        assert_eq!(ByteSize(1024).to_string(), "1,0 Ko");
        assert_eq!(ByteSize(3_972_844_749).to_string(), "3,7 Go");
        assert_eq!(ByteSize(150 * 1024 * 1024).to_string(), "150 Mo");
    }
}
