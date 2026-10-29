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

#[test]
fn a_search_finds_a_laid_out_block_below_the_first_row_of_its_part() {
    let source = "First paragraph.\n\nSecond paragraph, far down.\n";
    let document = crate::parse::parse(source);
    let mut terminal = Fake::new(80, 10, &[b"/far down\r"]);
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
    let last = terminal.frames().pop().expect("a frame");
    assert!(!last.contains("not found"), "{last:?}");
}

#[test]
fn keys_after_a_band_key_in_the_same_read_are_kept() {
    let images = |keys: &[&[u8]]| {
        let (document, mut inputs) = two_bands();
        let mut terminal = Fake::new(80, 10, keys);
        run_document(
            &document,
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        );
        let frames = terminal.frames();
        frames
            .iter()
            .flat_map(|frame| sent(frame))
            .collect::<Vec<String>>()
    };
    // Read one after the other, Tab selects the second band and → then
    // sends it scrolled; read at once, they send that image too.
    let apart = images(&[b"\t", b"\x1b[C"]);
    let scrolled = apart.last().expect("an image sent");
    let together = images(&[b"\t\x1b[C"]);
    assert!(together.contains(scrolled), "{together:?} {apart:?}");
}

/// The images sent in the last frame that sent any, when the terminal
/// gives `keys` for two bands.
fn last_sent(keys: &[&[u8]]) -> Vec<String> {
    let (document, mut inputs) = two_bands();
    let mut terminal = Fake::new(80, 10, keys);
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    (frames.iter().rev().map(|frame| sent(frame)))
        .find(|images| !images.is_empty())
        .expect("a frame that sent an image")
}

#[test]
fn a_click_behind_the_help_does_not_select_a_band() {
    // The help is open when the click lands on the second band, and any
    // key closes it. The arrow then scrolls the band that was selected
    // before, the first, as if the click had not been.
    let click: &[u8] = b"\x1b[<0;10;6M";
    let right: &[u8] = b"\x1b[C";
    assert_eq!(last_sent(&[b"?", click, b"x", right]), last_sent(&[right]));
}

#[test]
fn the_sideways_wheel_behind_the_help_does_not_scroll_or_select_a_band() {
    let wheel: &[u8] = b"\x1b[<67;10;6M";
    let right: &[u8] = b"\x1b[C";
    assert_eq!(last_sent(&[b"?", wheel, b"x", right]), last_sent(&[right]));
}

#[test]
fn a_click_behind_the_search_prompt_does_not_select_a_band() {
    let click: &[u8] = b"\x1b[<0;10;6M";
    let right: &[u8] = b"\x1b[C";
    assert_eq!(
        last_sent(&[b"/", click, b"\x1b", right]),
        last_sent(&[right])
    );
}

/// The frames drawn for two wide code blocks, shown as text, with reading
/// mode for `M` to turn on, when the terminal gives `keys`.
fn text_frames(keys: &[&[u8]]) -> Vec<String> {
    let long = "a".repeat(200);
    let source = format!("```\n{long}\n```\n\n```\n{long}\n```\n");
    let document = crate::parse::parse(&source);
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs::new(kitty());
    let options = Options {
        reading: Some(reading()),
        ..Options::new(SearchCase::Smart)
    };
    run_document(&document, &mut inputs, options, &mut terminal);
    terminal.frames()
}

#[test]
fn w_in_text_then_m_turns_reading_mode_on_with_wide_blocks_scrolling() {
    let frames = text_frames(&[b"W", b"M"]);
    // The text shows no images, whatever W did; M then shows the pages,
    // with the two code blocks as bands of four rows.
    let last = frames.last().expect("a frame");
    assert!(!frames[0].contains("\u{10EEEE}"));
    assert_eq!(last.matches(",r=4,").count(), 2, "{:?}", sent(last));
}

#[test]
fn m_then_w_in_text_wraps_no_longer() {
    let frames = text_frames(&[b"M", b"W"]);
    // M shows the wrapped blocks in the page; W then scrolls them.
    assert_eq!(
        frames[1].matches(",r=4,").count(),
        0,
        "{:?}",
        sent(&frames[1])
    );
    assert_eq!(
        frames[2].matches(",r=4,").count(),
        2,
        "{:?}",
        sent(&frames[2])
    );
}

#[test]
fn w_twice_in_text_then_m_wraps() {
    let frames = text_frames(&[b"W", b"W", b"M"]);
    let last = frames.last().expect("a frame");
    assert!(last.contains("\u{10EEEE}"));
    assert_eq!(last.matches(",r=4,").count(), 0);
}

/// How many times the pager asked for the inline pictures while it showed
/// `document` for `keys`.
fn asked_for_inline_pictures(keys: &[&[u8]]) -> usize {
    let asked = std::cell::Cell::new(0);
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs::new(kitty());
    let options = Options {
        reading: Some(reading()),
        inline: Some(Box::new(|_, _, _| asked.set(asked.get() + 1))),
        ..Options::new(SearchCase::Smart)
    };
    run_document(&numbers(3), &mut inputs, options, &mut terminal);
    asked.get()
}

#[test]
fn m_starts_the_inline_pictures_loading_only_when_it_turns_reading_mode_on() {
    assert_eq!(asked_for_inline_pictures(&[]), 0);
    assert_eq!(asked_for_inline_pictures(&[b"M"]), 1);
    // Off again: nothing more to ask for; on again: asked, and the loader
    // starts only what is missing.
    assert_eq!(asked_for_inline_pictures(&[b"M", b"M"]), 1);
    assert_eq!(asked_for_inline_pictures(&[b"M", b"M", b"M"]), 2);
}

#[test]
fn the_status_line_tells_that_reading_mode_is_on() {
    let frames = frames(&numbers(3), kitty(), &[b"M", b"M"]);
    // The key is in bold, so the word is told after its own escapes.
    let told = |frame: &String| frame.contains("M\x1b[0;7m reading");
    let reading = frames.iter().position(|frame| frame.contains('\u{10EEEE}'));
    let reading = reading.expect("a frame in reading mode");
    assert!(!told(&frames[0]), "{:?}", frames[0]);
    assert!(told(&frames[reading]), "{:?}", frames[reading]);
    assert!(!told(frames.last().unwrap()));
}

/// The frames drawn for `document` in reading mode in a window of 80 × 10
/// cells, when the terminal gives `keys`.
fn reading_frames(
    document: &Document,
    wide: crate::reading::page::Wide,
    keys: &[&[u8]],
) -> Vec<String> {
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs {
        reading: Some(Reading { wide, ..reading() }),
        ..Inputs::new(kitty())
    };
    run_document(
        document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    terminal.frames()
}

/// How many images a frame sends.
fn sends(frame: &str) -> usize {
    frame.matches("\x1b_Ga=T").count()
}

#[test]
fn a_search_marks_its_matches_in_the_page_and_shows_the_one_it_found() {
    let filler = "A paragraph of filler.\n\n".repeat(30);
    let document = crate::parse::parse(&format!("{filler}Then the needle is here.\n"));
    let frames = reading_frames(&document, crate::reading::page::Wide::Wrap, &[b"/needle\r"]);
    // The first frame sends the page's parts in view; the search moves to
    // the match, whose part is drawn anew with the mark in it.
    assert!(sends(&frames[0]) >= 1);
    let last = frames.last().unwrap();
    assert!(sends(last) >= 1, "{} frames", frames.len());
    assert!(last.contains("1/1"), "{last:?}");
}

#[test]
fn a_search_scrolls_a_band_to_its_match_and_draws_it_anew() {
    let long = "a".repeat(200);
    let document = crate::parse::parse(&format!("Text.\n\n```\n{long} needle\n```\n"));
    let frames = reading_frames(
        &document,
        crate::reading::page::Wide::Scroll,
        &[b"/needle\r"],
    );
    // The band is sent again, scrolled to the match and with it marked: an
    // image that the first frame did not send.
    let ids = |frame: &str| -> Vec<String> {
        frame
            .split("\x1b_Ga=T,")
            .skip(1)
            .filter_map(|rest| rest.split(',').find_map(|part| part.strip_prefix("i=")))
            .map(str::to_owned)
            .collect()
    };
    let before = ids(&frames[0]);
    let new: Vec<String> = (frames[1..].iter().flat_map(|frame| ids(frame)))
        .filter(|id| !before.contains(id))
        .collect();
    assert!(!new.is_empty(), "{new:?} against {before:?}");
}

#[test]
fn a_link_in_a_page_is_a_hyperlink_over_its_placeholders_in_the_frame() {
    let document = crate::parse::parse("Go [here](https://e.example/) now.\n");
    let frames = reading_frames(&document, crate::reading::page::Wide::Wrap, &[]);
    let first = &frames[0];
    let open = first
        .find("\x1b]8;;https://e.example/\x1b\\")
        .expect("the link opens");
    let close = first[open..].find("\x1b]8;;\x1b\\").expect("and closes") + open;
    // Over placeholders only, and not over the whole row.
    let linked = &first[open..close];
    assert!(linked.contains('\u{10EEEE}'), "{linked:?}");
    assert!(linked.matches('\u{10EEEE}').count() < 10, "{linked:?}");
}

#[test]
fn tab_selects_a_link_with_a_box_and_its_url_on_the_status_line_and_escape_lets_it_go() {
    let document = crate::parse::parse(
        "A [first](https://one.example/) and [second](https://two.example/) link.\n",
    );
    let frames = reading_frames(
        &document,
        crate::reading::page::Wide::Wrap,
        &[b"\t", b"\t", b"\x1b[Z", b"\x1b", b""],
    );
    // The frame's own hyperlinks name the urls too; the status line is the
    // last row, after the placeholders.
    let told = |frame: &String| {
        let bar = frame.rsplit("\x1b[10;1H").next().unwrap_or("");
        ["https://one.example/", "https://two.example/"]
            .iter()
            .find(|url| bar.contains(**url))
            .map(|url| url.to_string())
    };
    assert_eq!(told(&frames[0]), None);
    assert_eq!(told(&frames[1]).as_deref(), Some("https://one.example/"));
    assert_eq!(told(&frames[2]).as_deref(), Some("https://two.example/"));
    assert_eq!(told(&frames[3]).as_deref(), Some("https://one.example/"));
    assert_eq!(told(&frames[4]), None);
    // The selection is a box in the page's image: another image is sent.
    assert!(sends(&frames[1]) >= 1, "{:?}", frames[1]);
}

#[test]
fn n_to_a_match_in_another_band_counts_it_and_the_count_stays_after_the_band_moves() {
    let long = "a".repeat(200);
    let document = crate::parse::parse(&format!(
        "```\n{long} needle\n```\n\nText.\n\n```\n{long} needle\n```\n"
    ));
    let frames = reading_frames(
        &document,
        crate::reading::page::Wide::Scroll,
        &[b"/needle\r", b"n"],
    );
    // The second match is the current one in the frame after `n`, whose
    // band has scrolled to it: the layout that shows the band anew does not
    // start the search over.
    assert!(frames[1].contains("1/2"), "{:?}", frames[1]);
    assert!(frames[2].contains("2/2"), "{:?}", frames[2]);
}
