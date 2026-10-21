//! What kiri knows about Mermaid, whichever renderer draws it: its
//! directives and front matter, the settings kiri gives a diagram, and
//! mermaid-cli's command line. merman, which draws it inside kiri, is in
//! `merman.rs`, in a build with the `mermaid` feature.

#[cfg(feature = "mermaid")]
mod merman;

use super::Theme;
use super::external::Output;
use super::language::{BuiltInRenderer, ConfigFile, Language, ThemeNames};

/// Mermaid.
pub(super) const MERMAID: Language = Language {
    name: "mermaid",
    aliases: &[],
    display_name: "Mermaid",
    file_extensions: &["mmd", "mermaid"],
    source_extension: "mmd",
    theme_names: ThemeNames {
        light: "default",
        dark: "dark",
    },
    default_command: Some(MMDC),
    config: Some(ConfigFile {
        extension: "json",
        text: config,
    }),
    chosen_page: Some(chosen_page),
    first_line,
    // The background of Mermaid's dark theme.
    dark_background: Some([0x33, 0x33, 0x33]),
    built_in: BUILT_IN,
};

/// merman, in a build that has it.
#[cfg(feature = "mermaid")]
const BUILT_IN: Option<BuiltInRenderer> = Some(BuiltInRenderer {
    version: merman::version,
    render: merman::render,
});

#[cfg(not(feature = "mermaid"))]
const BUILT_IN: Option<BuiltInRenderer> = None;

/// mermaid-cli's command line, which the configuration file can also give.
pub const MMDC: &[&str] = &[
    "mmdc",
    "-i",
    "{input}",
    "-o",
    "{output}",
    "-t",
    "{theme}",
    "-b",
    "transparent",
    "-c",
    "{config}",
    "-s",
    "{scale}",
];

/// The Mermaid settings that kiri gives a diagram, whichever renderer
/// draws it, and that the diagram's own directive or front matter
/// overrides: GitHub's look and layout. GitHub draws with Mermaid 11,
/// which lays diagrams out with dagre; Mermaid 12, which merman follows,
/// with ELK.
const MERMAID_SITE: [(&str, &str); 2] = [("look", "classic"), ("layout", "dagre")];

/// The kinds of Mermaid diagram whose configuration has `useMaxWidth`,
/// as Mermaid 12 names them. Mermaid's configuration type leaves
/// `treemap` out, but its default configuration has it.
const MERMAID_DIAGRAMS: &[&str] = &[
    "flowchart",
    "swimlane",
    "agentflow",
    "sequence",
    "gantt",
    "journey",
    "timeline",
    "class",
    "state",
    "er",
    "pie",
    "quadrantChart",
    "xyChart",
    "requirement",
    "architecture",
    "mindmap",
    "ishikawa",
    "kanban",
    "gitGraph",
    "c4",
    "sankey",
    "packet",
    "block",
    "eventmodeling",
    "treeView",
    "radar",
    "usecase",
    "venn",
    "wardley-beta",
    "cynefin",
    "railroad",
    "treemap",
];

/// The Mermaid configuration that a command gets as `{config}`, which a
/// diagram's own directive or front matter overrides, for a command that
/// writes `output`.
///
/// It has each diagram drawn at its own width, rather than at most as wide
/// as the page it is drawn on, which in mmdc's browser is 800 pixels: kiri
/// shrinks a diagram to the window itself. It has the look and layout that
/// the built-in renderer gives too, so that both draw a diagram alike.
///
/// For SVG, it has the labels drawn as SVG text, which kiri can draw,
/// rather than as HTML, which it cannot: Mermaid 11 and later read the
/// first `htmlLabels`, Mermaid 10 the others. A raster image is a
/// browser's drawing, which has the labels as Mermaid draws them.
pub(crate) fn config(output: Output) -> String {
    let mut config = serde_json::json!({});
    for diagram in MERMAID_DIAGRAMS {
        config[diagram] = serde_json::json!({"useMaxWidth": false});
    }
    if output == Output::Svg {
        config["htmlLabels"] = false.into();
        for diagram in ["flowchart", "class", "state"] {
            config[diagram]["htmlLabels"] = false.into();
        }
    }
    for (key, value) in MERMAID_SITE {
        config[key] = value.into();
    }
    format!("{config}\n")
}

/// The page a Mermaid diagram chooses its colours for, in a directive
/// such as `%%{init: {"theme": "forest"}}%%` or under `config:` in its
/// front matter, read from the source for a renderer that does not tell.
/// `None` when it chooses nothing.
fn chosen_page(source: &str) -> Option<Theme> {
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

/// The first line of a diagram that a warning names it by: past any front
/// matter, directives and comments.
fn first_line(source: &str) -> Option<&str> {
    let mut lines = source.lines().map(str::trim);
    let mut front_matter = true;
    while let Some(text) = lines.next() {
        if text.is_empty() || text.starts_with("%%") {
            continue;
        }
        if text == "---" && front_matter {
            // Up to the next `---`.
            lines.by_ref().find(|text| *text == "---");
            front_matter = false;
            continue;
        }
        return Some(text);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(chosen_page(&source), theme, "{source}");
        }
    }

    #[test]
    fn mermaid_draws_diagrams_at_their_own_width() {
        // Not as wide as the page at most, which is 800 pixels in mmdc's
        // browser.
        for output in [Output::Svg, Output::Raster] {
            let config: serde_json::Value = serde_json::from_str(&config(output)).unwrap();
            let diagrams = [
                "flowchart",
                "sequence",
                "class",
                "state",
                "er",
                "gitGraph",
                "treemap",
            ];
            for diagram in diagrams {
                assert_eq!(config[diagram]["useMaxWidth"], false, "{diagram}");
            }
        }
    }

    #[test]
    fn a_diagram_s_first_line_is_past_its_front_matter_and_comments() {
        let source = "\n%%{init: {\"theme\": \"dark\"}}%%\n  flowchart TD\n  A --> B\n";
        assert_eq!(first_line(source), Some("flowchart TD"));
        let front_matter = "---\ntitle: T\n---\n%% a comment\nsequenceDiagram\n";
        assert_eq!(first_line(front_matter), Some("sequenceDiagram"));
        assert_eq!(first_line("---\ntitle: T\n---\n"), None);
        assert_eq!(first_line("\n"), None);
    }
}
