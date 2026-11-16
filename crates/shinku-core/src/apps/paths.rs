//! Des chemins Windows, comparés **comme des textes** : aucune lecture du
//! disque, aucune résolution. Un chemin est comparé sans tenir compte de la
//! casse, avec `\` pour séparateur.
//!
//! Ces fonctions décident de la **forme** d'un chemin ; ce qu'il y a sur le
//! disque est l'affaire de la machine ([`AppsHost`]).

use crate::apps::host::AppsHost;

/// Vrai pour `C:\…` ou `C:/…` : une lettre de lecteur, jamais un chemin réseau.
pub fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// Le chemin plié pour la comparaison : minuscules, `\` partout, sans barre finale.
pub fn norm(path: &str) -> String {
    let mut text = path.replace('/', "\\").to_lowercase();
    while text.ends_with('\\') && text.len() > 3 {
        text.pop();
    }
    text
}

/// Vrai si `child` est **strictement** dans `parent` (comparaison de textes).
pub fn is_inside(child: &str, parent: &str) -> bool {
    let (child, parent) = (norm(child), norm(parent));
    let parent = parent.trim_end_matches('\\');
    child.len() > parent.len() + 1
        && child.starts_with(parent)
        && child.as_bytes()[parent.len()] == b'\\'
}

/// Vrai si les deux chemins désignent le même, ou si `child` est dedans.
pub fn is_same_or_inside(child: &str, parent: &str) -> bool {
    norm(child) == norm(parent) || is_inside(child, parent)
}

/// Le dernier nom d'un chemin.
pub fn last_name(path: &str) -> &str {
    path.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Le dossier qui contient ce chemin (`C:\` pour un dossier de la racine).
pub fn parent(path: &str) -> Option<&str> {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let cut = trimmed.rfind(['\\', '/'])?;
    // Sous la racine d'un lecteur, le parent est la racine elle-même (`C:\`).
    Some(if cut <= 2 { &trimmed[..3] } else { &trimmed[..cut] })
}

/// Vrai si la forme de ce chemin est sûre : absolu, sans `.`, `..` ni `~` (nom
/// court), sans nom qui se termine par un point ou une espace (Windows le
/// lirait comme un autre nom), sans caractère interdit dans un nom.
pub fn is_plain(path: &str) -> bool {
    if !is_absolute(path) || path.contains('~') || path.contains('\0') {
        return false;
    }
    path[3..].split(['\\', '/']).all(|part| {
        // Un nom vide (`\\`) n'a pas de sens : le chemin n'est pas propre.
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part.ends_with(['.', ' '])
            && !part.contains(['<', '>', '"', '|', '?', '*', ':'])
    }) || path[3..].is_empty()
}

/// L'emplacement d'installation lu dans le registre, prêt à servir : développé,
/// sans la paire de guillemets qui l'entoure, sans barres finales, et **absent**
/// (`None`) s'il n'est pas un chemin propre d'un lecteur fixe. Un lecteur
/// racine seul (`C:\`) n'est pas un emplacement d'app.
pub fn clean_location(text: &str, host: &dyn AppsHost) -> Option<String> {
    let expanded = host.expand(text);
    let mut text = expanded.trim();
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        text = text[1..text.len() - 1].trim();
    }
    let text = text.trim_end_matches(['\\', '/']);
    if text.is_empty() || text.contains('%') || !is_plain(text) {
        return None;
    }
    host.is_local_fixed(text).then(|| text.replace('/', "\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_drive_letter_path_is_absolute() {
        assert!(is_absolute(r"C:\Users"));
        assert!(is_absolute("D:/Jeux"));
        assert!(!is_absolute(r"\\serveur\partage"));
        assert!(!is_absolute(r"\\?\C:\Users"));
        assert!(!is_absolute(r"C:Users"));
        assert!(!is_absolute("Users"));
        assert!(!is_absolute(""));
    }

    #[test]
    fn inside_is_strict_and_whole_names_only() {
        assert!(is_inside(r"C:\A\B", r"c:\a"));
        assert!(is_inside(r"C:\A\B\", r"C:\A"));
        assert!(!is_inside(r"C:\A", r"C:\A"));
        assert!(!is_inside(r"C:\AB", r"C:\A"), "un nom qui commence pareil n'est pas dedans");
        assert!(is_same_or_inside(r"C:\A", r"c:\a\"));
        assert!(is_inside(r"C:\A", r"C:\"));
    }

    #[test]
    fn names_and_parents_are_read_from_the_end() {
        assert_eq!(last_name(r"C:\Users\Léa\AppData\Local\Discord"), "Discord");
        assert_eq!(last_name(r"C:\Users\Léa\AppData\Local\Discord\"), "Discord");
        assert_eq!(parent(r"C:\Users\Léa"), Some(r"C:\Users"));
        assert_eq!(parent(r"C:\Users"), Some(r"C:\"));
        assert_eq!(parent(r"C:\"), None);
    }

    #[test]
    fn a_plain_path_has_no_dots_no_short_names_no_odd_names() {
        assert!(is_plain(r"C:\Users\Léa\AppData\Local\Python 3.11"));
        assert!(is_plain(r"D:\Jeux\Hades"));
        assert!(!is_plain(r"C:\a\..\b"));
        assert!(!is_plain(r"C:\a\.\b"));
        assert!(!is_plain(r"C:\PROGRA~1\App"));
        assert!(!is_plain(r"C:\a\dossier."));
        assert!(!is_plain(r"C:\a\dossier "));
        assert!(!is_plain(r"C:\a\b?"));
        assert!(!is_plain(r"C:\a\\b"));
        assert!(!is_plain(r"\\serveur\partage\x"));
        assert!(!is_plain("relatif"));
    }
}
