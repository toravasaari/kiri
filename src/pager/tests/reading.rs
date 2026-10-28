//! `M`, which turns reading mode on and off.

use std::rc::Rc;

use super::*;
use crate::reading::Fonts;
use crate::render::reading::Reading;

fn kitty() -> Capabilities {
    Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    }
}

fn reading() -> Reading {
    Reading {
        fonts: Rc::new(Fonts::fixture()),
        wide: crate::reading::page::Wide::Wrap,
    }
}

/// The frames drawn for `document` with `caps`, in text first, with
/// reading mode for `M` to turn on, when the terminal gives `keys`.
fn frames(document: &Document, caps: Capabilities, keys: &[&[u8]]) -> Vec<String> {
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs::new(caps);
    let options = Options {
        reading: Some(reading()),
        ..Options::new(SearchCase::Smart)
    };
    run_document(document, &mut inputs, options, &mut terminal);
    terminal.frames()
}

#[test]
fn m_turns_reading_mode_on_and_off() {
    let frames = frames(&numbers(3), kitty(), &[b"M", b"M"]);
    let last = frames.len() - 1;
    let reading = (frames.iter()).position(|frame| frame.contains('\u{10EEEE}'));
    let reading = reading.expect("a frame in reading mode");
    assert!(frames[0].contains("Line 1") && !frames[0].contains('\u{10EEEE}'));
    assert!(!frames[reading].contains("Line 1"), "{:?}", frames[reading]);
    assert!(frames[last].contains("Line 1") && !frames[last].contains('\u{10EEEE}'));
}

#[test]
fn m_keeps_the_block_at_the_top() {
    let down: &[u8] = &[b'j'; 20];
    let frames = frames(&numbers(40), kitty(), &[down, b"M", b"M"]);
    let first_line = |frame: &str| {
        let text: String = frame
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
            .collect();
        text.split("Line ")
            .nth(1)
            .map(|rest| rest.split(' ').next().unwrap_or("").to_owned())
    };
    let before = (frames.iter())
        .rev()
        .find(|frame| !frame.contains('\u{10EEEE}') && frame.contains("Line 11"));
    assert!(before.is_some(), "the view moved down");
    let last = frames.last().unwrap();
    assert_eq!(first_line(last), first_line(before.unwrap()), "{last:?}");
}

#[test]
fn without_images_m_says_why() {
    let frames = frames(&numbers(3), Capabilities::ansi(80), &[b"M"]);
    let last = frames.last().unwrap();
    assert!(
        last.contains("reading mode needs a terminal that draws images"),
        "{last:?}"
    );
}

#[test]
fn an_arrow_scrolls_the_selected_band_sideways() {
    let long = "a".repeat(200);
    let document = crate::parse::parse(&format!("Text.\n\n```\n{long}\n```\n"));
    let mut terminal = Fake::new(80, 10, &[b"\x1b[C"]);
    let mut inputs = Inputs {
        reading: Some(Reading {
            wide: crate::reading::page::Wide::Scroll,
            ..reading()
        }),
        ..Inputs::new(kitty())
    };
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    let sent = |frame: &str| frame.matches("\x1b_Ga=T").count();
    let first = frames.first().expect("a frame");
    let last = frames.last().expect("a frame");
    // The band is sent anew, scrolled, and the page beside it is not.
    assert!(sent(first) >= 2, "{}", sent(first));
    assert_eq!(sent(last), 1, "{} frames", frames.len());
}

/// A document with two wide code blocks, the second at the bottom of a
/// window of 10 rows, laid out to scroll.
fn two_bands() -> (Document, Inputs) {
    let long = "a".repeat(200);
    let source = format!("```\n{long}\n```\n\n```\n{long}\n```\n");
    let inputs = Inputs {
        reading: Some(Reading {
            wide: crate::reading::page::Wide::Scroll,
            ..reading()
        }),
        ..Inputs::new(kitty())
    };
    (crate::parse::parse(&source), inputs)
}

/// The ids of the images sent in `frame`.
fn sent(frame: &str) -> Vec<String> {
    (frame.split("\x1b_Ga=T").skip(1))
        .filter_map(|keys| {
            keys.split(",i=")
                .nth(1)?
                .split(',')
                .next()
                .map(str::to_owned)
        })
        .collect()
}

#[test]
fn a_click_selects_the_band_under_it_and_the_sideways_wheel_scrolls_it() {
    let (document, mut inputs) = two_bands();
    // The second band starts on the fifth row: a click there, then the
    // sideways wheel to the right on it.
    let keys: &[&[u8]] = &[b"\x1b[<0;10;6M", b"\x1b[<67;10;6M"];
    let mut terminal = Fake::new(80, 10, keys);
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    // The click sends both bands anew, the second now selected; the
    // wheel sends the second scrolled.
    assert_eq!(frames.len(), 3, "{} frames", frames.len());
    assert_eq!(sent(&frames[1]).len(), 2, "{:?}", sent(&frames[1]));
    assert_eq!(sent(&frames[2]).len(), 1, "{:?}", sent(&frames[2]));
}

#[test]
fn the_wheel_with_shift_away_from_a_band_scrolls_down() {
    let frames = frames(&numbers(30), kitty(), &[b"\x1b[<69;10;3M"]);
    assert!(frames[0].contains("Line 1"));
    assert!(
        !frames.last().unwrap().contains("Line 1 "),
        "{:?}",
        frames.last()
    );
}

#[test]
fn w_switches_wide_blocks_between_wrapping_and_scrolling() {
    let long = "a".repeat(200);
    let document = crate::parse::parse(&format!("Text.\n\n```\n{long}\n```\n"));
    let mut terminal = Fake::new(80, 10, &[b"W", b"W"]);
    let mut inputs = Inputs {
        reading: Some(reading()),
        ..Inputs::new(kitty())
    };
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    let bands = |frame: &str| frame.matches(",r=4,").count();
    // Wrapped, no band; scrolling, a band of four rows; wrapped again.
    assert_eq!(frames.len(), 3, "{} frames", frames.len());
    assert_eq!(bands(&frames[0]), 0);
    assert_eq!(bands(&frames[1]), 1, "{:?}", sent(&frames[1]));
    assert_eq!(bands(&frames[2]), 0);
}
