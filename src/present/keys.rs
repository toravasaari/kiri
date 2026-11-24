//! Keys read from the terminal, and the `Decoder` that turns the bytes the
//! terminal sends into `Key` values.
//!
//! The lookup of a key's action is not here: what each key does is data in
//! `src/session/commands.rs`, which reads `Key` from this file.

use crate::present::kitty::graphics::{self, ImageId};
use crate::present::kitty::query;

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
    Left,
    Right,
    /// Left and Right with Alt, `CSI 1;3 D` and `CSI 1;3 C`, which xterm,
    /// kitty, Ghostty and tmux send. Other modified arrows are
    /// [`Key::Other`].
    AltLeft,
    AltRight,
    /// Shift+Tab; Tab itself arrives as Ctrl+I.
    BackTab,
    PageUp,
    PageDown,
    /// Page Up and Page Down with Shift, which move between files. Other
    /// keys with a modifier are [`Key::Other`].
    ShiftPageUp,
    ShiftPageDown,
    Home,
    End,
    /// A character with Alt, which arrives after an escape character.
    Alt(char),
    /// A character typed after the colon that starts a command, as `n`
    /// in `:n`, which the pager takes as one key with it. The terminal
    /// sends no such key.
    Command(char),
    /// The terminal tells that the window, or the tmux pane, has the
    /// focus again, `CSI I`, or lost it, `CSI O`, with focus reporting
    /// on. Not keys, so they leave the help and the prompt open.
    FocusIn,
    FocusOut,
    /// kitty's answer to a check of image `.0`: whether it holds it.
    /// Not a key either.
    Checked(ImageId, bool),
    /// The terminal's answer to the query for its cell size: a cell's
    /// width and height in pixels. Not a key either.
    CellSize(u16, u16),
    /// The mouse wheel turned a notch up. The key of a notch wherever the
    /// pointer was, as the table of commands names it; the terminal's
    /// notch is [`Key::Wheel`], which carries the place.
    WheelUp,
    /// The mouse wheel turned a notch down; see [`Key::WheelUp`].
    WheelDown,
    /// The mouse wheel turned a notch, down or up, with the pointer at a
    /// column and a row, counted from 1, and `ctrl` whether Ctrl was held.
    /// Shift makes it [`Key::Sideways`] instead.
    Wheel {
        down: bool,
        ctrl: bool,
        column: u16,
        row: u16,
    },
    /// The left button pressed at a column and a row, counted from 1.
    Click {
        column: u16,
        row: u16,
    },
    /// The mouse's back button, number 8, pressed.
    MouseBack,
    /// The mouse's forward button, number 9, pressed.
    MouseForward,
    /// A notch of the sideways wheel, to the right or to the left, or of
    /// the wheel with Shift, down or up, at a column and a row.
    Sideways {
        right: bool,
        shift: bool,
        column: u16,
        row: u16,
    },
    /// The mouse moved with the left button held, to a column and a row.
    Drag {
        column: u16,
        row: u16,
    },
    /// The left button let go at a column and a row.
    Release {
        column: u16,
        row: u16,
    },
    /// Any other mouse event, which the pager ignores.
    Mouse,
    /// A sequence the pager does not use.
    Other,
}

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
///
/// It is above the longest answer a terminal gives to a check, which kitty
/// words in about 112 bytes: the check at rest makes bursts of answers
/// ordinary, and a read may cut one anywhere.
const PENDING: usize = 256;

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
        // A mouse event without SGR encoding: the button, the column and
        // the row follow, a byte each, 32 more than their value.
        b'[' if input.get(2) == Some(&b'M') => {
            let bytes = input.get(3..6)?;
            let [button, column, row] = [0, 1, 2].map(|i| bytes[i].wrapping_sub(32));
            let at = (u16::from(column), u16::from(row));
            (mouse(Some(button.into()), true, at), 6)
        }
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
            let key = match graphics::checked(&input[..end + 2]) {
                Some((id, held)) => Key::Checked(id, held),
                None => Key::Other,
            };
            (key, end + 2)
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
        (b"", b'C') => Key::Right,
        (b"", b'D') => Key::Left,
        (b"1;3", b'D') => Key::AltLeft,
        (b"1;3", b'C') => Key::AltRight,
        (b"", b'Z') => Key::BackTab,
        (b"" | b"1", b'H') | (b"1" | b"7", b'~') => Key::Home,
        (b"" | b"1", b'F') | (b"4" | b"8", b'~') => Key::End,
        (b"5", b'~') => Key::PageUp,
        (b"6", b'~') => Key::PageDown,
        // The modifier 2 is Shift.
        (b"5;2", b'~') => Key::ShiftPageUp,
        (b"6;2", b'~') => Key::ShiftPageDown,
        (b"", b'I') => Key::FocusIn,
        (b"", b'O') => Key::FocusOut,
        // The terminal's answer to the cell size query, `6;height;width`.
        ([b'6', b';', rest @ ..], b't') => (std::str::from_utf8(rest).ok())
            .and_then(query::cell_size_answer)
            .map_or(Key::Other, |(width, height)| Key::CellSize(width, height)),
        // A mouse event in SGR encoding, `<` and the button, the column
        // and the row, pressed with `M` and released with `m`.
        ([b'<', rest @ ..], b'M' | b'm') => {
            let fields: Vec<u32> = (std::str::from_utf8(rest).unwrap_or(""))
                .split(';')
                .map(|field| field.parse().unwrap_or(0))
                .collect();
            let button = fields.first().copied();
            let at = |index: usize| {
                let value = fields.get(index).copied().unwrap_or(0);
                u16::try_from(value).unwrap_or(u16::MAX)
            };
            mouse(button, last == b'M', (at(1), at(2)))
        }
        _ => Key::Other,
    }
}

/// The key of a mouse event with the button code `button`, `pressed` or
/// released, at the column and row `at`: a notch of the wheel up or down,
/// with or without Alt or Ctrl, of the sideways wheel or of the wheel with
/// Shift, a press of the left button or of the back or the forward button,
/// or else [`Key::Mouse`].
fn mouse(button: Option<u32>, pressed: bool, (column, row): (u16, u16)) -> Key {
    let sideways = |right, shift| Key::Sideways {
        right,
        shift,
        column,
        row,
    };
    // Shift, Alt and Ctrl add 4, 8 and 16; the wheel's buttons are 64
    // and up. Shift is read first, so Ctrl with Shift stays the sideways
    // wheel. Ctrl is read for the wheel only; Alt does nothing in kiri and
    // is dropped, and Shift and Ctrl change no other button.
    let ctrl = button.is_some_and(|button| button & 16 != 0);
    let shift = button.is_some_and(|button| button & 4 != 0);
    match button.map(|button| button & !(4 | 8 | 16)) {
        Some(64) if pressed && shift => sideways(false, true),
        Some(65) if pressed && shift => sideways(true, true),
        Some(64) if pressed => Key::Wheel {
            down: false,
            ctrl,
            column,
            row,
        },
        Some(65) if pressed => Key::Wheel {
            down: true,
            ctrl,
            column,
            row,
        },
        Some(66) if pressed => sideways(false, false),
        Some(67) if pressed => sideways(true, false),
        Some(0) if pressed => Key::Click { column, row },
        Some(0) => Key::Release { column, row },
        Some(32) => Key::Drag { column, row },
        Some(128) if pressed => Key::MouseBack,
        Some(129) if pressed => Key::MouseForward,
        _ => Key::Mouse,
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

#[cfg(test)]
mod props;

#[cfg(test)]
mod tests;
