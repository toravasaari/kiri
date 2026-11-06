//! Styled lines to bytes: SGR attributes, OSC 8 hyperlinks, cursor movement,
//! scaled text through `kitty::text`, and images through `encoder`, which
//! sends them, and the placeholders of `kitty::graphics`, which show them.

use std::io::{self, Write};
use std::ops::Range;
use std::time::Duration;

use crate::present::encoder::Encoder;
use crate::present::kitty;
use crate::present::kitty::graphics;
use crate::render::loading::{self, Ink};
use crate::typeset::caps::Capabilities;
use crate::typeset::color::{Color, Underline};
use crate::typeset::images::Images;
use crate::typeset::images::size::cell_size;
use crate::typeset::reading::engine::RowLink;
use crate::typeset::size::TextSize;
use crate::typeset::style::{Embed, ImageRow, Line, Span, Style};
use crate::typeset::theme::Theme;
use crate::typeset::wrap::width;

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

/// Ends the update [`begin_update`] started.
pub fn end_update(out: &mut impl Write) -> io::Result<()> {
    out.write_all(b"\x1b[?2026l")
}

/// Writes `lines` on `page`. `images` holds the pixels of the images the
/// lines show; each is sent before its first row. Returns what the
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
    for line in lines {
        for row in 0..writer.rows(line) {
            writer.row(line, row, out)?;
            out.write_all(b"\n")?;
        }
    }
    Ok(encoder)
}

/// The rows `line` occupies on a terminal with `caps`: as many as the
/// largest scale of its text, unless the terminal gets plain text.
pub fn rows(line: &Line, caps: &Capabilities) -> u8 {
    if caps.styling { line.rows() } else { 1 }
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
    /// The colour of placeholders and of the bands below them.
    ink: Ink,
    /// How long the bands have moved, which tells where their light part
    /// is.
    elapsed: Duration,
    /// What the terminal holds of the images.
    encoder: &'a mut Encoder,
    /// Whether the rows written since it was last asked sent image data.
    sent: bool,
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
            ink: Ink::new(page, caps),
            elapsed: Duration::ZERO,
            encoder,
            sent: false,
        }
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

    /// Erases the whole screen in the page's background and leaves the
    /// cursor at the top left with the terminal's default style.
    pub fn clear(&mut self, out: &mut impl Write) -> io::Result<()> {
        self.encoder.cleared();
        let fill = self.fill();
        transition(&Style::default(), &fill, self.caps, out)?;
        out.write_all(b"\x1b[H\x1b[2J")?;
        transition(&fill, &Style::default(), self.caps, out)
    }

    /// Writes the loading band over the row and places it there, sending
    /// its image first if it has not been sent.
    fn write_band(
        &mut self,
        band: &crate::typeset::style::Band,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let cell = cell_size(self.caps);
        let mut counted = Counted {
            out: &mut *out,
            bytes: 0,
        };
        let id = (self.encoder).band(band.columns, cell, self.ink, self.elapsed, &mut counted)?;
        self.sent |= counted.bytes > 0;
        out.write_all(b"\x1b7")?;
        if band.indent > 0 {
            write!(out, "\x1b[{}C", band.indent)?;
        }
        graphics::place(out, id, loading::band_offset(cell.1, band.low))?;
        out.write_all(b"\x1b8")
    }

    /// Writes row `row` of `line`, sending the image it shows first if it
    /// has not been sent, and leaves the cursor at its end with the
    /// terminal's default style.
    pub fn row(&mut self, line: &Line, row: u8, out: &mut impl Write) -> io::Result<()> {
        let caps = self.caps;
        if !caps.styling {
            let text: String = (line.lead.iter().chain(&line.spans).chain(&line.trail))
                .map(|span| span.text.as_str())
                .collect();
            return out.write_all(text.trim_end().as_bytes());
        }
        // Over the row, from where it starts; not behind a panel, which
        // it would be drawn over, nor under tmux, where a placement lands
        // at kitty's cursor rather than tmux's. The status line still
        // tells the progress.
        let drawn = row == 0 && !self.dim && !caps.tmux_passthrough;
        if let Some(band) = line.band.as_ref().filter(|_| drawn) {
            self.write_band(band, out)?;
        }
        let image = match &line.image {
            Some(ImageRow {
                placement,
                row,
                background,
                columns,
                page,
                overlay,
                links,
                ..
            }) => {
                if let Some(page) = page {
                    (self.encoder).page(&placement.key, page, overlay.as_ref());
                }
                let mut counted = Counted {
                    out: &mut *out,
                    bytes: 0,
                };
                let id = (self.encoder).id(placement, self.images, caps, &mut counted)?;
                self.sent |= counted.bytes > 0;
                Some((id, *row, columns.clone(), *background, links))
            }
            None => None,
        };
        let (background, foreground, dim) = (self.background, self.foreground, self.dim);
        let fill = self.fill();
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
        if row == 0 {
            for span in &line.spans {
                if let Some(embed) = &span.embed {
                    self.embed_cells(span, embed, 0, &paint, &mut current, out)?;
                    continue;
                }
                let style = paint(&span.style);
                transition(&current, &style, caps, out)?;
                if span.style.size != TextSize::NORMAL {
                    kitty::text::write_sized(out, span.style.size, &span.text)?;
                } else {
                    out.write_all(span.text.as_bytes())?;
                }
                current = style;
            }
            if let Some((id, image_row, columns, under, links)) = image {
                // Placeholders are neither underlined nor links, and
                // their foreground colour is the image id. Their
                // background shows where the image is transparent.
                let plain = paint(&Style {
                    bg: under,
                    ..Style::default()
                });
                transition(&current, &plain, caps, out)?;
                // The links of a page are hyperlinks over its placeholders.
                // Without hyperlinks the cells are for selecting a link only.
                let links = if caps.hyperlinks { &links[..] } else { &[] };
                let runs = clip(links, &columns);
                graphics::write_row_linked(out, id, image_row, columns, &runs)?;
                // The placeholders leave the terminal's foreground.
                current = Style { fg: None, ..plain };
            }
        } else if line.spans.iter().any(|span| span.embed.is_some()) {
            let reach = background.is_some() || !line.trail.is_empty();
            self.embed_rows(&line.spans, row, reach, &paint, &mut current, out)?;
        } else if (background.is_some() || !line.trail.is_empty()) && block_width > 0 {
            // Step over the scaled text to write the trail and fill the
            // rest of the row. `CSI 0 C` would still move one cell.
            write!(out, "\x1b[{block_width}C")?;
        }
        write_spans(&line.trail, paint, &mut current, caps, out)?;
        if background.is_some() {
            // Erase to the end of the line in the background colour.
            transition(&current, &fill, caps, out)?;
            out.write_all(b"\x1b[K")?;
            current = fill;
        }
        transition(&current, &Style::default(), caps, out)
    }
}

impl Writer<'_> {
    /// Writes row `row`, below the first, of a line of `spans` with images
    /// in them: steps over each text span and writes the next row of each
    /// image in its cells, and with `reach`, steps on to the end of the
    /// spans.
    fn embed_rows(
        &mut self,
        spans: &[Span],
        row: u8,
        reach: bool,
        paint: &impl Fn(&Style) -> Style,
        current: &mut Style,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let mut step = 0;
        for span in spans {
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
                    self.embed_cells(span, embed, row, paint, current, out)?;
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
                    write!(out, "{}", " ".repeat(width(std::slice::from_ref(span))))?;
                    *current = blank;
                }
                None => step += width(std::slice::from_ref(span)),
            }
        }
        if step > 0 && reach {
            write!(out, "\x1b[{step}C")?;
        }
        Ok(())
    }

    /// Writes the placeholders of the image `embed` in the cells of `span`
    /// on row `row` of the line: its image row `embed.row + row`. They have
    /// the background of the span and are inside its link, but are neither
    /// underlined nor in the span's other attributes; their foreground
    /// colour is the image id.
    fn embed_cells(
        &mut self,
        span: &Span,
        embed: &Embed,
        row: u8,
        paint: &impl Fn(&Style) -> Style,
        current: &mut Style,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let mut counted = Counted {
            out: &mut *out,
            bytes: 0,
        };
        let id = (self.encoder).id(&embed.placement, self.images, self.caps, &mut counted)?;
        self.sent |= counted.bytes > 0;
        let plain = paint(&Style {
            bg: span.style.bg,
            link: span.style.link.clone(),
            ..Style::default()
        });
        transition(current, &plain, self.caps, out)?;
        let columns = 0..embed.placement.size.columns;
        graphics::write_row(out, id, embed.row + u16::from(row), columns)?;
        // The placeholders leave the terminal's foreground.
        *current = Style { fg: None, ..plain };
        Ok(())
    }
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
        self.encoder.lost();
    }

    /// Sends the images `ids` again as their rows are next written, as
    /// the terminal may have lost them.
    pub fn lost_some(&mut self, ids: &[graphics::ImageId]) {
        self.encoder.lost_some(ids);
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

    /// Sends image `id` again, as kitty answered it does not hold it.
    pub fn send_again(&mut self, id: graphics::ImageId, out: &mut impl Write) -> io::Result<()> {
        (self.encoder).send_again(id, self.images, self.caps, out)
    }

    /// Deletes from the terminal the images the encoder no longer keeps.
    pub fn delete_unused(&mut self, out: &mut impl Write) -> io::Result<()> {
        self.encoder.delete_unused(out)
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

    /// Whether the rows written since this was last asked sent image
    /// data; asking forgets it.
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

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;

    fn link(columns: Range<u16>) -> RowLink {
        RowLink {
            columns,
            url: Rc::from("https://l.example/"),
            link: (0, 0),
        }
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
        writer.row(&line, 0, &mut out).unwrap();
        assert!(writer.sent_image());
        assert!(!writer.sent_image());
        writer.row(&line, 0, &mut out).unwrap();
        assert!(!writer.sent_image(), "the image is held: nothing is sent");
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
        use crate::typeset::style::Band;

        // High on a blank line, six cells in, and low over a line of text.
        let high = Line {
            band: Some(Band {
                columns: 4,
                indent: 6,
                low: false,
                key: "a.png".into(),
                above: 1,
            }),
            ..Line::blank()
        };
        let low = Line {
            band: Some(Band {
                columns: 4,
                indent: 0,
                low: true,
                key: "b.png".into(),
                above: 0,
            }),
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
        writer.row(&high, 0, &mut out).unwrap();
        writer.row(&low, 0, &mut out).unwrap();
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
    fn no_band_is_drawn_under_tmux() {
        use crate::typeset::style::Band;

        // A direct placement lands at kitty's cursor, not tmux's.
        let line = Line {
            band: Some(Band {
                columns: 4,
                indent: 0,
                low: false,
                key: "a.png".into(),
                above: 1,
            }),
            ..Line::blank()
        };
        let caps = Capabilities {
            cell_pixels: Some((10, 20)),
            tmux_passthrough: true,
            ..Capabilities::kitty(40)
        };
        let images = Images::new();
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
        let mut out = Vec::new();
        writer.row(&line, 0, &mut out).unwrap();
        assert!(!writer.has_bands());
        let out = String::from_utf8(out).unwrap();
        assert!(!out.contains("a=p"), "{out:?}");
        assert!(!out.contains("\x1b_G"), "{out:?}");
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
}
