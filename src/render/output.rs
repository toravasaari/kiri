//! Styled lines to bytes: SGR attributes, OSC 8 hyperlinks, cursor movement,
//! scaled text through `kitty::text` and images through `kitty::graphics`.

use std::collections::HashMap;
use std::io::{self, Write};

use image::RgbaImage;
use image::imageops;

use super::image::{Placement, Size, cell_size};
use super::style::{ImageRow, Line, Span, Style};
use super::wrap::width;
use crate::caps::Capabilities;
use crate::color::{Color, Underline};
use crate::images::{Images, Source};
use crate::kitty;
use crate::kitty::graphics;

/// The colours of the page the document is drawn on. `None` leaves the
/// terminal's own showing through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Page {
    /// Painted across the whole terminal width behind the document.
    pub background: Option<Color>,
    /// For text that has no colour of its own.
    pub foreground: Option<Color>,
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
/// lines show; each is sent before its first row.
pub fn write(
    lines: &[Line],
    images: &Images,
    caps: &Capabilities,
    page: Page,
    out: &mut impl Write,
) -> io::Result<()> {
    let mut writer = Writer::new(images, caps, page);
    for line in lines {
        for row in 0..writer.rows(line) {
            writer.row(line, row, out)?;
            out.write_all(b"\n")?;
        }
    }
    Ok(())
}

/// The rows `line` occupies on a terminal with `caps`: a scaled line
/// takes as many as its scale, unless the terminal gets plain text.
pub fn rows(line: &Line, caps: &Capabilities) -> u8 {
    if caps.styling { line.scale.max(1) } else { 1 }
}

/// Writes lines row by row, wherever the cursor is, and remembers which
/// images it has sent.
pub struct Writer<'a> {
    images: &'a Images,
    caps: &'a Capabilities,
    background: Option<Color>,
    foreground: Option<Color>,
    /// Every line dimmed, as behind a panel.
    dim: bool,
    /// The images sent so far, by where and how large they are shown.
    sent: HashMap<Placement, graphics::ImageId>,
}

impl<'a> Writer<'a> {
    /// A writer for lines on `page` with the pixels in `images`.
    pub fn new(images: &'a Images, caps: &'a Capabilities, page: Page) -> Self {
        Self {
            images,
            caps,
            background: page.background.filter(|_| caps.color),
            foreground: page.foreground.filter(|_| caps.color),
            dim: false,
            sent: HashMap::new(),
        }
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

    /// Erases the whole screen in the page's background and leaves the
    /// cursor at the top left with the terminal's default style.
    pub fn clear(&self, out: &mut impl Write) -> io::Result<()> {
        let fill = self.fill();
        transition(&Style::default(), &fill, self.caps, out)?;
        out.write_all(b"\x1b[H\x1b[2J")?;
        transition(&fill, &Style::default(), self.caps, out)
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
        let image = match &line.image {
            Some(ImageRow { placement, row }) => {
                let id = match self.sent.get(placement.as_ref()) {
                    Some(&id) => id,
                    None => {
                        let id = send(placement, self.images, caps, out)?;
                        self.sent.insert(placement.as_ref().clone(), id);
                        id
                    }
                };
                Some((id, *row, placement.size.columns))
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
        let scale = line.scale.max(1);
        // A scaled line occupies `scale` rows but is written only on the
        // first; the rows below get the lead, the trail and the fill around
        // it.
        let block_width = width(&line.spans) * usize::from(scale);
        let mut current = Style::default();
        write_spans(&line.lead, paint, &mut current, caps, out)?;
        if row == 0 {
            for span in &line.spans {
                let style = paint(&span.style);
                transition(&current, &style, caps, out)?;
                if scale > 1 {
                    kitty::text::write_scaled(out, scale, &span.text)?;
                } else {
                    out.write_all(span.text.as_bytes())?;
                }
                current = style;
            }
            if let Some((id, image_row, columns)) = image {
                // Placeholders are neither underlined nor links, and
                // their foreground colour is the image id.
                let plain = paint(&Style::default());
                transition(&current, &plain, caps, out)?;
                graphics::write_row(out, id, image_row, columns)?;
                // The placeholders leave the terminal's foreground.
                current = Style { fg: None, ..plain };
            }
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
    /// Writes `spans` in their own styles, without the page's colours, and
    /// leaves the cursor at their end with the terminal's default style.
    pub fn bar(&self, spans: &[Span], out: &mut impl Write) -> io::Result<()> {
        let mut current = Style::default();
        write_spans(spans, Style::clone, &mut current, self.caps, out)?;
        transition(&current, &Style::default(), self.caps, out)
    }
}

/// Scales the image `placement` shows to its size and sends it.
fn send(
    placement: &Placement,
    images: &Images,
    caps: &Capabilities,
    out: &mut impl Write,
) -> io::Result<graphics::ImageId> {
    let Some(Ok(source)) = images.get(&placement.url) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("no pixels for image {}", placement.url),
        ));
    };
    let pixels = fit(source, placement.size, cell_size(caps));
    let cells = graphics::Cells {
        columns: placement.size.columns,
        rows: placement.size.rows,
    };
    let (width, height) = pixels.dimensions();
    let id = graphics::ImageId::new(pixels.as_raw(), width, height, cells);
    let pixels = graphics::Pixels {
        width,
        height,
        rgba: pixels.as_raw(),
    };
    graphics::transmit(out, id, &pixels, cells)?;
    Ok(id)
}

/// `source` drawn at `size` and placed at the top left of the cells it
/// covers, the rest transparent. The terminal then shows it pixel for pixel
/// when the cells are `cell` pixels, and scales it as a whole when they are
/// not.
fn fit(source: &Source, size: Size, cell: (u16, u16)) -> RgbaImage {
    let mut canvas = RgbaImage::new(
        u32::from(size.columns) * u32::from(cell.0),
        u32::from(size.rows) * u32::from(cell.1),
    );
    imageops::replace(&mut canvas, &source.render(size.width, size.height), 0, 0);
    canvas
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
    use super::*;

    #[test]
    fn empty_scaled_line_does_not_step_over_anything() {
        let line = Line {
            lead: Vec::new(),
            spans: Vec::new(),
            image: None,
            trail: Vec::new(),
            scale: 2,
        };
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
