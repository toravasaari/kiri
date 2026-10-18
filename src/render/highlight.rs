//! Syntax highlighting for code blocks, with syntect's bundled grammars.

use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use super::style::{Span, Style, push};
use crate::color::{Color, Underline};

/// Highlighted code: one list of spans per source line, and the theme's
/// background colour.
pub struct Highlighted {
    pub lines: Vec<Vec<Span>>,
    pub background: Option<Color>,
}

// Loading the bundled sets takes milliseconds, so only documents with code
// pay for it, and only once.
fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn themes() -> &'static ThemeSet {
    static THEMES: OnceLock<ThemeSet> = OnceLock::new();
    THEMES.get_or_init(ThemeSet::load_defaults)
}

/// `None` if `theme` is not a bundled theme name. Unknown languages are
/// shown in the theme's colours without highlighting.
pub fn highlight(code: &str, language: Option<&str>, theme_name: &str) -> Option<Highlighted> {
    let theme = themes().themes.get(theme_name)?;
    let syntaxes = syntaxes();
    let syntax = language
        .and_then(|token| syntaxes.find_syntax_by_token(token))
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut lines = Vec::new();
    for line in LinesWithEndings::from(code) {
        let ranges = highlighter.highlight_line(line, syntaxes).ok()?;
        let mut spans = Vec::new();
        for (style, text) in ranges {
            let text = text.trim_end_matches(['\n', '\r']);
            let fg = style.foreground;
            push(
                &mut spans,
                text,
                &Style {
                    fg: Some(Color::Rgb(fg.r, fg.g, fg.b)),
                    bold: style.font_style.contains(FontStyle::BOLD),
                    italic: style.font_style.contains(FontStyle::ITALIC),
                    underline: if style.font_style.contains(FontStyle::UNDERLINE) {
                        Underline::Single
                    } else {
                        Underline::None
                    },
                    ..Style::default()
                },
            );
        }
        lines.push(spans);
    }
    Some(Highlighted {
        lines,
        background: background(theme_name)?,
    })
}

/// Text colour of a bundled theme; `None` if there is no such theme.
pub fn foreground(theme: &str) -> Option<Option<Color>> {
    let theme = themes().themes.get(theme)?;
    Some(
        theme
            .settings
            .foreground
            .map(|fg| Color::Rgb(fg.r, fg.g, fg.b)),
    )
}

/// Background colour of a bundled theme; `None` if there is no such theme.
pub fn background(theme: &str) -> Option<Option<Color>> {
    let theme = themes().themes.get(theme)?;
    Some(
        theme
            .settings
            .background
            .map(|bg| Color::Rgb(bg.r, bg.g, bg.b)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_keywords_differently_from_names() {
        let highlighted = highlight("fn main() {}\n", Some("rust"), "base16-ocean.dark").unwrap();
        let line = &highlighted.lines[0];
        let keyword = line.iter().find(|span| span.text.contains("fn")).unwrap();
        let name = line.iter().find(|span| span.text.contains("main")).unwrap();
        assert_ne!(keyword.style.fg, name.style.fg);
        assert!(highlighted.background.is_some());
    }

    #[test]
    fn falls_back_to_plain_text_for_unknown_languages() {
        let highlighted = highlight("a\nb\n", Some("no-such-language"), "InspiredGitHub").unwrap();
        assert_eq!(highlighted.lines.len(), 2);
    }

    #[test]
    fn rejects_unknown_themes() {
        assert!(highlight("a", None, "no-such-theme").is_none());
    }

    #[test]
    fn bundles_the_code_themes_a_theme_can_name() {
        let bundled: Vec<&str> = themes().themes.keys().map(String::as_str).collect();
        assert_eq!(bundled, crate::theme::CODE_THEMES);
    }
}
