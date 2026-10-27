//! kitty's text sizing protocol (OSC 66).
//!
//! <https://sw.kovidgoyal.net/kitty/text-sizing-protocol/>

use std::io::{self, Write};

use crate::size::{HorizontalAlign, TextSize, VerticalAlign};

/// The protocol's limit for the text in one escape sequence.
const MAX_PAYLOAD: usize = 4096;

/// Writes `text` at `size`. The text occupies `size.scale()` rows, but the
/// cursor stays on the first one: the caller moves it down. Text at a
/// fraction is written in the groups it is packed in, each as wide as
/// `TextSize::width` measures it.
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
    if size.fraction().is_some() {
        // Zero-width text joins the group after it, so a group of no
        // cells is a span of nothing else. It shows nothing, and `w=0`
        // would ask the terminal to measure it.
        for group in size.groups(text).iter().filter(|group| group.cells > 0) {
            // Left is the protocol's default.
            let align = match group.align {
                HorizontalAlign::Left => "",
                HorizontalAlign::Right => ":h=1",
                HorizontalAlign::Centre => ":h=2",
            };
            write!(
                out,
                "\x1b]66;{keys}{align}:w={};{}\x1b\\",
                group.cells, group.text
            )?;
        }
        return Ok(());
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
                format!("\x1b]66;{keys}:w=1;x\x1b\\"),
                "{align:?}"
            );
        }
    }

    fn written(size: TextSize, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        write_sized(&mut out, size, text).unwrap();
        (String::from_utf8(out).unwrap().split_terminator("\x1b\\"))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn writes_text_at_a_fraction_in_packed_groups() {
        let half = Fraction::new(1, 2).unwrap();
        let size = TextSize::NORMAL.with_fraction(half, VerticalAlign::Top);
        assert_eq!(written(size, "abc"), ["\x1b]66;s=1:n=1:d=2:w=2;abc"]);
        assert_eq!(written(size, "a日"), ["\x1b]66;s=1:n=1:d=2:w=2;a日"]);
        let seven_eighths = Fraction::new(7, 8).unwrap();
        let size = TextSize::new(2)
            .unwrap()
            .with_fraction(seven_eighths, VerticalAlign::Bottom);
        assert_eq!(
            written(size, "abcdefghij"),
            [
                "\x1b]66;s=2:n=7:d=8:v=1:h=1:w=7;abcdefgh",
                "\x1b]66;s=2:n=7:d=8:v=1:w=2;ij"
            ]
        );
    }

    #[test]
    fn writes_where_a_group_sits_in_its_cells() {
        let three_quarters = Fraction::new(3, 4).unwrap();
        let size = TextSize::NORMAL.with_fraction(three_quarters, VerticalAlign::Top);
        // Left is the protocol's default.
        assert_eq!(
            written(size, "x a b c d e f g h y"),
            [
                "\x1b]66;s=1:n=3:d=4:w=3;x a ",
                "\x1b]66;s=1:n=3:d=4:h=2:w=3;b c ",
                "\x1b]66;s=1:n=3:d=4:h=1:w=3;d e ",
                "\x1b]66;s=1:n=3:d=4:h=1:w=3;f g ",
                "\x1b]66;s=1:n=3:d=4:h=2:w=1;h",
                "\x1b]66;s=1:n=3:d=4:w=2; y"
            ]
        );
    }

    #[test]
    fn writes_no_group_of_no_cells() {
        let half = Fraction::new(1, 2).unwrap();
        let size = TextSize::NORMAL.with_fraction(half, VerticalAlign::Top);
        // `w=0` would ask the terminal to measure the text itself.
        assert_eq!(written(size, "\u{200b}"), Vec::<String>::new());
        // A zero-width character before text joins its group.
        assert_eq!(
            written(size, "\u{200b}ab"),
            ["\x1b]66;s=1:n=1:d=2:w=1;\u{200b}ab"]
        );
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
