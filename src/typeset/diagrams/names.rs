//! What kiri says and knows about a diagram language by its name: its theme
//! names, how a message calls it, and the page its source chooses.

use crate::typeset::diagrams::language::{self, ThemeNames, language};
use crate::typeset::diagrams::request::Theme;

/// The name of `theme` that the external command for `language` gets as
/// `{theme}`: the language's own theme names, and otherwise `light` or
/// `dark`.
pub fn theme_name(language: &str, theme: Theme) -> &'static str {
    let names = self::language(language).map_or(ThemeNames::PLAIN, |language| language.theme_names);
    names.of(theme)
}

/// The name of `language` in a sentence, such as `Mermaid`, and otherwise
/// the name as the info string gives it.
pub fn display_name(language: &str) -> &str {
    self::language(language).map_or(language, |known| known.display_name)
}

/// The extension of a file of source in `language`, by which a program
/// such as `mmdc` tells what the file is: the language's own, and
/// otherwise its name in lower case, without what a file name cannot
/// safely have.
pub fn extension(language: &str) -> String {
    match self::language(language) {
        Some(known) => known.source_extension.to_owned(),
        None => {
            let name: String = (language.chars())
                .filter(char::is_ascii_alphanumeric)
                .map(|c| c.to_ascii_lowercase())
                .collect();
            if name.is_empty() {
                "txt".to_owned()
            } else {
                name
            }
        }
    }
}

/// The page a diagram in `language` chooses its colours for, read from
/// its `source` for a renderer that does not tell; `None` when it chooses
/// nothing, or the language has no way to.
pub fn chosen_theme(language: &str, source: &str) -> Option<Theme> {
    (self::language(language)?.chosen_page?)(source)
}

/// The colour of the panel under a diagram in `language` that is in dark
/// colours on a light page, when its renderer does not tell its
/// background.
pub fn dark_background(language: &str) -> Option<[u8; 3]> {
    self::language(language)?.dark_background
}

/// What a message calls a diagram in `language`, as the info string
/// gives it: `formula` for a formula, and otherwise a diagram in that
/// language, such as `mermaid diagram`.
pub fn noun(language: &str) -> String {
    match self::language(language).and_then(|known| known.noun) {
        Some(noun) => noun.to_owned(),
        None => format!("{language} diagram"),
    }
}

/// Whether a diagram in `language` is centred in its room, as a formula
/// is, rather than at its left edge.
pub fn centred(language: &str) -> bool {
    self::language(language).is_some_and(|known| known.centred)
}

/// How a warning names the diagram `source` in `language` that starts on
/// `line` of its document: by that line, or else by the diagram's own
/// first line, past anything in it that is not the diagram, such as front
/// matter, directives and comments.
pub fn name(language: &str, source: &str, line: usize, by_line: bool) -> String {
    let noun = noun(language);
    if by_line && line > 0 {
        return format!("{noun} on line {line}");
    }
    let first_line = match self::language(language) {
        Some(known) => (known.first_line)(source),
        None => language::first_line(source),
    };
    match first_line {
        Some(text) => format!("{noun} \"{text}\""),
        None => noun,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_has_mermaid_names_for_mermaid() {
        assert_eq!(theme_name("mermaid", Theme::Light), "default");
        assert_eq!(theme_name("Mermaid", Theme::Dark), "dark");
        assert_eq!(theme_name("dot", Theme::Light), "light");
        assert_eq!(theme_name("dot", Theme::Dark), "dark");
        assert_eq!(extension("Mermaid"), "mmd");
        assert_eq!(display_name("mermaid"), "Mermaid");
        assert_eq!(display_name("dot"), "dot");
        assert_eq!(extension("Dot"), "dot");
        // A name that is no safe file name.
        assert_eq!(extension("../C++"), "c");
        assert_eq!(extension("/"), "txt");
    }

    #[test]
    fn mermaid_s_dark_theme_has_a_dark_grey_background() {
        assert_eq!(dark_background("Mermaid"), Some([0x33, 0x33, 0x33]));
        assert_eq!(dark_background("dot"), None);
    }

    #[test]
    fn a_mermaid_diagram_chooses_its_theme_in_its_source() {
        let forest = "%%{init: {\"theme\": \"forest\"}}%%\nflowchart TD\n";
        assert_eq!(chosen_theme("Mermaid", forest), Some(Theme::Light));
        assert_eq!(chosen_theme("dot", forest), None);
    }

    #[test]
    fn a_formula_is_called_a_formula() {
        assert_eq!(noun("math"), "formula");
        assert_eq!(noun("Math"), "formula");
        assert_eq!(noun("mermaid"), "mermaid diagram");
        assert_eq!(noun("dot"), "dot diagram");
        assert!(centred("math") && !centred("mermaid") && !centred("dot"));
        assert_eq!(name("math", "\\frac{a\n", 4, true), "formula on line 4");
        assert_eq!(
            name("math", "\n\\frac{a\n", 4, false),
            "formula \"\\frac{a\""
        );
    }

    #[test]
    fn names_a_diagram_by_its_line_or_its_first_line() {
        let source = "\n%%{init: {\"theme\": \"dark\"}}%%\n  flowchart TD\n  A --> B\n";
        assert_eq!(
            name("mermaid", source, 12, true),
            "mermaid diagram on line 12"
        );
        assert_eq!(
            name("mermaid", source, 12, false),
            "mermaid diagram \"flowchart TD\""
        );
        let front_matter = "---\ntitle: T\n---\nsequenceDiagram\n";
        assert_eq!(
            name("mermaid", front_matter, 1, false),
            "mermaid diagram \"sequenceDiagram\""
        );
        assert_eq!(name("mermaid", "\n", 1, false), "mermaid diagram");
        // Another language's first line is its first line.
        assert_eq!(
            name("dot", "%% x\ndigraph {}\n", 1, false),
            "dot diagram \"%% x\""
        );
    }
}
