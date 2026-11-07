//! The bytes that put a sixel in the cells of a text row: blank cells to
//! hold the row's style and its links, and the sixel drawn over them.

use std::io::{self, Write};
use std::ops::Range;

/// One of the sixels of a row: the pixels of some of its cells, from the
/// cell `offset` cells from the row's first, as a `DCS ... ST` sequence.
/// A row wider than the terminal's sixel limit is several, side by side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub offset: u16,
    pub bytes: Vec<u8>,
}

/// A hyperlink over some of a row's cells, counted from the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link<'a> {
    pub columns: Range<u16>,
    pub url: &'a str,
}

/// Writes `tiles` over `width` cells from the cursor, in the style the
/// terminal has now, and leaves the cursor after them.
///
/// The cursor is saved and the cells are written blank, with each of `links`
/// as a hyperlink over its cells; then each tile is drawn from the saved
/// position, moved right by its offset, so that no column is counted and it
/// does not matter where the terminal leaves the cursor after a sixel. A
/// character written over a cell removes its pixels, and that is why the
/// blank cells come first and nothing is written to these cells after.
///
/// The cursor ends `width` cells on, unless that is past the last column of
/// the window (`reaches_last`): a terminal then stops it on the last cell,
/// and what is written from there removes the sixel's last cell. Nothing
/// should be written after such a row, and this writes no move.
pub fn write_cells(
    out: &mut impl Write,
    width: u16,
    tiles: &[Tile],
    links: &[Link<'_>],
    reaches_last: bool,
) -> io::Result<()> {
    out.write_all(b"\x1b7")?;
    let mut open = false;
    for at in 0..width {
        if let Some(link) = links.iter().find(|link| link.columns.start == at) {
            // Control characters would end the sequence early.
            let url: String = link.url.chars().filter(|c| !c.is_control()).collect();
            write!(out, "\x1b]8;;{url}\x1b\\")?;
            open = true;
        }
        out.write_all(b" ")?;
        if links.iter().any(|link| link.columns.end == at + 1) {
            out.write_all(b"\x1b]8;;\x1b\\")?;
            open = false;
        }
    }
    // A link that reaches past the last cell ends there.
    if open {
        out.write_all(b"\x1b]8;;\x1b\\")?;
    }
    out.write_all(b"\x1b8")?;
    for tile in tiles.iter().filter(|tile| !tile.bytes.is_empty()) {
        if tile.offset > 0 {
            write!(out, "\x1b[{}C", tile.offset)?;
        }
        out.write_all(&tile.bytes)?;
        out.write_all(b"\x1b8")?;
    }
    if !reaches_last && width > 0 {
        write!(out, "\x1b[{width}C")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(width: u16, tiles: &[Tile], links: &[Link<'_>], reaches_last: bool) -> String {
        let mut out = Vec::new();
        write_cells(&mut out, width, tiles, links, reaches_last).unwrap();
        String::from_utf8(out).unwrap().replace('\x1b', "ESC ")
    }

    fn tile(offset: u16, bytes: &str) -> Tile {
        Tile {
            offset,
            bytes: bytes.as_bytes().to_vec(),
        }
    }

    #[test]
    fn blank_cells_the_sixel_over_them_and_a_step_past_them() {
        let out = written(3, &[tile(0, "<s>")], &[], false);
        assert_eq!(out, "ESC 7   ESC 8<s>ESC 8ESC [3C");
    }

    #[test]
    fn tiles_start_at_their_own_cells() {
        let out = written(5, &[tile(0, "<a>"), tile(3, "<b>")], &[], false);
        assert_eq!(out, "ESC 7     ESC 8<a>ESC 8ESC [3C<b>ESC 8ESC [5C");
    }

    #[test]
    fn a_row_that_reaches_the_last_column_writes_no_step() {
        let out = written(2, &[tile(0, "<s>")], &[], true);
        assert_eq!(out, "ESC 7  ESC 8<s>ESC 8");
    }

    #[test]
    fn links_are_hyperlinks_over_their_blank_cells_and_end_with_the_row() {
        let links = [
            Link {
                columns: 1..2,
                url: "https://a.example/",
            },
            Link {
                columns: 3..9,
                url: "https://b.example/\x1b]8;;evil",
            },
        ];
        let out = written(4, &[tile(0, "<s>")], &links, false);
        let expected = [
            "\x1b7 ",
            "\x1b]8;;https://a.example/\x1b\\ \x1b]8;;\x1b\\ ",
            "\x1b]8;;https://b.example/]8;;evil\x1b\\ ",
            "\x1b]8;;\x1b\\\x1b8<s>\x1b8\x1b[4C",
        ]
        .concat()
        .replace('\x1b', "ESC ");
        assert_eq!(out, expected);
    }

    #[test]
    fn an_empty_tile_is_not_drawn() {
        let out = written(2, &[tile(0, "")], &[], false);
        assert_eq!(out, "ESC 7  ESC 8ESC [2C");
    }
}
