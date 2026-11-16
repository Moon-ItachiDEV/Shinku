//! Quelle commande lancer pour désinstaller une app.
//!
//! Ce sont des fonctions pures : la machine n'est interrogée que par
//! [`AppsHost`] (variables, fichiers, dossier système). Rien n'est lancé ici.
//!
//! **Shinku n'ajoute aucune option** (`/S`, `/quiet`, `/norestart`…) : il lance
//! la ligne que l'éditeur a écrite, ou, pour Windows Installer, la commande
//! standard de retrait d'un produit. C'est le désinstalleur de l'éditeur qui
//! travaille, à l'écran.
//!
//! Les règles, dans l'ordre :
//!
//! 1. **Windows Installer** : `WindowsInstaller` vaut 1 **et** le nom de la
//!    sous-clé est un GUID entre accolades. Alors `msiexec.exe` du dossier
//!    système, avec `/x {GUID}`. `UninstallString` est ignorée : pour ces apps
//!    elle vaut souvent `/I`, qui modifie au lieu de retirer ;
//! 2. **sinon `UninstallString`**, développée et découpée comme le ferait
//!    `CreateProcess` : entre guillemets, exactement ce texte (`.exe` essayé en
//!    plus) ; sans guillemets, le plus court préfixe qui est un fichier. Un
//!    programme **sans dossier** (`MsiExec.exe /X{…}`, `RunDll32 …`) n'est
//!    cherché que dans le dossier système, puis dans le dossier de Windows,
//!    jamais ailleurs : c'est toujours un chemin complet qui est lancé ;
//! 3. **inutilisable** si la ligne est vide, si aucun fichier n'est trouvé, si
//!    le chemin est réseau ou sur un disque qui n'est pas fixe, si l'existence
//!    ne peut pas être dite, si plus de 64 préfixes seraient à essayer, ou si le
//!    fichier trouvé ne finit pas par `.exe`, `.com`, `.bat` ou `.cmd` : le
//!    shell ouvrirait n'importe quel autre fichier selon son association (un
//!    `.msi` lancerait une installation).
//!
//! L'élévation (`elevated`) : vraie pour les inscriptions du PC, jamais pour
//! celles du compte. D'après plusieurs sources tierces, Windows lance élevé le
//! désinstalleur d'une inscription du PC sans regarder son manifeste ; lancé
//! sans droits, un désinstalleur qui ne s'élève pas tout seul retirerait l'app
//! à moitié. Une inscription du compte peut être écrite par n'importe quel
//! programme du compte : on ne l'élève jamais. C'est toujours Windows qui
//! demande l'accord, par sa fenêtre ; Shinku reste sans droits.

use crate::apps::Source;
use crate::apps::host::{AppsHost, Existence, RawRegistration};
use crate::apps::paths::is_absolute;

/// Nombre maximal de préfixes essayés pour une ligne sans guillemets.
const MAX_PREFIXES: usize = 64;

/// Les seules extensions que le shell lance comme des programmes.
const PROGRAM_EXTENSIONS: [&str; 4] = [".exe", ".com", ".bat", ".cmd"];

/// Ce que Shinku lancera pour désinstaller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Lancer ce programme (chemin complet) avec ces paramètres, tels quels.
    Run {
        /// Le chemin complet du programme.
        program: String,
        /// Le reste de la ligne, sans rien ajouter ni retirer.
        parameters: String,
        /// Avec la demande de droits de Windows.
        elevated: bool,
        /// C'est Windows Installer : son processus dure jusqu'à la fin, on peut
        /// donc le suivre.
        installer: bool,
    },
    /// Rien d'assez sûr à lancer.
    Unusable,
}

/// Vrai pour un GUID entre accolades : `{8-4-4-4-12}` en hexadécimal.
pub fn is_guid(key: &str) -> bool {
    let Some(inner) = key.strip_prefix('{').and_then(|rest| rest.strip_suffix('}')) else {
        return false;
    };
    let parts: Vec<&str> = inner.split('-').collect();
    parts.len() == 5
        && parts.iter().zip([8, 4, 4, 4, 12]).all(|(part, length)| {
            part.len() == length && part.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

/// Ce que cherche une recherche de programme.
enum Found {
    /// Le premier chemin qui est un fichier.
    File(String),
    /// Aucun fichier, avec certitude.
    Nothing,
    /// Impossible de conclure : lecteur qui n'est pas fixe, accès refusé.
    Unsure,
}

/// Cherche ce candidat : tel quel puis avec `.exe`. Un chemin absolu doit être
/// sur un lecteur fixe. Un nom sans dossier (si `bare_allowed`) n'est cherché
/// que dans le dossier système puis dans celui de Windows.
fn find(host: &dyn AppsHost, candidate: &str, bare_allowed: bool) -> Found {
    let folders: Vec<String> = if is_absolute(candidate) {
        if !host.is_local_fixed(candidate) {
            return Found::Unsure;
        }
        vec![String::new()]
    } else if bare_allowed && !candidate.contains(['\\', '/', ':']) {
        match host.system_dirs() {
            Some(dirs) => vec![format!("{}\\", dirs.system), format!("{}\\", dirs.windows)],
            None => return Found::Unsure,
        }
    } else {
        return Found::Nothing;
    };
    for folder in folders {
        for path in [format!("{folder}{candidate}"), format!("{folder}{candidate}.exe")] {
            match host.existence(&path) {
                Existence::File => return Found::File(path),
                Existence::Unsure => return Found::Unsure,
                Existence::Folder | Existence::Absent => {}
            }
        }
    }
    Found::Nothing
}

/// Vrai si le fichier a l'une des extensions que le shell lance comme un programme.
fn is_program(path: &str) -> bool {
    let lower = path.to_lowercase();
    PROGRAM_EXTENSIONS.iter().any(|extension| lower.ends_with(extension))
}

/// La commande à lancer pour désinstaller cette inscription.
pub fn plan(
    registration: &RawRegistration,
    source: Source,
    key: &str,
    host: &dyn AppsHost,
) -> Plan {
    // 1. Windows Installer : msiexec du dossier système, jamais UninstallString.
    if registration.windows_installer == Some(1) && is_guid(key) {
        let Some(dirs) = host.system_dirs() else { return Plan::Unusable };
        let program = format!("{}\\msiexec.exe", dirs.system);
        return if host.existence(&program) == Existence::File {
            Plan::Run { program, parameters: format!("/x {key}"), elevated: false, installer: true }
        } else {
            Plan::Unusable
        };
    }

    // 2. La ligne de l'éditeur.
    let Some(line) = registration.uninstall_string.as_deref() else { return Plan::Unusable };
    let expanded = host.expand(line);
    let text = expanded.trim();
    if text.is_empty() {
        return Plan::Unusable;
    }
    let (program, parameters) = if let Some(quoted) = text.strip_prefix('"') {
        let end = quoted.find('"').unwrap_or(quoted.len());
        let candidate = quoted[..end].trim();
        if candidate.is_empty() {
            return Plan::Unusable;
        }
        match find(host, candidate, true) {
            Found::File(path) => {
                (path, quoted.get(end + 1..).unwrap_or_default().trim_start().to_owned())
            }
            Found::Nothing | Found::Unsure => return Plan::Unusable,
        }
    } else {
        match find_unquoted(host, text) {
            Some(found) => found,
            None => return Plan::Unusable,
        }
    };
    if !is_program(&program) {
        return Plan::Unusable;
    }
    Plan::Run { program, parameters, elevated: source != Source::Account, installer: false }
}

/// Une ligne sans guillemets : le plus court préfixe, coupé aux blancs, qui est
/// un fichier. Rend le fichier et le reste de la ligne.
fn find_unquoted(host: &dyn AppsHost, text: &str) -> Option<(String, String)> {
    let mut tried = 0usize;
    let mut previous = "";
    let cuts = text
        .char_indices()
        .filter(|(at, letter)| letter.is_whitespace() && *at > 0)
        .map(|(at, _)| at)
        .chain(std::iter::once(text.len()));
    for at in cuts {
        let prefix = text[..at].trim_end();
        if prefix.is_empty() || prefix == previous {
            continue;
        }
        previous = prefix;
        tried += 1;
        if tried > MAX_PREFIXES {
            return None;
        }
        // Seul le premier mot peut être un nom sans dossier.
        match find(host, prefix, tried == 1) {
            Found::File(path) => return Some((path, text[prefix.len()..].trim_start().to_owned())),
            Found::Unsure => return None,
            Found::Nothing => {}
        }
    }
    None
}
