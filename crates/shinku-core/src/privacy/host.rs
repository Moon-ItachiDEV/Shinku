//! La machine, vue par le module « Confidentialité ».
//!
//! Comme [`crate::startup::host::StartupHost`], le trait [`PrivacyHost`] est
//! tout ce que le module sait de Windows **hors des fichiers** (qui passent par
//! [`crate::fs`], sur le vrai disque, y compris dans les essais) : les processus,
//! le verrou d'un profil, le registre, les raccourcis récents, le presse-papiers.
//! La vraie implémentation est dans `platform/windows/privacy.rs`. Les essais
//! utilisent [`crate::privacy::fake::FakePrivacyHost`].
//!
//! **Aucune méthode ne reçoit un chemin de registre** : une clé est une
//! énumération compilée ([`TypedKey`]), et un nom de valeur est celui que
//! l'énumération de cette même clé a rendu.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::privacy::rules::{TypedKey, ValueName};

/// Pourquoi une clé de registre ne se lit ou ne s'efface pas.
#[derive(Debug)]
pub enum RegistryError {
    /// La clé est un lien de registre (`SymbolicLinkValue`) : on n'y touche pas.
    Link,
    /// Autre erreur du système.
    Io(io::Error),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Link => f.write_str("la clé est un lien de registre"),
            Self::Io(error) => write!(f, "erreur du système : {error}"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Les noms des valeurs d'une clé, sans leurs données.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistryValues {
    /// Les noms lus (4096 au plus, de 255 unités au plus).
    pub names: Vec<ValueName>,
    /// Faux si une borne a laissé quelque chose de côté : la clé ne peut alors
    /// pas être donnée pour vidée.
    pub complete: bool,
}

/// Ce que le presse-papiers a fait de l'historique.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryClear {
    /// Windows a vidé l'historique (Windows+V).
    Cleared,
    /// L'historique est désactivé : le cas normal, il n'y a rien à vider.
    Disabled,
    /// `ClearHistory` a rendu faux, ou n'a pas pu être appelé.
    Failed,
}

/// Ce que le presse-papiers a fait du contenu actuel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrentClear {
    /// Le contenu actuel est vidé.
    Emptied,
    /// Une autre app tient le presse-papiers, malgré quelques essais.
    Busy,
    /// Windows a refusé.
    Failed,
}

/// Les deux gestes du presse-papiers, dans l'ordre : l'historique, puis le contenu actuel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipboardClear {
    /// L'historique.
    pub history: HistoryClear,
    /// Le contenu actuel.
    pub current: CurrentClear,
}

/// La machine, pour la « Confidentialité ».
pub trait PrivacyHost: Send + Sync + fmt::Debug {
    /// `%LOCALAPPDATA%`, sous la forme interne, s'il est défini.
    fn local_app_data(&self) -> Option<PathBuf>;

    /// Vrai si un dossier de profils Firefox existe pour ce compte.
    fn firefox_present(&self) -> bool;

    /// Les noms des programmes en cours d'exécution, **toutes sessions**.
    /// `None` : la machine ne sait pas le dire (alors tout navigateur est tenu pour ouvert).
    fn running_processes(&self) -> Option<Vec<String>>;

    /// Vrai si le verrou `lockfile` du dossier `User Data` donné est tenu par
    /// quelqu'un. Un fichier absent n'est pas tenu. Shinku ne crée jamais ce fichier.
    fn lockfile_held(&self, user_data: &Path) -> bool;

    /// Nombre de raccourcis `.lnk` posés directement dans le dossier « Récents »
    /// du compte, sans les lire. Les noms ne sont ni gardés ni rendus.
    fn recent_count(&self) -> io::Result<u32>;

    /// Efface les fichiers et dossiers récents : `SHAddToRecentDocs(SHARD_PIDL, NULL)`.
    /// L'appel ne rend rien : le résultat se relit avec [`Self::recent_count`].
    fn clear_recent(&self) -> io::Result<()>;

    /// Les noms des valeurs de cette clé de `HKCU`. `Ok(None)` : la clé n'existe pas.
    fn registry_values(&self, key: TypedKey) -> Result<Option<RegistryValues>, RegistryError>;

    /// Supprime cette valeur de cette clé. Une valeur déjà absente n'est pas une erreur.
    fn registry_delete(&self, key: TypedKey, name: &ValueName) -> Result<(), RegistryError>;

    /// Attend ce nombre de millisecondes (les essais n'attendent pas).
    fn pause(&self, millis: u64);

    /// Le presse-papiers contient quelque chose en ce moment.
    fn clipboard_filled(&self) -> io::Result<bool>;

    /// L'historique du presse-papiers (Windows+V) est activé. Shinku ne le lit pas
    /// et ne le compte pas : il sait seulement s'il existe.
    fn clipboard_history_enabled(&self) -> io::Result<bool>;

    /// Vide l'historique du presse-papiers puis son contenu actuel. `owner` est
    /// la fenêtre de Shinku (0 : aucune) ; l'appel vient d'un fil de travail, pas du fil de la fenêtre.
    fn clear_clipboard(&self, owner: isize) -> ClipboardClear;
}

/// La machine hors de Windows : elle ne voit rien, et n'efface rien.
#[derive(Clone, Copy, Debug, Default)]
pub struct InertPrivacyHost;

impl PrivacyHost for InertPrivacyHost {
    fn local_app_data(&self) -> Option<PathBuf> {
        None
    }

    fn firefox_present(&self) -> bool {
        false
    }

    fn running_processes(&self) -> Option<Vec<String>> {
        None
    }

    fn lockfile_held(&self, _user_data: &Path) -> bool {
        false
    }

    fn recent_count(&self) -> io::Result<u32> {
        Ok(0)
    }

    fn clear_recent(&self) -> io::Result<()> {
        Ok(())
    }

    fn registry_values(&self, _key: TypedKey) -> Result<Option<RegistryValues>, RegistryError> {
        Ok(None)
    }

    fn registry_delete(&self, _key: TypedKey, _name: &ValueName) -> Result<(), RegistryError> {
        Ok(())
    }

    fn pause(&self, _millis: u64) {}

    fn clipboard_filled(&self) -> io::Result<bool> {
        Ok(false)
    }

    fn clipboard_history_enabled(&self) -> io::Result<bool> {
        Ok(false)
    }

    fn clear_clipboard(&self, _owner: isize) -> ClipboardClear {
        ClipboardClear { history: HistoryClear::Disabled, current: CurrentClear::Emptied }
    }
}
