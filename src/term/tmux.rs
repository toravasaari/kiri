//! Running inside tmux: kitty's graphics sequences reach kitty only
//! wrapped in tmux's passthrough, `ESC P tmux ; … ESC \` with every ESC
//! inside doubled.

use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::config::Environment;

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

/// Whether kiri runs inside tmux: `TMUX` set and not empty.
pub fn in_tmux(env: &Environment) -> bool {
    env.tmux.as_ref().is_some_and(|value| !value.is_empty())
}

/// What kiri learnt from tmux about the pane it runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    /// The terminal outside tmux, as tmux's `client_termtype` names it,
    /// such as `kitty(0.47.1)`.
    pub client: Option<String>,
    /// The pane's `allow-passthrough` as kiri found it: `off`, `on` or
    /// `all`.
    pub passthrough: Option<String>,
    /// Whether kiri turned the pane's `allow-passthrough` on.
    pub turned_on: bool,
}

/// Asks tmux through `run` for the outer terminal and the pane's
/// `allow-passthrough`, and turns the option on when it is `off`, as
/// `kitten icat` does; `on` and `all` stay as they are. The pane is
/// `TMUX_PANE`, or tmux's current one without it.
pub fn prepare(env: &Environment, run: Run) -> Pane {
    let pane = env.tmux_pane.as_deref().and_then(|pane| pane.to_str());
    let target: &[&str] = match pane {
        Some(pane) => &["-t", pane],
        None => &[],
    };
    let ask = "#{client_termtype}|#{allow-passthrough}";
    let answer = run(&[&["display-message", "-p"], target, &[ask]].concat());
    let known = |field: &str| (!field.is_empty()).then(|| field.to_owned());
    let (client, passthrough) = match answer.as_deref().and_then(|a| a.split_once('|')) {
        Some((client, passthrough)) => (known(client), known(passthrough)),
        None => (None, None),
    };
    let turned_on = passthrough.as_deref() == Some("off")
        && run(&[&["set-option", "-p"], target, &["allow-passthrough", "on"]].concat()).is_some();
    Pane {
        client,
        passthrough,
        turned_on,
    }
}

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

    const ASK: &str = "#{client_termtype}|#{allow-passthrough}";

    #[test]
    fn prepare_turns_passthrough_on_when_off() {
        let (calls, pane) = prepared(&pane_three(), &[Some("kitty(0.47.1)|off"), Some("")]);
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
        let nothing = Pane {
            client: None,
            passthrough: None,
            turned_on: false,
        };
        assert_eq!(pane, nothing);
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
    fn without_wrapping_nothing_changes() {
        assert_eq!(wrapped(&[GRAPHICS.as_bytes()], false), GRAPHICS);
    }
}
