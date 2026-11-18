//! Property tests of `places`: whatever the text and the term, the places
//! are those that a list of the text's characters gives, as the first
//! search found them.

use std::ops::Range;

use proptest::prelude::*;

use super::*;
use crate::testing::props_config;

/// The places of `term` in `text` found over a list of its characters.
fn places_by_list(text: &str, term: &[char], ignore_case: bool) -> Vec<Range<usize>> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut found = Vec::new();
    if term.is_empty() {
        return found;
    }
    let mut start = 0;
    while start + term.len() <= chars.len() {
        let candidate = &chars[start..start + term.len()];
        let matches = candidate
            .iter()
            .zip(term)
            .all(|(&(_, c), &t)| same(c, t, ignore_case));
        if matches {
            let end = chars
                .get(start + term.len())
                .map_or(text.len(), |&(at, _)| at);
            found.push(chars[start].0..end);
            start += term.len();
        } else {
            start += 1;
        }
    }
    found
}

/// Characters that a search compares in its ways: in both cases, a space
/// and a no-break space, of one to four bytes, and one whose lower case is
/// two characters.
fn char() -> impl Strategy<Value = char> {
    prop_oneof![
        Just('a'),
        Just('A'),
        Just('b'),
        Just(' '),
        Just('\u{a0}'),
        Just('é'),
        Just('É'),
        Just('ї'),
        Just('Ї'),
        Just('İ'),
        Just('i'),
        Just('😀'),
    ]
}

proptest! {
    #![proptest_config(props_config())]

    #[test]
    fn places_are_those_a_list_of_the_characters_gives(
        text in proptest::collection::vec(char(), 0..30),
        term in proptest::collection::vec(char(), 0..4),
        ignore_case in any::<bool>(),
    ) {
        let text: String = text.into_iter().collect();
        prop_assert_eq!(
            places(&text, &term, ignore_case),
            places_by_list(&text, &term, ignore_case)
        );
    }
}
