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
