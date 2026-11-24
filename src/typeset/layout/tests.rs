use super::*;
use crate::typeset::driver::layout;
use crate::typeset::images::Placeholder;
use crate::typeset::parse::parse;
use crate::typeset::style::text;

/// What `lay_out` returns for a layout with `inputs` of the document
/// numbered `number`, given the width of the text and its margin. The
/// keys of its images carry the same number.
pub(super) fn with_layout<T>(
    inputs: &Inputs,
    number: usize,
    lay_out: impl FnOnce(&Layout, usize, usize) -> T,
) -> T {
    with_layout_in(inputs, number, number, &mut Blocks::default(), lay_out)
}

#[test]
fn plain_headings_have_no_text_sizing() {
    let document = parse("# Kiri\n");
    let mut inputs = inputs();
    let sized = layout(&document, &inputs);
    assert!(sized.iter().any(|line| line.rows() > 1), "{sized:?}");
    inputs.headings = Headings::Plain;
    let lines = layout(&document, &inputs);
    assert!(lines.iter().all(|line| line.rows() == 1), "{lines:?}");
    let mut out = Vec::new();
    crate::present::render::render(&document, &inputs, &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(!out.contains("\x1b]66;"), "{out:?}");
}

#[test]
fn a_raw_html_block_wraps_to_the_width_and_keeps_its_text() {
    let source = format!("<div>\n  {}</div>\n", "word ".repeat(30));
    let lines = layout(&parse(&source), &inputs());
    let texts: Vec<String> = lines.iter().map(|line| text(&line.spans)).collect();
    assert!(
        texts.iter().all(|line| line.chars().count() <= 60),
        "{texts:?}"
    );
    assert_eq!(texts.concat(), source.replace('\n', ""), "{texts:?}");
}

fn inputs() -> Inputs {
    Inputs::new(Capabilities::kitty(60))
}

/// What each line of `source` laid out with `inputs` shows of an
/// image: its text when it shows none, and then the band drawn over
/// it, if any: how wide, how far in, and whether low or high in the
/// row.
fn shown(source: &str, inputs: &Inputs) -> Vec<String> {
    let lines = layout(&parse(source), inputs);
    (lines.iter())
        .map(|line| {
            let shown = match &line.image {
                Some(row) => {
                    let placement = &row.placement;
                    let size = placement.size;
                    let what = format!("{:?} {}", placement.shows, placement.key);
                    format!("{what} {}×{} row {}", size.columns, size.rows, row.row)
                }
                None => text(&line.spans).trim_end().to_owned(),
            };
            match line
                .loading
                .as_ref()
                .and_then(|loading| loading.band.as_ref())
            {
                Some(band) => {
                    let at = if band.low { "low" } else { "high" };
                    format!("{shown} [band {} at {} {at}]", band.columns, band.indent)
                }
                None => shown,
            }
        })
        .collect()
}

/// Inputs for kitty with cells of 10 × 20 pixels, a CSS pixel to a
/// pixel, and `key` loading with `placeholder` in its place.
fn loading(key: impl Into<ImageKey>, placeholder: Placeholder) -> Inputs {
    let mut inputs = Inputs::new(Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(60)
    });
    let key = inputs.drawn(key);
    inputs.images.wait(key, placeholder);
    inputs
}

/// The pixels of an image of `width` × `height`.
fn pixels(width: u32, height: u32) -> crate::typeset::images::Source {
    crate::typeset::images::Source::Raster(::image::RgbaImage::new(width, height))
}

#[test]
fn an_image_that_loads_shows_its_old_image_at_its_size_and_the_band_in_the_blank_line_below() {
    let inputs = loading("a.png", Placeholder::Dimmed(pixels(40, 30)));
    assert_eq!(
        shown("Above\n\n![a](a.png)\n\nBelow\n", &inputs),
        [
            "Above",
            "",
            "Placeholder a.png 4×2 row 0",
            "Placeholder a.png 4×2 row 1",
            " [band 4 at 2 high]",
            "Below",
        ]
    );
}

#[test]
fn a_terminal_that_places_no_band_gets_the_loading_data_and_no_band() {
    let source = "![a](a.png)\n\nBelow\n";
    let cells = Some((10, 20));
    let tmux = |caps: Capabilities| Capabilities {
        cell_pixels: cells,
        tmux_passthrough: true,
        ..caps
    };
    let sixel = Capabilities::sixel(60);
    let terminals = [
        ("kitty", Capabilities::kitty(60), true),
        ("kitty under tmux", tmux(Capabilities::kitty(60)), false),
        ("sixel", sixel, false),
    ];
    for (name, caps, placed) in terminals {
        let caps = Capabilities {
            cell_pixels: cells,
            ..caps
        };
        let mut inputs = Inputs::new(caps);
        let key = inputs.drawn("a.png");
        inputs.images.wait(key, Placeholder::Dimmed(pixels(40, 30)));
        let lines = layout(&parse(source), &inputs);
        let loading: Vec<_> = lines
            .iter()
            .filter_map(|line| line.loading.as_ref())
            .collect();
        // The order the images load in needs the key and the lines above
        // whether or not a band is drawn.
        assert_eq!(loading.len(), 1, "{name}");
        assert_eq!(loading[0].key, ImageKey::from("a.png"), "{name}");
        assert_eq!(loading[0].above, 2, "{name}");
        assert_eq!(loading[0].band.is_some(), placed, "{name}");
    }
}

#[test]
fn without_a_blank_line_below_the_band_goes_over_the_bottom_of_the_last_row() {
    let inputs = loading("a.png", Placeholder::Dimmed(pixels(40, 30)));
    // At the end, and in a tight list, after the bullet.
    assert_eq!(
        shown("Above\n\n![a](a.png)\n", &inputs),
        [
            "Above",
            "",
            "Placeholder a.png 4×2 row 0",
            "Placeholder a.png 4×2 row 1 [band 4 at 2 low]",
        ]
    );
    let lines = shown("- ![a](a.png)\n- b\n", &inputs);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[1].ends_with("row 1 [band 4 at 4 low]"), "{lines:?}");
}

#[test]
fn a_placeholder_takes_the_size_the_old_image_had_and_the_new_block_asks_for() {
    let inputs = loading("a.png", Placeholder::Dimmed(pixels(40, 30)));
    assert_eq!(
        shown("<img src=\"a.png\" width=\"80\">\n", &inputs),
        [
            "Placeholder a.png 8×3 row 0",
            "Placeholder a.png 8×3 row 1",
            "Placeholder a.png 8×3 row 2 [band 8 at 2 low]",
        ]
    );
}

#[test]
fn a_diagram_that_loads_without_a_size_stays_code_with_the_band_over_its_last_row() {
    let key = ImageKey::rendered("mermaid", "flowchart TD\n");
    let inputs = loading(key, Placeholder::Unsized);
    let lines = shown("```mermaid\nflowchart TD\n```\n", &inputs);
    assert!(
        lines.iter().any(|line| line.contains("flowchart TD")),
        "{lines:?}"
    );
    assert!(
        lines.last().unwrap().ends_with("[band 56 at 2 low]"),
        "{lines:?}"
    );
}

#[test]
fn an_image_that_loads_without_a_size_shows_its_alt_text_with_the_band_over_it() {
    let inputs = loading("a.png", Placeholder::Unsized);
    let lines = shown("![a picture](a.png)\n", &inputs);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("a picture"), "{lines:?}");
    assert!(lines[0].ends_with("[band 56 at 2 low]"), "{lines:?}");
}

/// The band in `lines`, where it is, which image it tells of and how
/// many lines above it show that.
fn band_of(lines: &[Line]) -> (usize, String, usize) {
    let (at, loading) = (lines.iter().enumerate())
        .find_map(|(at, line)| Some((at, line.loading.as_ref()?)))
        .expect("a band");
    (at, loading.key.to_string(), loading.above)
}

#[test]
fn a_band_tells_which_image_loads_and_the_lines_above_it_that_show_it() {
    let inputs = loading("a.png", Placeholder::Dimmed(pixels(40, 30)));
    // On the blank line below the two rows of the image.
    let below = layout(&parse("Above\n\n![a](a.png)\n\nBelow\n"), &inputs);
    assert_eq!(band_of(&below), (4, "a.png".to_owned(), 2));
    // Over the last of them.
    let over = layout(&parse("Above\n\n![a](a.png)\n"), &inputs);
    assert_eq!(band_of(&over), (3, "a.png".to_owned(), 1));
    // A diagram's code, from its first line.
    let key = ImageKey::rendered("mermaid", "a\nb\n");
    let inputs = loading(key.clone(), Placeholder::Unsized);
    let code = layout(&parse("```mermaid\na\nb\n```\n"), &inputs);
    assert_eq!(
        band_of(&code),
        (code.len() - 1, key.to_string(), code.len() - 1)
    );
}

#[test]
fn a_box_of_rows_takes_those_rows_for_an_image_and_a_diagram() {
    let inputs = loading("a.png", Placeholder::Rows(3));
    assert_eq!(
        shown("<img src=\"a.png\" width=\"80\">\n", &inputs),
        [
            "Placeholder a.png 1×3 row 0",
            "Placeholder a.png 1×3 row 1",
            "Placeholder a.png 1×3 row 2 [band 1 at 2 low]",
        ]
    );
    let code = "flowchart TD\n  A --> B\n  B --> C\n  C --> D\n";
    let inputs = loading(ImageKey::rendered("mermaid", code), Placeholder::Rows(3));
    let lines = shown(&format!("```mermaid\n{code}```\n"), &inputs);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[0].starts_with("Placeholder mermaid"), "{lines:?}");
}

#[test]
fn an_image_that_failed_and_loads_again_shows_its_old_image_as_a_reload_does() {
    let source = "![a](a.png)\n";
    let mut failed = Inputs::new(Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(60)
    });
    // `insert` drops a placeholder, so the wait comes after it.
    failed.images.insert("a.png", Err(ImageError::Remote));
    failed
        .images
        .wait("a.png".into(), Placeholder::Dimmed(pixels(40, 30)));
    let reloading = loading("a.png", Placeholder::Dimmed(pixels(40, 30)));
    let lines = shown(source, &failed);
    assert_eq!(lines, shown(source, &reloading));
    assert!(lines[0].starts_with("Placeholder a.png 4×2"), "{lines:?}");
}

#[test]
fn nothing_tells_of_a_load_where_the_terminal_draws_no_images() {
    let mut inputs = Inputs::new(Capabilities::ansi(60));
    inputs
        .images
        .wait("a.png".into(), Placeholder::Dimmed(pixels(40, 30)));
    let lines = shown("![a picture](a.png)\n", &inputs);
    assert_eq!(lines.len(), 1, "{lines:?}");
}

/// Inputs for kitty with cells of 10 × 20 pixels, a CSS pixel to a
/// pixel, and `key` loaded as `image`.
fn loaded(key: impl Into<ImageKey>, image: crate::typeset::images::Source) -> Inputs {
    let mut inputs = Inputs::new(Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(60)
    });
    let key = inputs.drawn(key);
    inputs.images.insert(key, Ok(image));
    inputs
}

/// Each line of `source` laid out with `inputs`, as its lead and
/// spans show, with a row of an image as how many cells it covers.
fn rows(source: &str, inputs: &Inputs) -> Vec<String> {
    let lines = layout(&parse(source), inputs);
    (lines.iter())
        .map(|line| {
            let before = format!("{}{}", text(&line.lead), text(&line.spans));
            match &line.image {
                Some(row) => format!("{before}⟦{} cells⟧", row.placement.size.columns),
                None => before.trim_end().to_owned(),
            }
        })
        .collect()
}

#[test]
fn a_formula_is_centred_and_a_diagram_is_not() {
    // 4 cells in the 56 between the margins of 2.
    let formula = loaded(ImageKey::rendered("math", "x\n"), pixels(40, 20));
    assert_eq!(
        rows("$$x$$\n", &formula),
        [format!("{}⟦4 cells⟧", " ".repeat(2 + 26))]
    );
    let diagram = loaded(ImageKey::rendered("mermaid", "x\n"), pixels(40, 20));
    assert_eq!(rows("```mermaid\nx\n```\n", &diagram), ["  ⟦4 cells⟧"]);
    let image = loaded("a.png", pixels(40, 20));
    assert_eq!(rows("![a](a.png)\n", &image), ["  ⟦4 cells⟧"]);
}

#[test]
fn a_formula_is_centred_in_the_room_of_its_list_item_quote_or_alert() {
    let formula = || loaded(ImageKey::rendered("math", "x\n"), pixels(40, 20));
    // 4 cells in the 54 after the bullet.
    assert_eq!(
        rows("- $$x$$\n", &formula()),
        [format!("  • {}⟦4 cells⟧", " ".repeat(25))]
    );
    let quoted = rows("> $$x$$\n", &formula());
    assert_eq!(quoted.len(), 1, "{quoted:?}");
    assert!(
        quoted[0].ends_with(&format!("{}⟦4 cells⟧", " ".repeat(25))),
        "{quoted:?}"
    );
    let alert = rows("> [!NOTE]\n> $$x$$\n", &formula());
    assert!(
        alert
            .last()
            .unwrap()
            .ends_with(&format!("{}⟦4 cells⟧", " ".repeat(25))),
        "{alert:?}"
    );
}

#[test]
fn a_formula_wider_than_the_room_fills_it() {
    let formula = loaded(ImageKey::rendered("math", "x\n"), pixels(1000, 20));
    assert_eq!(rows("$$x$$\n", &formula), ["  ⟦56 cells⟧"]);
}

#[test]
fn a_formula_that_loads_is_centred_with_its_band() {
    let key = ImageKey::rendered("math", "x\n");
    let inputs = loading(key, Placeholder::Dimmed(pixels(40, 20)));
    assert_eq!(
        shown("$$x$$\n\nBelow\n", &inputs),
        [
            "Placeholder math block 4×1 row 0",
            " [band 4 at 28 high]",
            "Below",
        ]
    );
}

/// The rule lines of `source` laid out with `inputs`, as the text they
/// show, indent and all.
fn rules(source: &str, inputs: &Inputs) -> Vec<String> {
    let lines = layout(&parse(source), inputs);
    (lines.iter())
        .map(|line| text(&line.lead) + &text(&line.spans))
        .filter(|line| line.contains('─'))
        .collect()
}

#[test]
fn a_heading_rule_and_a_thematic_break_are_as_wide_as_the_text_and_start_in_its_column() {
    // 60 cells with margins of 2: the text, and the rule with it, takes 56.
    let wanted = format!("  {}", "─".repeat(56));
    for source in ["# Kiri\n\ntext\n", "## Kiri\n\ntext\n"] {
        assert_eq!(
            rules(source, &inputs()),
            std::slice::from_ref(&wanted),
            "{source:?}"
        );
    }
    // So is a thematic break.
    assert_eq!(rules("---\n", &inputs()), [wanted]);
    // Without margins the text, and the rule, take the layout's width.
    let mut plain = inputs();
    plain.theme = plain.theme.without_margins();
    for source in ["# Kiri\n\ntext\n", "---\n"] {
        assert_eq!(rules(source, &plain), ["─".repeat(60)], "{source:?}");
    }
}
