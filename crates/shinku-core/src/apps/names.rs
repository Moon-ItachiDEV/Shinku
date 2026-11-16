//! Les noms : de quoi décider qu'un dossier « porte le nom » d'une app.
//!
//! Tout repose sur deux valeurs que beaucoup d'installeurs n'écrivent pas, ou
//! écrivent mal : `InstallLocation` et le nom affiché. D'où une règle d'ensemble
//! : un dossier n'est proposé que s'il porte **exactement** le nom nu de l'app.
//! Manquer un reste est sans gravité ; proposer le dossier d'une autre app ne
//! l'est pas.
//!
//! Les noms sont **pliés par la casse seulement** (`é` ne devient pas `e`) : un
//! pliage plus large ferait ressembler deux noms distincts, et ce module préfère
//! rater un reste que se tromper de dossier.

/// Les noms trop communs pour désigner une app : un dossier qui les porte est
/// un dossier de Windows ou un dossier partagé, pas celui d'une app.
const COMMON: [&str; 46] = [
    "microsoft",
    "windows",
    "google",
    "mozilla",
    "adobe",
    "intel",
    "nvidia",
    "amd",
    "apple",
    "packages",
    "programs",
    "temp",
    "tmp",
    "cache",
    "caches",
    "common",
    "data",
    "update",
    "updates",
    "logs",
    "crashdumps",
    "apps",
    "app",
    "application",
    "local",
    "roaming",
    "locallow",
    "user",
    "default",
    "setup",
    "install",
    "installer",
    "uninstall",
    "launcher",
    "player",
    "client",
    "server",
    "tools",
    "games",
    "jeux",
    "bin",
    "current",
    "x64",
    "x86",
    "win32",
    "win64",
];

/// Les derniers mots qui disent l'architecture, jamais le nom d'une app.
const ARCHITECTURES: [&str; 6] = ["x64", "x86", "64-bit", "32-bit", "64bit", "32bit"];

/// Les mots qui terminent le nom d'une société, retirés pour comparer le nom de
/// l'éditeur à un dossier.
const COMPANY_SUFFIXES: [&str; 9] =
    ["inc", "llc", "ltd", "gmbh", "corporation", "corp", "s.a", "sas", "ab"];

/// Un nom plié pour la comparaison : en minuscules.
pub fn fold(text: &str) -> String {
    text.to_lowercase()
}

/// Un texte plié pour le tri et la recherche : minuscules, et les lettres
/// accentuées du français et des langues voisines ramenées à leur lettre.
pub fn fold_loose(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|letter| match letter {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            other => other,
        })
        .collect()
}

/// Vrai pour un mot fait de chiffres et de points, avec au moins un chiffre, et
/// éventuellement un `v` collé devant : `3.11.4`, `v2`, `105.0.4970.21`.
fn is_version_word(word: &str) -> bool {
    let digits = word.strip_prefix('v').unwrap_or(word);
    digits.chars().any(|letter| letter.is_ascii_digit())
        && digits.chars().all(|letter| letter.is_ascii_digit() || letter == '.')
}

/// Le nom nu d'une app : son nom plié, privé, tant que quelque chose change :
///
/// - d'un groupe final entre parenthèses (`(64-bit)`, `(x64 fr)`, `(User)`) ;
/// - d'un dernier mot fait de chiffres et de points, précédé ou non d'un `v`
///   collé, et du mot `version` qui le précède ;
/// - d'un dernier mot `x64`, `x86`, `64-bit`, `32-bit`, `64bit`, `32bit`.
///
/// On ne retire jamais le dernier mot qui reste : un nom n'est jamais vidé.
pub fn bare_name(name: &str) -> String {
    let mut text = fold(&name.split_whitespace().collect::<Vec<_>>().join(" "));
    loop {
        let before = text.len();
        // Un groupe final entre parenthèses.
        if text.ends_with(')')
            && let Some(open) = text.rfind('(')
            && !text[..open].trim().is_empty()
        {
            text.truncate(open);
            text.truncate(text.trim_end().len());
        }
        // Un numéro de version, et le mot `version` devant lui.
        if let Some((rest, last)) = text.rsplit_once(' ')
            && is_version_word(last)
        {
            text = rest.trim_end().to_owned();
            if let Some((rest, "version")) = text.rsplit_once(' ') {
                text = rest.trim_end().to_owned();
            }
        }
        // Une architecture.
        if let Some((rest, last)) = text.rsplit_once(' ')
            && ARCHITECTURES.contains(&last)
        {
            text = rest.trim_end().to_owned();
        }
        if text.len() == before {
            return text;
        }
    }
}

/// Vrai si ce nom nu peut désigner une app : trois caractères au moins, et pas
/// l'un des noms trop communs.
pub fn is_usable(bare: &str) -> bool {
    bare.chars().count() >= 3 && !COMMON.contains(&bare)
}

/// Le nom nu de l'app, s'il peut servir.
pub fn usable_bare(name: &str) -> Option<String> {
    let bare = bare_name(name);
    is_usable(&bare).then_some(bare)
}

/// Les mots entiers d'un nom plié : coupé aux espaces, sans la ponctuation qui
/// entoure un mot.
pub fn words(folded: &str) -> Vec<&str> {
    folded
        .split_whitespace()
        .map(|word| word.trim_matches(|letter: char| "()[]{},;:".contains(letter)))
        .filter(|word| !word.is_empty())
        .collect()
}

/// Le nom de l'éditeur, plié et privé de sa forme juridique (`Inc.`, `LLC`,
/// `Ltd`, `GmbH`, `Corporation`, `Corp.`, `S.A.`, `SAS`, `AB`) et de la
/// ponctuation finale. `None` s'il ne peut pas servir de nom de dossier.
pub fn publisher_key(publisher: &str) -> Option<String> {
    let mut text = fold(&publisher.split_whitespace().collect::<Vec<_>>().join(" "));
    loop {
        let trimmed = text.trim_end_matches([',', '.', ' ']).to_owned();
        let Some((rest, last)) = trimmed.rsplit_once(' ') else {
            text = trimmed;
            break;
        };
        if COMPANY_SUFFIXES.contains(&last) {
            text = rest.trim_end_matches([',', ' ']).to_owned();
        } else {
            text = trimmed;
            break;
        }
    }
    (is_usable(&text) && !COMPANY_SUFFIXES.contains(&text.as_str())).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_names_are_reduced_to_their_bare_name() {
        for (name, bare) in [
            ("Python 3.11.4 (64-bit)", "python"),
            ("7-Zip 23.01 (x64)", "7-zip"),
            ("Mozilla Firefox (x64 fr)", "mozilla firefox"),
            ("Git version 2.42.0", "git"),
            ("Microsoft Visual Studio Code (User)", "microsoft visual studio code"),
            ("Opera Stable 105.0.4970.21", "opera stable"),
            ("Notepad++ (64-bit x64)", "notepad++"),
            ("LibreOffice 7.6.4.1", "libreoffice"),
            ("Java 8 Update 391", "java 8 update"),
            ("Unity 2022.3.10f1", "unity 2022.3.10f1"),
            ("VLC media player", "vlc media player"),
            ("Discord", "discord"),
            ("Hades", "hades"),
            ("Foo v2.1", "foo"),
            ("Foo 64-bit", "foo"),
        ] {
            assert_eq!(bare_name(name), bare, "{name}");
        }
    }

    #[test]
    fn a_name_is_never_emptied() {
        assert_eq!(bare_name("7"), "7");
        assert_eq!(bare_name("(x64)"), "(x64)");
        assert_eq!(bare_name("2023 (64-bit)"), "2023");
        assert_eq!(bare_name(""), "");
    }

    #[test]
    fn too_short_or_too_common_names_never_serve() {
        for name in ["Ab", "Microsoft", "Google", "Windows", "Data", "Launcher", "x64", "Programs"]
        {
            assert_eq!(usable_bare(name), None, "{name}");
        }
        assert_eq!(usable_bare("Hades").as_deref(), Some("hades"));
        assert_eq!(usable_bare("7-Zip 23.01 (x64)").as_deref(), Some("7-zip"));
        assert_eq!(usable_bare("Git version 2.42.0").as_deref(), Some("git"));
    }

    #[test]
    fn a_publisher_loses_its_legal_form() {
        assert_eq!(publisher_key("Discord Inc.").as_deref(), Some("discord"));
        assert_eq!(publisher_key("Team Cherry").as_deref(), Some("team cherry"));
        assert_eq!(publisher_key("Valve Corporation").as_deref(), Some("valve"));
        assert_eq!(publisher_key("Spotify AB").as_deref(), Some("spotify"));
        assert_eq!(publisher_key("Foo, LLC").as_deref(), Some("foo"));
        assert_eq!(publisher_key("Bar S.A.").as_deref(), Some("bar"));
        assert_eq!(publisher_key("Acme GmbH Inc.").as_deref(), Some("acme"));
        // Trop commun : jamais un dossier de société à explorer.
        assert_eq!(publisher_key("Microsoft Corporation"), None);
        assert_eq!(publisher_key("Google LLC"), None);
        assert_eq!(publisher_key("Inc."), None);
    }

    #[test]
    fn loose_folding_ignores_case_and_accents() {
        assert_eq!(fold_loose("Éléphant ÇA"), "elephant ca");
        assert_eq!(fold("Éléphant"), "éléphant");
    }

    #[test]
    fn whole_words_lose_their_punctuation() {
        assert_eq!(words("vlc media player"), ["vlc", "media", "player"]);
        assert_eq!(words("foo (x64), bar"), ["foo", "x64", "bar"]);
    }
}
