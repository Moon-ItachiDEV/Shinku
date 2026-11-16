//! Les **voisines** : les autres apps installées, et ce qu'elles réclament.
//!
//! Au moment de chercher des restes (et de les retirer), toutes les
//! inscriptions des **quatre places** sont relues, sans le filtre de la liste :
//! les composants cachés comptent aussi. Si une place est illisible ou
//! incomplète, **aucun reste n'est proposé** : on ne sait pas à qui sont les
//! dossiers.
//!
//! Deux protections :
//!
//! - **la famille** : une voisine est de la même famille qu'un nom si son nom nu
//!   lui est égal, ou commence par lui suivi d'une espace (« Python 3.12.1
//!   (64-bit) », « Python Launcher », « Unity Hub », la nouvelle version d'une
//!   app mise à niveau pendant l'attente). La règle vaut **pour chaque nom
//!   qui sert à chercher** (le nom nu, et le dernier nom de l'emplacement) ;
//! - **ce qu'une voisine réclame** : un dossier est écarté si l'emplacement
//!   d'une voisine est égal à lui ou dedans ; si l'emplacement d'une voisine le
//!   contient (sauf si le garde refuse cet emplacement comme racine : une
//!   inscription mal écrite, `C:\` par exemple, ne doit pas éteindre tous les
//!   restes du PC) ; ou si le texte de son désinstalleur ou de son icône
//!   **contient** le chemin du dossier suivi d'une barre inverse, où que ce soit
//!   dans la ligne (`cmd /c "…\X\u.bat"`, `rundll32 "…\X\y.dll",Run`).
//!
//! **Une voisine qu'on ne sait pas lire ne protège rien, donc on le dit.** Une
//! voisine écrite en nom court (`PROGRA~1`), en `\\?\…`, avec `.` ou `..`, est
//! remise en forme longue et complète par la machine
//! ([`AppsHost::long_path`]). Pour son emplacement : si la machine n'y arrive
//! pas et que le chemin n'est pas **absent avec certitude**, on ne sait pas où
//! vit cette voisine ; pour son désinstalleur ou son icône qui contiennent un
//! nom court : si le fichier n'en est pas tiré en forme longue (le programme
//! que lancerait [`crate::apps::launch::plan`], hors hôtes comme `cmd.exe`).
//! Dans ces cas, les voisines sont **illisibles** ([`read`] rend `None`) : rien
//! n'est proposé, rien n'est retiré, et la page le dit (`complete: false`).
//! Conséquence assumée : sur un PC où une vieille app est inscrite d'une façon
//! que Shinku ne sait pas lire, il ne propose aucun reste.

use crate::apps::host::{AppsHost, Existence, RawRegistration};
use crate::apps::launch::{Plan, plan};
use crate::apps::paths::{is_absolute, is_inside, is_plain, is_same_or_inside, last_name, norm};
use crate::apps::{Place, Source, names};
use crate::startup::command::HOSTS;

/// Ce que Shinku retient d'une voisine.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Neighbour {
    /// Son nom nu (vide si elle n'a pas de nom).
    bare: String,
    /// Son emplacement d'installation, en forme longue et complète.
    location: Option<String>,
    /// Sa commande de désinstallation, développée et pliée.
    uninstall: String,
    /// Son icône, développée et pliée.
    icon: String,
    /// Les fichiers qu'on a pu tirer de sa commande ou de son icône quand ils
    /// sont écrits en nom court, en forme longue et pliés.
    files: Vec<String>,
}

/// Les voisines, lues dans les quatre places.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Neighbours {
    list: Vec<Neighbour>,
}

/// Vrai si le texte porte un nom court : `~` suivi d'un chiffre.
fn has_short_name(text: &str) -> bool {
    text.as_bytes().windows(2).any(|pair| pair[0] == b'~' && pair[1].is_ascii_digit())
}

/// Le fichier d'une icône : sans guillemets, sans l'indice final (`,0`).
fn icon_file(text: &str) -> &str {
    let text = text.trim();
    if let Some(quoted) = text.strip_prefix('"') {
        return quoted.split('"').next().unwrap_or_default().trim();
    }
    match text.rsplit_once(',') {
        Some((file, index))
            if index.trim().trim_start_matches('-').bytes().all(|b| b.is_ascii_digit()) =>
        {
            file.trim()
        }
        _ => text,
    }
}

/// L'emplacement d'une voisine. `Ok(None)` : elle ne réclame aucun dossier de
/// cette machine ; `Err` : on ne sait pas où elle vit.
fn location_of(text: &str, host: &dyn AppsHost) -> Result<Option<String>, ()> {
    let expanded = host.expand(text);
    let mut text = expanded.trim();
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        text = text[1..text.len() - 1].trim();
    }
    let text = text.trim_end_matches(['\\', '/']);
    // Sans lecteur (vide, réseau, relatif, variable inconnue) : rien d'ici.
    let drive = text.strip_prefix(r"\\?\").unwrap_or(text);
    if !is_absolute(drive) || text.contains('%') {
        return Ok(None);
    }
    if let Some(long) = host.long_path(text)
        && is_plain(&long)
    {
        return Ok(host.is_local_fixed(&long).then_some(long));
    }
    // La machine n'y arrive pas. Absent avec certitude, ou sur un disque qui
    // n'est pas fixe : aucune réclamation. Sinon, on ne sait pas.
    if !is_absolute(text) || !host.is_local_fixed(text) {
        return Ok(None);
    }
    match host.existence(text) {
        Existence::Absent => Ok(None),
        _ => Err(()),
    }
}

/// La voisine que décrit cette inscription, ou `None` si elle ne se lit pas.
fn neighbour(raw: &RawRegistration, host: &dyn AppsHost) -> Option<Neighbour> {
    let location = match raw.install_location.as_deref() {
        Some(text) => location_of(text, host).ok()?,
        None => None,
    };
    let mut files = Vec::new();
    let uninstall_text = host.expand(raw.uninstall_string.as_deref().unwrap_or_default());
    if has_short_name(&uninstall_text) {
        match plan(raw, Source::Machine64, &raw.key, host) {
            // Windows Installer : la ligne n'est pas ce qui se lance.
            Plan::Run { installer: true, .. } => {}
            Plan::Run { program, .. }
                if !HOSTS.contains(&last_name(&program).to_lowercase().as_str()) =>
            {
                files.push(norm(&host.long_path(&program).filter(|long| is_plain(long))?));
            }
            _ => return None,
        }
    }
    let icon_text = host.expand(raw.display_icon.as_deref().unwrap_or_default());
    if has_short_name(&icon_text) {
        let file = icon_file(&icon_text);
        files.push(norm(&host.long_path(file).filter(|long| is_plain(long))?));
    }
    Some(Neighbour {
        bare: raw.display_name.as_deref().map(names::bare_name).unwrap_or_default(),
        location,
        uninstall: norm(uninstall_text.trim()),
        icon: norm(icon_text.trim()),
        files,
    })
}

/// Lit les voisines. `None` si une place est illisible ou incomplète, ou si une
/// voisine est écrite d'une façon qu'on ne sait pas lire.
pub fn read(host: &dyn AppsHost) -> Option<Neighbours> {
    let mut list = Vec::new();
    for place in Place::ALL {
        let source = host.registrations(place).ok()?;
        if !source.complete || source.entries.len() > crate::apps::MAX_KEYS {
            return None;
        }
        for raw in &source.entries {
            list.push(neighbour(raw, host)?);
        }
    }
    Some(Neighbours { list })
}

impl Neighbours {
    /// Vrai si une voisine est de la même famille que ce nom (nu).
    pub fn has_family(&self, bare: &str) -> bool {
        if bare.is_empty() {
            return false;
        }
        let prefix = format!("{bare} ");
        self.list
            .iter()
            .any(|neighbour| neighbour.bare == bare || neighbour.bare.starts_with(&prefix))
    }

    /// Vrai si une voisine réclame ce dossier ou ce fichier.
    pub fn claims(&self, candidate: &str, host: &dyn AppsHost) -> bool {
        let candidate = norm(candidate);
        let inside = format!("{candidate}\\");
        self.list.iter().any(|neighbour| {
            let by_location = neighbour.location.as_deref().is_some_and(|location| {
                // Son emplacement est le candidat, ou dedans.
                is_same_or_inside(location, &candidate)
                    // Ou il le contient, sauf si le garde le refuse comme racine.
                    || (is_inside(&candidate, location) && host.root_allowed(location))
            });
            by_location
                // Le chemin du candidat, suivi d'une barre, où que ce soit dans la ligne.
                || neighbour.uninstall.contains(&inside)
                || neighbour.icon.contains(&inside)
                || neighbour.files.iter().any(|file| file.starts_with(&inside))
        })
    }
}
