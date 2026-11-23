//! Running inside tmux: kitty's graphics sequences reach kitty only wrapped in
//! tmux's passthrough, `ESC P tmux ; … ESC \` with every ESC inside doubled.
//!
//! This module only wraps the bytes: `passthrough`, `Wrapping`, which wraps a
//! stream of bytes as it is written, and `Pane`, the plain data of the pane kiri
//! runs in. Running `tmux` and asking it what it knows is in `src/term/tmux.rs`,
//! which passes the run id and the pane in as values.

use std::io::{self, Write};

use crate::present::kitty::graphics;

/// The start of tmux's passthrough: a DCS for tmux.
const OPEN: &str = "\x1bPtmux;";

/// The most output one passthrough takes before it is closed and the next
/// graphics sequence opens a new one: 512 KiB, checked at the end of a
/// sequence. Two limits set it. tmux drops a whole sequence that does not
/// fit its input buffer, which is at least 1 MB in every tmux (fixed up to
/// 3.5a; from 3.6 the server option `input-buffer-size`, with 1 MB as both
/// default and minimum), and kiri's sequences are at most about 4 KB, so
/// this stays far under it. And Ghostty on Wayland makes pointer requests
/// for each passthrough, because tmux switches mouse reporting off and on
/// after each one, and a burst of them exits Ghostty when the compositor
/// stalls (B24), so the passthroughs should be few.
const BUDGET: usize = 512 * 1024;

/// `sequence` wrapped in tmux's passthrough, whatever it is.
pub fn passthrough(sequence: &str) -> String {
    format!("{OPEN}{}\x1b\\", sequence.replace('\x1b', "\x1b\x1b"))
}

/// What kiri learnt from tmux about the pane it runs in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pane {
    /// The terminal outside tmux, as tmux's `client_termtype` names it,
    /// such as `kitty(0.47.1)`.
    pub client: Option<String>,
    /// The pane's `allow-passthrough` as kiri found it: `off`, `on` or
    /// `all`.
    pub passthrough: Option<String>,
    /// Whether kiri turned the pane's `allow-passthrough` on.
    pub turned_on: bool,
    /// tmux's `focus-events`, which tells the pager when the pane is
    /// shown again; `None` when tmux did not tell.
    pub focus_events: Option<bool>,
    /// Whether tmux passes OSC 8 links on to the terminal outside: the
    /// `hyperlinks` feature among `client_termfeatures`; `None` when tmux
    /// did not tell.
    pub hyperlinks: Option<bool>,
    /// The terminal outside tmux as its `TERM` names it, tmux's
    /// `client_termname`, such as `xterm-kitty`: the name to give
    /// `terminal-features` in tmux.conf.
    pub term_name: Option<String>,
}

/// A writer that wraps kitty graphics sequences, [`graphics::START`]
/// (`ESC _ G`) to `ESC \`, in tmux's passthrough on its way to `inner`,
/// when told to, and passes everything else untouched.
///
/// Sequences that follow each other share one passthrough, as tmux
/// switches mouse reporting off and on after each (see [`BUDGET`]). The
/// passthrough is closed at the first byte that is not part of a graphics
/// sequence, when it has taken [`BUDGET`] bytes (checked at the end of a
/// sequence, so the next one opens a new passthrough), and at the end of
/// every write that is not in the middle of a sequence: nothing stays open
/// between writes, so what another writer sends straight to the terminal,
/// as the check of the images sent (`src/term/check.rs`) does, does not end
/// up inside it.
///
/// A sequence may come over several writes, and its passthrough stays open
/// to its end. The start of one at the end of a write, such as an ESC or
/// `ESC _`, waits for the next write. A flush does not release it: the
/// bytes that follow decide whether it is wrapped, and kiri writes every
/// other sequence whole.
pub struct Wrapping<W: Write> {
    inner: W,
    wrap: bool,
    state: State,
    /// Whether a passthrough is open while `state` is not inside a
    /// sequence: the previous sequence ended and the next byte decides
    /// whether it carries on.
    open: bool,
    /// The bytes written inside the open passthrough, after its start.
    used: usize,
}

/// Where the bytes written so far leave a `Wrapping`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Outside any graphics sequence.
    Outside,
    /// After this many bytes of [`graphics::START`] outside, held back
    /// until the next byte tells whether a graphics sequence starts.
    Start(usize),
    /// Inside a graphics sequence, already wrapped.
    Inside,
    /// After an ESC inside, which may end the sequence.
    InsideEsc,
}

const ESC: u8 = 0x1b;

/// The end of tmux's passthrough: ST.
const CLOSE: &[u8] = b"\x1b\\";

impl<W: Write> Wrapping<W> {
    /// A writer to `inner` that wraps graphics sequences if `wrap`.
    pub fn new(inner: W, wrap: bool) -> Self {
        Self {
            inner,
            wrap,
            state: State::Outside,
            open: false,
            used: 0,
        }
    }

    /// The writer this one writes to.
    pub fn into_inner(self) -> W {
        self.inner
    }

    /// Closes the open passthrough, if any, in `out`.
    fn close(&mut self, out: &mut Vec<u8>) {
        if self.open {
            out.extend_from_slice(CLOSE);
            self.open = false;
        }
    }

    /// Takes `byte`, with what it writes in `out`.
    fn step(&mut self, byte: u8, out: &mut Vec<u8>) {
        let start = graphics::START.as_bytes();
        match (self.state, byte) {
            (State::Outside, _) if byte == start[0] => self.state = State::Start(1),
            (State::Outside, _) => {
                self.close(out);
                out.push(byte);
            }
            (State::Start(held), _) if byte == start[held] && held + 1 == start.len() => {
                if !self.open {
                    out.extend_from_slice(OPEN.as_bytes());
                    self.open = true;
                    self.used = 0;
                }
                let first = graphics::START.replace('\x1b', "\x1b\x1b");
                out.extend_from_slice(first.as_bytes());
                self.used += first.len();
                self.state = State::Inside;
            }
            (State::Start(held), _) if byte == start[held] => self.state = State::Start(held + 1),
            (State::Start(held), _) => {
                self.close(out);
                out.extend_from_slice(&start[..held]);
                self.state = State::Outside;
                self.step(byte, out);
            }
            (State::Inside, ESC) => {
                out.extend_from_slice(&[ESC, ESC]);
                self.used += 2;
                self.state = State::InsideEsc;
            }
            (State::Inside, _) => {
                out.push(byte);
                self.used += 1;
            }
            (State::InsideEsc, b'\\') => {
                out.push(b'\\');
                self.used += 1;
                self.state = State::Outside;
                if self.used >= BUDGET {
                    self.close(out);
                }
            }
            (State::InsideEsc, _) => {
                self.state = State::Inside;
                self.step(byte, out);
            }
        }
    }
}

impl<W: Write> Write for Wrapping<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.wrap {
            return self.inner.write(buf);
        }
        let mut out = Vec::with_capacity(buf.len() + 16);
        for &byte in buf {
            self.step(byte, &mut out);
        }
        // Between sequences nothing may stay open, also with the start of
        // one held back: it opens a passthrough of its own if it comes.
        if matches!(self.state, State::Outside | State::Start(_)) {
            self.close(&mut out);
        }
        self.inner.write_all(&out)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests;
