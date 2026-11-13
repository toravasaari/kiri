//! `M`, which turns reading mode on and off.

use std::rc::Rc;

use super::*;
use crate::typeset::reading::Fonts;
use crate::typeset::reading::engine::Reading;

/// The terminal the test is on: kitty's, or sixel's under [`on_both`].
fn screen() -> Capabilities {
    Capabilities {
        cell_pixels: Some((10, 20)),
        ..screen_caps(80)
    }
}

pub(super) fn reading() -> Reading {
    Reading {
        pictures: Default::default(),
        fonts: Rc::new(Fonts::fixture()),
        wide: crate::typeset::reading::page::Wide::Wrap,
    }
}

/// The terminal after `document` was shown with `caps`, in text first,
/// with reading mode for `M` to turn on, when it gives `keys`.
fn ran(document: &Document, caps: Capabilities, keys: &[&[u8]]) -> Fake {
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs::new(caps);
    let options = Options {
        reading: Some(reading()),
        ..Options::new(SearchCase::Smart)
    };
    run_document(document, &mut inputs, options, &mut terminal);
    terminal
}

/// Whether `seen` shows a page of reading mode.
fn shows_page(seen: &Seen) -> bool {
    seen.text().contains("⟦page")
}

/// How many bands of rows `seen` shows: the first rows of them.
fn bands(seen: &Seen) -> usize {
    (seen.text().split("⟦band").skip(1))
        .filter(|rest| {
            rest.split('⟧')
                .next()
                .is_some_and(|marker| marker.ends_with(" r0"))
        })
        .count()
}

#[test]
fn m_turns_reading_mode_on_and_off() {
    on_both(|| {
        let terminal = ran(&numbers(3), screen(), &[b"M", b"M"]);
        // The screen after each frame, not the frame: one that does not write a
        // row would not show what it still holds.
        let screens = terminal.seen();
        let reading = (screens.iter()).position(shows_page);
        let reading = reading.expect("a frame in reading mode");
        assert!(screens[0].text().contains("Line 1") && !shows_page(&screens[0]));
        assert!(
            !screens[reading].text().contains("Line 1"),
            "{:?}",
            screens[reading].text()
        );
        let last = screens.last().unwrap();
        assert!(last.text().contains("Line 1") && !shows_page(last));
    });
}

#[test]
fn m_keeps_the_block_at_the_top() {
    on_both(|| {
        let down: &[u8] = &[b'j'; 20];
        let terminal = ran(&numbers(40), screen(), &[down, b"M", b"M"]);
        let screens = terminal.seen();
        let first_line = |seen: &Seen| {
            let text = seen.text();
            text.split("Line ").nth(1).map(|rest| {
                rest.split(|c: char| !c.is_ascii_alphanumeric())
                    .next()
                    .unwrap_or("")
                    .to_owned()
            })
        };
        let before = (screens.iter())
            .rev()
            .find(|seen| !shows_page(seen) && seen.text().contains("Line 11"));
        assert!(before.is_some(), "the view moved down");
        let last = screens.last().unwrap();
        assert_eq!(
            first_line(last),
            first_line(before.unwrap()),
            "{:?}",
            last.text()
        );
    });
}

/// What the first line of text shown is after `M` back to text, from a view
/// at "Line 11" in reading mode, when the document was read again meanwhile
/// as `reload` says (or not read again when `None`), once with `follow` on.
fn text_after_reload(reload: Option<Document>, follow: settings::Follow) -> String {
    let down: &[u8] = &[b'j'; 20];
    let reload_asked = reload.is_some();
    let mut then = Then::new(reload.clone().map(Reloaded::Document));
    let reads: Vec<&[u8]> = match reload {
        Some(_) => vec![b"M", down, b"r", b"", b"M", b""],
        None => vec![b"M", down, b"", b"", b"M", b""],
    };
    let mut terminal = Fake::new(80, 10, &reads);
    let mut inputs = Inputs::new(screen());
    let options = Options {
        reading: Some(reading()),
        files: Some(&mut then),
        follow,
        ..Options::new(SearchCase::Smart)
    };
    run_document(&numbers(40), &mut inputs, options, &mut terminal);
    let screens = terminal.seen();
    let reloaded = screens.iter().any(|seen| seen.text().contains("reloaded"));
    assert_eq!(
        reloaded,
        reload_asked,
        "{:?}",
        screens.last().map(Seen::text)
    );
    let last = (screens.iter()).rev().find(|seen| !shows_page(seen));
    let text = last.expect("a text frame").text();
    let rest = text.split("Line ").nth(1).expect("a line");
    (rest.split(|c: char| !c.is_ascii_alphanumeric()).next())
        .unwrap_or("")
        .to_owned()
}

#[test]
fn a_reload_in_reading_mode_keeps_the_place_when_blocks_are_added_above_it() {
    on_both(|| {
        let added: String = (1..=40)
            .map(|n| format!("Line {n}\n\n"))
            .collect::<String>();
        let new = crate::typeset::parse::parse(&format!("Inserted one\n\nInserted two\n\n{added}"));
        let without = text_after_reload(None, settings::Follow::Off);
        assert_eq!(
            text_after_reload(Some(new.clone()), settings::Follow::Off),
            without
        );
        assert_eq!(text_after_reload(Some(new), settings::Follow::On), without);
    });
}

#[test]
fn without_images_m_says_why() {
    let terminal = ran(&numbers(3), Capabilities::ansi(80), &[b"M"]);
    let last = terminal.seen().last().unwrap().text();
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
        ..Inputs::new(screen())
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
        ..Inputs::new(screen())
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
    on_both(|| {
        let terminal = ran(&numbers(30), screen(), &[b"\x1b[<69;10;3M"]);
        let screens = terminal.seen();
        let shows_first = |seen: &Seen| seen.rows.iter().any(|row| row.text().trim() == "Line 1");
        assert!(shows_first(&screens[0]));
        assert!(
            !shows_first(screens.last().unwrap()),
            "{:?}",
            screens.last().unwrap().text()
        );
    });
}

#[test]
fn w_switches_wide_blocks_between_wrapping_and_scrolling() {
    on_both(|| {
        let long = "a".repeat(200);
        let document = crate::typeset::parse::parse(&format!("Text.\n\n```\n{long}\n```\n"));
        let mut terminal = Fake::new(80, 10, &[b"W", b"W"]);
        let mut inputs = Inputs {
            reading: Some(reading()),
            ..Inputs::new(screen())
        };
        run_document(
            &document,
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        );
        let screens = terminal.seen();
        // Wrapped, no band; scrolling, a band of four rows; wrapped again.
        assert_eq!(screens.len(), 3, "{} frames", screens.len());
        assert_eq!(bands(&screens[0]), 0);
        assert_eq!(bands(&screens[1]), 1, "{:?}", screens[1].text());
        assert_eq!(bands(&screens[2]), 0);
    });
}

#[test]
fn a_search_finds_a_laid_out_block_below_the_first_row_of_its_part() {
    on_both(|| {
        let source = "First paragraph.\n\nSecond paragraph, far down.\n";
        let document = crate::typeset::parse::parse(source);
        let mut terminal = Fake::new(80, 10, &[b"/far down\r"]);
        let mut inputs = Inputs {
            reading: Some(reading()),
            ..Inputs::new(screen())
        };
        run_document(
            &document,
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        );
        let last = terminal.seen().last().expect("a frame").text();
        assert!(!last.contains("not found"), "{last:?}");
    });
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

/// The terminal after two wide code blocks were shown as text, with reading
/// mode for `M` to turn on, when it gives `keys`.
fn text_frames(keys: &[&[u8]]) -> Fake {
    let long = "a".repeat(200);
    let source = format!("```\n{long}\n```\n\n```\n{long}\n```\n");
    let document = crate::typeset::parse::parse(&source);
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs::new(screen());
    let options = Options {
        reading: Some(reading()),
        ..Options::new(SearchCase::Smart)
    };
    run_document(&document, &mut inputs, options, &mut terminal);
    terminal
}

#[test]
fn w_in_text_then_m_turns_reading_mode_on_with_wide_blocks_scrolling() {
    on_both(|| {
        let terminal = text_frames(&[b"W", b"M"]);
        let screens = terminal.seen();
        // The text shows no images, whatever W did; M then shows the pages,
        // with the two code blocks as bands of four rows.
        let last = screens.last().expect("a frame");
        assert!(!screens[0].text().contains('⟦'));
        assert_eq!(bands(last), 2, "{:?}", last.text());
    });
}

#[test]
fn m_then_w_in_text_wraps_no_longer() {
    on_both(|| {
        let terminal = text_frames(&[b"M", b"W"]);
        let screens = terminal.seen();
        // M shows the wrapped blocks in the page; W then scrolls them.
        assert_eq!(bands(&screens[1]), 0, "{:?}", screens[1].text());
        assert_eq!(bands(&screens[2]), 2, "{:?}", screens[2].text());
    });
}

#[test]
fn w_twice_in_text_then_m_wraps() {
    on_both(|| {
        let terminal = text_frames(&[b"W", b"W", b"M"]);
        let last = terminal.seen().last().expect("a frame");
        assert!(shows_page(last));
        assert_eq!(bands(last), 0);
    });
}

/// What a call of the planner was given: the number of blocks in the
/// content, whether reading mode was on, and the cells.
type Asked = (usize, bool, Option<(u16, u16)>);

/// What each call of the planner was given while the pager showed
/// `numbers(3)` for `keys`.
fn planned_for(keys: &[&[u8]]) -> Vec<Asked> {
    let calls = std::cell::RefCell::new(Vec::new());
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs::new(screen());
    let mut then = Then::new([Reloaded::Document(numbers(5))]);
    let options = Options {
        reading: Some(reading()),
        files: Some(&mut then),
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
    on_both(|| {
        let terminal = ran(&numbers(3), screen(), &[b"M", b"M"]);
        let screens = terminal.seen();
        // The key is in bold, and the word after it in reverse video.
        let told = |seen: &Seen| {
            seen.text().contains("M reading")
                && seen.style_of(" reading").is_some_and(|style| style.reverse)
        };
        let reading = (screens.iter()).position(shows_page);
        let reading = reading.expect("a frame in reading mode");
        assert!(!told(&screens[0]), "{:?}", screens[0].text());
        assert!(told(&screens[reading]), "{:?}", screens[reading].text());
        let last = screens.last().unwrap();
        assert!(!told(last), "{:?}", last.text());
    });
}

/// The terminal after `document` was shown in reading mode in a window of
/// 80 × 10 cells, when it gives `keys`.
fn reading_frames(
    document: &Document,
    wide: crate::typeset::reading::page::Wide,
    keys: &[&[u8]],
) -> Fake {
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs {
        reading: Some(Reading { wide, ..reading() }),
        ..Inputs::new(screen())
    };
    run_document(
        document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    terminal
}

#[test]
fn a_search_marks_its_matches_in_the_page_and_shows_the_one_it_found() {
    on_both(|| {
        let filler = "A paragraph of filler.\n\n".repeat(30);
        let document = crate::typeset::parse::parse(&format!("{filler}Then the needle is here.\n"));
        let terminal = reading_frames(
            &document,
            crate::typeset::reading::page::Wide::Wrap,
            &[b"/needle\r"],
        );
        let frames = terminal.frames();
        // The first frame sends the page's parts in view; the search moves to
        // the match, whose part is drawn anew with the mark in it.
        assert!(!image_data(&frames[0]).is_empty());
        let last = frames.last().unwrap();
        assert!(!image_data(last).is_empty(), "{} frames", frames.len());
        let told = terminal.seen().last().unwrap().text();
        assert!(told.contains("1/1"), "{told:?}");
    });
}

#[test]
fn a_search_scrolls_a_band_to_its_match_and_draws_it_anew() {
    let long = "a".repeat(200);
    let document = crate::typeset::parse::parse(&format!("Text.\n\n```\n{long} needle\n```\n"));
    let terminal = reading_frames(
        &document,
        crate::typeset::reading::page::Wide::Scroll,
        &[b"/needle\r"],
    );
    let frames = terminal.frames();
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
fn a_link_in_a_page_is_a_hyperlink_over_its_cells() {
    on_both(|| {
        let document = crate::typeset::parse::parse("Go [here](https://e.example/) now.\n");
        let terminal = reading_frames(&document, crate::typeset::reading::page::Wide::Wrap, &[]);
        let first = &terminal.seen()[0];
        let cells: Vec<_> = (first.rows.iter().flat_map(|row| &row.cells))
            .filter(|cell| cell.style.link.as_deref() == Some("https://e.example/"))
            .collect();
        // Over the page's cells only, and not over the whole row.
        assert!(!cells.is_empty(), "{:?}", first.text());
        assert!(cells.len() < 10, "{} cells", cells.len());
        assert!(
            cells
                .iter()
                .all(|cell| cell.text.is_empty() || cell.text.starts_with("⟦page")),
            "{:?}",
            first.text()
        );
    });
}

#[test]
fn tab_selects_a_link_with_a_box_and_its_url_on_the_status_line_and_escape_lets_it_go() {
    on_both(|| {
        let document = crate::typeset::parse::parse(
            "A [first](https://one.example/) and [second](https://two.example/) link.\n",
        );
        let terminal = reading_frames(
            &document,
            crate::typeset::reading::page::Wide::Wrap,
            &[b"\t", b"\t", b"\x1b[Z", b"\x1b", b""],
        );
        let screens = terminal.seen();
        // The page's own hyperlinks name the urls too; the status line is the
        // last row.
        let told = |seen: &Seen| {
            let bar = seen.row(10);
            ["https://one.example/", "https://two.example/"]
                .iter()
                .find(|url| bar.contains(**url))
                .map(|url| url.to_string())
        };
        assert_eq!(told(&screens[0]), None);
        assert_eq!(told(&screens[1]).as_deref(), Some("https://one.example/"));
        assert_eq!(told(&screens[2]).as_deref(), Some("https://two.example/"));
        assert_eq!(told(&screens[3]).as_deref(), Some("https://one.example/"));
        assert_eq!(told(&screens[4]), None);
        // The selection is a box in the page's image: another image is sent.
        let frames = terminal.frames();
        assert!(!image_data(&frames[1]).is_empty(), "{:?}", frames[1]);
    });
}

#[test]
fn n_to_a_match_in_another_band_counts_it_and_the_count_stays_after_the_band_moves() {
    on_both(|| {
        let long = "a".repeat(200);
        let document = crate::typeset::parse::parse(&format!(
            "```\n{long} needle\n```\n\nText.\n\n```\n{long} needle\n```\n"
        ));
        let terminal = reading_frames(
            &document,
            crate::typeset::reading::page::Wide::Scroll,
            &[b"/needle\r", b"n"],
        );
        let screens = terminal.seen();
        // The second match is the current one in the frame after `n`, whose
        // band has scrolled to it: the layout that shows the band anew does not
        // start the search over.
        assert!(screens[1].text().contains("1/2"), "{:?}", screens[1].text());
        assert!(screens[2].text().contains("2/2"), "{:?}", screens[2].text());
    });
}

#[test]
fn tab_selects_a_link_without_hyperlinks_and_none_are_written() {
    on_both(|| {
        let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
        let mut terminal = Fake::new(80, 10, &[b"\t"]);
        let caps = Capabilities {
            hyperlinks: false,
            ..screen()
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
        let bar = terminal.seen()[1].row(10);
        assert!(bar.contains("https://one.example/"), "{bar:?}");
        assert!(
            frames.iter().all(|frame| !frame.contains("\x1b]8;")),
            "{frames:?}"
        );
    });
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
    let out = written(&document, screen(), &[b"\t", b"\r", b""]);
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
        ..Inputs::new(screen())
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
    let out = written(&document, screen(), &[b"\t", b"\r", b""]);
    assert!(!out.contains("\x1b]52;"), "in what was written");
    assert!(!out.contains("link copied"), "in what was written");
}

#[test]
fn enter_with_the_selected_link_off_the_screen_scrolls_and_copies_nothing() {
    on_both(|| {
        let filler = "A paragraph of filler.\n\n".repeat(30);
        let document = crate::typeset::parse::parse(&format!(
            "A [first](https://one.example/) link.\n\n{filler}"
        ));
        // Tab selects the link, and three pages down it is off the screen.
        let keys: [&[u8]; 6] = [b"\t", b"\x06", b"\x06", b"\x06", b"\r", b""];
        let mut terminal = Fake::new(80, 10, &keys);
        let mut inputs = Inputs {
            reading: Some(reading()),
            ..Inputs::new(screen())
        };
        run_document(
            &document,
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        );
        let screens = terminal.seen();
        let before = &screens[screens.len() - 2];
        let last = screens.last().unwrap();
        assert_ne!(last, before, "the view did not move");
        let out = String::from_utf8(terminal.out).unwrap();
        assert!(!out.contains("\x1b]52;"), "in what was written");
    });
}

#[test]
fn the_status_line_tells_a_selected_link_s_address_only_while_it_is_on_the_screen() {
    on_both(|| {
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
            ..Inputs::new(screen())
        };
        run_document(
            &document,
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        );
        let told: Vec<bool> = (terminal.seen().iter())
            .map(|seen| seen.row(10).contains("https://one.example/"))
            .collect();
        // At the start none is selected; then it is, then it is off the screen
        // for three pages down and two back, and then it is there again.
        assert_eq!(
            told,
            [false, true, false, false, false, false, false, true],
            "{told:?}"
        );
    });
}

#[test]
fn in_tmux_with_passthrough_the_copy_goes_through_to_kitty() {
    let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
    let caps = Capabilities {
        tmux_passthrough: true,
        ..screen()
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
    on_both(|| {
        let long = "x".repeat(150);
        let document = crate::typeset::parse::parse(&format!(
            "| a | b |\n|---|---|\n| [near](https://one.example/) | {long}[far](https://two.example/) |\n"
        ));
        let terminal = reading_frames(
            &document,
            crate::typeset::reading::page::Wide::Scroll,
            &[b"\t", b"\t", b"\t", b""],
        );
        // The status line is the last row.
        let told = |seen: &Seen| {
            let bar = seen.row(10);
            ["https://one.example/", "https://two.example/"]
                .iter()
                .find(|url| bar.contains(**url))
                .map(|url| url.to_string())
        };
        let told: Vec<Option<String>> = terminal.seen().iter().map(told).collect();
        assert_eq!(told.get(1), Some(&Some("https://one.example/".to_owned())));
        assert_eq!(told.get(2), Some(&Some("https://two.example/".to_owned())));
        // On round, the band scrolled back to the first.
        assert_eq!(told.get(3), Some(&Some("https://one.example/".to_owned())));
    });
}

#[test]
fn a_selected_link_in_a_band_scrolled_clear_of_it_is_not_on_the_screen() {
    on_both(|| {
        let long = "x".repeat(150);
        let document = crate::typeset::parse::parse(&format!(
            "| a | b |\n|---|---|\n| near | {long}[far](https://two.example/) |\n"
        ));
        // Tab scrolls the band to the link; `H` then scrolls it back by
        // most of its width, three times, to the start.
        let terminal = reading_frames(
            &document,
            crate::typeset::reading::page::Wide::Scroll,
            &[b"\t", b"H", b"H", b"H", b""],
        );
        let told: Vec<bool> = (terminal.seen().iter())
            .map(|seen| seen.row(10).contains("https://two.example/"))
            .collect();
        assert_eq!(told.get(1), Some(&true), "{told:?}");
        assert_eq!(told.last(), Some(&false), "{told:?}");
    });
}

#[test]
fn the_status_line_cuts_a_long_link_in_the_middle() {
    on_both(|| {
        let url = "https://example.org/a/very/long/path/that/goes/on/and/on/to/the/page.html";
        let document = crate::typeset::parse::parse(&format!("A [link]({url}) in a line.\n"));
        let mut terminal = Fake::new(60, 10, &[b"\t", b""]);
        let mut inputs = Inputs {
            reading: Some(reading()),
            ..Inputs::new(screen())
        };
        run_document(
            &document,
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        );
        let bar = terminal.seen()[1].row(10);
        assert!(bar.contains("https://example.org/"), "{bar:?}");
        assert!(bar.contains("…"), "{bar:?}");
        assert!(bar.contains("/page.html"), "{bar:?}");
    });
}

#[test]
fn b_paints_the_rows_around_the_page_too() {
    let mut caps = screen();
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
