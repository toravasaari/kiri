//! The contents bar in a view that a link to another Markdown file opens
//! over the document: the bar lists the view's headings, and `t`, the
//! bar's keys and the help's Contents topic work there as in the base.

use super::opening::{BACKSPACE, opens, with_rows};
use super::sidebar::{bar_of, document_of, status_of};
use super::*;
use crate::session::areas::Role;
use crate::session::sidebar::Visibility;

/// The view's headings, with enough lines under them to scroll.
fn other() -> Document {
    let lines: String = (1..=30).map(|n| format!("Line {n}.\n\n")).collect();
    crate::typeset::parse::parse(&format!(
        "# Other\n\nfirst\n\n## Other two\n\n{lines}## Other three\n\nlast\n"
    ))
}

/// A base with a link to `docs/other.md` and headings of its own, shown in
/// a window of 120 × 12 with the bar, the reader pressing `keys`; `after`
/// looks at the loop once the reader has quit.
fn viewed(keys: &[&[u8]], after: impl FnOnce(&Driver<'_, '_, '_, Fake>)) -> Fake {
    let base = with_rows("# Base\n\n[go](docs/other.md)\n\n## Base two", 20);
    let mut then = opens([other()]);
    let options = Options {
        files: Some(&mut then),
        sidebar: Visibility::Auto,
        ..Options::new(SearchCase::Smart)
    };
    let mut terminal = Fake::new(120, 12, keys);
    let mut inputs = Inputs::new(screen_caps(120));
    let mut loads = Loads::inline_builds();
    let session = session_for(&options);
    let mut driver = Driver::new(
        Content::Document(&base),
        None,
        session,
        &mut inputs,
        options,
        &mut loads,
        &mut terminal,
    );
    while driver.round().unwrap() {}
    after(&driver);
    drop(driver);
    loads.halt();
    loads.wait(crate::images::STOPPING);
    terminal
}

#[test]
fn the_bar_in_a_view_lists_the_headings_of_the_view_and_takes_the_focus_with_t() {
    on_both(|| {
        let terminal = viewed(&[b"\t", b"\r", b"", b"t", b"j", b"q"], |_| {});
        let screens = terminal.seen();
        let opened = screens
            .iter()
            .position(|seen| bar_of(seen).contains("Other two"))
            .expect("the view's headings in the bar");
        let bar = bar_of(&screens[opened]);
        assert!(!bar.contains("Base"), "{}", screens[opened].text());
        // `t` gives the bar the focus, and the status line names its keys.
        let focused = &screens[opened + 1];
        let line = status_of(focused);
        assert!(line.starts_with(" contents "), "{line}");
        // `j` moves the selection in the view's contents and does not
        // scroll the view.
        let moved = &screens[opened + 2];
        assert_eq!(document_of(moved), document_of(focused), "{}", moved.text());
    });
}

#[test]
fn j_in_the_bar_of_a_view_selects_the_second_heading_of_the_view() {
    on_both(|| {
        let mut state = (Role::Document, None);
        viewed(&[b"\t", b"\r", b"", b"t", b"j"], |rig| {
            state = (rig.state.key_area, rig.state.sidebar.selected);
        });
        assert_eq!(state, (Role::Contents, Some(1)));
    });
}

#[test]
fn the_help_opens_at_the_contents_from_the_bar_of_a_view() {
    on_both(|| {
        let helped = |keys: &[&[u8]]| -> Vec<String> {
            let terminal = viewed(keys, |_| {});
            terminal.seen().iter().map(Seen::text).collect()
        };
        let focused = helped(&[b"\t", b"\r", b"", b"t", b"?", b"q"]);
        assert!(focused.iter().any(|f| f.contains("go to the heading")));
        // Without the focus in the bar it opens at the topic of moving.
        let unfocused = helped(&[b"\t", b"\r", b"", b"?", b"q"]);
        assert!(!unfocused.iter().any(|f| f.contains("go to the heading")));
        assert!(unfocused.iter().any(|f| f.contains("down a line")));
    });
}

#[test]
fn a_view_closed_with_the_bar_focused_gives_the_focus_to_the_base() {
    on_both(|| {
        let mut area = Role::Contents;
        let keys: &[&[u8]] = &[b"\t", b"\r", b"", b"t", BACKSPACE, b"j", b"q"];
        let terminal = viewed(keys, |rig| area = rig.state.key_area);
        assert_eq!(area, Role::Document);
        // The base is shown again with the document's status line, and the
        // `j` after it scrolled the base, not a bar.
        let last = terminal.seen().last().unwrap();
        assert!(bar_of(last).contains("Base two"), "{}", last.text());
        assert!(status_of(last).starts_with(" kiri "), "{}", status_of(last));
    });
}
