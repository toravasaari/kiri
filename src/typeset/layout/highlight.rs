//! Syntax highlighting for code blocks, with the grammars of two-face and
//! the themes of syntect.
//!
//! A fence's language is looked up in three steps: the names of the
//! generated table in `languages.rs` (GitHub's names and kiri's own), then
//! syntect's lookup by grammar name or file extension, then the table's file
//! extensions. The table comes first so that a name in it, such as `fs`,
//! which kiri gives to F#, is not taken by a grammar that merely lists it
//! as an extension.

use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

mod languages;

use crate::typeset::color::{Color, Underline};
use crate::typeset::style::{Span, Style, push};

/// The contrast WCAG asks of text (level AA), which code tokens get.
const READABLE: f64 = 4.5;

/// Highlighted code: one list of spans per source line, and the theme's
/// background colour.
pub struct Highlighted {
    pub lines: Vec<Vec<Span>>,
    pub background: Option<Color>,
}

// Loading the syntax set takes milliseconds, so only documents with code
// pay for it, and only once.
fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
}

fn themes() -> &'static ThemeSet {
    static THEMES: OnceLock<ThemeSet> = OnceLock::new();
    THEMES.get_or_init(ThemeSet::load_defaults)
}

/// The grammar for a fence's language: the table's names first, then
/// syntect's own lookup by name or file extension, then the table's file
/// extensions. Plain text if none knows it.
fn find_syntax(language: Option<&str>) -> &'static SyntaxReference {
    let syntaxes = syntaxes();
    let by_name = |name: &str| syntaxes.find_syntax_by_name(name);
    language
        .and_then(|token| {
            let token = token.to_lowercase();
            languages::name(&token)
                .and_then(by_name)
                .or_else(|| syntaxes.find_syntax_by_token(&token))
                .or_else(|| languages::extension(&token).and_then(by_name))
        })
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text())
}

/// `None` if `theme` is not a bundled theme name. Unknown languages are
/// shown in the theme's colours without highlighting. A token whose colour
/// is hard to read on the theme's background is made lighter or darker,
/// in its own hue, until it reaches WCAG's contrast for text.
pub fn highlight(code: &str, language: Option<&str>, theme_name: &str) -> Option<Highlighted> {
    let theme = themes().themes.get(theme_name)?;
    let syntaxes = syntaxes();
    let syntax = find_syntax(language);
    let mut highlighter = HighlightLines::new(syntax, theme);
    let background = background(theme_name)?;
    let readable = |color: Color| match background {
        Some(background) => color.readable_on(background, READABLE),
        None => color,
    };

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
                    fg: Some(readable(Color::Rgb(fg.r, fg.g, fg.b))),
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
    Some(Highlighted { lines, background })
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
    fn every_token_is_readable_on_the_code_background_in_every_theme() {
        let samples = [
            (
                "yaml",
                "# A comment\nname: release-notes\nversion: 1.10\ntags: [a, b]\n",
            ),
            ("rust", "// A comment\nfn main() { let x = \"text\"; }\n"),
        ];
        for theme in crate::typeset::theme::CODE_THEMES {
            for (language, code) in samples {
                let highlighted = highlight(code, Some(language), theme).unwrap();
                let background = highlighted.background.unwrap();
                for span in highlighted.lines.iter().flatten() {
                    let ratio = span.style.fg.unwrap().contrast(background).unwrap();
                    assert!(ratio >= 4.5, "{theme} {language} {:?}: {ratio}", span.text);
                }
            }
        }
    }

    #[test]
    fn picks_the_grammar_a_fence_name_stands_for() {
        let bash = "Bourne Again Shell (bash)";
        for (fence, grammar) in [
            ("jsonc", "JSON"),
            ("typst", "Typst"),
            ("h", "C"),
            ("fs", "F#"),
            ("shell", bash),
            ("console", bash),
            ("shellscript", bash),
            ("proto3", "Protocol Buffer"),
            ("sass", "Sass"),
            ("properties", "Java Properties"),
            ("pp", "Puppet"),
            ("csharp", "C#"),
            ("JSONC", "JSON"),
        ] {
            assert_eq!(find_syntax(Some(fence)).name, grammar, "{fence}");
        }
    }

    #[test]
    fn rejects_unknown_themes() {
        assert!(highlight("a", None, "no-such-theme").is_none());
    }

    #[test]
    fn bundles_the_code_themes_a_theme_can_name() {
        let bundled: Vec<&str> = themes().themes.keys().map(String::as_str).collect();
        assert_eq!(bundled, crate::typeset::theme::CODE_THEMES);
    }

    /// Fence languages that are not code, or are plain text on purpose.
    const NOT_CODE: [&str; 4] = ["math", "mermaid", "text", "txt"];

    fn sample(language: &str) -> &'static str {
        match language {
            "sh" | "shell" | "console" | "fish" => {
                "# note\necho \"hi\" | grep -v x\nif true; then ls; fi\n"
            }
            "toml" => "# note\n[package]\nname = \"kiri\"\nversion = 1\n",
            "json" | "jsonc" => "{\"a\": [1, true, \"s\"]}\n",
            "yaml" => "# note\nname: kiri\nversion: 1.10\n",
            "python" => "# note\ndef f(x):\n    return \"s\" + str(1)\n",
            "typescript" => "// note\nconst x: number = 1;\nfunction f(s: string) { return s; }\n",
            "csharp" => "// note\nclass A { public int F(string s) { return 1; } }\n",
            "markdown" => "# Title\n\nSome *text* and `code`.\n",
            _ => "// note\nfn main() { let x = \"s\"; }\n",
        }
    }

    fn is_highlighted(language: &str) -> bool {
        let code = sample(language);
        let plain = highlight(code, None, "base16-ocean.dark").unwrap();
        let coloured = highlight(code, Some(language), "base16-ocean.dark").unwrap();
        let colours = |h: &Highlighted| {
            let mut all: Vec<_> = h.lines.iter().flatten().map(|s| s.style.fg).collect();
            all.dedup();
            all.len()
        };
        colours(&coloured) > colours(&plain)
    }

    fn fence_languages(path: &std::path::Path, found: &mut std::collections::BTreeSet<String>) {
        if path.is_dir() {
            for entry in std::fs::read_dir(path).unwrap() {
                fence_languages(&entry.unwrap().path(), found);
            }
        } else if path.extension().is_some_and(|ext| ext == "md") {
            // The fence we are inside: its character and its length.
            let mut open: Option<(char, usize)> = None;
            for line in std::fs::read_to_string(path).unwrap().lines() {
                let line = line.trim_start();
                let Some(mark) = line.chars().next().filter(|c| *c == '`' || *c == '~') else {
                    continue;
                };
                let length = line.chars().take_while(|c| *c == mark).count();
                if length < 3 {
                    continue;
                }
                let info = line.trim_start_matches(mark);
                match open {
                    Some((open_mark, open_length)) => {
                        if mark == open_mark && length >= open_length && info.trim().is_empty() {
                            open = None;
                        }
                    }
                    None => {
                        let name: String = info
                            .trim_start()
                            .chars()
                            .take_while(|c| !c.is_whitespace() && *c != '{')
                            .collect();
                        if !name.is_empty() {
                            found.insert(name);
                        }
                        open = Some((mark, length));
                    }
                }
            }
        }
    }

    #[test]
    fn highlights_every_language_the_docs_and_examples_fence() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = std::collections::BTreeSet::new();
        for path in ["docs", "examples", "README.md", "AGENTS.md"] {
            fence_languages(&root.join(path), &mut found);
        }
        assert!(found.contains("toml"), "the scan finds nothing: {found:?}");
        let missing: Vec<_> = found
            .iter()
            .filter(|name| !NOT_CODE.contains(&name.as_str()) && !is_highlighted(name))
            .collect();
        assert!(missing.is_empty(), "not highlighted: {missing:?}");
    }

    #[test]
    fn highlights_the_names_github_knows_for_common_languages() {
        let missing: Vec<_> = [
            "shell",
            "console",
            "typescript",
            "csharp",
            "jsonc",
            "git-config",
        ]
        .into_iter()
        .filter(|name| !is_highlighted(name))
        .collect();
        assert!(missing.is_empty(), "not highlighted: {missing:?}");
    }
}
