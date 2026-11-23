use super::*;

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
