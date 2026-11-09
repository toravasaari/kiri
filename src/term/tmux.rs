//! Running inside tmux: kitty's graphics sequences reach kitty only
//! wrapped in tmux's passthrough, `ESC P tmux ; … ESC \` with every ESC
//! inside doubled.

use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::config::Environment;
use crate::present::kitty::graphics;

/// The start of tmux's passthrough: a DCS for tmux.
const OPEN: &str = "\x1bPtmux;";

/// The most output one passthrough takes before it is closed and the next
/// graphics sequence opens a new one: 512 KiB, checked at the end of a
/// sequence. Two limits set it. tmux drops a whole sequence that does not
/// fit its input buffer, which is at least 1 MB in every tmux (fixed up to
/// 3.4; from 3.6 the server option `input-buffer-size`, with 1 MB as both
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

/// Runs `tmux` with the arguments: its output, trimmed, when it succeeds,
/// and `None` when it fails or cannot run.
pub type Run<'a> = &'a dyn Fn(&[&str]) -> Option<String>;

/// Runs the `tmux` on `PATH`, as [`Run`] says.
pub fn run(args: &[&str]) -> Option<String> {
    let out = Command::new("tmux")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    out.status.success().then(|| text.trim().to_owned())
}

/// Something unique to this run of kiri, to mix into its image ids: every
/// pane of a kitty window shares kitty's images, and kiri runs in other
/// panes, also in other containers, whose process ids may be the same.
pub fn run_id() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    // Each `RandomState` has keys of its own, random for each process.
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    hasher.write_u128(now.map_or(0, |now| now.as_nanos()));
    hasher.finish()
}

/// Whether kiri runs inside tmux: `TMUX` set and not empty.
pub fn in_tmux(env: &Environment) -> bool {
    env.tmux.as_ref().is_some_and(|value| !value.is_empty())
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

/// Asks tmux through `run` for the outer terminal, the pane's
/// `allow-passthrough`, `focus-events` and whether tmux passes links, and
/// turns `allow-passthrough` on when it is `off`, as
/// `kitten icat` does; `on` and `all` stay as they are. The pane is
/// `TMUX_PANE`, or tmux's current one without it.
pub fn prepare(env: &Environment, run: Run) -> Pane {
    let pane = env.tmux_pane.as_deref().and_then(|pane| pane.to_str());
    let target: &[&str] = match pane {
        Some(pane) => &["-t", pane],
        None => &[],
    };
    let ask = "#{client_termtype}|#{allow-passthrough}|#{focus-events}\
               |#{client_termfeatures}|#{client_termname}";
    let answer = run(&[&["display-message", "-p"], target, &[ask]].concat());
    let known = |field: &str| (!field.is_empty()).then(|| field.to_owned());
    let mut fields = answer.as_deref().map(|answer| answer.splitn(5, '|'));
    let mut next = || fields.as_mut().and_then(Iterator::next);
    let (client, passthrough) = match (next(), next()) {
        (Some(client), Some(passthrough)) => (known(client), known(passthrough)),
        _ => (None, None),
    };
    // A flag in a format is 1 or 0.
    let focus_events = match next() {
        Some("1" | "on") => Some(true),
        Some("0" | "off") => Some(false),
        _ => None,
    };
    // The features are a list such as `bpaste,clipboard,hyperlinks,focus`.
    let hyperlinks = next()
        .and_then(known)
        .map(|features| features.split(',').any(|feature| feature == "hyperlinks"));
    let term_name = next().and_then(known);
    let turned_on = passthrough.as_deref() == Some("off")
        && run(&[&["set-option", "-p"], target, &["allow-passthrough", "on"]].concat()).is_some();
    Pane {
        client,
        passthrough,
        turned_on,
        focus_events,
        hyperlinks,
        term_name,
    }
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
/// as the check of tmux's passthrough does, does not end up inside it.
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
    fn two_sequences_in_a_row_share_a_passthrough() {
        let two = "\x1b_Ga=d\x1b\\\x1b_Gi=2;B\x1b\\";
        let expected = "\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\
                        \x1b\x1b_Gi=2;B\x1b\x1b\\\x1b\\";
        assert_eq!(wrapped(&[two.as_bytes()], true), expected);
        // Also when they come in separate writes of one frame's worth.
        let parts = [
            b"\x1b_Ga=d\x1b\\".as_slice(),
            b"\x1b_Gi=2;B\x1b\\".as_slice(),
        ];
        assert_eq!(
            wrapped(&parts, true),
            "\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\x1b\\\
                                         \x1bPtmux;\x1b\x1b_Gi=2;B\x1b\x1b\\\x1b\\"
        );
    }

    #[test]
    fn another_byte_between_sequences_closes_the_passthrough() {
        let text = "\x1b_Ga=d\x1b\\x\x1b_Gi=2;B\x1b\\";
        let expected = "\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\x1b\\x\
                        \x1bPtmux;\x1b\x1b_Gi=2;B\x1b\x1b\\\x1b\\";
        assert_eq!(wrapped(&[text.as_bytes()], true), expected);
        // An escape sequence of another kind closes it too, and the ESC
        // that might have started a graphics sequence is not lost.
        let other = "\x1b_Ga=d\x1b\\\x1b[1m\x1b_Gi=2;B\x1b\\";
        let expected = "\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\x1b\\\x1b[1m\
                        \x1bPtmux;\x1b\x1b_Gi=2;B\x1b\x1b\\\x1b\\";
        assert_eq!(wrapped(&[other.as_bytes()], true), expected);
    }

    #[test]
    fn the_end_of_a_write_closes_the_passthrough() {
        let mut inner = Vec::new();
        let mut out = Wrapping::new(&mut inner, true);
        out.write_all(b"\x1b_Ga=d\x1b\\\x1b_Gi=2;B\x1b\\").unwrap();
        // Nothing is open now: a write straight to the tty beside this
        // one would not end up inside the passthrough.
        assert!(out.inner.ends_with(b"\x1b\x1b\\\x1b\\"));
        // A held ESC does not keep it open either.
        out.write_all(b"\x1b_Gi=3;C\x1b\\\x1b").unwrap();
        assert!(out.inner.ends_with(b"\x1b\x1b\\\x1b\\"));
        out.write_all(b"[1m").unwrap();
        let text = String::from_utf8(inner).unwrap();
        assert_eq!(text.matches("\x1bPtmux;").count(), 2);
        assert!(text.ends_with("\x1b\\\x1b[1m"));
    }

    /// A graphics sequence with a payload of `n` bytes.
    fn sequence(n: usize) -> String {
        format!("\x1b_Gi=1;{}\x1b\\", "A".repeat(n))
    }

    #[test]
    fn the_budget_closes_the_passthrough_and_a_new_one_opens() {
        const CHUNK: usize = 4096;
        let count = BUDGET / CHUNK + 3;
        let all = sequence(CHUNK).repeat(count);
        let text = wrapped(&[all.as_bytes()], true);
        let passthroughs: Vec<&str> = text.split("\x1bPtmux;").skip(1).collect();
        assert_eq!(passthroughs.len(), 2, "one budget's worth, then the rest");
        let first = passthroughs[0].len();
        assert!(first >= BUDGET, "{first}");
        assert!(first < BUDGET + CHUNK + 16, "{first}");
        assert!(passthroughs.iter().all(|one| one.ends_with("\x1b\\")));
        // Nothing but the sequences is lost, none cut by the close.
        assert_eq!(text.matches("\x1b\x1b_G").count(), count);
        assert_eq!(text.matches("\x1b\x1b\\").count(), count);
    }

    #[test]
    fn a_sequence_split_over_writes_keeps_the_passthrough_open() {
        let two = "\x1b_Ga=d\x1b\\\x1b_Gi=2;B\x1b\\";
        // Cut inside the second sequence: nothing closes the passthrough
        // at the end of the first write, as a sequence is open there, and
        // the second sequence is wrapped whole, in the same passthrough.
        let (head, tail) = two.as_bytes().split_at(two.len() - 4);
        let text = wrapped(&[head, tail], true);
        let expected = "\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\
                        \x1b\x1b_Gi=2;B\x1b\x1b\\\x1b\\";
        assert_eq!(text, expected);
        // A sequence open at the end of a write stays open: nothing
        // closes the passthrough until the sequence ends.
        let mut inner = Vec::new();
        let mut out = Wrapping::new(&mut inner, true);
        out.write_all(head).unwrap();
        assert!(!out.inner.ends_with(b"\x1b\\"));
        out.write_all(tail).unwrap();
        assert!(inner.ends_with(b"\x1b\x1b\\\x1b\\"));
    }

    #[test]
    fn other_escape_sequences_pass_untouched() {
        let other = "\x1b[1m\x1b]8;;u\x1b\\x\x1b\x1b_X\x1b\\";
        assert_eq!(wrapped(&[other.as_bytes()], true), other);
        let bytes: Vec<&[u8]> = other.as_bytes().chunks(1).collect();
        assert_eq!(wrapped(&bytes, true), other);
    }

    use std::cell::RefCell;

    /// The calls a fake tmux got, and what [`prepare`] found, when it
    /// answers each call in turn from `answers`.
    fn prepared(env: &Environment, answers: &[Option<&str>]) -> (Vec<Vec<String>>, Pane) {
        let calls = RefCell::new(Vec::new());
        let run = |args: &[&str]| {
            let mut calls = calls.borrow_mut();
            calls.push(args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>());
            answers
                .get(calls.len() - 1)
                .copied()
                .flatten()
                .map(str::to_owned)
        };
        let pane = prepare(env, &run);
        (calls.into_inner(), pane)
    }

    fn pane_three() -> Environment {
        Environment {
            tmux: Some("/tmp/tmux-1000/default,1,0".into()),
            tmux_pane: Some("%3".into()),
            ..Environment::default()
        }
    }

    const ASK: &str = "#{client_termtype}|#{allow-passthrough}|#{focus-events}\
                       |#{client_termfeatures}|#{client_termname}";

    #[test]
    fn prepare_turns_passthrough_on_when_off() {
        let answers = [
            Some("kitty(0.47.1)|off|1|bpaste,clipboard,focus,title|xterm-kitty"),
            Some(""),
        ];
        let (calls, pane) = prepared(&pane_three(), &answers);
        assert_eq!(
            calls,
            [
                vec!["display-message", "-p", "-t", "%3", ASK],
                vec!["set-option", "-p", "-t", "%3", "allow-passthrough", "on"],
            ]
        );
        let expected = Pane {
            client: Some("kitty(0.47.1)".into()),
            passthrough: Some("off".into()),
            turned_on: true,
            focus_events: Some(true),
            hyperlinks: Some(false),
            term_name: Some("xterm-kitty".into()),
        };
        assert_eq!(pane, expected);
    }

    #[test]
    fn prepare_tells_when_turning_passthrough_on_fails() {
        let (calls, pane) = prepared(&pane_three(), &[Some("kitty(0.47.1)|off"), None]);
        assert_eq!(calls.len(), 2);
        assert!(!pane.turned_on);
        assert_eq!(pane.passthrough.as_deref(), Some("off"));
    }

    #[test]
    fn prepare_leaves_on_and_all_alone() {
        for value in ["on", "all"] {
            let answer = format!("kitty(0.47.1)|{value}");
            let (calls, pane) = prepared(&pane_three(), &[Some(&answer)]);
            assert_eq!(calls.len(), 1, "{value}");
            assert_eq!(pane.passthrough.as_deref(), Some(value));
            assert!(!pane.turned_on);
        }
    }

    #[test]
    fn prepare_without_tmux_answers_nothing() {
        let (calls, pane) = prepared(&pane_three(), &[None]);
        assert_eq!(calls.len(), 1);
        assert_eq!(pane, Pane::default());
    }

    #[test]
    fn prepare_tells_whether_tmux_passes_links() {
        for (features, links) in [
            ("bpaste,clipboard,hyperlinks,focus,title", Some(true)),
            ("hyperlinks", Some(true)),
            ("bpaste,clipboard,focus,title", Some(false)),
            // Only the whole name counts.
            ("nohyperlinks,hyperlinks2", Some(false)),
            // Without a client, or in a tmux without the format.
            ("", None),
        ] {
            let answer = format!("kitty(0.47.1)|on|1|{features}|xterm-kitty");
            let (_, pane) = prepared(&pane_three(), &[Some(&answer)]);
            assert_eq!(pane.hyperlinks, links, "{features:?}");
            assert_eq!(pane.term_name.as_deref(), Some("xterm-kitty"));
            assert_eq!(pane.focus_events, Some(true));
        }
        // An older answer, or one cut short, tells nothing about links.
        let (_, pane) = prepared(&pane_three(), &[Some("kitty(0.47.1)|on|1")]);
        assert_eq!(pane.hyperlinks, None);
        assert_eq!(pane.term_name, None);
        let (_, pane) = prepared(&pane_three(), &[Some("kitty(0.47.1)|on|1|hyperlinks|")]);
        assert_eq!(pane.hyperlinks, Some(true));
        assert_eq!(pane.term_name, None);
    }

    #[test]
    fn prepare_tells_tmux_s_focus_events() {
        // A flag in a format is 1 or 0; older tmux may say on or off.
        for (value, focus) in [
            ("1", Some(true)),
            ("on", Some(true)),
            ("0", Some(false)),
            ("off", Some(false)),
            ("", None),
        ] {
            let answer = format!("kitty(0.47.1)|on|{value}");
            let (_, pane) = prepared(&pane_three(), &[Some(&answer)]);
            assert_eq!(pane.focus_events, focus, "{value:?}");
            assert_eq!(pane.passthrough.as_deref(), Some("on"));
        }
    }

    #[test]
    fn empty_fields_are_not_known() {
        // Without a client attached, tmux has no client_termtype; an old
        // tmux without the option has no allow-passthrough either.
        let (calls, pane) = prepared(&pane_three(), &[Some("|")]);
        assert_eq!(calls.len(), 1);
        assert_eq!(pane.client, None);
        assert_eq!(pane.passthrough, None);
        let (_, pane) = prepared(&pane_three(), &[Some("|on")]);
        assert_eq!(pane.client, None);
        assert_eq!(pane.passthrough.as_deref(), Some("on"));
    }

    #[test]
    fn an_empty_tmux_is_not_tmux() {
        assert!(in_tmux(&pane_three()));
        let empty = Environment {
            tmux: Some("".into()),
            ..pane_three()
        };
        assert!(!in_tmux(&empty));
        assert!(!in_tmux(&Environment::default()));
    }

    #[test]
    fn without_tmux_pane_no_target_is_given() {
        let env = Environment {
            tmux_pane: None,
            ..pane_three()
        };
        let (calls, pane) = prepared(&env, &[Some("kitty(0.47.1)|off"), Some("")]);
        assert_eq!(
            calls,
            [
                vec!["display-message", "-p", ASK],
                vec!["set-option", "-p", "allow-passthrough", "on"],
            ]
        );
        assert!(pane.turned_on);
    }

    #[test]
    fn a_tmux_that_fails_answers_nothing() {
        // Also when tmux is not on PATH at all.
        assert_eq!(run(&["-S", "/nonexistent/kiri-socket", "info"]), None);
    }

    #[test]
    fn a_flush_does_not_release_a_held_esc() {
        let mut inner = Vec::new();
        let mut out = Wrapping::new(&mut inner, true);
        out.write_all(b"x\x1b").unwrap();
        out.flush().unwrap();
        assert_eq!(out.inner.as_slice(), b"x");
        out.write_all(b"[1m").unwrap();
        out.flush().unwrap();
        assert_eq!(inner, b"x\x1b[1m");
    }

    #[test]
    fn without_wrapping_nothing_changes() {
        assert_eq!(wrapped(&[GRAPHICS.as_bytes()], false), GRAPHICS);
    }
}
