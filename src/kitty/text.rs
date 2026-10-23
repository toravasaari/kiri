//! kitty's text sizing protocol (OSC 66).
//!
//! <https://sw.kovidgoyal.net/kitty/text-sizing-protocol/>

use std::io::{self, Write};

use crate::size::{TextSize, VerticalAlign};

/// The protocol's limit for the text in one escape sequence.
const MAX_PAYLOAD: usize = 4096;

/// Writes `text` at `size`. The text occupies `size.scale()` rows, but the
/// cursor stays on the first one: the caller moves it down.
pub fn write_sized(out: &mut impl Write, size: TextSize, text: &str) -> io::Result<()> {
    let mut keys = format!("s={}", size.scale());
    if let Some(fraction) = size.fraction() {
        keys += &format!(":n={}:d={}", fraction.numerator(), fraction.denominator());
        // Top is the protocol's default.
        match size.align() {
            VerticalAlign::Top => {}
            VerticalAlign::Bottom => keys += ":v=1",
            VerticalAlign::Centre => keys += ":v=2",
        }
    }
    let mut rest = text;
    while !rest.is_empty() {
        let mut end = rest.len().min(MAX_PAYLOAD);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        write!(out, "\x1b]66;{keys};{}\x1b\\", &rest[..end])?;
        rest = &rest[end..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::size::Fraction;

    #[test]
    fn wraps_a_whole_scale_in_one_sequence() {
        let mut out = Vec::new();
        write_sized(&mut out, TextSize::new(2).unwrap(), "Title").unwrap();
        assert_eq!(out, b"\x1b]66;s=2;Title\x1b\\");
    }

    #[test]
    fn writes_a_fraction_with_its_alignment() {
        let half = Fraction::new(1, 2).unwrap();
        for (align, keys) in [
            (VerticalAlign::Top, "s=1:n=1:d=2"),
            (VerticalAlign::Bottom, "s=1:n=1:d=2:v=1"),
            (VerticalAlign::Centre, "s=1:n=1:d=2:v=2"),
        ] {
            let mut out = Vec::new();
            let size = TextSize::NORMAL.with_fraction(half, align);
            write_sized(&mut out, size, "x").unwrap();
            assert_eq!(
                String::from_utf8(out).unwrap(),
                format!("\x1b]66;{keys};x\x1b\\"),
                "{align:?}"
            );
        }
    }

    #[test]
    fn splits_long_text_at_character_boundaries() {
        let text = "ä".repeat(MAX_PAYLOAD);
        let mut out = Vec::new();
        write_sized(&mut out, TextSize::new(2).unwrap(), &text).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out.matches("\x1b]66;").count(), 2);
        assert_eq!(out.matches('ä').count(), MAX_PAYLOAD);
    }
}
