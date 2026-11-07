//! What the highlights over the document look like together: the colouring
//! after a reload, the search's matches and the selected link, in the text
//! layout and in reading mode, and what the status line tells of them.

use std::io::Read;
use std::rc::Rc;

use super::*;
use crate::present::seen::Cell;
use crate::session::search::Search;
use crate::session::status::Found;
use crate::typeset::style::{Line, Span};

/// Matches in yellow and the current one in orange, as text colours and
/// not as reverse video.
fn coloured() -> crate::typeset::theme::Theme {
    crate::typeset::theme::parse(
        "[search]\nmatch_foreground = \"#101010\"\nmatch_background = \"#e0e000\"\n\
         current_foreground = \"#101010\"\ncurrent_background = \"#d04000\"\n",
    )
    .unwrap()
}

const YELLOW: Color = Color::Rgb(224, 224, 0);
const ORANGE: Color = Color::Rgb(208, 64, 0);

/// The tint of the colouring after a reload on `known_colours`.
const TINT: Color = Color::Rgb(30, 30, 30);

/// The terminal after `old` was shown, reloaded to `new` by `r` while
/// following on auto, and then given `keys`, in a window of 60 × 21.
fn followed(old: &Document, new: Document, keys: &[&[u8]]) -> Fake {
    let mut then = Then::new([Reloaded::Document(new)]);
    let mut reads = vec![b"r".as_slice()];
    reads.extend(keys);
    let mut terminal = Fake::new(60, 21, &reads);
    let mut inputs = Inputs {
        theme: coloured(),
        ..Inputs::new(known_colours())
    };
    run_document(
        old,
        &mut inputs,
        Options {
            files: Some(&mut then),
            follow: settings::Follow::Auto,
            jump: FollowJump::First,
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    terminal
}

#[test]
fn a_match_on_a_line_with_the_colouring_of_a_reload_has_the_match_s_background() {
    on_both(|| {
        let new = super::following::numbers_changed(40, &[30, 31, 32]);
        // "Line 3" is found in each of the changed lines, and the first of them
        // is the current match, which fades in; the next ones are not.
        let terminal = followed(&numbers(40), new, &[b"/Line 3\r"]);
        let screens = terminal.seen();
        // The background of `text` on screen row `row`.
        let colour =
            |seen: &Seen, row: u16, text: &str| seen.style_in(row, text).and_then(|s| s.bg);
        // Before the search the lines of the change have the tint.
        let tinted = &screens[2];
        assert_eq!(colour(tinted, 6, "Line 31 changed"), Some(TINT));
        // After it the match is in the match's colours, and what follows it on
        // the line is still tinted.
        let last = screens.last().unwrap();
        assert_eq!(colour(last, 6, "Line 3"), Some(YELLOW), "{}", last.text());
        assert_eq!(colour(last, 8, "Line 3"), Some(YELLOW), "{}", last.text());
        assert_eq!(colour(last, 6, "1 changed"), Some(TINT), "{}", last.text());
        assert_eq!(colour(last, 8, "2 changed"), Some(TINT), "{}", last.text());
    });
}

/// The frames drawn for `document` in text, in a window of 60 × 6, when
/// the terminal gives `keys`.
fn text_frames(document: &Document, keys: &[&[u8]]) -> Fake {
    let mut terminal = Fake::new(60, 6, keys);
    // Kitty, for the hyperlinks that the links are of.
    let caps = Capabilities {
        truecolor: true,
        foreground: known_colours().foreground,
        background: known_colours().background,
        ..screen_caps(80)
    };
    let mut inputs = Inputs {
        theme: coloured(),
        ..Inputs::new(caps)
    };
    run_document(
        document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    terminal
}

/// A link of which the match is the first word, and a second match.
const LINKED: &str = "See [kiri link](https://one.example/) and kiri.\n";

/// The keys that search for `kiri`, wait out the fade of the jump and
/// select the link.
fn search_and_select() -> Vec<&'static [u8]> {
    let mut keys: Vec<&[u8]> = vec![b"/kiri\r"];
    keys.extend([b"".as_slice(); 30]);
    keys.push(b"\t");
    keys
}

#[test]
fn a_match_in_the_selected_link_in_text_keeps_the_match_s_colours() {
    on_both(|| {
        let document = crate::typeset::parse::parse(LINKED);
        let mut keys = search_and_select();
        keys.push(b"");
        let terminal = text_frames(&document, &keys);
        let seen = terminal.seen().last().unwrap();
        let style = |text: &str| seen.style_in(1, text).expect(text);
        // The match is in its own colours, not in reverse video; the rest of
        // the link is in reverse video, and the text outside it is plain.
        let kiri = style("kiri");
        assert_eq!(kiri.bg, Some(ORANGE), "{}", seen.text());
        assert!(!kiri.reverse, "{}", seen.text());
        assert!(style(" link").reverse, "{}", seen.text());
        assert!(!style(" and ").reverse, "{}", seen.text());
        // The other match is in the match colour, as before.
        assert_eq!(style("kiri.").bg, Some(YELLOW), "{}", seen.text());
    });
}

#[test]
fn escape_takes_the_matches_and_the_selected_link_away_in_text() {
    on_both(|| {
        let document = crate::typeset::parse::parse(LINKED);
        let mut keys = search_and_select();
        keys.extend([b"\x1b".as_slice(), b""]);
        let terminal = text_frames(&document, &keys);
        let screens = terminal.seen();
        let before = &screens[screens.len() - 2];
        let after = screens.last().unwrap();
        assert!(before.text().contains("https://one.example/"), "{before:?}");
        // The cells of the document, above the status line.
        let cells = |seen: &Seen| -> Vec<Cell> {
            (seen.rows[..5].iter().flat_map(|row| row.cells.clone())).collect()
        };
        assert!(cells(before).iter().any(|cell| cell.style.reverse));
        let shown = cells(after);
        assert!(shown.iter().all(|cell| !cell.style.reverse), "{after:?}");
        assert!(
            shown.iter().all(|cell| cell.style.bg.is_none()),
            "{after:?}"
        );
        // Neither the link nor the search is told, and the status line has the
        // name again.
        let status = after.row(6);
        assert!(!status.contains("https://one.example/"), "{status:?}");
        assert!(!status.contains("/kiri"), "{status:?}");
        assert!(status.contains("notes.md"), "{status:?}");
    });
}

/// The pixels of each image `frame` sends, as RGBA.
fn pixels(frame: &str) -> Vec<Vec<u8>> {
    use base64::Engine;
    let mut images = Vec::new();
    let mut data = String::new();
    let mut rest = frame;
    while let Some(start) = rest.find("\x1b_G") {
        let body = &rest[start + 3..];
        let end = body.find("\x1b\\").unwrap();
        let (keys, chunk) = body[..end].split_once(';').unwrap_or((&body[..end], ""));
        data.push_str(chunk);
        if keys.split(',').any(|key| key == "m=0") {
            let compressed = base64::engine::general_purpose::STANDARD
                .decode(&data)
                .unwrap();
            let mut rgba = Vec::new();
            flate2::read::ZlibDecoder::new(&compressed[..])
                .read_to_end(&mut rgba)
                .unwrap();
            images.push(rgba);
            data.clear();
        }
        rest = &body[end + 2..];
    }
    images
}

/// How many pixels of the images `frame` sends are `rgb`, give or take 3
/// a channel.
fn count(frame: &str, rgb: [u8; 3]) -> usize {
    (pixels(frame).iter())
        .flat_map(|image| image.chunks(4))
        .filter(|pixel| (0..3).all(|at| pixel[at].abs_diff(rgb[at]) <= 3))
        .count()
}

/// How many pixels of the images `frame` sends are yellow with some of
/// another colour mixed in: red and green high, blue low.
fn yellowish(frame: &str) -> usize {
    (pixels(frame).iter())
        .flat_map(|image| image.chunks(4))
        .filter(|pixel| pixel[0] > 180 && pixel[1] > 180 && pixel[2] < 130)
        .count()
}

/// How many pixels of the sixels `frame` writes are within `tolerance` of
/// `rgb` in each channel, as a terminal would draw them. A sixel colour has
/// whole percentages, so it is a few steps off.
fn sixel_count(frame: &str, rgb: [u8; 3], tolerance: u8) -> usize {
    (frame.split("\x1bP").skip(1))
        .map(|rest| {
            let end = rest.find("\x1b\\").expect("a sixel ends with ST");
            crate::present::sixel::tests::decode(format!("\x1bP{}\x1b\\", &rest[..end]).as_bytes())
        })
        .flat_map(|sixel| sixel.pixels)
        .flatten()
        .filter(|pixel| (0..3).all(|at| pixel[at].abs_diff(rgb[at]) <= tolerance))
        .count()
}

/// The match colours of reading mode on a theme that sets none.
const BROWSER_CURRENT: [u8; 3] = [0xff, 0x96, 0x32];
const BROWSER_FOUND: [u8; 3] = [0xff, 0xff, 0x00];

/// The frames drawn in reading mode for a page with a link and a word
/// that is a match twice, the second one in the link's text.
fn reading_page(keys: &[&[u8]]) -> Fake {
    let document = crate::typeset::parse::parse(
        "A needle, then [a needle link](https://one.example/) and more.\n",
    );
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs {
        reading: Some(Reading {
            pictures: Default::default(),
            fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
        }),
        ..Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..screen_caps(80)
        })
    };
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    terminal
}

#[test]
fn a_match_and_the_selected_link_are_drawn_on_one_page_in_reading_mode() {
    on_both(|| {
        let terminal = reading_page(&[b"/needle\r", b"\t", b""]);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 3, "{} frames", frames.len());
        // What the search and the selection draw in the page is in the
        // pixels that kitty is sent; a sixel terminal is written the rows
        // that changed, and each frame writes some.
        if on_sixel() {
            assert!(!image_data(&frames[1]).is_empty(), "{:?}", frames[1]);
            assert!(!image_data(&frames[2]).is_empty(), "{:?}", frames[2]);
            assert_ne!(image_data(&frames[1]), image_data(&frames[2]));
            // The rows are decoded: the search has both matches, the first
            // one current; selecting the link covers the exact yellow of
            // the match in it.
            assert!(sixel_count(&frames[1], BROWSER_CURRENT, 3) > 100);
            assert!(sixel_count(&frames[1], BROWSER_FOUND, 3) > 100);
            assert!(sixel_count(&frames[2], BROWSER_CURRENT, 3) > 100);
            assert_eq!(sixel_count(&frames[2], BROWSER_FOUND, 3), 0);
            assert_eq!(sixel_count(&frames[0], BROWSER_FOUND, 3), 0);
        } else {
            // The search draws the page with both matches, the first one
            // current.
            assert!(count(&frames[1], BROWSER_CURRENT) > 100);
            assert!(count(&frames[1], BROWSER_FOUND) > 100);
            // Selecting the link draws it again, with the current match
            // still in the colours of a match, and the match in the link
            // still yellow under the link's frame.
            assert!(count(&frames[2], BROWSER_CURRENT) > 100);
            // The link's translucent box is drawn over the box of the match
            // in it, so that it is no longer the exact yellow, but still
            // yellowish.
            assert_eq!(count(&frames[2], BROWSER_FOUND), 0);
            assert!(yellowish(&frames[2]) > 100, "{}", yellowish(&frames[2]));
            assert_eq!(yellowish(&frames[0]), 0);
            assert_ne!(pixels(&frames[1]), pixels(&frames[2]));
        }
        let screens = terminal.seen();
        assert!(
            screens[2].row(10).contains("https://one.example/"),
            "{:?}",
            screens[2].text()
        );
    });
}

#[test]
fn escape_takes_the_matches_and_the_selected_link_away_in_reading_mode() {
    on_both(|| {
        let terminal = reading_page(&[b"/needle\r", b"\t", b"\x1b", b""]);
        let (frames, screens) = (terminal.frames(), terminal.seen());
        let last = frames.last().unwrap();
        assert_eq!(frames.len(), 4, "{} frames", frames.len());
        assert!(
            screens[2].row(10).contains("https://one.example/"),
            "{:?}",
            screens[2].text()
        );
        // The page was drawn with its marks before; what is sent now has
        // neither them nor the box. (A sixel terminal is written the rows
        // that changed, which are then those that had them.)
        if on_sixel() {
            assert!(!image_data(last).is_empty(), "{last:?}");
            assert!(sixel_count(&frames[2], BROWSER_CURRENT, 3) > 100);
            assert_eq!(sixel_count(last, BROWSER_CURRENT, 3), 0);
            assert_eq!(sixel_count(last, BROWSER_FOUND, 3), 0);
        } else {
            assert!(count(&frames[2], BROWSER_CURRENT) > 100);
            assert_eq!(count(last, BROWSER_CURRENT), 0);
            assert_eq!(count(last, BROWSER_FOUND), 0);
        }
        let bar = screens.last().unwrap().row(10);
        assert!(!bar.contains("https://one.example/"), "{bar:?}");
        assert!(!bar.contains("needle"), "{bar:?}");
    });
}

#[test]
fn the_status_line_tells_a_message_before_nowhere_before_a_place() {
    let lines = [Line::new(vec![Span::new("a needle", Default::default())])];
    let mut state = State::new(SearchCase::Smart);
    state.search = Some(Search::new(&lines, "needle", SearchCase::Smart, 0));
    state.shown = true;
    state.nowhere = true;
    state.told = Some(Told::new("link copied"));
    fn found(state: &State) -> Option<Found<'_>> {
        state.found(None, None)
    }
    assert_eq!(
        found(&state),
        Some(Found::Told {
            message: "link copied",
            fading: None,
        })
    );
    state.told = None;
    assert_eq!(found(&state), Some(Found::Nowhere("needle")));
    state.nowhere = false;
    assert_eq!(
        found(&state),
        Some(Found::At {
            term: "needle",
            current: 1,
            total: 1,
            flash: None,
        })
    );
    // A search that is not shown tells nothing.
    state.shown = false;
    assert_eq!(found(&state), None);
}
