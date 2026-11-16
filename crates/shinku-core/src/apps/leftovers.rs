//! Ce qu'une app a laissé dans le compte : les chercher, les revérifier, les
//! mettre à la corbeille.
//!
//! Règle d'ensemble : **un dossier n'est proposé que s'il porte le nom de
//! l'app**, et jamais tant qu'une app de la même famille est installée (voir
//! [`crate::apps::claims`]). Trois familles de restes :
//!
//! - **A, le dossier de l'app** ([`LeftoverKind::Folder`]) : l'emplacement
//!   d'installation, si son dernier nom, plié, est exactement le nom nu de
//!   l'app. `…\Local\Discord` pour « Discord », `D:\Jeux\Hades` pour « Hades » ;
//!   jamais `D:\Jeux`, `…\Local\Programs`, `C:\Windows`, un dossier de
//!   lanceur (`D:\Steam` pour un jeu) ;
//! - **B, réglages et données** ([`LeftoverKind::Data`]) : dans `%APPDATA%`,
//!   `%LOCALAPPDATA%` et `LocalLow` (pas `…\Programs` : ce qu'on y trouve est
//!   un programme), un sous-dossier direct dont le nom plié est **exactement**
//!   le nom nu (jamais « contient », jamais « commence par »), ou le dernier nom
//!   de l'emplacement s'il est aussi un mot entier du nom de l'app ; et, sous un
//!   dossier dont le nom est exactement celui de l'éditeur, un sous-dossier qui
//!   porte l'un de ces noms. Le dossier de l'éditeur lui-même n'est jamais
//!   proposé à ce titre ;
//! - **C, raccourcis** ([`LeftoverKind::Shortcut`]) : les `.lnk` du menu
//!   Démarrer et du Bureau du compte dont la cible brute (lue, jamais suivie)
//!   est dans l'emplacement, et dont la cible est **absente avec certitude**.
//!
//! Un candidat est écarté sans rien dire s'il n'est pas un chemin propre d'un
//! lecteur fixe, s'il n'existe plus, si un lien est sur son chemin, si une
//! voisine le réclame, s'il contient le dossier de `shinku.exe`, s'il est dans
//! un dossier synchronisé, si le garde le refuse (quelle que soit la raison),
//! ou, pour un dossier, si on n'a pas pu le compter en entier ou si un lien s'y
//! trouve. **Tous les restes proposés sont retirables** : ce qui ne l'est pas
//! n'est pas dans la liste.

use serde::Serialize;

use crate::apps::claims::{self, Neighbours};
use crate::apps::host::{AccountFolder, AppsHost, Existence, Measure};
use crate::apps::launch::Plan;
use crate::apps::paths::{
    clean_location, is_inside, is_plain, is_same_or_inside, last_name, norm, parent,
};
use crate::apps::{MAX_LEFTOVERS, Scope, names};
use crate::platform::{RecycleError, Sent};
use crate::startup::command::HOSTS;

/// La famille d'un reste.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LeftoverKind {
    /// Le dossier de l'app.
    Folder,
    /// Réglages et données de l'app.
    Data,
    /// Un raccourci qui ne mène plus nulle part.
    Shortcut,
}

/// Un reste proposé.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leftover {
    /// Sa clé, opaque : c'est tout ce que la page renvoie.
    pub key: u32,
    /// Sa famille.
    pub kind: LeftoverKind,
    /// Son chemin, à montrer.
    pub path: String,
    /// Sa taille, en octets.
    pub bytes: Option<u64>,
    /// Son nombre de fichiers.
    pub files: Option<u64>,
}

/// Ce que Shinku sait de l'app, noté **avant** de lancer son désinstalleur :
/// ensuite, l'inscription n'existe plus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Le nom affiché.
    pub name: String,
    /// Le nom nu (voir [`names::bare_name`]).
    pub bare: String,
    /// L'éditeur affiché.
    pub publisher: Option<String>,
    /// Le compte, ou le PC.
    pub scope: Scope,
    /// L'emplacement d'installation : `InstallLocation` nettoyé, sinon le
    /// dossier du programme du plan (sauf msiexec et les hôtes de
    /// [`HOSTS`]). Jamais sous le dossier de Windows.
    pub location: Option<String>,
}

impl Snapshot {
    /// Prend la photo de cette inscription.
    pub fn take(
        raw: &crate::apps::host::RawRegistration,
        name: &str,
        publisher: Option<&str>,
        scope: Scope,
        plan: &Plan,
        host: &dyn AppsHost,
    ) -> Self {
        let declared = raw.install_location.as_deref().and_then(|text| clean_location(text, host));
        let location = declared.or_else(|| match plan {
            Plan::Run { program, .. } => {
                let file = last_name(program).to_lowercase();
                if HOSTS.contains(&file.as_str()) {
                    return None;
                }
                parent(program).and_then(|folder| clean_location(folder, host))
            }
            Plan::Unusable => None,
        });
        // Le dossier de Windows n'est l'emplacement d'aucune app.
        let location = location.filter(|location| {
            host.system_dirs().is_none_or(|dirs| !is_same_or_inside(location, &dirs.windows))
        });
        Self {
            name: name.to_owned(),
            bare: names::bare_name(name),
            publisher: publisher.map(str::to_owned),
            scope,
            location,
        }
    }

    /// Le nom nu, s'il peut servir à reconnaître un dossier.
    pub fn usable_bare(&self) -> Option<&str> {
        names::is_usable(&self.bare).then_some(self.bare.as_str())
    }
}

/// Ce que la recherche a trouvé.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Les restes proposés.
    pub leftovers: Vec<Leftover>,
    /// Une app de la même famille est encore installée : rien n'est proposé.
    pub family: bool,
    /// Faux si les inscriptions n'ont pas pu être relues en entier : rien n'est
    /// proposé non plus.
    pub complete: bool,
}

/// Ce que vaut un chemin à l'instant où on le regarde.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    /// N'existe plus.
    Gone,
    /// À ne pas proposer, ni retirer.
    No,
    /// Retirable ; sa taille.
    Fine(Option<Measure>),
}

/// Ce qui juge un candidat : la machine, les voisines, et le dossier de Shinku.
struct Judge<'a> {
    host: &'a dyn AppsHost,
    neighbours: &'a Neighbours,
    own_dir: Option<String>,
}

impl Judge<'_> {
    /// Un dossier ou un raccourci est-il à proposer, ou à retirer ?
    fn verdict(&self, path: &str, kind: LeftoverKind) -> Verdict {
        let host = self.host;
        // Sa forme d'abord : aucun accès au disque pour un chemin douteux.
        if !is_plain(path) || !host.is_local_fixed(path) {
            return Verdict::No;
        }
        let is_folder = kind != LeftoverKind::Shortcut;
        match (host.existence(path), is_folder) {
            (Existence::Absent, _) => return Verdict::Gone,
            (Existence::Folder, true) | (Existence::File, false) => {}
            _ => return Verdict::No,
        }
        // Un lien n'importe où sur le chemin, pas seulement au bout.
        let chain_end = if is_folder { Some(path) } else { parent(path) };
        if !chain_end.is_some_and(|end| host.plain_chain(end)) {
            return Verdict::No;
        }
        if self.neighbours.claims(path, host) {
            return Verdict::No;
        }
        if let Some(own) = &self.own_dir
            && (is_same_or_inside(own, path) || is_inside(path, own))
        {
            return Verdict::No;
        }
        if host.in_sync_root(path) || !host.root_allowed(path) {
            return Verdict::No;
        }
        let measure = host.measure(path);
        if is_folder && !measure.is_some_and(|measure| !measure.has_link) {
            return Verdict::No;
        }
        Verdict::Fine(measure)
    }
}

/// Vrai si le dernier nom est un mot qui peut désigner l'app : plus qu'un
/// numéro, pas un nom trop commun.
fn is_meaningful_word(word: &str) -> bool {
    names::is_usable(word) && word.chars().any(char::is_alphabetic)
}

/// Les noms qu'un dossier de réglages peut porter : le nom nu, et le dernier
/// nom de l'emplacement s'il est aussi un mot entier du nom de l'app (`VLC`
/// pour « VLC media player » ; pas `Application`, `bin`, `x64`, un numéro).
fn data_names(snapshot: &Snapshot) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    if let Some(bare) = snapshot.usable_bare() {
        found.push(bare.to_owned());
    }
    if let Some(location) = &snapshot.location {
        let last = names::fold(last_name(location));
        let folded_name = names::fold(&snapshot.name);
        if is_meaningful_word(&last)
            && names::words(&folded_name).contains(&last.as_str())
            && !found.contains(&last)
        {
            found.push(last);
        }
    }
    found
}

/// Vrai si ce chemin est l'emplacement de l'app, le contient, ou est dedans :
/// seul A peut proposer l'emplacement, avec sa preuve par le nom.
fn touches_location(path: &str, snapshot: &Snapshot) -> bool {
    snapshot.location.as_deref().is_some_and(|location| {
        norm(path) == norm(location) || is_inside(path, location) || is_inside(location, path)
    })
}

/// Les candidats, dans l'ordre : A, B, C. Pas encore jugés.
fn candidates(
    host: &dyn AppsHost,
    snapshot: &Snapshot,
    neighbours: &Neighbours,
) -> Vec<(LeftoverKind, String)> {
    let mut found: Vec<(LeftoverKind, String)> = Vec::new();

    // A. Le dossier de l'app : son dernier nom est exactement le nom nu.
    if let (Some(bare), Some(location)) = (snapshot.usable_bare(), &snapshot.location)
        && names::fold(last_name(location)) == bare
    {
        found.push((LeftoverKind::Folder, location.clone()));
    }

    // B. Réglages et données.
    // La famille vaut pour chaque nom : un nom dont une voisine est de la
    // famille est abandonné (le nom nu, lui, a déjà été vérifié).
    let wanted: Vec<String> =
        data_names(snapshot).into_iter().filter(|name| !neighbours.has_family(name)).collect();
    if !wanted.is_empty() {
        let publisher = snapshot.publisher.as_deref().and_then(names::publisher_key);
        for folder in [AccountFolder::Roaming, AccountFolder::Local, AccountFolder::LocalLow] {
            let Some(root) = host.account_folder(folder) else { continue };
            let Ok(children) = host.children(&root) else { continue };
            for child in children.iter().filter(|child| child.is_dir && !child.is_link) {
                let folded = names::fold(&child.name);
                let path = format!("{}\\{}", root.trim_end_matches('\\'), child.name);
                if wanted.contains(&folded) && !touches_location(&path, snapshot) {
                    found.push((LeftoverKind::Data, path.clone()));
                }
                // Sous le dossier de l'éditeur : un sous-dossier au nom de l'app.
                // Le dossier de l'éditeur lui-même n'est jamais proposé à ce titre.
                if publisher.as_deref() == Some(folded.as_str())
                    && let Ok(inner) = host.children(&path)
                {
                    for sub in inner.iter().filter(|sub| sub.is_dir && !sub.is_link) {
                        let inner = format!("{path}\\{}", sub.name);
                        if wanted.contains(&names::fold(&sub.name))
                            && !touches_location(&inner, snapshot)
                        {
                            found.push((LeftoverKind::Data, inner));
                        }
                    }
                }
            }
        }
    }

    // C. Les raccourcis dont la cible était dans l'emplacement, et n'existe plus.
    if let Some(location) = &snapshot.location {
        for (folder, shallow) in [(AccountFolder::StartMenu, false), (AccountFolder::Desktop, true)]
        {
            let Some(root) = host.account_folder(folder) else { continue };
            let Ok(shortcuts) = host.shortcuts(&root) else { continue };
            for shortcut in shortcuts {
                if shallow && parent(&shortcut.path).is_none_or(|up| norm(up) != norm(&root)) {
                    continue;
                }
                let Some(raw) = shortcut.target.as_deref() else { continue };
                let target = host.expand(raw);
                let dangling = is_plain(&target)
                    && is_same_or_inside(&target, location)
                    && host.is_local_fixed(&target)
                    && host.existence(&target) == Existence::Absent;
                if dangling {
                    found.push((LeftoverKind::Shortcut, shortcut.path));
                }
            }
        }
    }

    // Une seule fois chacun ; rien dans un dossier déjà proposé.
    let mut kept: Vec<(LeftoverKind, String)> = Vec::new();
    for (kind, path) in found {
        let key = norm(&path);
        let repeated = kept.iter().any(|(_, known)| norm(known) == key);
        if !repeated {
            kept.push((kind, path));
        }
    }
    let all = kept.clone();
    kept.retain(|(_, path)| !all.iter().any(|(_, other)| is_inside(path, other)));
    kept
}

/// Cherche ce que l'app a laissé. Les clés des restes partent de `next_key`.
pub fn search(host: &dyn AppsHost, snapshot: &Snapshot, next_key: &mut u32) -> Found {
    let nothing = |family, complete| Found { leftovers: Vec::new(), family, complete };
    let Some(neighbours) = claims::read(host) else { return nothing(false, false) };
    if neighbours.has_family(&snapshot.bare) {
        return nothing(true, true);
    }
    let judge = Judge { host, neighbours: &neighbours, own_dir: host.own_dir() };
    let mut leftovers = Vec::new();
    for (kind, path) in candidates(host, snapshot, &neighbours) {
        if leftovers.len() >= MAX_LEFTOVERS {
            break;
        }
        if let Verdict::Fine(measure) = judge.verdict(&path, kind) {
            leftovers.push(Leftover {
                key: *next_key,
                kind,
                path,
                bytes: measure.map(|measure| measure.bytes),
                files: measure.map(|measure| measure.files),
            });
            *next_key = next_key.wrapping_add(1);
        }
    }
    Found { leftovers, family: false, complete: true }
}

/// Ce qui est arrivé à un reste que la personne a demandé de retirer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoveOutcome {
    /// Parti à la corbeille.
    Removed,
    /// N'existait plus.
    AlreadyGone,
    /// Shinku ne le retire pas (ce n'est plus ce qu'il avait proposé).
    Refused,
    /// La corbeille ne l'a pas pris, ou pas en entier.
    Failed,
}

/// Pourquoi la corbeille ne l'a pas pris.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoveReason {
    /// Un programme l'utilise encore.
    InUse,
    /// La corbeille ne peut pas le recevoir. Jamais de suppression définitive à la place.
    Refused,
    /// Une partie est restée.
    Partial,
    /// Windows l'a supprimé pour de bon : il n'est pas dans la corbeille.
    Destroyed,
}

/// Le sort d'un reste demandé.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveResult {
    /// La clé du reste.
    pub key: u32,
    /// Ce qui est arrivé.
    pub outcome: RemoveOutcome,
    /// Pourquoi, pour un échec de la corbeille.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<RemoveReason>,
}

/// La cible brute d'un raccourci, relue à l'instant.
fn raw_target(host: &dyn AppsHost, path: &str) -> Option<String> {
    let up = parent(path)?;
    host.shortcuts(up).ok()?.into_iter().find(|shortcut| norm(&shortcut.path) == norm(path))?.target
}

/// Met à la corbeille les restes demandés, après avoir **tout revérifié à
/// l'instant** : il existe encore, aucun lien sur son chemin, le garde
/// l'accepte, aucune voisine ne le réclame et aucune de la même famille n'est
/// apparue ; pour un dossier, il se compte encore en entier et sans lien ; pour
/// un raccourci, sa cible est toujours dans l'emplacement et toujours absente.
///
/// Les inscriptions sont relues une fois par appel : illisibles, tout est refusé.
/// Rend le sort de chaque clé demandée (une fois chacune), et le nombre
/// d'octets partis à la corbeille.
pub fn remove(
    host: &dyn AppsHost,
    snapshot: &Snapshot,
    leftovers: &[Leftover],
    keys: &[u32],
) -> (Vec<RemoveResult>, u64) {
    let mut unique: Vec<u32> = Vec::new();
    for key in keys {
        if !unique.contains(key) {
            unique.push(*key);
        }
    }
    let result = |key, outcome, reason| RemoveResult { key, outcome, reason };
    let neighbours = claims::read(host).filter(|known| !known.has_family(&snapshot.bare));
    let Some(neighbours) = neighbours else {
        let refused = unique.iter().map(|&key| result(key, RemoveOutcome::Refused, None));
        return (refused.collect(), 0);
    };
    let judge = Judge { host, neighbours: &neighbours, own_dir: host.own_dir() };

    let mut results = Vec::new();
    let mut bytes = 0u64;
    for key in unique {
        let Some(leftover) = leftovers.iter().find(|leftover| leftover.key == key) else {
            results.push(result(key, RemoveOutcome::Refused, None));
            continue;
        };
        let path = leftover.path.as_str();
        let measure = match judge.verdict(path, leftover.kind) {
            Verdict::Gone => {
                results.push(result(key, RemoveOutcome::AlreadyGone, None));
                continue;
            }
            Verdict::No => {
                results.push(result(key, RemoveOutcome::Refused, None));
                continue;
            }
            Verdict::Fine(measure) => measure,
        };
        // Un reste de réglages porte un nom qui servait à le trouver : si une
        // voisine est maintenant de la famille de ce nom, il ne part pas.
        if leftover.kind == LeftoverKind::Data
            && neighbours.has_family(&names::fold(last_name(path)))
        {
            results.push(result(key, RemoveOutcome::Refused, None));
            continue;
        }
        if leftover.kind == LeftoverKind::Shortcut && !shortcut_still_dangles(host, snapshot, path)
        {
            results.push(result(key, RemoveOutcome::Refused, None));
            continue;
        }
        match host.recycle(path) {
            // Windows dit avoir supprimé pour de bon : jamais compté comme envoyé à
            // la corbeille, et jamais cru sur parole. Le disque tranche : absent
            // avec certitude, c'est `destroyed` ; encore là, un échec sans raison.
            Ok(Sent::Destroyed) => {
                let reason =
                    (host.existence(path) == Existence::Absent).then_some(RemoveReason::Destroyed);
                results.push(result(key, RemoveOutcome::Failed, reason));
            }
            Ok(Sent::InBin) if host.existence(path) == Existence::Absent => {
                bytes = bytes.saturating_add(measure.map_or(0, |measure| measure.bytes));
                results.push(result(key, RemoveOutcome::Removed, None));
            }
            // La corbeille dit oui, mais le chemin est encore là : jamais « parti ».
            Ok(Sent::InBin) => {
                results.push(result(key, RemoveOutcome::Failed, Some(RemoveReason::Partial)));
            }
            Err(error) => {
                let reason = match error {
                    RecycleError::Locked => Some(RemoveReason::InUse),
                    RecycleError::Refused | RecycleError::Unavailable => {
                        Some(RemoveReason::Refused)
                    }
                    RecycleError::AccessDenied | RecycleError::Failed(_) => None,
                };
                results.push(result(key, RemoveOutcome::Failed, reason));
            }
        }
    }
    (results, bytes)
}

/// Un raccourci à retirer mène toujours dans l'emplacement, et nulle part.
fn shortcut_still_dangles(host: &dyn AppsHost, snapshot: &Snapshot, path: &str) -> bool {
    let (Some(location), Some(raw)) = (&snapshot.location, raw_target(host, path)) else {
        return false;
    };
    let target = host.expand(&raw);
    is_plain(&target)
        && is_same_or_inside(&target, location)
        && host.is_local_fixed(&target)
        && host.existence(&target) == Existence::Absent
}
