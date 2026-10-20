//! Mermaid diagrams, drawn by merman.

use std::collections::HashSet;

use merman::svg::SvgPipeline;
use merman::{
    Engine, MermaidConfig, OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest,
};

use super::Theme;

/// The diagram `source` as an SVG document, in `theme` unless the diagram
/// chooses its own, or merman's message saying why it could not be drawn.
pub fn render(source: &str, theme: Theme) -> Result<String, String> {
    // GitHub's themes and look. The site configuration is what a diagram's
    // own directive or front matter overrides.
    let site = MermaidConfig::from_value(serde_json::json!({
        "theme": match theme {
            Theme::Light => "default",
            Theme::Dark => "dark",
        },
        "look": "classic",
    }));
    let renderer = Renderer::new().with_engine(Engine::new().with_site_config(site));
    // SVG that resvg draws: text as SVG text rather than HTML labels.
    let request = SvgRequest {
        pipeline: Some(SvgPipeline::resvg_safe()),
        ..SvgRequest::default()
    };
    match renderer.render(RenderRequest::svg(source, OperationControl::new(), request)) {
        Ok(RenderOutput::Svg(Some(svg))) => Ok(with_references_found(&svg.into_parts().0)),
        Ok(_) => Err("no Mermaid diagram".into()),
        Err(error) => Err(error.to_string()),
    }
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
    use crate::images::{DEFAULT_MAX_FILE_SIZE, Fonts, decode};

    const FLOWCHART: &str = "flowchart TD\n    A[Start] --> B[Done]\n";

    /// The fill of a node in the CSS of `svg`.
    fn node_fill(svg: &str) -> &str {
        let rule = svg.find(".node rect").expect("a rule for nodes");
        let fill = rule + svg[rule..].find("fill:").expect("a fill") + "fill:".len();
        let end = fill + svg[fill..].find(';').expect("the end of the fill");
        &svg[fill..end]
    }

    #[test]
    fn draws_a_flowchart_as_svg() {
        let svg = render(FLOWCHART, Theme::Light).unwrap();
        assert!(svg.starts_with("<svg"), "{svg}");
        assert!(svg.contains("Start") && svg.contains("Done"), "{svg}");
    }

    #[test]
    fn draws_in_the_default_theme_on_a_light_page_and_dark_on_a_dark_one() {
        // The node colours of Mermaid's default and dark themes.
        assert_eq!(
            node_fill(&render(FLOWCHART, Theme::Light).unwrap()),
            "#ECECFF"
        );
        assert_eq!(
            node_fill(&render(FLOWCHART, Theme::Dark).unwrap()),
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
                assert_eq!(node_fill(&render(&source, theme).unwrap()), "#cde498");
            }
        }
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

    #[test]
    fn notes_keep_their_shadow_filter_in_mermaid_12_s_look() {
        // The CSS of this look points notes at `#drop-shadow`, which merman
        // names `merman-drop-shadow`; resvg leaves out an element whose
        // filter is missing.
        let source = "---\nconfig:\n  look: neo\n  theme: redux-color\n---\n\
            sequenceDiagram\n    A->>B: hello\n    Note right of B: a note\n";
        let svg = render(source, Theme::Light).unwrap();
        // The note's fill in this theme.
        let note = pixels_of(&svg, [0xff, 0xf5, 0xad]);
        assert!(note > 1000, "{note} pixels of the note's colour");
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
        let error = render("flowchart TD\n    A[Unclosed --> B\n", Theme::Light).unwrap_err();
        assert!(error.contains("missing `]`"), "{error}");
        let error = render("not a diagram\n", Theme::Light).unwrap_err();
        assert!(
            error.contains("No Mermaid diagram type detected"),
            "{error}"
        );
    }
}
