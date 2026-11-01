//! Mermaid diagrams, drawn by merman inside kiri.

use std::collections::HashSet;

use merman::svg::SvgPipeline;
use merman::theme_color::{ColorChannel, ThemeColor, is_dark};
use merman::{
    Engine, MermaidConfig, OperationControl, RenderOutput, RenderTarget, Renderer, SvgRequest,
};

use crate::typeset::diagrams::error::DiagramError;
use crate::typeset::diagrams::mermaid::{Colours, MERMAID, MERMAID_SITE, page};
use crate::typeset::diagrams::request::{Drawn, Theme};

/// merman's version. Cargo tells a crate only its own version, so a test
/// keeps this one in step with `Cargo.lock`.
const MERMAN_VERSION: &str = "0.9.1";

/// merman's version and the Mermaid version it follows.
pub fn version() -> String {
    format!(
        "merman {MERMAN_VERSION}, Mermaid {}",
        merman::baseline::PINNED_MERMAID_BASELINE_VERSION
    )
}

/// The diagram `source` as an SVG document, in `theme` unless the diagram
/// chooses its own, or merman's message saying why it could not be drawn.
pub fn render(source: &str, theme: Theme) -> Result<Drawn, DiagramError> {
    // GitHub's themes, look and layout. The site configuration is what a
    // diagram's own directive or front matter overrides.
    let mut site = serde_json::json!({ "theme": MERMAID.theme_names.of(theme) });
    for (key, value) in MERMAID_SITE {
        site[key] = value.into();
    }
    let site = MermaidConfig::from_value(site);
    let renderer = Renderer::new().with_engine(Engine::new().with_site_config(site));
    // SVG that resvg draws: text as SVG text rather than HTML labels.
    let request = SvgRequest {
        pipeline: Some(SvgPipeline::resvg_safe()),
        ..SvgRequest::default()
    };
    let no_diagram = || DiagramError::from("no Mermaid diagram");
    let diagram = (renderer.prepare_semantic(source, OperationControl::new()))
        .map_err(|error| DiagramError::from(error.to_string()))?
        .ok_or_else(no_diagram)?;
    // The diagram's own configuration over the site's.
    let config = diagram.metadata().effective_config.as_value();
    let theme = page_of(config).unwrap_or(theme);
    let background = background_of(config);
    match diagram.render(RenderTarget::Svg(request)) {
        Ok(RenderOutput::Svg(Some(svg))) => {
            let svg = with_references_found(&svg.into_parts().0);
            Ok(Drawn::svg(with_labels_unclipped(&svg), theme, background))
        }
        Ok(_) => Err(no_diagram()),
        Err(error) => Err(error.to_string().into()),
    }
}

/// The page that a diagram of the Mermaid configuration `config` is drawn
/// for, as `mermaid::page` decides it. `None` when the configuration does
/// not say.
fn page_of(config: &serde_json::Value) -> Option<Theme> {
    let variables = config.get("themeVariables");
    let variable = |name: &str| variables.and_then(|variables| variables.get(name));
    let colours = Colours {
        theme: config.get("theme").and_then(serde_json::Value::as_str),
        dark_mode: variable("darkMode").and_then(serde_json::Value::as_bool) == Some(true),
        background: variable("background").and_then(serde_json::Value::as_str),
    };
    page(&colours, |colour| is_dark(colour).ok())
}

/// The background colour of the theme in the Mermaid configuration
/// `config`, when it gives one that is a colour.
fn background_of(config: &serde_json::Value) -> Option<[u8; 3]> {
    let background = config.get("themeVariables")?.get("background")?.as_str()?;
    let colour = ThemeColor::parse(background).ok()?;
    let channel = |channel| colour.channel(channel).round().clamp(0.0, 255.0) as u8;
    Some([
        channel(ColorChannel::Red),
        channel(ColorChannel::Green),
        channel(ColorChannel::Blue),
    ])
}

/// `svg` with each reference to an element it does not have, such as
/// `url(#drop-shadow)`, pointed at the element of that name that merman
/// prefixes with the diagram's id, `merman-drop-shadow`. resvg leaves out
/// an element whose filter is missing, and with it a sequence diagram's
/// notes in Mermaid 12's look.
fn with_references_found(svg: &str) -> String {
    let ids: HashSet<&str> = attribute_values(svg, " id=\"").collect();
    let Some(root) = attribute_values(svg, "<svg id=\"").next() else {
        return svg.to_owned();
    };
    let mut fixed = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(start) = rest.find("url(#") {
        let name_start = start + "url(#".len();
        let Some(length) = rest[name_start..].find(')') else {
            break;
        };
        let name = &rest[name_start..name_start + length];
        fixed.push_str(&rest[..name_start]);
        let prefixed = format!("{root}-{name}");
        if !ids.contains(name) && ids.contains(prefixed.as_str()) {
            fixed.push_str(&prefixed);
        } else {
            fixed.push_str(name);
        }
        rest = &rest[name_start + length..];
    }
    fixed.push_str(rest);
    fixed
}

/// `svg` with the boxes around labels, nested `<svg>` elements, letting
/// their text overflow. merman sizes a box by its own measure of the text,
/// and the font resvg draws it with is often wider, which would cut off
/// the ends of class diagram labels.
fn with_labels_unclipped(svg: &str) -> String {
    let mut fixed = String::with_capacity(svg.len());
    let mut rest = svg;
    // The root element is left as it is.
    let mut root = true;
    while let Some(start) = rest.find("<svg") {
        let Some(length) = rest[start..].find('>') else {
            break;
        };
        let end = start + length;
        fixed.push_str(&rest[..start]);
        let tag = &rest[start..end];
        if root {
            fixed.push_str(tag);
            root = false;
        } else {
            fixed.push_str(&tag.replace("overflow:hidden", "overflow:visible"));
        }
        rest = &rest[end..];
    }
    fixed.push_str(rest);
    fixed
}

/// The values of the attribute that `start`, such as ` id="`, begins, in
/// the order they come.
fn attribute_values<'a>(svg: &'a str, start: &'a str) -> impl Iterator<Item = &'a str> {
    svg.match_indices(start).filter_map(move |(index, _)| {
        let value = &svg[index + start.len()..];
        value.find('"').map(|end| &value[..end])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::Fonts;
    use crate::typeset::images::drawn::decode_diagram;
    use crate::typeset::images::{DEFAULT_MAX_FILE_SIZE, Source, decode};
    use resvg::usvg;

    const FLOWCHART: &str = "flowchart TD\n    A[Start] --> B[Done]\n";

    #[test]
    fn the_merman_version_is_the_one_in_cargo_lock() {
        let lock = include_str!("../../../../Cargo.lock");
        let entry = format!("name = \"merman\"\nversion = \"{MERMAN_VERSION}\"\n");
        assert!(lock.contains(&entry), "{entry}");
        assert!(version().starts_with("merman "), "{}", version());
    }

    /// The fill of a node in the CSS of `svg`.
    fn node_fill(svg: &str) -> &str {
        let rule = svg.find(".node rect").expect("a rule for nodes");
        let fill = rule + svg[rule..].find("fill:").expect("a fill") + "fill:".len();
        let end = fill + svg[fill..].find(';').expect("the end of the fill");
        &svg[fill..end]
    }

    #[test]
    fn draws_a_flowchart_as_svg() {
        let svg = render(FLOWCHART, Theme::Light).unwrap().text();
        assert!(svg.starts_with("<svg"), "{svg}");
        assert!(svg.contains("Start") && svg.contains("Done"), "{svg}");
    }

    #[test]
    fn draws_in_the_default_theme_on_a_light_page_and_dark_on_a_dark_one() {
        // The node colours of Mermaid's default and dark themes.
        assert_eq!(
            node_fill(&render(FLOWCHART, Theme::Light).unwrap().text()),
            "#ECECFF"
        );
        assert_eq!(
            node_fill(&render(FLOWCHART, Theme::Dark).unwrap().text()),
            "#1f2020"
        );
    }

    #[test]
    fn a_theme_the_diagram_sets_itself_wins() {
        // Forest's node colour, from a directive and from front matter.
        let directive = format!("%%{{init: {{\"theme\": \"forest\"}}}}%%\n{FLOWCHART}");
        let front_matter = format!("---\nconfig:\n  theme: forest\n---\n{FLOWCHART}");
        for source in [directive, front_matter] {
            for theme in [Theme::Light, Theme::Dark] {
                assert_eq!(
                    node_fill(&render(&source, theme).unwrap().text()),
                    "#cde498"
                );
            }
        }
    }

    #[test]
    fn tells_the_theme_a_diagram_chooses_for_itself() {
        let drawn = |source: &str, page| render(source, page).unwrap().theme;
        let with = |theme: &str| format!("---\nconfig:\n  theme: {theme}\n---\n{FLOWCHART}");
        // kiri's own choice.
        assert_eq!(drawn(FLOWCHART, Theme::Dark), Theme::Dark);
        assert_eq!(drawn(FLOWCHART, Theme::Light), Theme::Light);
        for theme in ["default", "forest", "neutral", "base", "neo", "redux"] {
            assert_eq!(drawn(&with(theme), Theme::Dark), Theme::Light, "{theme}");
        }
        for theme in ["dark", "neo-dark", "redux-dark"] {
            assert_eq!(drawn(&with(theme), Theme::Light), Theme::Dark, "{theme}");
        }
        let directive = format!("%%{{init: {{\"theme\": \"default\"}}}}%%\n{FLOWCHART}");
        assert_eq!(drawn(&directive, Theme::Dark), Theme::Light);
        // Mermaid's switch for the base theme on a dark page.
        let dark_base = format!(
            "---\nconfig:\n  theme: base\n  themeVariables:\n    darkMode: true\n---\n{FLOWCHART}"
        );
        assert_eq!(drawn(&dark_base, Theme::Light), Theme::Dark);
    }

    #[test]
    fn the_page_is_the_one_the_source_tells_for_other_renderers() {
        // External programs draw the same diagrams; kiri reads their page
        // from the source.
        for source in super::super::tests::CHOOSING {
            let source = format!("{source}{FLOWCHART}");
            for page in [Theme::Light, Theme::Dark] {
                let chosen = super::super::chosen_page(&source).unwrap_or(page);
                let drawn = render(&source, page).unwrap().theme;
                assert_eq!(drawn, chosen, "{source} on {page:?}");
            }
        }
    }

    #[test]
    fn tells_the_background_of_a_dark_theme() {
        let background = |source: &str, page| render(source, page).unwrap().background;
        // Mermaid's dark theme.
        assert_eq!(background(FLOWCHART, Theme::Dark), Some([0x33, 0x33, 0x33]));
        let own = |colour: &str| {
            format!(
                "---\nconfig:\n  theme: base\n  themeVariables:\n    darkMode: true\n    \
                 background: \"{colour}\"\n---\n{FLOWCHART}"
            )
        };
        assert_eq!(
            background(&own("#102030"), Theme::Light),
            Some([0x10, 0x20, 0x30])
        );
        assert_eq!(
            background(&own("hsl(0, 0%, 10%)"), Theme::Light),
            Some([26, 26, 26])
        );
    }

    /// How many pixels of `svg`, drawn at its natural size, are `colour`.
    fn pixels_of(svg: &str, colour: [u8; 3]) -> usize {
        let fonts = Fonts::none();
        let image = decode(svg.as_bytes(), DEFAULT_MAX_FILE_SIZE, &fonts).unwrap();
        let (width, height) = image.size();
        let drawn = image.render(width, height);
        let same = |pixel: &&image::Rgba<u8>| pixel.0[..3] == colour && pixel.0[3] == 255;
        drawn.pixels().filter(same).count()
    }

    /// The paths of the edges between the nodes of a flowchart.
    fn edges(svg: &str) -> Vec<&str> {
        let paths = svg.match_indices("<path ").map(|(at, _)| {
            let tag = &svg[at..];
            &tag[..tag.find('>').expect("the end of the tag")]
        });
        let links: Vec<&str> = paths.filter(|tag| tag.contains("flowchart-link")).collect();
        assert!(!links.is_empty(), "{svg}");
        (links.into_iter())
            .map(|tag| attribute_values(tag, " d=\"").next().expect("a path"))
            .collect()
    }

    #[test]
    fn flowcharts_are_laid_out_with_curved_edges_as_on_github() {
        // Dagre, Mermaid 11's layout, which GitHub uses, draws curves; ELK,
        // Mermaid 12's and merman's default, lines at right angles.
        let source = "flowchart TD\n    A[Start] --> B{Ok?}\n    B -->|yes| C[Done]\n\
            B -->|no| A\n";
        let curved = |source: &str| {
            let svg = render(source, Theme::Light).unwrap().text();
            let edges = edges(&svg);
            (edges.iter().all(|d| d.contains('C')), edges.join("\n"))
        };
        let (all, edges) = curved(source);
        assert!(all, "{edges}");
        // A diagram may still ask for ELK.
        let elk = format!("---\nconfig:\n  layout: elk\n---\n{source}");
        let (all, edges) = curved(&elk);
        assert!(!all && !edges.contains('C'), "{edges}");
    }

    #[test]
    fn notes_keep_their_shadow_filter_in_mermaid_12_s_look() {
        // The CSS of this look points notes at `#drop-shadow`, which merman
        // names `merman-drop-shadow`; resvg leaves out an element whose
        // filter is missing.
        let source = "---\nconfig:\n  look: neo\n  theme: redux-color\n---\n\
            sequenceDiagram\n    A->>B: hello\n    Note right of B: a note\n";
        let svg = render(source, Theme::Light).unwrap().text();
        // The note's fill in this theme.
        let note = pixels_of(&svg, [0xff, 0xf5, 0xad]);
        assert!(note > 1000, "{note} pixels of the note's colour");
    }

    /// How many groups in `group` and below are clipped.
    fn clipped(group: &usvg::Group) -> usize {
        let own = usize::from(group.clip_path().is_some());
        let below: usize = (group.children().iter())
            .map(|node| match node {
                usvg::Node::Group(group) => clipped(group),
                _ => 0,
            })
            .sum();
        own + below
    }

    #[test]
    fn class_diagram_labels_are_not_clipped() {
        // merman measures text with its own metrics and fits each label in
        // a box of that size; the font resvg draws with is often wider.
        let source = "classDiagram\n    class KittyBackend {\n        -Capabilities caps\n\
            +render(doc, out) Result\n    }\n";
        let svg = render(source, Theme::Light).unwrap().text();
        let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).unwrap();
        assert_eq!(clipped(tree.root()), 0);
    }

    #[test]
    fn merman_still_places_an_edge_label_past_the_left_edge() {
        // merman measures text with metrics of its own, narrower than many
        // fonts, such as DejaVu Sans here, and centres this label so close
        // to the left edge that its first letter falls outside the drawing.
        // kiri grows each diagram to show all of its text
        // (`typeset::images::drawn::decode_diagram`). When this test fails, merman has
        // fixed the fault, and the growing can go unless another renderer
        // needs it.
        let source = "stateDiagram-v2\n    [*] --> Parsed\n\
            Parsed --> Loaded: images and diagrams\n    Loaded --> Shown\n";
        let svg = render(source, Theme::Light).unwrap().text();
        let fonts = Fonts::fixture();
        let width = |image: Source| image.size().0;
        let file = width(decode(svg.as_bytes(), DEFAULT_MAX_FILE_SIZE, &fonts).unwrap());
        let grown = width(decode_diagram(svg.as_bytes(), DEFAULT_MAX_FILE_SIZE, &fonts).unwrap());
        assert!(
            grown > file,
            "{grown} pixels grown, {file} as merman drew it"
        );
    }

    #[test]
    fn lets_only_nested_svg_elements_overflow() {
        let svg = "<svg id=\"merman\" style=\"overflow:hidden\"><g style=\"overflow:hidden\">\
            <svg style=\"overflow:hidden;opacity:1\"><text>overflow:hidden</text></svg></g></svg>";
        assert_eq!(
            with_labels_unclipped(svg),
            "<svg id=\"merman\" style=\"overflow:hidden\"><g style=\"overflow:hidden\">\
            <svg style=\"overflow:visible;opacity:1\"><text>overflow:hidden</text></svg></g></svg>"
        );
    }

    #[test]
    fn points_references_at_the_elements_merman_prefixes() {
        let svg = "<svg id=\"merman\"><style>.a{filter:url(#shadow);}\
            .b{fill:url(#gradient);}.c{filter:url(#none)}</style>\
            <filter id=\"merman-shadow\"/><linearGradient id=\"gradient\"/>\
            <linearGradient id=\"merman-gradient\"/></svg>";
        let fixed = with_references_found(svg);
        assert!(fixed.contains(".a{filter:url(#merman-shadow);}"), "{fixed}");
        // An element of the name itself, or none either way, is left.
        assert!(fixed.contains(".b{fill:url(#gradient);}"), "{fixed}");
        assert!(fixed.contains(".c{filter:url(#none)}"), "{fixed}");
        assert_eq!(fixed.len(), svg.len() + "merman-".len());
    }

    #[test]
    fn a_syntax_error_is_merman_s_message() {
        let error = render("flowchart TD\n    A[Unclosed --> B\n", Theme::Light)
            .unwrap_err()
            .to_string();
        assert!(error.contains("missing `]`"), "{error}");
        let error = render("not a diagram\n", Theme::Light)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("No Mermaid diagram type detected"),
            "{error}"
        );
    }
}
