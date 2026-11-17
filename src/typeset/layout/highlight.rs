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

use syntect::highlighting::{
    FontStyle, HighlightIterator, HighlightState, Style as SyntectStyle, ThemeSet,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};
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

/// The language to highlight the file called `name` in, as `highlight`
/// takes it, or `None` when the highlighter has no grammar for the file,
/// which is then shown without highlighting. The whole name comes first,
/// as grammars list some, such as `Makefile`, `.gitignore` and
/// `Cargo.lock`, and then the extension; the lookup is the fence
/// language's, so what is called code is what gets highlighted.
pub fn file_language(name: &str) -> Option<String> {
    let extension = std::path::Path::new(name)
        .extension()
        .and_then(std::ffi::OsStr::to_str);
    [Some(name), extension]
        .into_iter()
        .flatten()
        .map(str::to_lowercase)
        .find(|token| find_syntax(Some(token)).name != PLAIN_TEXT)
}

/// The name of the grammar that `find_syntax` falls back to.
const PLAIN_TEXT: &str = "Plain Text";

/// The grammar that starts a file with `line`, by the patterns grammars
/// give for a first line, such as a shebang or `<?xml`: the grammar's name,
/// which `highlight` takes as a language. `None` when no grammar claims
/// the line.
pub fn first_line_language(line: &str) -> Option<&'static str> {
    let syntax = syntaxes().find_syntax_by_first_line(line)?;
    (syntax.name != PLAIN_TEXT).then_some(syntax.name.as_str())
}

/// The highlighting of code that goes on from where it stopped: it is fed
/// the lines of the code a stretch at a time and keeps what syntect needs
/// of the lines before. A line that syntect fails on, which a grammar's
/// mistake may cause, and the lines after it are left without colours, in
/// the theme's text colour, since the state after a failure is not known.
pub struct Highlighter {
    colours: syntect::highlighting::Highlighter<'static>,
    /// The grammar of the language, which a fresh start parses from.
    syntax: &'static SyntaxReference,
    parse: ParseState,
    state: HighlightState,
    background: Option<Color>,
    failed: bool,
}

/// Where a [`Highlighter`] is in the code it was fed: what syntect keeps of
/// the lines before, so that a highlighter made again goes on from there.
#[derive(Debug, Clone)]
pub struct Saved {
    parse: ParseState,
    state: HighlightState,
    failed: bool,
}

impl Highlighter {
    /// Where the highlighter is, to be put back by [`Highlighter::restore`]
    /// in a highlighter made for the same language and theme.
    pub fn save(&self) -> Saved {
        Saved {
            parse: self.parse.clone(),
            state: self.state.clone(),
            failed: self.failed,
        }
    }

    /// Puts the highlighter where `saved` was.
    pub fn restore(&mut self, saved: &Saved) {
        self.parse = saved.parse.clone();
        self.state = saved.state.clone();
        self.failed = saved.failed;
    }

    /// A highlighter for code in `language` in the bundled theme
    /// `theme_name`, or `None` if there is no such theme. Unknown languages
    /// are shown in the theme's colours without highlighting. A token whose
    /// colour is hard to read on the theme's background is made lighter or
    /// darker, in its own hue, until it reaches WCAG's contrast for text.
    pub fn new(language: Option<&str>, theme_name: &str) -> Option<Self> {
        let theme = themes().themes.get(theme_name)?;
        let colours = syntect::highlighting::Highlighter::new(theme);
        let state = HighlightState::new(&colours, ScopeStack::new());
        let syntax = find_syntax(language);
        Some(Self {
            colours,
            syntax,
            parse: ParseState::new(syntax),
            state,
            background: background(theme_name)?,
            failed: false,
        })
    }

    /// Starts again as at the top of a text, as a guess of where the
    /// highlighting is at a line whose lines before were not fed.
    pub fn guess(&mut self) {
        self.parse = ParseState::new(self.syntax);
        self.state = HighlightState::new(&self.colours, ScopeStack::new());
        self.failed = false;
    }

    /// The theme's background colour.
    pub fn background(&self) -> Option<Color> {
        self.background
    }

    /// The spans of each of the lines of `code`, which is whole lines that
    /// follow those it was fed before.
    pub fn feed(&mut self, code: &str) -> Vec<Vec<Span>> {
        LinesWithEndings::from(code)
            .map(|line| {
                let ranges = match self.failed {
                    true => None,
                    false => self.ranges(line),
                };
                let mut spans = Vec::new();
                match ranges {
                    Some(ranges) => {
                        for (style, text) in ranges {
                            self.push(&mut spans, text, style);
                        }
                    }
                    None => {
                        self.failed = true;
                        let style = self.colours.get_default();
                        self.push(&mut spans, line, style);
                    }
                }
                spans
            })
            .collect()
    }

    /// The styled parts of `line`, or `None` when syntect fails on it.
    fn ranges<'a>(&mut self, line: &'a str) -> Option<Vec<(SyntectStyle, &'a str)>> {
        #[cfg(test)]
        if line.contains('\0') {
            return None;
        }
        let ops = self.parse.parse_line(line, syntaxes()).ok()?;
        let iter = HighlightIterator::new(&mut self.state, &ops, line, &self.colours);
        Some(iter.collect())
    }

    /// Adds `text`, without its line ending, to `spans` in `style`.
    fn push(&self, spans: &mut Vec<Span>, text: &str, style: SyntectStyle) {
        let text = text.trim_end_matches(['\n', '\r']);
        let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
        let fg = match self.background {
            Some(background) => fg.readable_on(background, READABLE),
            None => fg,
        };
        push(
            spans,
            text,
            &Style {
                fg: Some(fg),
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
}

/// `code` highlighted whole, as a [`Highlighter`] fed all of it does;
/// `None` if `theme` is not a bundled theme name.
pub fn highlight(code: &str, language: Option<&str>, theme_name: &str) -> Option<Highlighted> {
    let mut highlighter = Highlighter::new(language, theme_name)?;
    let lines = highlighter.feed(code);
    Some(Highlighted {
        lines,
        background: highlighter.background(),
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

    /// The distinct foreground colours of a line's spans.
    fn colours(line: &[Span]) -> usize {
        let mut all: Vec<_> = line.iter().map(|span| span.style.fg).collect();
        all.dedup();
        all.len()
    }

    #[test]
    fn a_line_that_fails_leaves_the_lines_from_it_on_without_colours() {
        // In a test build a line with a NUL fails, as a broken grammar's
        // would.
        let code = "fn a() {}\nfn b() {}\nfn \0c() {}\nfn d() {}\n";
        let theme = "base16-ocean.dark";
        let highlighted = highlight(code, Some("rust"), theme).unwrap();
        assert_eq!(highlighted.lines.len(), 4);
        for line in &highlighted.lines[..2] {
            assert!(colours(line) > 1, "{line:?}");
        }
        // The text colour of the theme, as plain text has it; a span keeps no
        // control character.
        let plain = highlight("x\n", None, theme).unwrap();
        let text = plain.lines[0][0].style.fg;
        for (line, source) in highlighted.lines[2..]
            .iter()
            .zip(["fn \u{fffd}c() {}", "fn d() {}"])
        {
            assert_eq!(line.len(), 1, "{line:?}");
            assert_eq!(line[0].text, source);
            assert_eq!(line[0].style.fg, text, "{line:?}");
        }
    }

    #[test]
    fn a_highlighter_goes_on_where_it_stopped() {
        let code = "/* one\ntwo\n*/ fn f() {}\nlet s = 1;\n";
        let whole = highlight(code, Some("rust"), "base16-ocean.dark").unwrap();
        let mut highlighter = Highlighter::new(Some("rust"), "base16-ocean.dark").unwrap();
        let mut lines = highlighter.feed("/* one\n");
        lines.extend(highlighter.feed("two\n*/ fn f() {}\n"));
        lines.extend(highlighter.feed("let s = 1;\n"));
        assert_eq!(lines, whole.lines);
        assert_eq!(highlighter.background(), whole.background);
        // The comment goes on over the lines handed in between.
        assert_eq!(colours(&lines[1]), 1, "{:?}", lines[1]);
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

    #[test]
    fn knows_the_files_it_has_a_grammar_for() {
        for name in [
            "a.yaml",
            "a.YML",
            "a.json",
            "Cargo.toml",
            "main.rs",
            "page.html",
        ] {
            assert!(file_language(name).is_some(), "{name}");
        }
        // By the whole name, which has no extension or a misleading one.
        for name in [
            "Makefile",
            "Dockerfile",
            ".gitignore",
            "Cargo.lock",
            "CMakeLists.txt",
        ] {
            assert!(file_language(name).is_some(), "{name}");
        }
        for name in ["a.txt", "a.xyz", "LICENSE", "README", "notes", ""] {
            assert_eq!(file_language(name), None, "{name}");
        }
    }

    #[test]
    fn the_language_of_a_file_is_one_that_highlights() {
        for (name, grammar) in [
            ("a.yaml", "YAML"),
            ("Makefile", "Makefile"),
            (".gitignore", "Git Ignore"),
            ("Cargo.lock", "TOML"),
            ("CMakeLists.txt", "CMake"),
        ] {
            let language = file_language(name).unwrap();
            assert_eq!(find_syntax(Some(&language)).name, grammar, "{name}");
        }
    }

    #[test]
    fn a_first_line_tells_the_grammar() {
        let shell = first_line_language("#!/bin/sh").unwrap();
        assert_eq!(find_syntax(Some(shell)).name, "Bourne Again Shell (bash)");
        let python = first_line_language("#!/usr/bin/env python3").unwrap();
        assert_eq!(find_syntax(Some(python)).name, "Python");
        assert!(first_line_language("<?xml version=\"1.0\"?>").is_some());
        assert_eq!(first_line_language("# Title"), None);
        assert_eq!(first_line_language(""), None);
    }
}
