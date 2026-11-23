//! Reading mode's pages shown in the terminal: each page is cut into parts
//! of whole rows, each part an image drawn when a line first shows it, and
//! each row of the page a line that shows one row of a part through
//! kitty's Unicode placeholders.
//!
//! This file is the typesetting engine's page driver for reading mode: it
//! turns the pieces that reading mode's pen (`reading::pen`) holds into pages
//! from `reading::page`, keeps the cache of drawn parts, and gives the rows of
//! a page as styled lines. What the pager does with those lines belongs to the state engine.
//!
//! The layout of a page does not belong here, nor the painting of text,
//! nor anything about escape sequences, files or fonts as I/O. The pixel
//! layout is `src/typeset/reading/page.rs`, and a block kind of reading mode
//! is a file of its own beside it.
//!
//! Page layout and painting go to the typesetting engine's reading module
//! (`src/typeset/reading/`), and bytes to the terminal layer
//! (`src/present/`). The placement map in docs/architecture.md has the
//! paths.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use image::RgbaImage;

use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::images::ImageKey;
use crate::typeset::images::size::{Placement, Shows, Size};
use crate::typeset::layout::expand_tabs;
use crate::typeset::layout::{self, highlight};
use crate::typeset::model::Block;
use crate::typeset::reading::Fonts;
use crate::typeset::reading::gallery::Gallery;
use crate::typeset::reading::glyph_cache::GlyphCache;
use crate::typeset::reading::look::mark_colors;
use crate::typeset::reading::page::{
    self, Frame, Highlight, ItemRef, LinkPlace, MarkColors, Overlay, Page, Plain, Wide,
};
use crate::typeset::reading::pen::Setting;
use crate::typeset::reading::pixels::Pixels;
use crate::typeset::reading::places::{self, Ids, Shown};
use crate::typeset::reading::text::{Brush, Run};
use crate::typeset::style::{Band, ImageRow, Line, Span, Style};

/// The most rows a picture is shown at, as many as an image can cover.
pub(super) const MAX_ROWS: u16 = 297;
/// The rows of one part of a page, the image drawn at a time.
pub(super) const PART_ROWS: u16 = 30;

/// What reading mode needs besides the document: the fonts it lays text
/// out in.
#[derive(Debug, Clone)]
pub struct Reading {
    pub fonts: Rc<Fonts>,
    /// What becomes of code blocks and tables wider than the area.
    pub wide: Wide,
    /// Whether headings start on whole rows, as `snap_to_rows` says.
    pub snap: bool,
    /// How large the text and the pictures are, in whole percent.
    pub zoom: u16,
    /// The pictures of the last layout, scaled.
    pub pictures: Rc<RefCell<PictureCache>>,
    /// The glyphs drawn, which every part drawn shares.
    pub glyphs: Rc<RefCell<GlyphCache>>,
}

/// Images scaled to the size a layout shows them at, kept from one layout
/// to the next so that a layout does not scale every picture again. Only
/// what the last layout used stays.
#[derive(Debug, Default)]
pub struct PictureCache {
    entries: HashMap<PictureKey, (Rc<Pixels>, u64)>,
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
            .map(|(pixels, _)| pixels.bytes())
            .sum()
    }

    /// Ends a layout: what it did not use is let go.
    pub fn finish(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.entries.retain(|key, _| used.contains(key));
    }

    /// The picture under `key`, scaled by `scale` when it is not held.
    pub(super) fn get(
        &mut self,
        key: PictureKey,
        scale: impl FnOnce() -> RgbaImage,
    ) -> (Rc<Pixels>, u64) {
        self.used.insert(key.clone());
        if let Some((pixels, id)) = self.entries.get(&key) {
            return (Rc::clone(pixels), *id);
        }
        self.scaled += 1;
        let (pixels, id) = Pixels::with_id(scale());
        let pixels = Rc::new(pixels);
        self.entries.insert(key, (Rc::clone(&pixels), id));
        (pixels, id)
    }
}

/// A page cut into parts, which draw themselves when they are first
/// shown.
pub struct Parts {
    /// What the page shows and how, hashed: two layouts of the same text
    /// at the same size share their parts, so the terminal keeps them.
    pub id: u64,
    pub(super) page: Page,
    pub(super) cell: (u16, u16),
    pub(super) columns: u16,
    pub(super) rows: u16,
    /// The columns each part is drawn at and covers, up to `columns`.
    pub(super) part_columns: Vec<u16>,
    /// The blocks of the page in the layout's table.
    pub(super) ids: Vec<Ids>,
    /// The colours of a search's marks on it.
    marks: MarkColors,
    /// The pieces of the page's links by row.
    pub(super) link_rows: places::LinkRows,
    /// The glyphs of every page drawn, which the parts draw through.
    pub(super) glyphs: Rc<RefCell<GlyphCache>>,
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
    /// The columns part `part` is drawn at and covers.
    pub(super) fn columns_of(&self, part: u32) -> u16 {
        (self.part_columns.get(part as usize)).map_or(self.columns, |&columns| columns)
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
        let columns = match band {
            Some(_) => self.columns,
            None => self.columns_of(u32::from(row / PART_ROWS)),
        };
        let mut claims: Vec<(u16, bool, usize)> = Vec::new();
        for &(number, at) in self.link_candidates(shown.map(|(band, ..)| band), row) {
            let piece = &self.page.links()[number].pieces[at];
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
        // A cell goes to the link over its centre, and then to the earlier.
        let mut owners: Vec<(u16, usize)> = Vec::new();
        claims.sort_by_key(|&(at, over, number)| (at, !over, number));
        for (at, _, number) in claims {
            if at < columns && owners.last().is_none_or(|&(last, _)| last != at) {
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
        }) = line.image.as_deref()
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

/// A page's lines, and what the page leaves to the next one.
pub struct Cut {
    pub lines: Vec<Line>,
    /// How many of the blocks the page shows.
    pub taken: usize,
    /// The bottom margin of the last of them, which the next page starts
    /// with.
    pub margin: Option<f32>,
}

/// The lines that show `blocks`, which reading mode lays out, marked with
/// their `ids`, as `setting` says: the window, the colours, the sizes and
/// the pictures.
#[expect(clippy::too_many_lines)]
pub fn lines(blocks: &[&Block], ids: &[Ids], reading: &Reading, setting: &Setting) -> Cut {
    let Setting {
        inputs,
        cell,
        look,
        area,
        columns,
        ..
    } = setting;
    let (one, code, images, caps) = (inputs.look, &inputs.code, inputs.images, inputs.caps);
    let (inline_errors, (document, diagrams)) = (inputs.inline_errors, inputs.shown);
    let (cell, columns, text, left) = (*cell, *columns, area.text, area.left);
    let (cell_width, cell_height) = (f32::from(cell.0), f32::from(cell.1));
    let highlight = Syntect {
        theme: code.theme.as_deref(),
        caps,
    };
    let gallery = Gallery {
        images,
        document,
        diagrams,
        cell,
        inline_errors,
        keys: RefCell::new(Vec::new()),
        reading,
    };
    let mut page = page::lay_out(
        blocks,
        look,
        &reading.fonts,
        (&highlight, &gallery),
        Frame::of(setting, reading.wide, reading.snap),
    );
    // The page shows the blocks it took, and nothing it did not.
    let (taken, margin) = (page.blocks.len(), page.margin);
    let (blocks, ids) = (&blocks[..taken], &ids[..taken.min(ids.len())]);
    super::animated::show_first_frames(&mut page, images);
    let rows = ((page.height / cell_height).ceil() as u16).max(1);
    // The id names these pixels to the terminal, which keeps an image it
    // holds under the same id, so it must cover everything that changes
    // them: the blocks, the look (every colour, and the font size), the
    // code theme (its tokens' colours can differ where the block's do not),
    // the sizes, whether wide blocks wrap or scroll, whether headings
    // snap to rows, the margin the page goes on with, the pictures, by
    // their pixels, and the fonts, by their fingerprint. An input to the
    // pixels that is not here yet goes in here too.
    let mut hasher = DefaultHasher::new();
    blocks.hash(&mut hasher);
    reading.fonts.fingerprint().hash(&mut hasher);
    format!("{look:?}").hash(&mut hasher);
    (
        text.to_bits(),
        cell,
        columns,
        reading.wide,
        reading.snap,
        &code.theme,
    )
        .hash(&mut hasher);
    page.pictures.hash(&mut hasher);
    setting.margin.map(f32::to_bits).hash(&mut hasher);
    let loading = page.loading.clone();
    let writer = one.background.filter(|_| caps.color);
    let part_columns = page.part_columns((cell, columns, rows), writer, &caps.palette);
    let link_rows = places::LinkRows::new(&page, rows, cell_height);
    let parts = Rc::new(Parts {
        id: hasher.finish(),
        page,
        cell,
        columns,
        rows,
        part_columns,
        marks: mark_colors(one, caps, look),
        ids: ids.to_vec(),
        link_rows,
        glyphs: Rc::clone(&reading.glyphs),
    });
    super::animated::note_stills(&parts, images);
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
    let size = |columns: u16, rows: u16| Size {
        width: u32::from(columns) * u32::from(cell.0),
        height: u32::from(rows) * u32::from(cell.1),
        columns,
        rows,
    };
    let row_blocks = places::row_blocks(&parts, ids, rows);
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
                    size(columns, slice_rows),
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
                let wide = parts.columns_of(u32::from(part));
                (key, size(wide, part_rows), row % PART_ROWS, (None, row))
            }
        };
        let placement = Rc::new(Placement::block(key, size, Shows::Image));
        let shown = Shown {
            placement,
            row: image_row,
            at,
        };
        lines.push(places::line(&parts, ids, row, &row_blocks, &lead, shown));
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
        let columns = ((loading.width / cell_width).ceil() as u16).clamp(1, caps.max_image_cells());
        let indent = (loading.x / cell_width).floor() as usize;
        let band = Band {
            columns,
            indent,
            low: true,
        };
        lines[last].loading = Some(layout::loading(caps, key.clone(), last - first, band).into());
    }
    Cut {
        lines,
        taken,
        margin,
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

/// The bands that `lines` show, in order.
pub fn bands(lines: &[Line]) -> Vec<BandRows> {
    let mut bands: Vec<BandRows> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(ImageRow {
            placement,
            page: Some(parts),
            ..
        }) = line.image.as_deref()
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

#[cfg(test)]
mod tests;
