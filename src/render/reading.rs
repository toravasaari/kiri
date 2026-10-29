//! Reading mode's pages shown in the terminal: each page is cut into parts
//! of whole rows, each part an image drawn when a line first shows it, and
//! each row of the page a line that shows one row of a part through
//! kitty's Unicode placeholders.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
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
    self, AlertLook, Frame, FrontMatterShown, Highlight, Hit, ItemRef, LinkPlace, Look, MarkColors,
    Overlay, Page, Picture, Pictures, Plain, Wide,
};
use crate::reading::text::{Brush, Run};
use crate::render::FrontMatterStyle;
use crate::theme::{SearchColor, TableStyle, Theme};

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
    /// The pictures of the last layout, scaled.
    pub pictures: Rc<RefCell<PictureCache>>,
}

/// Images scaled to the size a layout shows them at, kept from one layout
/// to the next so that a layout does not scale every picture again. Only
/// what the last layout used stays.
#[derive(Debug, Default)]
pub struct PictureCache {
    entries: HashMap<PictureKey, (Rc<RgbaImage>, u64)>,
    /// What the layout under way has used.
    used: HashSet<PictureKey>,
    /// How many pictures were scaled, not taken from here.
    scaled: usize,
}

/// An image, by its key, what its pixels are and the size it is scaled to.
type PictureKey = (ImageKey, u64, (u32, u32));

impl PictureCache {
    /// The pictures held, and how many were ever scaled.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn scaled(&self) -> usize {
        self.scaled
    }

    /// The bytes of the pixels held.
    pub fn bytes(&self) -> usize {
        self.entries
            .values()
            .map(|(pixels, _)| pixels.as_raw().len())
            .sum()
    }

    /// Ends a layout: what it did not use is let go.
    pub fn finish(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.entries.retain(|key, _| used.contains(key));
    }

    /// The picture under `key`, scaled by `scale` when it is not held.
    fn get(&mut self, key: PictureKey, scale: impl FnOnce() -> RgbaImage) -> (Rc<RgbaImage>, u64) {
        self.used.insert(key.clone());
        if let Some((pixels, id)) = self.entries.get(&key) {
            return (Rc::clone(pixels), *id);
        }
        self.scaled += 1;
        let (pixels, id) = hashed(scale());
        let pixels = Rc::new(pixels);
        self.entries.insert(key, (Rc::clone(&pixels), id));
        (pixels, id)
    }
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
    /// The colours of a search's marks on it.
    marks: MarkColors,
}

/// A link over some cells of a row of a page: the columns of the page's
/// image it covers, where it goes, and which of the page's links it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowLink {
    pub columns: std::ops::Range<u16>,
    pub url: Rc<str>,
    /// The page, by the address of its parts, and the link's number among
    /// the page's links.
    pub link: (usize, usize),
}

impl Parts {
    /// The places of a search in the page's texts, from the top down:
    /// `places` gives the byte ranges of the term in a text.
    pub fn find(&self, places: &dyn Fn(&str) -> Vec<std::ops::Range<usize>>) -> Vec<Hit> {
        self.page.find(places)
    }

    /// The links on the page, from the top down.
    pub fn links(&self) -> &[LinkPlace] {
        self.page.links()
    }

    /// The row of the page that link `number` starts on.
    pub fn link_row(&self, number: usize) -> usize {
        let Some(link) = self.page.links().get(number) else {
            return 0;
        };
        let first = link.pieces.first().map_or(0.0, |piece| piece.y);
        let top = link
            .band
            .and_then(|band| self.band_top(band as u32))
            .unwrap_or(0.0);
        ((top + first) / f32::from(self.cell.1)).floor().max(0.0) as usize
    }

    /// The links over the cells of row `row` of the page, or of band
    /// `band`'s rows when it is given, scrolled `offset` pixels, as they
    /// are laid over its placeholders. A link covers the cells whose
    /// centres its text is over, in the rows whose centres its line is
    /// over, and at least the cell at the middle of its text; where two
    /// links want a cell, the one over its centre has it, and then the
    /// earlier one.
    pub fn row_links(&self, address: usize, band: Option<(u32, u32)>, row: u16) -> Vec<RowLink> {
        let (cell_width, cell_height) = (f32::from(self.cell.0), f32::from(self.cell.1));
        let middle = (f32::from(row) + 0.5) * cell_height;
        let shown = band.and_then(|(number, offset)| {
            let band = self.page.bands.get(number as usize)?;
            Some((number as usize, band.x, band.visible, offset as f32))
        });
        // The cell each link claims: for those over its centre, a link
        // number to beat another's, and 0 for those it has only for its
        // middle.
        let mut claims: Vec<(u16, bool, usize)> = Vec::new();
        for (number, link) in self.page.links().iter().enumerate() {
            if link.band != shown.map(|(band, ..)| band) {
                continue;
            }
            for piece in &link.pieces {
                if !(piece.y..piece.y + piece.height).contains(&middle) {
                    continue;
                }
                // Where it is in the page's pixels, cut to what the band
                // shows.
                let (mut left, mut right) = (piece.x, piece.x + piece.width);
                if let Some((_, x, visible, offset)) = shown {
                    left = (x + left - offset).max(x);
                    right = (x + right - offset).min(x + visible);
                }
                if right <= left {
                    continue;
                }
                let first = (left / cell_width - 0.5).ceil().max(0.0) as u16;
                let last = (right / cell_width - 0.5).ceil().max(0.0) as u16;
                let over: Vec<u16> = (first..last).collect();
                if over.is_empty() {
                    let at = ((left + right) / 2.0 / cell_width).floor() as u16;
                    claims.push((at, false, number));
                }
                claims.extend(over.into_iter().map(|at| (at, true, number)));
            }
        }
        // A cell goes to the link over its centre, and then to the earlier.
        let mut owners: Vec<(u16, usize)> = Vec::new();
        claims.sort_by_key(|&(at, over, number)| (at, !over, number));
        for (at, _, number) in claims {
            if at < self.columns && owners.last().is_none_or(|&(last, _)| last != at) {
                owners.push((at, number));
            }
        }
        let mut found: Vec<RowLink> = Vec::new();
        for (at, number) in owners {
            match found.last_mut() {
                Some(last) if last.link.1 == number && last.columns.end == at => {
                    last.columns.end = at + 1;
                }
                _ => found.push(RowLink {
                    columns: at..at + 1,
                    url: Rc::clone(&self.page.links()[number].url),
                    link: (address, number),
                }),
            }
        }
        found
    }

    /// The page's size in cells and the size of a cell, in pixels.
    pub fn cells(&self) -> ((u16, u16), (u16, u16)) {
        ((self.columns, self.rows), self.cell)
    }

    /// Where, from the top of the page, band `band` starts and how far
    /// it scrolls: the pixels a hit in it is in start at its left edge.
    pub fn band_top(&self, band: u32) -> Option<f32> {
        self.page.bands.get(band as usize).map(|band| band.top)
    }

    /// An overlay of `marks`, each with whether it is the current one, and
    /// the selected link, in the page's colours for them; `None` when
    /// there is nothing to draw.
    pub fn overlay(
        &self,
        marks: Vec<(ItemRef, std::ops::Range<usize>, bool)>,
        link: Option<usize>,
    ) -> Option<Rc<Overlay>> {
        (!marks.is_empty() || link.is_some()).then(|| {
            let rows = (marks.iter())
                .map(|(item, range, _)| self.page.mark_rows(*item, range.clone()))
                .collect();
            Rc::new(Overlay {
                colors: self.marks,
                marks,
                link,
                rows,
                link_rows: link.map(|number| self.page.link_rows(number)),
            })
        })
    }
}

/// The colours a search's marks have in reading mode: those the theme
/// gives a match, as the text layout lays them over its text; where the
/// theme gives none, or there is no colour, the reverse, a box in the
/// text's colour with the text in the page's.
fn mark_colors(theme: &Theme, caps: &Capabilities, look: &Look) -> MarkColors {
    let dark = caps.dark_background != Some(false);
    let rgb_of = |color: Option<Color>| match color?.rgb_or_xterm(&caps.palette) {
        Color::Rgb(red, green, blue) => Some([red, green, blue]),
        Color::Palette(_) => None,
    };
    let page = (rgb_of(theme.background).or_else(|| rgb_of(caps.background))).unwrap_or(if dark {
        [0; 3]
    } else {
        [0xff; 3]
    });
    let reverse = (Some(page), Some(look.text));
    // `auto` is what a browser's find gives where the text layout has
    // reverse video: black on yellow, and black on orange for the current.
    let colors = |foreground, background, browser: (Brush, Brush)| match (foreground, background) {
        _ if !caps.color => reverse,
        (SearchColor::Auto, SearchColor::Auto) => (Some(browser.0), Some(browser.1)),
        (foreground, background) => {
            let pick = |color| match color {
                SearchColor::Color(color) => rgb_of(color),
                SearchColor::Auto => None,
            };
            (pick(foreground), pick(background))
        }
    };
    let search = &theme.search;
    MarkColors {
        found: colors(
            search.match_foreground,
            search.match_background,
            ([0, 0, 0], [0xff, 0xff, 0x00]),
        ),
        current: colors(
            search.current_foreground,
            search.current_background,
            ([0, 0, 0], [0xff, 0x96, 0x32]),
        ),
    }
}

/// The rows of one page of reading mode on `lines`, as the pager lays an
/// overlay over them.
pub struct PageRows {
    pub lines: std::ops::Range<usize>,
    pub parts: Rc<Parts>,
}

/// The pages on `lines`, in order, each the rows that share one page.
pub fn pages(lines: &[Line]) -> Vec<PageRows> {
    let mut pages: Vec<PageRows> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(ImageRow {
            page: Some(parts), ..
        }) = &line.image
        else {
            continue;
        };
        match pages.last_mut() {
            Some(last) if Rc::ptr_eq(&last.parts, parts) && last.lines.end == index => {
                last.lines.end = index + 1;
            }
            _ => pages.push(PageRows {
                lines: index..index + 1,
                parts: Rc::clone(parts),
            }),
        }
    }
    pages
}

/// Draws `overlay` over the rows `lines` of a page, or nothing over them
/// when it is `None`: their keys name what is drawn over each part by a
/// hash of the marks and the link in that part alone, so that a part with
/// none is the one it was, and what was drawn without it, or with another,
/// is not what is shown.
pub fn set_overlay(lines: &mut [Line], overlay: Option<&Rc<Overlay>>) {
    for line in lines {
        let Some(image) = &mut line.image else {
            continue;
        };
        // The pixel rows of the page the part of this row covers.
        let covered = |parts: &Parts, band: Option<u32>, slice: u32| {
            let height = f32::from(parts.cell.1);
            let from = (slice * u32::from(PART_ROWS)) as f32 * height;
            let top = band.and_then(|band| parts.band_top(band)).unwrap_or(0.0);
            let to = from + f32::from(PART_ROWS) * height;
            (top + from).floor() as u32..(top + to).ceil() as u32
        };
        let hash_of =
            |parts: Option<&Rc<Parts>>, band: Option<u32>, slice: u32| match (overlay, parts) {
                (Some(overlay), Some(parts)) => overlay.hash_over(covered(parts, band, slice)),
                _ => 0,
            };
        let key = match image.placement.key {
            ImageKey::Page {
                document,
                page,
                slice,
                ..
            } => ImageKey::Page {
                document,
                page,
                slice,
                overlay: hash_of(image.page.as_ref(), None, slice),
            },
            ImageKey::Band {
                document,
                page,
                band,
                slice,
                offset,
                selected,
                ..
            } => ImageKey::Band {
                document,
                page,
                band,
                slice,
                offset,
                selected,
                overlay: hash_of(image.page.as_ref(), Some(band), slice),
            },
            _ => continue,
        };
        let drawn = matches!(
            key,
            ImageKey::Page { overlay: hash, .. } | ImageKey::Band { overlay: hash, .. } if hash != 0
        );
        if key != image.placement.key {
            image.placement = Rc::new(Placement {
                key,
                ..(*image.placement).clone()
            });
        }
        image.overlay = overlay.filter(|_| drawn).cloned();
    }
}

impl Parts {
    /// The pixels of part `part`.
    pub fn draw(&self, part: u32, overlay: Option<&Overlay>) -> RgbaImage {
        let (width, height) = (u32::from(self.cell.0), u32::from(self.cell.1));
        let first = part * u32::from(PART_ROWS);
        let rows = (u32::from(self.rows) - first).min(u32::from(PART_ROWS));
        (self.page).draw_with(
            first * height,
            u32::from(self.columns) * width,
            rows * height,
            overlay,
        )
    }
}

impl Parts {
    /// The pixels of the rows of slice `slice` of band `band`, scrolled
    /// `offset` pixels, `selected` or not, as wide as the page.
    pub fn draw_band(
        &self,
        band: u32,
        slice: u32,
        offset: u32,
        selected: bool,
        overlay: Option<&Overlay>,
    ) -> RgbaImage {
        let cell = f32::from(self.cell.1);
        let total = (self.page.bands.get(band as usize)).map_or(0.0, |band| band.height);
        let from = (slice * u32::from(PART_ROWS)) as f32 * cell;
        let to = (from + f32::from(PART_ROWS) * cell).min(total);
        (self.page).draw_band_with(band as usize, offset as f32, selected, from..to, overlay)
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
/// in `theme`'s colours, with tables shaded as `stripes` says and front
/// matter shown as `front_matter` says, with the pictures in `images`, of the document
/// numbered `document` among those shown, and diagrams telling why they
/// failed when `inline_errors`.
pub fn lines(
    blocks: &[&Block],
    ids: &[BlockId],
    reading: &Reading,
    (theme, code, (stripes, front_matter)): (&Theme, &CodeColors, (bool, FrontMatterStyle)),
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
        cache: &reading.pictures,
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
            front_matter: match front_matter {
                FrontMatterStyle::Code => FrontMatterShown::Code,
                FrontMatterStyle::Table | FrontMatterStyle::Hide => FrontMatterShown::Table,
            },
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
        marks: mark_colors(theme, caps, &look),
    });
    let address = Rc::as_ptr(&parts) as usize;
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
        let (key, size, image_row, at) = match bands.iter().find(|(_, rows)| rows.contains(&row)) {
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
                    overlay: 0,
                };
                (
                    key,
                    size(slice_rows),
                    into % PART_ROWS,
                    (Some((*band, 0)), into),
                )
            }
            None => {
                let part = row / PART_ROWS;
                let part_rows = (rows - part * PART_ROWS).min(PART_ROWS);
                let key = ImageKey::Page {
                    document,
                    page: parts.id,
                    slice: u32::from(part),
                    overlay: 0,
                };
                (key, size(part_rows), row % PART_ROWS, (None, row))
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
        // blocks belong to none.
        let block = (placed.iter().zip(ids)).find(|(placed, _)| placed.rows.contains(&middle));
        let line = Line {
            lead: vec![lead.clone()],
            image: Some(ImageRow {
                placement,
                row: image_row,
                background: None,
                columns: 0..columns,
                text: None,
                page: Some(Rc::clone(&parts)),
                overlay: None,
                links: parts.row_links(address, at.0, at.1),
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
    /// The pictures of the last layout, which this one takes from.
    cache: &'a RefCell<PictureCache>,
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
            (Some(Ok(source)), _) => {
                let Some(size) = sizes::pixel_size(source.size(), wanted, self.cell, room) else {
                    return Picture::None;
                };
                let print = self.images.print(&key).unwrap_or(0);
                let (pixels, id) = (self.cache.borrow_mut())
                    .get((key.clone(), print, size), || source.render(size.0, size.1));
                Picture::Ready {
                    pixels,
                    id,
                    loading: None,
                    depth: 0.0,
                }
            }
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

/// `pixels` with an id from what they are.
fn hashed(pixels: RgbaImage) -> (RgbaImage, u64) {
    let mut hasher = DefaultHasher::new();
    pixels.dimensions().hash(&mut hasher);
    pixels.as_raw().hash(&mut hasher);
    let id = hasher.finish();
    (pixels, id)
}

/// `pixels` as a picture, with an id from what they are.
fn ready(pixels: RgbaImage, loading: Option<usize>) -> Picture {
    let (pixels, id) = hashed(pixels);
    Picture::Ready {
        pixels: Rc::new(pixels),
        id,
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
            overlay,
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
                    overlay,
                },
                ..(*image.placement).clone()
            });
            // The links in the band are where it shows them now.
            if let Some(parts) = &image.page {
                let row = slice * u32::from(PART_ROWS) + u32::from(image.row);
                let address = Rc::as_ptr(parts) as usize;
                image.links = parts.row_links(address, Some((band, offset)), row as u16);
            }
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
        table_style: match theme.table_style {
            TableStyle::Frame => page::TableStyle::Frame,
            TableStyle::Grid => page::TableStyle::Grid,
        },
        stripe: stripes.then(|| mix(text, page, STRIPE)),
        page,
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
                pictures: Default::default(),
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
                    page: Some(_),
                    ..
                }) => {
                    let (kind, slice) = match placement.key {
                        ImageKey::Page { slice, .. } => ("page", slice),
                        ImageKey::Band { band, .. } => ("band", band),
                        _ => panic!("a page's key: {placement:?}"),
                    };
                    let lead: String = line.lead.iter().map(|span| span.text.as_str()).collect();
                    let block = line.block.map_or("-".to_owned(), |_| "block".to_owned());
                    format!("{}{kind} {row}/{slice} {block}", lead.len())
                }
                _ => crate::render::style::text(&line.spans),
            })
            .collect()
    }

    #[test]
    fn a_document_of_text_code_and_an_alert_is_one_page() {
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
            let image = parts.draw_band(band, slice, 0, false, None);
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
        row.page.as_ref().expect("a page").draw(0, None)
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
        let greens = (first_part(&lines).pixels())
            .filter(|pixel| pixel.0 == [0, 255, 0, 255])
            .count();
        assert_eq!(greens, 0);
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

    /// The boxes of the two marks, the first of two "word"s as found and the
    /// second as the current one, drawn with `theme` on `caps`: how many
    /// pixels each box has of `colours`.
    fn mark_boxes(theme: Theme, caps: Capabilities, colours: [[u8; 3]; 2]) -> [usize; 2] {
        let inputs = Inputs {
            theme,
            ..inputs(caps)
        };
        let lines = layout::layout(&parse("A word here.\n\nAnother word.\n"), &inputs);
        let parts = Rc::clone(&pages(&lines)[0].parts);
        let hits = parts.find(&|text: &str| {
            text.match_indices("word")
                .map(|(at, found)| at..at + found.len())
                .collect()
        });
        let marks = vec![
            (hits[0].item, hits[0].range.clone(), false),
            (hits[1].item, hits[1].range.clone(), true),
        ];
        let drawn = parts.draw(0, parts.overlay(marks, None).as_deref());
        colours.map(|colour| {
            drawn
                .pixels()
                .filter(|pixel| pixel.0[..3] == colour && pixel.0[3] == 255)
                .count()
        })
    }

    #[test]
    fn a_theme_with_auto_search_colours_gets_the_browsers_find_colours() {
        // The other matches black on yellow, the current one black on
        // orange, where the text layout reads `auto` as reverse video.
        let [found, current] =
            mark_boxes(Theme::default(), kitty(80), [[255, 255, 0], [255, 150, 50]]);
        assert!(found > 200 && current > 200, "{found} {current}");
    }

    #[test]
    fn marks_in_the_themes_own_colours_are_those_and_without_colour_the_reverse() {
        // The paper themes' own: the others on their grey, the current in
        // the text colour.
        let light = Theme::paper(crate::theme::Paper::Light);
        let search = light.search;
        let rgb = |color: crate::theme::SearchColor| match color {
            crate::theme::SearchColor::Color(Some(crate::color::Color::Rgb(r, g, b))) => [r, g, b],
            other => panic!("a colour: {other:?}"),
        };
        let colours = [rgb(search.match_background), rgb(search.current_background)];
        let [found, current] = mark_boxes(light, kitty(80), colours);
        assert!(found > 200 && current > 200, "{found} {current}");
        // A colour the theme sets is used.
        let mut custom = Theme::default();
        custom.search.match_background =
            crate::theme::SearchColor::Color(Some(crate::color::Color::Rgb(1, 2, 3)));
        let [found, _] = mark_boxes(custom, kitty(80), [[1, 2, 3], [255, 150, 50]]);
        assert!(found > 200, "{found}");
        // Without colour: a box in the text's colour.
        let plain = Capabilities {
            color: false,
            ..kitty(80)
        };
        let text = look(&Theme::default(), &CodeColors::default(), true, &plain).text;
        let [found, current] = mark_boxes(Theme::default(), plain, [text, text]);
        assert!(found > 400 && current == found, "{found} {current}");
    }

    #[test]
    fn an_overlay_changes_the_key_of_the_rows_it_is_drawn_on_and_what_is_drawn() {
        let mut lines = layout::layout(
            &parse("A word here.\n\nAnother word.\n"),
            &inputs(kitty(80)),
        );
        let pages = pages(&lines);
        let [page] = &pages[..] else {
            panic!("one page");
        };
        let parts = Rc::clone(&page.parts);
        let hits = parts.find(&|text: &str| {
            text.match_indices("word")
                .map(|(at, found)| at..at + found.len())
                .collect()
        });
        assert_eq!(hits.len(), 2);
        let marks = vec![(hits[0].item, hits[0].range.clone(), true)];
        let overlay = parts.overlay(marks, None).expect("an overlay");
        let key = |lines: &[Line]| lines[0].image.as_ref().unwrap().placement.key.clone();
        let before = key(&lines);
        set_overlay(&mut lines[page.lines.clone()], Some(&overlay));
        assert_ne!(key(&lines), before);
        // The same overlay is the same key, and none is the key it was.
        let marked = key(&lines);
        set_overlay(&mut lines, Some(&overlay));
        assert_eq!(key(&lines), marked);
        set_overlay(&mut lines, None);
        assert_eq!(key(&lines), before);
        assert!(parts.overlay(Vec::new(), None).is_none());
        // The default theme's `auto` is the browsers' orange for the
        // current match.
        let plain = parts.draw(0, None);
        let drawn = parts.draw(0, Some(&overlay));
        assert_ne!(plain, drawn);
        let boxed = (drawn.pixels())
            .filter(|pixel| pixel.0 == [0xff, 0x96, 0x32, 255])
            .count();
        assert!(boxed > 200, "{boxed}");
    }

    /// The links over the rows of the lines of `source` laid out in a
    /// window of 80 cells of 10 × 20 pixels: each row's columns and urls.
    fn row_links(source: &str, caps: Capabilities) -> Vec<Vec<(std::ops::Range<u16>, String)>> {
        let lines = layout::layout(&parse(source), &inputs(caps));
        (lines.iter())
            .filter_map(|line| line.image.as_ref())
            .map(|image| {
                (image.links.iter())
                    .map(|link| (link.columns.clone(), link.url.to_string()))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_link_in_a_page_is_a_hyperlink_over_the_cells_its_text_is_over() {
        let rows = row_links("Go [here](https://e.example/) now.\n\nPlain.\n", kitty(80));
        let [(columns, url)] = &rows[0][..] else {
            panic!("one link on the first row: {rows:?}");
        };
        assert_eq!(url, "https://e.example/");
        // "Go " is about 24 pixels, "here" about 33: cells 2 or 3 to 5 or
        // 6 of 10 pixels, covering the word and not much past it.
        assert!((2..=3).contains(&columns.start), "{columns:?}");
        assert!((5..=7).contains(&columns.end), "{columns:?}");
        assert!(rows[1].is_empty() && rows[2].is_empty());
        // A terminal without hyperlinks has the cells too, to select a link
        // by; the writer leaves them out.
        let rows = row_links(
            "Go [here](https://e.example/) now.\n",
            Capabilities {
                hyperlinks: false,
                ..kitty(80)
            },
        );
        assert_eq!(rows[0].len(), 1);
    }

    #[test]
    fn a_linked_picture_in_a_line_is_a_hyperlink_over_the_cells_it_is_over() {
        let mut inputs = inputs(kitty(80));
        inputs
            .images
            .insert("a.png", Ok(raster(60, 10, [255, 0, 0])));
        let lines = layout::layout(
            &parse("Some [![a](a.png)](https://badge.example/) more.\n"),
            &inputs,
        );
        let links = &lines[0].image.as_ref().unwrap().links;
        let [link] = &links[..] else {
            panic!("one link on the first row: {links:?}");
        };
        assert_eq!(&*link.url, "https://badge.example/");
        // The picture is 60 pixels, 6 cells of 10, which the link covers.
        assert!(
            link.columns.len() >= 5 && link.columns.len() <= 7,
            "{:?}",
            link.columns
        );
    }

    #[test]
    fn a_linked_picture_alone_on_its_line_is_a_hyperlink_over_its_cells() {
        let mut inputs = inputs(kitty(80));
        inputs
            .images
            .insert("a.png", Ok(raster(60, 40, [255, 0, 0])));
        let lines = layout::layout(&parse("[![a](a.png)](https://badge.example/)\n"), &inputs);
        // Over the cells of both rows of the picture, as wide as it is.
        for (row, line) in lines.iter().take(2).enumerate() {
            let links = &line.image.as_ref().unwrap().links;
            let [link] = &links[..] else {
                panic!("one link on row {row}: {links:?}");
            };
            assert_eq!(&*link.url, "https://badge.example/");
            assert_eq!(link.columns, 0..6);
        }
    }

    #[test]
    fn a_link_broken_over_two_lines_is_over_two_rows() {
        let words = "word ".repeat(30);
        let rows = row_links(&format!("[{words}](https://e.example/)\n"), kitty(80));
        let linked: Vec<_> = rows.iter().filter(|row| !row.is_empty()).collect();
        assert!(linked.len() >= 2, "{rows:?}");
        assert!(linked.iter().all(|row| row.len() == 1));
    }

    #[test]
    fn where_two_links_want_a_cell_the_one_over_its_centre_has_it_and_then_the_earlier() {
        // Two narrow letters in two links, side by side in one cell of 10
        // pixels: the cell's centre is over the second, so it has the cell
        // and the first, which has nothing over its own centre, has none.
        let rows = row_links(
            "[i](https://one.example/)[l](https://two.example/)\n",
            kitty(80),
        );
        assert_eq!(
            rows[0],
            [(0..1, "https://two.example/".to_owned())],
            "{rows:?}"
        );
        // Two links that are both only over their middle cell, in the same
        // one: the earlier has it.
        let rows = row_links(
            "[.](https://one.example/)[.](https://two.example/)\n",
            kitty(80),
        );
        assert_eq!(rows[0].len(), 1, "{rows:?}");
        assert_eq!(rows[0][0].1, "https://one.example/");
    }

    #[test]
    fn the_links_in_a_band_move_with_the_band() {
        let long = "x".repeat(150);
        let source = format!("| a |\n|---|\n| {long}[far](https://far.example/) |\n");
        let document = parse(&source);
        let mut inputs = inputs(kitty(80));
        if let Some(reading) = &mut inputs.reading {
            reading.wide = Wide::Scroll;
        }
        let mut lines = layout::layout(&document, &inputs);
        let at = |lines: &[Line]| -> Vec<usize> {
            (lines.iter().enumerate())
                .filter(|(_, line)| {
                    line.image
                        .as_ref()
                        .is_some_and(|image| !image.links.is_empty())
                })
                .map(|(index, _)| index)
                .collect()
        };
        // Out of sight at the start of the band; in sight when it is
        // scrolled to its end, over the cells that show the word.
        assert!(at(&lines).is_empty());
        let band = &bands(&lines)[0];
        let (rows, most) = (band.lines.clone(), band.most());
        set_band(&mut lines[rows], most, true);
        let shown = at(&lines);
        assert!(!shown.is_empty());
        let link = &lines[shown[0]].image.as_ref().unwrap().links[0];
        assert_eq!(&*link.url, "https://far.example/");
        assert!(link.columns.end <= 80);
    }

    /// The cache of `inputs`.
    fn cache(inputs: &Inputs) -> Rc<RefCell<PictureCache>> {
        Rc::clone(&inputs.reading.as_ref().expect("reading mode").pictures)
    }

    #[test]
    fn a_picture_is_scaled_once_for_layouts_that_show_it_alike() {
        let mut inputs = inputs(kitty(80));
        inputs
            .images
            .insert("a.png", Ok(raster(40, 20, [255, 0, 0])));
        let document = parse("Text.\n\n![a](a.png)\n");
        let first = layout::layout(&document, &inputs);
        let cache = cache(&inputs);
        assert_eq!(cache.borrow().scaled(), 1);
        let second = layout::layout(&document, &inputs);
        assert_eq!(cache.borrow().scaled(), 1);
        assert_eq!(cache.borrow().len(), 1);
        // The same pictures: the same page.
        assert_eq!(page_id(&first), page_id(&second));
        // Other pixels under the same key are scaled anew, and the old ones
        // go; so does a picture the next layout does not show.
        inputs
            .images
            .insert("a.png", Ok(raster(40, 20, [0, 0, 255])));
        layout::layout(&document, &inputs);
        assert_eq!(cache.borrow().scaled(), 2);
        assert_eq!(cache.borrow().len(), 1);
        layout::layout(&parse("Nothing.\n"), &inputs);
        assert!(cache.borrow().is_empty());
    }

    #[test]
    fn printing_files_one_by_one_holds_the_pictures_of_the_last_only() {
        let mut inputs = inputs(kitty(80));
        let cache = cache(&inputs);
        for number in 0..4 {
            inputs.images.insert(
                ImageKey::from("a.png").in_document(number),
                Ok(raster(200, 100, [255, 0, 0])),
            );
            let file = crate::model::File {
                name: format!("{number}.md"),
                document: parse("![a](a.png)\n"),
            };
            layout::file(&file, number, &inputs);
            assert!(
                cache.borrow().len() <= 1,
                "{} after file {number}",
                cache.borrow().len()
            );
        }
    }

    #[test]
    fn a_picture_shown_at_another_size_is_another_entry_and_the_old_size_goes() {
        let mut inputs = inputs(kitty(80));
        inputs
            .images
            .insert("a.png", Ok(raster(1000, 20, [255, 0, 0])));
        let document = parse("![a](a.png)\n");
        layout::layout(&document, &inputs);
        let cache = cache(&inputs);
        // A window of another width shrinks it to another column.
        inputs.caps.width = 60;
        layout::layout(&document, &inputs);
        assert_eq!(cache.borrow().scaled(), 2);
        assert_eq!(cache.borrow().len(), 1);
    }

    #[test]
    fn moving_the_current_match_redraws_only_the_parts_it_leaves_and_enters() {
        let document = parse(&"A word here.\n\n".repeat(120));
        let mut lines = layout::layout(&document, &inputs(kitty(80)));
        let page = &pages(&lines)[0];
        let (parts, rows) = (Rc::clone(&page.parts), page.lines.clone());
        let hits = parts.find(&|text: &str| {
            text.match_indices("word")
                .map(|(at, found)| at..at + found.len())
                .collect()
        });
        let count = hits.len();
        assert_eq!(count, 120);
        // Every match marked, the current one the first or the last.
        let overlay = |current: usize| {
            let marks = (hits.iter().enumerate())
                .map(|(number, hit)| (hit.item, hit.range.clone(), number == current))
                .collect();
            parts.overlay(marks, None)
        };
        let keys = |lines: &[Line]| -> Vec<_> {
            (lines[rows.clone()].iter())
                .step_by(30)
                .map(|line| line.image.as_ref().unwrap().placement.key.clone())
                .collect()
        };
        set_overlay(&mut lines[rows.clone()], overlay(0).as_ref());
        let first = keys(&lines);
        set_overlay(&mut lines[rows.clone()], overlay(count - 1).as_ref());
        let last = keys(&lines);
        assert!(first.len() >= 4, "{} parts", first.len());
        let changed = first.iter().zip(&last).filter(|(a, b)| a != b).count();
        // The part the current match leaves and the one it enters.
        assert_eq!(changed, 2, "{first:?} {last:?}");
    }

    #[test]
    fn the_look_has_the_themes_table_style() {
        let style =
            |theme: &Theme| look(theme, &CodeColors::default(), true, &kitty(80)).table_style;
        assert_eq!(style(&Theme::default()), page::TableStyle::Frame);
        let grid = Theme {
            table_style: TableStyle::Grid,
            ..Theme::default()
        };
        assert_eq!(style(&grid), page::TableStyle::Grid);
    }

    #[test]
    fn an_image_that_is_not_there_is_its_alt_text() {
        let lines = layout::layout(&parse("![the alt text](a.png)\n"), &inputs(kitty(80)));
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
    fn the_blocks_not_in_the_page_take_the_column_from_the_area_s_edge() {
        let note = "A note long enough to wrap. ".repeat(8);
        let mut document = parse("Text.\n");
        document.blocks.push(Block::Error(note));
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
