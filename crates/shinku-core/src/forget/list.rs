//! La liste fermée des fichiers de réglages et de journaux de Shinku.
//!
//! Elle est faite des **constantes de nom** des modules qui écrivent ces
//! fichiers (`settings`, `clean::journal`, `game::store`, `startup::store`,
//! `apps::notes`) et
//! du nom de leur voisin temporaire (`write_replacing` écrit à côté, puis met en
//! place : une coupure laisse un `.tmp`). Aucun nom n'est recopié ici.
//!
//! `--oublier` n'efface rien d'autre dans ces dossiers : un fichier qui n'est
//! pas dans la liste y reste, et le dossier `Shinku` avec lui.

use std::ffi::OsString;

use crate::platform::KnownFolder;
use crate::settings::temporary_name;
use crate::{apps, clean, game, settings, startup};

/// Un dossier `Shinku` et les fichiers que Shinku y écrit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// Le dossier connu dans lequel vit le dossier de Shinku.
    pub folder: KnownFolder,
    /// Le nom du dossier de Shinku.
    pub dir: &'static str,
    /// Les noms de fichiers, voisins `.tmp` compris.
    pub files: Vec<OsString>,
}

/// Un fichier écrit par `write_replacing`, avec son voisin temporaire.
fn written(name: &str) -> [OsString; 2] {
    [OsString::from(name), temporary_name(name.as_ref())]
}

/// Les deux dossiers de Shinku, avec ce qu'ils contiennent : d'abord
/// `%APPDATA%\Shinku` (réglages et journal), puis `%LOCALAPPDATA%\Shinku` (mode
/// jeu, démarrage et désinstallations notées).
pub fn closed_list() -> [Group; 2] {
    let mut roaming: Vec<OsString> = Vec::new();
    roaming.extend(written(settings::FILE_NAME));
    // Le fichier illisible est mis de côté par un `rename` : il n'a pas de `.tmp`.
    roaming.push(OsString::from(settings::UNREADABLE_NAME));
    roaming.extend(written(clean::journal::FILE_NAME));

    let mut local: Vec<OsString> = Vec::new();
    local.extend(written(game::store::FILE_NAME));
    local.extend(written(startup::store::FILE_NAME));
    local.extend(written(apps::notes::FILE_NAME));

    [
        Group { folder: KnownFolder::RoamingAppData, dir: settings::APP_DIR, files: roaming },
        Group { folder: KnownFolder::LocalAppData, dir: game::store::APP_DIR, files: local },
    ]
}
