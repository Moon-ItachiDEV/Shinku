//! Les **voisines** : les autres apps installées, et ce qu'elles réclament.
//!
//! Au moment de chercher des restes (et de les retirer), toutes les
//! inscriptions des **quatre places** sont relues, sans le filtre de la liste :
//! les composants cachés comptent aussi. Si une place est illisible ou
//! incomplète, **aucun reste n'est proposé** : on ne sait pas à qui sont les
//! dossiers. C'est la **seule** cause d'illisibilité.
//!
//! **On ne touche jamais au disque pour une voisine.** Ce qu'elle écrit
//! (`InstallLocation`, `UninstallString`, `DisplayIcon`) est un texte, qu'on
//! compare aux écritures du **candidat**, lui qui existe et vient d'être
//! vérifié. Une voisine orpheline, une icône absente, un emplacement dans le
//! profil d'un autre compte, un lecteur réseau débranché : rien n'est lu, rien
//! n'attend, rien ne rend le PC illisible.
//!
//! Deux protections :
//!
//! - **la famille** : une voisine est de la même famille qu'un nom si son nom nu
//!   lui est égal, ou commence par lui suivi d'une espace (« Python 3.12.1
//!   (64-bit) », « Python Launcher », « Unity Hub », la nouvelle version d'une
//!   app mise à niveau pendant l'attente). La règle vaut **pour chaque nom qui
//!   sert à chercher** (le nom nu, et le dernier nom de l'emplacement) ;
//! - **ce qu'une voisine réclame**, voir [`Neighbours::claims`].
//!
//! Les **écritures d'un candidat** : chaque composant de son chemin a une
//! écriture longue (celle du candidat) et, à côté, une écriture courte
//! (`PROGRA~1`, rendue par [`AppsHost::short_path`]). Si la machine ne rend pas
//! d'écriture courte, ou si elle n'a pas le même nombre de composants, elle est
//! **inconnue**. Un composant d'un texte de voisine est admis s'il est l'une des
//! écritures de celui de même rang. Les `.` et `..` d'un texte sont résolus
//! **par le texte seul** (jamais plus haut que le lecteur).
//!
//! Une voisine réclame un candidat si :
//!
//! 1. **une ligne le mentionne** : le texte de son désinstalleur ou de son icône
//!    (tel quel, et avec `.` et `..` résolus) porte, quelque part, le chemin du
//!    candidat composant par composant, suivi d'une barre, de la fin du texte,
//!    d'un guillemet, d'un blanc ou d'une virgule. `/dir="C:\…\Hades"` réclame
//!    `Hades` ; `…\Foo2\x.exe` ne réclame pas `…\Foo` ;
//! 2. **son emplacement** est le candidat, ou dedans, ou le contient (sauf si
//!    le garde refuse cet emplacement comme racine : une inscription mal écrite
//!    ne doit pas éteindre tous les restes du PC) ;
//! 3. **le candidat n'a pas d'écriture courte connue** et qu'un texte de voisine
//!    porte un nom court (`~` suivi d'un chiffre) : dans le doute, il est écarté
//!    (ce candidat seul, jamais tout le PC).
//!
//! Limites assumées : une voisine inscrite à travers une jonction ou un `subst`
//! n'est comparée que par son texte ; une app Windows Installer sans
//! emplacement ni icône ne protège aucun dossier.

use crate::apps::host::{AppsHost, RawRegistration};
use crate::apps::paths::{is_absolute, norm};
use crate::apps::{Place, names};

/// Ce que Shinku retient d'une voisine : des textes, rien d'autre.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Neighbour {
    /// Son nom nu (vide si elle n'a pas de nom).
    bare: String,
    /// Son emplacement, développé, nettoyé, `.` et `..` résolus, plié ; `None`
    /// s'il n'est pas de la forme `X:\…`.
    location: Option<String>,
    /// Les textes de son désinstalleur et de son icône : chacun tel quel, et avec
    /// `.` et `..` résolus (développés et pliés).
    lines: Vec<String>,
    /// L'un de ses textes porte un nom court (`~` suivi d'un chiffre).
    short: bool,
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

/// Résout `.` et `..` **par le texte seul** : un segment exactement `..` entre
/// deux barres retire le précédent, un segment exactement `.` disparaît. On ne
/// remonte jamais au-dessus d'un lecteur (un segment qui finit par `:`).
fn resolve_dots(text: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for part in text.split('\\') {
        match part {
            "." => {}
            ".." => {
                if kept.last().is_some_and(|last| last.len() > 1 && !last.ends_with(':')) {
                    kept.pop();
                }
            }
            other => kept.push(other),
        }
    }
    kept.join("\\")
}

/// Une voisine, lue sans toucher au disque.
fn neighbour(raw: &RawRegistration, host: &dyn AppsHost) -> Neighbour {
    let expanded = |text: Option<&str>| host.expand(text.unwrap_or_default());
    let location_text = expanded(raw.install_location.as_deref());
    let uninstall = expanded(raw.uninstall_string.as_deref());
    let icon = expanded(raw.display_icon.as_deref());
    let short = [&location_text, &uninstall, &icon].iter().any(|text| has_short_name(text));

    let trimmed = location_text.trim_matches(|c: char| c.is_whitespace() || c == '"');
    let drive = trimmed.strip_prefix(r"\\?\").unwrap_or(trimmed);
    let drive = drive.trim_end_matches(['\\', '/']);
    let location = (!location_text.contains('%') && is_absolute(drive))
        .then(|| norm(&resolve_dots(&drive.replace('/', "\\"))))
        .filter(|text| is_absolute(text));

    let mut lines = Vec::new();
    for text in [&uninstall, &icon] {
        let folded = norm(text.trim());
        let resolved = resolve_dots(&folded);
        if resolved != folded {
            lines.push(resolved);
        }
        lines.push(folded);
    }
    Neighbour {
        bare: raw.display_name.as_deref().map(names::bare_name).unwrap_or_default(),
        location,
        lines,
        short,
    }
}

/// Lit les voisines. `None` si une place est illisible, incomplète ou trop
/// grande : c'est la seule cause. Aucun accès au disque.
pub fn read(host: &dyn AppsHost) -> Option<Neighbours> {
    let mut list = Vec::new();
    for place in Place::ALL {
        let source = host.registrations(place).ok()?;
        if !source.complete || source.entries.len() > crate::apps::MAX_KEYS {
            return None;
        }
        list.extend(source.entries.iter().map(|raw| neighbour(raw, host)));
    }
    Some(Neighbours { list })
}

/// Les écritures admises de chaque composant d'un candidat.
struct Writings {
    /// Par rang : l'écriture longue, et la courte si elle est connue.
    parts: Vec<Vec<String>>,
    /// Vrai si l'écriture courte est connue (même nombre de composants).
    short_known: bool,
}

impl Writings {
    fn of(candidate: &str, host: &dyn AppsHost) -> Self {
        let long: Vec<String> = norm(candidate).split('\\').map(str::to_owned).collect();
        let short: Option<Vec<String>> = host
            .short_path(candidate)
            .map(|short| norm(&short).split('\\').map(str::to_owned).collect())
            .filter(|short: &Vec<String>| short.len() == long.len());
        let short_known = short.is_some();
        let parts = long
            .iter()
            .enumerate()
            .map(|(rank, part)| {
                let mut writings = vec![part.clone()];
                if let Some(other) = short.as_ref().map(|short| &short[rank])
                    && other != part
                {
                    writings.push(other.clone());
                }
                writings
            })
            .collect();
        Self { parts, short_known }
    }

    /// Ce texte mentionne-t-il le candidat, composant par composant ?
    fn mentioned_in(&self, text: &str) -> bool {
        let Some(drive) = self.parts.first().and_then(|writings| writings.first()) else {
            return false;
        };
        let start = format!("{drive}\\");
        text.match_indices(&start).any(|(at, _)| self.walk(&text[at..]))
    }

    fn walk(&self, mut rest: &str) -> bool {
        for (rank, writings) in self.parts.iter().enumerate() {
            let last = rank + 1 == self.parts.len();
            let hit = writings.iter().find(|writing| {
                rest.starts_with(writing.as_str()) && {
                    let after = rest[writing.len()..].chars().next();
                    if last {
                        after.is_none_or(|c| matches!(c, '\\' | '"' | ',') || c.is_whitespace())
                    } else {
                        after == Some('\\')
                    }
                }
            });
            let Some(writing) = hit else { return false };
            rest = &rest[(writing.len() + usize::from(!last)).min(rest.len())..];
        }
        true
    }

    /// Cet emplacement (déjà plié, de la forme `x:\…`) est-il le candidat, ou
    /// dedans, ou le contient-il ? `Some(true)` : égal ou dedans ; `Some(false)` :
    /// il contient le candidat, avec le nombre de composants en commun ; `None` :
    /// sans rapport.
    fn compare(&self, location: &str) -> Option<usize> {
        let parts: Vec<&str> = location.split('\\').collect();
        let common = parts.len().min(self.parts.len());
        let same =
            (0..common).all(|rank| self.parts[rank].iter().any(|writing| writing == parts[rank]));
        same.then_some(parts.len())
    }
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

    /// Vrai si une voisine réclame ce dossier ou ce fichier (voir l'en-tête du
    /// module). Seul le candidat est interrogé sur le disque, pour son écriture
    /// courte.
    pub fn claims(&self, candidate: &str, host: &dyn AppsHost) -> bool {
        let writings = Writings::of(candidate, host);
        let original: Vec<&str> =
            candidate.trim_end_matches(['\\', '/']).split(['\\', '/']).collect();
        self.list.iter().any(|neighbour| {
            // 3. Pas d'écriture courte connue, et une voisine en nom court.
            if !writings.short_known && neighbour.short {
                return true;
            }
            // 1. Une ligne qui le mentionne.
            if neighbour.lines.iter().any(|line| writings.mentioned_in(line)) {
                return true;
            }
            // 2. Son emplacement.
            let Some(count) = neighbour.location.as_deref().and_then(|text| writings.compare(text))
            else {
                return false;
            };
            if count >= writings.parts.len() {
                return true;
            }
            // Il contient le candidat : sauf si le garde le refuse comme racine.
            // (Un emplacement `X:\` seul n'est jamais retenu : il a au moins
            // deux composants.)
            let prefix = original[..count].join("\\");
            host.root_allowed(&prefix)
        })
    }
}
