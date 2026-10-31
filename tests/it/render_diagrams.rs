//! Diagrams in code blocks: drawn as images where they can be, and code
//! otherwise.

use image::RgbaImage;
use kiri::images::{DEFAULT_MAX_FILE_SIZE, Fonts, ImageError, ImageKey, Images, Source};
use kiri::render::{Inputs, RenderErrors};
use kiri::typeset::caps::Capabilities;
use kiri::typeset::theme::{Paper, Theme};

use crate::common::{
    WIDTH, drawn, fixture_images_with, readable, render, render_fixture, sent, stub_diagram_images,
    stub_diagrams, visible, with_images,
};

const FLOWCHART: &str = "Before.\n\n```mermaid\nflowchart TD\n  A --> B\n```\n\nAfter.\n";

#[test]
fn kitty_mermaid() {
    let images = fixture_images_with("mermaid.md", &stub_diagrams());
    insta::assert_snapshot!(readable(&render_fixture(
        "mermaid.md",
        &with_images(&Capabilities::kitty(WIDTH), images)
    )));
}

#[test]
fn kitty_math() {
    let options = kiri::images::LoadOptions {
        renderer: &kiri::diagrams::BuiltIn,
        ..kiri::images::LoadOptions::default()
    };
    let images = fixture_images_with("math.md", &options);
    insta::assert_snapshot!(readable(&render_fixture(
        "math.md",
        &with_images(&Capabilities::kitty(WIDTH), images)
    )));
}

#[test]
fn a_diagram_is_drawn_in_place_of_its_code() {
    let images = stub_diagram_images(FLOWCHART);
    let out = render(FLOWCHART, &with_images(&Capabilities::kitty(WIDTH), images));
    assert_eq!(sent(&out).len(), 1, "{out:?}");
    // Two lines and 23 characters of source, 330 × 40 CSS pixels, in
    // cells of 10 × 20.
    assert_eq!(drawn(&out), ["⟦row 0: 33 cells⟧", "⟦row 1: 33 cells⟧"]);
    assert!(!out.contains("flowchart"), "{out:?}");
    assert!(out.contains("Before.") && out.contains("After."), "{out:?}");
}

#[test]
fn a_diagram_that_did_not_draw_stays_code() {
    let source = "```mermaid\nflowchart TD\n  A[open --> B\n```\n";
    let images = stub_diagram_images(source);
    let out = render(source, &with_images(&Capabilities::kitty(WIDTH), images));
    assert!(sent(&out).is_empty(), "{out:?}");
    assert!(out.contains("A[open --> B"), "{out:?}");
}

#[test]
fn diagrams_stay_code_without_images_or_graphics() {
    // No images loaded, as with --no-images.
    let out = render(
        FLOWCHART,
        &with_images(&Capabilities::kitty(WIDTH), Images::new()),
    );
    assert!(sent(&out).is_empty(), "{out:?}");
    assert!(out.contains("flowchart TD"), "{out:?}");
    // Loaded, but the terminal cannot draw them.
    for caps in [Capabilities::ansi(WIDTH), Capabilities::plain(WIDTH)] {
        let images = stub_diagram_images(FLOWCHART);
        let out = render(FLOWCHART, &with_images(&caps, images));
        assert!(drawn(&out).is_empty(), "{out:?}");
        assert!(out.contains("flowchart TD"), "{out:?}");
    }
}

/// What `out` sent of its one image: its size in pixels and in cells,
/// which are whole cells, and how much of it the image covers.
fn placed(out: &str) -> ([String; 4], (u32, u32)) {
    let sent = sent(out);
    assert_eq!(sent.len(), 1, "{out:?}");
    let keys = ["s", "v", "c", "r"].map(|key| sent[0].keys[key].clone());
    let width: u32 = keys[0].parse().unwrap();
    let height = sent[0].rgba.len() as u32 / 4 / width;
    let image = RgbaImage::from_raw(width, height, sent[0].rgba.clone()).unwrap();
    let covered = (image.enumerate_pixels())
        .filter(|(_, _, pixel)| pixel.0[3] > 0)
        .fold((0, 0), |(w, h), (x, y, _)| (w.max(x + 1), h.max(y + 1)));
    (keys, covered)
}

#[test]
fn a_raster_diagram_is_placed_at_the_size_of_its_css_pixels() {
    // As a browser draws it at three pixels to the CSS pixel: 60 × 30
    // pixels are 20 × 10 CSS pixels, as large as such an SVG.
    let dense = Source::Dense {
        image: RgbaImage::from_pixel(60, 30, image::Rgba([0, 0, 255, 255])),
        css: (20, 10),
    };
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
        <rect width="20" height="10" fill="#0000ff"/></svg>"##;
    let svg = kiri::images::decode(svg, DEFAULT_MAX_FILE_SIZE, &Fonts::none()).unwrap();
    assert!(matches!(svg, Source::Svg(_)));
    // Cells 40 pixels high, two terminal pixels to the CSS pixel.
    let caps = Capabilities {
        height: Some(24),
        cell_pixels: Some((10, 40)),
        ..Capabilities::kitty(WIDTH)
    };
    let source = "```mermaid\nflowchart TD\n```\n";
    let key = ImageKey::block("mermaid", "flowchart TD\n");
    let in_document = |image: &Source| {
        let mut images = Images::new();
        images.insert(key.clone(), Ok(image.clone()));
        placed(&render(source, &with_images(&caps, images)))
    };
    let alone = |image: &Source| {
        let mut out = Vec::new();
        kiri::render::render_image(image.clone(), &caps, &mut out).unwrap();
        placed(&String::from_utf8(out).unwrap())
    };
    // 40 × 20 pixels, in a sent image of whole cells.
    let expected = (["40", "40", "4", "1"].map(String::from), (40, 20));
    assert_eq!(in_document(&svg), expected);
    assert_eq!(in_document(&dense), expected);
    assert_eq!(alone(&svg), expected);
    assert_eq!(alone(&dense), expected);
}

/// Diagrams that `StubDiagrams` cannot draw: at the top level, in a list,
/// a quote, an alert and details.
const BROKEN: &str = "Before.\n\n```mermaid\nflowchart TD\n  A[open --> B\n```\n\nAfter.\n\n\
                      - Item:\n\n  ```mermaid\n  flowchart LR\n    C[shut\n  ```\n\n\
                      > Quoted:\n>\n> ```mermaid\n> flowchart TD\n>   D[quoted\n> ```\n\n\
                      > [!NOTE]\n> ```mermaid\n> flowchart TD\n>   E[noted\n> ```\n\n\
                      <details>\n<summary>More</summary>\n\n\
                      ```mermaid\nflowchart TD\n  F[hidden\n```\n\n</details>\n";

/// How many diagrams `BROKEN` has.
const BROKEN_COUNT: usize = 5;

/// `BROKEN` rendered for `caps` with `theme`, its errors told as `errors`
/// says.
fn broken(caps: &Capabilities, theme: Theme, errors: RenderErrors) -> String {
    let inputs = Inputs {
        images: stub_diagram_images(BROKEN),
        theme,
        render_errors: errors,
        ..Inputs::new(caps.clone())
    };
    render(BROKEN, &inputs)
}

const WHY: &str = "mermaid diagram could not be drawn: unclosed label";

#[test]
fn a_diagram_that_did_not_draw_tells_why_below_its_code() {
    let out = broken(
        &Capabilities::kitty(WIDTH),
        Theme::default(),
        RenderErrors::default(),
    );
    assert!(sent(&out).is_empty(), "{out:?}");
    insta::assert_snapshot!(visible(&out));
}

#[test]
fn the_error_below_a_diagram_is_red_on_the_paper_pages() {
    for (paper, name) in [(Paper::Light, "light"), (Paper::Dark, "dark")] {
        let out = broken(
            &Capabilities::kitty(WIDTH),
            Theme::paper(paper),
            RenderErrors::Both,
        );
        insta::assert_snapshot!(
            format!("error_below_a_diagram_on_{name}_paper"),
            visible(&out)
        );
    }
}

#[test]
fn the_error_below_a_diagram_is_plain_text_in_plain_output() {
    let out = broken(
        &Capabilities::plain(WIDTH),
        Theme::default(),
        RenderErrors::Both,
    );
    assert!(!out.contains('\x1b'), "{out:?}");
    insta::assert_snapshot!(out);
}

#[test]
fn the_error_below_a_diagram_has_no_colour_without_colours() {
    let caps = Capabilities {
        color: false,
        ..Capabilities::kitty(WIDTH)
    };
    let out = broken(&caps, Theme::default(), RenderErrors::Both);
    assert!(out.contains("✗ mermaid diagram could not be"), "{out:?}");
    assert!(crate::common::colour_codes(&out).is_empty(), "{out:?}");
}

#[test]
fn the_setting_decides_whether_the_document_tells_why() {
    for (errors, inline) in [
        (RenderErrors::Inline, true),
        (RenderErrors::Both, true),
        (RenderErrors::Stderr, false),
    ] {
        for caps in [Capabilities::kitty(200), Capabilities::plain(200)] {
            let out = broken(&caps, Theme::default(), errors);
            assert_eq!(
                out.matches(WHY).count(),
                BROKEN_COUNT * usize::from(inline),
                "{errors:?}: {out:?}"
            );
            assert!(out.contains("A[open --> B"), "{errors:?}: {out:?}");
        }
    }
    // Nothing to tell for a diagram that was drawn, or for a code block
    // that is no diagram.
    let source = format!("{FLOWCHART}\n```rust\nfn main() {{\n```\n");
    let images = stub_diagram_images(&source);
    let out = render(&source, &with_images(&Capabilities::kitty(WIDTH), images));
    assert!(!out.contains("could not be drawn"), "{out:?}");
}

#[test]
fn the_error_below_a_diagram_wraps_in_a_narrow_window() {
    let out = broken(
        &Capabilities::plain(20),
        Theme::default(),
        RenderErrors::Both,
    );
    insta::assert_snapshot!(out);
    // Down to windows too narrow for a word.
    for width in 3..=12 {
        for caps in [Capabilities::kitty(width), Capabilities::plain(width)] {
            let out = broken(&caps, Theme::default(), RenderErrors::Both);
            assert_eq!(out.matches('✗').count(), BROKEN_COUNT, "{width}: {out:?}");
        }
    }
}

/// A diagram that does not draw, alone.
const ONE_BROKEN: &str = "```mermaid\nflowchart TD\n  A[open --> B\n```\n";

/// `ONE_BROKEN` rendered for `caps` with `theme`, without kiri's own
/// sequences: its code's lines and then its error's lines.
fn one_broken(caps: Capabilities, theme: Theme) -> (Vec<String>, Vec<String>) {
    let inputs = Inputs {
        images: stub_diagram_images(ONE_BROKEN),
        theme,
        ..Inputs::new(caps)
    };
    let shown = crate::common::strip_own_sequences(&render(ONE_BROKEN, &inputs));
    let lines: Vec<String> = shown.lines().map(str::to_owned).collect();
    let error = lines.iter().position(|line| line.contains('✗'));
    let error = error.unwrap_or_else(|| panic!("{shown}"));
    let (code, error) = lines.split_at(error);
    (code.to_vec(), error.to_vec())
}

#[test]
fn the_error_below_a_diagram_is_no_wider_than_the_code_in_its_box() {
    let cells = |line: &str| line.trim_end().chars().count();
    for width in 20..=110 {
        let (code, error) = one_broken(Capabilities::kitty(width), Theme::default());
        // The box's lines reach its right edge, and its code starts a cell
        // in from its left edge.
        let edge = code[0].chars().count();
        let left = code[1].len() - code[1].trim_start().len() - 1;
        for line in &error {
            assert!(cells(line) <= edge - 2, "{width}: {code:?} {error:?}");
            assert!(line[..left].trim().is_empty(), "{width}: {error:?}");
        }
        assert!(error[0][left..].starts_with('✗'), "{width}: {error:?}");
    }
}

#[test]
fn without_a_code_box_the_error_lines_up_with_the_code() {
    let no_padding = Theme {
        code_padding: false,
        ..Theme::default()
    };
    let colourless = Capabilities {
        color: false,
        ..Capabilities::kitty(WIDTH)
    };
    // The margin, and the indent of code without a box.
    for (caps, theme, indent) in [
        (Capabilities::plain(WIDTH), Theme::default(), "    "),
        (colourless, Theme::default(), "    "),
        (Capabilities::plain(WIDTH), no_padding.clone(), "  "),
        (Capabilities::kitty(WIDTH), no_padding, "  "),
    ] {
        let (code, error) = one_broken(caps, theme);
        assert!(
            code[0].starts_with(&format!("{indent}flowchart")),
            "{code:?}"
        );
        assert!(error[0].starts_with(&format!("{indent}✗")), "{error:?}");
        for line in &error[1..] {
            let text = line
                .strip_prefix(indent)
                .unwrap_or_else(|| panic!("{error:?}"));
            assert!(!text.starts_with(' '), "{error:?}");
        }
    }
}

#[test]
fn an_error_of_several_lines_keeps_them_apart() {
    // As mmdc tells a parse error: its pointer points at a column of the
    // line above it, so the two stay lines of their own, side by side.
    let mut images = Images::new();
    images.insert(
        ImageKey::block("mermaid", "flowchart TD\n  A[open --> B\n"),
        Err(ImageError::Diagram {
            name: "mermaid diagram on line 1".to_owned(),
            reason: "Parse error on line 2:\n...A[open --> B\n-----^\n  Expecting SQE".to_owned(),
        }),
    );
    let inputs = Inputs {
        images,
        ..Inputs::new(Capabilities::plain(100))
    };
    let shown = render(ONE_BROKEN, &inputs);
    let lines: Vec<&str> = shown.lines().collect();
    let error = lines.iter().position(|line| line.contains('✗')).unwrap();
    let indent = lines[error].find('✗').unwrap();
    assert!(
        lines[error].ends_with("could not be drawn: Parse error on line 2:"),
        "{shown}"
    );
    assert_eq!(
        lines[error + 1],
        format!("{}...A[open --> B", " ".repeat(indent))
    );
    assert_eq!(lines[error + 2], format!("{}-----^", " ".repeat(indent)));
    // A line's own indent stays too.
    assert_eq!(
        lines[error + 3],
        format!("{}  Expecting SQE", " ".repeat(indent))
    );
}
