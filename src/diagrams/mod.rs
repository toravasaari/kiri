//! Diagrams written as code, such as Mermaid, drawn as SVG.
//!
//! A renderer takes a diagram's source and a theme and gives SVG text. It
//! knows nothing about terminals: the image loader decodes the SVG as it
//! does an SVG file, and another backend could use the same SVG.

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
    /// background is unknown counts as light.
    pub fn for_page(dark_background: Option<bool>) -> Self {
        match dark_background {
            Some(true) => Theme::Dark,
            Some(false) | None => Theme::Light,
        }
    }
}

/// Draws the diagrams of some languages.
pub trait Renderer {
    /// Whether code blocks in `language`, as the info string gives it, are
    /// drawn as diagrams.
    fn draws(&self, language: &str) -> bool;

    /// The diagram `source`, written in `language`, as an SVG document, or
    /// why it could not be drawn.
    fn render(&self, language: &str, source: &str, theme: Theme) -> Result<String, String>;
}

/// The renderers built into kiri: none yet.
#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltIn;

impl Renderer for BuiltIn {
    fn draws(&self, _language: &str) -> bool {
        false
    }

    fn render(&self, language: &str, _source: &str, _theme: Theme) -> Result<String, String> {
        Err(format!("{language} diagrams are not drawn in this build"))
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

    fn render(&self, language: &str, _source: &str, _theme: Theme) -> Result<String, String> {
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
    use super::*;

    #[test]
    fn an_unknown_page_counts_as_light() {
        assert_eq!(Theme::for_page(None), Theme::Light);
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
