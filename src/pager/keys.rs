//! Keys read from the terminal, and what each does in the pager. The
//! bindings follow `less` for now.

use super::view::Motion;

/// A key as the terminal sends it without kitty's keyboard protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    /// A letter with Ctrl, as its lower-case letter.
    Ctrl(char),
    Enter,
    Escape,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    /// A character with Alt, which arrives after an escape character.
    Alt(char),
    /// A sequence the pager does not use.
    Other,
}

/// What a key asks the pager to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Move(Motion),
    Quit,
}

/// The keys in `input`. Terminals write each key's sequence whole, so a
/// sequence cut off at the end is taken as it is: a lone escape character
/// is the Escape key, and one before a character is Alt with it.
pub fn decode(input: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut rest = input;
    while let Some(&byte) = rest.first() {
        let (key, length) = match byte {
            0x1b => escape(rest),
            b'\r' | b'\n' => (Key::Enter, 1),
            0x01..=0x1a => (Key::Ctrl(char::from(byte - 1 + b'a')), 1),
            0x00..=0x1f | 0x7f => (Key::Other, 1),
            _ => character(rest),
        };
        keys.push(key);
        rest = &rest[length..];
    }
    keys
}

/// The key that starts with the escape character at the start of `input`,
/// and its length.
fn escape(input: &[u8]) -> (Key, usize) {
    match input.get(1) {
        Some(b'[') => {
            // Parameters and intermediates up to the final byte.
            let body = &input[2..];
            match body.iter().position(|byte| (0x40..=0x7e).contains(byte)) {
                Some(end) => (csi(&body[..end], body[end]), end + 3),
                None => (Key::Other, input.len()),
            }
        }
        Some(b'O') => match input.get(2) {
            Some(&last) => (csi(b"", last), 3),
            None => (Key::Other, 2),
        },
        // Strings the terminal sends as answers, such as a late answer to
        // the probe: OSC, DCS, APC, PM and SOS. They end with ST, or BEL
        // for OSC.
        Some(b']' | b'P' | b'_' | b'^' | b'X') => {
            let body = &input[2..];
            let end = (0..body.len()).find_map(|i| match body[i..] {
                [0x07, ..] => Some(i + 1),
                [0x1b, b'\\', ..] => Some(i + 2),
                _ => None,
            });
            (Key::Other, end.map_or(input.len(), |end| end + 2))
        }
        Some(0x20..=0x7e | 0x80..) => match character(&input[1..]) {
            (Key::Char(c), length) => (Key::Alt(c), length + 1),
            _ => (Key::Escape, 1),
        },
        _ => (Key::Escape, 1),
    }
}

/// The key of a control sequence with `parameters` and the final byte
/// `last`.
fn csi(parameters: &[u8], last: u8) -> Key {
    match (parameters, last) {
        (b"", b'A') => Key::Up,
        (b"", b'B') => Key::Down,
        (b"" | b"1", b'H') | (b"1" | b"7", b'~') => Key::Home,
        (b"" | b"1", b'F') | (b"4" | b"8", b'~') => Key::End,
        (b"5", b'~') => Key::PageUp,
        (b"6", b'~') => Key::PageDown,
        _ => Key::Other,
    }
}

/// The character at the start of `input`, and its length in bytes.
fn character(input: &[u8]) -> (Key, usize) {
    let length = match input[0] {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    };
    let bytes = &input[..length.min(input.len())];
    match std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.chars().next())
    {
        Some(c) => (Key::Char(c), bytes.len()),
        None => (Key::Other, 1),
    }
}

/// What `key` does; `None` for a key without a binding.
pub fn action(key: Key) -> Option<Action> {
    use Key::{Char, Ctrl};
    let motion = match key {
        Char('j' | 'e') | Ctrl('e' | 'n') | Key::Enter | Key::Down => Motion::Down(1),
        Char('k' | 'y') | Ctrl('y' | 'p' | 'k') | Key::Up => Motion::Up(1),
        Char(' ' | 'f') | Ctrl('f' | 'v') | Key::PageDown => Motion::PageDown,
        Char('b') | Ctrl('b') | Key::PageUp => Motion::PageUp,
        Char('d') | Ctrl('d') => Motion::HalfPageDown,
        Char('u') | Ctrl('u') => Motion::HalfPageUp,
        Char('g' | '<') | Key::Home => Motion::Start,
        Char('G' | '>') | Key::End => Motion::End,
        Char('q' | 'Q') | Ctrl('c') => return Some(Action::Quit),
        _ => return None,
    };
    Some(Action::Move(motion))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_characters_and_control_keys() {
        assert_eq!(
            decode("jä\r\n\x06\x7f".as_bytes()),
            [
                Key::Char('j'),
                Key::Char('ä'),
                Key::Enter,
                Key::Enter,
                Key::Ctrl('f'),
                Key::Other,
            ]
        );
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
    fn unknown_sequences_are_one_key_each() {
        // F5, Shift+Up and a focus event, each followed by a letter.
        assert_eq!(
            decode(b"\x1b[15~q\x1b[1;2Aq\x1b[Iq"),
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
    fn a_lone_escape_is_the_escape_key() {
        assert_eq!(decode(b"\x1b"), [Key::Escape]);
    }

    #[test]
    fn escape_before_a_character_is_alt() {
        assert_eq!(decode(b"\x1bj"), [Key::Alt('j')]);
        assert_eq!(decode("\x1bä".as_bytes()), [Key::Alt('ä')]);
        assert_eq!(action(Key::Alt('q')), None);
        assert_eq!(action(Key::Alt('j')), None);
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
    fn invalid_utf_8_is_skipped() {
        assert_eq!(decode(b"\xffj"), [Key::Other, Key::Char('j')]);
    }

    #[test]
    fn keys_move_as_in_less() {
        let moves = |keys: &[Key], motion| {
            for &key in keys {
                assert_eq!(action(key), Some(Action::Move(motion)), "{key:?}");
            }
        };
        let down = [
            Key::Char('j'),
            Key::Char('e'),
            Key::Ctrl('e'),
            Key::Ctrl('n'),
            Key::Enter,
            Key::Down,
        ];
        moves(&down, Motion::Down(1));
        let up = [
            Key::Char('k'),
            Key::Char('y'),
            Key::Ctrl('y'),
            Key::Ctrl('p'),
            Key::Ctrl('k'),
            Key::Up,
        ];
        moves(&up, Motion::Up(1));
        let page_down = [
            Key::Char(' '),
            Key::Char('f'),
            Key::Ctrl('f'),
            Key::Ctrl('v'),
            Key::PageDown,
        ];
        moves(&page_down, Motion::PageDown);
        moves(
            &[Key::Char('b'), Key::Ctrl('b'), Key::PageUp],
            Motion::PageUp,
        );
        moves(&[Key::Char('d'), Key::Ctrl('d')], Motion::HalfPageDown);
        moves(&[Key::Char('u'), Key::Ctrl('u')], Motion::HalfPageUp);
        moves(&[Key::Char('g'), Key::Char('<'), Key::Home], Motion::Start);
        moves(&[Key::Char('G'), Key::Char('>'), Key::End], Motion::End);
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        for key in [Key::Char('q'), Key::Char('Q'), Key::Ctrl('c')] {
            assert_eq!(action(key), Some(Action::Quit), "{key:?}");
        }
    }

    #[test]
    fn other_keys_do_nothing() {
        for key in [Key::Char('x'), Key::Escape, Key::Other, Key::Ctrl('z')] {
            assert_eq!(action(key), None, "{key:?}");
        }
    }
}
