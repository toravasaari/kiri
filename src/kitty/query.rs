//! Queries that tell whether a terminal speaks kitty's protocols, and how to
//! read the answers.

/// Cells the cursor moves for each text sizing test: the tests draw a space
/// with a width of this many cells, then at this scale.
const STEP: u32 = 2;

/// Two text sizing tests: a space with `w=2`, a width of two cells, and a
/// space with `s=2`, twice the normal scale. kitty's documentation
/// recommends a cursor position report before and after each, which
/// [`text_sizing_answered`] then reads.
/// <https://sw.kovidgoyal.net/kitty/text-sizing-protocol/>
pub fn text_sizing() -> [String; 2] {
    [
        format!("\x1b]66;w={STEP}; \x1b\\"),
        format!("\x1b]66;s={STEP}; \x1b\\"),
    ]
}

/// Erases what the [`text_sizing`] tests drew: their row, and the rows
/// below it that the space at scale [`STEP`] also covers. Erasing only the
/// first row leaves the rest of that space in place, and kitty then draws
/// a scaled character on the next row to the right of it. The cursor ends
/// at the start of the tests' row.
pub fn erase_text_sizing_tests() -> String {
    let below = "\x1b[B\x1b[2K".repeat(STEP as usize - 1);
    format!("\r\x1b[2K\x1b7{below}\x1b8")
}

/// Whether the cursor columns reported around the [`text_sizing`] tests
/// show that the terminal moved the cursor for both.
pub fn text_sizing_answered(columns: &[u32]) -> bool {
    matches!(columns, [a, b, c, ..] if *b == a + STEP && *c == b + STEP)
}

/// Asks the graphics protocol to check a one-pixel image without storing
/// it. A terminal that supports the protocol answers with
/// [`GRAPHICS_ANSWER`]; others ignore the query.
pub const GRAPHICS: &str = "\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\";

/// The start of a successful answer to [`GRAPHICS`], for the same image id.
/// Both start as every graphics sequence does, with
/// [`graphics::START`](super::graphics::START), which a test checks.
pub const GRAPHICS_ANSWER: &str = "\x1b_Gi=31;OK";

/// Asks for the size of one cell in pixels (XTWINOPS 16), which kitty
/// answers also where the kernel's window size has no pixels.
pub const CELL_SIZE: &str = "\x1b[16t";

/// The cell size as width and height in pixels from the answer to
/// [`CELL_SIZE`] in `text`: `ESC [ 6 ; height ; width t`. `None` without an
/// answer or for a size of 0.
pub fn cell_size(text: &str) -> Option<(u16, u16)> {
    text.split("\x1b[6;").skip(1).find_map(|part| {
        let (height, rest) = part.split_once(';')?;
        let width = &rest[..rest.find('t')?];
        let size = (width.parse().ok()?, height.parse().ok()?);
        (size.0 > 0 && size.1 > 0).then_some(size)
    })
}

/// The first kitty release with the graphics protocol's Unicode
/// placeholders, as major and minor version.
const PLACEHOLDERS_SINCE: (u32, u32) = (0, 28);

/// Whether a terminal that names itself `name_version` in its XTVERSION
/// answer, such as `kitty(0.47.1)`, has the graphics protocol's Unicode
/// placeholders. Only kitty's own name counts; other terminals that have
/// them are not recognised yet.
pub fn placeholders_by_version(name_version: &str) -> bool {
    let Some(version) = name_version
        .strip_prefix("kitty(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    let mut parts = version.split('.').map(str::parse::<u32>);
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => (major, minor) >= PLACEHOLDERS_SINCE,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitty_has_placeholders_from_0_28() {
        assert!(placeholders_by_version("kitty(0.28.0)"));
        assert!(placeholders_by_version("kitty(0.47.1)"));
        assert!(placeholders_by_version("kitty(0.100.0)"));
        assert!(placeholders_by_version("kitty(1.0.0)"));
        assert!(!placeholders_by_version("kitty(0.27.1)"));
        assert!(!placeholders_by_version("kitty(0.9.0)"));
    }

    #[test]
    fn other_names_and_versions_promise_no_placeholders() {
        assert!(!placeholders_by_version("WezTerm 20240203-110809-5046fc22"));
        assert!(!placeholders_by_version("foot(1.16.2)"));
        assert!(!placeholders_by_version("kitty"));
        assert!(!placeholders_by_version("kitty()"));
        assert!(!placeholders_by_version("kitty(next)"));
        assert!(!placeholders_by_version("kitty(0.28.0"));
        assert!(!placeholders_by_version("notkitty(0.30.0)"));
    }

    #[test]
    fn erasing_the_tests_clears_every_row_the_scaled_space_covers() {
        // The space at scale 2 is two rows tall. Its lower half, left on
        // the next row, pushes a scaled heading drawn there to the right.
        assert_eq!(
            erase_text_sizing_tests(),
            "\r\x1b[2K\x1b7\x1b[B\x1b[2K\x1b8"
        );
    }

    #[test]
    fn reads_the_cell_size_answer() {
        assert_eq!(cell_size("\x1b[6;19;9t"), Some((9, 19)));
        assert_eq!(
            cell_size("\x1b[5;1R\x1b[6;38;18t\x1b[?62;c"),
            Some((18, 38))
        );
        // A cursor position report on row 6 starts the same way.
        assert_eq!(
            cell_size("\x1b[6;1R\x1b[6;3R\x1b]11;rgb:0/0/0\x1b\\\x1b[6;19;9t"),
            Some((9, 19))
        );
        assert_eq!(cell_size("\x1b[6;1R\x1b_Gi=31;OK\x1b\\\x1b[?62;c"), None);
    }

    #[test]
    fn a_missing_or_empty_cell_size_answer_tells_nothing() {
        assert_eq!(cell_size("\x1b[5;1R\x1b[?62;c"), None);
        assert_eq!(cell_size("\x1b[6;0;9t"), None);
        assert_eq!(cell_size("\x1b[6;19;0t"), None);
        assert_eq!(cell_size("\x1b[6;19t"), None);
        assert_eq!(cell_size("\x1b[6;a;9t"), None);
        // The window size in pixels (XTWINOPS 14) is not the cell size.
        assert_eq!(cell_size("\x1b[4;760;1080t"), None);
    }

    #[test]
    fn the_graphics_query_and_its_answer_are_graphics_sequences() {
        use super::super::graphics::START;
        assert!(GRAPHICS.starts_with(START));
        assert!(GRAPHICS_ANSWER.starts_with(START));
    }

    #[test]
    fn text_sizing_needs_both_tests_to_move_the_cursor() {
        assert!(text_sizing_answered(&[1, 3, 5]));
        assert!(!text_sizing_answered(&[1, 3, 3]));
        assert!(!text_sizing_answered(&[1, 1, 1]));
        assert!(!text_sizing_answered(&[1, 3]));
    }
}
