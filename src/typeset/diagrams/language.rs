//! The languages kiri draws diagrams in, and what it knows about each:
//! plain data, with a function where languages differ in what they do.
//! A new language is one more entry in [`LANGUAGES`].

use crate::diagrams::external::Output;
use crate::diagrams::{Drawn, Request, Theme};
use crate::typeset::diagrams::{math, mermaid};

/// A language kiri draws diagrams in.
#[derive(Debug)]
pub struct Language {
    /// Its name as the info string of a code block gives it, in lower
    /// case; code blocks name it in any case.
    pub name: &'static str,
    /// Other names a code block may give it, in lower case.
    pub aliases: &'static [&'static str],
    /// Its name in a sentence, such as `Mermaid`.
    pub display_name: &'static str,
    /// What a message calls one of its diagrams, such as `formula`, when
    /// not a diagram in the language.
    pub noun: Option<&'static str>,
    /// Whether its diagrams are centred in their room, as GitHub centres
    /// formulas, rather than at its left edge.
    pub centred: bool,
    /// The extensions of its files, which kiri shows on their own as a
    /// diagram, in lower case; `--format` takes them too.
    pub file_extensions: &'static [&'static str],
    /// The extension of the file an external program gets the source in,
    /// by which a program such as `mmdc` tells what it is.
    pub source_extension: &'static str,
    /// The names of its themes for a light and a dark page, which an
    /// external command gets as `{theme}`.
    pub theme_names: ThemeNames,
    /// The command line kiri looks for to draw it when nothing is
    /// configured; its first part is the program.
    pub default_command: Option<&'static [&'static str]>,
    /// The configuration file an external command gets as `{config}`.
    pub config: Option<ConfigFile>,
    /// The page that a diagram's source chooses its colours for, read
    /// from the source for a renderer that does not tell; `None` when it
    /// chooses nothing.
    pub chosen_page: Option<fn(&str) -> Option<Theme>>,
    /// The first line of a diagram's source that a warning names it by,
    /// past anything that is not the diagram itself; `None` when it has
    /// none.
    pub first_line: fn(&str) -> Option<&str>,
    /// The background of its dark theme: the colour of the panel under a
    /// diagram in dark colours on a light page, when its renderer does
    /// not tell its own.
    pub dark_background: Option<[u8; 3]>,
    /// The renderer built into kiri, in a build that has one.
    pub built_in: Option<BuiltInRenderer>,
}

/// The names of a language's themes for a light and a dark page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeNames {
    pub light: &'static str,
    pub dark: &'static str,
}

impl ThemeNames {
    /// The names of a language without themes of its own.
    pub const PLAIN: ThemeNames = ThemeNames {
        light: "light",
        dark: "dark",
    };

    /// The name of the theme for a page of `theme`.
    pub fn of(self, theme: Theme) -> &'static str {
        match theme {
            Theme::Light => self.light,
            Theme::Dark => self.dark,
        }
    }
}

/// The configuration file an external command for a language gets.
#[derive(Debug, Clone, Copy)]
pub struct ConfigFile {
    /// The extension of its file, such as `json`.
    pub extension: &'static str,
    /// Its text for a command that writes `Output`.
    pub text: fn(Output) -> String,
}

/// A renderer built into kiri for one language.
#[derive(Debug, Clone, Copy)]
pub struct BuiltInRenderer {
    /// What draws the diagrams, with its version, such as
    /// `merman 0.9.1, Mermaid 12.1.0`.
    pub version: fn() -> String,
    /// The diagram a request asks for as an image in a theme for its
    /// page, unless it chooses its own colours, or why it could not be
    /// drawn.
    pub render: fn(&Request) -> Result<Drawn, String>,
    /// How far below its baseline a drawn diagram reaches, as a fraction
    /// of the image's height, for a diagram that has a baseline to sit on
    /// in a line of text, such as an inline formula; `None` for the rest.
    pub baseline: fn(&str) -> Option<f64>,
}

/// The languages kiri draws diagrams in.
pub static LANGUAGES: &[Language] = &[mermaid::MERMAID, math::MATH];

/// The languages kiri draws for itself, which no code block is meant to
/// name and no setting or program applies to.
static INTERNAL: &[Language] = &[math::MATH_INLINE];

/// The language that a code block's info string `name` names, in any
/// case, by its name or an alias.
pub fn language(name: &str) -> Option<&'static Language> {
    LANGUAGES.iter().chain(INTERNAL).find(|language| {
        let is = |other: &str| name.eq_ignore_ascii_case(other);
        is(language.name) || language.aliases.iter().any(|alias| is(alias))
    })
}

/// The first line of `source` with something on it, for a language
/// without syntax to skip.
pub fn first_line(source: &str) -> Option<&str> {
    source.lines().map(str::trim).find(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_is_found_by_its_name_in_any_case() {
        for name in ["mermaid", "Mermaid", "MERMAID"] {
            assert_eq!(
                language(name).map(|language| language.name),
                Some("mermaid")
            );
        }
        assert!(language("dot").is_none());
        assert!(language("mmd").is_none());
    }

    #[test]
    fn names_and_extensions_are_in_lower_case() {
        for language in LANGUAGES {
            let names = [language.name, language.source_extension].into_iter();
            let names = names.chain(language.aliases.iter().copied());
            for name in names.chain(language.file_extensions.iter().copied()) {
                assert_eq!(name, name.to_ascii_lowercase(), "{}", language.name);
            }
        }
    }

    #[test]
    fn the_first_line_is_the_first_with_something_on_it() {
        assert_eq!(first_line("\n  \n  digraph {\n}\n"), Some("digraph {"));
        assert_eq!(first_line(" \n"), None);
    }
}
