//! Reading mode's pages shown in the terminal: each page is cut into parts
//! of whole rows, each part an image drawn when a line first shows it, and
//! each row of the page a line that shows one row of a part through
//! kitty's Unicode placeholders.

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use image::RgbaImage;

use super::block::BlockId;
use super::highlight;
use super::image::{Placement, Shows, Size, cell_size};
use super::layout::{CodeColors, expand_tabs};
use super::style::{ImageRow, Line, Span, Style};
use crate::caps::Capabilities;
use crate::color::Color;
use crate::images::ImageKey;
use crate::model::Block;
use crate::reading::Fonts;
use crate::reading::page::{self, Frame, Highlight, Look, Page, Plain, Wide};
use crate::reading::text::{Brush, Run};
use crate::theme::Theme;

/// The text's font size, as a share of the cell height: 16 pixels in rows
/// of 20, as a browser sets text in rows of that height.
const FONT_SIZE: f32 = 0.8;
/// The widest the text is, in font sizes.
const TEXT_WIDTH: f32 = 42.0;
/// How much wider than the text the area is that tables and code may
/// take: the area is centred in the window, and text starts at its left
/// edge.
const AREA: f32 = 1.5;
/// The cells kept free on each side of the area.
const SIDE: u16 = 2;
/// How much of the text colour a shaded table row mixes into the page, as
/// in the text layout.
const STRIPE: f32 = 0.06;
/// The rows of one part of a page, the image drawn at a time.
const PART_ROWS: u16 = 30;

/// What reading mode needs besides the document: the fonts it lays text
/// out in.
#[derive(Debug, Clone)]
pub struct Reading {
    pub fonts: Rc<Fonts>,
    /// What becomes of code blocks and tables wider than the area.
    pub wide: Wide,
}

/// Where reading mode sets its text in a window: the area that tables
/// and code may take, centred, and the text at its left edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    /// The cells before the area, and its width in cells.
    pub left: u16,
    pub area: u16,
    /// The text's width in pixels, and in whole cells: the column that
    /// the blocks laid out as text take.
    pub text: f32,
    pub column: u16,
}

impl Area {
    /// The area in a window `width` cells wide on a terminal with `caps`.
    pub fn of(caps: &Capabilities, width: usize) -> Self {
        let (cell_width, cell_height) = cell_size(caps);
        let (cell_width, size) = (
            f32::from(cell_width),
            (FONT_SIZE * f32::from(cell_height)).round(),
        );
        let window = u16::try_from(width).unwrap_or(u16::MAX);
        let room = window.saturating_sub(2 * SIDE).max(1);
        let text = (TEXT_WIDTH * size).min(f32::from(room) * cell_width);
        let area = ((AREA * text / cell_width).ceil() as u16).min(room);
        Self {
            left: (window - area) / 2,
            area,
            text,
            column: ((text / cell_width) as u16).clamp(1, area),
        }
    }
}

/// A page cut into parts, which draw themselves when they are first
/// shown.
pub struct Parts {
    /// What the page shows and how, hashed: two layouts of the same text
    /// at the same size share their parts, so the terminal keeps them.
    pub id: u64,
    page: Page,
    cell: (u16, u16),
    columns: u16,
    rows: u16,
}

impl Parts {
    /// The pixels of part `part`.
    pub fn draw(&self, part: u32) -> RgbaImage {
        let (width, height) = (u32::from(self.cell.0), u32::from(self.cell.1));
        let first = part * u32::from(PART_ROWS);
        let rows = (u32::from(self.rows) - first).min(u32::from(PART_ROWS));
        (self.page).draw(
            first * height,
            u32::from(self.columns) * width,
            rows * height,
        )
    }
}

impl Parts {
    /// The pixels of band `band` scrolled `offset` pixels, `selected` or
    /// not, as wide as the page.
    pub fn draw_band(&self, band: u32, offset: u32, selected: bool) -> RgbaImage {
        (self.page).draw_band(band as usize, offset as f32, selected)
    }

    /// The furthest band `band` scrolls, in pixels.
    pub fn band_most(&self, band: u32) -> u32 {
        (self.page.bands.get(band as usize)).map_or(0, |band| band.most().ceil() as u32)
    }

    /// The width band `band` shows at a time, in pixels.
    pub fn band_visible(&self, band: u32) -> u32 {
        (self.page.bands.get(band as usize)).map_or(0, |band| band.visible as u32)
    }
}

impl fmt::Debug for Parts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parts")
            .field("id", &self.id)
            .field("rows", &self.rows)
            .finish()
    }
}

impl PartialEq for Parts {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Parts {}

/// The lines that show `blocks`, which reading mode lays out, marked with
/// their `ids`, in a window `width` cells wide on a terminal with `caps`,
/// in `theme`'s colours; `document` is the number of the document they
/// are in among those shown.
pub fn lines(
    blocks: &[&Block],
    ids: &[BlockId],
    reading: &Reading,
    (theme, code, stripes): (&Theme, &CodeColors, bool),
    caps: &Capabilities,
    width: usize,
    document: usize,
) -> Vec<Line> {
    let cell = cell_size(caps);
    let (cell_width, cell_height) = (f32::from(cell.0), f32::from(cell.1));
    let look = look(theme, code, stripes, caps);
    let Area {
        left, area, text, ..
    } = Area::of(caps, width);
    // The page covers the area, as tables and code may reach across it.
    let columns = area.clamp(1, caps.max_image_cells);
    let highlight = Syntect {
        theme: code.theme.as_deref(),
        caps,
    };
    let page = page::lay_out(
        blocks,
        &look,
        &reading.fonts,
        &highlight,
        Frame {
            width: text,
            area: f32::from(columns) * cell_width,
            row: cell_height,
            wide: reading.wide,
        },
    );
    let rows = ((page.height / cell_height).ceil() as u16).max(1);
    let mut hasher = DefaultHasher::new();
    format!("{blocks:?}").hash(&mut hasher);
    format!("{look:?}").hash(&mut hasher);
    (text.to_bits(), cell, columns, reading.wide).hash(&mut hasher);
    let placed: Vec<_> = page.blocks.clone();
    let parts = Rc::new(Parts {
        id: hasher.finish(),
        page,
        cell,
        columns,
        rows,
    });
    let lead = Span::new(&" ".repeat(usize::from(left)), Style::default());
    let mut lines = Vec::with_capacity(usize::from(rows));
    // The rows each band covers, from the first, which it starts on.
    let bands: Vec<(u32, std::ops::Range<u16>)> = (parts.page.bands.iter().enumerate())
        .map(|(band, placed)| {
            let first = (placed.top / cell_height).round() as u16;
            let count =
                ((placed.height / cell_height).round() as u16).clamp(1, caps.max_image_cells);
            (band as u32, first..first + count)
        })
        .collect();
    let size = |rows: u16| Size {
        width: u32::from(columns) * u32::from(cell.0),
        height: u32::from(rows) * u32::from(cell.1),
        columns,
        rows,
    };
    for row in 0..rows {
        let (key, size, image_row) = match bands.iter().find(|(_, rows)| rows.contains(&row)) {
            // A band's rows show the band, at the start of its scrolling,
            // and not selected, until the pager scrolls or selects it.
            Some((band, band_rows)) => (
                ImageKey::Band {
                    document,
                    page: parts.id,
                    band: *band,
                    offset: 0,
                    selected: false,
                },
                size(band_rows.len() as u16),
                row - band_rows.start,
            ),
            None => {
                let part = row / PART_ROWS;
                let part_rows = (rows - part * PART_ROWS).min(PART_ROWS);
                let key = ImageKey::Page {
                    document,
                    page: parts.id,
                    slice: u32::from(part),
                };
                (key, size(part_rows), row % PART_ROWS)
            }
        };
        let placement = Rc::new(Placement {
            key,
            size,
            shows: Shows::Image,
        });
        let (top, bottom) = (
            f32::from(row) * cell_height,
            f32::from(row + 1) * cell_height,
        );
        let middle = (top + bottom) / 2.0;
        // A row belongs to the block its middle is in; the rows between
        // blocks belong to none. The row a block starts on carries its text
        // for a search.
        let block = (placed.iter().zip(ids)).find(|(placed, _)| placed.rows.contains(&middle));
        let starts =
            (placed.iter()).find(|placed| placed.rows.start >= top && placed.rows.start < bottom);
        let line = Line {
            lead: vec![lead.clone()],
            image: Some(ImageRow {
                placement,
                row: image_row,
                background: None,
                columns: 0..columns,
                text: starts.map(|placed| placed.text.clone()),
                page: Some(Rc::clone(&parts)),
            }),
            block: block.map(|(_, id)| *id),
            ..Line::blank()
        };
        lines.push(line);
    }
    lines
}

/// How reading mode draws text in `theme` on a terminal with `caps`: its
/// size from the cell height, and its colours from the theme, or from the
/// terminal where the theme leaves them.
/// Tables shade every second body row with `stripes`.
pub fn look(theme: &Theme, code: &CodeColors, stripes: bool, caps: &Capabilities) -> Look {
    let (_, cell_height) = cell_size(caps);
    let rgb = |color: Option<Color>| match color?.rgb_or_xterm(&caps.palette) {
        Color::Rgb(red, green, blue) => Some([red, green, blue]),
        Color::Palette(_) => None,
    };
    let dark = caps.dark_background != Some(false);
    let contrast = if dark { [0xff; 3] } else { [0; 3] };
    let text = rgb(theme.foreground)
        .or_else(|| rgb(caps.foreground))
        .unwrap_or(contrast);
    let page = (rgb(theme.background).or_else(|| rgb(caps.background))).unwrap_or(if dark {
        [0; 3]
    } else {
        [0xff; 3]
    });
    let muted = rgb(theme.muted).unwrap_or_else(|| mix(text, page, 0.6));
    let border = mix(text, page, 0.25);
    let headings = [1, 2, 3, 4, 5, 6].map(|level| rgb(theme.heading(level).color).unwrap_or(text));
    let chip = match super::layout::inline_code_background(theme, caps) {
        Some(color) => rgb(Some(color)),
        None => None,
    };
    Look {
        size: (FONT_SIZE * f32::from(cell_height)).round(),
        text,
        headings,
        link: rgb(theme.link).unwrap_or(text),
        code: rgb(theme.inline_code).unwrap_or(text),
        chip,
        quote: muted,
        border: rgb(theme.rule).unwrap_or(border),
        marker: rgb(theme.list_marker).unwrap_or(text),
        code_block: rgb(code.background),
        code_text: rgb(code.text).unwrap_or(text),
        label: rgb(code.label),
        table_border: rgb(theme.table_border).unwrap_or(border),
        stripe: stripes.then(|| mix(text, page, STRIPE)),
    }
}

/// Code highlighted as the text layout highlights it, with `theme`, or in
/// the plain colour of code without one.
struct Syntect<'a> {
    theme: Option<&'a str>,
    caps: &'a Capabilities,
}

impl Highlight for Syntect<'_> {
    fn lines(&self, language: Option<&str>, code: &str) -> Vec<Vec<Run>> {
        let code: String = code.lines().map(|line| expand_tabs(line) + "\n").collect();
        let highlighted =
            (self.theme).and_then(|theme| highlight::highlight(&code, language, theme));
        let Some(highlighted) = highlighted else {
            return Plain.lines(language, &code);
        };
        (highlighted.lines.into_iter())
            .map(|spans| {
                (spans.into_iter())
                    .map(|span| Run {
                        bold: span.style.bold,
                        italic: span.style.italic,
                        color: span.style.fg.and_then(|color| rgb(color, self.caps)),
                        ..Run::new(&span.text)
                    })
                    .collect()
            })
            .collect()
    }
}

fn rgb(color: Color, caps: &Capabilities) -> Option<Brush> {
    match color.rgb_or_xterm(&caps.palette) {
        Color::Rgb(red, green, blue) => Some([red, green, blue]),
        Color::Palette(_) => None,
    }
}

/// `share` of `color` over `page`.
fn mix(color: Brush, page: Brush, share: f32) -> Brush {
    let channel = |index: usize| {
        let (a, b) = (f32::from(color[index]), f32::from(page[index]));
        (b + (a - b) * share).round() as u8
    };
    [channel(0), channel(1), channel(2)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::render::{Inputs, layout, output};

    /// A terminal with cells of 10 by 20 pixels, as the tests assume.
    fn kitty(width: u16) -> Capabilities {
        Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(width)
        }
    }

    fn inputs(caps: Capabilities) -> Inputs {
        Inputs {
            reading: Some(Reading {
                fonts: Rc::new(Fonts::fixture()),
                wide: Wide::Wrap,
            }),
            ..Inputs::new(caps)
        }
    }

    const DOCUMENT: &str =
        "# Title\n\nSome *text*.\n\n```\ncode\n```\n\n> [!NOTE]\n> A note.\n\nMore text.\n";

    /// What each line shows: `page R/P` for row R of part P of a page,
    /// with the block's text where one starts, or the line's text.
    fn shown(lines: &[Line]) -> Vec<String> {
        (lines.iter())
            .map(|line| match &line.image {
                Some(ImageRow {
                    placement,
                    row,
                    text,
                    page: Some(_),
                    ..
                }) => {
                    let (kind, slice) = match placement.key {
                        ImageKey::Page { slice, .. } => ("page", slice),
                        ImageKey::Band { band, .. } => ("band", band),
                        _ => panic!("a page's key: {placement:?}"),
                    };
                    let lead: String = line.lead.iter().map(|span| span.text.as_str()).collect();
                    let text = text
                        .as_deref()
                        .map(|text| format!(" {text:?}"))
                        .unwrap_or_default();
                    let block = line.block.map_or("-".to_owned(), |_| "block".to_owned());
                    format!("{}{kind} {row}/{slice} {block}{text}", lead.len())
                }
                _ => crate::render::style::text(&line.spans),
            })
            .collect()
    }

    #[test]
    fn text_blocks_show_as_rows_of_a_page_between_the_other_blocks() {
        let lines = layout::layout(&parse(DOCUMENT), &inputs(kitty(80)));
        insta::assert_snapshot!(shown(&lines).join("\n"));
    }

    #[test]
    fn a_code_block_wider_than_the_area_scrolls_in_rows_of_its_own() {
        let long = "a".repeat(200);
        let document = parse(&format!("Text.\n\n```\n{long}\n```\n\nMore text.\n"));
        let mut inputs = inputs(kitty(80));
        if let Some(reading) = &mut inputs.reading {
            reading.wide = Wide::Scroll;
        }
        let lines = layout::layout(&document, &inputs);
        insta::assert_snapshot!(shown(&lines).join("\n"));
    }

    #[test]
    fn without_images_the_text_is_laid_out_as_text() {
        let caps = Capabilities::ansi(80);
        let document = parse(DOCUMENT);
        let reading = layout::layout(&document, &inputs(caps.clone()));
        let text = layout::layout(&document, &Inputs::new(caps));
        assert_eq!(reading, text);
    }

    #[test]
    fn the_column_is_42_font_sizes_wide_at_the_left_of_an_area_half_as_wide_again() {
        // 16 px text: 672 px or 68 cells, in an area of 101 of 160 cells,
        // which the page covers for the tables and code that reach into it.
        let lines = layout::layout(&parse("Text.\n"), &inputs(kitty(160)));
        let row = lines[0].image.as_ref().expect("a page's row");
        assert_eq!(row.columns, 0..101);
        let lead: usize = lines[0].lead.iter().map(|span| span.text.len()).sum();
        assert_eq!(lead, (160 - 101) / 2);
        // In a narrow window, the window less two cells on each side.
        let lines = layout::layout(&parse("Text.\n"), &inputs(kitty(40)));
        let row = lines[0].image.as_ref().expect("a page's row");
        assert_eq!(row.columns, 0..36);
        assert_eq!(lines[0].lead[0].text.len(), 2);
    }

    #[test]
    fn the_other_blocks_take_the_column_from_the_area_s_edge() {
        let note = "A note long enough to wrap. ".repeat(8);
        let document = parse(&format!("Text.\n\n> [!NOTE]\n> {note}\n"));
        let lines = layout::layout(&document, &inputs(kitty(160)));
        let alert: Vec<&Line> = (lines.iter())
            .filter(|line| line.image.is_none() && !line.is_blank())
            .collect();
        assert!(!alert.is_empty());
        for line in alert {
            let lead: String = line.lead.iter().map(|span| span.text.as_str()).collect();
            assert_eq!(lead.len(), 29, "{line:?}");
            let cells = crate::render::wrap::width(&line.lead)
                + crate::render::wrap::width(&line.spans)
                + crate::render::wrap::width(&line.trail);
            // As wide as the text's column, 67 cells, not the area.
            assert!(cells <= 29 + 67, "{cells}");
        }
    }

    #[test]
    fn the_same_text_at_the_same_size_is_the_same_page() {
        let id = |width| {
            let lines = layout::layout(&parse(DOCUMENT), &inputs(kitty(width)));
            match &lines[0].image.as_ref().expect("a page's row").placement.key {
                ImageKey::Page { page, .. } => *page,
                key => panic!("a page's key: {key:?}"),
            }
        };
        assert_eq!(id(120), id(120));
        assert_ne!(id(120), id(60));
    }

    #[test]
    fn printing_sends_each_part_once_with_its_rows() {
        let caps = kitty(80);
        let inputs = inputs(caps.clone());
        let lines = layout::layout(&parse(&"Paragraph.\n\n".repeat(25)), &inputs);
        let mut out = Vec::new();
        output::write(
            &lines,
            &inputs.images,
            &caps,
            output::Page::default(),
            &mut out,
        )
        .unwrap();
        let out = String::from_utf8(out).unwrap();
        let sent: Vec<&str> = (out.split("\x1b_G").skip(1))
            .filter_map(|command| command.split(';').next())
            .filter(|keys| keys.starts_with("a=T"))
            .collect();
        // 25 paragraphs of 24 px, 16 px apart: 984 px, or 50 rows in
        // parts of 30 and 20.
        assert_eq!(sent.len(), 2, "{sent:?}");
        // The parts cover the area, 76 of the 80 columns.
        assert!(sent[0].contains("s=760,v=600,c=76,r=30"), "{sent:?}");
        assert!(sent[1].contains("s=760,v=400,c=76,r=20"), "{sent:?}");
        assert_eq!(out.matches('\u{10EEEE}').count(), 50 * 76);
    }

    #[test]
    fn the_look_takes_the_theme_s_colours_and_the_cell_height() {
        let mut caps = kitty(80);
        caps.cell_pixels = Some((12, 25));
        let theme = Theme {
            link: Some(Color::Rgb(1, 2, 3)),
            ..Theme::default()
        };
        let look = look(&theme, &CodeColors::default(), true, &caps);
        assert_eq!(look.size, 20.0);
        assert_eq!(look.link, [1, 2, 3]);
    }
}
