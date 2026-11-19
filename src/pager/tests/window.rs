//! What the pager takes from the window's size: the layout's width is
//! `--width` when it is given, and the window keeps its own columns.

use super::*;

#[test]
fn a_width_given_apart_from_the_window_leaves_the_window_its_columns() {
    let mut caps = Capabilities::ansi(80);
    let mut cell = None;
    fit(&mut caps, Some(window(120, 40)), Some(40), &mut cell);
    assert_eq!((caps.width, caps.window_width), (40, 120));
    fit(&mut caps, Some(window(100, 40)), None, &mut cell);
    assert_eq!((caps.width, caps.window_width), (100, 100));
}

#[test]
fn without_a_size_the_status_line_is_as_wide_as_the_window_was() {
    // The window's size was known, then lost: `fit` keeps the columns it
    // last found, and `--width` does not narrow the status line.
    let mut caps = Capabilities::ansi(80);
    let mut cell = None;
    fit(&mut caps, Some(window(120, 40)), Some(40), &mut cell);
    fit(&mut caps, None, Some(40), &mut cell);
    let inputs = Inputs::new(caps);
    let blocks = crate::typeset::block::Blocks::default();
    let map = crate::typeset::locate::Map::new(&[], &blocks);
    let files = Files::new(&[], &map, 0);
    let status = status_for("doc", None, (files, Vec::new()), &inputs, (None, true));
    assert_eq!(status.width, 120);
}
