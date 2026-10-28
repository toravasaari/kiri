//! Reading mode's pages shown in the terminal: each page is cut into parts
//! of whole rows, each part an image drawn when a line first shows it, and
//! each row of the page a line that shows one row of a part through
//! kitty's Unicode placeholders.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use image::RgbaImage;

use super::block::BlockId;
use super::highlight;
use super::image::{self as sizes, Placement, Shows, Size, Wanted, cell_size};
use super::layout::{CodeColors, expand_tabs};
use super::loading;
use super::style::{Band, ImageRow, Line, Span, Style};
use crate::caps::Capabilities;
use crate::color::Color;
use crate::diagrams::INLINE_FORMULA;
use crate::images::{ImageError, ImageKey, Images, Placeholder, Source};
use crate::model::{self, AlertKind, Block};
use crate::reading::Fonts;
use crate::reading::page::{
    self, AlertLook, Frame, Highlight, Look, Page, Picture, Pictures, Plain, Wide,
};
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
/// The most rows a picture is shown at, as many as an image can cover.
const MAX_ROWS: u16 = 297;
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
    /// The pixels of the rows of slice `slice` of band `band`, scrolled
    /// `offset` pixels, `selected` or not, as wide as the page.
    pub fn draw_band(&self, band: u32, slice: u32, offset: u32, selected: bool) -> RgbaImage {
        let cell = f32::from(self.cell.1);
        let total = (self.page.bands.get(band as usize)).map_or(0.0, |band| band.height);
        let from = (slice * u32::from(PART_ROWS)) as f32 * cell;
        let to = (from + f32::from(PART_ROWS) * cell).min(total);
        (self.page).draw_band(band as usize, offset as f32, selected, from..to)
    }

    /// The furthest band `band` scrolls, in pixels.
    pub fn band_most(&self, band: u32) -> u32 {
        (self.page.bands.get(band as usize)).map_or(0, |band| band.most().ceil() as u32)
    }

    /// How far a key scrolls a band sideways: four font sizes.
    pub fn step(&self) -> u32 {
        (4.0 * self.page.look().size).round() as u32
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
/// in `theme`'s colours, with the pictures in `images`, of the document
/// numbered `document` among those shown, and diagrams telling why they
/// failed when `inline_errors`.
pub fn lines(
    blocks: &[&Block],
    ids: &[BlockId],
    reading: &Reading,
    (theme, code, stripes): (&Theme, &CodeColors, bool),
    (images, inline_errors, document): (&Images, bool, usize),
    caps: &Capabilities,
    width: usize,
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
    let gallery = Gallery {
        images,
        document,
        cell,
        inline_errors,
        keys: RefCell::new(Vec::new()),
    };
    let page = page::lay_out(
        blocks,
        &look,
        &reading.fonts,
        (&highlight, &gallery),
        Frame {
            width: text,
            area: f32::from(columns) * cell_width,
            row: cell_height,
            wide: reading.wide,
        },
    );
    let rows = ((page.height / cell_height).ceil() as u16).max(1);
    // The id names these pixels to the terminal, which keeps an image it
    // holds under the same id, so it must cover everything that changes
    // them: the blocks, the look (every colour, and the font size), the
    // sizes, whether wide blocks wrap or scroll, and the pictures, by their
    // pixels. The fonts and the
    // code theme's name are not in it: the fonts are the same for a run,
    // and the theme has so far always come with another colour in `Look`.
    // An input to the pixels that does not, goes in here too.
    let mut hasher = DefaultHasher::new();
    format!("{blocks:?}").hash(&mut hasher);
    format!("{look:?}").hash(&mut hasher);
    (text.to_bits(), cell, columns, reading.wide).hash(&mut hasher);
    page.pictures.hash(&mut hasher);
    let placed: Vec<_> = page.blocks.clone();
    let loading = page.loading.clone();
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
            let count = ((placed.height / cell_height).round() as u16).max(1);
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
            // and not selected, until the pager scrolls or selects it, in
            // slices of at most 30 rows as pages are.
            Some((band, band_rows)) => {
                let (into, count) = (row - band_rows.start, band_rows.len() as u16);
                let slice = into / PART_ROWS;
                let slice_rows = (count - slice * PART_ROWS).min(PART_ROWS);
                let key = ImageKey::Band {
                    document,
                    page: parts.id,
                    band: *band,
                    slice: u32::from(slice),
                    offset: 0,
                    selected: false,
                };
                (key, size(slice_rows), into % PART_ROWS)
            }
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
    // Over the bottom row of each picture still loading, a band that says
    // so, from where the picture starts.
    let keys = gallery.keys.into_inner();
    for loading in loading {
        let (Some(key), true) = (keys.get(loading.tag), !lines.is_empty()) else {
            continue;
        };
        let first = ((loading.rows.start / cell_height).floor() as usize).min(lines.len() - 1);
        let last = (((loading.rows.end / cell_height).ceil() as usize).saturating_sub(1))
            .clamp(first, lines.len() - 1);
        let columns = ((loading.width / cell_width).ceil() as u16).clamp(1, caps.max_image_cells);
        lines[last].band = Some(Band {
            columns,
            indent: (loading.x / cell_width).floor() as usize,
            low: true,
            key: key.clone(),
            above: last - first,
        });
    }
    lines
}

/// The pictures of a document, found in the images that were loaded for it.
struct Gallery<'a> {
    images: &'a Images,
    /// The number of the document among those shown together, which the
    /// keys of its images carry.
    document: usize,
    cell: (u16, u16),
    /// Whether a diagram that could not be drawn tells why below its code.
    inline_errors: bool,
    /// The keys of the pictures that are loading, which their tags number.
    keys: RefCell<Vec<ImageKey>>,
}

impl Gallery<'_> {
    /// The picture the images hold under `key`, at the size `wanted` asks
    /// for in `room`; `language` when it is a diagram.
    fn picture(
        &self,
        key: ImageKey,
        wanted: Wanted,
        room: (f32, f32),
        language: Option<&str>,
    ) -> Picture {
        let room = (
            f64::from(room.0),
            f64::from(room.1).min(f64::from(MAX_ROWS) * f64::from(self.cell.1)),
        );
        let tag = || {
            let mut keys = self.keys.borrow_mut();
            keys.push(key.clone());
            keys.len() - 1
        };
        let drawn = |source: &Source, dimmed: bool| {
            let size = sizes::pixel_size(source.size(), wanted, self.cell, room)?;
            let mut pixels = source.render(size.0, size.1);
            if dimmed {
                pixels = loading::dimmed(pixels);
            }
            Some(pixels)
        };
        match (self.images.get(&key), self.images.placeholder(&key)) {
            (Some(Ok(source)), _) => match drawn(source, false) {
                Some(pixels) => ready(pixels, None),
                None => Picture::None,
            },
            (Some(Err(ImageError::Diagram { reason, .. })), _) if language.is_some() => {
                let noun = language.map(crate::diagrams::noun).unwrap_or_default();
                let message = format!("✗ {noun} could not be drawn: {reason}");
                Picture::Failed(self.inline_errors.then_some(message))
            }
            (Some(Err(_)), _) => Picture::None,
            (_, Some(Placeholder::Dimmed(old))) => match drawn(old, true) {
                Some(pixels) => ready(pixels, Some(tag())),
                None => Picture::None,
            },
            (_, Some(&Placeholder::Rows(rows))) => Picture::Loading {
                width: u32::from(self.cell.0),
                height: u32::from(rows) * u32::from(self.cell.1),
                tag: tag(),
            },
            _ => Picture::None,
        }
    }
}

/// `pixels` as a picture, with an id from what they are.
fn ready(pixels: RgbaImage, loading: Option<usize>) -> Picture {
    let mut hasher = DefaultHasher::new();
    pixels.dimensions().hash(&mut hasher);
    pixels.as_raw().hash(&mut hasher);
    Picture::Ready {
        pixels: Rc::new(pixels),
        id: hasher.finish(),
        loading,
        depth: 0.0,
    }
}

impl Pictures for Gallery<'_> {
    fn formula(&self, source: &str, room: (f32, f32)) -> Picture {
        let key = ImageKey::block(INLINE_FORMULA, source).in_document(self.document);
        let picture = self.picture(key, Wanted::default(), room, Some(INLINE_FORMULA));
        match picture {
            Picture::Ready {
                pixels,
                id,
                loading,
                ..
            } => {
                // The renderer tells how much of the image is below the
                // formula's baseline.
                let below = crate::diagrams::baseline_depth(INLINE_FORMULA, source);
                let depth = below.map_or(0.0, |below| below as f32 * pixels.height() as f32);
                Picture::Ready {
                    pixels,
                    id,
                    loading,
                    depth,
                }
            }
            other => other,
        }
    }

    fn image(&self, shown: &model::Image, room: (f32, f32)) -> Picture {
        let key = ImageKey::from(shown.url.as_str()).in_document(self.document);
        let wanted = Wanted {
            width: shown.width,
            height: shown.height,
        };
        self.picture(key, wanted, room, None)
    }

    fn diagram(&self, language: &str, code: &str, room: (f32, f32)) -> Picture {
        // Spares building a key for every code block of a document
        // without images.
        if self.images.is_empty() && !self.images.loading() {
            return Picture::None;
        }
        let key = ImageKey::block(language, code).in_document(self.document);
        self.picture(key, Wanted::default(), room, Some(language))
    }
}

/// The rows of one band in a run of lines, as the pager scrolls it.
#[derive(Debug, Clone)]
pub struct BandRows {
    /// The lines that show it.
    pub lines: std::ops::Range<usize>,
    /// The page it is on, and its number there.
    pub parts: Rc<Parts>,
    pub band: u32,
    /// How far it is scrolled, and whether it is selected.
    pub offset: u32,
    pub selected: bool,
}

impl BandRows {
    /// How far it scrolls at most, in pixels.
    pub fn most(&self) -> u32 {
        self.parts.band_most(self.band)
    }

    /// The width it shows at a time, in pixels.
    pub fn visible(&self) -> u32 {
        self.parts.band_visible(self.band)
    }
}

/// The bands that `lines` show, in order.
pub fn bands(lines: &[Line]) -> Vec<BandRows> {
    let mut bands: Vec<BandRows> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(ImageRow {
            placement,
            page: Some(parts),
            ..
        }) = &line.image
        else {
            continue;
        };
        let ImageKey::Band {
            page,
            band,
            offset,
            selected,
            ..
        } = placement.key
        else {
            continue;
        };
        match bands.last_mut() {
            Some(last) if last.parts.id == page && last.band == band && last.lines.end == index => {
                last.lines.end = index + 1;
            }
            _ => bands.push(BandRows {
                lines: index..index + 1,
                parts: Rc::clone(parts),
                band,
                offset,
                selected,
            }),
        }
    }
    bands
}

/// Shows the band on `lines` scrolled `offset` pixels, and `selected` or
/// not, in place of how they showed it.
pub fn set_band(lines: &mut [Line], offset: u32, selected: bool) {
    for line in lines {
        let Some(image) = &mut line.image else {
            continue;
        };
        if let ImageKey::Band {
            document,
            page,
            band,
            slice,
            ..
        } = image.placement.key
        {
            image.placement = Rc::new(Placement {
                key: ImageKey::Band {
                    document,
                    page,
                    band,
                    slice,
                    offset,
                    selected,
                },
                ..(*image.placement).clone()
            });
        }
    }
}

/// How reading mode draws text in `theme` on a terminal with `caps`: its
/// size from the cell height, and its colours from the theme, or from the
/// terminal where the theme leaves them. Tables shade every second body
/// row when `stripes` is set.
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
    // An alert is a tint of its colour over the page, and inline code in it
    // a stronger one, when the theme has tints, as the text layout has them.
    let alert = |kind| {
        let color = rgb(theme.alerts.color(kind, dark)).unwrap_or(text);
        let tint = |share| theme.alerts.tint.then(|| mix(color, page, share));
        AlertLook {
            color,
            tint: tint(0.15),
            chip: tint(0.3).filter(|_| chip.is_some()),
        }
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
        error: rgb(theme.error).unwrap_or([0xf8, 0x51, 0x49]),
        table_border: rgb(theme.table_border).unwrap_or(border),
        stripe: stripes.then(|| mix(text, page, STRIPE)),
        alerts: [
            AlertKind::Note,
            AlertKind::Tip,
            AlertKind::Important,
            AlertKind::Warning,
            AlertKind::Caution,
        ]
        .map(alert),
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
    use crate::images::ImageError;
    use crate::parse::parse;
    use crate::render::{Inputs, RenderErrors, layout, output};

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
    fn a_band_taller_than_the_placeholder_limit_is_cut_into_slices_of_30_rows() {
        // Each line is wider than the area, so the block is one band of
        // about 300 lines, taller than the 297 rows a placement may have.
        let code: String = (0..300)
            .map(|n| format!("{n} {}\n", "a".repeat(200)))
            .collect();
        let document = parse(&format!("```\n{code}```\n"));
        let mut inputs = inputs(kitty(80));
        if let Some(reading) = &mut inputs.reading {
            reading.wide = Wide::Scroll;
        }
        let lines = layout::layout(&document, &inputs);
        let rows: Vec<&ImageRow> = (lines.iter())
            .filter_map(|line| line.image.as_ref())
            .filter(|row| matches!(row.placement.key, ImageKey::Band { .. }))
            .collect();
        // Every row of the band is a band row, as there are lines of code
        // at 29 pixels each, in rows of 20, and a scroll bar.
        assert!(rows.len() > 297, "{} band rows", rows.len());
        // Each row's image is at most 30 rows tall, its row is within it,
        // and the slices add up to the band's rows.
        assert!(
            rows.iter().all(|row| row.placement.size.rows <= 30),
            "a slice over 30 rows"
        );
        assert!(rows.iter().all(|row| row.row < row.placement.size.rows));
        let mut slices: Vec<(u32, u16)> = (rows.iter())
            .filter_map(|row| match row.placement.key {
                ImageKey::Band { slice, .. } => Some((slice, row.placement.size.rows)),
                _ => None,
            })
            .collect();
        slices.dedup();
        // The pixels of each slice are as many as its placement covers.
        let mut drawn = Vec::new();
        for row in &rows {
            let ImageKey::Band { band, slice, .. } = row.placement.key else {
                continue;
            };
            if std::mem::replace(&mut drawn, vec![slice]) == [slice] {
                continue;
            }
            let parts = row.page.as_ref().expect("a page");
            let image = parts.draw_band(band, slice, 0, false);
            let size = row.placement.size;
            assert_eq!((image.width(), image.height()), (size.width, size.height));
        }
        assert_eq!(
            slices
                .iter()
                .map(|&(_, rows)| usize::from(rows))
                .sum::<usize>(),
            rows.len()
        );
    }

    /// A raster image of `width` × `height` pixels, all of `colour`.
    fn raster(width: u32, height: u32, colour: [u8; 3]) -> Source {
        Source::Raster(RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([colour[0], colour[1], colour[2], 255]),
        ))
    }

    /// The first part's pixels of the page that `lines` show.
    fn first_part(lines: &[Line]) -> RgbaImage {
        let row = lines[0].image.as_ref().expect("a page's row");
        row.page.as_ref().expect("a page").draw(0)
    }

    fn page_id(lines: &[Line]) -> u64 {
        match lines[0].image.as_ref().expect("a row").placement.key {
            ImageKey::Page { page, .. } => page,
            ref key => panic!("a page's key: {key:?}"),
        }
    }

    #[test]
    fn an_image_block_is_drawn_into_the_page_at_its_css_size() {
        let mut inputs = inputs(kitty(80));
        inputs
            .images
            .insert("a.png", Ok(raster(100, 40, [255, 0, 0])));
        let lines = layout::layout(&parse("![a](a.png)\n"), &inputs);
        let part = first_part(&lines);
        // One CSS pixel is a pixel at rows of 20, as without reading mode.
        assert_eq!(part.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(part.get_pixel(99, 39).0, [255, 0, 0, 255]);
        assert_eq!(part.get_pixel(100, 0).0[3], 0);
        // The two rows it covers belong to its block, and nothing else
        // draws a picture there.
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn an_image_wider_than_the_column_shrinks_to_it() {
        let mut inputs = inputs(kitty(160));
        inputs
            .images
            .insert("a.png", Ok(raster(1600, 200, [0, 255, 0])));
        let lines = layout::layout(&parse("![a](a.png)\n"), &inputs);
        let part = first_part(&lines);
        // The column is 42 font sizes, 672 pixels.
        assert_eq!(part.get_pixel(671, 0).0, [0, 255, 0, 255]);
        assert_eq!(part.get_pixel(672, 0).0[3], 0);
        assert_eq!(part.get_pixel(0, 83).0, [0, 255, 0, 255]);
        assert_eq!(part.get_pixel(0, 84).0[3], 0);
    }

    #[test]
    fn an_image_in_the_middle_of_text_is_a_box_in_its_line() {
        let mut inputs = inputs(kitty(80));
        inputs
            .images
            .insert("a.png", Ok(raster(20, 10, [255, 0, 0])));
        let lines = layout::layout(&parse("Some text ![a](a.png) after it.\n"), &inputs);
        let part = first_part(&lines);
        let reds = part
            .pixels()
            .filter(|pixel| pixel.0 == [255, 0, 0, 255])
            .count();
        assert_eq!(reds, 20 * 10);
        // The line is still one row, as the picture is shorter than the line.
        assert_eq!(lines.len(), 2);
        // The same text without the image shows its alt text and no box.
        let lines = layout::layout(
            &parse("Some text ![a](a.png) after it.\n"),
            &self::inputs(kitty(80)),
        );
        let reds = first_part(&lines)
            .pixels()
            .filter(|pixel| pixel.0 == [255, 0, 0, 255])
            .count();
        assert_eq!(reds, 0);
    }

    #[test]
    fn a_formula_in_the_middle_of_text_is_a_box_in_its_line() {
        let mut inputs = inputs(kitty(80));
        inputs.images.insert(
            ImageKey::block("math-inline", "x^2\n"),
            Ok(raster(20, 10, [0, 255, 0])),
        );
        let lines = layout::layout(&parse("Let $x^2$ be one.\n"), &inputs);
        let greens = first_part(&lines)
            .pixels()
            .filter(|pixel| pixel.0 == [0, 255, 0, 255])
            .count();
        assert_eq!(greens, 20 * 10);
        // Without its picture the source is shown, as written.
        let lines = layout::layout(&parse("Let $x^2$ be one.\n"), &self::inputs(kitty(80)));
        let row = lines[0].image.as_ref().expect("a page's row");
        assert_eq!(row.text.as_deref(), Some("Let $x^2$ be one."));
    }

    #[test]
    fn a_formula_s_baseline_is_the_text_s_and_an_image_s_is_its_bottom_edge() {
        // The lowest row of a picture 20 pixels high in a line of text:
        // an image sits on the baseline, a formula with something below its
        // own baseline reaches below it.
        let lowest = |source: &str, key: ImageKey| {
            let mut inputs = inputs(kitty(80));
            inputs.images.insert(key, Ok(raster(20, 20, [0, 255, 0])));
            let lines = layout::layout(&parse(source), &inputs);
            let part = first_part(&lines);
            (0..part.height())
                .rev()
                .find(|&y| (0..part.width()).any(|x| part.get_pixel(x, y).0 == [0, 255, 0, 255]))
                .expect("the picture is drawn")
        };
        let image = lowest("A ![a](a.png) b.\n", ImageKey::from("a.png"));
        let formula = lowest(
            "A $\\frac{a}{b}$ b.\n",
            ImageKey::block("math-inline", "\\frac{a}{b}\n"),
        );
        // The line's baseline is a little higher than the image's bottom
        // edge was, as the text sets it; a fraction's depth is a fifth of
        // its image or more.
        assert!(formula >= image + 3, "{formula} against {image}");
    }

    #[test]
    fn an_image_that_is_not_there_is_its_alt_text() {
        let lines = layout::layout(&parse("![the alt text](a.png)\n"), &inputs(kitty(80)));
        let row = lines[0].image.as_ref().expect("a page's row");
        assert_eq!(row.text.as_deref(), Some("the alt text"));
        assert!(first_part(&lines).pixels().any(|pixel| pixel.0[3] > 0));
    }

    #[test]
    fn an_image_that_is_loading_has_a_band_over_its_last_row() {
        let mut inputs = inputs(kitty(80));
        let key = ImageKey::from("a.png");
        inputs.images.wait(key.clone(), Placeholder::Rows(3));
        let lines = layout::layout(&parse("![a](a.png)\n\nAfter.\n"), &inputs);
        // Three rows reserved, and the band on the third, as the lines of
        // the page above it show the image too.
        let bands: Vec<(usize, &Band)> = (lines.iter().enumerate())
            .filter_map(|(index, line)| line.band.as_ref().map(|band| (index, band)))
            .collect();
        let [(index, band)] = bands[..] else {
            panic!("one band: {bands:?}");
        };
        assert_eq!(index, 2);
        assert_eq!(band.key, key);
        assert_eq!(band.above, 2);
        assert!(band.low);
        assert_eq!(band.columns, 1);
        // The paragraph below starts after the reserved room and its
        // margin.
        let row = lines[3].image.as_ref().expect("a row");
        assert_eq!(row.text.as_deref(), Some("After."));
    }

    #[test]
    fn the_old_image_shows_dimmed_with_a_band_while_a_new_one_loads() {
        let mut inputs = inputs(kitty(80));
        let key = ImageKey::from("a.png");
        inputs.images.wait(
            key.clone(),
            Placeholder::Dimmed(raster(100, 40, [255, 0, 0])),
        );
        let lines = layout::layout(&parse("![a](a.png)\n"), &inputs);
        let pixel = first_part(&lines).get_pixel(0, 0).0;
        assert_eq!(&pixel[..3], &[255, 0, 0]);
        assert!(pixel[3] > 0 && pixel[3] < 255, "{pixel:?}");
        let band = (lines.iter()).find_map(|line| line.band.as_ref());
        assert_eq!(band.map(|band| band.key.clone()), Some(key));
    }

    #[test]
    fn the_same_page_with_other_pixels_is_another_page() {
        let id = |colour| {
            let mut inputs = inputs(kitty(80));
            inputs.images.insert("a.png", Ok(raster(100, 40, colour)));
            page_id(&layout::layout(&parse("![a](a.png)\n"), &inputs))
        };
        assert_eq!(id([255, 0, 0]), id([255, 0, 0]));
        assert_ne!(id([255, 0, 0]), id([0, 0, 255]));
    }

    #[test]
    fn a_formula_is_centred_in_the_column_and_a_failed_diagram_says_why() {
        let mut inputs = inputs(kitty(80));
        inputs.images.insert(
            ImageKey::block("math", "x\n"),
            Ok(raster(100, 40, [255, 255, 255])),
        );
        let lines = layout::layout(&parse("```math\nx\n```\n"), &inputs);
        let part = first_part(&lines);
        // (672 - 100) / 2 = 286.
        assert_eq!(part.get_pixel(286, 0).0, [255, 255, 255, 255]);
        assert_eq!(part.get_pixel(285, 0).0[3], 0);
        assert_eq!(part.get_pixel(386, 0).0[3], 0);
        // A diagram that failed is its code and the reason below it,
        // unless the reason is told on standard error alone.
        let failed = |render_errors| {
            let mut inputs = Inputs {
                render_errors,
                ..self::inputs(kitty(80))
            };
            let error = ImageError::Diagram {
                name: "diagram".into(),
                reason: "no program".into(),
            };
            inputs
                .images
                .insert(ImageKey::block("mermaid", "graph\n"), Err(error));
            layout::layout(&parse("```mermaid\ngraph\n```\n"), &inputs).len()
        };
        assert!(failed(RenderErrors::Both) > failed(RenderErrors::Stderr));
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
        let document = parse(&format!(
            "Text.\n\n<details>\n<summary>Why</summary>\n\n{note}\n\n</details>\n"
        ));
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
