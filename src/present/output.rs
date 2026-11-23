//! Styled lines to bytes: SGR attributes, OSC 8 hyperlinks, cursor movement,
//! scaled text through `kitty::text`, and images through `encoder`, which
//! sends them, and the placeholders of `kitty::graphics`, which show them.

use std::io::{self, Write};
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use crate::present::encoder::{Encoder, Inset, Near};
use crate::present::kitty;
use crate::present::kitty::graphics;
use crate::present::sixel::{self, Rgb};
use crate::typeset::caps::{Capabilities, Effort, Graphics};
use crate::typeset::color::{Color, Underline};
use crate::typeset::images::Images;
use crate::typeset::images::size::cell_size;
use crate::typeset::loading::{BAND_HEIGHT, Ink};
use crate::typeset::reading::engine::RowLink;
use crate::typeset::size::TextSize;
use crate::typeset::style::{Embed, ImageRow, Line, Span, Style};
use crate::typeset::theme::Theme;
use crate::typeset::wrap::width;

/// How the image of a row is drawn: kitty's placeholders of an image row,
/// or the sixels of the row.
enum Drawn {
    /// The id of the image, the row of it, and the inset over some of its
    /// cells.
    Kitty(graphics::ImageId, u16, Option<Inset>),
    Sixel(Rc<Vec<sixel::Tile>>),
}

/// The colours of the page the document is drawn on. `None` leaves the
/// terminal's own showing through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Page {
    /// Painted across the whole terminal width behind the document.
    pub background: Option<Color>,
    /// For text that has no colour of its own.
    pub foreground: Option<Color>,
}

impl Page {
    /// The page of `theme`: its background and text colours.
    pub fn of(theme: &Theme) -> Self {
        Page {
            background: theme.background,
            foreground: theme.foreground,
        }
    }
}

/// What a writer brings to the whole screen beyond the bytes of its rows:
/// the background a clear paints every row in, rows with no line too, and
/// the cell size its images are placed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Canvas {
    pub background: Option<Color>,
    pub cell: Option<(u16, u16)>,
}

/// Moves the cursor to `row` and `column`, counted from 1 at the top left.
pub fn move_to(row: usize, column: usize, out: &mut impl Write) -> io::Result<()> {
    write!(out, "\x1b[{row};{column}H")
}

/// Starts an update that the terminal shows at once at its end, through
/// synchronized output. Terminals without it ignore it.
pub fn begin_update(out: &mut impl Write) -> io::Result<()> {
    out.write_all(b"\x1b[?2026h")
}

/// Scrolls the whole screen by `rows`: up when positive, so that the row
/// below the top one is then the top, and down when negative. Rows that
/// scroll in are blank. It uses no scroll region and leaves the cursor.
pub fn scroll(rows: isize, out: &mut impl Write) -> io::Result<()> {
    match rows {
        0 => Ok(()),
        up if up > 0 => write!(out, "\x1b[{up}S"),
        down => write!(out, "\x1b[{}T", down.unsigned_abs()),
    }
}

/// Ends the update [`begin_update`] started.
pub fn end_update(out: &mut impl Write) -> io::Result<()> {
    out.write_all(b"\x1b[?2026l")
}

/// Writes `lines` on `page`. `images` holds the pixels of the images the
/// lines show; each is sent right before the cells of its first row. Returns what the
/// terminal holds of them, to check that they came.
pub fn write(
    lines: &[Line],
    images: &Images,
    caps: &Capabilities,
    page: Page,
    out: &mut impl Write,
) -> io::Result<Encoder> {
    let mut encoder = Encoder::new();
    encoder.play(caps.playback());
    let mut writer = Writer::new(images, caps, page, &mut encoder);
    // Printed rows stay in the terminal's scrollback, which a narrower
    // window wraps anew: a run of short placeholders would then go on at
    // the start of a line, with no cell on its left to take its position
    // from.
    writer.whole_placeholders(true);
    for line in lines {
        // A sixel drawn on the last row of the screen scrolls it, and the
        // saved cursor of the row's cells then lies on the row below the
        // sixel, which leaves a blank row. A newline and a step up first
        // scroll the screen if the cursor is on its last row, and leave it
        // on its row if not.
        let sixel = writer.sixel()
            && (line.image.is_some() || line.spans.iter().any(|s| s.embed.is_some()));
        for row in 0..writer.rows(line) {
            if sixel {
                out.write_all(b"\n\x1bM")?;
            }
            // What the terminal keeps comes right before the row's cells.
            let (mut sends, mut cells) = (Vec::new(), Vec::new());
            writer.row(line, row, &mut sends, &mut cells)?;
            out.write_all(&sends)?;
            out.write_all(&cells)?;
            out.write_all(b"\n")?;
        }
    }
    Ok(encoder)
}

/// The rows `line` occupies on a terminal with `caps`: as many as the
/// largest scale of its text, unless the terminal gets plain text.
pub fn rows(line: &Line, caps: &Capabilities) -> u8 {
    line.rows_on(caps)
}

/// The rows `lines` take on a terminal with `caps`.
pub fn total_rows(lines: &[Line], caps: &Capabilities) -> usize {
    lines.iter().map(|line| usize::from(rows(line, caps))).sum()
}

/// Writes lines row by row, wherever the cursor is, with the images sent
/// through an encoder.
pub struct Writer<'a> {
    images: &'a Images,
    caps: &'a Capabilities,
    background: Option<Color>,
    foreground: Option<Color>,
    /// Every line dimmed, as behind a panel.
    dim: bool,
    /// Placeholders written whole, each cell with its position: on a row a
    /// bar is drawn over, and in print mode.
    whole: bool,
    /// The colour of placeholders and of the bands below them.
    ink: Ink,
    /// How long the bands have moved, which tells where their light part
    /// is.
    elapsed: Duration,
    /// What the terminal holds of the images.
    encoder: &'a mut Encoder,
    /// Whether the rows written since it was last asked sent image data or
    /// the command that deletes a replaced image.
    sent: bool,
    /// The sends of the row being written, which [`Writer::row`] hands over
    /// apart from its cells.
    sends: Vec<u8>,
    /// Where the rows written now end, when they are in an area; `None`
    /// is the window's right edge.
    edge: Option<Edge>,
}

/// The right edge of an area a row is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    /// The cells across the area; a row starts at its first.
    pub columns: usize,
    /// Whether the area reaches the window's right edge, where a row is
    /// ended with `ESC [ K`. Elsewhere that would erase what is beside the
    /// area, and the row is ended by erasing the cells it has left.
    pub window: bool,
}

impl<'a> Writer<'a> {
    /// A writer for lines on `page` with the pixels in `images`, sent
    /// through `encoder`.
    pub fn new(
        images: &'a Images,
        caps: &'a Capabilities,
        page: Page,
        encoder: &'a mut Encoder,
    ) -> Self {
        Self {
            images,
            caps,
            background: page.background.filter(|_| caps.color),
            foreground: page.foreground.filter(|_| caps.color),
            dim: false,
            whole: false,
            ink: Ink::new(page.foreground, caps),
            elapsed: Duration::ZERO,
            encoder,
            sent: false,
            sends: Vec::new(),
            edge: None,
        }
    }

    /// The rows written from now on are in an area that ends at `edge`; with
    /// `None` they are on the whole window, as they are at first.
    pub fn edge(&mut self, edge: Option<Edge>) {
        self.edge = edge;
    }

    /// The cells a row has, from its first to the right edge it is written
    /// to.
    fn reach(&self) -> usize {
        // Without an area the window's right edge is what a row runs to;
        // `width` stands in for it, which differs only under `--width`.
        // Where a sixel's last column is then right is open: see
        // docs/roadmap.md, 5.1.2.
        self.edge
            .map_or(usize::from(self.caps.width), |edge| edge.columns)
    }

    /// Tells the terminal what `command` says of an animation.
    pub fn control(
        &mut self,
        command: &crate::present::encoder::Command,
        out: &mut impl Write,
    ) -> io::Result<()> {
        self.encoder.control(command, out)
    }

    /// What the frames of `picture` are made from, for each placement shown.
    pub fn frame_jobs(
        &self,
        picture: &crate::typeset::images::ImageKey,
    ) -> Vec<(
        crate::typeset::images::size::Placement,
        crate::typeset::images::build::FramesJob,
    )> {
        self.encoder.frame_jobs(picture, self.images, self.caps)
    }

    /// Takes the frames built for `picture` at `placement`.
    pub fn install_frames(
        &mut self,
        placement: crate::typeset::images::size::Placement,
        picture: crate::typeset::images::ImageKey,
        frames: Vec<image::RgbaImage>,
    ) {
        self.encoder.install_frames(placement, picture, frames);
    }

    /// Tells where each animation is in its life, which decides what is
    /// drawn in its picture.
    pub fn show_marks(
        &mut self,
        marks: std::collections::HashMap<
            crate::typeset::images::ImageKey,
            crate::typeset::images::marks::State,
        >,
    ) {
        self.encoder.show_marks(marks);
    }

    /// Has the animations that kiri plays show the frames `frames` tells.
    pub fn show_frames(
        &mut self,
        frames: std::collections::HashMap<crate::typeset::images::ImageKey, usize>,
    ) {
        self.encoder.show_frames(frames);
    }

    /// The animations sent with their frames since this was last asked.
    pub fn take_fresh(&mut self) -> Vec<crate::typeset::images::ImageKey> {
        self.encoder.take_fresh()
    }

    /// Dims the lines written from now on, or stops dimming them.
    pub fn dim(&mut self, dim: bool) {
        self.dim = dim;
    }

    /// Writes the placeholders of the rows from now on whole, or stops. A
    /// row that a bar is drawn over needs them whole, as the cells after
    /// the bar would otherwise take their position from its text; so do
    /// printed rows, which the terminal may wrap anew.
    pub fn whole_placeholders(&mut self, whole: bool) {
        self.whole = whole;
    }

    /// How placeholders are written: short on a terminal known to take a
    /// cell's position from the cell on its left, unless the row is to be
    /// whole.
    fn form(&self) -> graphics::Form {
        let terminal = match &self.caps.graphics {
            Graphics::Kitty(kitty) => kitty.terminal.as_deref(),
            _ => None,
        };
        match !self.whole && kitty::query::infers_positions(terminal) {
            true => graphics::Form::Short,
            false => graphics::Form::Whole,
        }
    }

    /// The rows `line` occupies.
    pub fn rows(&self, line: &Line) -> u8 {
        rows(line, self.caps)
    }

    /// The style that fills the rest of a row: the page's background.
    pub fn fill(&self) -> Style {
        Style {
            bg: self.background,
            ..Style::default()
        }
    }

    /// The canvas this writer draws on: the page's background as it is
    /// shown (none without colour) and the cell size of its capabilities.
    pub fn canvas(&self) -> Canvas {
        Canvas {
            background: self.background,
            cell: self.caps.cell_pixels,
        }
    }

    /// The state part of a clear: tells the encoder the screen is cleared,
    /// so that what is drawn in cells is sent anew. [`Writer::clear_bytes`]
    /// writes the clear itself; a frame calls this before it renders and
    /// that after it has written its sends.
    pub fn cleared(&mut self) {
        self.encoder.cleared();
    }

    /// Erases the whole screen in the page's background and leaves the
    /// cursor at the top left with the terminal's default style.
    pub fn clear_bytes(&self, out: &mut impl Write) -> io::Result<()> {
        let fill = self.fill();
        transition(&Style::default(), &fill, self.caps, out)?;
        out.write_all(b"\x1b[H\x1b[2J")?;
        transition(&fill, &Style::default(), self.caps, out)
    }

    /// Runs `send`, which gets the encoder and `out`, and notes whether it
    /// wrote any bytes to `out`, for [`Writer::sent_image`]. The bytes may be
    /// image data or the command that deletes an image that is replaced:
    /// both count, as a row that wrote either is written again. Every use of
    /// the encoder that may send an image while a row is written goes
    /// through here. [`Writer::send_again`] does not: it sends outside any
    /// row, and what it sends is no part of a row's bytes.
    fn sending<W: Write + ?Sized, T>(
        &mut self,
        out: &mut W,
        send: impl FnOnce(&mut Encoder, &mut Counted<'_, W>) -> io::Result<T>,
    ) -> io::Result<T> {
        let mut counted = Counted { out, bytes: 0 };
        let result = send(&mut *self.encoder, &mut counted)?;
        self.sent |= counted.bytes > 0;
        Ok(result)
    }

    /// As `sending`, for what the terminal keeps from frame to frame: the
    /// bytes go to the row's sends, not to its cells.
    fn sending_apart<T>(
        &mut self,
        send: impl FnOnce(&mut Encoder, &mut Counted<'_, Vec<u8>>) -> io::Result<T>,
    ) -> io::Result<T> {
        let mut sends = std::mem::take(&mut self.sends);
        let result = self.sending(&mut sends, send);
        self.sends = sends;
        result
    }

    /// Whether the pictures behind a panel are darkened by a shade placed
    /// over them: where the terminal places images as bands are.
    pub fn shades(&self) -> bool {
        self.caps.places_bands()
    }

    /// How hard an image is compressed on this terminal.
    pub fn effort(&self) -> Effort {
        self.caps.effort()
    }

    /// Writes the loading band over the row and places it there, sending
    /// its image first if it has not been sent.
    fn write_band(
        &mut self,
        band: &crate::typeset::style::Band,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let cell = cell_size(self.caps);
        let (ink, elapsed, effort) = (self.ink, self.elapsed, self.caps.effort());
        let id = self.sending(out, |encoder, out| {
            encoder.band(band.columns, cell, ink, elapsed, effort, out)
        })?;
        out.write_all(b"\x1b7")?;
        if band.indent > 0 {
            write!(out, "\x1b[{}C", band.indent)?;
        }
        graphics::place(out, id, band_offset(cell.1, band.low))?;
        out.write_all(b"\x1b8")
    }

    /// Writes row `row` of `line` in two parts: to `sends` what the terminal
    /// keeps from frame to frame, which is the data of the images the row
    /// shows if it does not hold them yet and the delete of one they
    /// replace, and to `cells` what is drawn on the screen: text,
    /// placeholders, sixels and a loading band, which stays whole among the
    /// cells as it is sent anew with each clear. The sends come before
    /// anything that erases the row: the caller decides where they go. The
    /// cells leave the cursor at the row's end with the terminal's default
    /// style.
    pub fn row(
        &mut self,
        line: &Line,
        row: u8,
        sends: &mut impl Write,
        cells: &mut impl Write,
    ) -> io::Result<()> {
        self.sends.clear();
        let result = self.cells(line, row, cells);
        let apart = std::mem::take(&mut self.sends);
        result?;
        sends.write_all(&apart)
    }

    /// The cells of `row`, with its sends collected in `self.sends`.
    fn cells(&mut self, line: &Line, row: u8, out: &mut impl Write) -> io::Result<()> {
        let caps = self.caps;
        if !caps.styling {
            let text: String = (line.lead.iter().chain(&line.spans).chain(&line.trail))
                .map(|span| span.text.as_str())
                .collect();
            return out.write_all(text.trim_end().as_bytes());
        }
        // Drawn over the row, from where it starts, and not behind a panel,
        // which it would be drawn over; the line holds a band only where
        // Capabilities::places_bands says bands are placed, and elsewhere
        // the status line tells the progress.
        let drawn = row == 0 && !self.dim;
        let band = line
            .loading
            .as_ref()
            .and_then(|loading| loading.band.as_ref());
        if let Some(band) = band.filter(|_| drawn) {
            debug_assert!(caps.places_bands(), "a band on a terminal that places none");
            self.write_band(band, out)?;
        }
        let image = match &line.image {
            Some(image) => Some((image, self.draw_image(image)?)),
            None => None,
        };
        let (background, foreground, dim) = (self.background, self.foreground, self.dim);
        let paint = |style: &Style| Style {
            fg: style.fg.or(foreground),
            bg: style.bg.or(background),
            dim: style.dim || dim,
            ..style.clone()
        };
        // A line with scaled text occupies as many rows as its largest
        // scale but is written only on the first; the rows below get the
        // lead, the trail and the fill around it.
        let block_width = width(&line.spans);
        let mut current = Style::default();
        // A sixel that reaches the last column: nothing is written after it,
        // as that would remove its last cell.
        let mut reaches_last = false;
        for (at, span) in line.lead.iter().enumerate() {
            let style = paint(&span.style);
            transition(&current, &style, caps, out)?;
            // What is for the first row only is blank below it.
            if row > 0 && line.lead_once.get(at) == Some(&true) {
                write!(out, "{}", " ".repeat(width(std::slice::from_ref(span))))?;
            } else {
                out.write_all(span.text.as_bytes())?;
            }
            current = style;
        }
        let lead_width = width(&line.lead);
        if row == 0 {
            let mut column = lead_width;
            for span in &line.spans {
                if let Some(embed) = &span.embed {
                    let at = Placed { row: 0, column };
                    reaches_last = self.embed_cells(span, embed, at, &paint, &mut current, out)?;
                    column += width(std::slice::from_ref(span));
                    if reaches_last {
                        break;
                    }
                    continue;
                }
                column += width(std::slice::from_ref(span));
                let style = paint(&span.style);
                transition(&current, &style, caps, out)?;
                if span.style.size != TextSize::NORMAL {
                    kitty::text::write_sized(out, span.style.size, &span.text)?;
                } else {
                    out.write_all(span.text.as_bytes())?;
                }
                current = style;
            }
            if let Some((image, drawn)) = image.filter(|_| !reaches_last) {
                reaches_last =
                    self.write_image_row(line, image, drawn, &paint, &mut current, out)?;
            }
        } else if line.spans.iter().any(|span| span.embed.is_some()) {
            let reach = background.is_some() || !line.trail.is_empty();
            reaches_last = self.embed_rows(
                &line.spans,
                Placed {
                    row,
                    column: lead_width,
                },
                reach,
                &paint,
                &mut current,
                out,
            )?;
        } else if (background.is_some() || !line.trail.is_empty()) && block_width > 0 {
            // Step over the scaled text to write the trail and fill the
            // rest of the row. `CSI 0 C` would still move one cell.
            write!(out, "\x1b[{block_width}C")?;
        }
        if reaches_last {
            return transition(&current, &Style::default(), caps, out);
        }
        write_spans(&line.trail, paint, &mut current, caps, out)?;
        if background.is_some() {
            let image = line.image.as_ref().filter(|_| row == 0);
            let image = image.map_or(0, |image| {
                usize::from(image.columns.end.saturating_sub(image.columns.start))
            });
            let used = lead_width + width(&line.spans) + width(&line.trail) + image;
            return self.fill_row(&current, used, out);
        }
        transition(&current, &Style::default(), caps, out)
    }
}

impl Writer<'_> {
    /// Ends a row of which `used` cells are written, the cursor after them
    /// and in the style `current`, in the page's background, and leaves the
    /// terminal's default style. At the window's right edge that is a clear
    /// to the end of the row. In an area that stops short of it, a clear
    /// would take the cells beside the area: the cells the area has left are
    /// erased, and no more.
    fn fill_row(&self, current: &Style, used: usize, out: &mut impl Write) -> io::Result<()> {
        let caps = self.caps;
        match self.edge.filter(|edge| !edge.window) {
            None => {
                let fill = self.fill();
                transition(current, &fill, caps, out)?;
                out.write_all(b"\x1b[K")?;
                transition(&fill, &Style::default(), caps, out)
            }
            Some(edge) => {
                transition(current, &Style::default(), caps, out)?;
                self.erase_chars(edge.columns.saturating_sub(used), out)
            }
        }
    }
}

impl Writer<'_> {
    /// Gets what `image`, one row of an image shown as a block, is drawn
    /// with: for kitty its id, with the image in the row's sends if the
    /// terminal does not hold it, for sixel the sixels of the row.
    fn draw_image(&mut self, image: &ImageRow) -> io::Result<Drawn> {
        let ImageRow {
            placement,
            row,
            background,
            columns,
            page,
            overlay,
            ..
        } = image;
        if let Some(page) = page {
            (self.encoder).page(&placement.key, page, overlay.as_ref());
        }
        let (images, caps) = (self.images, self.caps);
        if self.sixel() {
            let under = self.shown(*background);
            let tiles = (self.encoder).sixel_row(placement, *row, columns, under, images, caps)?;
            return Ok(Drawn::Sixel(tiles));
        }
        let (id, inset) =
            self.sending_apart(|encoder, out| encoder.part_id(placement, images, caps, out))?;
        Ok(Drawn::Kitty(id, *row, inset))
    }

    /// Writes the image of a row, `drawn`, after the text of `line` in the
    /// cells of `image.columns`. Returns whether a sixel reaches the last
    /// column of the window, after which nothing may be written.
    fn write_image_row(
        &self,
        line: &Line,
        image: &ImageRow,
        drawn: Drawn,
        paint: &impl Fn(&Style) -> Style,
        current: &mut Style,
        out: &mut impl Write,
    ) -> io::Result<bool> {
        let caps = self.caps;
        // Placeholders are neither underlined nor links, and their
        // foreground colour is the low 24 bits of the image id. Their
        // background shows where the image is transparent.
        let plain = paint(&Style {
            bg: image.background,
            ..Style::default()
        });
        transition(current, &plain, caps, out)?;
        // The links of a page are hyperlinks over its placeholders. Without
        // hyperlinks the cells are for selecting a link only.
        let links = if caps.hyperlinks {
            &image.links[..]
        } else {
            &[]
        };
        let columns = image.columns.clone();
        let runs = clip(links, &columns);
        match drawn {
            Drawn::Kitty(id, row, inset) => {
                let form = self.form();
                match inset.filter(|inset| inset.rows.contains(&row)) {
                    None => graphics::write_row_linked(out, id, row, columns, &runs, form)?,
                    Some(inset) => {
                        // A part of a page is written whole, so the inset's
                        // columns are the part's.
                        debug_assert_eq!(columns.start, 0, "a part written whole");
                        let at = inset.columns.clone();
                        let pieces = [
                            (id, row, columns.start..at.start.max(columns.start), 0),
                            (inset.id, row - inset.rows.start, at.clone(), at.start),
                            (id, row, at.end.min(columns.end)..columns.end, 0),
                        ];
                        // From left to right, each naming its first cell and
                        // setting its colour, with the links over it.
                        for (id, row, written, from) in pieces {
                            if written.is_empty() {
                                continue;
                            }
                            let runs = clip(links, &written);
                            let cells = written.start - from..written.end - from;
                            graphics::write_row_linked(out, id, row, cells, &runs, form)?;
                        }
                    }
                }
                // The placeholders leave the terminal's foreground.
                *current = Style { fg: None, ..plain };
                Ok(false)
            }
            Drawn::Sixel(tiles) => {
                // The sixel is drawn over blank cells in the style of the
                // row; the links are on those cells.
                let cells = columns.end.saturating_sub(columns.start);
                let start = width(&line.lead) + width(&line.spans);
                let reaches_last = start + usize::from(cells) >= self.reach();
                let links: Vec<_> = (runs.iter())
                    .map(|run| sixel::Link {
                        columns: run.columns.clone(),
                        url: run.url,
                    })
                    .collect();
                sixel::write_cells(out, cells, &tiles, &links, reaches_last)?;
                *current = plain;
                Ok(reaches_last)
            }
        }
    }

    /// Writes row `row`, below the first, of a line of `spans` with images
    /// in them: steps over each text span and writes the next row of each
    /// image in its cells, and with `reach`, steps on to the end of the
    /// spans.
    fn embed_rows(
        &mut self,
        spans: &[Span],
        at: Placed,
        reach: bool,
        paint: &impl Fn(&Style) -> Style,
        current: &mut Style,
        out: &mut impl Write,
    ) -> io::Result<bool> {
        let mut step = 0;
        let Placed { row, mut column } = at;
        for span in spans {
            let cells = width(std::slice::from_ref(span));
            match span
                .embed
                .as_ref()
                .filter(|embed| u16::from(row) < embed.rows)
            {
                Some(embed) => {
                    if step > 0 {
                        write!(out, "\x1b[{step}C")?;
                        step = 0;
                    }
                    let at = Placed { row, column };
                    if self.embed_cells(span, embed, at, paint, current, out)? {
                        return Ok(true);
                    }
                }
                // Normal-size cells with a background, such as a list's
                // marker in an alert, are drawn blank below the first row.
                None if span.style.size == TextSize::NORMAL && span.style.bg.is_some() => {
                    if step > 0 {
                        write!(out, "\x1b[{step}C")?;
                        step = 0;
                    }
                    let blank = paint(&Style {
                        bg: span.style.bg,
                        ..Style::default()
                    });
                    transition(current, &blank, self.caps, out)?;
                    write!(out, "{}", " ".repeat(cells))?;
                    *current = blank;
                }
                None => step += cells,
            }
            column += cells;
        }
        if step > 0 && reach {
            write!(out, "\x1b[{step}C")?;
        }
        Ok(false)
    }

    /// Writes row `embed.row + at.row` of the image `embed` in the cells of
    /// `span` at `at`. On kitty these are placeholders, whose foreground
    /// colour is the image id; on sixel they are blank cells with the row's
    /// sixel over them. Either way they have the span's background and
    /// link, but no underline or other attribute. Returns whether a sixel
    /// reaches the last column, after which nothing may be written.
    fn embed_cells(
        &mut self,
        span: &Span,
        embed: &Embed,
        at: Placed,
        paint: &impl Fn(&Style) -> Style,
        current: &mut Style,
        out: &mut impl Write,
    ) -> io::Result<bool> {
        let Placed { row, column } = at;
        let (images, caps) = (self.images, self.caps);
        let plain = paint(&Style {
            bg: span.style.bg,
            link: span.style.link.clone(),
            ..Style::default()
        });
        let columns = embed.placement.size.columns;
        if self.sixel() {
            // The sixel is drawn over blank cells in the style of the span.
            transition(current, &plain, self.caps, out)?;
            let under = self.shown(span.style.bg);
            let image_row = embed.row + u16::from(row);
            let tiles = (self.encoder).sixel_row(
                &embed.placement,
                image_row,
                &(0..columns),
                under,
                images,
                caps,
            )?;
            let reaches_last = column + usize::from(columns) >= self.reach();
            sixel::write_cells(out, columns, &tiles, &[], reaches_last)?;
            *current = plain;
            return Ok(reaches_last);
        }
        let id =
            self.sending_apart(|encoder, out| encoder.id(&embed.placement, images, caps, out))?;
        transition(current, &plain, self.caps, out)?;
        let columns = 0..columns;
        let form = self.form();
        graphics::write_row(out, id, embed.row + u16::from(row), columns, form)?;
        // The placeholders leave the terminal's foreground.
        *current = Style { fg: None, ..plain };
        Ok(false)
    }
}

/// Where an image in text is written: on row `row` of its line, whose
/// cells start at `column` of the window.
#[derive(Clone, Copy)]
struct Placed {
    row: u8,
    column: usize,
}

/// A writer that counts the bytes that go through it.
struct Counted<'w, W: Write + ?Sized> {
    out: &'w mut W,
    bytes: usize,
}

impl<W: Write + ?Sized> Write for Counted<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.out.write(buf)?;
        self.bytes += written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// `links` as runs over the placeholders of the image's columns `written`,
/// counted from the first of them: what is over other columns is left out
/// and a link that reaches past them ends at their edge.
fn clip<'a>(links: &'a [RowLink], written: &Range<u16>) -> Vec<graphics::LinkRun<'a>> {
    (links.iter())
        .filter_map(|link| {
            let from = link.columns.start.max(written.start);
            let to = link.columns.end.min(written.end);
            (from < to).then(|| graphics::LinkRun {
                columns: from - written.start..to - written.start,
                url: &link.url,
            })
        })
        .collect()
}

impl Writer<'_> {
    /// Whether kitty's graphics go through tmux, which drops them while
    /// kiri's pane is not shown.
    pub fn through_tmux(&self) -> bool {
        self.caps.tmux_passthrough
    }

    /// Sends every image again as its rows are next written, as the
    /// terminal may have lost them.
    pub fn lost(&mut self) {
        // A sixel is drawn with its row, so nothing is sent again.
        if !self.sixel() {
            self.encoder.lost();
        }
    }

    /// Sends the images `ids` again as their rows are next written, as
    /// the terminal may have lost them.
    pub fn lost_some(&mut self, ids: &[graphics::ImageId]) {
        if !self.sixel() {
            self.encoder.lost_some(ids);
        }
    }

    /// Asks kitty whether it holds the images sent through tmux since the
    /// last check, as tmux drops output for a client that cannot keep
    /// up; it answers each with [`graphics::checked`]'s answer. Returns
    /// the images asked about; none without tmux.
    pub fn check(&mut self, out: &mut impl Write) -> io::Result<Vec<graphics::ImageId>> {
        let sent = self.encoder.take_sent();
        for &(id, cells) in &sent {
            graphics::check(out, id, cells)?;
        }
        Ok(sent.into_iter().map(|(id, _)| id).collect())
    }

    /// Asks kitty whether it holds the images shown by the `lines` that are
    /// on the screen, as the reader has stopped; nothing on sixel.
    pub fn check_view(&mut self, lines: &[Line], out: &mut impl Write) -> io::Result<()> {
        if self.sixel() {
            return Ok(());
        }
        (self.encoder).check_view(lines, self.images, self.caps, out)
    }

    /// Writes the cursor move that makes kitty show an image sent again
    /// under placeholders on the screen; the caller flushes what it sent
    /// before it, and it afterwards.
    pub fn nudge(&self, out: &mut impl Write) -> io::Result<()> {
        if self.sixel() {
            return Ok(());
        }
        graphics::nudge(out)
    }

    /// Sends image `id` again, as kitty answered it does not hold it.
    pub fn send_again(&mut self, id: graphics::ImageId, out: &mut impl Write) -> io::Result<()> {
        (self.encoder).send_again(id, self.images, self.caps, out)
    }

    /// Notes that kitty answered it holds image `id`: its tries to send it
    /// again for being lost start anew.
    pub fn held_by_terminal(&mut self, id: graphics::ImageId) {
        self.encoder.held_by_terminal(id);
    }

    /// Deletes the images that image `id` replaced, as kitty answered
    /// the check of `id`.
    pub fn answered(&mut self, id: graphics::ImageId, out: &mut impl Write) -> io::Result<()> {
        if self.sixel() {
            return Ok(());
        }
        self.encoder.answered(id, out)
    }

    /// Tells the encoder which images the next frame shows and which are
    /// near them, in each of the shown `areas`, so that past its budget
    /// it lets go of others first. Call it beside the draw. Nothing on
    /// sixel, which holds no images.
    pub fn near(&mut self, areas: &[Near<'_>]) {
        if !self.sixel() {
            self.encoder.near(areas, self.images, self.caps);
        }
    }

    /// Deletes from the terminal the images the encoder no longer keeps.
    pub fn delete_unused(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.sixel() {
            self.encoder.discard_unused();
            return Ok(());
        }
        self.encoder.delete_unused(out)
    }

    /// Whether images are drawn as sixels.
    pub fn sixel(&self) -> bool {
        self.caps.is_sixel()
    }

    /// The colour the cells of a row show under its image: `under`, the
    /// colour of the block it lies in, else the page's, else the
    /// terminal's own, where kiri knows one; a colour that kiri writes in
    /// 256 colours is the one of those that the terminal shows. `None`
    /// without colours.
    fn shown(&self, under: Option<Color>) -> Option<Rgb> {
        if !self.caps.color {
            return None;
        }
        let (color, written) = match under.or(self.background) {
            Some(color) => (color, true),
            None => (self.caps.background?, false),
        };
        let Color::Rgb(red, green, blue) = color.rgb_or_xterm(&self.caps.palette) else {
            unreachable!("a colour that is RGB or else xterm's is RGB");
        };
        let approximated = written && !self.caps.truecolor && matches!(color, Color::Rgb(..));
        Some(match approximated {
            true => match Color::xterm(rgb_to_256(red, green, blue)) {
                Color::Rgb(r, g, b) => [r, g, b],
                Color::Palette(_) => unreachable!("xterm's colours are RGB"),
            },
            false => [red, green, blue],
        })
    }

    /// Draws the bands of the images that are loading from now on as they
    /// are `elapsed` after their start.
    pub fn at(&mut self, elapsed: Duration) {
        self.elapsed = elapsed;
    }

    /// Whether a band below an image that is loading is on the screen.
    pub fn has_bands(&self) -> bool {
        self.encoder.has_bands()
    }

    /// Whether the rows written since this was last asked wrote image data
    /// or the command that deletes an image; asking forgets it.
    pub fn sent_image(&mut self) -> bool {
        std::mem::take(&mut self.sent)
    }

    /// Erases `count` cells from the cursor on, in the page's background,
    /// leaving the cursor where it is and the terminal's default style. A
    /// scaled character among them goes whole, both rows. Nothing for 0.
    pub fn erase_chars(&self, count: usize, out: &mut impl Write) -> io::Result<()> {
        if count == 0 {
            return Ok(());
        }
        let fill = self.fill();
        transition(&Style::default(), &fill, self.caps, out)?;
        write!(out, "\x1b[{count}X")?;
        transition(&fill, &Style::default(), self.caps, out)
    }

    /// Scrolls the whole screen by `rows`, up when positive, with the
    /// colours of the page in effect, so that the rows that come in are
    /// blank in the page's background, as the rows a clear and an erase
    /// leave are. The default style is back after it.
    pub fn scroll(&self, rows: isize, out: &mut impl Write) -> io::Result<()> {
        let fill = self.fill();
        out.write_all(b"\x1b[0m")?;
        transition(&Style::default(), &fill, self.caps, out)?;
        scroll(rows, out)?;
        transition(&fill, &Style::default(), self.caps, out)
    }

    /// Erases the row the cursor is on in the page's background, leaving
    /// the cursor where it is and the terminal's default style.
    pub fn erase_line(&self, out: &mut impl Write) -> io::Result<()> {
        let fill = self.fill();
        transition(&Style::default(), &fill, self.caps, out)?;
        out.write_all(b"\x1b[2K")?;
        transition(&fill, &Style::default(), self.caps, out)
    }

    /// Writes `spans` in their own styles, without the page's colours, and
    /// leaves the cursor at their end with the terminal's default style.
    pub fn bar(&self, spans: &[Span], out: &mut impl Write) -> io::Result<()> {
        let mut current = Style::default();
        write_spans(spans, Style::clone, &mut current, self.caps, out)?;
        transition(&current, &Style::default(), self.caps, out)
    }
}

/// Writes normal-size spans, leaving `current` at the last one's style.
fn write_spans(
    spans: &[Span],
    paint: impl Fn(&Style) -> Style,
    current: &mut Style,
    caps: &Capabilities,
    out: &mut impl Write,
) -> io::Result<()> {
    for span in spans {
        let style = paint(&span.style);
        transition(current, &style, caps, out)?;
        out.write_all(span.text.as_bytes())?;
        *current = style;
    }
    Ok(())
}

fn transition(
    from: &Style,
    to: &Style,
    caps: &Capabilities,
    out: &mut impl Write,
) -> io::Result<()> {
    if from.link != to.link {
        if from.link.is_some() {
            out.write_all(b"\x1b]8;;\x1b\\")?;
        }
        if let Some(url) = &to.link {
            // Control characters would end the sequence early.
            let url: String = url.chars().filter(|c| !c.is_control()).collect();
            write!(out, "\x1b]8;;{url}\x1b\\")?;
        }
    }
    if from.attributes() != to.attributes() {
        out.write_all(b"\x1b[0")?;
        for code in sgr_codes(to, caps) {
            write!(out, ";{code}")?;
        }
        out.write_all(b"m")?;
    }
    Ok(())
}

fn sgr_codes(style: &Style, caps: &Capabilities) -> Vec<String> {
    let mut codes = Vec::new();
    for (on, code) in [
        (style.bold, "1"),
        (style.dim, "2"),
        (style.italic, "3"),
        (style.reverse, "7"),
        (style.strike, "9"),
    ] {
        if on {
            codes.push(code.to_owned());
        }
    }
    if let Some(code) = underline_code(style.underline, caps) {
        codes.push(code.to_owned());
    }
    if !caps.color {
        return codes;
    }
    let truecolor = caps.truecolor;
    if let Some(color) = style.fg {
        codes.push(color_code(color, 30, truecolor));
    }
    if let Some(color) = style.bg {
        codes.push(color_code(color, 40, truecolor));
    }
    // An underline colour (SGR 58) comes with the underline styles: kitty
    // introduced both, and the terminals that took one took the other.
    if let Some(color) = style.underline_color.filter(|_| caps.styled_underlines)
        && style.underline != Underline::None
    {
        codes.push(underline_color_code(color, truecolor));
    }
    codes
}

/// SGR 58, which has no short form for the first 16 colours.
fn underline_color_code(color: Color, truecolor: bool) -> String {
    match color {
        Color::Palette(index) => format!("58;5;{index}"),
        Color::Rgb(r, g, b) if truecolor => format!("58;2;{r};{g};{b}"),
        Color::Rgb(r, g, b) => format!("58;5;{}", rgb_to_256(r, g, b)),
    }
}

/// Underline styles other than single use the `4:n` form that kitty
/// introduced; other terminals get a single underline.
fn underline_code(underline: Underline, caps: &Capabilities) -> Option<&'static str> {
    match underline {
        Underline::None => None,
        Underline::Single => Some("4"),
        _ if !caps.styled_underlines => Some("4"),
        Underline::Double => Some("4:2"),
        Underline::Curly => Some("4:3"),
        Underline::Dotted => Some("4:4"),
        Underline::Dashed => Some("4:5"),
    }
}

/// `base` is 30 for foreground and 40 for background.
fn color_code(color: Color, base: u8, truecolor: bool) -> String {
    match color {
        Color::Palette(index @ 0..=7) => (base + index).to_string(),
        Color::Palette(index @ 8..=15) => (base + 60 + index - 8).to_string(),
        Color::Palette(index) => format!("{};5;{index}", base + 8),
        Color::Rgb(r, g, b) if truecolor => format!("{};2;{r};{g};{b}", base + 8),
        Color::Rgb(r, g, b) => format!("{};5;{}", base + 8, rgb_to_256(r, g, b)),
    }
}

/// Nearest entry of the 256-colour palette, from the 6×6×6 colour cube or
/// the grey ramp. The first 16 entries are skipped: they follow the theme.
fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let nearest_level = |v: u8| {
        (0..LEVELS.len())
            .min_by_key(|&i| (LEVELS[i] - i32::from(v)).abs())
            .expect("LEVELS is not empty")
    };
    let distance = |(r2, g2, b2): (i32, i32, i32)| {
        let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
        (r - r2).pow(2) + (g - g2).pow(2) + (b - b2).pow(2)
    };

    let (ri, gi, bi) = (nearest_level(r), nearest_level(g), nearest_level(b));
    let cube = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let average = (i32::from(r) + i32::from(g) + i32::from(b)) / 3;
    let grey_index = ((average - 8) / 10).clamp(0, 23);
    let grey_value = 8 + 10 * grey_index;

    if distance((grey_value, grey_value, grey_value)) < distance(cube) {
        232 + grey_index as u8
    } else {
        16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8
    }
}

/// How many pixels below the top of its cell the band goes: a little
/// below the top of a blank line, or over the bottom of a line with text
/// or an image on it, when `low`.
pub fn band_offset(cell_height: u16, low: bool) -> u16 {
    let band = BAND_HEIGHT as u16;
    match low {
        true => cell_height.saturating_sub(band + (cell_height / 10).max(1)),
        false => (cell_height / 6).min(cell_height.saturating_sub(band)),
    }
}

#[cfg(test)]
mod tests;
