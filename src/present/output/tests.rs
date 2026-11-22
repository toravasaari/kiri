use std::rc::Rc;

use super::*;

fn link(columns: Range<u16>) -> RowLink {
    RowLink {
        columns,
        url: Rc::from("https://l.example/"),
        link: (0, 0),
    }
}

/// Writes row `row` of `line` to `out` as a caller that has no use for
/// the two parts apart does: the sends, then the cells.
fn row_in(writer: &mut Writer<'_>, line: &Line, row: u8, out: &mut Vec<u8>) -> io::Result<()> {
    let mut cells = Vec::new();
    writer.row(line, row, out, &mut cells)?;
    out.extend_from_slice(&cells);
    Ok(())
}

#[test]
fn a_row_gives_the_data_of_its_image_apart_from_its_cells() {
    let (line, images) = embedded(0, 1, Style::default());
    let caps = Capabilities::kitty(40);
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
    let (mut sends, mut cells) = (Vec::new(), Vec::new());
    writer.row(&line, 0, &mut sends, &mut cells).unwrap();
    let (sends, cells) = (
        String::from_utf8(sends).unwrap(),
        String::from_utf8(cells).unwrap(),
    );
    assert!(
        sends.starts_with("\x1b_G") && sends.contains("a=T"),
        "{sends:?}"
    );
    assert!(!sends.contains(graphics::PLACEHOLDER), "{sends:?}");
    assert!(
        !cells.contains("\x1b_G"),
        "no image data in the cells: {cells:?}"
    );
    assert!(cells.contains(graphics::PLACEHOLDER), "{cells:?}");
    // Held now: the second row has no sends.
    let (mut sends, mut cells) = (Vec::new(), Vec::new());
    writer.row(&line, 0, &mut sends, &mut cells).unwrap();
    assert!(sends.is_empty(), "{sends:?}");
}

#[test]
fn printed_rows_have_each_rows_sends_right_before_its_cells() {
    let (line, images) = embedded(0, 1, Style::default());
    let caps = Capabilities::kitty(40);
    let mut out = Vec::new();
    write(&[line], &images, &caps, Page::default(), &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    let (data, text, cells) = (
        out.find("\x1b_G").unwrap(),
        out.find("ab").unwrap(),
        out.find(graphics::PLACEHOLDER).unwrap(),
    );
    assert!(data < text && text < cells, "{out:?}");
    assert_eq!(out.matches("a=T").count(), 1, "{out:?}");
}

#[test]
fn a_loading_band_is_sent_and_placed_among_the_cells() {
    let line = Line {
        loading: Some(Box::new(crate::typeset::style::Loading {
            key: "a.png".into(),
            above: 1,
            band: Some(crate::typeset::style::Band {
                columns: 4,
                indent: 0,
                low: false,
            }),
        })),
        ..Line::blank()
    };
    let caps = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(40)
    };
    let images = Images::new();
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
    let (mut sends, mut cells) = (Vec::new(), Vec::new());
    writer.row(&line, 0, &mut sends, &mut cells).unwrap();
    assert!(sends.is_empty(), "{:?}", String::from_utf8_lossy(&sends));
    let cells = String::from_utf8(cells).unwrap();
    assert!(cells.starts_with("\x1b_G"), "{cells:?}");
    assert!(cells.contains("\x1b7"), "{cells:?}");
    assert!(writer.sent_image(), "a band that was sent counts as sent");
}

fn erased(page: Page, count: usize) -> String {
    let images = Images::new();
    let caps = Capabilities::kitty(40);
    let mut encoder = Encoder::new();
    let writer = Writer::new(&images, &caps, page, &mut encoder);
    let mut out = Vec::new();
    writer.erase_chars(count, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn erase_chars_erases_in_the_page_background_and_leaves_the_default_style() {
    let page = Page {
        background: Some(Color::Rgb(1, 2, 3)),
        foreground: None,
    };
    assert_eq!(erased(page, 5), "\x1b[0;48;2;1;2;3m\x1b[5X\x1b[0m");
}

#[test]
fn erase_chars_without_a_page_colour_writes_only_the_erase() {
    assert_eq!(erased(Page::default(), 4), "\x1b[4X");
    assert_eq!(erased(Page::default(), 0), "");
}

fn erased_row(page: Page) -> String {
    let images = Images::new();
    let caps = Capabilities::kitty(40);
    let mut encoder = Encoder::new();
    let writer = Writer::new(&images, &caps, page, &mut encoder);
    let mut out = Vec::new();
    writer.erase_line(&mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn erase_line_erases_the_row_in_the_page_background() {
    let page = Page {
        background: Some(Color::Rgb(1, 2, 3)),
        foreground: None,
    };
    assert_eq!(erased_row(page), "\x1b[0;48;2;1;2;3m\x1b[2K\x1b[0m");
    assert_eq!(erased_row(Page::default()), "\x1b[2K");
}

#[test]
fn the_canvas_is_the_page_background_shown_and_the_cell_size() {
    let page = Page {
        background: Some(Color::Rgb(1, 2, 3)),
        foreground: None,
    };
    let shown = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(40)
    };
    let hidden = Capabilities {
        color: false,
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(40)
    };
    let images = Images::new();
    let mut encoder = Encoder::new();
    let writer = Writer::new(&images, &shown, page, &mut encoder);
    assert_eq!(
        writer.canvas(),
        Canvas {
            background: Some(Color::Rgb(1, 2, 3)),
            cell: Some((10, 20)),
        }
    );
    let mut encoder = Encoder::new();
    let writer = Writer::new(&images, &hidden, page, &mut encoder);
    assert_eq!(writer.canvas().background, None);
    assert_eq!(writer.canvas().cell, Some((10, 20)));
}

#[test]
fn a_row_that_sends_its_image_says_so_once() {
    let (line, images) = embedded(0, 1, Style::default());
    let caps = Capabilities::kitty(40);
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
    let mut out = Vec::new();
    row_in(&mut writer, &line, 0, &mut out).unwrap();
    assert!(writer.sent_image());
    assert!(!writer.sent_image());
    row_in(&mut writer, &line, 0, &mut out).unwrap();
    assert!(!writer.sent_image(), "the image is held: nothing is sent");
}

/// An image of `columns` × `rows` cells of 10 × 20 pixels, red above
/// and blue below, and its placement.
fn two_toned(columns: u16, rows: u16) -> (Images, Rc<crate::typeset::images::size::Placement>) {
    use crate::typeset::images::size::{Placement, Shows, Size};
    use crate::typeset::images::{ImageKey, Source};
    let (width, height) = (10 * u32::from(columns), 20 * u32::from(rows));
    let size = Size {
        width,
        height,
        columns,
        rows,
    };
    let pixels = image::RgbaImage::from_fn(width, height, |_, y| {
        image::Rgba(if y < height / 2 {
            [200, 0, 0, 255]
        } else {
            [0, 0, 200, 255]
        })
    });
    let mut images = Images::new();
    images.insert("s.png", Ok(Source::Raster(pixels)));
    let placement = Placement::block(ImageKey::from("s.png"), size, Shows::Image);
    (images, Rc::new(placement))
}

/// What a writer with `caps` writes for row `row` of `line`.
fn row_of(line: &Line, images: &Images, caps: &Capabilities, page: Page) -> Vec<u8> {
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(images, caps, page, &mut encoder);
    let mut out = Vec::new();
    row_in(&mut writer, line, 0, &mut out).unwrap();
    out
}

#[test]
fn a_block_image_row_on_sixel_is_blank_cells_and_a_slice_between_saved_cursors() {
    use crate::present::sixel::{Options, Prepared};
    let (images, placement) = two_toned(3, 2);
    let caps = Capabilities::sixel(40);
    for image_row in 0..2 {
        let line = Line::image(Rc::clone(&placement), image_row);
        let out = row_of(&line, &images, &caps, Page::default());
        // The slice is the image's own row, one cell high.
        let pixels = image::RgbaImage::from_fn(30, 40, |_, y| {
            image::Rgba(if y < 20 {
                [200, 0, 0, 255]
            } else {
                [0, 0, 200, 255]
            })
        });
        let prepared = Prepared::new(&pixels, Options::new());
        let slice = prepared.slice(0, 20 * u32::from(image_row), 30, 20);
        let mut expected = b"\x1b7   \x1b8".to_vec();
        expected.extend_from_slice(&slice);
        expected.extend_from_slice(b"\x1b8\x1b[3C");
        assert_eq!(
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&expected),
            "row {image_row}"
        );
    }
}

#[test]
fn printed_sixel_rows_are_written_one_row_up_from_the_bottom_of_the_screen() {
    // A sixel drawn on the last row scrolls the screen, and the saved
    // cursor then lies on the row below it: a newline and a step up
    // first leave the row clear of the bottom, whatever the cursor was.
    let (images, placement) = two_toned(3, 2);
    let caps = Capabilities::sixel(40);
    let lines = [
        Line::new(vec![Span::new("text", Style::default())]),
        Line::image(Rc::clone(&placement), 0),
        Line::image(Rc::clone(&placement), 1),
    ];
    let mut out = Vec::new();
    write(&lines, &images, &caps, Page::default(), &mut out).unwrap();
    let out = String::from_utf8_lossy(&out).replace('\x1b', "ESC ");
    assert!(out.starts_with("text\n"), "{out:?}");
    assert_eq!(out.matches("\nESC M").count(), 2, "{out:?}");
    assert!(out.contains("\nESC MESC 7"), "{out:?}");
    // Text rows are written as ever.
    let mut plain = Vec::new();
    write(&lines[..1], &images, &caps, Page::default(), &mut plain).unwrap();
    assert_eq!(plain, b"text\n");
}

/// kitty's capabilities in a window of 40 columns, for a terminal that
/// names itself `name`.
fn kitty_named(name: Option<&str>) -> Capabilities {
    let mut caps = Capabilities::kitty(40);
    if let Graphics::Kitty(kitty) = &mut caps.graphics {
        kitty.terminal = name.map(str::to_owned);
    }
    caps
}

/// The cells of row 1 of an image that name their position, in what is
/// written.
fn named_cells(out: &[u8]) -> usize {
    named_cells_of(out, 1)
}

/// The cells of row `row` of an image that name their position.
fn named_cells_of(out: &[u8], row: usize) -> usize {
    let whole = format!("{}{}", graphics::PLACEHOLDER, graphics::DIACRITICS[row]);
    String::from_utf8_lossy(out).matches(&whole).count()
}

#[test]
fn placeholders_are_short_where_the_terminal_takes_positions_from_the_left() {
    let (images, placement) = two_toned(3, 2);
    let line = Line::image(Rc::clone(&placement), 1);
    // kitty and Ghostty: the first cell names its position.
    for name in ["kitty(0.48.2)", "ghostty 1.1.3"] {
        let out = row_of(&line, &images, &kitty_named(Some(name)), Page::default());
        assert_eq!(
            named_cells(&out),
            1,
            "{name}: {:?}",
            String::from_utf8_lossy(&out)
        );
        assert_eq!(cells(&String::from_utf8_lossy(&out)), 3, "{name}");
    }
    // Another terminal, or one that names nothing: every cell does.
    for name in [None, Some("WezTerm 20240203-110809-5046fc22")] {
        let out = row_of(&line, &images, &kitty_named(name), Page::default());
        assert_eq!(named_cells(&out), 3, "{name:?}");
    }
}

#[test]
fn an_image_in_text_is_short_where_the_terminal_takes_positions_from_the_left() {
    let (line, images) = embedded(0, 1, Style::default());
    let caps = kitty_named(Some("ghostty 1.1.3"));
    let out = row_of(&line, &images, &caps, Page::default());
    let text = String::from_utf8_lossy(&out);
    // Its first cell names the position, the two after it do not.
    assert_eq!(named_cells_of(&out, 0), 1, "{text:?}");
    assert_eq!(cells(&text), 3, "{text:?}");
}

#[test]
fn printed_rows_have_their_placeholders_whole() {
    // Printed rows stay in the scrollback, which the terminal may wrap
    // anew; the cells at the start of a wrapped line then still name
    // their positions.
    let (images, placement) = two_toned(3, 2);
    let line = Line::image(Rc::clone(&placement), 1);
    let caps = kitty_named(Some("kitty(0.48.2)"));
    let mut out = Vec::new();
    write(&[line], &images, &caps, Page::default(), &mut out).unwrap();
    assert_eq!(named_cells(&out), 3, "{:?}", String::from_utf8_lossy(&out));
}

#[test]
fn a_row_to_be_drawn_over_has_its_placeholders_whole() {
    let (images, placement) = two_toned(3, 2);
    let line = Line::image(Rc::clone(&placement), 1);
    let caps = kitty_named(Some("kitty(0.48.2)"));
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
    writer.whole_placeholders(true);
    let mut out = Vec::new();
    row_in(&mut writer, &line, 0, &mut out).unwrap();
    assert_eq!(named_cells(&out), 3, "{:?}", String::from_utf8_lossy(&out));
}

#[test]
fn a_block_image_row_that_reaches_the_last_column_is_followed_by_nothing() {
    let (images, placement) = two_toned(3, 2);
    let line = Line::image(Rc::clone(&placement), 0);
    let page = Page {
        background: Some(Color::Rgb(1, 2, 3)),
        foreground: None,
    };
    // In a window of 3 columns the cells reach the last one: the step
    // past them stops there and a clear to the end of the row would
    // remove the last cell's pixels.
    let narrow = Capabilities::sixel(3);
    let out = String::from_utf8_lossy(&row_of(&line, &images, &narrow, page)).into_owned();
    let after = out.rsplit("\x1b\\").next().unwrap();
    assert_eq!(after, "\x1b8\x1b[0m", "{out:?}");
    // In a wider one the row is stepped over and cleared to its end.
    let wide = Capabilities::sixel(40);
    let out = String::from_utf8_lossy(&row_of(&line, &images, &wide, page)).into_owned();
    assert!(out.ends_with("\x1b8\x1b[3C\x1b[K\x1b[0m"), "{out:?}");
}

#[test]
fn a_sixel_reaches_the_last_column_of_its_area_not_only_of_the_window() {
    let (images, placement) = two_toned(3, 2);
    let line = Line::image(Rc::clone(&placement), 0);
    let page = Page {
        background: Some(Color::Rgb(1, 2, 3)),
        foreground: None,
    };
    let caps = Capabilities::sixel(40);
    let row_in_area = |edge| {
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, &caps, page, &mut encoder);
        writer.edge(Some(edge));
        let mut out = Vec::new();
        row_in(&mut writer, &line, 0, &mut out).unwrap();
        String::from_utf8_lossy(&out).into_owned()
    };
    // An area of 3 columns in a window of 40: the cells reach its edge,
    // so nothing is written after them, not even the step past them,
    // which would stop on the last cell and remove its pixels.
    let out = row_in_area(Edge {
        columns: 3,
        window: false,
    });
    let after = out.rsplit("\x1b\\").next().unwrap();
    assert_eq!(after, "\x1b8\x1b[0m", "{out:?}");
    // An area of 10: the row is stepped over and the 7 cells left in the
    // area are erased, not cleared to the window's edge.
    let out = row_in_area(Edge {
        columns: 10,
        window: false,
    });
    assert!(
        out.ends_with("\x1b8\x1b[3C\x1b[0m\x1b[0;48;2;1;2;3m\x1b[7X\x1b[0m"),
        "{out:?}"
    );
    // The same area reaching the window's edge ends with the clear.
    let out = row_in_area(Edge {
        columns: 10,
        window: true,
    });
    assert!(out.ends_with("\x1b8\x1b[3C\x1b[K\x1b[0m"), "{out:?}");
}

#[test]
fn the_cells_under_a_sixel_row_are_blank_in_the_background_of_the_block() {
    let (images, placement) = two_toned(2, 1);
    let mut line = Line::image(placement, 0);
    line.image.as_mut().unwrap().background = Some(Color::Rgb(9, 8, 7));
    let out = row_of(&line, &images, &Capabilities::sixel(40), Page::default());
    let out = String::from_utf8_lossy(&out).into_owned();
    assert!(
        out.starts_with("\x1b[0;48;2;9;8;7m\x1b7  \x1b8\x1bP"),
        "{out:?}"
    );
}

#[test]
fn the_links_of_a_page_row_are_on_the_blank_cells_under_its_sixel() {
    let (images, placement) = two_toned(4, 1);
    let mut line = Line::image(placement, 0);
    line.image.as_mut().unwrap().links = vec![link(1..3)];
    let caps = Capabilities {
        hyperlinks: true,
        ..Capabilities::sixel(40)
    };
    let out = row_of(&line, &images, &caps, Page::default());
    let out = String::from_utf8_lossy(&out).into_owned();
    let cells = " \x1b]8;;https://l.example/\x1b\\  \x1b]8;;\x1b\\ ";
    assert!(
        out.starts_with(&format!("\x1b7{cells}\x1b8\x1bP")),
        "{out:?}"
    );
    // Without hyperlinks the cells are plain.
    let plain = row_of(&line, &images, &Capabilities::sixel(40), Page::default());
    assert!(String::from_utf8_lossy(&plain).starts_with("\x1b7    \x1b8"));
}

#[test]
fn a_row_wider_than_the_terminal_takes_is_drawn_as_tiles() {
    let (images, placement) = two_toned(5, 1);
    let line = Line::image(placement, 0);
    let mut caps = Capabilities::sixel(40);
    caps.graphics = crate::typeset::caps::Graphics::Sixel(crate::typeset::caps::SixelGraphics {
        max_width: 20,
        ..crate::typeset::caps::SixelGraphics::unknown(None)
    });
    let out = row_of(&line, &images, &caps, Page::default());
    let out = String::from_utf8_lossy(&out).into_owned();
    // Cells of 10 pixels: tiles of 2, 2 and 1 cells at their own offsets.
    assert_eq!(out.matches("\x1bP").count(), 3, "{out:?}");
    let starts: Vec<_> = out.split("\x1b8").collect();
    assert!(starts[1].starts_with("\x1bP"), "{out:?}");
    assert!(starts[2].starts_with("\x1b[2C\x1bP"), "{out:?}");
    assert!(starts[3].starts_with("\x1b[4C\x1bP"), "{out:?}");
    assert!(out.ends_with("\x1b8\x1b[5C"), "{out:?}");
}

/// The sixel of row `row` of the 30 x 20 `rows` pixels of `embedded`.
fn embedded_slice(row: u32) -> Vec<u8> {
    use crate::present::sixel::{Options, Prepared};
    let pixels = image::RgbaImage::from_pixel(30, 40, image::Rgba([9, 9, 9, 255]));
    Prepared::new(&pixels, Options::new()).slice(0, 20 * row, 30, 20)
}

#[test]
fn an_image_in_text_is_blank_cells_and_a_slice_between_saved_cursors_on_sixel() {
    let (line, images) = embedded(0, 2, Style::default());
    let caps = Capabilities::sixel(40);
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
    for row in 0..2u8 {
        let mut out = Vec::new();
        row_in(&mut writer, &line, row, &mut out).unwrap();
        // The first row has the text around the image; the next ones
        // step over it.
        let (before, after) = match row {
            0 => (b"ab".to_vec(), b"cd".to_vec()),
            _ => (b"\x1b[2C".to_vec(), Vec::new()),
        };
        let mut expected = before;
        expected.extend_from_slice(b"\x1b7   \x1b8");
        expected.extend_from_slice(&embedded_slice(u32::from(row)));
        expected.extend_from_slice(b"\x1b8\x1b[3C");
        expected.extend_from_slice(&after);
        assert_eq!(
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&expected),
            "row {row}"
        );
        assert!(!writer.sent_image());
    }
}

#[test]
fn an_image_in_text_shows_its_rows_from_the_one_the_line_starts_at_on_sixel() {
    let (line, images) = embedded(1, 2, Style::default());
    let caps = Capabilities::sixel(40);
    let out = row_of(&line, &images, &caps, Page::default());
    let slice = embedded_slice(1);
    assert!(
        out.windows(slice.len()).any(|window| window == slice),
        "{:?}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn the_blank_cells_under_an_image_in_text_have_the_style_of_its_span() {
    let style = Style {
        bg: Some(Color::Rgb(9, 8, 7)),
        underline: crate::typeset::color::Underline::Single,
        ..Style::default()
    };
    let (line, images) = embedded(0, 1, style);
    let out = row_of(&line, &images, &Capabilities::sixel(40), Page::default());
    let out = String::from_utf8_lossy(&out).into_owned();
    // Not underlined, and in the span's background.
    assert!(
        out.contains("ab\x1b[0;48;2;9;8;7m\x1b7   \x1b8\x1bP"),
        "{out:?}"
    );
}

#[test]
fn an_image_in_text_that_reaches_the_last_column_is_followed_by_nothing() {
    let (line, images) = embedded(0, 1, Style::default());
    // `ab` and three cells fill a window of 5 columns: nothing after
    // the slice, which would remove its last cell.
    let out = row_of(&line, &images, &Capabilities::sixel(5), Page::default());
    let out = String::from_utf8_lossy(&out).into_owned();
    let after = out.rsplit("\x1b\\").next().unwrap();
    assert_eq!(after, "\x1b8", "{out:?}");
    // In a window of 6 columns the cursor steps over the cells.
    let out = row_of(&line, &images, &Capabilities::sixel(6), Page::default());
    let out = String::from_utf8_lossy(&out).into_owned();
    assert!(out.ends_with("\x1b8\x1b[3Ccd"), "{out:?}");
}

#[test]
fn an_image_row_below_the_first_that_reaches_the_last_column_is_followed_by_nothing() {
    let (line, images) = embedded(0, 2, Style::default());
    let row_1 = |columns: u16| {
        let caps = Capabilities::sixel(columns);
        let mut encoder = Encoder::new();
        let page = Page {
            background: Some(Color::Rgb(1, 2, 3)),
            foreground: None,
        };
        let mut writer = Writer::new(&images, &caps, page, &mut encoder);
        let mut out = Vec::new();
        row_in(&mut writer, &line, 1, &mut out).unwrap();
        String::from_utf8_lossy(&out).into_owned()
    };
    // Two cells of text and three of image fill 5 columns: nothing
    // after the slice, no step and no clear.
    let out = row_1(5);
    let after = out.rsplit("\x1b\\").next().unwrap();
    assert_eq!(after, "\x1b8\x1b[0m", "{out:?}");
    // In 6 columns the cursor steps over the cells and the text, and the
    // row is cleared.
    let out = row_1(6);
    assert!(out.ends_with("\x1b8\x1b[3C\x1b[2C\x1b[K\x1b[0m"), "{out:?}");
}

#[test]
fn a_printed_line_with_only_an_image_in_text_is_written_one_row_up() {
    let (line, images) = embedded(0, 1, Style::default());
    let caps = Capabilities::sixel(40);
    let mut out = Vec::new();
    write(&[line], &images, &caps, Page::default(), &mut out).unwrap();
    let out = String::from_utf8_lossy(&out).replace('\x1b', "ESC ");
    assert!(out.starts_with("\nESC M"), "{out:?}");
    assert_eq!(out.matches("\nESC M").count(), 1, "{out:?}");
}

#[test]
fn sixel_writes_nothing_to_delete_or_to_send_again() {
    let (images, placement) = two_toned(3, 1);
    let line = Line::image(placement, 0);
    let caps = Capabilities::sixel(40);
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
    let mut out = Vec::new();
    row_in(&mut writer, &line, 0, &mut out).unwrap();
    assert!(!writer.sent_image());
    // The image goes when no line places it, and what is lost is not
    // sent again: the next row is drawn from its cache all the same.
    writer.encoder.keep(&[], &caps);
    writer.lost();
    let mut deleted = Vec::new();
    writer.delete_unused(&mut deleted).unwrap();
    assert!(deleted.is_empty(), "{deleted:?}");
}

#[test]
fn the_colour_under_a_sixel_is_the_one_the_cells_show() {
    let (images, _) = two_toned(1, 1);
    let shown = |caps: &Capabilities, page: Option<Color>, under: Option<Color>| {
        let page = Page {
            background: page,
            foreground: None,
        };
        let mut encoder = Encoder::new();
        Writer::new(&images, caps, page, &mut encoder).shown(under)
    };
    let page = Some(Color::Rgb(10, 20, 30));
    let approximated = Capabilities {
        truecolor: false,
        ..Capabilities::sixel(40)
    };
    // Written in 24 bits, the colour is exact; in 256 colours it is the
    // palette entry the terminal shows for it.
    assert_eq!(
        shown(&Capabilities::sixel(40), page, None),
        Some([10, 20, 30])
    );
    let entry = match Color::xterm(rgb_to_256(10, 20, 30)) {
        Color::Rgb(r, g, b) => [r, g, b],
        Color::Palette(_) => unreachable!(),
    };
    assert_ne!(entry, [10, 20, 30]);
    assert_eq!(shown(&approximated, page, None), Some(entry));
    // A block's own colour is above the page's.
    let tint = Some(Color::Rgb(90, 80, 70));
    assert_eq!(
        shown(&Capabilities::sixel(40), page, tint),
        Some([90, 80, 70])
    );
    // With no page colour the terminal's own shows, as it is.
    let terminal = Capabilities {
        background: Some(Color::Rgb(5, 6, 7)),
        ..approximated.clone()
    };
    assert_eq!(shown(&terminal, None, None), Some([5, 6, 7]));
    assert_eq!(shown(&approximated, None, None), None);
    // Without colours nothing is known.
    let mono = Capabilities {
        color: false,
        ..terminal
    };
    assert_eq!(shown(&mono, page, tint), None);
}

#[test]
fn links_are_clipped_to_the_written_columns_and_counted_from_the_first() {
    let runs = |links: &[RowLink], written: Range<u16>| -> Vec<Range<u16>> {
        clip(links, &written)
            .into_iter()
            .map(|run| run.columns)
            .collect()
    };
    let written = 4..10;
    // Before, across the start, inside, across the end and after.
    assert_eq!(
        runs(&[link(0..3)], written.clone()),
        Vec::<Range<u16>>::new()
    );
    assert_eq!(runs(&[link(2..6)], written.clone()), vec![0..2]);
    assert_eq!(runs(&[link(5..8)], written.clone()), vec![1..4]);
    assert_eq!(runs(&[link(8..14)], written.clone()), vec![4..6]);
    assert_eq!(
        runs(&[link(10..12)], written.clone()),
        Vec::<Range<u16>>::new()
    );
    // Touching the edges leaves nothing, and a link over all of them
    // is all of them.
    assert_eq!(
        runs(&[link(0..4)], written.clone()),
        Vec::<Range<u16>>::new()
    );
    assert_eq!(runs(&[link(0..20)], written), vec![0..6]);
    // Several, in order, each by itself.
    assert_eq!(
        runs(&[link(0..5), link(6..7), link(9..12)], 4..10),
        [0..1, 2..3, 5..6]
    );
}

use crate::typeset::size::{Fraction, VerticalAlign};

#[test]
fn empty_scaled_line_does_not_step_over_anything() {
    let scaled = Style {
        size: TextSize::new(2).unwrap(),
        ..Style::default()
    };
    let line = Line::new(vec![Span::new("", scaled)]);
    let mut out = Vec::new();
    write(
        &[line],
        &Images::new(),
        &Capabilities::kitty(20),
        Page {
            background: Some(Color::Rgb(0, 0, 0)),
            ..Page::default()
        },
        &mut out,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(!out.contains("\x1b[0C"), "{out:?}");
}

#[test]
fn a_line_is_as_tall_as_its_tallest_span() {
    let scaled = Style {
        size: TextSize::new(2).unwrap(),
        ..Style::default()
    };
    let line = Line {
        trail: vec![Span::new("|", Style::default())],
        ..Line::new(vec![
            Span::new("Big", scaled),
            Span::new(" small", Style::default()),
        ])
    };
    let caps = Capabilities::kitty(40);
    assert_eq!(rows(&line, &caps), 2);
    assert_eq!(rows(&line, &Capabilities::plain(40)), 1);
    let mut out = Vec::new();
    write(&[line], &Images::new(), &caps, Page::default(), &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    let rows: Vec<&str> = out.split_terminator('\n').collect();
    assert_eq!(
        rows,
        ["\x1b]66;s=2;Big\x1b\\ small|", "\x1b[12C|"],
        "{out:?}"
    );
}

#[test]
fn steps_over_text_at_a_fraction_as_it_is_packed() {
    let fraction = Fraction::new(7, 8).unwrap();
    let size = TextSize::new(2)
        .unwrap()
        .with_fraction(fraction, VerticalAlign::Top);
    let line = Line {
        trail: vec![Span::new("|", Style::default())],
        ..Line::new(vec![Span::new(
            "abcdefgh",
            Style {
                size,
                ..Style::default()
            },
        )])
    };
    let mut out = Vec::new();
    let caps = Capabilities::kitty(40);
    write(&[line], &Images::new(), &caps, Page::default(), &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    let rows: Vec<&str> = out.split_terminator('\n').collect();
    // Eight characters at 7/8 of scale 2 take 14 cells.
    assert_eq!(
        rows,
        ["\x1b]66;s=2:n=7:d=8:w=7;abcdefgh\x1b\\|", "\x1b[14C|"],
        "{out:?}"
    );
}

/// A line `ab`, an image three columns wide and `rows` rows high that
/// the line shows from image row `row`, and `cd`.
fn embedded(row: u16, rows: u16, style: Style) -> (Line, Images) {
    use crate::typeset::images::size::{Placement, Shows, Size};
    use crate::typeset::images::{ImageKey, Source};
    use crate::typeset::style::Embed;
    let size = Size {
        width: 30,
        height: 20 * u32::from(rows),
        columns: 3,
        rows,
    };
    let placement = Placement::block(ImageKey::from("e.png"), size, Shows::Image);
    let mut images = Images::new();
    let pixels =
        image::RgbaImage::from_pixel(30, 20 * u32::from(rows), image::Rgba([9, 9, 9, 255]));
    images.insert("e.png", Ok(Source::Raster(pixels)));
    let embed = Embed::new(Rc::new(placement), row, rows);
    let spans = vec![
        Span::new("ab", Style::default()),
        Span::embed("alt", style, embed),
        Span::new("cd", Style::default()),
    ];
    (Line::new(spans), images)
}

/// The rows the line is written as, without the image's transmission.
fn written(line: Line, images: &Images, page: Page) -> Vec<String> {
    let mut out = Vec::new();
    let caps = Capabilities::kitty(40);
    write(&[line], images, &caps, page, &mut out).unwrap();
    let mut out = String::from_utf8(out).unwrap();
    // Without the graphics commands that send the image.
    while let Some(start) = out.find("\x1b_G") {
        let end = out[start..]
            .find("\x1b\\")
            .map_or(out.len(), |end| start + end + 2);
        out.replace_range(start..end, "");
    }
    out.split_terminator('\n').map(str::to_owned).collect()
}

fn cells(row: &str) -> usize {
    row.matches(graphics::PLACEHOLDER).count()
}

#[test]
fn an_embed_is_the_placeholders_of_its_image_between_the_text_around_it() {
    let (line, images) = embedded(0, 1, Style::default());
    let rows = written(line, &images, Page::default());
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(cells(row), 3, "{row:?}");
    let first = row.find(graphics::PLACEHOLDER).expect("placeholders");
    let last = row.rfind(graphics::PLACEHOLDER).expect("placeholders");
    assert!(
        row[..first].ends_with("ab") || row[..first].contains("ab"),
        "{row:?}"
    );
    assert!(row[last..].contains("cd"), "{row:?}");
    assert!(!row.contains("alt"), "{row:?}");
}

#[test]
fn a_line_of_two_rows_writes_the_next_image_row_over_the_cells_of_each_embed() {
    let (line, images) = embedded(0, 2, Style::default());
    let rows = written(line, &images, Page::default());
    assert_eq!(rows.len(), 2);
    let diacritic = |row: usize| graphics::DIACRITICS[row];
    let first = format!("{}{}", graphics::PLACEHOLDER, diacritic(0));
    let second = format!("{}{}", graphics::PLACEHOLDER, diacritic(1));
    assert!(
        rows[0].contains(&first) && !rows[0].contains(&second),
        "{rows:?}"
    );
    assert_eq!(cells(&rows[1]), 3, "{rows:?}");
    assert!(
        rows[1].contains(&second) && !rows[1].contains(&first),
        "{rows:?}"
    );
    // The text before it is stepped over, not written.
    assert!(rows[1].starts_with("\x1b[2C"), "{rows:?}");
    assert!(!rows[1].contains("ab"), "{rows:?}");
}

#[test]
fn a_split_line_shows_the_image_row_its_embed_says() {
    let (line, images) = embedded(2, 1, Style::default());
    let rows = written(line, &images, Page::default());
    assert_eq!(rows.len(), 1);
    let third = format!("{}{}", graphics::PLACEHOLDER, graphics::DIACRITICS[2]);
    assert_eq!(rows[0].matches(&third).count(), 3, "{rows:?}");
}

#[test]
fn an_embed_in_a_link_is_inside_the_run_of_the_link() {
    let style = Style {
        link: Some(Rc::from("https://example.org/")),
        underline: Underline::Single,
        ..Style::default()
    };
    let (line, images) = embedded(0, 1, style);
    let rows = written(line, &images, Page::default());
    let row = &rows[0];
    let open = row
        .find("\x1b]8;;https://example.org/\x1b\\")
        .expect("link opens");
    let first = row.find(graphics::PLACEHOLDER).expect("placeholders");
    let last = row.rfind(graphics::PLACEHOLDER).expect("placeholders");
    let close = row[last..].find("\x1b]8;;\x1b\\").expect("link closes") + last;
    assert!(open < first && last < close, "{row:?}");
    // No underline behind the placeholders.
    let before = &row[open..first];
    assert!(!before.contains("4m") && !before.contains(";4"), "{row:?}");
}

#[test]
fn text_of_the_normal_size_with_a_background_is_blank_on_the_rows_below_the_first() {
    // A marker in an alert, beside a line of two rows.
    let (mut line, images) = embedded(0, 2, Style::default());
    let tint = Style {
        bg: Some(Color::Rgb(1, 2, 3)),
        ..Style::default()
    };
    line.spans[0] = Span::new("ab", tint);
    let rows = written(line, &images, Page::default());
    assert_eq!(rows.len(), 2);
    let blank = rows[1].find("  ").expect("two blanks");
    assert!(rows[1][..blank].contains("48;2;1;2;3"), "{rows:?}");
    assert!(
        rows[1].find(graphics::PLACEHOLDER).unwrap() > blank,
        "{rows:?}"
    );
}

#[test]
fn an_embed_has_the_background_of_its_span() {
    let style = Style {
        bg: Some(Color::Rgb(1, 2, 3)),
        ..Style::default()
    };
    let (line, images) = embedded(0, 1, style);
    let rows = written(line, &images, Page::default());
    let row = &rows[0];
    let first = row.find(graphics::PLACEHOLDER).expect("placeholders");
    assert!(row[..first].contains("48;2;1;2;3"), "{row:?}");
}

#[test]
fn a_band_is_placed_where_its_line_says_without_moving_the_cursor() {
    use crate::typeset::style::{Band, Loading};

    // High on a blank line, six cells in, and low over a line of text.
    let high = Line {
        loading: Some(Box::new(Loading {
            key: "a.png".into(),
            above: 1,
            band: Some(Band {
                columns: 4,
                indent: 6,
                low: false,
            }),
        })),
        ..Line::blank()
    };
    let low = Line {
        loading: Some(Box::new(Loading {
            key: "b.png".into(),
            above: 0,
            band: Some(Band {
                columns: 4,
                indent: 0,
                low: true,
            }),
        })),
        ..Line::new(vec![Span::new("code", Style::default())])
    };
    let caps = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(40)
    };
    let images = Images::new();
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
    let mut out = Vec::new();
    row_in(&mut writer, &high, 0, &mut out).unwrap();
    row_in(&mut writer, &low, 0, &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    let id = (out.split("\x1b_Ga=t,q=2,f=32,o=z,i=").nth(1))
        .and_then(|rest| rest.split(',').next())
        .unwrap();
    // A sixth of the 20-pixel cell down, and three pixels ending two
    // above its bottom.
    let place = |down: u16| format!("\x1b_Ga=p,q=2,i={id},C=1,Y={down},z=1\x1b\\");
    let high = format!("\x1b7\x1b[6C{}\x1b8", place(3));
    let low = format!("\x1b7{}\x1b8code", place(15));
    assert!(out.contains(&high), "{out:?}");
    assert!(out.contains(&low), "{out:?}");
}

#[test]
fn an_underline_colour_needs_styled_underlines_and_colours() {
    let style = Style {
        underline: Underline::Dashed,
        underline_color: Some(Color::Palette(8)),
        ..Style::default()
    };
    let mut caps = Capabilities::kitty(10);
    assert_eq!(sgr_codes(&style, &caps), ["4:5", "58;5;8"]);
    caps.styled_underlines = false;
    assert_eq!(sgr_codes(&style, &caps), ["4"]);
    caps = Capabilities::kitty(10);
    caps.color = false;
    assert_eq!(sgr_codes(&style, &caps), ["4:5"]);
}

#[test]
fn underline_styles_follow_their_own_capability() {
    let styled = Capabilities {
        styled_underlines: true,
        ..Capabilities::ansi(20)
    };
    assert_eq!(underline_code(Underline::Dotted, &styled), Some("4:4"));
    let single = Capabilities {
        styled_underlines: false,
        ..Capabilities::kitty(20)
    };
    assert_eq!(underline_code(Underline::Dotted, &single), Some("4"));
    assert_eq!(underline_code(Underline::None, &single), None);
}

#[test]
fn maps_palette_colours_to_short_codes() {
    assert_eq!(color_code(Color::Palette(4), 30, false), "34");
    assert_eq!(color_code(Color::Palette(8), 30, false), "90");
    assert_eq!(color_code(Color::Palette(200), 40, false), "48;5;200");
}

#[test]
fn approximates_rgb_without_truecolor() {
    assert_eq!(color_code(Color::Rgb(255, 0, 0), 30, true), "38;2;255;0;0");
    assert_eq!(color_code(Color::Rgb(255, 0, 0), 30, false), "38;5;196");
    assert_eq!(color_code(Color::Rgb(0, 0, 0), 30, false), "38;5;16");
    // Dark grey goes to the grey ramp, not to a dark teal in the cube.
    assert_eq!(color_code(Color::Rgb(43, 48, 59), 40, false), "48;5;236");
}
