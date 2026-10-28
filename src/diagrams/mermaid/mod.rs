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
    noun: None,
    centred: false,
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
    render: |request| merman::render(request.source, request.theme),
    baseline: |_| None,
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
    let colours = Colours {
        theme: value_of(&settings, "theme"),
        dark_mode: value_of(&settings, "darkMode") == Some("true"),
        background: value_of(&settings, "background"),
    };
    page(&colours, is_dark)
}

/// What a diagram's Mermaid settings say about its colours, as they are
/// written.
#[derive(Debug, Default)]
pub(super) struct Colours<'a> {
    /// Its theme, such as `forest`.
    pub theme: Option<&'a str>,
    /// Mermaid's dark mode, `darkMode` among the theme's variables.
    pub dark_mode: bool,
    /// Its background, `background` among the theme's variables.
    pub background: Option<&'a str>,
}

/// The backgrounds of Mermaid 12's themes.
const THEME_BACKGROUNDS: &[(&str, &str)] = &[
    ("base", "#f4f4f4"),
    ("dark", "#333"),
    ("default", "white"),
    ("forest", "white"),
    ("neo", "#ffffff"),
    ("neo-dark", "#333"),
    ("neutral", "#ffffff"),
    ("redux", "#ffffff"),
    ("redux-color", "#ffffff"),
    ("redux-dark", "#333"),
    ("redux-dark-color", "#333"),
];

/// The page a diagram whose settings say `colours` is drawn for, as
/// Mermaid derives its other colours: a dark one in dark mode, and
/// otherwise one as dark as its background, its own or else its theme's,
/// as `is_dark` tells a colour. `None` when the settings choose neither a
/// background that `is_dark` tells nor a theme that Mermaid has, which it
/// leaves out. Every renderer decides by this, whether it reads the
/// settings from the diagram's source or from the renderer.
pub(super) fn page(colours: &Colours, is_dark: impl Fn(&str) -> Option<bool>) -> Option<Theme> {
    if colours.dark_mode {
        return Some(Theme::Dark);
    }
    let theme = (colours.theme)
        .and_then(|name| THEME_BACKGROUNDS.iter().find(|(theme, _)| *theme == name))
        .map(|&(_, background)| background);
    let dark = (colours.background.and_then(&is_dark)).or_else(|| theme.and_then(&is_dark))?;
    Some(if dark { Theme::Dark } else { Theme::Light })
}

/// Whether the CSS colour `colour` is dark, as Mermaid tells: its relative
/// luminance is below a half. `None` for what is no colour written in hex,
/// `white` or `black`.
fn is_dark(colour: &str) -> Option<bool> {
    let [red, green, blue] = match colour.to_ascii_lowercase().as_str() {
        "white" => [255; 3],
        "black" => [0; 3],
        hex => {
            let digits = hex.strip_prefix('#')?;
            let digit = |at: usize, of: usize| -> Option<u8> {
                let text = digits.get(at * of..(at + 1) * of)?;
                let value = u8::from_str_radix(text, 16).ok()?;
                Some(if of == 1 { value * 17 } else { value })
            };
            let of = match digits.len() {
                3 | 4 => 1,
                6 | 8 => 2,
                _ => return None,
            };
            [digit(0, of)?, digit(1, of)?, digit(2, of)?]
        }
    };
    // As Mermaid's colour library computes it, which compares the channel
    // before it is scaled to 0..1 with the threshold.
    let linear = |channel: u8| {
        let scaled = f64::from(channel) / 255.0;
        match channel {
            0 => 0.0,
            _ => ((scaled + 0.055) / 1.055).powf(2.4),
        }
    };
    let luminance = 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue);
    Some(luminance < 0.5)
}

/// The value of the first `key` in `text`, YAML or JSON: a quoted string,
/// or else a word.
fn value_of<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let quote = |c: char| c == '"' || c == '\'';
    text.match_indices(key).find_map(|(at, _)| {
        if text[..at].ends_with(word) {
            return None;
        }
        let rest = text[at + key.len()..].trim_start_matches(quote);
        let rest = rest.trim_start_matches([' ', '\t']).strip_prefix(':')?;
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(open) if quote(open) => {
                let rest = &rest[1..];
                &rest[..rest.find(open)?]
            }
            _ => &rest[..rest.find(|c: char| !word(c)).unwrap_or(rest.len())],
        };
        Some(value).filter(|value| !value.is_empty())
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
pub(super) mod tests {
    use super::*;

    /// The start of diagrams that choose their colours, or try to, in
    /// their front matter or a directive.
    pub(super) const CHOOSING: &[&str] = &[
        "",
        "---\nconfig:\n  theme: forest\n---\n",
        "---\nconfig:\n  theme: neo-dark\n---\n",
        "---\nconfig:\n  theme: redux-dark-color\n---\n",
        "---\nconfig:\n  theme: base\n  themeVariables:\n    darkMode: true\n---\n",
        "---\nconfig:\n  theme: \"no-such-dark\"\n---\n",
        "---\nconfig:\n  theme: \"null\"\n---\n",
        "%%{init: {\"theme\":\"base\",\"themeVariables\":{\"background\":\"#111111\"}}}%%\n",
        "%%{init: {'theme': 'dark', 'themeVariables': {'background': '#fff'}}}%%\n",
        "%%{init: {\"theme\": \"dark\", \"themeVariables\": {\"background\": \"#ffffff\", \"darkMode\": true}}}%%\n",
    ];

    #[test]
    fn tells_a_dark_colour_from_a_light_one() {
        for dark in ["#333", "#333333", "#000a", "#11111180", "black", "#7a7a7a"] {
            assert_eq!(is_dark(dark), Some(true), "{dark}");
        }
        for light in ["#f4f4f4", "#FFF", "White", "#bcbcbc"] {
            assert_eq!(is_dark(light), Some(false), "{light}");
        }
        for unknown in ["", "#12", "#gggggg", "hsl(0, 0%, 10%)", "navy"] {
            assert_eq!(is_dark(unknown), None, "{unknown}");
        }
    }

    #[test]
    fn the_page_is_as_dark_as_the_background_mermaid_draws_for() {
        // Dark mode, and otherwise the diagram's own background or its
        // theme's, as Mermaid derives the other colours from them.
        let expected = [
            None,
            Some(Theme::Light),
            Some(Theme::Dark),
            Some(Theme::Dark),
            Some(Theme::Dark),
            // A theme Mermaid does not have, which it leaves out.
            None,
            None,
            Some(Theme::Dark),
            Some(Theme::Light),
            Some(Theme::Dark),
        ];
        for (source, page) in CHOOSING.iter().zip(expected) {
            let source = format!("{source}flowchart TD\n  A --> B\n");
            assert_eq!(chosen_page(&source), page, "{source}");
        }
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
