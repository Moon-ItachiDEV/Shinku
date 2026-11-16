//! Le nom montré pour confirmer un changement : **une seule fonction**, pour la
//! page (le champ `registered`) et pour la boîte de l'assistant élevé.
//!
//! Elle est faite pour une **confirmation**, pas pour une liste : rien n'est
//! retiré en silence (contrairement à [`crate::startup::clean_text`]). Ce qui
//! n'est pas un caractère ordinaire est **montré** entre ⟨ ⟩ (`⟨U+202E⟩`), pour
//! qu'un nom ne puisse pas se déguiser. Les homoglyphes (une lettre grecque qui
//! ressemble à une latine) passent : c'est une limite écrite.

/// Nombre de caractères affichés au plus (avant le `…` final).
pub const SHOWN_LIMIT: usize = 80;

/// Signes courants gardés tels quels, en plus des lettres, des chiffres, de
/// l'ASCII imprimable et de l'espace.
const COMMON_SIGNS: [char; 16] =
    ['«', '»', '’', '‘', '“', '”', '–', '—', '·', '…', '®', '©', '™', '°', '€', '£'];

/// Lettres invisibles connues (remplissages de Hangul) : montrées, même si
/// Unicode les range parmi les lettres.
const INVISIBLE_LETTERS: [u32; 4] = [0x115F, 0x1160, 0x3164, 0xFFA0];

/// Un nom prêt à être montré.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownName {
    /// Le texte à afficher.
    pub text: String,
    /// Vrai si quelque chose a été montré entre ⟨ ⟩ ou coupé.
    pub altered: bool,
}

fn is_kept(character: char) -> bool {
    if INVISIBLE_LETTERS.contains(&u32::from(character)) {
        return false;
    }
    character.is_alphanumeric()
        || ('\u{20}'..='\u{7E}').contains(&character)
        || COMMON_SIGNS.contains(&character)
}

/// Le nom, mis en forme pour une confirmation.
pub fn shown_name(name: &str) -> ShownName {
    let mut text = String::new();
    let mut shown = 0usize;
    let mut altered = false;
    for character in name.chars() {
        let piece = if is_kept(character) {
            character.to_string()
        } else {
            altered = true;
            format!("⟨U+{:04X}⟩", u32::from(character))
        };
        let width = piece.chars().count();
        if shown + width > SHOWN_LIMIT {
            text.push('…');
            return ShownName { text, altered: true };
        }
        shown += width;
        text.push_str(&piece);
    }
    ShownName { text, altered }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_are_kept_as_they_are() {
        for name in
            ["Discord", "Mon app 2", "Réglages « son »", "Ünïcode ®", "a.b-c_d (x86)", "日本語"]
        {
            assert_eq!(
                shown_name(name),
                ShownName { text: name.to_owned(), altered: false },
                "{name}"
            );
        }
    }

    #[test]
    fn what_is_not_ordinary_is_shown_never_removed() {
        let shown = shown_name("Sécu\u{202E}rité\u{A0}x\u{0}\t");
        assert_eq!(shown.text, "Sécu⟨U+202E⟩rité⟨U+00A0⟩x⟨U+0000⟩⟨U+0009⟩");
        assert!(shown.altered);
        for hidden in [0x115F, 0x1160, 0x3164, 0xFFA0, 0x200B, 0xFEFF, 0x2028] {
            let character = char::from_u32(hidden).unwrap();
            let shown = shown_name(&format!("a{character}b"));
            assert_eq!(shown.text, format!("a⟨U+{hidden:04X}⟩b"), "{hidden:X}");
            assert!(shown.altered);
        }
        // Un caractère hors du plan de base : au moins quatre chiffres, plus si besoin.
        assert_eq!(shown_name("\u{1F600}").text, "⟨U+1F600⟩");
    }

    #[test]
    fn a_long_name_is_cut_at_eighty_displayed_characters_and_says_so() {
        let exact = "a".repeat(SHOWN_LIMIT);
        assert_eq!(shown_name(&exact), ShownName { text: exact.clone(), altered: false });
        let cut = shown_name(&format!("{exact}b"));
        assert_eq!(cut.text, format!("{exact}…"));
        assert!(cut.altered);
        // Un signe montré n'est jamais coupé en deux.
        let near = format!("{}\u{202E}", "a".repeat(SHOWN_LIMIT - 3));
        let cut = shown_name(&near);
        assert_eq!(cut.text, format!("{}…", "a".repeat(SHOWN_LIMIT - 3)));
        assert!(cut.altered);
    }

    #[test]
    fn the_shown_text_never_hides_a_bidi_or_control_character() {
        let tricky: String = (0u32..0x2100)
            .filter_map(char::from_u32)
            .filter(|c| {
                c.is_control()
                    || matches!(u32::from(*c), 0x200B..=0x200F | 0x202A..=0x202E | 0x2066..=0x2069)
            })
            .collect();
        let shown = shown_name(&tricky);
        assert!(shown.altered);
        assert!(
            !shown.text.chars().any(|c| c.is_control() || ('\u{202A}'..='\u{202E}').contains(&c))
        );
    }
}
