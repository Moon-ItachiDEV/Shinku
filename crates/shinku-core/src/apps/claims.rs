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
//! **Une seule mise en forme, des deux côtés** (candidat et voisines), avant
//! toute comparaison : `/` devient `\`, un préfixe `\\?\` ou `\\.\` d'un
//! emplacement est retiré, une suite de barres n'en fait qu'une, `.` et `..` sont
//! résolus par le texte. Les points et espaces **finaux** d'un composant sont
//! ignorés (Windows les ignore). Le pliage est celui de Windows, caractère par
//! caractère : la majuscule simple, sinon le caractère tel quel (`σ` et `ς` se
//! valent, `ß` reste `ß`). Un `%` dans un emplacement n'est pas un défaut : un
//! vrai dossier peut s'appeler `100% Jus`. Seule compte la forme `X:\…` après
//! développement.
//!
//! Une voisine réclame un candidat si :
//!
//! 1. **une ligne le mentionne** : le texte de son désinstalleur ou de son icône
//!    (tel quel, et avec `.` et `..` résolus) porte, quelque part, le chemin du
//!    candidat composant par composant. Après le dernier composant, la mention
//!    tient **sauf si** le caractère suivant est une lettre ou un chiffre : fin
//!    de texte, barre, guillemet, apostrophe, `;`, `)`, `=`, `|`, `:`, virgule,
//!    blanc ou point réclament (volontairement large). `-Target 'C:\…\Hades'`
//!    réclame `Hades` ; `…\Hades2\x.exe` ne réclame pas `…\Hades` ;
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
use crate::apps::paths::is_absolute;
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

/// La mise en forme commune : `/` devient `\`, un préfixe `\\?\` ou `\\.\`
/// (au début seulement) est retiré, une suite de barres n'en fait qu'une.
fn shape(text: &str) -> String {
    let slashed = text.replace('/', "\\");
    let bare =
        slashed.strip_prefix(r"\\?\").or_else(|| slashed.strip_prefix(r"\\.\")).unwrap_or(&slashed);
    let mut out = String::with_capacity(bare.len());
    for c in bare.chars() {
        if !(c == '\\' && out.ends_with('\\')) {
            out.push(c);
        }
    }
    out
}

/// Le pliage de Windows : caractère par caractère, la majuscule simple (si
/// `to_uppercase` rend exactement un caractère), sinon le caractère tel quel.
fn fold(text: &str) -> String {
    text.chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(one), None) => one,
                _ => c,
            }
        })
        .collect()
}

/// Un composant sans ses points et espaces finaux (Windows les ignore) ; le
/// composant lui-même s'il ne reste rien.
fn trim_part(part: &str) -> &str {
    let trimmed = part.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() { part } else { trimmed }
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
                if kept.last().is_some_and(|last| !last.is_empty() && !last.ends_with(':')) {
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
    let drive = shape(trimmed);
    let drive = drive.trim_end_matches('\\');
    let location =
        is_absolute(drive).then(|| fold(&resolve_dots(drive))).filter(|text| is_absolute(text));

    let mut lines = Vec::new();
    for text in [&uninstall, &icon] {
        let folded = fold(&shape(text.trim()));
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
    /// Le candidat mis en forme, **sans pliage**, par composant (pour le garde).
    original: Vec<String>,
    /// Par rang : l'écriture longue, et la courte si elle est connue (pliées,
    /// sans points ni espaces finaux).
    parts: Vec<Vec<String>>,
    /// Vrai si l'écriture courte est connue (même nombre de composants).
    short_known: bool,
}

/// Les composants d'un chemin mis en forme, sans barre finale.
fn components(path: &str) -> Vec<String> {
    shape(path).trim_end_matches('\\').split('\\').map(str::to_owned).collect()
}

impl Writings {
    fn of(candidate: &str, host: &dyn AppsHost) -> Self {
        let original = components(candidate);
        let short: Option<Vec<String>> = host
            .short_path(candidate)
            .map(|short| components(&short))
            .filter(|short| short.len() == original.len());
        let short_known = short.is_some();
        let parts = original
            .iter()
            .enumerate()
            .map(|(rank, part)| {
                let own = fold(trim_part(part));
                let mut writings = vec![own.clone()];
                if let Some(other) = short.as_ref().map(|short| fold(trim_part(&short[rank])))
                    && other != own
                {
                    writings.push(other);
                }
                writings
            })
            .collect();
        Self { original, parts, short_known }
    }

    /// Ce texte (déjà mis en forme et plié) mentionne-t-il le candidat,
    /// composant par composant ?
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
            let hit = writings.iter().find_map(|writing| {
                let after = rest.strip_prefix(writing.as_str())?;
                if last {
                    // Tout sauf une lettre ou un chiffre : réclamé.
                    return after.chars().next().is_none_or(|c| !c.is_alphanumeric()).then_some("");
                }
                // Les points et espaces finaux sont ignorés, puis une barre.
                let after = after.trim_start_matches(['.', ' ']);
                after.strip_prefix('\\')
            });
            let Some(next) = hit else { return false };
            rest = next;
        }
        true
    }

    /// Cet emplacement (déjà mis en forme et plié, de la forme `x:\…`) est-il le
    /// candidat, ou dedans, ou le contient-il ? `Some(n)` : ses `n` composants
    /// valent ceux du candidat sur la longueur commune (`n` au moins égal à ceux
    /// du candidat : égal ou dedans ; moins : il le contient) ; `None` : sans
    /// rapport.
    fn compare(&self, location: &str) -> Option<usize> {
        let parts: Vec<&str> = location.split('\\').map(trim_part).collect();
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
        let original = &writings.original;
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

/// Le pliage, pour les essais.
#[cfg(test)]
pub(super) fn tests_fold(text: &str) -> String {
    fold(text)
}
