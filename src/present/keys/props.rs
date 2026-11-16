//! Property tests of the key decoder: no input makes it panic, and an input
//! made of whole sequences decodes the same however the reads cut it.

use proptest::prelude::*;

use super::*;
use crate::testing::props_config;

/// What a terminal sends: keys, mouse and focus reports, answers that end
/// in ST or BEL, and plain characters.
fn token() -> impl Strategy<Value = Vec<u8>> {
    let known: &[&[u8]] = &[
        b"\x1b[A",
        b"\x1b[B",
        b"\x1bOC",
        b"\x1b[1;3D",
        b"\x1b[1;3C",
        b"\x1b[Z",
        b"\x1b[5~",
        b"\x1b[6;2~",
        b"\x1b[H",
        b"\x1b[8~",
        b"\x1b[I",
        b"\x1b[O",
        b"\x1b[6;20;10t",
        b"\x1b[M\xa0#$",
        b"\x1b",
        b"\x1b\x1b",
        b"\r",
        b"\x7f",
        b"\x03",
    ];
    prop_oneof![
        proptest::sample::select(known.to_vec()).prop_map(<[u8]>::to_vec),
        (0u32..140, 0u32..400, 0u32..400, any::<bool>()).prop_map(|(b, x, y, down)| {
            let last = if down { 'M' } else { 'm' };
            format!("\x1b[<{b};{x};{y}{last}").into_bytes()
        }),
        (
            ".{0,20}",
            prop_oneof![Just("\x1b\\"), Just("\x07")],
            0usize..4
        )
            .prop_map(|(text, end, kind)| {
                let text: String = text
                    .chars()
                    .filter(|c| !matches!(c, '\x1b' | '\x07'))
                    .collect();
                let intro = ["\x1b]", "\x1bP", "\x1b_G", "\x1b_Gi=7,p=1;"][kind];
                format!("{intro}{text}{end}").into_bytes()
            }),
        any::<char>().prop_map(|c| c.to_string().into_bytes()),
    ]
}

/// The length of each key that `decode` finds in `input`.
fn spans(input: &[u8]) -> Vec<usize> {
    let mut rest = input;
    let mut lengths = Vec::new();
    while !rest.is_empty() {
        let (_, length) = key(rest).unwrap_or_else(|| cut_off(rest));
        lengths.push(length);
        rest = &rest[length..];
    }
    lengths
}

proptest! {
    #![proptest_config(props_config())]

    #[test]
    fn decoding_any_bytes_does_not_panic(input in proptest::collection::vec(any::<u8>(), 0..200)) {
        decode(&input);
        let mut decoder = Decoder::default();
        for chunk in input.chunks(7) {
            decoder.keys(chunk);
        }
        decoder.flush();
    }

    #[test]
    fn reads_cut_anywhere_decode_as_the_whole(
        tokens in proptest::collection::vec(token(), 0..12),
        cuts in proptest::collection::vec(any::<usize>(), 0..6),
    ) {
        let input = tokens.concat();
        // The decoder keeps at most PENDING bytes of a cut-off key.
        prop_assume!(spans(&input).into_iter().all(|length| length <= PENDING));
        let mut cuts: Vec<usize> = cuts.into_iter().map(|c| c % (input.len() + 1)).collect();
        cuts.sort_unstable();
        let mut decoder = Decoder::default();
        let mut keys = Vec::new();
        let mut from = 0;
        for cut in cuts.into_iter().chain([input.len()]) {
            keys.extend(decoder.keys(&input[from..cut]));
            from = cut;
        }
        keys.extend(decoder.flush());
        prop_assert_eq!(keys, decode(&input));
    }
}

/// The known limit above `PENDING`, documented: a key longer than it, cut
/// by a read after more than `PENDING` bytes, is given up, and the rest of
/// it arrives as keys. Whole, the same bytes are one key.
#[test]
fn an_answer_cut_after_more_than_pending_bytes_is_given_up() {
    let answer = [
        &b"\x1b_Gi=7,p=1263096393;"[..],
        &[b'x'; 100],
        &b"\x1b\\"[..],
    ]
    .concat();
    assert!(answer.len() > PENDING);
    assert_eq!(decode(&answer).len(), 1);
    let mut decoder = Decoder::default();
    let mut keys = decoder.keys(&answer[..PENDING + 1]);
    keys.extend(decoder.keys(&answer[PENDING + 1..]));
    keys.extend(decoder.flush());
    assert_ne!(keys, decode(&answer));
    assert!(keys.len() > 1, "{keys:?}");
}
