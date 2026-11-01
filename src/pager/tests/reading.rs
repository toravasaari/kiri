//! `M`, which turns reading mode on and off.

use std::rc::Rc;

use super::*;
use crate::typeset::reading::Fonts;
use crate::typeset::reading::engine::Reading;

fn kitty() -> Capabilities {
    Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    }
}

pub(super) fn reading() -> Reading {
    Reading {
        pictures: Default::default(),
        fonts: Rc::new(Fonts::fixture()),
        wide: crate::typeset::reading::page::Wide::Wrap,
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
    let document = crate::typeset::parse::parse(&format!("Text.\n\n```\n{long}\n```\n"));
    let mut terminal = Fake::new(80, 10, &[b"\x1b[C"]);
    let mut inputs = Inputs {
        reading: Some(Reading {
            wide: crate::typeset::reading::page::Wide::Scroll,
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
            wide: crate::typeset::reading::page::Wide::Scroll,
            ..reading()
        }),
        ..Inputs::new(kitty())
    };
    (crate::typeset::parse::parse(&source), inputs)
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
    let document = crate::typeset::parse::parse(&format!("Text.\n\n```\n{long}\n```\n"));
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
    let document = crate::typeset::parse::parse(source);
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
    let document = crate::typeset::parse::parse(&source);
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

/// What a call of the planner was given: the number of blocks in the
/// content, whether reading mode was on, and the cells.
type Asked = (usize, bool, Option<(u16, u16)>);

/// What each call of the planner was given while the pager showed
/// `numbers(3)` for `keys`.
fn planned_for(keys: &[&[u8]]) -> Vec<Asked> {
    let calls = std::cell::RefCell::new(Vec::new());
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs::new(kitty());
    let mut then = Then::new([Reloaded::Document(numbers(5))]);
    let options = Options {
        reading: Some(reading()),
        reload: Some(&mut then),
        planner: Some(Box::new(|content, caps, reading| {
            let Content::Document(document) = content else {
                unreachable!("a document is shown");
            };
            let call = (document.blocks.len(), reading, caps.cell_pixels);
            calls.borrow_mut().push(call);
            Vec::new()
        })),
        ..Options::new(SearchCase::Smart)
    };
    run_document(&numbers(3), &mut inputs, options, &mut terminal);
    calls.into_inner()
}

#[test]
fn the_planner_is_asked_when_reading_mode_turns_on_or_off_and_not_at_each_layout() {
    let reading = |keys: &[&[u8]]| -> Vec<bool> {
        planned_for(keys).into_iter().map(|(_, on, _)| on).collect()
    };
    // Once at the start, as there is nothing planned yet.
    assert_eq!(reading(&[]), [false]);
    assert_eq!(reading(&[b"M"]), [false, true]);
    assert_eq!(reading(&[b"M", b"M"]), [false, true, false]);
    assert_eq!(reading(&[b"M", b"M", b"M"]), [false, true, false, true]);
    // Scrolling, searching and the like lay the document out again with
    // the same plan.
    assert_eq!(reading(&[b"j", b"G", b"g", b"?", b"?"]), [false]);
}

#[test]
fn the_planner_is_given_the_document_as_it_was_last_reloaded() {
    let seen: Vec<usize> = (planned_for(&[b"r", b"M"]).into_iter())
        .map(|(blocks, ..)| blocks)
        .collect();
    // The document as opened at the start; as reloaded when it is read
    // again, which plans it before it is shown, and when `M` asks again.
    assert_eq!(seen, [3, 5, 5]);
}

#[test]
fn the_planner_is_asked_again_for_the_new_cells_after_an_answer_that_changes_them() {
    let cells = |keys: &[&[u8]]| -> Vec<Option<(u16, u16)>> {
        planned_for(keys)
            .into_iter()
            .map(|(.., cells)| cells)
            .collect()
    };
    // The answer alone, without `M`.
    let alone = cells(&[b"\x1b[6;32;16t"]);
    assert_eq!(alone.last(), Some(&Some((16, 32))), "{alone:?}");
    assert_ne!(alone.first(), alone.last(), "{alone:?}");
    // The answer and `M` in one read: it is asked for the cells the answer
    // tells, with or without `M` behind it.
    let before = cells(&[b"\x1b[6;24;10t\x1b[6;32;16tM"]);
    assert_eq!(before.last(), Some(&Some((16, 32))), "{before:?}");
    let behind = cells(&[b"M\x1b[6;32;16t"]);
    assert_eq!(behind.last(), Some(&Some((16, 32))), "{behind:?}");
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
    wide: crate::typeset::reading::page::Wide,
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
    let document = crate::typeset::parse::parse(&format!("{filler}Then the needle is here.\n"));
    let frames = reading_frames(
        &document,
        crate::typeset::reading::page::Wide::Wrap,
        &[b"/needle\r"],
    );
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
    let document = crate::typeset::parse::parse(&format!("Text.\n\n```\n{long} needle\n```\n"));
    let frames = reading_frames(
        &document,
        crate::typeset::reading::page::Wide::Scroll,
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
    let document = crate::typeset::parse::parse("Go [here](https://e.example/) now.\n");
    let frames = reading_frames(&document, crate::typeset::reading::page::Wide::Wrap, &[]);
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
    let document = crate::typeset::parse::parse(
        "A [first](https://one.example/) and [second](https://two.example/) link.\n",
    );
    let frames = reading_frames(
        &document,
        crate::typeset::reading::page::Wide::Wrap,
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
    let document = crate::typeset::parse::parse(&format!(
        "```\n{long} needle\n```\n\nText.\n\n```\n{long} needle\n```\n"
    ));
    let frames = reading_frames(
        &document,
        crate::typeset::reading::page::Wide::Scroll,
        &[b"/needle\r", b"n"],
    );
    // The second match is the current one in the frame after `n`, whose
    // band has scrolled to it: the layout that shows the band anew does not
    // start the search over.
    assert!(frames[1].contains("1/2"), "{:?}", frames[1]);
    assert!(frames[2].contains("2/2"), "{:?}", frames[2]);
}

#[test]
fn tab_selects_a_link_without_hyperlinks_and_none_are_written() {
    let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
    let mut terminal = Fake::new(80, 10, &[b"\t"]);
    let caps = Capabilities {
        hyperlinks: false,
        ..kitty()
    };
    let mut inputs = Inputs {
        reading: Some(reading()),
        ..Inputs::new(caps)
    };
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    // The status line tells where the link goes, though no frame has a
    // hyperlink for the terminal to open.
    let bar = frames[1].rsplit("\x1b[10;1H").next().unwrap_or("");
    assert!(bar.contains("https://one.example/"), "{bar:?}");
    assert!(
        frames.iter().all(|frame| !frame.contains("\x1b]8;")),
        "{frames:?}"
    );
}

/// Everything written to a terminal of 80 × 10 cells for `document` in
/// reading mode with `caps`, when it gives `keys`.
fn written(document: &Document, caps: Capabilities, keys: &[&[u8]]) -> String {
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs {
        reading: Some(reading()),
        ..Inputs::new(caps)
    };
    run_document(
        document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    String::from_utf8(terminal.out).unwrap()
}

/// OSC 52 for the clipboard, with `https://one.example/` in base64.
const COPY_ONE: &str = "\x1b]52;c;aHR0cHM6Ly9vbmUuZXhhbXBsZS8=\x1b\\";

#[test]
fn enter_copies_the_selected_link_s_url_and_the_status_line_says_so() {
    let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
    let out = written(&document, kitty(), &[b"\t", b"\r", b""]);
    assert_eq!(out.matches(COPY_ONE).count(), 1, "OSC 52 sequences written");
    // The sequence comes before the frame that tells it was copied.
    let copied = out.find(COPY_ONE).unwrap();
    let told = out.rfind("link copied").expect("the status line says so");
    assert!(copied < told, "in what was written");
}

#[test]
fn enter_without_a_selected_link_scrolls_down_a_line_and_copies_nothing() {
    let filler = "A paragraph of filler.\n\n".repeat(30);
    let document = crate::typeset::parse::parse(&format!(
        "A [first](https://one.example/) link.\n\n{filler}"
    ));
    let mut terminal = Fake::new(80, 10, &[b"\r"]);
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
    assert!(terminal.frames().len() >= 2, "the view did not move");
    let out = String::from_utf8(terminal.out).unwrap();
    assert!(!out.contains("\x1b]52;"), "in what was written");
    assert!(!out.contains("link copied"), "in what was written");
}

#[test]
fn enter_with_a_selected_link_that_has_no_address_copies_nothing() {
    let document = crate::typeset::parse::parse("A [first]() link.\n");
    let out = written(&document, kitty(), &[b"\t", b"\r", b""]);
    assert!(!out.contains("\x1b]52;"), "in what was written");
    assert!(!out.contains("link copied"), "in what was written");
}

#[test]
fn enter_with_the_selected_link_off_the_screen_scrolls_and_copies_nothing() {
    let filler = "A paragraph of filler.\n\n".repeat(30);
    let document = crate::typeset::parse::parse(&format!(
        "A [first](https://one.example/) link.\n\n{filler}"
    ));
    // Tab selects the link, and three pages down it is off the screen.
    let keys: [&[u8]; 6] = [b"\t", b"\x06", b"\x06", b"\x06", b"\r", b""];
    let mut terminal = Fake::new(80, 10, &keys);
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
    let before = frames[frames.len() - 2].clone();
    let last = frames.last().unwrap();
    assert_ne!(*last, before, "the view did not move");
    let out = String::from_utf8(terminal.out).unwrap();
    assert!(!out.contains("\x1b]52;"), "in what was written");
}

#[test]
fn the_status_line_tells_a_selected_link_s_address_only_while_it_is_on_the_screen() {
    let filler = "A paragraph of filler.\n\n".repeat(30);
    let document = crate::typeset::parse::parse(&format!(
        "A [first](https://one.example/) link.\n\n{filler}"
    ));
    // Tab selects the link, three pages down it is off the screen, and
    // three pages back it is there again, still selected.
    let keys: [&[u8]; 8] = [
        b"\t", b"\x06", b"\x06", b"\x06", b"\x02", b"\x02", b"\x02", b"",
    ];
    let mut terminal = Fake::new(80, 10, &keys);
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
    let told: Vec<bool> = (terminal.frames().iter())
        .map(|frame| {
            let bar = frame.rsplit("\x1b[10;1H").next().unwrap_or("");
            bar.contains("https://one.example/")
        })
        .collect();
    // At the start none is selected; then it is, then it is off the screen
    // for three pages down and two back, and then it is there again.
    assert_eq!(
        told,
        [false, true, false, false, false, false, false, true],
        "{told:?}"
    );
}

#[test]
fn in_tmux_with_passthrough_the_copy_goes_through_to_kitty() {
    let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
    let caps = Capabilities {
        tmux_passthrough: true,
        ..kitty()
    };
    let out = written(&document, caps, &[b"\t", b"\r", b""]);
    let wrapped = format!("\x1bPtmux;{}\x1b\\", COPY_ONE.replace('\x1b', "\x1b\x1b"));
    assert_eq!(
        out.matches(&wrapped).count(),
        1,
        "wrapped OSC 52 sequences written"
    );
    assert!(!out.contains(COPY_ONE), "in what was written");
}

#[test]
fn tab_selects_a_link_in_a_band_scrolled_out_of_view_and_scrolls_the_band_to_it() {
    let long = "x".repeat(150);
    let document = crate::typeset::parse::parse(&format!(
        "| a | b |\n|---|---|\n| [near](https://one.example/) | {long}[far](https://two.example/) |\n"
    ));
    let frames = reading_frames(
        &document,
        crate::typeset::reading::page::Wide::Scroll,
        &[b"\t", b"\t", b"\t", b""],
    );
    // The status line is the last row, after the placeholders.
    let told = |frame: &String| {
        let bar = frame.rsplit("\x1b[10;1H").next().unwrap_or("");
        ["https://one.example/", "https://two.example/"]
            .iter()
            .find(|url| bar.contains(**url))
            .map(|url| url.to_string())
    };
    let told: Vec<Option<String>> = frames.iter().map(told).collect();
    assert_eq!(told.get(1), Some(&Some("https://one.example/".to_owned())));
    assert_eq!(told.get(2), Some(&Some("https://two.example/".to_owned())));
    // On round, the band scrolled back to the first.
    assert_eq!(told.get(3), Some(&Some("https://one.example/".to_owned())));
}

#[test]
fn the_status_line_cuts_a_long_link_in_the_middle() {
    let url = "https://example.org/a/very/long/path/that/goes/on/and/on/to/the/page.html";
    let document = crate::typeset::parse::parse(&format!("A [link]({url}) in a line.\n"));
    let mut terminal = Fake::new(60, 10, &[b"\t", b""]);
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
    let bar = frames[1].rsplit("\x1b[10;1H").next().unwrap_or("");
    assert!(bar.contains("https://example.org/"), "{bar:?}");
    assert!(bar.contains("…"), "{bar:?}");
    assert!(bar.contains("/page.html"), "{bar:?}");
}

#[test]
fn b_paints_the_rows_around_the_page_too() {
    let mut caps = kitty();
    caps.truecolor = true;
    caps.background = Some(crate::typeset::color::Color::Rgb(16, 32, 48));
    let mut terminal = Fake::new(80, 10, &[b"B"]);
    let mut inputs = Inputs {
        reading: Some(reading()),
        ..Inputs::new(caps)
    };
    run_document(
        &numbers(3),
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    assert_eq!(frames.len(), 2, "{frames:?}");
    // One shade off the terminal's, which kitty would show through.
    let painted = "\x1b[0;48;2;17;33;49m\x1b[H\x1b[2J";
    assert!(!frames[0].contains("48;2;17;33;49"), "{:?}", frames[0]);
    // The whole screen, beside and below the page too.
    assert!(frames[1].contains(painted), "{:?}", frames[1]);
    // The page is laid out on that background, which makes it opaque.
    let shade = crate::typeset::color::Color::Rgb(17, 33, 49);
    assert_eq!(inputs.theme.background, Some(shade));
    let page = |frame: &str| sent_ids(frame);
    assert_ne!(page(&frames[0]), page(&frames[1]), "a new page image");
}
