//! Where the view goes when Tab selects a picture that is not wholly in it.

use super::*;
use crate::typeset::images::Source;
use crate::typeset::images::animation::{Animation, Frame, Plays};

/// An animation of 20 × 60 pixels, which takes three rows of the text
/// layout, in a window of cells of 10 × 20 pixels.
fn clip(name: &str) -> (String, Source) {
    let frames = [[255, 0, 0, 255], [0, 255, 0, 255]]
        .into_iter()
        .map(|rgba| Frame {
            image: image::RgbaImage::from_pixel(20, 60, image::Rgba(rgba)),
            gap: Duration::from_secs(1),
        })
        .collect();
    let animation = Animation {
        frames,
        plays: Plays::Forever,
        over_budget: false,
    };
    (
        name.to_owned(),
        Source::Animated(std::sync::Arc::new(animation)),
    )
}

/// A document of `before` paragraphs, the picture `a.gif`, and then `between`
/// paragraphs and the picture `b.gif` when `second`, and `after` paragraphs.
/// A paragraph takes two rows with the blank row below it; a picture takes
/// three rows and a blank one.
fn document(before: usize, between: Option<usize>, after: usize) -> (Document, Inputs) {
    let mut source = String::new();
    for n in 0..before {
        source.push_str(&format!("before {n}\n\n"));
    }
    source.push_str("![first](a.gif)\n\n");
    let mut names = vec!["a.gif"];
    if let Some(between) = between {
        for n in 0..between {
            source.push_str(&format!("between {n}\n\n"));
        }
        source.push_str("![second](b.gif)\n\n");
        names.push("b.gif");
    }
    for n in 0..after {
        source.push_str(&format!("after {n}\n\n"));
    }
    let mut inputs = Inputs::new(Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    });
    for name in names {
        let (name, clip) = clip(name);
        inputs.images.insert(name.as_str(), Ok(clip));
    }
    (crate::typeset::parse::parse(&source), inputs)
}

/// The row of the document's text `marker` in the last frame of the pager,
/// in a window of 8 rows, 7 of them for the document, after the keys
/// `reads`. A picture's first row is 2 rows below the marker `before N`
/// above it, and 4 above the marker `between N` or `after N` below it.
fn row_after(
    (document, mut inputs): (Document, Inputs),
    reads: &[&[u8]],
    marker: &str,
) -> Option<u16> {
    let mut reads = reads.to_vec();
    reads.push(b"q");
    let mut terminal = Fake::new(60, 8, &reads);
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let screens = terminal.screens();
    screen_row_of(screens.last().expect("a frame"), 8, marker)
}

const TAB: &[u8] = b"\t";
const BACK: &[u8] = b"\x1b[Z";
const DOWN: &[u8] = b"j";

#[test]
fn a_picture_partly_below_the_view_is_brought_up_to_its_bottom_edge() {
    // The picture is on lines 6 to 8, and only its first row is in view.
    // Afterwards its three rows end on the last row of the document's 7:
    // rows 5 to 7, and `before 2` two rows above it.
    let row = row_after(document(3, None, 8), &[TAB], "before 2");
    assert_eq!(row, Some(3));
}

#[test]
fn a_picture_partly_above_the_view_is_brought_down_to_its_top_edge() {
    let mut reads = vec![DOWN; 7];
    reads.push(TAB);
    // The picture's rows are 1 to 3, and `after 0` is on row 5.
    let row = row_after(document(3, None, 8), &reads, "after 0");
    assert_eq!(row, Some(5));
}

#[test]
fn a_picture_wholly_below_the_view_is_shown_in_the_middle() {
    // On lines 12 to 14: two rows above it, and two below.
    let row = row_after(document(6, None, 8), &[TAB], "before 5");
    assert_eq!(row, Some(1));
}

#[test]
fn a_picture_wholly_above_the_view_is_shown_in_the_middle() {
    let row = row_after(document(3, None, 20), &[b"G", TAB], "before 2");
    assert_eq!(row, Some(1));
}

#[test]
fn a_picture_that_is_wholly_in_the_view_does_not_move_it() {
    let row = row_after(document(1, None, 8), &[TAB], "before 0");
    assert_eq!(row, Some(1));
}

#[test]
fn tab_goes_on_to_a_picture_far_down_and_round_to_the_first_in_the_middle() {
    // The first picture is on lines 6 to 8, the second on lines 60 to 62.
    let both = || document(3, Some(25), 8);
    let after = |reads: &[&[u8]], marker| row_after(both(), reads, marker);
    assert_eq!(after(&[TAB], "before 2"), Some(3));
    assert_eq!(after(&[TAB, TAB], "between 24"), Some(1));
    // Round to the first, wholly above the view now.
    assert_eq!(after(&[TAB, TAB, TAB], "before 2"), Some(1));
    // Shift+Tab from the first goes to the last, wholly below the view.
    assert_eq!(after(&[TAB, BACK], "between 24"), Some(1));
    // And back from the last to the first, above the view.
    assert_eq!(after(&[TAB, BACK, BACK], "before 2"), Some(1));
}
