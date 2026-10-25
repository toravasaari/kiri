//! Diagrams written as code, such as Mermaid, drawn as images.
//!
//! A renderer takes a diagram's source and a theme and gives an image
//! file: SVG text, or a raster image such as the PNG of a browser's
//! drawing. It knows nothing about terminals: the image loader decodes it
//! as it does an image file, and another backend could use the same
//! image.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub mod external;
pub mod find;
mod language;
mod math;
mod mermaid;

pub use external::{Command, External, ExternalSettings};
pub use language::{LANGUAGES, Language, ThemeNames, language};
pub use mermaid::MMDC;

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

/// A diagram drawn as an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawn {
    /// The image file: SVG, or a raster image such as PNG, told apart by
    /// its content.
    pub image: Vec<u8>,
    /// How many pixels of a raster image make one CSS pixel, as a browser
    /// draws for a display of that density. An SVG has its own size.
    pub scale: u32,
    /// The page its colours are for: the theme it was asked for, unless
    /// the diagram chose colours of its own.
    pub theme: Theme,
    /// The background colour of its theme, when the renderer tells it.
    pub background: Option<[u8; 3]>,
}

impl Drawn {
    /// A diagram drawn as the SVG document `svg`, for a page of `theme`,
    /// with the background colour `background` when the renderer tells
    /// it.
    pub fn svg(svg: impl Into<String>, theme: Theme, background: Option<[u8; 3]>) -> Self {
        Drawn {
            image: svg.into().into_bytes(),
            scale: 1,
            theme,
            background,
        }
    }

    /// The image as text, as an SVG is.
    #[cfg(test)]
    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.image).into_owned()
    }
}

/// A diagram to draw.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// Its language, as the info string of its code block gives it.
    pub language: &'a str,
    pub source: &'a str,
    /// The page it is drawn for, unless it chooses its own colours.
    pub theme: Theme,
    /// The colour of the text around it, which a formula is drawn in;
    /// `None` when it is not known.
    pub color: Option<[u8; 3]>,
    /// Set when this drawing alone is to stop, as when the diagram is no
    /// longer wanted: a program drawing it is stopped, and one that has
    /// not started does not. A renderer built into kiri cannot stop.
    pub stop: Option<&'a AtomicBool>,
}

impl<'a> Request<'a> {
    pub fn new(language: &'a str, source: &'a str, theme: Theme) -> Self {
        Request {
            language,
            source,
            theme,
            color: None,
            stop: None,
        }
    }

    /// The same request, for a page whose text is in `color`.
    pub fn in_color(self, color: Option<[u8; 3]>) -> Self {
        Request { color, ..self }
    }

    /// The same request, stopped when `stop` is set.
    pub fn stopped_by(self, stop: &'a AtomicBool) -> Self {
        Request {
            stop: Some(stop),
            ..self
        }
    }
}

/// Draws the diagrams of some languages.
pub trait Renderer {
    /// Whether code blocks in `language`, as the info string gives it, are
    /// drawn as diagrams.
    fn draws(&self, language: &str) -> bool;

    /// The diagram that `request` asks for as an image, or why it could
    /// not be drawn.
    fn render(&self, request: &Request) -> Result<Drawn, String>;

    /// Each of `requests` drawn as `render` draws it, in order; `done` is
    /// called as each one is finished.
    fn render_all(
        &self,
        requests: &[Request],
        done: &(dyn Fn() + Sync),
    ) -> Vec<Result<Drawn, String>> {
        render_one_by_one(self, requests, done)
    }
}

/// `render_all` by `renderer`, one diagram after another.
fn render_one_by_one<R: Renderer + ?Sized>(
    renderer: &R,
    requests: &[Request],
    done: &(dyn Fn() + Sync),
) -> Vec<Result<Drawn, String>> {
    (requests.iter())
        .map(|request| {
            let drawn = renderer.render(request);
            done();
            drawn
        })
        .collect()
}

/// `render_all` by `renderer`, drawing up to `at_once` diagrams at a time.
fn render_in_parallel<R: Renderer + Sync + ?Sized>(
    renderer: &R,
    requests: &[Request],
    at_once: usize,
    done: &(dyn Fn() + Sync),
) -> Vec<Result<Drawn, String>> {
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<Result<Drawn, String>>>> =
        requests.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..at_once.max(1).min(requests.len()) {
            // As large a stack as the main thread's, for a built-in
            // renderer among the external ones.
            let thread = std::thread::Builder::new().stack_size(8 << 20);
            let spawned = thread.spawn_scoped(scope, || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(request) = requests.get(i) else {
                        break;
                    };
                    let drawn = renderer.render(request);
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

/// The renderers built into kiri, such as merman for Mermaid in a build
/// with the `mermaid` feature.
#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltIn;

impl BuiltIn {
    /// The renderer built in for `language`, in this build.
    fn of(language: &str) -> Option<language::BuiltInRenderer> {
        self::language(language)?.built_in
    }

    /// What this build has built in to draw `language`, with its version,
    /// such as `merman 0.9.1, Mermaid 12.1.0`; `None` when it has nothing.
    pub fn version(language: &str) -> Option<String> {
        Self::of(language).map(|built_in| (built_in.version)())
    }
}

impl Renderer for BuiltIn {
    fn draws(&self, language: &str) -> bool {
        Self::of(language).is_some()
    }

    fn render(&self, request: &Request) -> Result<Drawn, String> {
        match Self::of(request.language) {
            Some(built_in) => (built_in.render)(request),
            None => Err(format!(
                "{} diagrams are not drawn in this build",
                request.language
            )),
        }
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
    /// each language that has one, run as `settings` say.
    pub fn new(commands: BTreeMap<String, Command>, settings: ExternalSettings) -> Self {
        Renderers {
            external: External::new(commands, settings),
        }
    }

    /// The built-in renderers, and the external commands that `configured`
    /// gives or that `search` finds for the languages `no_search` does not
    /// have, run as `settings` say; see `find::choose`.
    pub fn chosen(
        configured: &BTreeMap<String, Vec<String>>,
        no_search: &BTreeSet<String>,
        search: &find::Search,
        settings: ExternalSettings,
    ) -> Self {
        Self::new(find::commands(configured, no_search, search), settings)
    }

    /// Whether programs outside kiri draw any of `languages`.
    pub fn runs_programs_for<'a>(&self, languages: impl IntoIterator<Item = &'a str>) -> bool {
        (languages.into_iter()).any(|language| self.external.draws(language))
    }
}

impl Renderer for Renderers {
    fn draws(&self, language: &str) -> bool {
        self.external.draws(language) || BuiltIn.draws(language)
    }

    fn render(&self, request: &Request) -> Result<Drawn, String> {
        match self.external.draws(request.language) {
            true => self.external.render(request),
            false => BuiltIn.render(request),
        }
    }

    /// Several at once when an external command draws any of them: it
    /// spends most of its time starting up, as `mmdc` does a browser.
    fn render_all(
        &self,
        requests: &[Request],
        done: &(dyn Fn() + Sync),
    ) -> Vec<Result<Drawn, String>> {
        if (requests.iter()).any(|request| self.external.draws(request.language)) {
            render_in_parallel(self, requests, external::AT_ONCE, done)
        } else {
            // On this thread, whose stack is larger.
            render_one_by_one(self, requests, done)
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

    fn render(&self, request: &Request) -> Result<Drawn, String> {
        Err(format!("{} diagrams are not drawn", request.language))
    }
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

/// How a warning names the diagram `source` in `language` that starts on
/// `line` of its document: by that line, or else by the diagram's own
/// first line, past anything in it that is not the diagram, such as front
/// matter, directives and comments.
pub fn name(language: &str, source: &str, line: usize, by_line: bool) -> String {
    let noun = noun(language);
    if by_line {
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
        let settings = || ExternalSettings::new(std::env::temp_dir());
        let renderers = Renderers::chosen(
            &commands,
            &BTreeSet::new(),
            &find::Search::default(),
            settings(),
        );
        assert!(renderers.draws("mermaid") && renderers.draws("Mermaid"));
        // It fails, and the built-in renderer does not step in.
        let drawn = renderers.render(&Request::new(
            "mermaid",
            "flowchart TD\n  A --> B\n",
            Theme::Light,
        ));
        assert_eq!(drawn, Err("sh exited with status 4".to_owned()));
        let request = Request::new("mermaid", "flowchart TD\n", Theme::Dark);
        let drawn = renderers.render_all(&[request], &|| {});
        assert_eq!(drawn, [Err("sh exited with status 4".to_owned())]);
        let none = Renderers::new(BTreeMap::new(), settings());
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

        fn render(&self, request: &Request) -> Result<Drawn, String> {
            let now = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            self.now.fetch_sub(1, Ordering::SeqCst);
            match request.source {
                "bad" => Err(format!("{}: bad", request.language)),
                source => Ok(Drawn::svg(source, request.theme, None)),
            }
        }
    }

    #[test]
    fn draws_up_to_four_diagrams_at_once_and_keeps_their_order() {
        let sources: Vec<String> = (0..10).map(|n| n.to_string()).collect();
        let mut diagrams: Vec<Request> = sources
            .iter()
            .map(|source| Request::new("mermaid", source, Theme::Dark))
            .collect();
        diagrams[3].source = "bad";
        let slow = Slow::default();
        let done = AtomicUsize::new(0);
        let count = || {
            done.fetch_add(1, Ordering::SeqCst);
        };
        let drawn = render_in_parallel(&slow, &diagrams, external::AT_ONCE, &count);
        let svgs: Vec<Result<String, String>> = (drawn.into_iter())
            .map(|drawn| drawn.map(|drawn| drawn.text()))
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
        assert!(render_in_parallel(&slow, &[], 4, &count).is_empty());
    }

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
        for language in LANGUAGES.iter().map(|language| language.name) {
            let drawn = BuiltIn.draws(language);
            assert_eq!(BuiltIn::version(language).is_some(), drawn, "{language}");
        }
        assert_eq!(BuiltIn::version("dot"), None);
    }

    #[test]
    fn formulas_are_built_in_in_every_build() {
        for language in ["math", "Math"] {
            assert!(BuiltIn.draws(language), "{language}");
        }
        assert!(BuiltIn::version("math").unwrap().starts_with("RaTeX "));
        let drawn = BuiltIn.render(&Request::new("math", "x^2", Theme::Dark));
        assert!(drawn.unwrap().text().starts_with("<svg"));
    }

    #[test]
    fn a_formula_is_called_a_formula() {
        assert_eq!(noun("math"), "formula");
        assert_eq!(noun("Math"), "formula");
        assert_eq!(noun("mermaid"), "mermaid diagram");
        assert_eq!(noun("dot"), "dot diagram");
        assert_eq!(name("math", "\\frac{a\n", 4, true), "formula on line 4");
        assert_eq!(
            name("math", "\n\\frac{a\n", 4, false),
            "formula \"\\frac{a\""
        );
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
        // Another language's first line is its first line.
        assert_eq!(
            name("dot", "%% x\ndigraph {}\n", 1, false),
            "dot diagram \"%% x\""
        );
    }
}
