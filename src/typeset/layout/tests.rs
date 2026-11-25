use super::*;
use crate::typeset::driver::{Content, content, layout};
use crate::typeset::images::Placeholder;
use crate::typeset::locate::Map;
use crate::typeset::model::Document;
use crate::typeset::parse::parse;
use crate::typeset::style::text;
use crate::typeset::theme::BarColor;

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
    rows_of(&parse(source), inputs)
}

/// `rows` for a document made by hand.
fn rows_of(document: &Document, inputs: &Inputs) -> Vec<String> {
    let lines = layout(document, inputs);
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

/// The cell widths of the rows of a header above the file `name`, laid out
/// with `inputs`: the rows before the text of the document.
fn header_widths(name: &str, inputs: &Inputs) -> Vec<usize> {
    use crate::typeset::model::File;
    let shown = [File {
        name: name.into(),
        document: parse("Text\n"),
    }];
    let stream = crate::typeset::driver::files(&shown, inputs);
    (stream.lines.iter())
        .take_while(|line| !text(&line.spans).contains("Text"))
        .map(|line| crate::typeset::wrap::width(&line.spans))
        // The blank line below the header is not part of it.
        .filter(|&width| width > 0)
        .collect()
}

#[test]
fn a_file_header_with_a_band_is_as_wide_as_the_window_not_the_text() {
    let mut inputs = Inputs::new(Capabilities::kitty(60));
    inputs.theme.file_header.background = BarColor::Color(Some(Color::Rgb(0x30, 0x30, 0x30)));
    inputs.theme.max_width = 30;
    let widths = header_widths("dir/a.md", &inputs);
    assert!(widths.len() >= 3, "{widths:?}");
    assert!(widths.iter().all(|&width| width == 60), "{widths:?}");
    // A long name wraps at the window's width, not the text's.
    let long = format!("dir/{}", "n".repeat(50));
    let widths = header_widths(&long, &inputs);
    assert_eq!(widths.len(), 3, "{widths:?}");
    assert!(widths.iter().all(|&width| width == 60), "{widths:?}");
}

/// Two files `names` laid out with `inputs` as a stream, and the map of
/// it.
fn headed(names: &[&str], inputs: &Inputs) -> (Vec<Line>, Map) {
    use crate::typeset::model::File;
    let shown: Vec<File> = (names.iter())
        .map(|name| File {
            name: (*name).into(),
            document: parse("Text\n"),
        })
        .collect();
    let stream = crate::typeset::driver::files(&shown, inputs);
    let map = Map::new(&stream.lines, &stream.blocks);
    (stream.lines, map)
}

fn banded(width: u16) -> Inputs {
    let mut inputs = Inputs::new(Capabilities::kitty(width));
    inputs.theme.file_header.background = BarColor::Color(Some(Color::Rgb(0x30, 0x30, 0x30)));
    inputs
}

#[test]
fn the_layout_records_the_lines_and_the_sticky_row_of_each_header() {
    let (lines, map) = headed(&["dir/a.md", "b.md"], &banded(60));
    let (rows, header) = map.header(0).expect("the first header");
    assert_eq!(rows, 0..3);
    assert_eq!((header.rows, header.name), (3, 1));
    // The sticky row is the name row, made by the same code.
    assert_eq!(header.sticky.spans, lines[1].spans);
    let (second, _) = map.header(1).expect("the second header");
    let start = lines
        .iter()
        .position(|line| text(&line.spans).contains("b.md"));
    assert_eq!(second.start, start.unwrap() - 1);
    assert_eq!(second.len(), 3);
    assert!(map.header(2).is_none());
}

#[test]
fn a_wrapped_name_has_more_lines_and_one_sticky_row() {
    let long = format!("dir/{}", "n".repeat(50));
    let (lines, map) = headed(&[&long], &banded(30));
    let (rows, header) = map.header(0).expect("a header");
    assert!(rows.len() > 3, "{rows:?}");
    assert_eq!((rows.len(), header.name), (header.rows, 1));
    let sticky = text(&header.sticky.spans);
    assert_eq!(crate::typeset::wrap::width(&header.sticky.spans), 30);
    // Cut from the left, the end of the name stays.
    assert!(
        sticky.contains("…") && sticky.trim_end().ends_with("nnnn"),
        "{sticky:?}"
    );
    assert!(!lines[rows].is_empty());
}

#[test]
fn a_sticky_name_keeps_the_files_own_name_and_cuts_the_directory() {
    let (_, map) = headed(&["some/long/directory/file.md"], &banded(24));
    let (_, header) = map.header(0).unwrap();
    let sticky = text(&header.sticky.spans);
    assert_eq!(crate::typeset::wrap::width(&header.sticky.spans), 24);
    assert!(sticky.starts_with("▌ "), "{sticky:?}");
    assert!(sticky.starts_with("▌ …"), "{sticky:?}");
    assert!(
        sticky.trim_end().ends_with("ng/directory/file.md"),
        "{sticky:?}"
    );
    let file = (header.sticky.spans.iter()).find(|span| span.text == "file.md");
    assert!(
        file.is_some_and(|span| span.style.bold),
        "{:?}",
        header.sticky
    );
    // Too narrow for the file's name: its end after `…`.
    let (_, map) = headed(&["some/file-with-a-long-name.md"], &banded(12));
    let sticky = text(&map.header(0).unwrap().1.sticky.spans);
    assert!(
        sticky.contains('…') && sticky.trim_end().ends_with("name.md"),
        "{sticky:?}"
    );
}

#[test]
fn an_unstyled_header_is_one_line_of_arrows_and_its_sticky_row_is_the_same() {
    let inputs = Inputs::new(Capabilities::plain(60));
    let (lines, map) = headed(&["dir/a.md", "b.md"], &inputs);
    let (rows, header) = map.header(0).expect("a header");
    assert_eq!((rows, header.name), (0..1, 0));
    assert_eq!(text(&lines[0].spans), "==> dir/a.md <==");
    assert_eq!(text(&header.sticky.spans), "==> dir/a.md <==");
}

#[test]
fn a_lone_document_has_no_header_record() {
    let stream = crate::typeset::driver::content(
        crate::typeset::driver::Content::Document(&parse("Text\n")),
        &banded(60),
    );
    let map = Map::new(&stream.lines, &stream.blocks);
    assert!(map.header(0).is_none());
}

#[test]
fn footnotes_are_drawn_after_the_text_as_a_rule_and_a_numbered_list_with_no_anchor_shown() {
    let lines = layout(&parse("A[^1] text.\n\n[^1]: The note.\n"), &inputs());
    let texts: Vec<String> = lines.iter().map(|line| text(&line.spans)).collect();
    let all = texts.join("\n");
    assert!(all.contains("[1] The note. \u{21a9}"), "{texts:?}");
    assert!(!all.contains("fn-1") && !all.contains("<a"), "{texts:?}");
    let note = texts.iter().position(|t| t.contains("The note.")).unwrap();
    let body = texts.iter().position(|t| t.contains("text.")).unwrap();
    assert!(note > body, "{texts:?}");
}

fn paragraph(text: &str) -> Block {
    Block::Paragraph(vec![Inline::Text(text.into())])
}

fn heading(level: u8, text: &str) -> Block {
    Block::Heading {
        level,
        content: vec![Inline::Text(text.into())],
        id: None,
    }
}

/// A document of the blocks in a centred wrapper.
fn centred(blocks: Vec<Block>) -> Document {
    Document::new(vec![Block::Centered(blocks)])
}

#[test]
fn the_lines_of_a_centred_paragraph_start_after_half_of_their_spare_room() {
    let words = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen";
    let alone = rows_of(&Document::new(vec![paragraph(words)]), &inputs());
    let wrapped = rows_of(&centred(vec![paragraph(words)]), &inputs());
    assert!(alone.len() > 1, "{alone:?}");
    assert_eq!(wrapped.len(), alone.len());
    for (alone, wrapped) in alone.iter().zip(&wrapped) {
        // 56 cells between the margins of 2.
        let line = alone.trim_start();
        let pad = 2 + (56 - line.chars().count()) / 2;
        assert_eq!(wrapped, &format!("{}{line}", " ".repeat(pad)));
    }
}

#[test]
fn the_lines_of_a_centred_paragraph_with_breaks_are_as_wide_as_their_words() {
    let source = "<p align=\"center\">\n  Line one <br>\n  Line two  \n</p>\n";
    let rows = rows_of(&parse(source), &inputs());
    // 56 cells between the margins of 2, and 8 cells of words in each line.
    let pad = " ".repeat(2 + (56 - 8) / 2);
    assert_eq!(rows, [format!("{pad}Line one"), format!("{pad}Line two")]);
}

#[test]
fn the_padding_of_a_centred_line_is_no_place_of_the_document() {
    let document = centred(vec![paragraph("Centred words")]);
    let stream = content(Content::Document(&document), &inputs());
    let map = Map::new(&stream.lines, &stream.blocks);
    let line = stream
        .lines
        .iter()
        .position(|line| text(&line.spans).contains("Centred"))
        .unwrap();
    let pad = text(&stream.lines[line].spans).find('C').unwrap();
    assert!(pad > 10, "{pad}");
    let lead = crate::typeset::wrap::width(&stream.lines[line].lead);
    for column in lead..lead + pad {
        assert_eq!(map.hit(&stream.lines, line, column), None, "{column}");
    }
    assert!(map.hit(&stream.lines, line, lead + pad).is_some());
    // What the line shows of the document is the words, from their first byte.
    let origins: Vec<_> = (stream.lines[line].spans.iter())
        .flat_map(|span| span.origins.iter())
        .collect();
    assert_eq!(origins.len(), 1, "{origins:?}");
    assert_eq!(origins[0].doc, 0..13);
}

#[test]
fn a_centred_heading_is_in_the_middle_of_the_room_sized_or_plain() {
    let document = centred(vec![heading(1, "Name")]);
    for headings in [Headings::Sized, Headings::Plain] {
        let inputs = Inputs {
            headings,
            ..inputs()
        };
        let lines = layout(&document, &inputs);
        let line = lines
            .iter()
            .find(|line| text(&line.spans).contains("Name"))
            .unwrap();
        let scale = usize::from(line.rows());
        assert_eq!(scale > 1, headings == Headings::Sized, "{headings:?}");
        let shown = crate::typeset::wrap::width(&line.spans);
        let name = 4 * scale;
        // The 58 cells after the heading's indent, as at the top.
        let (before, after) = (shown - name, 58 - shown);
        assert!(before > 10, "{headings:?}: {before}");
        // Whole cells of the heading's size: off by less than one.
        assert!(
            before.abs_diff(after) <= scale,
            "{headings:?}: {before} {after}"
        );
    }
}

#[test]
fn a_centred_wrapper_centres_a_picture_and_leaves_a_table_a_list_a_quote_and_code_where_they_are() {
    let key = || ImageKey::from("a.png");
    let picture = loaded(key(), pixels(40, 20));
    let image = Block::Paragraph(vec![Inline::Image(crate::typeset::model::Image {
        url: "a.png".into(),
        alt: vec![Inline::Text("a".into())],
        width: None,
        height: None,
    })]);
    // 4 cells in the 56 between the margins of 2.
    assert_eq!(
        rows_of(&centred(vec![image]), &picture),
        [format!("{}⟦4 cells⟧", " ".repeat(2 + 26))]
    );
    let source =
        "| a | b |\n|---|---|\n| 1 | 2 |\n\n- item\n\n> quoted\n\n```rust\nlet a = 1;\n```\n";
    let plain = parse(source);
    let wrapped = Document::new(vec![Block::Centered(plain.blocks.clone())]);
    assert_eq!(rows_of(&wrapped, &inputs()), rows_of(&plain, &inputs()));
}

#[test]
fn a_centred_wrapper_inside_a_quote_centres_in_the_room_the_quote_leaves() {
    let inner = Block::Centered(vec![paragraph("abcd")]);
    let quoted = rows_of(
        &Document::new(vec![Block::BlockQuote(vec![inner])]),
        &inputs(),
    );
    // The bar and its space take 2 of the 56; 4 letters in the 54 left.
    assert_eq!(quoted.len(), 1, "{quoted:?}");
    assert_eq!(quoted[0], format!("  │ {}abcd", " ".repeat((54 - 4) / 2)));
}

#[test]
fn a_centred_heading_drawn_as_an_image_is_in_the_middle_of_the_room() {
    use crate::typeset::images::{Source, heading_words};
    let content = [Inline::Text("Name".into())];
    let mut inputs = Inputs {
        headings: Headings::Images,
        ..Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        })
    };
    let look = with_layout(&inputs, 0, |layout, _, _| layout.looks.heading(1));
    let key = ImageKey::heading_of(1, heading_words(&content), (10, 20))
        .with_look(look)
        .in_document(0);
    // 10 cells wide and 2 rows high, one word.
    let image = ::image::RgbaImage::new(100, 40);
    let source = Source::Text {
        image,
        breaks: Vec::new(),
        ends: vec![10],
    };
    inputs.images.insert(key, Ok(source));
    let alone = rows_of(&Document::new(vec![heading(1, "Name")]), &inputs);
    assert!(alone[0].ends_with("⟦10 cells⟧"), "{alone:?}");
    // 10 cells in the 58 after the heading's indent of 2, as at the top.
    assert_eq!(
        rows_of(&centred(vec![heading(1, "Name")]), &inputs)[0],
        format!("{}⟦10 cells⟧", " ".repeat(2 + 24))
    );
}

#[test]
fn a_centred_heading_of_every_level_is_in_the_middle_of_the_room() {
    let document = centred(
        (1..=6)
            .map(|level| heading(level, "A centered Markdown heading"))
            .collect(),
    );
    for headings in [Headings::Sized, Headings::Plain] {
        let inputs = Inputs {
            headings,
            ..Inputs::new(Capabilities::kitty(100))
        };
        let lines = layout(&document, &inputs);
        let shown: Vec<_> = (lines.iter())
            .filter(|line| text(&line.spans).contains("centered"))
            .collect();
        assert_eq!(shown.len(), 6);
        for (index, line) in shown.into_iter().enumerate() {
            let level = index + 1;
            let first = (line.spans.iter())
                .position(|span| !span.text.trim().is_empty())
                .unwrap();
            let before = crate::typeset::wrap::width(&line.spans[..first]);
            // 96 cells between the margins of 2, at the 100 columns most.
            let after = 96 - before - crate::typeset::wrap::width(&line.spans[first..]);
            // Off the middle by less than one cell of the heading's size.
            let scale = usize::from(line.rows());
            assert!(
                before.abs_diff(after) < 2 * scale,
                "{headings:?} level {level} (scale {scale}): \
                 {before} cells before its text, {after} after"
            );
        }
    }
}

/// The cells of `line` that have a background, from the first to the end
/// of the last, and the blank cells before and after its text past its
/// lead.
fn heading_cells(line: &Line) -> ((usize, usize), usize, usize) {
    let (mut at, mut covered) = (0, (usize::MAX, 0));
    for span in line.lead.iter().chain(&line.spans).chain(&line.trail) {
        let cells = crate::typeset::wrap::width(std::slice::from_ref(span));
        if span.style.bg.is_some() && cells > 0 {
            covered = (covered.0.min(at), at + cells);
        }
        at += cells;
    }
    let row = text(&line.spans) + &text(&line.trail);
    let before = row.len() - row.trim_start().len();
    let after = row.len() - row.trim_end().len();
    (covered, before, after)
}

#[test]
fn a_heading_that_a_wrapper_centres_has_the_background_of_a_top_level_one() {
    let mut inputs = Inputs::new(Capabilities::ansi(60));
    for heading in &mut inputs.theme.headings {
        heading.background = Some(Color::Rgb(0x33, 0x55, 0x77));
        heading.indent = 2;
        heading.rule = true;
    }
    let row = |source: &str, words: &str| {
        let lines = layout(&parse(source), &inputs);
        let line = (lines.iter())
            .find(|line| text(&line.spans).contains(words))
            .unwrap_or_else(|| panic!("{words} in {source:?}"))
            .clone();
        heading_cells(&line)
    };
    let (top, ..) = row("# A plain title\n", "plain");
    for source in [
        "<h1 align=\"center\">A centred title</h1>\n",
        "<div align=\"center\">\n\n# A centred title\n\n</div>\n",
    ] {
        let (covered, before, after) = row(source, "centred");
        assert_eq!(covered, top, "{source:?}");
        assert!(
            before.abs_diff(after) <= 1,
            "{source:?}: {before} and {after}"
        );
    }
}

#[test]
fn an_anchor_of_html_takes_no_room_in_a_centred_line() {
    let inputs = Inputs::new(Capabilities::ansi(60));
    let rows = |source: &str| rows_of(&parse(source), &inputs);
    assert_eq!(
        rows("<p align=\"center\"><a name=\"x\"></a>A title</p>\n"),
        rows("<p align=\"center\">A title</p>\n")
    );
}
