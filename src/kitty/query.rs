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
pub const GRAPHICS_ANSWER: &str = "\x1b_Gi=31;OK";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_sizing_needs_both_tests_to_move_the_cursor() {
        assert!(text_sizing_answered(&[1, 3, 5]));
        assert!(!text_sizing_answered(&[1, 3, 3]));
        assert!(!text_sizing_answered(&[1, 1, 1]));
        assert!(!text_sizing_answered(&[1, 3]));
    }
}
