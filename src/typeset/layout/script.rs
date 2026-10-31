//! Unicode's superscript and subscript characters, for `<sup>` and `<sub>`
//! where text cannot be drawn smaller.

use crate::typeset::model::Inline;

/// Which way a script goes from the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Sub,
    Sup,
}

/// `text` in `script` characters, such as `²` for `2` above the line;
/// `None` unless every character has one. A space stays a space.
pub fn characters(script: Script, text: &str) -> Option<String> {
    text.chars()
        .map(|c| match (c, script) {
            (' ', _) => Some(' '),
            (c, Script::Sup) => superscript(c),
            (c, Script::Sub) => subscript(c),
        })
        .collect()
}

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' | '−' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'a' => 'ᵃ',
        'b' => 'ᵇ',
        'c' => 'ᶜ',
        'd' => 'ᵈ',
        'e' => 'ᵉ',
        'f' => 'ᶠ',
        'g' => 'ᵍ',
        'h' => 'ʰ',
        'i' => 'ⁱ',
        'j' => 'ʲ',
        'k' => 'ᵏ',
        'l' => 'ˡ',
        'm' => 'ᵐ',
        'n' => 'ⁿ',
        'o' => 'ᵒ',
        'p' => 'ᵖ',
        'r' => 'ʳ',
        's' => 'ˢ',
        't' => 'ᵗ',
        'u' => 'ᵘ',
        'v' => 'ᵛ',
        'w' => 'ʷ',
        'x' => 'ˣ',
        'y' => 'ʸ',
        'z' => 'ᶻ',
        'A' => 'ᴬ',
        'B' => 'ᴮ',
        'D' => 'ᴰ',
        'E' => 'ᴱ',
        'G' => 'ᴳ',
        'H' => 'ᴴ',
        'I' => 'ᴵ',
        'J' => 'ᴶ',
        'K' => 'ᴷ',
        'L' => 'ᴸ',
        'M' => 'ᴹ',
        'N' => 'ᴺ',
        'O' => 'ᴼ',
        'P' => 'ᴾ',
        'R' => 'ᴿ',
        'T' => 'ᵀ',
        'U' => 'ᵁ',
        'V' => 'ⱽ',
        'W' => 'ᵂ',
        _ => return None,
    })
}

fn subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' | '−' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'h' => 'ₕ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        _ => return None,
    })
}

/// `content` with every sub- and superscript in it replaced by its own
/// content.
pub(super) fn without_scripts(content: &[Inline]) -> Vec<Inline> {
    let mut out = Vec::new();
    for inline in content {
        out.push(match inline {
            Inline::Subscript(inner) | Inline::Superscript(inner) => {
                out.extend(without_scripts(inner));
                continue;
            }
            Inline::Emphasis(inner) => Inline::Emphasis(without_scripts(inner)),
            Inline::Strong(inner) => Inline::Strong(without_scripts(inner)),
            Inline::Strikethrough(inner) => Inline::Strikethrough(without_scripts(inner)),
            Inline::Kbd(inner) => Inline::Kbd(without_scripts(inner)),
            Inline::Mark(inner) => Inline::Mark(without_scripts(inner)),
            Inline::Link { url, content } => Inline::Link {
                url: url.clone(),
                content: without_scripts(content),
            },
            inline => inline.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_digit_and_sign_has_both_scripts() {
        assert_eq!(
            characters(Script::Sup, "0123456789+-=()").as_deref(),
            Some("⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾")
        );
        assert_eq!(
            characters(Script::Sub, "0123456789+-=()").as_deref(),
            Some("₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎")
        );
    }

    #[test]
    fn letters_have_the_scripts_unicode_gives_them() {
        assert_eq!(
            characters(Script::Sup, "abcdefghijklmnoprstuvwxyz").as_deref(),
            Some("ᵃᵇᶜᵈᵉᶠᵍʰⁱʲᵏˡᵐⁿᵒᵖʳˢᵗᵘᵛʷˣʸᶻ")
        );
        assert_eq!(characters(Script::Sup, "TM").as_deref(), Some("ᵀᴹ"));
        assert_eq!(
            characters(Script::Sub, "aehijklmnoprstuvx").as_deref(),
            Some("ₐₑₕᵢⱼₖₗₘₙₒₚᵣₛₜᵤᵥₓ")
        );
        // Unicode has no superscript q or C, and no subscript b or y.
        for (script, text) in [
            (Script::Sup, "q"),
            (Script::Sup, "C"),
            (Script::Sub, "b"),
            (Script::Sub, "y"),
        ] {
            assert_eq!(characters(script, text), None, "{text}");
        }
    }

    #[test]
    fn one_character_without_a_script_leaves_all_as_they_are() {
        assert_eq!(characters(Script::Sup, "2π"), None);
        assert_eq!(characters(Script::Sup, "1 2").as_deref(), Some("¹ ²"));
        assert_eq!(characters(Script::Sub, "").as_deref(), Some(""));
    }
}
