//! Property tests of the reading of kitty's check answers: no bytes make it
//! panic, and answers are still read with unrelated text around them.

use proptest::prelude::*;

use super::*;
use crate::testing::props_config;

fn image(id: u32) -> ImageId {
    let answer = format!("\x1b_Gi={id},p=1263096393;OK\x1b\\");
    graphics::checked(answer.as_bytes()).unwrap().0
}

fn sent(ids: &[u32]) -> Vec<(ImageId, Cells)> {
    let cells = Cells {
        columns: 4,
        rows: 2,
    };
    ids.iter().map(|&id| (image(id), cells)).collect()
}

/// Text that is no part of an answer: no escape and no bell.
fn noise() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        any::<char>().prop_filter("no ESC or BEL", |c| !matches!(c, '\x1b' | '\x07')),
        0..12,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

proptest! {
    #![proptest_config(props_config())]

    #[test]
    fn reading_any_bytes_does_not_panic(
        bytes in proptest::collection::vec(any::<u8>(), 0..300),
    ) {
        lost(&bytes, &sent(&[1, 2, 3]));
        ended(&bytes);
        for sequence in sequences(&bytes) {
            prop_assert!(sequence.starts_with(b"\x1b_") && sequence.ends_with(b"\x1b\\"));
        }
    }

    #[test]
    fn answers_are_read_with_text_around_them(
        held in proptest::collection::vec(any::<bool>(), 1..6),
        last in any::<bool>(),
        before in noise(),
        between in noise(),
        after in noise(),
    ) {
        let ids: Vec<u32> = (1..=held.len() as u32).collect();
        let mut plain = String::new();
        let mut noisy = before.clone();
        for (id, &held) in ids.iter().zip(&held) {
            let answer = if held {
                format!("\x1b_Gi={id},p=1263096393;OK\x1b\\")
            } else {
                format!("\x1b_Gi={id},p=1263096393;ENOENT:Put command refers to non-existent image\x1b\\")
            };
            plain.push_str(&answer);
            noisy.push_str(&answer);
            noisy.push_str(&between);
        }
        if last {
            plain.push_str("\x1b_Gi=31;OK\x1b\\");
            noisy.push_str("\x1b_Gi=31;OK\x1b\\");
        }
        noisy.push_str(&after);
        let expected: Vec<ImageId> = ids
            .iter()
            .zip(&held)
            .filter(|(_, held)| !**held)
            .map(|(&id, _)| image(id))
            .collect();
        let sent = sent(&ids);
        prop_assert_eq!(lost(plain.as_bytes(), &sent), Some(expected.clone()));
        prop_assert_eq!(lost(noisy.as_bytes(), &sent), Some(expected));
    }
}
