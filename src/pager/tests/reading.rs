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
