//! Styled lines to bytes: SGR attributes, OSC 8 hyperlinks, scaled text.

use std::io::{self, Write};

use super::style::{Color, Line, Style};
use crate::kitty;
use crate::term::Capabilities;

pub fn write(lines: &[Line], caps: &Capabilities, out: &mut impl Write) -> io::Result<()> {
    for line in lines {
        if !caps.styling {
            let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
            writeln!(out, "{}", text.trim_end())?;
            continue;
        }
        let mut current = Style::default();
        for span in &line.spans {
            transition(&current, &span.style, caps, out)?;
            current = span.style.clone();
            if line.scale > 1 {
                kitty::text::write_scaled(out, line.scale, &span.text)?;
            } else {
                out.write_all(span.text.as_bytes())?;
            }
        }
        transition(&current, &Style::default(), caps, out)?;
        for _ in 0..line.scale.max(1) {
            out.write_all(b"\n")?;
        }
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
        for code in sgr_codes(to, caps.truecolor) {
            write!(out, ";{code}")?;
        }
        out.write_all(b"m")?;
    }
    Ok(())
}

fn sgr_codes(style: &Style, truecolor: bool) -> Vec<String> {
    let mut codes = Vec::new();
    for (on, code) in [
        (style.bold, "1"),
        (style.dim, "2"),
        (style.italic, "3"),
        (style.underline, "4"),
        (style.strike, "9"),
    ] {
        if on {
            codes.push(code.to_owned());
        }
    }
    if let Some(color) = style.fg {
        codes.push(color_code(color, 30, truecolor));
    }
    if let Some(color) = style.bg {
        codes.push(color_code(color, 40, truecolor));
    }
    codes
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

/// Nearest entry in the 6×6×6 colour cube of the 256-colour palette.
fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    let level = |v: u8| -> u8 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            (v - 35) / 40
        }
    };
    16 + 36 * level(r) + 6 * level(g) + level(b)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
