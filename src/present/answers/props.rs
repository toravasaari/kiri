//! Property tests of the answer decoder: no bytes make it panic, and a real
//! answer is still read with unrelated text before and after it.

use proptest::prelude::*;

use super::*;
use crate::term::testing::{
    KITTY_BEHIND_TMUX, SIXEL_FOOT_BEHIND_TMUX, TMUX_3_8, TMUX_3_8_SECOND_ROUND, XTERM_407,
    ghostty_1_1_3, kitty_0_47_1,
};
use crate::testing::props_config;

/// The answers of the terminals the probe has been measured on.
fn real_answers() -> Vec<String> {
    vec![
        kitty_0_47_1(false),
        kitty_0_47_1(true),
        ghostty_1_1_3(false),
        ghostty_1_1_3(true),
        XTERM_407.to_owned(),
        TMUX_3_8.to_owned(),
        SIXEL_FOOT_BEHIND_TMUX.to_owned(),
        [TMUX_3_8, KITTY_BEHIND_TMUX, TMUX_3_8_SECOND_ROUND].concat(),
    ]
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
    fn interpreting_any_bytes_does_not_panic(
        bytes in proptest::collection::vec(any::<u8>(), 0..300),
    ) {
        interpret(&bytes);
        device_attributes(&bytes);
        version(&String::from_utf8_lossy(&bytes));
    }

    /// Bytes that look like answers, so that the readers go further than
    /// random bytes take them.
    #[test]
    fn interpreting_pieces_of_answers_does_not_panic(
        pieces in proptest::collection::vec(
            proptest::sample::select(vec![
                "\x1b[", "\x1b]10;rgb:", "\x1b]4;", "\x1bP>|", "\x1b\\", "\x07", ";", "?", "c",
                "R", "S", "1", "4", "65535", "ffff", "/", "kitty(0.47.1)", "tmux 3.8", "\x1b_G",
                "i=31;OK", "t", "6", "9999999999", "$r", "38:2::1:2:3m",
            ]),
            0..30,
        ),
    ) {
        let text = pieces.concat();
        interpret(text.as_bytes());
        device_attributes(text.as_bytes());
        version(&text);
    }

    #[test]
    fn a_real_answer_is_read_with_text_around_it(
        which in 0..real_answers().len(),
        before in noise(),
        after in noise(),
    ) {
        let answer = &real_answers()[which];
        let around = format!("{before}{answer}{after}");
        prop_assert_eq!(interpret(around.as_bytes()), interpret(answer.as_bytes()));
        prop_assert_eq!(
            device_attributes(around.as_bytes()),
            device_attributes(answer.as_bytes())
        );
    }
}
