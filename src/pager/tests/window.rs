//! What the pager takes from the window's size: the layout's width is
//! `--width` when it is given, and the window keeps its own columns.

use super::*;

#[test]
fn a_width_given_apart_from_the_window_leaves_the_window_its_columns() {
    let mut caps = Capabilities::ansi(80);
    let mut cell = None;
    fit(&mut caps, Some(window(120, 40)), (Some(40), 0), &mut cell);
    assert_eq!((caps.width, caps.window_width), (40, 120));
    fit(&mut caps, Some(window(100, 40)), (None, 0), &mut cell);
    assert_eq!((caps.width, caps.window_width), (100, 100));
}

#[test]
fn without_a_size_the_status_line_is_as_wide_as_the_window_was() {
    // The window's size was known, then lost: `fit` keeps the columns it
    // last found, and `--width` does not narrow the status line.
    let mut caps = Capabilities::ansi(80);
    let mut cell = None;
    fit(&mut caps, Some(window(120, 40)), (Some(40), 0), &mut cell);
    fit(&mut caps, None, (Some(40), 0), &mut cell);
    let inputs = Inputs::new(caps);
    let blocks = crate::typeset::block::Blocks::default();
    let map = crate::typeset::locate::Map::new(&[], &blocks);
    let files = Files::new(&[], &map, 0);
    let status = status_for("doc", None, (files, Vec::new()), &inputs, (None, true, 0));
    assert_eq!(status.width, 120);
}

#[test]
fn the_layout_is_as_wide_as_the_window_less_the_bar() {
    let mut caps = Capabilities::ansi(80);
    let mut cell = None;
    fit(&mut caps, Some(window(120, 40)), (None, 30), &mut cell);
    assert_eq!((caps.width, caps.window_width), (90, 120));
    // A window that grows or shrinks takes the bar's width from it again.
    fit(&mut caps, Some(window(110, 40)), (None, 30), &mut cell);
    assert_eq!((caps.width, caps.window_width), (80, 110));
    // The bar closes: the layout is the window's.
    fit(&mut caps, Some(window(110, 40)), (None, 0), &mut cell);
    assert_eq!((caps.width, caps.window_width), (110, 110));
}

#[test]
fn a_width_given_limits_the_documents_area_not_the_bar() {
    let mut caps = Capabilities::ansi(80);
    let mut cell = None;
    // `--width` narrower than what the bar leaves: the width given.
    fit(&mut caps, Some(window(140, 40)), (Some(60), 30), &mut cell);
    assert_eq!((caps.width, caps.window_width), (60, 140));
    // Wider than what the bar leaves: what it leaves.
    fit(&mut caps, Some(window(140, 40)), (Some(130), 30), &mut cell);
    assert_eq!((caps.width, caps.window_width), (110, 140));
    // Without the bar `--width` is the layout's width as it was.
    fit(&mut caps, Some(window(140, 40)), (Some(130), 0), &mut cell);
    assert_eq!(caps.width, 130);
}

#[test]
fn reading_modes_area_is_centred_in_what_the_bar_leaves() {
    use crate::typeset::reading::pen::Area as Pen;
    let mut caps = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    };
    let mut cell = None;
    fit(&mut caps, Some(window(140, 40)), (None, 30), &mut cell);
    let theme = crate::typeset::theme::Theme::default();
    let look = crate::typeset::theme::Look::new(&theme, &caps);
    let area = Pen::of(&look, &caps, usize::from(caps.width), 100);
    // The area lies inside the 110 columns right of the bar, with the same
    // room on both sides of it, to a cell.
    let right = 110 - (area.left + area.area);
    assert!(area.left + area.area <= 110, "{area:?}");
    assert!(right.abs_diff(area.left) <= 1, "{area:?}");
}
