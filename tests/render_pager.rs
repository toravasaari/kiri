//! Pager frames: which lines are drawn where, as escape sequences.

mod common;

use common::{WIDTH, visible};
use kiri::images::Images;
use kiri::pager::view::{Motion, View};
use kiri::render::Inputs;
use kiri::render::encoder::Encoder;
use kiri::render::output::{Page, Writer};
use kiri::typeset::caps::Capabilities;

/// The frame `kiri::pager::draw` writes for `source` in a window of `rows`
/// rows, after `motions`.
fn frame(source: &str, caps: Capabilities, page: Page, rows: usize, motions: &[Motion]) -> String {
    let document = kiri::typeset::parse::parse(source);
    let inputs = Inputs::new(caps);
    let lines = kiri::typeset::layout::layout(&document, &inputs);
    let images = Images::new();
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, &inputs.caps, page, &mut encoder);
    let heights = lines
        .iter()
        .map(|line| usize::from(writer.rows(line)))
        .collect();
    let mut view = View::new(heights, rows);
    for &motion in motions {
        view.scroll(motion);
    }
    let mut out = Vec::new();
    let shown = &lines[view.visible()];
    kiri::pager::draw(shown, view.rows(), &[], &[], &[], &mut writer, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

const DOCUMENT: &str = "# Title\n\nOne\n\nTwo\n\nThree\n\nFour\n";

#[test]
fn a_frame_places_each_row_and_clears_the_rest() {
    let out = frame(
        DOCUMENT,
        Capabilities::kitty(WIDTH),
        Page::default(),
        6,
        &[],
    );
    // One synchronized update.
    assert!(out.starts_with("\x1b[?2026h"), "{out:?}");
    assert!(out.ends_with("\x1b[?2026l"), "{out:?}");
    assert!(!out.contains('\n'), "{out:?}");
    insta::assert_snapshot!(visible(&out));
}

#[test]
fn a_heading_scrolled_past_is_not_drawn() {
    let out = frame(
        DOCUMENT,
        Capabilities::kitty(WIDTH),
        Page::default(),
        6,
        &[Motion::Down(1)],
    );
    assert!(!out.contains("Title"), "{out:?}");
    assert!(out.contains("\x1b[1;1H"), "{out:?}");
}

#[test]
fn the_screen_is_cleared_in_the_page_colour() {
    let page = Page {
        background: Some(kiri::typeset::color::Color::Rgb(1, 2, 3)),
        foreground: None,
    };
    let out = frame(DOCUMENT, Capabilities::kitty(WIDTH), page, 6, &[]);
    let clear = out.find("\x1b[2J").expect("the screen is cleared");
    let colour = out.find("48;2;1;2;3").expect("the page colour is set");
    assert!(colour < clear, "{out:?}");
    insta::assert_snapshot!(visible(&out));
}
