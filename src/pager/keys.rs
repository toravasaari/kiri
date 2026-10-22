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
    /// Backspace, which terminals send as DEL or as Ctrl+H.
    Backspace,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    /// A character with Alt, which arrives after an escape character.
    Alt(char),
    /// A character typed after the colon that starts a command, as `n`
    /// in `:n`, which the pager takes as one key with it. The terminal
    /// sends no such key.
    Command(char),
    /// A sequence the pager does not use.
    Other,
}

/// What a key asks the pager to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Move(Motion),
    /// Opens the search prompt.
    Search,
    /// Moves to the next match of the last search.
    Next,
    /// Moves to the previous match of the last search.
    Previous,
    /// Stops showing the last search, which `n` takes up again.
    Clear,
    /// Moves to the start of the next file.
    NextFile,
    /// Moves to the start of the file the view is in, or of the one
    /// before when the view is there already.
    PreviousFile,
    Help,
    Quit,
}

impl Action {
    /// Whether the action moves between files, which only several files
    /// have.
    pub fn between_files(self) -> bool {
        matches!(self, Action::NextFile | Action::PreviousFile)
    }
}

/// The key that starts a command, as in `less`: the key after it is the
/// command's, as [`Key::Command`].
pub const COMMAND: Key = Key::Char(':');

/// The keys in `input`, all of it: a sequence cut off at the end is taken
/// as it is. A lone escape character is the Escape key, and one before a
/// character is Alt with it.
fn decode(input: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut rest = input;
    while !rest.is_empty() {
        let (key, length) = key(rest).unwrap_or_else(|| cut_off(rest));
        keys.push(key);
        rest = &rest[length..];
    }
    keys
}

/// The most bytes kept for a sequence that a read cut off; a longer one is
/// taken as it is, so that keys pressed after an answer that never ends
/// are not lost for good.
const PENDING: usize = 64;

/// Keys from reads of the terminal, which may cut a key's sequence in
/// two: the part at the end of one read waits for the rest in the next.
#[derive(Debug, Default)]
pub struct Decoder {
    pending: Vec<u8>,
}

impl Decoder {
    /// The keys in `input`, after what the last read left cut off.
    pub fn keys(&mut self, input: &[u8]) -> Vec<Key> {
        let mut bytes = std::mem::take(&mut self.pending);
        bytes.extend_from_slice(input);
        let mut keys = Vec::new();
        let mut rest = &bytes[..];
        while !rest.is_empty() {
            let Some((key, length)) =
                key(rest).or_else(|| (rest.len() > PENDING).then(|| cut_off(rest)))
            else {
                self.pending = rest.to_vec();
                break;
            };
            keys.push(key);
            rest = &rest[length..];
        }
        keys
    }

    /// What the last read left cut off, taken as it is, when no more came.
    pub fn flush(&mut self) -> Vec<Key> {
        decode(&std::mem::take(&mut self.pending))
    }
}

/// The key at the start of `input` and its length, or `None` when `input`
/// ends before the key does.
fn key(input: &[u8]) -> Option<(Key, usize)> {
    let byte = input[0];
    match byte {
        0x1b => escape(input),
        b'\r' | b'\n' => Some((Key::Enter, 1)),
        0x7f | 0x08 => Some((Key::Backspace, 1)),
        0x01..=0x1a => Some((Key::Ctrl(char::from(byte - 1 + b'a')), 1)),
        0x00..=0x1f => Some((Key::Other, 1)),
        _ => character(input),
    }
}

/// The key of a sequence that `input` cuts off, and its length.
fn cut_off(input: &[u8]) -> (Key, usize) {
    match input {
        [0x1b, b'[' | b']' | b'P' | b'_' | b'^' | b'X', ..] | [0x1b, b'O'] => {
            (Key::Other, input.len())
        }
        [0x1b, ..] => (Key::Escape, 1),
        _ => (Key::Other, 1),
    }
}

/// The key that starts with the escape character at the start of `input`,
/// and its length.
fn escape(input: &[u8]) -> Option<(Key, usize)> {
    let key = match input.get(1)? {
        b'[' => {
            // Parameters and intermediates up to the final byte.
            let body = &input[2..];
            let end = body.iter().position(|byte| (0x40..=0x7e).contains(byte))?;
            (csi(&body[..end], body[end]), end + 3)
        }
        b'O' => (csi(b"", *input.get(2)?), 3),
        // Strings the terminal sends as answers, such as a late answer to
        // the probe: OSC, DCS, APC, PM and SOS. They end with ST, or BEL
        // for OSC.
        b']' | b'P' | b'_' | b'^' | b'X' => {
            let body = &input[2..];
            let end = (0..body.len()).find_map(|i| match body[i..] {
                [0x07, ..] => Some(i + 1),
                [0x1b, b'\\', ..] => Some(i + 2),
                // Another sequence starts: this one was cut off.
                [0x1b, _, ..] => Some(i),
                _ => None,
            })?;
            (Key::Other, end + 2)
        }
        0x20..=0x7e | 0x80.. => match character(&input[1..])? {
            (Key::Char(c), length) => (Key::Alt(c), length + 1),
            _ => (Key::Escape, 1),
        },
        _ => (Key::Escape, 1),
    };
    Some(key)
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
fn character(input: &[u8]) -> Option<(Key, usize)> {
    let length = match input[0] {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    };
    let bytes = input.get(..length)?;
    let key = match std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.chars().next())
    {
        Some(c) => (Key::Char(c), length),
        None => (Key::Other, 1),
    };
    Some(key)
}

/// The keys of one action, as the help shows them.
#[derive(Debug)]
pub struct Binding {
    pub keys: &'static [Key],
    /// The keys most people use, shortest first.
    pub names: &'static str,
    /// What the keys do.
    pub what: &'static str,
    pub action: Action,
}

/// Every key the pager knows, in the order the help lists them.
pub const BINDINGS: &[Binding] = {
    use Key::{Char, Ctrl};
    use Motion::*;
    &[
        Binding {
            keys: &[
                Char('j'),
                Char('e'),
                Ctrl('e'),
                Ctrl('n'),
                Key::Enter,
                Key::Down,
            ],
            names: "j ↓ Enter",
            what: "down a line",
            action: Action::Move(Down(1)),
        },
        Binding {
            keys: &[
                Char('k'),
                Char('y'),
                Ctrl('y'),
                Ctrl('p'),
                Ctrl('k'),
                Key::Up,
            ],
            names: "k ↑",
            what: "up a line",
            action: Action::Move(Up(1)),
        },
        Binding {
            keys: &[Char(' '), Char('f'), Ctrl('f'), Ctrl('v'), Key::PageDown],
            names: "Space f PgDn",
            what: "down a page",
            action: Action::Move(PageDown),
        },
        Binding {
            keys: &[Char('b'), Ctrl('b'), Key::PageUp],
            names: "b PgUp",
            what: "up a page",
            action: Action::Move(PageUp),
        },
        Binding {
            keys: &[Char('d'), Ctrl('d')],
            names: "d",
            what: "down half a page",
            action: Action::Move(HalfPageDown),
        },
        Binding {
            keys: &[Char('u'), Ctrl('u')],
            names: "u",
            what: "up half a page",
            action: Action::Move(HalfPageUp),
        },
        Binding {
            keys: &[Char('g'), Char('<'), Key::Home],
            names: "g Home",
            what: "top",
            action: Action::Move(Start),
        },
        Binding {
            keys: &[Char('G'), Char('>'), Key::End],
            names: "G End",
            what: "bottom",
            action: Action::Move(End),
        },
        Binding {
            keys: &[Key::Command('n')],
            names: ":n",
            what: "next file",
            action: Action::NextFile,
        },
        Binding {
            keys: &[Key::Command('p')],
            names: ":p",
            what: "previous file",
            action: Action::PreviousFile,
        },
        Binding {
            keys: &[Char('/')],
            names: "/",
            what: "search",
            action: Action::Search,
        },
        Binding {
            keys: &[Char('n')],
            names: "n",
            what: "next match",
            action: Action::Next,
        },
        Binding {
            keys: &[Char('N')],
            names: "N",
            what: "previous match",
            action: Action::Previous,
        },
        Binding {
            keys: &[Key::Escape],
            names: "Esc",
            what: "hide the search",
            action: Action::Clear,
        },
        Binding {
            keys: &[Char('?'), Char('h')],
            names: "? h",
            what: "this help",
            action: Action::Help,
        },
        Binding {
            keys: &[Char('q'), Char('Q'), Ctrl('c')],
            names: "q",
            what: "quit",
            action: Action::Quit,
        },
    ]
};

/// What `key` does; `None` for a key without a binding.
pub fn action(key: Key) -> Option<Action> {
    BINDINGS
        .iter()
        .find(|binding| binding.keys.contains(&key))
        .map(|binding| binding.action)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn question_mark_and_h_open_the_help() {
        for key in [Key::Char('?'), Key::Char('h')] {
            assert_eq!(action(key), Some(Action::Help), "{key:?}");
        }
    }

    #[test]
    fn the_help_names_keys_of_their_own_binding() {
        for binding in BINDINGS {
            for name in binding.names.split(' ') {
                let key = match name {
                    "↓" => Key::Down,
                    "↑" => Key::Up,
                    "Enter" => Key::Enter,
                    "Esc" => Key::Escape,
                    "Space" => Key::Char(' '),
                    "PgDn" => Key::PageDown,
                    "PgUp" => Key::PageUp,
                    "Home" => Key::Home,
                    "End" => Key::End,
                    ":n" => Key::Command('n'),
                    ":p" => Key::Command('p'),
                    name if name.chars().count() == 1 => Key::Char(name.chars().next().unwrap()),
                    name => panic!("unknown key name {name}"),
                };
                assert!(binding.keys.contains(&key), "{name} in {}", binding.names);
            }
        }
        assert!(
            BINDINGS
                .iter()
                .any(|binding| binding.action == Action::Help)
        );
        assert!(
            BINDINGS
                .iter()
                .any(|binding| binding.action == Action::Quit)
        );
    }

    #[test]
    fn a_slash_searches_and_n_moves_between_matches() {
        assert_eq!(action(Key::Char('/')), Some(Action::Search));
        assert_eq!(action(Key::Char('n')), Some(Action::Next));
        assert_eq!(action(Key::Char('N')), Some(Action::Previous));
        assert_eq!(action(Key::Escape), Some(Action::Clear));
    }

    #[test]
    fn colon_n_and_colon_p_move_between_files() {
        assert_eq!(action(Key::Command('n')), Some(Action::NextFile));
        assert_eq!(action(Key::Command('p')), Some(Action::PreviousFile));
        // On their own, n and p are no commands.
        assert_eq!(action(Key::Char('p')), None);
        assert_eq!(action(COMMAND), None);
        assert_eq!(action(Key::Command('x')), None);
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        for key in [Key::Char('q'), Key::Char('Q'), Key::Ctrl('c')] {
            assert_eq!(action(key), Some(Action::Quit), "{key:?}");
        }
    }

    #[test]
    fn other_keys_do_nothing() {
        for key in [Key::Char('x'), Key::Other, Key::Ctrl('z')] {
            assert_eq!(action(key), None, "{key:?}");
        }
    }
}
