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
    // In separate writes each sequence gets its own passthrough: the end
    // of a write closes it.
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
