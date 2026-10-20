//! Diagrams written as code, such as Mermaid, drawn as SVG.
//!
//! A renderer takes a diagram's source and a theme and gives SVG text. It
//! knows nothing about terminals: the image loader decodes the SVG as it
//! does an SVG file, and another backend could use the same SVG.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub mod external;
pub mod find;
#[cfg(feature = "mermaid")]
mod mermaid;

pub use external::{Command, External};

/// The languages kiri can draw diagrams in, as the info strings of code
/// blocks name them.
pub const LANGUAGES: &[&str] = &["mermaid"];

/// The colours a diagram is drawn in when it does not choose its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Theme {
    /// For a light page.
    #[default]
    Light,
    /// For a dark page.
    Dark,
}

impl Theme {
    /// The theme for a page whose background is dark or not; a page whose
    /// background is unknown counts as dark, as it does for code blocks.
    pub fn for_page(dark_background: Option<bool>) -> Self {
        match dark_background {
            Some(true) | None => Theme::Dark,
            Some(false) => Theme::Light,
        }
    }
}

/// A diagram drawn as SVG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawn {
    pub svg: String,
    /// The page its colours are for: the theme it was asked for, unless
    /// the diagram chose colours of its own.
    pub theme: Theme,
}

/// Draws the diagrams of some languages.
pub trait Renderer {
    /// Whether code blocks in `language`, as the info string gives it, are
    /// drawn as diagrams.
    fn draws(&self, language: &str) -> bool;

    /// The diagram `source`, written in `language`, as an SVG document in
    /// `theme` unless it chooses its own colours, or why it could not be
    /// drawn.
    fn render(&self, language: &str, source: &str, theme: Theme) -> Result<Drawn, String>;

    /// Each of `diagrams`, a language and a source, drawn as `render`
    /// draws it, in order; `done` is called as each one is finished.
    fn render_all(
        &self,
        diagrams: &[(&str, &str)],
        theme: Theme,
        done: &(dyn Fn() + Sync),
    ) -> Vec<Result<Drawn, String>> {
        render_one_by_one(self, diagrams, theme, done)
    }
}

/// `render_all` by `renderer`, one diagram after another.
fn render_one_by_one<R: Renderer + ?Sized>(
    renderer: &R,
    diagrams: &[(&str, &str)],
    theme: Theme,
    done: &(dyn Fn() + Sync),
) -> Vec<Result<Drawn, String>> {
    (diagrams.iter())
        .map(|&(language, source)| {
            let drawn = renderer.render(language, source, theme);
            done();
            drawn
        })
        .collect()
}

/// `render_all` by `renderer`, drawing up to `at_once` diagrams at a time.
pub fn render_in_parallel<R: Renderer + Sync + ?Sized>(
    renderer: &R,
    diagrams: &[(&str, &str)],
    theme: Theme,
    at_once: usize,
    done: &(dyn Fn() + Sync),
) -> Vec<Result<Drawn, String>> {
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<Result<Drawn, String>>>> =
        diagrams.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..at_once.max(1).min(diagrams.len()) {
            // As large a stack as the main thread's, for a built-in
            // renderer among the external ones.
            let thread = std::thread::Builder::new().stack_size(8 << 20);
            let spawned = thread.spawn_scoped(scope, || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(language, source)) = diagrams.get(i) else {
                        break;
                    };
                    let drawn = renderer.render(language, source, theme);
                    *results[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(drawn);
                    done();
                }
            });
            // A thread that does not start leaves its share to the others;
            // the last diagrams not drawn by any say so.
            if spawned.is_err() {
                break;
            }
        }
    });
    (results.into_iter())
        .map(|result| {
            let result = result.into_inner().unwrap_or_else(|e| e.into_inner());
            result.unwrap_or_else(|| Err("not drawn".to_owned()))
        })
        .collect()
}

/// The name of `theme` that the external command for `language` gets as
/// `{theme}`: Mermaid's own theme names for Mermaid, `light` or `dark`
/// otherwise.
pub fn theme_name(language: &str, theme: Theme) -> &'static str {
    let mermaid = language.eq_ignore_ascii_case("mermaid");
    match (theme, mermaid) {
        (Theme::Light, true) => "default",
        (Theme::Light, false) => "light",
        (Theme::Dark, _) => "dark",
    }
}

/// The extension of a file of source in `language`, by which a program
/// such as `mmdc` tells what the file is.
pub fn extension(language: &str) -> String {
    if language.eq_ignore_ascii_case("mermaid") {
        "mmd".to_owned()
    } else {
        language.to_ascii_lowercase()
    }
}

/// The page a Mermaid diagram chooses its colours for, in a directive
/// such as `%%{init: {"theme": "forest"}}%%` or under `config:` in its
/// front matter, read from the source for a renderer that does not tell.
/// `None` when it chooses nothing, and for other languages.
pub fn chosen_theme(language: &str, source: &str) -> Option<Theme> {
    if !language.eq_ignore_ascii_case("mermaid") {
        return None;
    }
    // The text of the front matter and of the directives.
    let mut settings = String::new();
    let mut lines = source.lines().skip_while(|line| line.trim().is_empty());
    let mut directive = false;
    let mut first = true;
    while let Some(line) = lines.next() {
        let text = line.trim();
        if first && text == "---" {
            for line in lines.by_ref().take_while(|line| line.trim() != "---") {
                settings.push_str(line);
                settings.push('\n');
            }
        } else if directive || text.starts_with("%%{") {
            settings.push_str(text);
            settings.push('\n');
            directive = !text.ends_with("}%%");
        }
        first = false;
    }
    if value_of(&settings, "darkMode") == Some("true") {
        return Some(Theme::Dark);
    }
    match value_of(&settings, "theme")? {
        "null" => None,
        name if name.ends_with("dark") => Some(Theme::Dark),
        _ => Some(Theme::Light),
    }
}

/// The value of the first `key` in `text`, YAML or JSON with or without
/// quotes, as a word.
fn value_of<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let quote = |c: char| c == '"' || c == '\'';
    text.match_indices(key).find_map(|(at, _)| {
        if text[..at].ends_with(word) {
            return None;
        }
        let rest = text[at + key.len()..].trim_start_matches(quote);
        let rest = rest.trim_start_matches([' ', '\t']).strip_prefix(':')?;
        let rest = rest.trim_start().trim_start_matches(quote);
        let end = rest.find(|c: char| !word(c)).unwrap_or(rest.len());
        Some(&rest[..end]).filter(|value| !value.is_empty())
    })
}

/// The renderers built into kiri: merman for Mermaid, in a build with the
/// `mermaid` feature.
#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltIn;

impl BuiltIn {
    fn is_mermaid(language: &str) -> bool {
        cfg!(feature = "mermaid") && language.eq_ignore_ascii_case("mermaid")
    }

    /// What this build has built in to draw `language`, with its version,
    /// such as `merman 0.9.1, Mermaid 12.1.0`; `None` when it has nothing.
    pub fn version(language: &str) -> Option<String> {
        #[cfg(feature = "mermaid")]
        if Self::is_mermaid(language) {
            return Some(mermaid::version());
        }
        let _ = language;
        None
    }
}

impl Renderer for BuiltIn {
    fn draws(&self, language: &str) -> bool {
        Self::is_mermaid(language)
    }

    fn render(&self, language: &str, source: &str, theme: Theme) -> Result<Drawn, String> {
        #[cfg(feature = "mermaid")]
        if Self::is_mermaid(language) {
            return mermaid::render(source, theme);
        }
        let _ = (source, theme);
        Err(format!("{language} diagrams are not drawn in this build"))
    }
}

/// The renderers a run draws with: for each language, its external
/// command when it has one, and otherwise the renderer built into kiri.
/// An external command that fails does not fall back to the built-in
/// renderer, so the user sees that their choice did not work.
#[derive(Debug)]
pub struct Renderers {
    external: External,
}

impl Renderers {
    /// The built-in renderers, and `commands`, the external command for
    /// each language that has one.
    pub fn new(commands: BTreeMap<String, Command>) -> Self {
        Renderers {
            external: External::new(commands),
        }
    }

    /// The built-in renderers, and the external commands that `configured`
    /// gives or that `search` finds; see `find::choose`.
    pub fn chosen(configured: &BTreeMap<String, Vec<String>>, search: &find::Search) -> Self {
        Self::new(find::commands(configured, search))
    }
}

impl Renderer for Renderers {
    fn draws(&self, language: &str) -> bool {
        self.external.draws(language) || BuiltIn.draws(language)
    }

    fn render(&self, language: &str, source: &str, theme: Theme) -> Result<Drawn, String> {
        match self.external.draws(language) {
            true => self.external.render(language, source, theme),
            false => BuiltIn.render(language, source, theme),
        }
    }

    /// Several at once when an external command draws any of them: it
    /// spends most of its time starting up, as `mmdc` does a browser.
    fn render_all(
        &self,
        diagrams: &[(&str, &str)],
        theme: Theme,
        done: &(dyn Fn() + Sync),
    ) -> Vec<Result<Drawn, String>> {
        if diagrams
            .iter()
            .any(|(language, _)| self.external.draws(language))
        {
            render_in_parallel(self, diagrams, theme, external::AT_ONCE, done)
        } else {
            // On this thread, whose stack is larger.
            render_one_by_one(self, diagrams, theme, done)
        }
    }
}

/// How the diagrams of a document are drawn.
#[derive(Clone, Copy)]
pub struct Diagrams<'a> {
    pub renderer: &'a dyn Renderer,
    pub theme: Theme,
    /// Whether a warning names a diagram by its line in the document, or
    /// by its own first line, for a document that has no lines the reader
    /// can look up, such as one piped in.
    pub by_line: bool,
}

impl Diagrams<'static> {
    /// No diagrams: every code block stays code.
    pub fn none() -> Self {
        Diagrams {
            renderer: &NoRenderer,
            theme: Theme::Light,
            by_line: true,
        }
    }
}

/// No renderer at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRenderer;

impl Renderer for NoRenderer {
    fn draws(&self, _language: &str) -> bool {
        false
    }

    fn render(&self, language: &str, _source: &str, _theme: Theme) -> Result<Drawn, String> {
        Err(format!("{language} diagrams are not drawn"))
    }
}

/// How a warning names the diagram `source` in `language` that starts on
/// `line` of its document: by that line, or else by the diagram's own
/// first line, past any front matter, directives and comments.
pub fn name(language: &str, source: &str, line: usize, by_line: bool) -> String {
    if by_line {
        return format!("{language} diagram on line {line}");
    }
    let mut lines = source.lines().map(str::trim);
    let mut first = None;
    while let Some(text) = lines.next() {
        if text.is_empty() || text.starts_with("%%") {
            continue;
        }
        if text == "---" && first.is_none() {
            // Front matter, up to the next `---`.
            lines.by_ref().find(|text| *text == "---");
            first = Some("");
            continue;
        }
        first = Some(text);
        break;
    }
    match first {
        Some(text) if !text.is_empty() => format!("{language} diagram \"{text}\""),
        _ => format!("{language} diagram"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_configured_command_draws_in_place_of_the_built_in_renderer() {
        let commands = BTreeMap::from([(
            "mermaid".to_owned(),
            ["/bin/sh", "-c", "exit 4", "{input}", "{output}"]
                .map(String::from)
                .to_vec(),
        )]);
        let renderers = Renderers::chosen(&commands, &find::Search::default());
        assert!(renderers.draws("mermaid") && renderers.draws("Mermaid"));
        // It fails, and the built-in renderer does not step in.
        let drawn = renderers.render("mermaid", "flowchart TD\n  A --> B\n", Theme::Light);
        assert_eq!(drawn, Err("sh exited with status 4".to_owned()));
        let drawn = renderers.render_all(&[("mermaid", "flowchart TD\n")], Theme::Dark, &|| {});
        assert_eq!(drawn, [Err("sh exited with status 4".to_owned())]);
        let none = Renderers::new(BTreeMap::new());
        assert_eq!(none.draws("mermaid"), cfg!(feature = "mermaid"));
        assert!(!none.draws("dot"));
    }

    /// Counts how many diagrams it draws at once.
    #[derive(Default)]
    struct Slow {
        now: AtomicUsize,
        most: AtomicUsize,
    }

    impl Renderer for Slow {
        fn draws(&self, _language: &str) -> bool {
            true
        }

        fn render(&self, language: &str, source: &str, theme: Theme) -> Result<Drawn, String> {
            let now = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            self.now.fetch_sub(1, Ordering::SeqCst);
            match source {
                "bad" => Err(format!("{language}: bad")),
                _ => Ok(Drawn {
                    svg: source.to_owned(),
                    theme,
                }),
            }
        }
    }

    #[test]
    fn draws_up_to_four_diagrams_at_once_and_keeps_their_order() {
        let sources: Vec<String> = (0..10).map(|n| n.to_string()).collect();
        let mut diagrams: Vec<(&str, &str)> = sources
            .iter()
            .map(|source| ("mermaid", source.as_str()))
            .collect();
        diagrams[3].1 = "bad";
        let slow = Slow::default();
        let done = AtomicUsize::new(0);
        let count = || {
            done.fetch_add(1, Ordering::SeqCst);
        };
        let drawn = render_in_parallel(&slow, &diagrams, Theme::Dark, external::AT_ONCE, &count);
        let svgs: Vec<Result<String, String>> = (drawn.into_iter())
            .map(|drawn| drawn.map(|drawn| drawn.svg))
            .collect();
        let expected: Vec<Result<String, String>> = (0..10)
            .map(|n| match n {
                3 => Err("mermaid: bad".to_owned()),
                n => Ok(n.to_string()),
            })
            .collect();
        assert_eq!(svgs, expected);
        // Never more than four, but more than one, however slowly the
        // threads start.
        let most = slow.most.load(Ordering::SeqCst);
        assert!((2..=4).contains(&most), "{most} at once");
        assert_eq!(done.load(Ordering::SeqCst), 10);
        assert!(render_in_parallel(&slow, &[], Theme::Dark, 4, &count).is_empty());
    }

    #[test]
    fn reads_the_theme_a_mermaid_diagram_chooses() {
        let flowchart = "flowchart TD\n  A --> B\n";
        let front_matter =
            |config: &str| format!("---\ntitle: T\nconfig:\n{config}---\n{flowchart}");
        let cases = [
            (flowchart.to_owned(), None),
            (
                format!("%%{{init: {{\"theme\": \"forest\"}}}}%%\n{flowchart}"),
                Some(Theme::Light),
            ),
            (
                format!("%%{{init: {{'theme':'dark'}}}}%%\n{flowchart}"),
                Some(Theme::Dark),
            ),
            (
                format!(
                    "%%{{ initialize: {{ \"themeVariables\": {{}}, \"theme\" : \"neutral\" }} }}%%\n{flowchart}"
                ),
                Some(Theme::Light),
            ),
            (
                format!(
                    "%%{{init: {{\"themeVariables\": {{\"primaryColor\": \"#000\"}}}}}}%%\n{flowchart}"
                ),
                None,
            ),
            (front_matter("  theme: base\n"), Some(Theme::Light)),
            (front_matter("  theme: \"redux-dark\"\n"), Some(Theme::Dark)),
            (
                front_matter("  theme: base\n  themeVariables:\n    darkMode: true\n"),
                Some(Theme::Dark),
            ),
            // Only the directives and the front matter count.
            (format!("{flowchart}  C[theme: dark]\n"), None),
        ];
        for (source, theme) in cases {
            assert_eq!(chosen_theme("mermaid", &source), theme, "{source}");
        }
        let forest = format!("%%{{init: {{\"theme\": \"forest\"}}}}%%\n{flowchart}");
        assert_eq!(chosen_theme("Mermaid", &forest), Some(Theme::Light));
        assert_eq!(chosen_theme("dot", &forest), None);
    }

    #[test]
    fn the_theme_has_mermaid_names_for_mermaid() {
        assert_eq!(theme_name("mermaid", Theme::Light), "default");
        assert_eq!(theme_name("Mermaid", Theme::Dark), "dark");
        assert_eq!(theme_name("dot", Theme::Light), "light");
        assert_eq!(theme_name("dot", Theme::Dark), "dark");
        assert_eq!(extension("Mermaid"), "mmd");
        assert_eq!(extension("Dot"), "dot");
    }

    #[test]
    fn mermaid_is_built_in_with_its_feature() {
        for language in ["mermaid", "Mermaid"] {
            assert_eq!(BuiltIn.draws(language), cfg!(feature = "mermaid"));
        }
        assert!(!BuiltIn.draws("dot"));
    }

    #[test]
    fn the_built_in_renderer_tells_its_version() {
        assert_eq!(
            BuiltIn::version("mermaid").is_some(),
            cfg!(feature = "mermaid")
        );
        for language in LANGUAGES {
            let drawn = BuiltIn.draws(language);
            assert_eq!(BuiltIn::version(language).is_some(), drawn, "{language}");
        }
        assert_eq!(BuiltIn::version("dot"), None);
    }

    #[test]
    fn an_unknown_page_counts_as_dark() {
        // As for code blocks and quotes.
        assert_eq!(Theme::for_page(None), Theme::Dark);
        assert_eq!(Theme::for_page(Some(false)), Theme::Light);
        assert_eq!(Theme::for_page(Some(true)), Theme::Dark);
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
    }
}
