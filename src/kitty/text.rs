//! kitty's text sizing protocol (OSC 66).
//!
//! <https://sw.kovidgoyal.net/kitty/text-sizing-protocol/>

use std::io::{self, Write};

/// The protocol's limit for the text in one escape sequence.
const MAX_PAYLOAD: usize = 4096;

/// Writes `text` at `scale` times the normal size. The text occupies `scale`
/// rows, but the cursor stays on the first one: the caller moves it down.
pub fn write_scaled(out: &mut impl Write, scale: u8, text: &str) -> io::Result<()> {
    let mut rest = text;
    while !rest.is_empty() {
        let mut end = rest.len().min(MAX_PAYLOAD);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        write!(out, "\x1b]66;s={scale};{}\x1b\\", &rest[..end])?;
        rest = &rest[end..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_text_in_one_sequence() {
        let mut out = Vec::new();
        write_scaled(&mut out, 2, "Title").unwrap();
        assert_eq!(out, b"\x1b]66;s=2;Title\x1b\\");
    }

    #[test]
    fn splits_long_text_at_character_boundaries() {
        let text = "ä".repeat(MAX_PAYLOAD);
        let mut out = Vec::new();
        write_scaled(&mut out, 2, &text).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out.matches("\x1b]66;").count(), 2);
        assert_eq!(out.matches('ä').count(), MAX_PAYLOAD);
    }
}
