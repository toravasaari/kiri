use super::*;

/// A notch of the wheel at `column` and `row`.
fn wheel(down: bool, column: u16, row: u16) -> Key {
    Key::Wheel {
        down,
        ctrl: false,
        column,
        row,
    }
}

/// A notch of the wheel with Ctrl held.
fn ctrl_wheel(down: bool, column: u16, row: u16) -> Key {
    Key::Wheel {
        down,
        ctrl: true,
        column,
        row,
    }
}

#[test]
fn decodes_characters_and_control_keys() {
    assert_eq!(
        decode("jä\r\n\x06\x01".as_bytes()),
        [
            Key::Char('j'),
            Key::Char('ä'),
            Key::Enter,
            Key::Enter,
            Key::Ctrl('f'),
            Key::Ctrl('a'),
        ]
    );
    assert_eq!(decode(b"\x00\x1f"), [Key::Other, Key::Other]);
}

#[test]
fn decodes_alt_left_and_alt_right_and_leaves_other_modified_arrows() {
    assert_eq!(decode(b"\x1b[1;3D\x1b[1;3C"), [Key::AltLeft, Key::AltRight]);
    assert_eq!(
        decode(b"\x1b[1;5D\x1b[1;2C\x1b[1;3A"),
        [Key::Other, Key::Other, Key::Other]
    );
}

#[test]
fn decodes_the_back_and_forward_buttons_of_the_mouse_in_both_encodings() {
    // SGR: buttons 8 and 9 are the codes 128 and 129; a release is
    // not an event, and Shift, Alt and Ctrl are masked off.
    assert_eq!(
        decode(b"\x1b[<128;3;4M\x1b[<129;3;4M"),
        [Key::MouseBack, Key::MouseForward]
    );
    assert_eq!(
        decode(b"\x1b[<128;3;4m\x1b[<129;3;4m"),
        [Key::Mouse, Key::Mouse]
    );
    assert_eq!(
        decode(b"\x1b[<132;3;4M\x1b[<137;3;4M\x1b[<145;3;4M"),
        [Key::MouseBack, Key::MouseForward, Key::MouseForward]
    );
    // Without SGR the code is a byte, 32 more.
    assert_eq!(
        decode(b"\x1b[M\xa0#$\x1b[M\xa1#$"),
        [Key::MouseBack, Key::MouseForward]
    );
}

#[test]
fn decodes_backspace_in_both_forms() {
    assert_eq!(decode(b"\x7f\x08"), [Key::Backspace, Key::Backspace]);
}

#[test]
fn decodes_cursor_and_editing_keys_in_both_forms() {
    let keys = decode(b"\x1b[A\x1b[B\x1bOA\x1bOB\x1b[5~\x1b[6~\x1b[H\x1b[F\x1b[1~\x1b[4~");
    assert_eq!(
        keys,
        [
            Key::Up,
            Key::Down,
            Key::Up,
            Key::Down,
            Key::PageUp,
            Key::PageDown,
            Key::Home,
            Key::End,
            Key::Home,
            Key::End,
        ]
    );
    assert_eq!(decode(b"\x1b[7~\x1b[8~"), [Key::Home, Key::End]);
}

#[test]
fn decodes_page_up_and_page_down_with_shift() {
    assert_eq!(
        decode(b"\x1b[5;2~\x1b[6;2~"),
        [Key::ShiftPageUp, Key::ShiftPageDown]
    );
    // With Ctrl, or Alt, they are still no keys of the pager's.
    assert_eq!(decode(b"\x1b[5;5~\x1b[6;3~"), [Key::Other, Key::Other]);
}

#[test]
fn decodes_ctrl_l() {
    assert_eq!(decode(b"\x0c"), [Key::Ctrl('l')]);
}

#[test]
fn decodes_kittys_answers_to_checks() {
    use crate::present::kitty::graphics::checked;

    let held = b"\x1b_Gi=7,p=1263096393;OK\x1b\\";
    let lost = b"\x1b_Gi=8,p=1263096393;ENOENT:Put command refers to non-existent image\x1b\\";
    let (seven, eight) = (checked(held).unwrap().0, checked(lost).unwrap().0);
    let input = [&held[..], b"j", lost, b"\x1b_Gi=31;OK\x1b\\"].concat();
    assert_eq!(
        decode(&input),
        [
            Key::Checked(seven, true),
            Key::Char('j'),
            Key::Checked(eight, false),
            Key::Other,
        ]
    );
    // Cut in two by the reads.
    let mut decoder = Decoder::default();
    assert_eq!(decoder.keys(&lost[..20]), []);
    assert_eq!(decoder.keys(&lost[20..]), [Key::Checked(eight, false)]);
}

#[test]
fn kittys_answer_of_112_bytes_cut_anywhere_by_a_read_is_one_key() {
    // The message is as long as kitty's, which makes the answer 112
    // bytes.
    let start = "\x1b_Gi=4242,p=1263096393;ENOENT:Put command refers to non-existent image";
    let message = format!("{start} with id: 4242");
    let answer = format!("{message:_<110}\x1b\\").into_bytes();
    assert_eq!(answer.len(), 112, "{answer:?}");
    let id = crate::present::kitty::graphics::checked(&answer);
    let Some((id, false)) = id else {
        panic!("not an answer: {answer:?}")
    };
    for cut in 0..=answer.len() {
        let mut decoder = Decoder::default();
        let mut keys = decoder.keys(&answer[..cut]);
        keys.extend(decoder.keys(&answer[cut..]));
        assert_eq!(keys, [Key::Checked(id, false)], "cut at {cut}");
    }
}

#[test]
fn decodes_the_terminal_s_answer_to_the_cell_size_query() {
    // The answer has the height first, as the query's does.
    assert_eq!(
        decode(b"\x1b[6;24;10tj"),
        [Key::CellSize(10, 24), Key::Char('j')]
    );
    let mut decoder = Decoder::default();
    assert_eq!(decoder.keys(b"\x1b[6;24"), []);
    assert_eq!(decoder.keys(b";10t"), [Key::CellSize(10, 24)]);
}

#[test]
fn other_sequences_that_end_in_t_or_start_with_6_are_not_a_cell_size() {
    // The window size in characters and the query itself, an answer
    // of no size, and keys that have a 6.
    assert_eq!(
        decode(b"\x1b[8;24;80t\x1b[16t\x1b[6;0;0t\x1b[6;24t\x1b[6~\x1b[6;2~"),
        [
            Key::Other,
            Key::Other,
            Key::Other,
            Key::Other,
            Key::PageDown,
            Key::ShiftPageDown
        ]
    );
}

#[test]
fn decodes_the_focus_coming_back() {
    assert_eq!(decode(b"\x1b[I\x1b[O"), [Key::FocusIn, Key::FocusOut]);
}

#[test]
fn decodes_the_sideways_arrows_and_shift_tab() {
    assert_eq!(
        decode(b"\x1b[D\x1b[C\x1b[Z\t"),
        [Key::Left, Key::Right, Key::BackTab, Key::Ctrl('i')]
    );
}

#[test]
fn unknown_sequences_are_one_key_each() {
    // F5, Shift+Up and Ctrl+Right, each followed by a letter.
    assert_eq!(
        decode(b"\x1b[15~q\x1b[1;2Aq\x1b[1;5Cq"),
        [
            Key::Other,
            Key::Char('q'),
            Key::Other,
            Key::Char('q'),
            Key::Other,
            Key::Char('q'),
        ]
    );
}

#[test]
fn decodes_the_mouse_wheel() {
    // Up and down at a column and a row, then with Ctrl.
    assert_eq!(
        decode(b"\x1b[<64;10;5M\x1b[<65;10;5M\x1b[<80;1;1M\x1b[<81;2;3M"),
        [
            wheel(false, 10, 5),
            wheel(true, 10, 5),
            ctrl_wheel(false, 1, 1),
            ctrl_wheel(true, 2, 3),
        ]
    );
    // Alt adds 8 and changes nothing; Ctrl with Alt is still Ctrl.
    assert_eq!(
        decode(b"\x1b[<72;1;1M\x1b[<88;1;1M"),
        [wheel(false, 1, 1), ctrl_wheel(false, 1, 1)]
    );
    // Without SGR encoding: three bytes after the sequence, the
    // button and the column and row, each 32 more than their value.
    assert_eq!(
        decode(b"\x1b[M`*%\x1b[Ma*%"),
        [wheel(false, 10, 5), wheel(true, 10, 5)]
    );
    // The same with Ctrl: 64 + 16 is 80, and 80 + 32 is `p`; 81 is `q`.
    assert_eq!(
        decode(b"\x1b[Mp*%\x1b[Mq*%"),
        [ctrl_wheel(false, 10, 5), ctrl_wheel(true, 10, 5)]
    );
}

#[test]
fn the_wheel_with_ctrl_and_shift_stays_the_sideways_wheel() {
    let sideways = |right| Key::Sideways {
        right,
        shift: true,
        column: 3,
        row: 4,
    };
    // 64 + 4 + 16 and 65 + 4 + 16.
    assert_eq!(
        decode(b"\x1b[<84;3;4M\x1b[<85;3;4M"),
        [sideways(false), sideways(true)]
    );
}

#[test]
fn decodes_a_click_and_the_sideways_wheel_with_where_they_are() {
    let keys = decode(b"\x1b[<0;10;5M\x1b[<66;3;4M\x1b[<67;3;4M\x1b[<69;7;8M\x1b[M -%");
    let sideways = |right, shift, column, row| Key::Sideways {
        right,
        shift,
        column,
        row,
    };
    assert_eq!(
        keys,
        [
            Key::Click { column: 10, row: 5 },
            sideways(false, false, 3, 4),
            sideways(true, false, 3, 4),
            sideways(true, true, 7, 8),
            Key::Click { column: 13, row: 5 },
        ]
    );
}

#[test]
fn other_mouse_events_are_no_keys() {
    // A right click and the middle button's release, each followed by
    // a letter.
    let keys = decode(b"\x1b[<2;200;99Mq\x1b[<1;10;5mq");
    assert_eq!(
        keys,
        [Key::Mouse, Key::Char('q'), Key::Mouse, Key::Char('q')]
    );
    // Without SGR encoding the column and row could be read as
    // keys, as a space and a q here.
    assert_eq!(decode(b"\x1b[M#qq"), [Key::Mouse]);
}

#[test]
fn decodes_the_left_button_held_while_it_moves_and_its_release_with_where() {
    let keys = decode(b"\x1b[<32;11;5M\x1b[<0;12;6m\x1b[<36;1;1M");
    assert_eq!(
        keys,
        [
            Key::Drag { column: 11, row: 5 },
            Key::Release { column: 12, row: 6 },
            // Shift, Alt and Ctrl are left out of the button, as for the wheel.
            Key::Drag { column: 1, row: 1 },
        ]
    );
}

#[test]
fn a_mouse_event_split_over_two_reads_is_one_key() {
    let mut decoder = Decoder::default();
    assert_eq!(decoder.keys(b"\x1b[<64;1"), []);
    assert_eq!(decoder.keys(b"0;5M"), [wheel(false, 10, 5)]);
    assert_eq!(decoder.keys(b"\x1b[M "), []);
    // A press of the left button, at column and row 81.
    let click = Key::Click {
        column: 81,
        row: 81,
    };
    assert_eq!(decoder.keys(b"qqj"), [click, Key::Char('j')]);
}

#[test]
fn a_lone_escape_is_the_escape_key() {
    assert_eq!(decode(b"\x1b"), [Key::Escape]);
}

#[test]
fn escape_before_a_character_is_alt() {
    assert_eq!(decode(b"\x1bj"), [Key::Alt('j')]);
    assert_eq!(decode("\x1bä".as_bytes()), [Key::Alt('ä')]);
}

#[test]
fn terminal_answers_are_no_keys() {
    // A late answer to the probe, ended by ST or BEL, then a key.
    let keys = decode(b"\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\q\x1b]4;1;rgb:cc/00/00\x07q");
    assert_eq!(
        keys,
        [Key::Other, Key::Char('q'), Key::Other, Key::Char('q')]
    );
    assert_eq!(
        decode(b"\x1bP>|kitty(0.47.1)\x1b\\j"),
        [Key::Other, Key::Char('j')]
    );
    // Cut off at the end of the read.
    assert_eq!(decode(b"\x1b]11;rgb:1e"), [Key::Other]);
}

#[test]
fn a_sequence_cut_off_at_the_end_is_one_key() {
    assert_eq!(decode(b"\x1b[1;"), [Key::Other]);
    assert_eq!(decode(b"\x1bO"), [Key::Other]);
}

#[test]
fn a_sequence_split_over_two_reads_is_one_key() {
    let mut decoder = Decoder::default();
    assert_eq!(decoder.keys(b"j\x1b["), [Key::Char('j')]);
    assert_eq!(decoder.keys(b"6~k"), [Key::PageDown, Key::Char('k')]);
    assert_eq!(decoder.keys(b"\x1b"), []);
    assert_eq!(decoder.keys(b"OA"), [Key::Up]);
    assert_eq!(decoder.keys(&"ä".as_bytes()[..1]), []);
    assert_eq!(decoder.keys(&"ä".as_bytes()[1..]), [Key::Char('ä')]);
    // A terminal answer in three reads.
    assert_eq!(decoder.keys(b"\x1b]11;rgb:1e"), []);
    assert_eq!(decoder.keys(b"1e/1e1e"), []);
    assert_eq!(decoder.keys(b"/2e2e\x1b\\q"), [Key::Other, Key::Char('q')]);
    assert_eq!(decoder.flush(), []);
}

#[test]
fn a_new_sequence_ends_a_cut_off_answer() {
    let mut decoder = Decoder::default();
    assert_eq!(decoder.keys(b"\x1b]11;"), []);
    assert_eq!(
        decoder.keys(b"\x1b[Aq"),
        [Key::Other, Key::Up, Key::Char('q')]
    );
}

#[test]
fn a_cut_off_answer_waits_for_its_end_only_so_long() {
    let mut decoder = Decoder::default();
    assert_eq!(decoder.keys(b"\x1b]"), []);
    let mut keys = Vec::new();
    for _ in 0..PENDING {
        keys.extend(decoder.keys(b"q"));
    }
    // The answer is given up, and the keys after it are keys again.
    assert_eq!(keys.first(), Some(&Key::Other), "{keys:?}");
    assert!(
        keys[1..].iter().all(|&key| key == Key::Char('q')),
        "{keys:?}"
    );
    assert_eq!(decoder.keys(b"j"), [Key::Char('j')]);
}

#[test]
fn a_cut_off_sequence_without_a_rest_is_taken_as_it_is() {
    let mut decoder = Decoder::default();
    assert_eq!(decoder.keys(b"\x1b"), []);
    assert_eq!(decoder.flush(), [Key::Escape]);
    assert_eq!(decoder.keys(b"\x1b[1;"), []);
    assert_eq!(decoder.flush(), [Key::Other]);
    assert_eq!(decoder.flush(), []);
}

#[test]
fn invalid_utf_8_is_skipped() {
    assert_eq!(decode(b"\xffj"), [Key::Other, Key::Char('j')]);
}
