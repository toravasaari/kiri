use super::*;
use crate::typeset::model::{AlertKind, List, ListItem};
use crate::typeset::parse::parse;
use crate::typeset::reading::text::Script;

/// The drawing the tests ask for, with a cache of its own that nothing
/// else shares: production draws through `draw_with` and the engine's.
impl Page {
    pub(crate) fn draw(&self, top: u32, width: u32, height: u32) -> RgbaImage {
        self.draw_with(&mut GlyphCache::default(), top, width, height, None)
    }

    pub(crate) fn draw_band(
        &self,
        band: usize,
        offset: f32,
        selected: bool,
        rows: Range<f32>,
    ) -> RgbaImage {
        let glyphs = &mut GlyphCache::default();
        self.draw_band_with(glyphs, band, offset, selected, rows, None)
    }
}

#[test]
fn a_container_lays_out_when_every_block_inside_it_does() {
    let paragraph = || Block::Paragraph(vec![Inline::Text("a".into())]);
    let wrapped = |inner: Block| {
        let item = |blocks| ListItem {
            checked: None,
            blocks,
        };
        [
            Block::BlockQuote(vec![inner.clone()]),
            Block::Alert {
                kind: AlertKind::Note,
                title: None,
                blocks: vec![inner.clone()],
            },
            Block::Details {
                summary: Vec::new(),
                blocks: vec![inner.clone()],
            },
            Block::Centered(vec![inner.clone()]),
            Block::List(List {
                start: None,
                tight: true,
                footnotes: false,
                items: vec![item(vec![paragraph()]), item(vec![inner])],
            }),
        ]
    };
    for left_out in [Block::Error("e".into()), Block::ImageFile("i.png".into())] {
        for container in wrapped(left_out) {
            assert!(!lays_out(&container), "{container:?}");
        }
    }
    for container in wrapped(paragraph()) {
        assert!(lays_out(&container), "{container:?}");
    }
}

const FRAME: Frame = Frame {
    width: 672.0,
    area: 1008.0,
    row: 20.0,
    wide: Wide::Wrap,
    snap: false,
    front_matter: FrontMatterShown::Table,
    budget: f32::INFINITY,
    margin: None,
};

const LOOK: Look = Look {
    spacing: crate::typeset::theme::GITHUB,
    marked: Marked {
        foreground: Some([0; 3]),
        background: Some([0xff, 0xff, 0]),
    },
    size: 16.0,
    text: [0xdd; 3],
    headings: [[0xee; 3]; 6],
    link: [0x40, 0x90, 0xf0],
    code: [0xcc; 3],
    chip: Some([0x30; 3]),
    quote: [0x90; 3],
    border: [0x3d, 0x44, 0x4d],
    marker: [0xaa; 3],
    code_block: Some([0x2b, 0x30, 0x3b]),
    code_text: [0xc0; 3],
    label: Some([0x80; 3]),
    table_border: [0x3d, 0x44, 0x4d],
    // The look the older tests were written for; the new ones say.
    table_style: TableStyle::Grid,
    stripe: Some([0x15, 0x1b, 0x23]),
    page: [0x0d, 0x11, 0x17],
    opaque: false,
    error: [0xf8, 0x51, 0x49],
    alerts: [
        AlertLook {
            color: [0x40, 0x90, 0xf0],
            bar: [0x40, 0x90, 0xf0],
            tint: Some([0x10, 0x20, 0x30]),
            chip: Some([0x20, 0x38, 0x50]),
        },
        AlertLook {
            color: [0x20, 0xc0, 0x40],
            bar: [0x20, 0xc0, 0x40],
            tint: Some([0x10, 0x30, 0x18]),
            chip: Some([0x20, 0x50, 0x30]),
        },
        AlertLook {
            color: [0xa0, 0x60, 0xf0],
            bar: [0xa0, 0x60, 0xf0],
            tint: None,
            chip: None,
        },
        AlertLook {
            color: [0xd0, 0xa0, 0x20],
            bar: [0xd0, 0xa0, 0x20],
            tint: None,
            chip: None,
        },
        AlertLook {
            color: [0xf0, 0x40, 0x40],
            bar: [0xf0, 0x40, 0x40],
            tint: None,
            chip: None,
        },
    ],
};

#[test]
fn marked_text_has_the_marks_colours_and_a_sub_or_sup_its_own_size_and_shift() {
    let runs_of = |inline: Inline| {
        let cursor = Shared::new(Cursor::off());
        let ink = Ink {
            pictures: &Plain,
            room: 600.0,
            cursor: &cursor,
        };
        runs(&[inline], &LOOK, ink)
    };
    let text = || vec![Inline::Text("x".into())];
    let [marked] = &runs_of(Inline::Mark(text()))[..] else {
        panic!("one run")
    };
    assert_eq!(marked.background, LOOK.marked.background);
    assert_eq!(marked.color, LOOK.marked.foreground);
    let script = |inline| runs_of(inline)[0].script;
    let spacing = LOOK.spacing;
    assert_eq!(
        script(Inline::Subscript(text())),
        Some(Script {
            size: spacing.script_size,
            drop: spacing.sub_drop
        })
    );
    assert_eq!(
        script(Inline::Superscript(text())),
        Some(Script {
            size: spacing.script_size,
            drop: -spacing.sup_raise
        })
    );
    assert_eq!(runs_of(Inline::Text("x".into()))[0].script, None);
}

fn page(markdown: &str) -> Page {
    let document = parse(markdown);
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    assert!(blocks.iter().all(|block| lays_out(block)), "{blocks:?}");
    lay_out(&blocks, &LOOK, &Fonts::fixture(), (&Plain, &Plain), FRAME)
}

/// The texts laid out on the page, each with its left edge, its first
/// baseline and its width.
fn texts(page: &Page) -> Vec<(String, f32, f32, f32)> {
    (page.items.iter())
        .filter_map(|item| match &item.shape {
            Shape::Text {
                paragraph, x, y, ..
            } => Some((
                paragraph.text().to_owned(),
                *x,
                y + paragraph.first_baseline(),
                paragraph.width(),
            )),
            _ => None,
        })
        .collect()
}

fn text<'a>(texts: &'a [(String, f32, f32, f32)], start: &str) -> &'a (String, f32, f32, f32) {
    (texts.iter())
        .find(|(text, ..)| text.starts_with(start))
        .unwrap_or_else(|| panic!("no text {start:?} in {texts:?}"))
}

#[test]
fn paragraphs_are_a_font_size_apart() {
    let page = page("One.\n\nTwo.\n");
    let [one, two] = &page.blocks[..] else {
        panic!("two blocks: {:?}", page.blocks);
    };
    assert_eq!(one.rows, 0.0..24.0);
    assert_eq!(two.rows, 40.0..64.0);
    assert_eq!(page.height, 64.0);
}

#[test]
fn margins_collapse_to_the_larger() {
    // A paragraph's 16 px below and a heading's 24 px above make 24.
    let page = page("One.\n\n### Three\n\nFour.\n");
    let [one, three, four] = &page.blocks[..] else {
        panic!("three blocks: {:?}", page.blocks);
    };
    assert_eq!(three.rows.start - one.rows.end, 24.0);
    assert_eq!(four.rows.start - three.rows.end, 16.0);
}

#[test]
fn the_first_block_has_no_margin_above() {
    let page = page("# Title\n");
    assert_eq!(page.blocks[0].rows.start, 0.0);
}

#[test]
fn large_headings_have_a_rule_below() {
    let page = page("## Two\n\n### Three\n");
    let rules: Vec<f32> = (page.items.iter())
        .filter_map(|item| match item.shape {
            Shape::Rect { y, height: 1.0, .. } => Some(y),
            _ => None,
        })
        .collect();
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert!(
        rules[0] > 30.0 && rules[0] < page.blocks[0].rows.end,
        "{rules:?}"
    );
}

#[test]
fn list_markers_sit_on_their_items_first_baseline_right_aligned() {
    let markdown = "8. eight\n9. nine\n10. ten and `code`\n";
    let texts = texts(&page(markdown));
    for (marker, item) in [("8.", "eight"), ("9.", "nine"), ("10.", "ten")] {
        let marker = text(&texts, marker);
        let item = text(&texts, item);
        assert_eq!(marker.2, item.2, "{marker:?} {item:?}");
        assert_eq!(marker.1 + marker.3, item.1 - 8.0, "{marker:?} {item:?}");
    }
    let (eight, ten) = (text(&texts, "8."), text(&texts, "10."));
    assert!(ten.1 < eight.1, "10. reaches further left than 8.");
}

#[test]
fn tight_list_items_are_half_a_line_apart() {
    let texts = texts(&page("- one\n- two\n"));
    let (one, two) = (text(&texts, "one"), text(&texts, "two"));
    assert_eq!(two.2 - one.2, 24.0 + 8.0);
    assert_eq!(one.1, 32.0);
}

#[test]
fn nested_lists_are_indented_with_their_own_bullets() {
    let texts = texts(&page("- one\n  - two\n"));
    assert_eq!(text(&texts, "two").1, 64.0);
    let markers: Vec<&str> = (texts.iter())
        .map(|(text, ..)| text.as_str())
        .filter(|text| ["•", "◦"].contains(text))
        .collect();
    assert_eq!(markers, ["◦", "•"]);
}

#[test]
fn a_quote_is_indented_with_a_bar_beside_it() {
    let page = page("> quoted\n> text\n");
    let texts = texts(&page);
    assert_eq!(text(&texts, "quoted").1, 16.0);
    let bar = (page.items.iter()).find_map(|item| match item.shape {
        Shape::Rect {
            x,
            y,
            width,
            height,
            ..
        } => Some((x, y, width, height)),
        _ => None,
    });
    assert_eq!(bar, Some((0.0, 0.0, 4.0, 24.0)));
}

/// The rectangles on the page: left edge, top, width, height, colour.
fn rects(page: &Page) -> Vec<(f32, f32, f32, f32, Brush)> {
    (page.items.iter())
        .filter_map(|item| match item.shape {
            Shape::Rect {
                x,
                y,
                width,
                height,
                color,
            } => Some((x, y, width, height, color)),
            _ => None,
        })
        .collect()
}

#[test]
fn an_alert_is_a_tinted_box_with_a_bar_an_icon_a_title_and_the_blocks() {
    let page = page("> [!NOTE]\n> A note.\n\nAfter.\n");
    let alert = LOOK.alert(AlertKind::Note);
    // 8 above, the title's 24 and 4, the text's 24 and 8 below.
    let height = 8.0 + 24.0 + 4.0 + 24.0 + 8.0;
    let tint = alert.tint.unwrap();
    assert_eq!(
        rects(&page),
        [
            (0.0, 0.0, 672.0, height, tint),
            (0.0, 0.0, 4.0, height, alert.bar),
        ]
    );
    // The icon is 16 pixels, after the bar and its padding and in the
    // middle of the title's first line.
    assert_eq!(pictures(&page), [(20.0, 8.0 + 4.0, 16, 16)]);
    let texts = texts(&page);
    assert_eq!(text(&texts, "Note").1, 20.0 + 16.0 + 8.0);
    assert_eq!(text(&texts, "A note.").1, 20.0);
    // The text below is a font size under it.
    assert_eq!(page.blocks[0].rows, 0.0..height);
    assert!(text(&texts, "After.").2 > height + 16.0);
}

#[test]
fn an_alert_without_a_tint_has_only_its_bar() {
    let page = page("> [!WARNING]\n> Careful.\n");
    let alert = LOOK.alert(AlertKind::Warning);
    let rects = rects(&page);
    assert_eq!(rects.len(), 1, "{rects:?}");
    assert_eq!(rects[0].4, alert.bar);
}

#[test]
fn an_alert_holds_any_blocks_in_its_own_width_and_a_title_of_its_own() {
    let page = page("> [!TIP] Custom title\n> - an item\n>\n>   more\n>\n> ```\n> code\n> ```\n");
    let texts = texts(&page);
    assert_eq!(text(&texts, "Custom title").1, 44.0);
    // The item's marker and text are inside the padding.
    assert!(text(&texts, "an item").1 >= 20.0 + 32.0);
    // The code block's background is as wide as the room inside.
    let code = (page.items.iter())
        .find_map(|item| match item.shape {
            Shape::RoundRect { x, width, .. } => Some((x, width)),
            _ => None,
        })
        .expect("a code block");
    assert_eq!(code, (20.0, 672.0 - 4.0 - 32.0));
}

/// Everything a page draws of tables, as lines of text: each rectangle
/// and each text with its place.
fn drawn(page: &Page) -> String {
    let mut out = String::new();
    for item in &page.items {
        match &item.shape {
            Shape::Rect {
                x,
                y,
                width,
                height,
                color,
            } => out += &format!("rect {x} {y} {width} {height} {color:?}\n"),
            Shape::Text {
                paragraph, x, y, ..
            } => {
                let text = paragraph.text();
                out += &format!("text {x} {y} {} {text:?}\n", paragraph.width());
            }
            _ => out += "other\n",
        }
    }
    out
}

#[test]
fn tables_are_laid_out_as_they_were_before_cells_could_hold_tables() {
    let page = page(
        "| Name | Kind | Count |\n|:-----|:----:|------:|\n| Fern | plant | 12 |\n| Crab | animal | 7 |\n| Moss | plant | 30 |\n\nAfter.\n\n| A | B |\n|---|---|\n| `inline code` and a [link](u) in a long cell that wraps at some point as it needs to | short |\n",
    );
    insta::assert_snapshot!(drawn(&page));
}

/// `markdown` laid out with its front matter shown as `shown`.
fn with_front_matter(markdown: &str, shown: FrontMatterShown) -> Page {
    let document = parse(markdown);
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    let frame = Frame {
        front_matter: shown,
        ..FRAME
    };
    lay_out(&blocks, &LOOK, &Fonts::fixture(), (&Plain, &Plain), frame)
}

#[test]
fn front_matter_is_a_table_of_its_keys_and_values_in_the_column() {
    let source = "---\ntitle: Hello\ntags: [a, b]\nempty:\n---\n\nText.\n";
    let page = with_front_matter(source, FrontMatterShown::Table);
    let texts = texts(&page);
    // The keys in a column of their own, the values in the next, a
    // list of words in one line.
    let key = text(&texts, "title");
    let value = text(&texts, "Hello");
    assert_eq!(key.1, 14.0);
    assert!(value.1 > key.1 + key.3, "{texts:?}");
    assert_eq!(value.2, key.2, "on one baseline");
    assert_eq!(text(&texts, "a, b").1, value.1);
    // The table's borders: a line over each of the three rows and one
    // under them, within the column.
    let lines: Vec<_> = rects(&page)
        .into_iter()
        .filter(|rect| rect.3 == 1.0)
        .collect();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines.iter().all(|line| line.0 == 0.0 && line.2 <= 672.0));
    // The text goes on below it.
    assert!(text(&texts, "Text.").2 > lines[3].1);
}

#[test]
fn the_zebra_of_the_front_matter_table_starts_from_its_second_row() {
    let source = "---\na: 1\nb: 2\nc: 3\nd: 4\n---\n";
    let page = with_front_matter(source, FrontMatterShown::Table);
    let stripe = LOOK.stripe.unwrap();
    let stripes: Vec<_> = rects(&page).into_iter().filter(|r| r.4 == stripe).collect();
    // Rows 2 and 4 of four, not the first.
    assert_eq!(stripes.len(), 2, "{stripes:?}");
    let first_row = text(&texts(&page), "a").2;
    assert!(stripes[0].1 > first_row, "{stripes:?}");
    // A Markdown table's header is no body row: its second body row is
    // the first shaded.
    let table = page_of_table();
    assert_eq!(rects(&table).iter().filter(|r| r.4 == stripe).count(), 1);
}

fn page_of_table() -> Page {
    page("| h |\n|---|\n| a |\n| b |\n")
}

#[test]
fn front_matter_has_a_blank_line_above_it() {
    let source = "---\na: 1\n---\n\nText.\n";
    let page = with_front_matter(source, FrontMatterShown::Table);
    // A line of the text's height, 24 pixels, before its top border.
    let top = rects(&page).iter().map(|r| r.1).fold(f32::MAX, f32::min);
    assert_eq!(top, 24.0);
    // Front matter as code has it too, and an empty one none.
    let code = with_front_matter(source, FrontMatterShown::Code);
    let block = (code.items.iter()).find_map(|item| match item.shape {
        Shape::RoundRect { y, .. } => Some(y),
        _ => None,
    });
    assert_eq!(block, Some(24.0));
    let empty = with_front_matter("---\n---\n\nText.\n", FrontMatterShown::Table);
    // Only the margin the empty block always had, 16 above the text.
    assert_eq!(text(&texts(&empty), "Text.").2, 33.0);
}

#[test]
fn the_tables_in_a_list_in_one_cell_have_a_gap_between_them() {
    let source = "---\nitems:\n  - a: 1\n  - b: 2\n---\n";
    let page = with_front_matter(source, FrontMatterShown::Table);
    let texts = texts(&page);
    // A table of one row is 36 pixels and its border: 37 from one to
    // the next, and a gap of a font size, 16, between them.
    let apart = text(&texts, "2").2 - text(&texts, "1").2;
    assert_eq!(apart, 37.0 + 16.0);
}

#[test]
fn the_tables_in_a_list_in_one_cell_have_the_same_column_widths() {
    let source = "---\nitems:\n  - k: short\n  - k: a much longer value\n---\n";
    let page = with_front_matter(source, FrontMatterShown::Table);
    // The one-row tables' vertical borders: 37 pixels tall, three for
    // each (the outer table's are taller).
    let mut borders: Vec<(f32, f32)> = rects(&page)
        .into_iter()
        .filter(|r| r.2 == 1.0 && r.3 == 37.0)
        .map(|r| (r.1, r.0))
        .collect();
    borders.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(borders.len(), 6, "{borders:?}");
    let (first, second) = (&borders[..3], &borders[3..]);
    let xs = |lines: &[(f32, f32)]| lines.iter().map(|b| b.1).collect::<Vec<_>>();
    assert_eq!(xs(first), xs(second));
}

#[test]
fn the_front_matter_table_is_as_wide_as_the_text_column() {
    let page = with_front_matter("---\na: 1\nb: 2\n---\n", FrontMatterShown::Table);
    let widest = rects(&page).iter().map(|r| r.2).fold(0.0, f32::max);
    assert_eq!(widest, 672.0);
    // The spare room goes to the columns in proportion to their content,
    // and the table's right border is the column's edge.
    let right = rects(&page)
        .iter()
        .filter(|r| r.2 == 1.0)
        .map(|r| r.0)
        .fold(0.0, f32::max);
    assert_eq!(right, 671.0);
    // A table too wide for the column is as it was, in the column.
    let long = "word ".repeat(200);
    let page = with_front_matter(&format!("---\na: {long}\n---\n"), FrontMatterShown::Table);
    assert!(rects(&page).iter().all(|r| r.0 + r.2 <= 672.5));
}

/// `markdown` laid out as `LOOK` but with tables in `style`, drawn as
/// far as its table reaches.
fn drawn_table(markdown: &str, style: TableStyle) -> RgbaImage {
    let document = parse(markdown);
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    let look = Look {
        table_style: style,
        ..LOOK
    };
    let page = lay_out(&blocks, &look, &Fonts::fixture(), (&Plain, &Plain), FRAME);
    // As wide as the widest rectangle or frame reaches.
    let reach = (rects(&page).iter().map(|r| r.0 + r.2))
        .chain(frames(&page).iter().map(|f| f.0 + f.2))
        .fold(0.0, f32::max);
    page.draw(0, reach.ceil() as u32 + 1, page.height.ceil() as u32)
}

/// The positions of the pixels that are the border's colour down the
/// column `at` of `image` when `vertical`, else along the row `at`.
fn border_at(image: &RgbaImage, vertical: bool, at: u32) -> Vec<u32> {
    let border = LOOK.table_border;
    let (width, height) = image.dimensions();
    let length = if vertical { height } else { width };
    (0..length)
        .filter(|&n| {
            let (x, y) = if vertical { (at, n) } else { (n, at) };
            let pixel = image.get_pixel(x, y).0;
            pixel[3] == 255 && pixel[..3] == border
        })
        .collect()
}

const TABLE: &str = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n";

#[test]
fn a_table_in_a_frame_has_rounded_corners_and_lines_under_its_header_and_between_columns() {
    let image = drawn_table(TABLE, TableStyle::Frame);
    let bottom = 3 * 36;
    let right = *border_at(&image, false, 54).last().expect("a right edge");
    // The corners are cut; the straight edges are the border.
    for (x, y) in [(0, 0), (right, 0), (0, bottom), (right, bottom)] {
        assert_eq!(image.get_pixel(x, y).0[3], 0, "corner {x},{y}");
    }
    // The shading of the last row stays inside the round corners.
    let stripe = LOOK.stripe.unwrap();
    for x in [0, right] {
        let pixel = image.get_pixel(x, bottom - 1).0;
        assert!(
            pixel[3] == 0 || pixel[..3] != stripe,
            "bottom corner {x}: {pixel:?}"
        );
    }
    assert_eq!(image.get_pixel(30, 0).0[..3], LOOK.table_border);
    assert_eq!(image.get_pixel(0, 40).0[..3], LOOK.table_border);
    // Down a column's middle: the top edge, the line under the header
    // and the bottom edge, and none between the body rows.
    let down = border_at(&image, true, 5);
    assert_eq!(down, [0, 36, bottom], "{down:?}");
    // Across a row: the left edge, the line between the columns and the
    // right edge.
    let across = border_at(&image, false, 54);
    assert_eq!(across.len(), 3, "{across:?}");
}

#[test]
fn a_table_in_a_grid_has_square_corners_and_a_line_round_every_cell() {
    let image = drawn_table(TABLE, TableStyle::Grid);
    let bottom = 3 * 36;
    let right = *border_at(&image, false, 54).last().expect("a right edge");
    for (x, y) in [(0, 0), (right, 0), (0, bottom), (right, bottom)] {
        assert_eq!(
            image.get_pixel(x, y).0[..3],
            LOOK.table_border,
            "corner {x},{y}"
        );
    }
    let down = border_at(&image, true, 5);
    assert_eq!(down, [0, 36, 72, bottom], "{down:?}");
    let across = border_at(&image, false, 54);
    assert_eq!(across.len(), 3, "{across:?}");
}

#[test]
fn a_table_in_a_shaded_row_paints_its_own_rows_by_its_own_zebra() {
    // S03a's shape: a list of maps under the second key, a shaded row.
    let source = "---\ntitle: T\ncolumns:\n  - aa: 1\n    bb: 2\n    cc: 3\n---\n";
    for style in [TableStyle::Frame, TableStyle::Grid] {
        let document = parse(source);
        let blocks: Vec<&Block> = document.blocks.iter().collect();
        let look = Look {
            table_style: style,
            ..LOOK
        };
        let page = lay_out(&blocks, &look, &Fonts::fixture(), (&Plain, &Plain), FRAME);
        let image = page.draw(0, 672, page.height.ceil() as u32);
        let texts = texts(&page);
        let at = |value: &str| nested_pixel(&image, &texts, value);
        let rgb = |brush: Brush| [brush[0], brush[1], brush[2], 255];
        // The first row is the page's colour, the second the stripe's,
        // the third the page's, whatever the outer row under it is.
        assert_eq!(at("1"), rgb(LOOK.page), "{style:?} first");
        assert_eq!(at("2"), rgb(LOOK.stripe.unwrap()), "{style:?} second");
        assert_eq!(at("3"), rgb(LOOK.page), "{style:?} third");
    }
}

/// The frames the page draws, as (left, top, width, height).
fn frames(page: &Page) -> Vec<(f32, f32, f32, f32)> {
    (page.items.iter())
        .filter_map(|item| match item.shape {
            Shape::Frame {
                x,
                y,
                width,
                height,
                ..
            } => Some((x, y, width, height)),
            _ => None,
        })
        .collect()
}

/// The pixel of a nested table's row for `value` (`1`, `2` or `3`, of
/// the keys `aa`, `bb`, `cc`): in the padding of the value's cell, 7
/// pixels left of its text, 10 above its baseline, clear of the text and
/// of the column's border about 13 pixels left of it.
fn nested_pixel(image: &RgbaImage, texts: &[(String, f32, f32, f32)], value: &str) -> [u8; 4] {
    let key = match value {
        "1" => "aa",
        "2" => "bb",
        "3" => "cc",
        _ => panic!("no row for {value:?}"),
    };
    let (_, _, baseline, _) = text(texts, key);
    let (_, value_x, ..) = text(texts, value);
    image
        .get_pixel(
            (*value_x - 7.0) as u32,
            (*baseline as u32).saturating_sub(10),
        )
        .0
}

#[test]
fn the_corners_of_a_nested_frame_show_the_row_it_is_in() {
    // The nested tables in a shaded outer row, one of three rows and
    // one of a single row: outside their rounded corners is the outer
    // stripe, not the page's colour.
    for source in [
        "---\ntitle: T\ncolumns:\n  - aa: 1\n    bb: 2\n    cc: 3\n---\n",
        "---\ntitle: T\ncolumns:\n  - aa: 1\n---\n",
    ] {
        let document = parse(source);
        let blocks: Vec<&Block> = document.blocks.iter().collect();
        let look = Look {
            table_style: TableStyle::Frame,
            ..LOOK
        };
        let page = lay_out(&blocks, &look, &Fonts::fixture(), (&Plain, &Plain), FRAME);
        let image = page.draw(0, 672, page.height.ceil() as u32);
        let nested = frames(&page)
            .into_iter()
            .find(|frame| frame.2 < 300.0)
            .expect("a nested frame");
        let (x, y, width, height) = (
            nested.0 as u32,
            nested.1 as u32,
            nested.2 as u32,
            nested.3 as u32,
        );
        let stripe = LOOK.stripe.unwrap();
        for (cx, cy, corner) in [
            (x, y, "top left"),
            (x + width - 1, y, "top right"),
            (x, y + height - 1, "bottom left"),
            (x + width - 1, y + height - 1, "bottom right"),
        ] {
            assert_eq!(
                image.get_pixel(cx, cy).0,
                [stripe[0], stripe[1], stripe[2], 255],
                "{corner} of {source:?}"
            );
        }
    }
}

#[test]
fn a_table_in_an_unshaded_row_paints_nothing_over_it() {
    // The first key's row is unshaded: the nested table's own rows are
    // transparent where its zebra says unshaded, and the stripe where it
    // says shaded.
    let source = "---\ncolumns:\n  - aa: 1\n    bb: 2\n---\n";
    let document = parse(source);
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    let page = lay_out(&blocks, &LOOK, &Fonts::fixture(), (&Plain, &Plain), FRAME);
    let image = page.draw(0, 672, page.height.ceil() as u32);
    let texts = texts(&page);
    let at = |value: &str| nested_pixel(&image, &texts, value);
    assert_eq!(at("1")[3], 0);
    let stripe = LOOK.stripe.unwrap();
    assert_eq!(at("2"), [stripe[0], stripe[1], stripe[2], 255]);
}

#[test]
fn a_value_with_keys_is_a_table_inside_its_cell() {
    let source = "---\nauthor:\n  name: Kim\n  mail: kim@example.com\n---\n";
    let page = with_front_matter(source, FrontMatterShown::Table);
    let texts = texts(&page);
    let author = text(&texts, "author");
    let name = text(&texts, "name");
    let kim = text(&texts, "Kim");
    // The nested table starts inside the cell of the outer one, past
    // its border and padding, and its rows are under each other.
    assert!(name.1 > author.1 + author.3 + 26.0, "{texts:?}");
    assert!(kim.1 > name.1 + name.3);
    assert!(text(&texts, "mail").2 > name.2);
    // Two tables' borders: the outer's two and the inner's three
    // lines across, and the same count of lines down.
    let across = rects(&page)
        .into_iter()
        .filter(|rect| rect.3 == 1.0)
        .count();
    assert_eq!(across, 2 + 3);
}

#[test]
fn front_matter_as_code_is_a_code_block_in_its_language() {
    let source = "---\ntitle: Hello\n---\n\nText.\n";
    let page = with_front_matter(source, FrontMatterShown::Code);
    let code = (page.items.iter()).any(|item| matches!(item.shape, Shape::RoundRect { .. }));
    assert!(code);
    assert!(rects(&page).iter().all(|rect| rect.3 != 1.0), "no table");
    // Front matter that is not a map is code too, and an empty one is
    // nothing.
    let list = with_front_matter("---\n- a\n- b\n---\n", FrontMatterShown::Table);
    assert!(
        list.items
            .iter()
            .any(|item| matches!(item.shape, Shape::RoundRect { .. }))
    );
    let empty = with_front_matter("---\n---\n\nText.\n", FrontMatterShown::Table);
    assert!(texts(&empty).iter().all(|text| text.0 == "Text."));
}

/// `blocks`, which the parser does not give, laid out.
fn laid_out(blocks: &[Block]) -> Page {
    let blocks: Vec<&Block> = blocks.iter().collect();
    lay_out(&blocks, &LOOK, &Fonts::fixture(), (&Plain, &Plain), FRAME)
}

/// The colours the texts on `page` are set in, one for each text.
fn text_colors(page: &Page) -> Vec<Brush> {
    (page.items.iter())
        .filter_map(|item| match &item.shape {
            Shape::Text { paragraph, .. } => Some(paragraph.look.color),
            _ => None,
        })
        .collect()
}

#[test]
fn a_note_is_muted_text_and_html_is_muted_source_in_the_code_font() {
    let page = laid_out(&[
        Block::Note("3 rows are left out.".into()),
        Block::Html("<div>\n  <b>x</b>\n</div>".into()),
    ]);
    let texts = texts(&page);
    assert_eq!(text(&texts, "3 rows").1, 0.0);
    assert_eq!(text(&texts, "<div>").1, 0.0);
    assert_eq!(text_colors(&page), [LOOK.quote, LOOK.quote]);
    // The source keeps its lines and indentation, in a font of its own
    // that is smaller than the text.
    let html = text(&texts, "<div>");
    assert!(html.0.contains("\n  <b>x</b>\n"), "{html:?}");
}

#[test]
fn details_is_its_summary_in_bold_after_a_triangle_and_the_blocks_below_it() {
    let page = page(
        "<details>\n<summary>More here</summary>\n\nInside.\n\n- an item\n\n</details>\n\nAfter.\n",
    );
    let texts = texts(&page);
    let summary = text(&texts, "▼ More here");
    let inside = text(&texts, "Inside.");
    assert_eq!((summary.1, inside.1), (0.0, 0.0));
    assert!(inside.2 > summary.2 + 16.0, "{texts:?}");
    assert!(text(&texts, "After.").2 > text(&texts, "an item").2);
    // The summary is bold, as it is in a browser.
    assert!(page.items.iter().any(|item| matches!(
        &item.shape,
        Shape::Text { paragraph, .. } if paragraph.text().starts_with("▼") && paragraph.look.weight >= 600.0
    )));
}

/// The urls of `links` and the number of pieces of each.
fn urls(links: &[LinkPlace]) -> Vec<(&str, usize)> {
    links.iter().map(|l| (&*l.url, l.pieces.len())).collect()
}

#[test]
fn a_link_is_where_its_text_is_a_piece_for_each_line() {
    let page = page(
        "Go [here for a long link text that wraps over lines](https://a.example) and \
             on to **[bold](https://b.example)**.\n",
    );
    let links = page.links();
    let urls = urls(links);
    assert_eq!(urls[1], ("https://b.example", 1), "{urls:?}");
    let (url, pieces) = urls[0];
    assert_eq!(url, "https://a.example");
    // The long one wraps in the 672 pixel column or does not; either
    // way its pieces are on the lines of the text, left to right.
    let first = links[0].pieces[0];
    assert!(first.x > 20.0 && first.width > 10.0, "{first:?}");
    assert_eq!((first.y, first.height), (0.0, 24.0));
    assert!(pieces >= 1);
    // The second is after the first on the same line.
    let second = links[1].pieces[0];
    assert!(second.x > first.x + first.width || second.y > first.y);
    assert!(links.iter().all(|link| link.band.is_none()));
}

#[test]
fn a_linked_picture_is_a_link_once_the_picture_has_come() {
    let markdown = "[![CI](ci.svg)](https://ci.example/) and [text](https://t.example/)\n";
    let given = Given {
        image: |_| solid(40, 16, [255, 0, 0], 7),
        ..NOTHING
    };
    let page_with = with_pictures(markdown, &given);
    let with = page_with.links();
    let page_without = with_pictures(markdown, &NOTHING);
    let without = page_without.links();
    let urls = |links: &[LinkPlace]| -> Vec<String> {
        links.iter().map(|link| link.url.to_string()).collect()
    };
    assert_eq!(urls(without), ["https://ci.example/", "https://t.example/"]);
    assert_eq!(urls(with), urls(without));
    // The picture's own box is the link's piece: 40 pixels wide, on
    // the line.
    let piece = with[0].pieces[0];
    assert_eq!(with[0].pieces.len(), 1);
    assert!((piece.width - 40.0).abs() < 0.5, "{piece:?}");
    assert!(with[1].pieces[0].x > piece.x + piece.width);
}

#[test]
fn a_paragraph_that_is_only_a_linked_picture_is_a_link_with_or_without_the_picture() {
    let markdown = "[![CI](ci.svg)](https://ci.example/)\n";
    let given = Given {
        image: |_| solid(40, 16, [255, 0, 0], 7),
        ..NOTHING
    };
    let page = with_pictures(markdown, &given);
    let [link] = page.links() else {
        panic!("one link: {:?}", page.links());
    };
    assert_eq!(&*link.url, "https://ci.example/");
    // The picture's own box.
    let piece = link.pieces[0];
    assert_eq!(
        (piece.x, piece.y, piece.width, piece.height),
        (0.0, 0.0, 40.0, 16.0)
    );
    // While it is missing the alt text is the link.
    let page = with_pictures(markdown, &NOTHING);
    let [link] = page.links() else {
        panic!("one link: {:?}", page.links());
    };
    assert_eq!(&*link.url, "https://ci.example/");
    assert!(link.pieces[0].width > 10.0);
    // An image alone with no link has none.
    assert!(with_pictures("![CI](ci.svg)\n", &given).links().is_empty());
    assert!(
        with_pictures("![CI](ci.svg)\n", &NOTHING)
            .links()
            .is_empty()
    );
}

#[test]
fn a_picture_inside_a_link_is_joined_with_the_text_of_the_same_link() {
    let given = Given {
        image: |_| solid(20, 10, [255, 0, 0], 7),
        ..NOTHING
    };
    let page = with_pictures(
        "[some ![i](i.png) more](https://l.example/) after\n",
        &given,
    );
    let links = page.links();
    assert_eq!(links.len(), 1, "{links:?}");
    // One piece, as the text, the picture and the text are side by side
    // on one line.
    assert_eq!(links[0].pieces.len(), 1, "{:?}", links[0].pieces);
    assert!(links[0].pieces[0].width > 20.0 + 40.0);
}

#[test]
fn two_links_side_by_side_are_two_links_and_one_link_in_two_styles_is_one() {
    let page = page("[a](u1)[b](u2) and [*c* d](u3)\n");
    assert_eq!(urls(page.links()), [("u1", 1), ("u2", 1), ("u3", 1)]);
}

#[test]
fn links_in_a_table_cell_a_list_and_a_band_are_in_the_page_or_the_band() {
    let page = page("- item [in a list](l)\n\n| a |\n|---|\n| [in a cell](c) |\n");
    let links = page.links();
    assert_eq!(urls(links), [("l", 1), ("c", 1)]);
    // The list's is indented by the marker's room; the cell's is
    // inside the table's border and below the list.
    assert!(links[0].pieces[0].x >= 32.0);
    assert!(links[1].pieces[0].y > links[0].pieces[0].y);
    let scrolled = scrolling(&format!("| a |\n|---|\n| [{}](w) |\n", "word".repeat(300)));
    let links = scrolled.links();
    assert_eq!(urls(links), [("w", 1)]);
    assert_eq!(links[0].band, Some(0));
    // In the band's own pixels, from its left edge.
    assert!(links[0].pieces[0].x >= 14.0 && links[0].pieces[0].x < 40.0);
}

/// A search for `term` as a plain text search finds it.
fn finds(term: &'static str) -> impl Fn(&str) -> Vec<Range<usize>> {
    move |text| {
        text.match_indices(term)
            .map(|(at, found)| at..at + found.len())
            .collect()
    }
}

#[test]
fn a_match_is_where_its_text_is_wherever_the_text_is() {
    let page = page("One word here.\n\n- a word in a list\n\n| word |\n|---|\n| other |\n");
    let hits = page.find(&finds("word"));
    assert_eq!(hits.len(), 3, "{hits:?}");
    // From the top down, each a piece the width of the word, on the
    // line of its text.
    assert!(hits[0].top < hits[1].top && hits[1].top < hits[2].top);
    assert_eq!(hits[0].pieces.len(), 1);
    let piece = hits[0].pieces[0];
    assert!(
        piece.x > 20.0 && piece.width > 20.0 && piece.width < 50.0,
        "{piece:?}"
    );
    assert_eq!((piece.y, piece.height), (0.0, 24.0));
    assert!(hits[1].pieces[0].x >= 32.0);
    assert!(hits.iter().all(|hit| hit.item.band.is_none()));
}

#[test]
fn a_match_over_two_lines_is_two_pieces_and_one_in_a_band_is_in_its_pixels() {
    let words = "word ".repeat(60);
    let page = page(&format!("{words}\n"));
    let hits = page.find(&finds("word word"));
    assert!(hits.iter().any(|hit| hit.pieces.len() == 2), "{hits:?}");
    let long = "a".repeat(200);
    let scrolled = scrolling(&format!("Text.\n\n```\n{long} needle\n```\n"));
    let hits = scrolled.find(&finds("needle"));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].item.band, Some(0));
    // Far to the right in the band's own pixels, and below its top.
    assert!(hits[0].pieces[0].x > 1000.0, "{:?}", hits[0].pieces);
    assert!(hits[0].top > 24.0);
}

/// The pixels of `image` in the box `piece` that are `colour`, within a
/// few steps of each channel, and not transparent.
fn colored(image: &RgbaImage, piece: Piece, colour: Brush) -> usize {
    let (x0, y0) = (piece.x.round() as u32, piece.y.round() as u32);
    let (x1, y1) = (
        (piece.x + piece.width).round() as u32,
        (piece.y + piece.height).round() as u32,
    );
    (x0..x1)
        .flat_map(|x| (y0..y1).map(move |y| (x, y)))
        .filter(|&(x, y)| {
            let pixel = image.get_pixel(x, y).0;
            pixel[3] > 200 && (0..3).all(|i| pixel[i].abs_diff(colour[i]) <= 3)
        })
        .count()
}

const MARKS: MarkColors = MarkColors {
    found: (Some([0x10, 0x10, 0x10]), Some([0xe0, 0xe0, 0x00])),
    current: (Some([0xff, 0xff, 0xff]), Some([0xd0, 0x40, 0x00])),
};

#[test]
fn marks_and_the_selected_link_are_drawn_over_the_page_in_a_page_or_a_band() {
    let page = page("A word, another word and [a link](u).\n");
    let hits = page.find(&finds("word"));
    let overlay = Overlay {
        colors: MARKS,
        marks: vec![
            (hits[0].item, hits[0].range.clone(), false),
            (hits[1].item, hits[1].range.clone(), true),
        ],
        link: Some(0),
        rows: Vec::new(),
        link_rows: None,
    };
    let plain = page.draw(0, 672, 24);
    let marked = page.draw_with(&mut GlyphCache::default(), 0, 672, 24, Some(&overlay));
    assert_ne!(plain, marked);
    assert!(colored(&marked, hits[0].pieces[0], [0xe0, 0xe0, 0x00]) > 100);
    assert!(colored(&marked, hits[1].pieces[0], [0xd0, 0x40, 0x00]) > 100);
    assert_eq!(colored(&plain, hits[0].pieces[0], [0xe0, 0xe0, 0x00]), 0);
    // The link has a box round it in its colour, above and below its
    // text, and none without the selection.
    let link = page.links()[0].pieces[0];
    let top = Piece {
        height: 1.0,
        ..link
    };
    assert!(colored(&marked, Piece { y: link.y, ..top }, LOOK.link) > 10);
    let unselected = Overlay {
        link: None,
        rows: Vec::new(),
        link_rows: None,
        ..overlay
    };
    let image = page.draw_with(&mut GlyphCache::default(), 0, 672, 24, Some(&unselected));
    assert_eq!(colored(&image, Piece { y: link.y, ..top }, LOOK.link), 0);
}

#[test]
fn a_mark_in_a_band_is_drawn_where_the_band_shows_it() {
    let long = "a".repeat(200);
    let page = scrolling(&format!("```\n{long} needle\n```\n"));
    let hits = page.find(&finds("needle"));
    let overlay = Overlay {
        colors: MARKS,
        marks: vec![(hits[0].item, hits[0].range.clone(), true)],
        link: None,
        rows: Vec::new(),
        link_rows: None,
    };
    let rows = 0.0..page.bands[0].height;
    let shown = |offset| {
        let glyphs = &mut GlyphCache::default();
        page.draw_band_with(glyphs, 0, offset, false, rows.clone(), Some(&overlay))
    };
    let count = |image: &RgbaImage| {
        image
            .pixels()
            .filter(|pixel| pixel.0[..3] == [0xd0, 0x40, 0x00])
            .count()
    };
    // Scrolled to the start the needle is out of sight; scrolled to its
    // place it is in the band, in the mark's colour.
    assert_eq!(count(&shown(0.0)), 0);
    let at = hits[0].pieces[0].x - 40.0;
    assert!(count(&shown(at)) > 100);
}

#[test]
fn task_items_get_a_box() {
    let page = page("- [x] done\n- [ ] to do\n");
    let tasks: Vec<bool> = (page.items.iter())
        .filter_map(|item| match item.shape {
            Shape::Task { checked, .. } => Some(checked),
            _ => None,
        })
        .collect();
    assert_eq!(tasks, [true, false]);
}

/// The left edge and width of each table's top border.
fn tables(page: &Page) -> Vec<(f32, f32)> {
    let mut tops: Vec<(f32, f32, f32)> = (page.items.iter())
        .filter_map(|item| match item.shape {
            Shape::Rect {
                x,
                y,
                width,
                height: 1.0,
                color,
                ..
            } if color == LOOK.table_border && width > 1.0 => Some((y, x, width)),
            _ => None,
        })
        .collect();
    tops.sort_by(|a, b| a.0.total_cmp(&b.0));
    tops.first()
        .map(|&(_, x, width)| (x, width))
        .into_iter()
        .collect()
}

#[test]
fn a_small_table_is_as_wide_as_its_content() {
    let page = page("| a | b |\n|---|---|\n| c | d |\n");
    let [(x, width)] = tables(&page)[..] else {
        panic!("one table");
    };
    assert_eq!(x, 0.0);
    assert!(width < 150.0, "{width}");
    let texts = texts(&page);
    let (a, b) = (text(&texts, "a"), text(&texts, "b"));
    assert_eq!(a.1, 1.0 + 13.0);
    assert!(b.1 > a.1 + a.3 + 26.0, "{a:?} {b:?}");
}

#[test]
fn a_wide_table_reaches_into_the_area_and_wraps_its_cells() {
    let words = "word ".repeat(60);
    let page = page(&format!("| a | b |\n|---|---|\n| {words} | {words} |\n"));
    let [(_, width)] = tables(&page)[..] else {
        panic!("one table");
    };
    assert!((width - 1008.0).abs() < 1.0, "{width}");
    let texts = texts(&page);
    assert!(text(&texts, "word").3 < 500.0);
}

#[test]
fn a_table_too_wide_even_wrapped_breaks_its_words_to_fit_the_area() {
    let long = "a".repeat(150);
    let page = page(&format!("| x | y |\n|---|---|\n| {long} | {long} |\n"));
    let [(_, width)] = tables(&page)[..] else {
        panic!("one table");
    };
    assert!(width <= 1008.0 + 0.5, "{width}");
    let texts = texts(&page);
    assert!(text(&texts, "aaa").3 < 500.0, "{texts:?}");
}

#[test]
fn every_second_body_row_is_shaded() {
    let page = page("| a |\n|---|\n| 1 |\n| 2 |\n| 3 |\n");
    let shaded = (page.items.iter())
        .filter(
            |item| matches!(item.shape, Shape::Rect { color, .. } if Some(color) == LOOK.stripe),
        )
        .count();
    assert_eq!(shaded, 1);
}

#[test]
fn a_column_is_aligned_as_the_table_says() {
    let page = page("| left | right |\n|:---|---:|\n| a | b |\n");
    let paragraphs: Vec<(String, f32)> = (page.items.iter())
        .filter_map(|item| match &item.shape {
            Shape::Text { paragraph, .. } => {
                let line = paragraph.pieces[0].layout.lines().next()?;
                Some((paragraph.text().to_owned(), line.metrics().offset))
            }
            _ => None,
        })
        .collect();
    let offset = |text: &str| paragraphs.iter().find(|(t, _)| t == text).unwrap().1;
    assert_eq!(offset("a"), 0.0);
    assert!(offset("b") > 0.0, "{paragraphs:?}");
}

/// A highlighter that colours every word of a line in one of two colours.
struct EveryWord;

impl Highlight for EveryWord {
    fn lines(&self, _: Option<&str>, code: &str) -> Vec<Vec<Run>> {
        let word = |at: usize, text: &str| Run {
            color: Some([[0x10; 3], [0x90; 3]][at % 2]),
            ..Run::new(text)
        };
        (code.lines())
            .map(|line| {
                let words = line.split_inclusive(' ').enumerate();
                words.map(|(at, text)| word(at, text)).collect()
            })
            .collect()
    }
}

#[test]
fn a_code_block_of_over_65535_style_runs_is_laid_out() {
    // 40 000 lines of two coloured words: 80 000 runs in the one block.
    let source = format!("```rust\n{}```\n", "let x\n".repeat(40_000));
    let document = parse(&source);
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    let page = lay_out(
        &blocks,
        &LOOK,
        &Fonts::fixture(),
        (&EveryWord, &Plain),
        FRAME,
    );
    let code = (page.items.iter())
        .find_map(|item| match &item.shape {
            Shape::Text { paragraph, .. } if paragraph.text().starts_with("let x\nlet") => {
                Some(paragraph)
            }
            _ => None,
        })
        .expect("the code of the block");
    assert!(code.pieces.len() > 1, "{} piece", code.pieces.len());
    assert_eq!(code.lines().len(), 40_000);
}

fn scrolling(markdown: &str) -> Page {
    let document = parse(markdown);
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    let frame = Frame {
        wide: Wide::Scroll,
        ..FRAME
    };
    lay_out(&blocks, &LOOK, &Fonts::fixture(), (&Plain, &Plain), frame)
}

#[test]
fn a_code_line_wider_than_the_area_scrolls_in_a_band_of_whole_rows() {
    let long = "a".repeat(200);
    let page = scrolling(&format!("Text.\n\n```\n{long}\n```\n\nMore.\n"));
    let [band] = &page.bands[..] else {
        panic!("one band");
    };
    assert_eq!(band.top % 20.0, 0.0);
    assert_eq!(band.height % 20.0, 0.0);
    assert!(band.top >= page.blocks[0].rows.end + 16.0, "{}", band.top);
    assert_eq!((band.x, band.visible), (0.0, 1008.0));
    assert!(band.width > 1008.0 + 500.0, "{}", band.width);
    assert!(band.most() > 500.0);
    // Its code stays one line, in the band and not on the page.
    assert!(code_blocks(&page).is_empty());
    let line = (band.items.iter()).find_map(|item| match &item.shape {
        Shape::Text { paragraph, .. } if paragraph.text() == long => Some(paragraph.lines().len()),
        _ => None,
    });
    assert_eq!(line, Some(1));
    assert!(page.blocks[2].rows.start >= band.top + band.height);
    // Wrapping, the same code has no band.
    assert!(page_wrapped(&long).bands.is_empty());
}

#[test]
fn a_band_in_an_alert_shows_the_alert_s_tint_on_an_opaque_page() {
    let long = "a".repeat(200);
    let document = parse(&format!(
        "> [!NOTE]\n> | a |\n> |---|\n> | b |\n> | {long} |\n"
    ));
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    let (look, frame) = (
        Look {
            opaque: true,
            ..LOOK
        },
        Frame {
            wide: Wide::Scroll,
            ..FRAME
        },
    );
    let page = lay_out(&blocks, &look, &Fonts::fixture(), (&Plain, &Plain), frame);
    let band = &page.bands[0];
    let image = page.draw_band(0, 0.0, false, 0.0..band.height);
    // Down a column in the middle of the band, clear of the text and
    // the fades: the tint of the alert, not the page, where the table
    // does not shade.
    let x = (band.x + band.visible / 2.0) as u32;
    let column: Vec<[u8; 4]> = (0..image.height())
        .map(|y| image.get_pixel(x, y).0)
        .collect();
    let (page_colour, tint) = ([0x0d, 0x11, 0x17, 255], [0x10, 0x20, 0x30, 255]);
    assert!(!column.contains(&page_colour), "{column:?}");
    assert!(column.contains(&tint), "{column:?}");
}

fn page_wrapped(long: &str) -> Page {
    page(&format!("```\n{long}\n```\n"))
}

#[test]
fn a_scrolling_table_keeps_its_words_whole() {
    let long = "a".repeat(150);
    let page = scrolling(&format!("| x | y |\n|---|---|\n| {long} | {long} |\n"));
    let [band] = &page.bands[..] else {
        panic!("one band");
    };
    let widest = (band.items.iter())
        .filter_map(|item| match &item.shape {
            Shape::Text { paragraph, .. } if paragraph.text() == long => {
                Some((paragraph.lines().len(), paragraph.width()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(widest.len(), 2);
    assert!(widest.iter().all(|&(lines, _)| lines == 1), "{widest:?}");
    assert!(band.width > 1008.0);
}

/// `markdown` laid out to scroll in an area `area` pixels wide.
fn scrolling_in(markdown: &str, area: f32) -> Page {
    scrolling_in_with(&Fonts::fixture(), markdown, area)
}

/// `markdown` laid out to scroll in an area `area` pixels wide, with
/// `fonts`, so that a test laying out many tables loads them once.
fn scrolling_in_with(fonts: &Fonts, markdown: &str, area: f32) -> Page {
    let document = parse(markdown);
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    let frame = Frame {
        wide: Wide::Scroll,
        area,
        ..FRAME
    };
    lay_out(&blocks, &LOOK, fonts, (&Plain, &Plain), frame)
}

#[test]
fn a_table_that_shares_the_room_exactly_is_no_band() {
    // Tables whose cells wrap to fill the room: the widths share the
    // room out, and their sum may come out a hair over it.
    for area in [700.0, 800.0, 864.0, 1008.0, 1100.0] {
        for words in 20..90 {
            let (left, right) = ("word ".repeat(words), "other text ".repeat(100 - words));
            let table = format!("| a | b | c |\n|---|---|---|\n| {left} | {right} | {left} |\n");
            let page = scrolling_in(&table, area);
            for band in &page.bands {
                // A band scrolls by more than float rounding.
                assert!(
                    band.most() > 2.0,
                    "{words} words in {area} px: {}",
                    band.most()
                );
            }
        }
    }
}

#[test]
fn a_code_line_that_reaches_the_edge_to_within_rounding_is_no_band() {
    let line = "a".repeat(120);
    let width = |area: f32| {
        let page = scrolling_in(&format!("```\n{line}\n```\n"), area);
        (page.bands.len(), page.items.len())
    };
    // The line and the block's padding, as wide as a very wide area
    // lets the block be.
    let natural = (scrolling_in(&format!("```\n{line}\n```\n"), 5000.0)
        .items
        .iter())
    .find_map(|item| match &item.shape {
        Shape::RoundRect { width, .. } => Some(*width),
        _ => None,
    })
    .expect("the block's background");
    assert_eq!(width(natural).0, 0);
    assert_eq!(width(natural - 0.25).0, 0);
    assert_eq!(width(natural - 4.0).0, 1);
}

#[test]
fn a_table_of_single_words_over_the_room_by_a_hair_is_no_band() {
    // Every cell is one word, so a column cannot be narrower than its
    // word, and the words may add up to a hair over the area.
    let words = [
        "alpha1",
        "bravo22",
        "charlie3",
        "deltaforce3",
        "echo4",
        "foxtrot55",
        "golf6",
        "hotel777",
    ];
    let fonts = Fonts::fixture();
    for columns in 6..=8 {
        let head = words[..columns].join(" | ");
        let body: Vec<String> = words[..columns]
            .iter()
            .rev()
            .map(|w| format!("{w}x"))
            .collect();
        let table = format!(
            "| {head} |\n|{}\n| {} |\n",
            "---|".repeat(columns),
            body.join(" | ")
        );
        for area in 672..1400 {
            let page = scrolling_in_with(&fonts, &table, area as f32);
            for band in &page.bands {
                // A band scrolls by more than what is rounding.
                assert!(
                    band.most() > FITS,
                    "{columns} columns in {area} px: {}",
                    band.most()
                );
            }
        }
    }
}

#[test]
fn a_table_of_single_words_over_the_room_by_a_hair_keeps_its_words_whole() {
    // The same tables: when they are no band, wrapping or scrolling,
    // no word is broken in the middle, so no cell has a second line.
    let words = [
        "alpha1",
        "bravo22",
        "charlie3",
        "deltaforce3",
        "echo4",
        "foxtrot55",
        "golf6",
        "hotel777",
    ];
    let mut cells = 0;
    let fonts = Fonts::fixture();
    for columns in 6..=8 {
        let head = words[..columns].join(" | ");
        let body: Vec<String> = words[..columns]
            .iter()
            .rev()
            .map(|w| format!("{w}x"))
            .collect();
        let table = format!(
            "| {head} |\n|{}\n| {} |\n",
            "---|".repeat(columns),
            body.join(" | ")
        );
        for area in 672..1400 {
            // Where scrolling needs no band the words fit, and so they
            // do when wrapping, which has the same room.
            if !scrolling_in_with(&fonts, &table, area as f32)
                .bands
                .is_empty()
            {
                continue;
            }
            for wide in [Wide::Scroll, Wide::Wrap] {
                let document = parse(&table);
                let blocks: Vec<&Block> = document.blocks.iter().collect();
                let frame = Frame {
                    wide,
                    area: area as f32,
                    ..FRAME
                };
                let page = lay_out(&blocks, &LOOK, &fonts, (&Plain, &Plain), frame);
                for item in &page.items {
                    if let Shape::Text { paragraph, .. } = &item.shape {
                        cells += 1;
                        assert_eq!(
                            paragraph.lines().len(),
                            1,
                            "{columns} columns in {area} px, {wide:?}: {:?}",
                            paragraph.text()
                        );
                    }
                }
            }
        }
    }
    assert!(cells > 1000, "{cells} cells looked at");
}

#[test]
fn a_table_that_fits_does_not_scroll() {
    let page = scrolling("| a | b |\n|---|---|\n| c | d |\n");
    assert!(page.bands.is_empty());
}

/// All the pixel rows of the first band of `page`.
fn whole(page: &Page) -> Range<f32> {
    0.0..page.bands[0].height
}

#[test]
fn a_code_band_keeps_its_label_in_the_top_right_corner_at_any_offset() {
    let page = scrolling(&format!("```rust\n{}\n```\n", "a".repeat(200)));
    let band = &page.bands[0];
    // Ink of the label, which is lighter than the code's background
    // and the padding above the code, in the corner of the band.
    let ink = |offset: f32| {
        let image = page.draw_band(0, offset, false, whole(&page));
        let right = (band.x + band.visible) as u32;
        (0..16)
            .flat_map(|y| (right - 60..right).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let pixel = image.get_pixel(x, y).0;
                pixel[3] > 128 && pixel[0] >= 0x50
            })
            .count()
    };
    for offset in [0.0, band.most() / 2.0, band.most()] {
        assert!(
            ink(offset) > 5,
            "{} pixels of label at {offset}",
            ink(offset)
        );
    }
}

#[test]
fn a_slice_of_a_band_is_the_same_pixels_as_those_rows_of_the_whole() {
    let code: String = (0..40)
        .map(|n| format!("{n} {}\n", "a".repeat(200)))
        .collect();
    let page = scrolling(&format!("```\n{code}```\n"));
    let band = &page.bands[0];
    let (offset, rows) = (50.0, whole(&page));
    let all = page.draw_band(0, offset, true, rows.clone());
    // Rows 600 to the end, with the code's end and the scroll bar,
    // against the same rows cut from the whole.
    let part = page.draw_band(0, offset, true, 600.0..rows.end);
    assert_eq!(part.height(), (rows.end - 600.0) as u32);
    let cut = image::imageops::crop_imm(&all, 0, 600, all.width(), part.height()).to_image();
    // Below the first row, where a glyph cut by the slice's edge may
    // antialias a level or two differently, the pixels are the same.
    let rest = |image: &RgbaImage| {
        image::imageops::crop_imm(image, 0, 1, image.width(), image.height() - 1).to_image()
    };
    assert_eq!(rest(&part), rest(&cut));
    let bar = (band.content + 4.0).round() as u32;
    assert!(bar > 600, "the scroll bar is in the last slice");
    // Inside the thumb, which starts 50 pixels in, scaled.
    let thumb = (50.0 / band.width * band.visible) as u32 + 5;
    assert_eq!(&part.get_pixel(thumb, bar - 600 + 1).0[..3], &LOOK.text);
}

/// The alpha of the band's pixel at (`x`, `y`).
fn alpha(image: &RgbaImage, x: u32, y: u32) -> u8 {
    image.get_pixel(x, y).0[3]
}

#[test]
fn a_band_fades_on_each_edge_with_more_beyond_it() {
    let page = scrolling(&format!("```\n{}\n```\n", "a".repeat(200)));
    let band = &page.bands[0];
    let start = page.draw_band(0, 0.0, false, whole(&page));
    let end = page.draw_band(0, band.most(), false, whole(&page));
    let middle = page.draw_band(0, band.most() / 2.0, false, whole(&page));
    // The code's background, opaque in the middle, nearly gone at an
    // edge that fades.
    let (left, right, inner) = (1, 1006, 500);
    assert_eq!(alpha(&start, inner, 10), 255);
    assert!(
        alpha(&start, right, 10) < 60,
        "{}",
        alpha(&start, right, 10)
    );
    assert_eq!(alpha(&start, left + 4, 10), 255);
    assert!(alpha(&end, left, 10) < 60);
    assert_eq!(alpha(&end, right - 4, 10), 255);
    assert!(alpha(&middle, left, 10) < 60 && alpha(&middle, right, 10) < 60);
}

#[test]
fn the_selected_band_s_thumb_is_brighter_and_thicker() {
    let page = scrolling(&format!("```\n{}\n```\n", "a".repeat(200)));
    let band = &page.bands[0];
    let bar = (band.content + 0.25 * 16.0).round() as u32;
    let (plain, selected) = (
        page.draw_band(0, 0.0, false, whole(&page)),
        page.draw_band(0, 0.0, true, whole(&page)),
    );
    let at = |image: &RgbaImage, y| image.get_pixel(20, y).0;
    assert_eq!(&at(&plain, bar + 1)[..3], &LOOK.quote);
    assert_eq!(&at(&selected, bar + 1)[..3], &LOOK.text);
    // Six pixels thick when selected, four otherwise.
    assert_eq!(at(&selected, bar + 5)[3], 255);
    assert_ne!(&at(&plain, bar + 5)[..3], &LOOK.quote);
    // The thumb is as long as the visible share: past it, the track.
    let thumb = (band.visible * band.visible / band.width) as u32;
    assert_ne!(&plain.get_pixel(thumb + 10, bar + 1).0[..3], &LOOK.quote);
}

#[test]
fn a_tick_shows_on_a_light_box() {
    let light = Look {
        link: [0xee; 3],
        ..LOOK
    };
    let document = parse("- [x] done\n");
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    let page = lay_out(&blocks, &light, &Fonts::fixture(), (&Plain, &Plain), FRAME);
    let image = page.draw(0, 40, 24);
    // The box is 13 pixels square, its right edge 8 pixels before the
    // text at 32: the tick is as dark as the box is light.
    let dark = (19..32)
        .flat_map(|x| (0..24).map(move |y| (x, y)))
        .filter(|&(x, y)| {
            let pixel = image.get_pixel(x, y).0;
            pixel[3] == 255 && pixel[0] < 0x40
        })
        .count();
    assert!(dark > 5, "{dark} dark pixels of the tick");
}

#[test]
fn every_block_of_a_document_is_laid_out() {
    let document = parse(
        "---\na: b\n---\n\n![x](x.png)\n\n```mermaid\ngraph\n```\n\n- ![y](y.png)\n\n  ```math\n  x\n  ```\n\n> quote\n\n> [!NOTE]\n> A note.\n\n<details>\n<summary>S</summary>\n\n> [!TIP]\n> Inside.\n\n</details>\n\n<div>x</div>\n\n| a |\n|---|\n| b |\n",
    );
    assert!(
        document.blocks.iter().all(lays_out),
        "{:?}",
        document.blocks
    );
    // Only what is no part of a document is left to the text layout.
    assert!(!lays_out(&Block::Error("unreadable".into())));
    assert!(!lays_out(&Block::ImageFile("a.png".into())));
    assert!(lays_out(&Block::Note("3 rows".into())));
}

/// The pictures a test gives: an image of `size` pixels in the colour
/// `colour`, or what else `picture` says.
struct Given {
    image: fn(&Image) -> Picture,
    diagram: fn(&str) -> Picture,
    formula: fn() -> Picture,
}

impl Pictures for Given {
    fn image(&self, image: &Image, _: (f32, f32)) -> Picture {
        (self.image)(image)
    }

    fn diagram(&self, language: &str, _: &str, _: (f32, f32)) -> Picture {
        (self.diagram)(language)
    }

    fn formula(&self, _: &str, _: (f32, f32)) -> Picture {
        (self.formula)()
    }
}

/// A picture of `width` × `height` pixels in `colour`.
fn solid(width: u32, height: u32, colour: [u8; 3], id: u64) -> Picture {
    let pixels = RgbaImage::from_pixel(
        width,
        height,
        image::Rgba([colour[0], colour[1], colour[2], 255]),
    );
    Picture::Ready {
        pixels: Rc::new(Pixels::new(pixels)),
        id,
        loading: None,
        depth: 0.0,
    }
}

fn with_pictures(markdown: &str, given: &Given) -> Page {
    page_with(&parse(markdown).blocks, given)
}

/// `blocks` laid out on a page, with the pictures `given`.
fn page_with(blocks: &[Block], given: &Given) -> Page {
    let blocks: Vec<&Block> = blocks.iter().collect();
    lay_out(&blocks, &LOOK, &Fonts::fixture(), (&Plain, given), FRAME)
}

const NOTHING: Given = Given {
    image: |_| Picture::None,
    diagram: |_| Picture::None,
    formula: || Picture::None,
};

/// The pictures on `page`: left edge, top and size.
fn pictures(page: &Page) -> Vec<(f32, f32, u32, u32)> {
    (page.items.iter())
        .filter_map(|item| match &item.shape {
            Shape::Picture { x, y, pixels } => Some((*x, *y, pixels.width(), pixels.height())),
            _ => None,
        })
        .collect()
}

#[test]
fn an_image_block_is_a_picture_at_its_size_at_the_left_of_the_column() {
    let given = Given {
        image: |_| solid(100, 50, [255, 0, 0], 7),
        ..NOTHING
    };
    let page = with_pictures("Before.\n\n![alt](x.png)\n\nAfter.\n", &given);
    assert_eq!(pictures(&page), [(0.0, 40.0, 100, 50)]);
    let [before, image, after] = &page.blocks[..] else {
        panic!("three blocks: {:?}", page.blocks);
    };
    // A paragraph's margin of 16 above and below, as for any block.
    assert_eq!(image.rows, 40.0..90.0);
    assert_eq!(after.rows.start - image.rows.end, 16.0);
    assert_eq!(before.rows.end, 24.0);
    assert_eq!(page.pictures, [7]);
}

#[test]
fn an_image_that_is_not_drawn_is_its_alt_text() {
    let page = with_pictures("![the alt text](x.png)\n", &NOTHING);
    assert!(pictures(&page).is_empty());
    let texts = texts(&page);
    assert_eq!(texts[0].0, "the alt text");
    // In the quote colour, as a muted note.
    let colour = (page.items.iter()).find_map(|item| match &item.shape {
        Shape::Text { paragraph, .. } => Some(paragraph.look.color),
        _ => None,
    });
    assert_eq!(colour, Some(LOOK.quote));
    assert!(page.pictures.is_empty());
}

#[test]
fn a_picture_still_loading_keeps_its_room_and_says_where_it_is() {
    let given = Given {
        image: |_| Picture::Loading {
            width: 10,
            height: 60,
            tag: 3,
        },
        ..NOTHING
    };
    let page = with_pictures("![alt](x.png)\n\nAfter.\n", &given);
    assert_eq!(page.blocks[0].rows, 0.0..60.0);
    assert_eq!(
        page.loading,
        [Loading {
            rows: 0.0..60.0,
            x: 0.0,
            width: 10.0,
            tag: 3
        }]
    );
    assert!(pictures(&page).is_empty());
}

#[test]
fn the_old_image_stands_in_while_a_new_one_loads() {
    let given = Given {
        image: |_| match solid(40, 30, [0, 0, 255], 9) {
            Picture::Ready { pixels, id, .. } => Picture::Ready {
                pixels,
                id,
                loading: Some(5),
                depth: 0.0,
            },
            other => other,
        },
        ..NOTHING
    };
    let page = with_pictures("![alt](x.png)\n", &given);
    assert_eq!(pictures(&page), [(0.0, 0.0, 40, 30)]);
    assert_eq!(page.loading.len(), 1);
    assert_eq!(page.loading[0].tag, 5);
    assert_eq!((page.loading[0].x, page.loading[0].width), (0.0, 40.0));
}

#[test]
fn a_formula_is_centred_in_the_column_and_a_diagram_is_not() {
    let given = Given {
        diagram: |_| solid(100, 40, [0, 255, 0], 1),
        ..NOTHING
    };
    let page = with_pictures("```math\nx\n```\n\n```mermaid\ngraph\n```\n", &given);
    let [(formula, ..), (diagram, ..)] = pictures(&page)[..] else {
        panic!("two pictures: {:?}", pictures(&page));
    };
    assert_eq!(formula, (672.0 - 100.0) / 2.0);
    assert_eq!(diagram, 0.0);
}

#[test]
fn a_diagram_that_failed_is_its_code_with_why_below_it_in_the_error_colour() {
    let given = Given {
        diagram: |_| Picture::Failed(Some("✗ Mermaid diagram could not be drawn: why".into())),
        ..NOTHING
    };
    let page = with_pictures("```mermaid\ngraph\n```\n", &given);
    assert_eq!(code_blocks(&page).len(), 1);
    let (_, _, code_height) = code_blocks(&page)[0];
    let message = (page.items.iter()).find_map(|item| match &item.shape {
        Shape::Text { paragraph, y, .. } if paragraph.text().starts_with('✗') => {
            Some((*y, paragraph.look.color))
        }
        _ => None,
    });
    let (y, colour) = message.expect("the reason");
    assert_eq!(colour, LOOK.error);
    assert!(y >= code_height, "{y} below code of {code_height}");
    // Without a reason to tell, the code alone.
    let quiet = Given {
        diagram: |_| Picture::Failed(None),
        ..NOTHING
    };
    let page = with_pictures("```mermaid\ngraph\n```\n", &quiet);
    assert_eq!(code_blocks(&page).len(), 1);
    assert!(texts(&page).iter().all(|(text, ..)| !text.starts_with('✗')));
}

/// The pictures standing in the lines of `page`'s paragraphs, by the
/// ids they carry.
fn inline_pictures(page: &Page) -> Vec<u64> {
    (page.items.iter())
        .filter_map(|item| match &item.shape {
            Shape::Text { paragraph, .. } => Some(paragraph.pictures.iter().map(|p| p.id)),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn images_in_a_paragraph_stand_in_its_lines() {
    let given = Given {
        image: |image| match image.url.as_str() {
            "a.png" => solid(20, 10, [255, 0, 0], 11),
            "b.png" => solid(30, 10, [0, 0, 255], 12),
            _ => Picture::None,
        },
        ..NOTHING
    };
    let page = with_pictures(
        "Badges ![a](a.png) and [![b](b.png)](https://example.com) and ![c](c.png)\n",
        &given,
    );
    // The two that are there are boxes; the one that is not is its alt
    // text, and none of them a picture block of its own.
    assert_eq!(inline_pictures(&page), [11, 12]);
    assert!(pictures(&page).is_empty());
    assert_eq!(page.pictures, [11, 12]);
}

#[test]
fn a_formula_in_a_paragraph_stands_in_its_line_or_is_its_source() {
    let given = Given {
        formula: || solid(30, 20, [0, 0, 255], 21),
        ..NOTHING
    };
    let markdown = "Let $x^2$ be, and cost $5 and $6.\n";
    let page = with_pictures(markdown, &given);
    assert_eq!(inline_pictures(&page), [21]);
    assert_eq!(page.pictures, [21]);
    // Without its picture it is the source, as written.
    let page = with_pictures(markdown, &NOTHING);
    assert!(inline_pictures(&page).is_empty());
}

#[test]
fn images_in_headings_and_table_cells_stand_in_their_text_too() {
    let given = Given {
        image: |_| solid(20, 10, [255, 0, 0], 11),
        ..NOTHING
    };
    let page = with_pictures(
        "# Title ![a](a.png)\n\n| h |\n|---|\n| cell ![b](b.png) |\n",
        &given,
    );
    assert_eq!(inline_pictures(&page), [11, 11]);
}

#[test]
fn a_picture_in_a_list_item_is_in_the_item_s_width() {
    let given = Given {
        diagram: |_| solid(100, 40, [0, 255, 0], 1),
        ..NOTHING
    };
    let page = with_pictures("- item\n\n  ```math\n  x\n  ```\n", &given);
    // The item starts 32 pixels in and is 640 wide: centred there.
    assert_eq!(pictures(&page)[0].0, 32.0 + (640.0 - 100.0) / 2.0);
}

/// The left edge, width and height of each code block's background.
fn code_blocks(page: &Page) -> Vec<(f32, f32, f32)> {
    (page.items.iter())
        .filter_map(|item| match item.shape {
            Shape::RoundRect {
                x, width, height, ..
            } => Some((x, width, height)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_code_block_is_at_least_as_wide_as_the_text() {
    let page = page("```\nx\n```\n");
    let [(x, width, height)] = code_blocks(&page)[..] else {
        panic!("one code block");
    };
    assert_eq!((x, width), (0.0, 672.0));
    assert_eq!(height, page.blocks[0].rows.end);
    assert!(height > 32.0 + 19.0, "{height}");
}

#[test]
fn a_long_code_line_reaches_into_the_area() {
    let page = page(&format!("```\n{}\n```\n", "a".repeat(100)));
    let [(_, width, _)] = code_blocks(&page)[..] else {
        panic!("one code block");
    };
    assert!(width > 672.0 && width < 1008.0, "{width}");
}

#[test]
fn a_code_line_longer_than_the_area_wraps_at_its_edge() {
    let long = "a".repeat(200);
    let page = page(&format!("```\n{long}\n```\n"));
    let [(_, width, _)] = code_blocks(&page)[..] else {
        panic!("one code block");
    };
    assert_eq!(width, 1008.0);
    let texts = texts(&page);
    let code = text(&texts, "aaa");
    assert!(code.3 <= 1008.0 - 32.0, "{code:?}");
    let lines = (page.items.iter()).find_map(|item| match &item.shape {
        Shape::Text { paragraph, .. } if paragraph.text() == long => Some(paragraph.lines().len()),
        _ => None,
    });
    assert_eq!(lines, Some(2));
}

#[test]
fn a_code_block_has_its_language_in_the_top_right_corner() {
    let page = page("```rust\nfn main() {}\n```\n");
    let texts = texts(&page);
    let label = text(&texts, "rust");
    assert_eq!(label.1 + label.3, 672.0 - 0.6 * 16.0);
    assert!(label.2 < 20.0, "{label:?}");
    // In the padding above the code, so that a long first line never
    // runs under it.
    let bottom = (page.items.iter()).find_map(|item| match &item.shape {
        Shape::Text { paragraph, y, .. } if paragraph.text() == "rust" => {
            Some(y + paragraph.height())
        }
        _ => None,
    });
    assert!(bottom.is_some_and(|bottom| bottom <= 16.0), "{bottom:?}");
}

#[test]
fn a_part_of_the_page_draws_what_reaches_into_it() {
    let page = page("---\n\nText.\n");
    let top = page.draw(0, 100, 4);
    assert_eq!(top.get_pixel(50, 0).0, [0x3d, 0x44, 0x4d, 255]);
    let below = page.draw(4, 100, 4);
    assert_eq!(below.get_pixel(50, 0).0, [0; 4]);
    assert_eq!((below.width(), below.height()), (100, 4));
}

// The front matter and table cases below pin how the page lays out each
// shape of grid.

#[test]
fn front_matter_with_a_value_that_has_keys_is_drawn_as_nested_tables() {
    let source = "---\nauthor:\n  name: Kim\n  links:\n    site: example.com\n    feed: example.com/feed\ntitle: Hello\n---\n";
    insta::assert_snapshot!(drawn(&with_front_matter(source, FrontMatterShown::Table)));
}

#[test]
fn front_matter_with_a_list_of_maps_is_drawn_as_a_stack() {
    let source = "---\nfeatures:\n  - icon: a\n    title: Fast\n  - icon: b\n    title: Modular\n  - More to come\n  - []\n---\n";
    insta::assert_snapshot!(drawn(&with_front_matter(source, FrontMatterShown::Table)));
}

#[test]
fn front_matter_nested_deeper_than_tables_go_is_drawn_as_code_lines() {
    let source = "---\nbuild:\n  targets:\n    linux:\n      flags:\n        opt: 3\n        lto: true\n      env:\n        - CI=1\n---\n";
    insta::assert_snapshot!(drawn(&with_front_matter(source, FrontMatterShown::Table)));
}

#[test]
fn front_matter_with_an_empty_map_and_an_empty_list_is_drawn_with_empty_cells() {
    let source = "---\nmap: {}\nlist: []\nafter: x\nmixed:\n  - {}\n  - []\n  - y\n---\n";
    insta::assert_snapshot!(drawn(&with_front_matter(source, FrontMatterShown::Table)));
}

#[test]
fn a_table_whose_words_must_break_is_drawn_with_the_words_broken() {
    let long = "a".repeat(150);
    let page = page(&format!("| x | y |\n|---|---|\n| {long} | {long} |\n"));
    insta::assert_snapshot!(drawn(&page));
}

/// The top of `markdown`'s last block, laid out with headings on rows
/// or not, in rows `row` pixels high.
fn last_top(markdown: &str, snap: bool, row: f32) -> f32 {
    let document = parse(markdown);
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    let frame = Frame { snap, row, ..FRAME };
    let page = lay_out(&blocks, &LOOK, &Fonts::fixture(), (&Plain, &Plain), frame);
    page.blocks.last().expect("a block").rows.start
}

#[test]
fn a_heading_moves_up_to_the_nearest_row_with_snap() {
    // A paragraph of 24 pixels and a heading's margin of 24 above it:
    // 8 pixels into the third row of 20, which leaves 16 above it.
    assert_eq!(last_top("One.\n\n## Two\n", false, 20.0), 48.0);
    assert_eq!(last_top("One.\n\n## Two\n", true, 20.0), 40.0);
}

#[test]
fn a_heading_moves_down_to_the_next_row_when_the_nearest_is_past_half() {
    // 48 is 10 pixels into a row of 19, past half of it.
    assert_eq!(last_top("One.\n\n## Two\n", true, 19.0), 57.0);
}

#[test]
fn a_heading_never_moves_closer_to_the_block_above_than_a_paragraph() {
    // 48 is 12 pixels into a row of 36, under half of it, but moving up
    // would leave 12 pixels above the heading, less than the 16
    // between two paragraphs, so it moves down instead.
    assert_eq!(last_top("One.\n\n## Two\n", true, 36.0), 72.0);
}

#[test]
fn headings_below_level_four_keep_their_place_with_snap() {
    assert_eq!(last_top("One.\n\n#### Four\n", true, 20.0), 40.0);
    assert_eq!(last_top("One.\n\n##### Five\n", true, 20.0), 48.0);
    assert_eq!(last_top("One.\n\nTwo.\n", true, 20.0), 40.0);
}

#[test]
fn a_heading_below_a_larger_margin_still_lands_on_a_row_and_keeps_its_gap() {
    // A thematic break's margin below it is 24, as large as the
    // heading's above it; every row height puts the heading on a row
    // with at least a paragraph's gap, 16, above it.
    for row in [17.0, 19.0, 20.0, 23.0, 36.0] {
        let document = parse("One.\n\n---\n\n## Two\n");
        let blocks: Vec<&Block> = document.blocks.iter().collect();
        let frame = Frame {
            snap: true,
            row,
            ..FRAME
        };
        let page = lay_out(&blocks, &LOOK, &Fonts::fixture(), (&Plain, &Plain), frame);
        let [.., rule, heading] = &page.blocks[..] else {
            panic!("blocks: {:?}", page.blocks);
        };
        let top = heading.rows.start;
        assert_eq!(top % row, 0.0, "row {row}: {top}");
        assert!(top - rule.rows.end >= 16.0, "row {row}: {top}");
    }
}

fn words(text: &str) -> Block {
    Block::Paragraph(vec![Inline::Text(text.into())])
}

/// The first text of `page` that starts with `start`: its left edge, and
/// the left and right edges of the words on each of its lines, from the
/// page's: a space at the end of a line hangs outside it.
fn edges(page: &Page, start: &str) -> (f32, Vec<(f32, f32)>) {
    (page.items.iter())
        .find_map(|item| match &item.shape {
            Shape::Text { paragraph, x, .. } if paragraph.text().starts_with(start) => {
                let text = paragraph.text();
                let lines = paragraph.lines_in(&(0..text.len()));
                let boxes = lines.into_iter().map(|(_, bytes)| {
                    let words = bytes.start..bytes.start + text[bytes].trim_end().len();
                    let found = paragraph.boxes(words);
                    (x + found[0].left, x + found[found.len() - 1].right)
                });
                Some((*x, boxes.collect()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text {start:?}"))
}

/// Whether each of the `lines` has as much room left of it as right of it
/// in the room from `from` to `to`, to within a pixel.
fn centred_in(lines: &[(f32, f32)], (from, to): (f32, f32)) -> bool {
    lines
        .iter()
        .all(|(left, right)| ((left - from) - (to - right)).abs() < 1.0)
}

#[test]
fn the_lines_of_a_centred_paragraph_have_as_much_room_on_the_left_as_on_the_right() {
    let long = "lorem ipsum dolor sit amet "
        .repeat(30)
        .trim_end()
        .to_owned();
    let alone = page_with(&[words(&long)], &NOTHING);
    let (_, lines) = edges(&alone, "lorem");
    assert!(lines.len() > 1 && lines.iter().all(|(left, _)| *left == 0.0));
    let page = page_with(&[Block::Centered(vec![words(&long)])], &NOTHING);
    let (x, lines) = edges(&page, "lorem");
    assert_eq!(x, 0.0);
    assert!(lines.len() > 1, "{lines:?}");
    assert!(centred_in(&lines, (0.0, 672.0)), "{lines:?}");
    // A short one is well in from the edge.
    let short = page_with(&[Block::Centered(vec![words("Short")])], &NOTHING);
    let (_, lines) = edges(&short, "Short");
    assert!(
        lines[0].0 > 200.0 && centred_in(&lines, (0.0, 672.0)),
        "{lines:?}"
    );
}

#[test]
fn the_lines_of_a_centred_paragraph_with_breaks_are_in_the_middle_by_their_words() {
    let source = "<p align=\"center\">\n  Line one <br>\n  Line two  \n</p>\n";
    let blocks = parse(source).blocks;
    let page = page_with(&blocks, &NOTHING);
    let (_, lines) = edges(&page, "Line one");
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(centred_in(&lines, (0.0, 672.0)), "{lines:?}");
}

#[test]
fn a_centred_heading_is_in_the_middle_and_its_rule_runs_the_whole_width() {
    let heading = Block::Heading {
        level: 2,
        content: vec![Inline::Text("Name".into())],
        id: None,
    };
    let page = page_with(&[Block::Centered(vec![heading])], &NOTHING);
    let (_, lines) = edges(&page, "Name");
    assert!(
        lines[0].0 > 200.0 && centred_in(&lines, (0.0, 672.0)),
        "{lines:?}"
    );
    let rules: Vec<(f32, f32)> = (page.items.iter())
        .filter_map(|item| match item.shape {
            Shape::Rect {
                x,
                width,
                height: 1.0,
                ..
            } => Some((x, width)),
            _ => None,
        })
        .collect();
    assert_eq!(rules, [(0.0, 672.0)]);
}

#[test]
fn a_centred_wrapper_centres_a_picture_a_linked_one_and_a_diagram() {
    let given = Given {
        image: |_| solid(100, 50, [255, 0, 0], 7),
        diagram: |_| solid(100, 40, [0, 255, 0], 1),
        ..NOTHING
    };
    let image = |link: bool| {
        let image = Inline::Image(crate::typeset::model::Image {
            url: "x.png".into(),
            alt: vec![Inline::Text("alt".into())],
            width: None,
            height: None,
        });
        match link {
            true => Block::Paragraph(vec![Inline::Link {
                url: "u".into(),
                title: None,
                content: vec![image],
            }]),
            false => Block::Paragraph(vec![image]),
        }
    };
    let diagram = Block::CodeBlock {
        language: Some("mermaid".into()),
        code: "graph\n".into(),
        whole_file: false,
    };
    let page = page_with(
        &[Block::Centered(vec![image(false), image(true), diagram])],
        &given,
    );
    let middle = (672.0 - 100.0) / 2.0;
    let xs: Vec<f32> = pictures(&page).iter().map(|picture| picture.0).collect();
    assert_eq!(xs, [middle; 3]);
    // The link covers the picture where it is.
    assert_eq!(page.links.len(), 1);
    assert_eq!(page.links[0].pieces[0].x, middle);
}

#[test]
fn a_centred_wrapper_leaves_a_table_a_list_a_quote_and_code_where_they_are() {
    let source = "| a | b |\n|---|---|\n| 1 | 2 |\n\n- one\n- two\n\n> quoted\n\n```rust\nlet a = 1;\n```\n\nplain\n";
    let blocks = parse(source).blocks;
    let alone = page_with(&blocks, &NOTHING);
    // The paragraph at the end is the one thing in the wrapper that moves.
    let (tail, rest) = blocks.split_last().unwrap();
    let wrapped = [
        Block::Centered(rest.to_vec()),
        Block::Centered(vec![tail.clone()]),
    ];
    let wrapped = page_with(&wrapped, &NOTHING);
    assert_eq!(texts(&wrapped).len(), texts(&alone).len());
    for (a, w) in texts(&alone).iter().zip(&texts(&wrapped)) {
        if !a.0.starts_with("plain") {
            assert_eq!(a, w);
        }
    }
    assert_eq!(edges(&alone, "plain").1[0].0, 0.0);
    assert!(edges(&wrapped, "plain").1[0].0 > 200.0);
    assert_eq!(alone.height, wrapped.height);
}

#[test]
fn a_centred_wrapper_inside_a_quote_centres_in_what_the_quote_leaves() {
    let page = page_with(
        &[Block::BlockQuote(vec![Block::Centered(vec![words(
            "Short",
        )])])],
        &NOTHING,
    );
    let (_, lines) = edges(&page, "Short");
    let indent = LOOK.spacing.quote_indent * LOOK.size;
    assert!(
        centred_in(&lines, (indent, 672.0)),
        "{lines:?} from {indent}"
    );
    assert!(lines[0].0 > indent + 100.0, "{lines:?}");
}
