//! Property tests of reading a CSV: whatever the source, the table and its
//! texts are what reading the whole source at once gives.

use proptest::prelude::*;

use super::Separator;
use super::grown::check_grown;
use super::tests::read_as_before;
use crate::testing::props_config;

/// Sources made of what changes how a CSV reads: quotes, both separators,
/// every line break, a space, a byte order mark, and letters of one and of
/// two bytes.
fn source() -> impl Strategy<Value = String> {
    let pieces = prop_oneof![
        3 => Just("a"),
        1 => Just("é"),
        2 => Just(","),
        2 => Just("\t"),
        3 => Just("\""),
        2 => Just("\n"),
        1 => Just("\r"),
        1 => Just("\r\n"),
        1 => Just(" "),
        1 => Just("\u{feff}"),
    ];
    proptest::collection::vec(pieces, 0..40).prop_map(|pieces| pieces.concat())
}

proptest! {
    #![proptest_config(props_config())]

    #[test]
    fn a_table_and_its_texts_are_what_reading_the_whole_source_gives(
        source in source(),
        tab in any::<bool>(),
    ) {
        let separator = if tab { Separator::Tab } else { Separator::Comma };
        if let Err(error) = read_as_before(&source, separator) {
            prop_assert!(false, "{:?}: {}", separator, error);
        }
    }

    #[test]
    fn a_source_grown_at_any_place_is_parsed_on_as_the_whole(
        source in source(),
        cut in any::<prop::sample::Index>(),
        tab in any::<bool>(),
        small in any::<bool>(),
    ) {
        let separator = if tab { Separator::Tab } else { Separator::Comma };
        let cuts: Vec<usize> =
            (0..=source.len()).filter(|&at| source.is_char_boundary(at)).collect();
        let cut = cuts[cut.index(cuts.len())];
        // A small capped view comes to its cap in sources this short.
        let max_cells = if small { 12 } else { super::MAX_CELLS };
        if let Err(error) = check_grown(&source[..cut], &source, separator, max_cells) {
            prop_assert!(false, "{}", error);
        }
    }
}
