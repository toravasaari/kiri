//! Running inside tmux: kitty's graphics sequences reach kitty only
//! wrapped in tmux's passthrough, `ESC P tmux ; … ESC \` with every ESC
//! inside doubled.

use std::io::{self, Write};

/// A writer that wraps each kitty graphics sequence, `ESC _ G … ESC \`,
/// in tmux's passthrough on its way to `inner`, when told to, and passes
/// everything else untouched. A sequence may come over several writes, so
/// an ESC, or `ESC _`, at the end of a write waits for the next one.
pub struct Wrapping<W: Write> {
    inner: W,
    wrap: bool,
    state: State,
}

/// Where the bytes written so far leave a `Wrapping`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Outside any graphics sequence.
    Outside,
    /// After an ESC outside, held back until the next byte tells whether
    /// a graphics sequence starts.
    Esc,
    /// After `ESC _`, held back likewise.
    Apc,
    /// Inside a graphics sequence, already wrapped.
    Inside,
    /// After an ESC inside, which may end the sequence.
    InsideEsc,
}

const ESC: u8 = 0x1b;

impl<W: Write> Wrapping<W> {
    /// A writer to `inner` that wraps graphics sequences if `wrap`.
    pub fn new(inner: W, wrap: bool) -> Self {
        Self {
            inner,
            wrap,
            state: State::Outside,
        }
    }

    /// The writer this one writes to.
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for Wrapping<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.wrap {
            return self.inner.write(buf);
        }
        let mut out = Vec::with_capacity(buf.len() + 16);
        for &byte in buf {
            self.state = step(self.state, byte, &mut out);
        }
        self.inner.write_all(&out)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// The state after `byte` in `state`, with what it writes in `out`.
fn step(state: State, byte: u8, out: &mut Vec<u8>) -> State {
    match (state, byte) {
        (State::Outside, ESC) => State::Esc,
        (State::Outside, _) => {
            out.push(byte);
            State::Outside
        }
        (State::Esc, b'_') => State::Apc,
        (State::Esc, _) => {
            out.push(ESC);
            step(State::Outside, byte, out)
        }
        (State::Apc, b'G') => {
            out.extend_from_slice(b"\x1bPtmux;\x1b\x1b_G");
            State::Inside
        }
        (State::Apc, _) => {
            out.extend_from_slice(b"\x1b_");
            step(State::Outside, byte, out)
        }
        (State::Inside, ESC) => {
            out.extend_from_slice(&[ESC, ESC]);
            State::InsideEsc
        }
        (State::Inside, _) => {
            out.push(byte);
            State::Inside
        }
        (State::InsideEsc, b'\\') => {
            out.extend_from_slice(b"\\\x1b\\");
            State::Outside
        }
        (State::InsideEsc, _) => step(State::Inside, byte, out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `parts`, written one after another, come out as.
    fn wrapped(parts: &[&[u8]], wrap: bool) -> String {
        let mut out = Wrapping::new(Vec::new(), wrap);
        for part in parts {
            out.write_all(part).unwrap();
        }
        out.flush().unwrap();
        String::from_utf8(out.into_inner()).unwrap()
    }

    const GRAPHICS: &str = "a\x1b_Gi=1;AAAA\x1b\\b";
    const WRAPPED: &str = "a\x1bPtmux;\x1b\x1b_Gi=1;AAAA\x1b\x1b\\\x1b\\b";

    #[test]
    fn wraps_a_graphics_sequence_for_tmux() {
        assert_eq!(wrapped(&[GRAPHICS.as_bytes()], true), WRAPPED);
    }

    #[test]
    fn a_sequence_split_over_writes_is_wrapped_whole() {
        let bytes: Vec<&[u8]> = GRAPHICS.as_bytes().chunks(1).collect();
        assert_eq!(wrapped(&bytes, true), WRAPPED);
    }

    #[test]
    fn two_sequences_in_a_row_are_wrapped_each() {
        let two = "\x1b_Ga=d\x1b\\\x1b_Gi=2;B\x1b\\";
        let expected = "\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\x1b\\\
                        \x1bPtmux;\x1b\x1b_Gi=2;B\x1b\x1b\\\x1b\\";
        assert_eq!(wrapped(&[two.as_bytes()], true), expected);
    }

    #[test]
    fn other_escape_sequences_pass_untouched() {
        let other = "\x1b[1m\x1b]8;;u\x1b\\x\x1b\x1b_X\x1b\\";
        assert_eq!(wrapped(&[other.as_bytes()], true), other);
        let bytes: Vec<&[u8]> = other.as_bytes().chunks(1).collect();
        assert_eq!(wrapped(&bytes, true), other);
    }

    #[test]
    fn without_wrapping_nothing_changes() {
        assert_eq!(wrapped(&[GRAPHICS.as_bytes()], false), GRAPHICS);
    }
}
