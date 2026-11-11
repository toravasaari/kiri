//! Animated images in the pager: nothing plays until the reader starts it;
//! when the terminal is told to play and stop them, and what is sent.

use super::screen::Shown;
use super::*;
use crate::typeset::images::Source;
use crate::typeset::images::animation::{Animation, Frame, Plays};

/// An animation of a red, a green and a blue frame, each shown for `gap`.
fn clip(gap: Duration, plays: Plays) -> Source {
    let frames = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
        .into_iter()
        .map(|rgba| Frame {
            image: image::RgbaImage::from_pixel(20, 20, image::Rgba(rgba)),
            gap,
        })
        .collect();
    Source::Animated(std::sync::Arc::new(Animation {
        frames,
        plays,
        over_budget: false,
    }))
}

/// A document of `lines` lines below an image `anim.gif`, and inputs for
/// kitty that hold it as a clip of frames of a second each.
fn animated(lines: usize, plays: Plays) -> (Document, Inputs) {
    let mut source = String::from("![an animation](anim.gif)\n\n");
    for line in 0..lines {
        source.push_str(&format!("line {line}\n\n"));
    }
    let document = crate::typeset::parse::parse(&source);
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs
        .images
        .insert("anim.gif", Ok(clip(Duration::from_secs(1), plays)));
    (document, inputs)
}

/// All the pager writes for `document` and `inputs` when the keys `reads`
/// come, one a read.
fn played(inputs: (Document, Inputs), reads: &[&[u8]]) -> (String, Fake) {
    played_in(6, inputs, reads)
}

/// [`played`] in a window of `columns` columns and `rows` rows.
fn played_wide(
    columns: u16,
    rows: u16,
    (document, mut inputs): (Document, Inputs),
    reads: &[&[u8]],
) -> (String, Fake) {
    let mut terminal = Fake::new(columns, rows, reads);
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    (String::from_utf8(terminal.out.clone()).unwrap(), terminal)
}

/// [`played`] in a window of `rows` rows.
fn played_in(
    rows: u16,
    (document, mut inputs): (Document, Inputs),
    reads: &[&[u8]],
) -> (String, Fake) {
    let mut terminal = Fake::new(60, rows, reads);
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    (String::from_utf8(terminal.out.clone()).unwrap(), terminal)
}

/// `n` reads that give no key.
fn idle(n: usize) -> Vec<&'static [u8]> {
    vec![b"".as_slice(); n]
}

/// A click on the first row of the window, where the image is: it starts
/// the animation, or stops it. A click selects no image, so no framed
/// image comes of it.
const CLICK: &[u8] = b"\x1b[<0;3;1M\x1b[<0;3;1m";

/// What the first image told says, in order: a selected animation is also
/// sent as a framed image, which is told what the plain one is.
fn told(out: &str) -> Vec<String> {
    let all = controlled(out);
    let first = all.first().map(|(id, _)| *id);
    (all.into_iter())
        .filter(|(id, _)| Some(*id) == first)
        .map(|(_, keys)| keys)
        .collect()
}

#[test]
fn an_animation_is_sent_as_its_first_frame_and_nothing_plays_by_itself() {
    let mut reads = idle(40);
    reads.push(b"q");
    let (out, _) = played(animated(0, Plays::Forever), &reads);
    // Its first frame only, and never started: the frames are sent when the
    // reader starts it.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 1, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 0, "{out:?}");
    assert_eq!(told(&out), Vec::<&str>::new(), "{out:?}");
    let reads: [&[u8]; 3] = [b"", CLICK, b"q"];
    let (out, _) = played(animated(0, Plays::Forever), &reads);
    // Then with every frame and the first one's gap.
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    assert!(out.contains(",r=1,z=1000\x1b\\"), "{out:?}");
}

#[test]
fn a_start_plays_as_often_as_the_file_says_up_to_the_setting() {
    let reads: [&[u8]; 4] = [b"", b"\t", b"p", b"q"];
    // A file that loops forever plays the setting's 3 times: a loop count
    // of 4, as kitty counts. A file of 2 plays plays 2 times, one of 5, 3.
    for (plays, expected) in [
        (Plays::Forever, "c=1,s=3,v=4"),
        (Plays::Times(2), "c=1,s=3,v=3"),
        (Plays::Times(5), "c=1,s=3,v=4"),
    ] {
        let (out, _) = played(animated(0, plays), &reads);
        assert_eq!(told(&out), [expected], "{plays:?}: {out:?}");
    }
}

#[test]
fn with_the_setting_at_0_a_file_that_loops_forever_plays_without_end() {
    let (document, mut inputs) = animated(0, Plays::Forever);
    let mut terminal = Fake::new(60, 6, &[b"", b"\t", b"p", b"q"]);
    run_document(
        &document,
        &mut inputs,
        Options {
            animation: Pacing {
                plays: 0,
                ..Pacing::quick()
            },
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    assert_eq!(told(&out), ["c=1,s=3,v=1"], "{out:?}");
}

#[test]
fn p_starts_the_animation_selected_and_a_second_p_stops_it_on_its_first_frame() {
    let reads: [&[u8]; 5] = [b"\t", b"p", b"", b"p", b"q"];
    let (out, terminal) = played(animated(0, Plays::Forever), &reads);
    assert_eq!(told(&out), ["c=1,s=3,v=4", "s=1", "c=1"], "{out:?}");
    // The status line says what p did.
    let frames = terminal.frames();
    // (The pill takes room, so that a message is cut short at 60 columns.)
    assert!(frames.iter().any(|frame| frame.contains("play")));
    assert!(frames.iter().any(|frame| frame.contains("stop")));
}

#[test]
fn p_with_nothing_selected_starts_the_first_animation_on_the_screen() {
    let reads: [&[u8]; 3] = [b"", b"p", b"q"];
    let (out, _) = played_in(14, stops(), &reads);
    // The first of the two on the screen, and not the second.
    assert_eq!(told(&out), ["c=1,s=3,v=4"], "{out:?}");
    let starts = controlled(&out)
        .into_iter()
        .filter(|(_, keys)| keys.starts_with("c=1,s=3"))
        .map(|(id, _)| id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(starts.len(), 1, "{starts:?}");
}

#[test]
fn p_with_no_animation_on_the_screen_says_so() {
    // Twelve lines of text, and the window shows the end of them.
    let (document, inputs) = animated(12, Plays::Forever);
    let reads: [&[u8]; 4] = [b"G", b"", b"p", b"q"];
    let (out, terminal) = played_in(5, (document, inputs), &reads);
    assert_eq!(told(&out), Vec::<String>::new());
    assert!(
        terminal
            .frames()
            .iter()
            .any(|frame| frame.contains("Tab selects an animation")),
        "{out:?}"
    );
}

#[test]
fn a_click_on_an_animation_starts_it_and_a_second_click_stops_it() {
    // The image is on the first rows of the window.
    let click: &[u8] = b"\x1b[<0;3;1M\x1b[<0;3;1m";
    let reads: [&[u8]; 5] = [b"", click, b"", click, b"q"];
    let (out, _) = played(animated(0, Plays::Forever), &reads);
    assert_eq!(told(&out), ["c=1,s=3,v=4", "s=1", "c=1"], "{out:?}");
}

#[test]
fn a_click_elsewhere_starts_nothing() {
    let click: &[u8] = b"\x1b[<0;3;5M\x1b[<0;3;5m";
    let (out, _) = played(animated(12, Plays::Forever), &[b"", click, b"q"]);
    assert_eq!(told(&out), Vec::<&str>::new(), "{out:?}");
}

/// Inputs for Ghostty, which shows the first frame of an animation and
/// leaves the rest to kiri.
fn ghostty(document_inputs: (Document, Inputs)) -> (Document, Inputs) {
    use crate::typeset::caps::{Graphics, KittyGraphics, Playback};
    let (document, mut inputs) = document_inputs;
    inputs.caps.graphics = Graphics::Kitty(KittyGraphics {
        max_image_cells: crate::present::kitty::graphics::MAX_CELLS,
        playback: Playback::Redraw,
        terminal: Some("ghostty 1.1.3".into()),
    });
    (document, inputs)
}

/// The id of the image that `frame` shows in its placeholders, from the
/// foreground colour that carries it.
fn image_id_of(frame: &str) -> Option<u32> {
    let at = frame.find("\x1b[38;2;")? + "\x1b[38;2;".len();
    let rgb: Vec<u32> = (frame[at..].split('m').next()?)
        .split(';')
        .filter_map(|n| n.parse().ok())
        .collect();
    Some(rgb[0] << 16 | rgb[1] << 8 | rgb[2])
}

/// The id of the image that each frame of `out` shows in its placeholders.
fn shown_ids(terminal: &Fake) -> Vec<u32> {
    terminal
        .frames()
        .iter()
        .filter_map(|frame| image_id_of(frame))
        .collect()
}

#[test]
fn in_ghostty_nothing_is_redrawn_until_the_reader_starts_it() {
    let mut reads = idle(30);
    reads.push(b"q");
    let (out, terminal) = played(ghostty(animated(0, Plays::Forever)), &reads);
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 1, "{out:?}");
    assert_eq!(shown_ids(&terminal).len(), 1);
}

#[test]
fn in_ghostty_kiri_redraws_the_image_with_each_frame_at_its_gap() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(60));
    reads.push(b"q");
    let (out, terminal) = played(ghostty(animated(0, Plays::Forever)), &reads);
    // Every frame is a still image of its own, sent when it is first shown,
    // and the terminal is told nothing else.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 3, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 0, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=a,").count(), 0, "{out:?}");
    let ids = shown_ids(&terminal);
    let mut changes = ids.clone();
    changes.dedup();
    // Frame 0, then 1, 2, 0, 1 ... a second each.
    assert!(changes.len() >= 5, "{changes:?}");
    assert_eq!(changes[0], changes[3], "{changes:?}");
    assert_ne!(changes[0], changes[1]);
    assert_ne!(changes[1], changes[2]);
    // Drawn when a frame changes or the pill moves, four times a second at
    // most, and not at every poll.
    assert!(ids.len() < 40, "{} frames drawn", ids.len());
}

#[test]
fn in_ghostty_what_plays_its_plays_rests_on_its_last_frame() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(120));
    reads.push(b"q");
    let (_, terminal) = played(ghostty(animated(0, Plays::Times(1))), &reads);
    let ids = shown_ids(&terminal);
    let mut changes = ids.clone();
    changes.dedup();
    // The frames, the last one resting: it is drawn when it comes, and not
    // with every poll after it. (The end, and the status line's message
    // fading, draw it a few times more.) The picture is too small for marks, so its first
    // frame is the still image and its last the ended one.
    assert_eq!(changes.len(), 3, "{changes:?}");
    // 120 polls came: not one draw each.
    assert!(ids.len() < 40, "{ids:?}");
}

#[test]
fn in_ghostty_a_click_stops_it_and_it_is_back_on_its_first_frame() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(14));
    reads.push(CLICK);
    reads.extend(idle(10));
    reads.push(b"q");
    let (_, terminal) = played(ghostty(animated(0, Plays::Forever)), &reads);
    let ids = shown_ids(&terminal);
    let first = ids[0];
    assert_eq!(*ids.last().unwrap(), first, "it ends on its first frame");
    let told = |text: &str| terminal.frames().iter().any(|f| f.contains(text));
    assert!(told("stop") && told("play"));
}

/// A document with the animation `anim.gif`, the still `still.png` and a
/// second animation `other.gif`, each under a line of text.
fn stops() -> (Document, Inputs) {
    let source = "![first](anim.gif)\n\nText\n\n![still](still.png)\n\n![second](other.gif)\n";
    let document = crate::typeset::parse::parse(source);
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    // Different gaps, so that they are two images.
    for (name, secs) in [("anim.gif", 1), ("other.gif", 2)] {
        let clip = clip(Duration::from_secs(secs), Plays::Forever);
        inputs.images.insert(name, Ok(clip));
    }
    inputs
        .images
        .insert("still.png", Ok(square([0, 0, 255, 255])));
    (document, inputs)
}

/// The status line of the screen after each frame, as text.
fn statuses(terminal: &Fake) -> Vec<String> {
    terminal
        .screens()
        .iter()
        .map(|screen| {
            screen_text(screen)
                .rsplit_once("\x1b[0;1m kiri")
                .map_or("", |(_, bar)| bar)
                .to_owned()
        })
        .collect()
}

#[test]
fn tab_stops_at_animated_images_and_the_status_line_names_the_one_selected() {
    let (_, terminal) = played_in(14, stops(), &[b"\t", b"\t", b"\t", b"q"]);
    let bars = statuses(&terminal);
    assert!(!bars[0].contains(".gif"), "{bars:?}");
    assert!(bars[1].contains("first"), "{bars:?}");
    // Not at the still image, which has no stop: the next is the second
    // animation, and then the first again.
    assert!(bars[2].contains("second"), "{bars:?}");
    assert!(bars[3].contains("first"), "{bars:?}");
}

#[test]
fn shift_tab_goes_back_from_the_last_animated_image() {
    let (_, terminal) = played_in(14, stops(), &[b"\x1b[Z", b"\x1b[Z", b"q"]);
    let bars = statuses(&terminal);
    assert!(bars[1].contains("second"), "{bars:?}");
    assert!(bars[2].contains("first"), "{bars:?}");
}

#[test]
fn tab_only_selects_and_starts_nothing() {
    let (out, _) = played_in(14, stops(), &[b"\t", b"\t", b"q"]);
    assert_eq!(told(&out), Vec::<&str>::new(), "{out:?}");
}

#[test]
fn the_selected_animated_image_is_sent_again_with_a_highlighted_frame() {
    let (out, _) = played_in(14, stops(), &[b"\t", b"q"]);
    // Plain, and again with the frame; the first has the frame in no pixel
    // of its edge. Both go with their frames.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 4, "{out:?}");
}

#[test]
fn enter_on_the_selected_animation_starts_it_and_a_second_enter_stops_it() {
    let (out, terminal) = played_in(14, stops(), &[b"\t", b"\r", b"", b"\r", b"q"]);
    assert!(
        told(&out).iter().any(|c| c.starts_with("c=1,s=3")),
        "{out:?}"
    );
    assert!(told(&out).iter().any(|c| c == "s=1"), "{out:?}");
    // And does not copy anything.
    assert!(!out.contains("\x1b]52;"), "{out:?}");
    assert!(
        !statuses(&terminal)
            .iter()
            .any(|bar| bar.contains("link copied"))
    );
}

#[test]
fn starting_another_animation_stops_the_one_that_plays() {
    // The first is started, then Tab goes to the second, which is started.
    let reads: [&[u8]; 6] = [b"\t", b"p", b"", b"\t", b"p", b"q"];
    let (out, _) = played_in(14, stops(), &reads);
    let all = controlled(&out);
    let starts: Vec<usize> = (0..all.len())
        .filter(|at| all[*at].1.starts_with("c=1,s=3"))
        .collect();
    let ids: std::collections::BTreeSet<_> = starts.iter().map(|at| all[*at].0).collect();
    assert!(ids.len() >= 2, "two animations start: {all:?}");
    // The first is stopped before the second starts.
    let stop = all
        .iter()
        .position(|(_, keys)| keys == "s=1")
        .expect("a stop");
    let second = all
        .iter()
        .rposition(|(_, k)| k.starts_with("c=1,s=3"))
        .unwrap();
    assert!(stop < second, "{all:?}");
}

#[test]
fn a_linked_animation_is_one_stop_and_enter_copies_its_address_and_p_starts_it() {
    let source = "Text [![linked](anim.gif)](https://example.com/x) more.\n";
    let document = crate::typeset::parse::parse(source);
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs
        .images
        .insert("anim.gif", Ok(clip(Duration::from_secs(1), Plays::Forever)));
    let (out, terminal) = played((document, inputs), &[b"\t", b"\t", b"\r", b"p", b"q"]);
    let bars = statuses(&terminal);
    // One stop: the address, not the file, and a second Tab stays on it.
    assert!(bars[1].contains("https://example.com/x"), "{bars:?}");
    assert!(
        !bars.iter().any(|bar| bar.contains("an animation")),
        "{bars:?}"
    );
    assert!(
        out.contains("\x1b]52;"),
        "enter copies the address: {out:?}"
    );
    // Enter did not start it; p did.
    assert_eq!(told(&out), ["c=1,s=3,v=4"], "{out:?}");
}

#[test]
fn a_click_on_a_linked_animation_does_not_start_it() {
    let source = "![plain](anim.gif)\n\n[![linked](other.gif)](https://example.com/x) text\n";
    let document = crate::typeset::parse::parse(source);
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    for name in ["anim.gif", "other.gif"] {
        inputs
            .images
            .insert(name, Ok(clip(Duration::from_secs(1), Plays::Forever)));
    }
    // The plain image is on the first row, the linked one on the third.
    let (out, _) = played_in(
        14,
        (document, inputs),
        &[
            b"\x1b[<0;3;1M\x1b[<0;3;1m",
            b"\x1b[<0;3;5M\x1b[<0;3;5m",
            b"q",
        ],
    );
    assert_eq!(told(&out), ["c=1,s=3,v=4"], "{out:?}");
}

#[test]
fn what_plays_stops_on_any_move_of_the_view_even_when_it_stays_in_view() {
    // Two lines down is a scroll that keeps the picture on the screen.
    let reads: [&[u8]; 5] = [b"\t", b"p", b"", b"j", b"q"];
    let (out, _) = played_in(14, animated(12, Plays::Forever), &reads);
    assert_eq!(told(&out), ["c=1,s=3,v=4", "s=1", "c=1"], "{out:?}");
}

#[test]
fn a_resize_stops_what_plays() {
    let (document, mut inputs) = animated(0, Plays::Forever);
    let reads: [&[u8]; 5] = [b"", CLICK, b"", b"", b"q"];
    let mut terminal = Fake::new(60, 6, &reads);
    terminal.sizes = [window(60, 6), window(60, 6), window(60, 6), window(70, 6)].into();
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    // Started, and stopped on its first frame by the resize, which puts the
    // still image back in its place: the image of the first frame, as the
    // first draw had it.
    assert_eq!(told(&out), ["c=1,s=3,v=4", "s=1", "c=1"], "{out:?}");
    let ids = shown_ids(&terminal);
    assert!(ids.iter().any(|id| *id != ids[0]), "it played: {ids:?}");
    assert_eq!(ids.last(), ids.first(), "{ids:?}");
}

#[test]
fn a_search_and_the_help_stop_what_plays() {
    for opening in [b"?".as_slice(), b"/"] {
        let reads: [&[u8]; 5] = [b"\t", b"p", b"", opening, b"q"];
        let (out, _) = played_in(14, animated(12, Plays::Forever), &reads);
        assert_eq!(
            &told(&out)[..3],
            ["c=1,s=3,v=4", "s=1", "c=1"],
            "{opening:?}: {out:?}"
        );
    }
}

#[test]
fn what_ended_is_not_stopped_by_a_move_of_the_view_and_is_not_started_by_it() {
    // One play takes three seconds, and the end comes a quarter of one more.
    let mut reads: Vec<&[u8]> = vec![b"\t", b"p"];
    reads.extend(idle(34));
    reads.extend([b"G".as_slice(), b"", b"g", b"", b"q"]);
    let (out, _) = played_in(10, animated(12, Plays::Times(1)), &reads);
    assert_eq!(told(&out), ["c=1,s=3,v=2"], "{out:?}");
}

#[test]
fn an_ended_animation_plays_again_on_a_click() {
    let click: &[u8] = b"\x1b[<0;3;1M\x1b[<0;3;1m";
    let mut reads: Vec<&[u8]> = vec![b"\t", b"p"];
    reads.extend(idle(34));
    reads.extend([click, b"q"]);
    let (out, _) = played(animated(0, Plays::Times(1)), &reads);
    assert_eq!(told(&out), ["c=1,s=3,v=2", "c=1,s=3,v=2"], "{out:?}");
}

#[test]
fn reloading_puts_what_ended_back_on_its_first_frame_and_nothing_plays() {
    let (document, mut inputs) = animated(0, Plays::Times(1));
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(34));
    reads.extend([b"r".as_slice(), b"", b"", b"q"]);
    let mut terminal = Fake::new(60, 6, &reads);
    let mut then = Then::new([Reloaded::Document(animated(0, Plays::Times(1)).0)]);
    run_document(
        &document,
        &mut inputs,
        Options {
            files: Some(&mut then),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    assert_eq!(told(&out), ["c=1,s=3,v=2", "s=1", "c=1"], "{out:?}");
    // And what is shown is the still image again, as before it played.
    let ids = shown_ids(&terminal);
    assert_eq!(ids.last(), ids.first(), "{ids:?}");
}

/// The pixels of each frame sent after the first, as RGBA bytes, from the
/// `a=f` sequences in `out` that fit one chunk.
fn frame_pixels(out: &str) -> Vec<Vec<u8>> {
    use base64::Engine;
    use std::io::Read;
    out.split("\x1b_Ga=f,")
        .skip(1)
        .map(|rest| {
            let data = rest
                .split_once(';')
                .unwrap()
                .1
                .split_once("\x1b\\")
                .unwrap()
                .0;
            let packed = base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap();
            let mut pixels = Vec::new();
            flate2::read::ZlibDecoder::new(&packed[..])
                .read_to_end(&mut pixels)
                .unwrap();
            pixels
        })
        .collect()
}

/// Whether some pixel of `pixels` is `rgba`.
fn has(pixels: &[u8], rgba: [u8; 4]) -> bool {
    pixels.chunks(4).any(|pixel| pixel == rgba)
}

/// An animation in a paragraph of reading mode.
fn in_reading_mode(reads: &[&[u8]]) -> (String, Fake) {
    let (document, mut inputs) = animated(0, Plays::Forever);
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    played((document, inputs), reads)
}

#[test]
fn in_reading_mode_the_part_that_holds_an_animation_plays_it() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(12));
    reads.push(b"q");
    let (out, _) = in_reading_mode(&reads);
    // The part of the page as it was, and then the part with the frames of
    // the picture in it, sent when the picture started.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 2, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    assert_eq!(told(&out), ["c=1,s=3,v=4"], "{out:?}");
    // The frames are the part drawn again with the picture's green and
    // blue frames in it.
    let frames = frame_pixels(&out);
    assert!(has(&frames[0], [0, 255, 0, 255]) && !has(&frames[0], [0, 0, 255, 255]));
    assert!(has(&frames[1], [0, 0, 255, 255]) && !has(&frames[1], [0, 255, 0, 255]));
}

#[test]
fn in_reading_mode_the_animation_stops_when_the_view_scrolls() {
    let reading = |lines| {
        let (document, mut inputs) = animated(lines, Plays::Forever);
        inputs.caps.cell_pixels = Some((10, 20));
        inputs.reading = Some(super::reading::reading());
        (document, inputs)
    };
    let mut reads: Vec<&[u8]> = vec![b"", b"\t", b"p"];
    reads.extend(idle(2));
    reads.push(b"G");
    reads.extend(idle(3));
    reads.push(b"g");
    reads.extend(idle(3));
    reads.push(b"q");
    // Lines below it fill the window once scrolled to the end, with the
    // text of the same part of the page on the screen and not the picture.
    let (out, _) = played(reading(12), &reads);
    assert_eq!(told(&out), ["c=1,s=3,v=4", "s=1", "c=1"], "{out:?}");
}

#[test]
fn in_reading_mode_p_tab_and_enter_work_on_the_picture() {
    let mut reads: Vec<&[u8]> = vec![b"", b"\t"];
    reads.extend([b"p".as_slice(), b"", b"p", b"", b"\r"]);
    reads.push(b"q");
    let (document, mut inputs) = animated(0, Plays::Forever);
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    let (out, terminal) = played((document, inputs), &reads);
    let all = told(&out);
    assert_eq!(all, ["c=1,s=3,v=4", "s=1", "c=1", "c=1,s=3,v=4"], "{out:?}");
    // Tab selects it, named by its file, and p and Enter act on it.
    assert!(
        statuses(&terminal)
            .iter()
            .any(|bar| bar.contains("an animation"))
    );
    // The selected part is sent again with the picture framed in yellow.
    assert!(out.matches("\x1b_Ga=T,").count() >= 2, "{out:?}");
}

#[test]
fn in_reading_mode_a_click_starts_the_picture_and_not_the_text_of_its_part() {
    let source = "![an animation](anim.gif)\n\nText below the picture.\n";
    let document = crate::typeset::parse::parse(source);
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs
        .images
        .insert("anim.gif", Ok(clip(Duration::from_secs(1), Plays::Forever)));
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    let picture: &[u8] = b"\x1b[<0;3;1M\x1b[<0;3;1m";
    let text: &[u8] = b"\x1b[<0;3;3M\x1b[<0;3;3m";
    let reads: [&[u8]; 4] = [b"", text, picture, b"q"];
    let (out, _) = played_in(10, (document, inputs), &reads);
    assert_eq!(told(&out), ["c=1,s=3,v=4"], "{out:?}");
}

#[test]
fn in_ghostty_reading_mode_redraws_the_part_with_each_frame() {
    let (document, mut inputs) = ghostty(animated(0, Plays::Forever));
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(60));
    reads.push(b"q");
    let (out, terminal) = played((document, inputs), &reads);
    // The part, and the picture's three frames, each an inset over the
    // part: the first too, though the part stopped shows the same pixels.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 4, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 0, "{out:?}");
    let mut changes = shown_ids(&terminal);
    changes.dedup();
    assert!(changes.len() >= 5, "{changes:?}");
}

/// A linked animation of 20 x 20 pixels in a quote of reading mode, which
/// starts 16 pixels in: in the second cell of the part and not at its edge.
fn quoted_link() -> (Document, Inputs) {
    let (_, mut inputs) = animated(0, Plays::Forever);
    let source = "> [![an animation](anim.gif)](https://example.org/)\n";
    let document = crate::typeset::parse::parse(source);
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    (document, inputs)
}

/// The runs of the image cells of a row: each image's id, the column of
/// the image its first cell shows, and how many cells it has, from the left.
fn runs(cells: &[Option<super::screen::Cell>]) -> Vec<(u32, u16, u16)> {
    let mut runs: Vec<(u32, u16, u16)> = Vec::new();
    for cell in cells.iter().flatten() {
        match runs.last_mut() {
            Some((id, first, count)) if *id == cell.id && *first + *count == cell.column => {
                *count += 1;
            }
            _ => runs.push((cell.id, cell.column, 1)),
        }
    }
    runs
}

/// Tab and p start the picture; then a few polls.
fn started() -> Vec<&'static [u8]> {
    let mut reads: Vec<&[u8]> = vec![b"", b"\t", b"p"];
    reads.extend(idle(4));
    reads.push(b"q");
    reads
}

#[test]
fn in_reading_mode_a_row_of_a_picture_that_plays_is_its_part_its_inset_and_its_part() {
    let (_, terminal) = played_in(10, quoted_link(), &started());
    let shown = Shown::of(&terminal, (10, 20));
    let last = *shown.with_images().last().unwrap();
    let ids = shown.ids(last);
    assert_eq!(ids.len(), 2, "the part and the picture's inset: {ids:?}");
    let (part, inset) = (ids[0], ids[1]);
    let mut inset_rows = Vec::new();
    for row in 0..10 {
        let cells = shown.row(last, row);
        let runs = runs(cells);
        if runs.iter().all(|(id, _, _)| *id == part) {
            assert!(runs.len() <= 1, "row {row}: {runs:?}");
            continue;
        }
        // The part up to the picture, the inset from its first column, and
        // the part again where the inset ends.
        let ids: Vec<u32> = runs.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(ids, [part, inset, part], "row {row}: {runs:?}");
        assert_eq!((runs[0].1, runs[1].1), (0, 0), "row {row}: {runs:?}");
        assert_eq!(runs[2].1, runs[0].2 + runs[1].2, "row {row}: {runs:?}");
        // The picture is 16 to 36 pixels across: the cells of 10 to 40.
        assert_eq!((runs[0].2, runs[1].2), (1, 3), "row {row}: {runs:?}");
        let first = cells
            .iter()
            .flatten()
            .find(|cell| cell.id == inset)
            .unwrap();
        inset_rows.push(first.row);
    }
    assert!(!inset_rows.is_empty(), "no row has the inset");
    let rows: Vec<u16> = (0..inset_rows.len() as u16).collect();
    assert_eq!(inset_rows, rows, "the inset's rows from its first");
}

#[test]
fn in_reading_mode_a_picture_that_plays_at_the_edge_of_its_part_has_its_inset_and_the_part() {
    // The picture of `in_reading_mode` is at the left edge of the part.
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(4));
    reads.push(b"q");
    let (_, terminal) = in_reading_mode(&reads);
    let shown = Shown::of(&terminal, (10, 20));
    let last = *shown.with_images().last().unwrap();
    let shown_ids = shown.ids(last);
    assert_eq!(
        shown_ids.len(),
        2,
        "the part and the picture's inset: {shown_ids:?}"
    );
    // The inset is first in the row, and the part goes on after it.
    let runs = runs(shown.row(last, 0));
    let ids: Vec<u32> = runs.iter().map(|(id, _, _)| *id).collect();
    assert_eq!(ids, shown_ids, "the inset and the part after it: {runs:?}");
    assert_eq!((runs[0].1, runs[0].2, runs[1].1), (0, 2, 2), "{runs:?}");
}

#[test]
fn in_reading_mode_the_link_of_a_picture_that_plays_covers_all_of_it_on_the_screen() {
    let (_, terminal) = played_in(10, quoted_link(), &started());
    let shown = Shown::of(&terminal, (10, 20));
    // The cells under the link, by row and column of the screen.
    let linked = |update: usize| {
        let mut cells = Vec::new();
        for row in 0..10 {
            let links = shown.row_links(update, row);
            let images = shown.row(update, row);
            for (column, link) in links.iter().enumerate() {
                if images.get(column).copied().flatten().is_some() {
                    cells.push((row, column, link.clone()));
                }
            }
        }
        cells
    };
    let updates = shown.with_images();
    let (first, last) = (updates[0], *updates.last().unwrap());
    assert_eq!(shown.ids(last).len(), 2, "it plays");
    let url = Some("https://example.org/".to_owned());
    let before = linked(first);
    assert!(before.iter().any(|(_, _, link)| *link == url), "a link");
    assert_eq!(
        linked(last),
        before,
        "the same cells linked as when it stood"
    );
}

#[test]
fn p_acts_on_the_selected_animation_only() {
    // Two animations in view, the first selected: only it starts, under
    // the two ids it has, plain and framed.
    let reads: [&[u8]; 4] = [b"", b"\t", b"p", b"q"];
    let (out, _) = played_in(14, stops(), &reads);
    let starts = told(&out)
        .iter()
        .filter(|c| c.starts_with("c=1,s=3"))
        .count();
    assert_eq!(starts, 1, "{out:?}");
}

#[test]
fn in_ghostty_the_redraws_come_at_the_gaps_when_a_gap_is_not_a_poll() {
    let (document, mut inputs) = ghostty(animated(0, Plays::Forever));
    let slow = clip(Duration::from_millis(130), Plays::Forever);
    inputs.images.insert("anim.gif", Ok(slow));
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(60));
    reads.push(b"q");
    // Without a limit to the plays, so that it goes on for the whole test.
    let mut terminal = Fake::new(60, 6, &reads);
    run_document(
        &document,
        &mut inputs,
        Options {
            animation: Pacing {
                plays: 0,
                ..Pacing::quick()
            },
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    // Each frame is drawn when its 130 ms are over, not at a multiple of a
    // poll.
    // (The status line's pill is drawn four times a second besides: only the
    // draws that show another image count.)
    let mut last = None;
    let draws: Vec<Duration> = terminal
        .frames()
        .iter()
        .scan(0usize, |at, frame| {
            *at += frame.len() + "\x1b[?2026h".len();
            Some((*at, image_id_of(frame)))
        })
        .filter(|(_, id)| {
            let changed = id.is_some() && *id != last;
            last = id.or(last);
            changed
        })
        .filter_map(|(end, _)| {
            let (_, at) = terminal.marks.iter().find(|(len, _)| *len >= end)?;
            Some(*at - terminal.marks[0].1)
        })
        .collect();
    let gaps: Vec<u128> = draws
        .windows(2)
        .map(|w| (w[1] - w[0]).as_millis())
        .collect();
    // Enough draws for the check to mean something, one a gap.
    assert!(gaps.len() >= 8, "{gaps:?} {out:.0}");
    assert!(
        gaps.iter().skip(1).take(7).all(|gap| *gap == 130),
        "{gaps:?}"
    );
}

/// A document of `before` paragraphs and then `jiggy.gif`, 512 × 512 pixels
/// and two frames, in reading mode on cells of 10 × 20 pixels: it is about
/// 26 rows tall, and `before` of 12 puts it across the 30th row, so that two
/// parts of the page show it.
fn jiggy_after(before: usize) -> (Document, Inputs) {
    let bytes = std::fs::read(crate::load::tests::fixtures().join("images/jiggy.gif")).unwrap();
    let fonts = &crate::images::Fonts::none();
    let source =
        crate::typeset::images::decode::decode_named_as(&bytes, None, 10_000_000, fonts, true);
    let mut text = String::new();
    for n in 0..before {
        text.push_str(&format!("Paragraph {n}\n\n"));
    }
    text.push_str("![jiggy](jiggy.gif)\n\nAfter\n");
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    inputs.images.insert("jiggy.gif", source);
    (crate::typeset::parse::parse(&text), inputs)
}

/// The ids that the controls of `out` name, in order, with their keys.
fn controlled(out: &str) -> Vec<(u32, String)> {
    out.split("\x1b_Ga=a,q=2,i=")
        .skip(1)
        .filter_map(|rest| {
            let (id, rest) = rest.split_once(',')?;
            let keys = rest.split_once("\x1b\\")?.0;
            (!keys.starts_with("r=1,")).then(|| (id.parse().unwrap(), keys.to_owned()))
        })
        .collect()
}

#[test]
fn in_reading_mode_a_picture_across_two_parts_plays_in_both_halves_together() {
    // A window tall enough for both halves, and a click on the picture.
    let click: &[u8] = b"\x1b[<0;5;30M\x1b[<0;5;30m";
    let reads: [&[u8]; 4] = [b"", click, b"", b"q"];
    let (out, _) = played_in(60, jiggy_after(12), &reads);
    let starts: Vec<_> = (controlled(&out).into_iter())
        .filter(|(_, keys)| keys.starts_with("c=1,s=3"))
        .collect();
    let ids: std::collections::BTreeSet<u32> = starts.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids.len(), 2, "two parts show it: {starts:?}");
}

#[test]
fn in_reading_mode_a_click_on_either_half_of_a_picture_starts_all_of_it() {
    // `d` three times shows the upper half only; the click is on its last
    // row, which is the picture's.
    let mut reads: Vec<&[u8]> = vec![b"d", b"d", b"d", b"d"];
    reads.extend(idle(12));
    reads.push(b"\x1b[<0;5;6M\x1b[<0;5;6m");
    reads.extend(idle(2));
    reads.push(b"q");
    let (out, _) = played_in(12, jiggy_after(12), &reads);
    let replays: Vec<u32> = (controlled(&out).into_iter())
        .filter(|(_, keys)| keys.starts_with("c=1,s=3"))
        .map(|(id, _)| id)
        .collect();
    let ids: std::collections::BTreeSet<_> = replays.iter().collect();
    assert_eq!(ids.len(), 2, "{replays:?}");
}

#[test]
fn in_reading_mode_a_picture_across_two_parts_has_one_tab_stop_and_stops_as_one() {
    let reads: [&[u8]; 7] = [b"", b"\t", b"\t", b"p", b"", b"p", b"q"];
    let (out, terminal) = played_in(60, jiggy_after(12), &reads);
    // Two Tabs stay on the one stop, and `p` stops the picture in both parts,
    // each under its plain and its framed id at most.
    let named = statuses(&terminal)
        .iter()
        .filter(|bar| bar.contains("jiggy"))
        .count();
    assert!(named >= 1);
    let stops = controlled(&out)
        .into_iter()
        .filter(|(_, keys)| keys == "s=1");
    let ids: std::collections::BTreeSet<_> = stops.map(|(id, _)| id).collect();
    assert!(ids.len() >= 2, "both parts are stopped: {ids:?}");
}

#[test]
fn in_reading_mode_the_two_halves_of_a_picture_show_the_same_frame_in_ghostty() {
    let (document, inputs) = ghostty(jiggy_after(12));
    let click: &[u8] = b"\x1b[<0;5;30M\x1b[<0;5;30m";
    let mut reads: Vec<&[u8]> = vec![b"", click];
    reads.extend(idle(60));
    reads.push(b"q");
    let (_, terminal) = played_in(60, (document, inputs), &reads);
    // Both parts are on the screen; while it plays every screen has an
    // inset in each, and the pairs of insets are those of the two frames of
    // the picture, never mixed. An inset is narrower than the part.
    let shown = Shown::of(&terminal, (10, 20));
    let pairs: std::collections::BTreeSet<Vec<u32>> = (shown.with_images().into_iter())
        .filter_map(|update| {
            let ids = shown.ids(update);
            let width = |id: &u32| shown.image(update, *id).map_or(0, |image| image.width);
            let widest = ids.iter().map(width).max()?;
            let insets: Vec<u32> = ids.into_iter().filter(|id| width(id) < widest).collect();
            (insets.len() == 2).then_some(insets)
        })
        .collect();
    assert_eq!(pairs.len(), 2, "{pairs:?}");
    let all: Vec<u32> = pairs.iter().flatten().copied().collect();
    let distinct: std::collections::BTreeSet<_> = all.iter().collect();
    assert_eq!(
        distinct.len(),
        all.len(),
        "an inset in one pair only: {pairs:?}"
    );
}

#[test]
fn in_the_text_layout_a_picture_half_in_view_is_one_image_a_click_starts_whole() {
    // 512 × 512 pixels in cells of the default size: taller than the window.
    let bytes = std::fs::read(crate::load::tests::fixtures().join("images/jiggy.gif")).unwrap();
    let fonts = &crate::images::Fonts::none();
    let source =
        crate::typeset::images::decode::decode_named_as(&bytes, None, 10_000_000, fonts, true);
    let document = crate::typeset::parse::parse("![jiggy](jiggy.gif)\n\nAfter\n");
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.images.insert("jiggy.gif", source);
    let reads: [&[u8]; 6] = [b"d", b"", CLICK, b"", b"d", b"q"];
    let (out, _) = played_in(10, (document, inputs), &reads);
    // One image: sent as a still image, again with its frames when the click
    // starts it, and stopped by the scroll after it.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 2, "{out:?}");
    assert_eq!(told(&out), ["c=1,s=3,v=4", "s=1", "c=1"], "{out:?}");
}

/// An animation of `frames` frames of `side` × `side` pixels, red, that
/// plays once.
fn square_animation(side: u32, frames: usize) -> Source {
    let frame = Frame {
        image: image::RgbaImage::from_pixel(side, side, image::Rgba([255, 0, 0, 255])),
        gap: Duration::from_millis(100),
    };
    Source::Animated(std::sync::Arc::new(Animation {
        frames: vec![frame; frames],
        plays: Plays::Forever,
        over_budget: false,
    }))
}

#[test]
fn in_reading_mode_a_small_animation_plays_in_a_part_whose_frames_would_not_fit() {
    // 100 × 100 pixels is 160 × 160 on cells of 16 × 32. The part of a page
    // in 200 columns and 30 rows of paragraphs is some 1600 × 960 pixels, and
    // 20 frames of it are over the budget; the picture's own cells are 2 MB.
    let text: String = (0..40).map(|n| format!("Paragraph {n}\n\n")).collect();
    let document = crate::typeset::parse::parse(&format!("![ball](ball.gif)\n\n{text}"));
    let mut inputs = Inputs::new(Capabilities::kitty(200));
    inputs.caps.cell_pixels = Some((16, 32));
    inputs.reading = Some(super::reading::reading());
    inputs
        .images
        .insert("ball.gif", Ok(square_animation(100, 20)));
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(40));
    reads.push(b"q");
    let (_, terminal) = played_wide(200, 50, (document, inputs), &reads);
    let shown = Shown::of(&terminal, (16, 32));
    // The part is over the budget with 20 frames: that was the warning.
    let first = shown.with_images()[0];
    let part = shown.ids(first)[0];
    let image = shown.image(first, part).unwrap();
    let part_bytes = u64::from(image.width) * u64::from(image.height) * 4;
    assert!(part_bytes * 20 > crate::typeset::images::animation::FRAME_BUDGET);
    // It plays all the same: 20 frames on the screen after the click.
    let last = *shown.with_images().last().unwrap();
    assert_eq!(shown.frames(last), 20, "the frames of the picture");
    assert!(
        terminal.frames().iter().all(|frame| !frame.contains('⚠')),
        "no warning"
    );
}

#[test]
fn in_reading_mode_a_picture_over_the_budget_on_the_edge_of_two_parts_sends_no_frames_and_says_so_once()
 {
    // 300 × 180 pixels is 960 × 576 on cells of 32 × 64, and 31 frames of the
    // cells it lies on take 68.6 MB, over the budget of 64 MiB. The picture
    // is over the edge between the first part and the second. With 2 frames
    // it fits, and the same reads play it.
    let text: String = (0..14).map(|n| format!("Paragraph {n}\n\n")).collect();
    let document = crate::typeset::parse::parse(&format!("{text}![wide](wide.gif)\n"));
    let run = |frames: usize| {
        let frame = Frame {
            image: image::RgbaImage::from_pixel(300, 180, image::Rgba([255, 0, 0, 255])),
            gap: Duration::from_millis(100),
        };
        let animation = Animation {
            frames: vec![frame; frames],
            plays: Plays::Forever,
            over_budget: false,
        };
        let mut inputs = Inputs::new(Capabilities::kitty(60));
        inputs.caps.cell_pixels = Some((32, 64));
        inputs.reading = Some(super::reading::reading());
        inputs.images.insert(
            "wide.gif",
            Ok(Source::Animated(std::sync::Arc::new(animation))),
        );
        // Two parts show it.
        let lines = crate::typeset::layout::layout(&document, &inputs);
        let keys: std::collections::HashSet<_> = (lines.iter())
            .filter_map(|line| line.image.as_ref().map(|row| row.placement.key.clone()))
            .collect();
        assert_eq!(keys.len(), 2, "the picture is over the edge of two parts");
        // Tab selects the picture and p starts it, if it can play.
        let mut reads: Vec<&[u8]> = vec![b"", b"\t", b"p"];
        reads.extend(idle(14));
        reads.push(b"q");
        let mut terminal = Fake::new(60, 40, &reads);
        run_document(
            &document,
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        );
        let out = String::from_utf8(terminal.out.clone()).unwrap();
        (out, inputs.images.notes().len())
    };
    // Under the budget the start sends the frames, in both parts.
    let (out, notes) = run(2);
    assert!(out.matches("\x1b_Ga=f,").count() >= 2, "frames are sent");
    assert!(!told(&out).is_empty(), "{out:?}");
    assert_eq!(notes, 0);
    // Over it nothing plays, and the one note is said once, for the one
    // picture, and not once for each part.
    let (out, notes) = run(31);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 0, "no frame is sent");
    assert!(told(&out).is_empty(), "{out:?}");
    assert_eq!(notes, 1);
}

#[test]
fn in_reading_mode_the_frames_of_a_part_are_not_sent_until_a_picture_in_it_starts() {
    let mut reads = idle(10);
    reads.push(b"q");
    let (out, _) = in_reading_mode(&reads);
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 1, "{out:?}");
    assert_eq!(
        out.matches("\x1b_Ga=f,").count(),
        0,
        "no frame yet: {out:?}"
    );
    // A click on the picture starts it: the frames are sent then.
    let reads: [&[u8]; 3] = [b"", CLICK, b"q"];
    let (out, _) = in_reading_mode(&reads);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
}

/// A page of reading mode with two animations in one part: `a.gif`, whose
/// frames are red, green and blue, and `b.gif`, whose frames are black,
/// yellow and white, one under the other.
fn two_in_a_part() -> (Document, Inputs) {
    let two = |colours: [[u8; 4]; 3]| {
        let frames = colours
            .iter()
            .map(|rgba| Frame {
                image: image::RgbaImage::from_pixel(40, 40, image::Rgba(*rgba)),
                gap: Duration::from_secs(1),
            })
            .collect();
        Source::Animated(std::sync::Arc::new(Animation {
            frames,
            plays: Plays::Forever,
            over_budget: false,
        }))
    };
    let document = crate::typeset::parse::parse("![a](a.gif)\n\n![b](b.gif)\n");
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    let rgb = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
    inputs.images.insert("a.gif", Ok(two(rgb)));
    let other = [[1, 2, 3, 255], [255, 255, 0, 255], [250, 250, 250, 255]];
    inputs.images.insert("b.gif", Ok(two(other)));
    (document, inputs)
}

#[test]
fn in_reading_mode_the_picture_started_plays_in_a_part_it_shares_and_the_other_stays_still() {
    // Tab twice selects the second picture; p starts it.
    let reads: [&[u8]; 6] = [b"", b"\t", b"\t", b"p", b"", b"q"];
    let (out, terminal) = played_in(20, two_in_a_part(), &reads);
    let starts = told(&out);
    assert!(
        starts.iter().any(|c| c.starts_with("c=1,s=3")),
        "{starts:?}"
    );
    // b's frames are laid in b's place, a stays red: its first frame. What
    // the screen shows of b's playing, frame by frame.
    let (red, green, yellow) = ([255, 0, 0, 255], [0, 255, 0, 255], [255, 255, 0, 255]);
    let shown = Shown::of(&terminal, (10, 20));
    let last = *shown.with_images().last().expect("a screen with an image");
    assert_eq!(shown.frames(last), 3, "b's three frames");
    for frame in 0..3 {
        let picture = shown.picture(last, frame).unwrap();
        assert!(
            has(&picture.rgba, red) && !has(&picture.rgba, green),
            "a stays on its first frame, in frame {frame}"
        );
    }
    let second = shown.picture(last, 1).unwrap();
    assert!(has(&second.rgba, yellow), "b's second frame");
    // Nothing says that a picture is held back.
    assert!(
        terminal.frames().iter().all(|frame| !frame.contains('⚠')),
        "no warning"
    );
}

#[test]
fn in_reading_mode_starting_the_other_picture_of_a_part_stops_the_first() {
    let reads: [&[u8]; 8] = [b"", b"\t", b"p", b"", b"\t", b"p", b"", b"q"];
    let (out, _) = played_in(20, two_in_a_part(), &reads);
    let all = controlled(&out);
    assert!(all.iter().any(|(_, keys)| keys == "s=1"), "{all:?}");
    let starts = all
        .iter()
        .filter(|(_, keys)| keys.starts_with("c=1,s=3"))
        .count();
    assert!(starts >= 2, "{all:?}");
}

/// The pixels of every image `out` sends, as RGBA bytes: the sequences that
/// start with `action` (`a=T,` for an image, `a=f,` for a frame), with the
/// chunks of one put together.
fn sent_pixels(out: &str, action: &str) -> Vec<Vec<u8>> {
    use base64::Engine;
    use std::io::Read;
    let mut all = Vec::new();
    let mut data = String::new();
    let mut mine = false;
    for sequence in out.split("\x1b_G").skip(1) {
        let (keys, rest) = sequence.split_once(';').unwrap_or((sequence, ""));
        let keys = keys.trim_end_matches("\x1b\\");
        if keys.starts_with("a=") {
            mine = keys.starts_with(action);
            data.clear();
        }
        if !mine || !(keys.starts_with(action) || keys.starts_with("q=2,m=")) {
            continue;
        }
        data.push_str(rest.split_once("\x1b\\").map_or(rest, |(data, _)| data));
        if keys.ends_with("m=0") {
            let packed = base64::engine::general_purpose::STANDARD
                .decode(&data)
                .unwrap();
            let mut pixels = Vec::new();
            flate2::read::ZlibDecoder::new(&packed[..])
                .read_to_end(&mut pixels)
                .unwrap();
            all.push(pixels);
            data.clear();
            mine = false;
        }
    }
    all
}

/// Whether some pixel of `pixels` is nearly white: a button's triangle or
/// an icon's arc, which no frame of the clips here has.
fn has_white(pixels: &[u8]) -> bool {
    pixels
        .chunks(4)
        .any(|p| p[0] > 235 && p[1] > 235 && p[2] > 235 && p[3] > 200)
}

/// A clip of 120 × 120 pixels, large enough for the marks.
fn big_clip(plays: Plays) -> Source {
    let frames = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
        .into_iter()
        .map(|rgba| Frame {
            image: image::RgbaImage::from_pixel(120, 120, image::Rgba(rgba)),
            gap: Duration::from_secs(1),
        })
        .collect();
    Source::Animated(std::sync::Arc::new(Animation {
        frames,
        plays,
        over_budget: false,
    }))
}

/// A document of the clip `anim.gif` of 120 × 120 pixels.
fn big_animated(plays: Plays) -> (Document, Inputs) {
    let (document, mut inputs) = animated(0, plays);
    inputs.images.insert("anim.gif", Ok(big_clip(plays)));
    (document, inputs)
}

#[test]
fn a_stopped_animation_has_a_play_button_drawn_into_its_first_frame() {
    let (out, _) = played_in(20, big_animated(Plays::Forever), &[b"", b"", b"q"]);
    let images = sent_pixels(&out, "a=T,");
    assert_eq!(images.len(), 1, "{out:?}");
    assert!(has_white(&images[0]), "the play button's triangle");
    assert!(has(&images[0], [255, 0, 0, 255]), "over the first frame");
    // No frame is sent yet, and so none can have a button.
    assert!(sent_pixels(&out, "a=f,").is_empty());
}

#[test]
fn a_small_animation_has_no_button() {
    let (out, _) = played_in(20, animated(0, Plays::Forever), &[b"", b"", b"q"]);
    let images = sent_pixels(&out, "a=T,");
    assert!(!has_white(&images[0]));
}

#[test]
fn what_plays_has_no_mark_in_any_of_its_frames() {
    let reads: [&[u8]; 5] = [b"", CLICK, b"", b"", b"q"];
    let (out, _) = played_in(20, big_animated(Plays::Forever), &reads);
    // The stopped image, and the one sent with the frames when it started.
    let images = sent_pixels(&out, "a=T,");
    assert_eq!(images.len(), 2, "{out:?}");
    assert!(has_white(&images[0]) && !has_white(&images[1]));
    for frame in sent_pixels(&out, "a=f,") {
        assert!(!has_white(&frame));
    }
}

#[test]
fn what_has_ended_shows_its_last_frame_with_a_replay_icon() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(36));
    reads.push(b"q");
    let (out, _) = played_in(20, big_animated(Plays::Times(1)), &reads);
    let images = sent_pixels(&out, "a=T,");
    assert_eq!(images.len(), 3, "stopped, playing, ended: {out:?}");
    let ended = &images[2];
    assert!(has_white(ended), "the replay icon");
    assert!(has(ended, [0, 0, 255, 255]), "over the last frame");
    assert!(!has(ended, [255, 0, 0, 255]) && !has(ended, [0, 255, 0, 255]));
}

#[test]
fn what_was_stopped_is_back_on_its_first_frame_with_its_button() {
    let reads: [&[u8]; 5] = [b"", CLICK, b"", CLICK, b"q"];
    let (out, terminal) = played_in(20, big_animated(Plays::Forever), &reads);
    // The stopped image is shown again by the last frame: its id comes
    // again in the placeholders, and no new image was sent for it.
    assert_eq!(sent_pixels(&out, "a=T,").len(), 2, "{out:?}");
    let ids = shown_ids(&terminal);
    assert_eq!(ids.first(), ids.last(), "{ids:?}");
}

#[test]
fn in_reading_mode_the_stopped_part_has_the_button_and_the_ended_one_the_icon() {
    let (document, mut inputs) = big_animated(Plays::Times(1));
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(36));
    reads.push(b"q");
    let (out, _) = played_in(20, (document, inputs), &reads);
    let images = sent_pixels(&out, "a=T,");
    assert_eq!(images.len(), 3, "stopped, playing, ended: {out:?}");
    assert!(has_white(&images[0]) && !has_white(&images[1]) && has_white(&images[2]));
}

#[test]
fn in_ghostty_the_marks_are_in_the_images_kiri_sends() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(36));
    reads.push(b"q");
    let (out, _) = played_in(20, ghostty(big_animated(Plays::Times(1))), &reads);
    let images = sent_pixels(&out, "a=T,");
    // The stopped image, the three frames, and the ended one.
    assert!(has_white(&images[0]), "button");
    assert!(
        !images[1..images.len() - 1]
            .iter()
            .any(|image| has_white(image))
    );
    assert!(has_white(images.last().unwrap()), "icon");
}

#[test]
fn the_status_line_tells_the_animation_that_plays_its_progress_and_what_it_was_when_it_ended() {
    // Three frames of a second, played once, in kitty, where the terminal
    // plays the frames and kiri only draws the status line.
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(40));
    reads.push(b"q");
    let (_, terminal) = played_wide(100, 6, animated(0, Plays::Times(1)), &reads);
    let bars = statuses(&terminal);
    let with = |text: &str| bars.iter().filter(|bar| bar.contains(text)).count();
    assert_eq!(with("an animation"), with(" s"), "no pill before the start");
    // Four a second over the 3 s play (13 with the first and the last), not
    // ten: the line is written when what the pill prints changes.
    let playing = with("▶ an animation");
    assert!(
        (10..=15).contains(&playing),
        "four times a second: {playing} bars"
    );
    for time in ["0.0 / 3.0 s", "1.0 / 3.0 s", "2.0 / 3.0 s"] {
        assert!(with(time) >= 1, "{time}");
    }
    // Ended: stopped on its last frame, and it stays.
    let last = bars.last().unwrap();
    assert!(
        last.contains("■ an animation") && last.contains("3.0 / 3.0 s"),
        "{last:?}"
    );
    // The first frames have no pill: it appears with the first start.
    assert!(!bars[0].contains("an animation"), "{:?}", bars[0]);
}

#[test]
fn what_plays_goes_on_telling_its_progress_when_another_picture_is_selected() {
    // The first starts, then Tab selects the second, which is told as a
    // selected picture beside the pill of the first.
    let reads: [&[u8]; 6] = [b"", b"\t", b"p", b"", b"\t", b"q"];
    let (_, terminal) = played_in(14, stops(), &reads);
    let bars = statuses(&terminal);
    let bar = bars.last().unwrap();
    assert!(bar.contains("second") && bar.contains("▶ first"), "{bar:?}");
}

/// The pager on `inputs`, its frames built only when the read numbered `release`
/// comes; all that it writes, and the terminal.
fn played_held(inputs: (Document, Inputs), reads: &[&[u8]], release: usize) -> (String, Fake) {
    let (document, mut inputs) = inputs;
    let (mut loads, held) = Loads::held_builds();
    let mut terminal = Fake::new(100, 20, reads);
    let mut seen = 0;
    terminal.hook = Some(Box::new(move |read| {
        if read == release && seen == 0 {
            seen = 1;
            held.release();
        }
    }));
    run_document_in(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut loads,
        &mut terminal,
    );
    (String::from_utf8(terminal.out.clone()).unwrap(), terminal)
}

#[test]
fn after_a_start_the_picture_waits_with_a_turning_mark_until_the_frames_are_built() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(9));
    reads.extend([b"q".as_slice()]);
    let (out, terminal) = played_held(big_animated(Plays::Forever), &reads, 8);
    let images = sent_pixels(&out, "a=T,");
    // The stopped image with its button, then the waiting images, each
    // with a white arc and another step of it, and last the frames' image
    // with no mark at all.
    assert!(images.len() >= 4, "{} images", images.len());
    assert!(has_white(&images[0]));
    let waiting = &images[1..images.len() - 1];
    assert!(waiting.iter().all(|image| has_white(image)));
    assert!(
        waiting.windows(2).all(|pair| pair[0] != pair[1]),
        "a step at a time"
    );
    assert!(!has_white(images.last().unwrap()));
    // No frame was sent, and nothing told to play, before the build came.
    let frames_at = out.find("\x1b_Ga=f,").expect("frames are sent in the end");
    let waiting_ids = shown_ids(&terminal);
    assert!(waiting_ids.len() >= 4, "{waiting_ids:?}");
    assert!(
        !out[..frames_at].contains(",s=3"),
        "no start before the frames"
    );
    assert_eq!(told(&out), ["c=1,s=3,v=4"], "{out:?}");
}

#[test]
fn behind_tmux_no_step_of_the_turning_mark_deletes_an_image_in_the_update_that_shows_it() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(9));
    reads.extend([b"q".as_slice()]);
    let (document, mut inputs) = big_animated(Plays::Forever);
    inputs.caps.tmux_passthrough = true;
    let (out, terminal) = played_held((document, inputs), &reads, 8);
    let sending: Vec<String> = (terminal.updates().into_iter())
        .filter(|update| update.contains("\x1b_Ga=T,"))
        .collect();
    assert!(sending.len() >= 4, "{} updates", sending.len());
    for update in &sending {
        assert!(deleted_ids(update).is_empty(), "{update:?}");
    }
    // None is left in kitty once the pager has quit.
    assert_eq!(sorted(deleted_ids(&out)), sorted(sent_ids(&out)), "{out:?}");
}

#[test]
fn the_play_counts_from_when_the_frames_are_ready() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(40));
    reads.push(b"q");
    // Ready at once, and ready 3 s after the start: the pill is at 0:00 at
    // the first draw after each.
    for release in [3, 33] {
        let (_, terminal) = played_held(big_animated(Plays::Times(1)), &reads, release);
        let bars = statuses(&terminal);
        let first = bars
            .iter()
            .position(|bar| bar.contains("▶"))
            .expect("it plays");
        assert!(bars[first].contains("0.0 / 3.0 s"), "{:?}", bars[first]);
    }
}

#[test]
fn a_move_of_the_view_a_second_click_and_a_reload_stop_the_wait_and_the_frames_never_play() {
    let scroll: [&[u8]; 3] = [b"", CLICK, b"G"];
    let click: [&[u8]; 4] = [b"", CLICK, b"", CLICK];
    for reads in [scroll.as_slice(), click.as_slice()] {
        let mut reads = reads.to_vec();
        reads.extend(idle(5));
        reads.push(b"q");
        let (out, _) = played_held(big_animated_in_page(), &reads, 6);
        assert!(!out.contains(",s=3"), "{reads:?}: {out:?}");
        assert!(!has_frames(&out), "built frames were thrown away: {out:?}");
    }
}

/// Whether `out` sends the frames of an animation after the first.
fn has_frames(out: &str) -> bool {
    out.contains("\x1b_Ga=f,")
}

/// A document of lines below `anim.gif` of 120 × 120 pixels.
fn big_animated_in_page() -> (Document, Inputs) {
    let (document, mut inputs) = animated(30, Plays::Forever);
    inputs
        .images
        .insert("anim.gif", Ok(big_clip(Plays::Forever)));
    (document, inputs)
}

#[test]
fn in_ghostty_the_waiting_mark_shows_too_and_the_frames_come_when_built() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(9));
    reads.extend(idle(20));
    reads.push(b"q");
    let (out, terminal) = played_held(ghostty(big_animated(Plays::Forever)), &reads, 8);
    let images = sent_pixels(&out, "a=T,");
    let waiting = images.iter().filter(|image| has_white(image)).count();
    assert!(waiting >= 3, "the button and the waiting steps: {waiting}");
    assert!(shown_ids(&terminal).len() > waiting, "then the frames");
    assert!(!has_frames(&out), "Ghostty is sent no frames, but images");
}

/// A document of `anim.gif` ("first"), twelve lines, and `other.gif`
/// ("second") at the end.
fn far_apart() -> (Document, Inputs) {
    let mut source = String::from("![first](anim.gif)\n\n");
    for line in 0..12 {
        source.push_str(&format!("line {line}\n\n"));
    }
    source.push_str("![second](other.gif)\n");
    let document = crate::typeset::parse::parse(&source);
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    for (name, secs) in [("anim.gif", 1), ("other.gif", 2)] {
        let clip = clip(Duration::from_secs(secs), Plays::Forever);
        inputs.images.insert(name, Ok(clip));
    }
    (document, inputs)
}

#[test]
fn tab_goes_on_to_the_next_animation_in_the_document_and_scrolls_to_it_and_wraps() {
    let reads: [&[u8]; 5] = [b"\t", b"\t", b"\t", b"\x1b[Z", b"q"];
    let (_, terminal) = played_in(6, far_apart(), &reads);
    let screens = terminal.screens();
    let bars = statuses(&terminal);
    let after = |text: &str, from: usize| (from..bars.len()).find(|at| bars[*at].contains(text));
    // The first is on the screen: selected as it stands. The second is far
    // down: the window moves to it. The third Tab comes round to the first.
    // Shift+Tab goes back to the second.
    let first = after("first", 0).expect("the first");
    let second = after("second", first).expect("the second");
    let round = after("first", second).expect("round to the first");
    let back = after("second", round).expect("back to the second");
    assert!(
        unstyled(&screen_text(&screens[first])).contains("line 0"),
        "the first is in view"
    );
    assert!(
        !unstyled(&screen_text(&screens[second])).contains("line 0"),
        "the window moved"
    );
    assert!(
        unstyled(&screen_text(&screens[round])).contains("line 0"),
        "and back to the top"
    );
    assert!(back > round);
}
#[test]
fn esc_clears_the_selection_and_a_second_esc_takes_the_pill_off_the_status_line() {
    let reads: [&[u8]; 8] = [b"", b"\t", b"p", b"\t", b"\x1b", b"\x1b", b"\x1b", b"q"];
    let (_, terminal) = played_in(14, stops(), &reads);
    let bars = statuses(&terminal);
    // Selected the second after starting the first: both told.
    let both = bars
        .iter()
        .rposition(|bar| bar.contains("second") && bar.contains("▶ first"))
        .expect("both told");
    // The first Esc: the selection goes, the pill stays.
    let cleared = (both..bars.len())
        .find(|at| !bars[*at].contains("second"))
        .expect("the selection goes");
    assert!(bars[cleared].contains("first"), "{:?}", bars[cleared]);
    // The hints are not on a line with the pill, and are back when it goes.
    assert!(!bars[cleared].contains("quit"), "{:?}", bars[cleared]);
    // The second: the pill goes too, and stays away.
    let off = (cleared..bars.len())
        .find(|at| !bars[*at].contains("first"))
        .expect("the pill goes");
    assert!(bars[off].contains("quit"), "{:?}", bars[off]);
    assert!(
        bars[off..]
            .iter()
            .all(|bar| !bar.contains("first") && !bar.contains("second"))
    );
    // It comes back with the next start.
    let reads: [&[u8]; 6] = [b"\t", b"p", b"\x1b", b"\x1b", b"p", b"q"];
    let (_, terminal) = played_in(14, stops(), &reads);
    let bars = statuses(&terminal);
    assert!(bars.last().unwrap().contains("first"), "{:?}", bars.last());
}
#[test]
fn the_selected_picture_is_told_in_the_yellow_of_its_frame_in_the_status_line() {
    let reads: [&[u8]; 3] = [b"\t", b"\t", b"q"];
    let (_, terminal) = played_in(14, stops(), &reads);
    let frames = terminal.frames();
    let bar = frames[2]
        .rsplit_once("\x1b[0;1m kiri")
        .map_or("", |(_, bar)| bar);
    // Palette colour 3, yellow, set before the name and not before the file's.
    let before = bar.split("second").next().unwrap();
    assert!(
        before.rsplit("\x1b[").next().unwrap().starts_with("0;33m") || before.contains("33m"),
        "{bar:?}"
    );
}

/// Whether `pixel` is the yellow of a selection frame, of no frame of the
/// clips here.
fn is_frame(pixel: &[u8]) -> bool {
    pixel[0] > 150 && pixel[1] > 150 && pixel[2] < 80 && pixel[3] == 255
}

/// For the image `pixels` of `width` pixels across: the rows that have a
/// run of at least `run` frame pixels in a line, as their numbers.
fn frame_rows(pixels: &[u8], width: usize, run: usize) -> Vec<usize> {
    let rows = pixels.chunks(width * 4);
    let long = |row: &[u8]| {
        let (mut best, mut now) = (0, 0);
        for pixel in row.chunks(4) {
            now = if is_frame(pixel) { now + 1 } else { 0 };
            best = best.max(now);
        }
        best >= run
    };
    rows.enumerate()
        .filter(|(_, row)| long(row))
        .map(|(n, _)| n)
        .collect()
}

/// The first row with a frame pixel in the column `x`, and the last.
fn frame_span(pixels: &[u8], width: usize, x: usize) -> Option<(usize, usize)> {
    let rows: Vec<usize> = pixels
        .chunks(width * 4)
        .enumerate()
        .filter(|(_, row)| is_frame(&row[x * 4..x * 4 + 4]))
        .map(|(n, _)| n)
        .collect();
    Some((*rows.first()?, *rows.last()?))
}

#[test]
fn the_frame_of_a_selected_picture_goes_round_that_picture_and_not_the_one_before_it() {
    // Tab twice: the second of the two pictures in the part is selected.
    let reads: [&[u8]; 4] = [b"", b"\t", b"\t", b"q"];
    let (out, _) = played_in(20, two_in_a_part(), &reads);
    let images = sent_pixels(&out, "a=T,");
    let last = images.last().expect("an image");
    // A part 56 cells wide of 10 pixels; the pictures are 40 pixels square,
    // one under the other, so the frame of the second starts below the first.
    let first = frame_span(&images[0], 560, 0);
    assert_eq!(first, None, "no frame before the selection");
    let (top, bottom) = frame_span(last, 560, 0).expect("a frame at the left edge");
    assert!(
        top >= 40,
        "the frame is round the first picture: {top}..{bottom}"
    );
    assert!(
        bottom - top >= 39 && bottom - top < 60,
        "one picture's height: {top}..{bottom}"
    );
}

#[test]
fn the_frame_goes_round_the_second_picture_whatever_state_it_is_in() {
    // Started, and ended after its three plays: the last image is the ended
    // part, with the frame still round the second.
    let mut reads: Vec<&[u8]> = vec![b"", b"\t", b"\t", b"p"];
    // Three plays of three seconds, and a quarter of one more.
    reads.extend(idle(100));
    reads.push(b"q");
    let (_, terminal) = played(two_in_a_part(), &reads);
    let shown = Shown::of(&terminal, (10, 20));
    // The screen as it was; with the first selected; with the second; then
    // waiting, playing and ended.
    let updates = shown.with_images();
    assert!(updates.len() >= 5, "{} screens", updates.len());
    // The part is 56 cells wide of 10 pixels, and the first picture is its
    // first 40 rows: from the screen where the frame is round the second
    // on, it is never round the first, in any frame that plays.
    let on_second = |update: usize, frame: usize| {
        let picture = shown.picture(update, frame).unwrap();
        assert_eq!(picture.width, 560);
        frame_span(&picture.rgba, 560, 0).filter(|(top, _)| *top >= 40)
    };
    let from = updates
        .iter()
        .position(|update| on_second(*update, 0).is_some())
        .expect("a screen with the frame round the second");
    let mut playing = false;
    for &update in &updates[from..] {
        playing |= shown.frames(update) == 3;
        for frame in 0..shown.frames(update).max(1) {
            let (top, bottom) = on_second(update, frame).unwrap_or_else(|| {
                panic!("screen {update}, frame {frame}: the frame is round the first")
            });
            assert!(
                bottom - top < 60,
                "screen {update}, frame {frame}: one picture's: {top}..{bottom}"
            );
        }
    }
    assert!(
        playing,
        "the frames of the playing picture were on the screen"
    );
}

#[test]
fn the_frame_of_a_picture_across_two_parts_is_whole_and_not_round_a_picture_before_it() {
    // A small picture first in the page, and then jiggy, 512 pixels square,
    // which has its first rows in the first part and the rest in the second.
    let (document, mut inputs) = jiggy_after(12);
    let source = "![small](anim.gif)\n\n";
    let text = format!(
        "{source}{}",
        (0..12)
            .map(|n| format!("Paragraph {n}\n\n"))
            .collect::<String>()
    );
    let document_text = format!("{text}![jiggy](jiggy.gif)\n\nAfter\n");
    let _ = document;
    let document = crate::typeset::parse::parse(&document_text);
    inputs
        .images
        .insert("anim.gif", Ok(clip(Duration::from_secs(1), Plays::Forever)));
    // Tab selects the small picture, and again, jiggy; G shows the rest of it.
    let reads: [&[u8]; 6] = [b"", b"\t", b"\t", b"", b"G", b"q"];
    let (out, _) = played_in(30, (document, inputs), &reads);
    let images = sent_pixels(&out, "a=T,");
    // The last images: jiggy selected, in the upper part and in the lower.
    let (upper, lower) = (&images[images.len() - 2], &images[images.len() - 1]);
    let upper_rows = upper.len() / 4 / 560;
    let (top, _) = frame_span(upper, 560, 0).expect("a frame in the upper part");
    assert!(
        top > 100,
        "the frame is round jiggy and not the small picture: from {top}"
    );
    assert_eq!(frame_rows(upper, 560, 400).len(), 1, "jiggy's top edge");
    assert!(upper_rows > 0);
    let (_, bottom) = frame_span(lower, 560, 0).expect("a frame in the lower part");
    assert_eq!(frame_rows(lower, 560, 400).len(), 1, "jiggy's bottom edge");
    assert!(bottom < lower.len() / 4 / 560, "{bottom}");
}

#[test]
fn frames_that_are_ready_at_once_still_show_the_waiting_mark_turning_for_its_minimum_time() {
    let (document, mut inputs) = big_animated(Plays::Forever);
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(12));
    reads.push(b"q");
    let mut terminal = Fake::new(60, 20, &reads);
    // The build is done where it starts, so the frames are ready at once.
    run_document(
        &document,
        &mut inputs,
        Options {
            animation: Pacing::new(3),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    let images = sent_pixels(&out, "a=T,");
    // The stopped image, the waiting mark in at least four of its positions
    // (half a second, eight a second), and then the frames' image.
    let waiting = &images[1..images.len() - 1];
    assert!(waiting.len() >= 4, "{} waiting images", waiting.len());
    assert!(waiting.iter().all(|image| has_white(image)));
    assert!(!has_white(images.last().unwrap()));
    let distinct: std::collections::HashSet<_> = waiting.iter().collect();
    assert_eq!(distinct.len(), waiting.len(), "each a step on");
    // It starts to play no sooner than the minimum after the click.
    let started = out.find(",c=1,s=3").expect("it plays");
    assert!(out[..started].matches("\x1b[?2026h").count() >= 4);
}

/// The pager on `stops()`, in the text layout, with reading mode to turn on
/// with `M`; every write, and the terminal.
fn text_then_reading(reads: &[&[u8]]) -> (String, Fake) {
    let (document, mut inputs) = stops();
    inputs.caps.cell_pixels = Some((10, 20));
    let mut terminal = Fake::new(100, 14, reads);
    run_document(
        &document,
        &mut inputs,
        Options {
            reading: Some(super::reading::reading()),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    (String::from_utf8(terminal.out.clone()).unwrap(), terminal)
}

#[test]
fn the_selected_animation_stays_selected_from_the_text_layout_to_reading_mode_and_back() {
    // Tab selects the first animation; M turns reading mode on, M off.
    let reads: [&[u8]; 6] = [b"", b"\t", b"M", b"", b"M", b"q"];
    let (_, terminal) = text_then_reading(&reads);
    let bars = statuses(&terminal);
    let named = |at: usize| bars[at].contains("first");
    let selected: Vec<usize> = (0..bars.len()).filter(|n| named(*n)).collect();
    // Selected before, and in the frames after each M.
    assert!(selected.len() >= 3, "{bars:?}");
    assert!(
        named(bars.len() - 1),
        "still selected at the end: {:?}",
        bars.last()
    );
}

#[test]
fn enter_starts_the_selected_animation_in_reading_mode_it_was_selected_in_the_text_layout() {
    let reads: [&[u8]; 6] = [b"", b"\t", b"M", b"", b"\r", b"q"];
    let (out, _) = text_then_reading(&reads);
    assert!(
        told(&out).iter().any(|c| c.starts_with("c=1,s=3")),
        "Enter started it: {:?}",
        told(&out)
    );
}

#[test]
fn the_selected_link_stays_selected_too() {
    let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
    let mut inputs = Inputs::new(Capabilities::kitty(100));
    inputs.caps.cell_pixels = Some((10, 20));
    let reads: [&[u8]; 5] = [b"", b"\t", b"M", b"", b"q"];
    let mut terminal = Fake::new(100, 6, &reads);
    run_document(
        &document,
        &mut inputs,
        Options {
            reading: Some(super::reading::reading()),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    let bars = statuses(&terminal);
    assert!(
        bars.last().unwrap().contains("https://one.example/"),
        "{bars:?}"
    );
}

#[test]
fn the_selection_is_found_again_when_the_layouts_do_not_stop_at_the_same_animations() {
    // The picture inside the line of text is a stop of the text layout, and
    // not of reading mode, where it shows its first frame: the block picture
    // after it is the second stop in one and the first in the other.
    let source = "A line with ![inline](anim.gif) a picture in it.\n\n![block](other.gif)\n";
    let document = crate::typeset::parse::parse(source);
    let (_, mut inputs) = stops();
    inputs.caps.cell_pixels = Some((10, 20));
    let reads: [&[u8]; 7] = [b"", b"\t", b"\t", b"M", b"", b"\r", b"q"];
    let mut terminal = Fake::new(100, 14, &reads);
    run_document(
        &document,
        &mut inputs,
        Options {
            reading: Some(super::reading::reading()),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    let bars = statuses(&terminal);
    // Selected in the text layout, and told after M too.
    assert!(
        bars.iter().filter(|bar| bar.contains("block")).count() >= 3,
        "{bars:?}"
    );
    // And Enter in reading mode starts it, and does not scroll down a line.
    assert!(
        told(&out).iter().any(|c| c.starts_with("c=1,s=3")),
        "{:?}",
        told(&out)
    );
}

/// A click on the status line, whose row is the last of `rows`.
fn click_on_the_status_line(rows: u16) -> Vec<u8> {
    format!("\x1b[<0;60;{rows}M\x1b[<0;60;{rows}m").into_bytes()
}

#[test]
fn the_pill_stays_when_its_picture_is_off_the_screen() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(8));
    reads.extend([b"G".as_slice(), b"", b"q"]);
    let (_, terminal) = played_wide(100, 6, animated(12, Plays::Forever), &reads);
    let bars = statuses(&terminal);
    let last = bars.last().unwrap();
    assert!(last.contains("anim") || last.contains("first"), "{last:?}");
    assert!(last.contains(" s"), "the pill's times: {last:?}");
}

#[test]
fn a_click_on_the_pill_scrolls_to_its_picture_and_selects_it() {
    let click = click_on_the_status_line(6);
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(8));
    reads.extend([b"G".as_slice(), b"", click.as_slice(), b"", b"p", b"q"]);
    let (out, terminal) = played_wide(100, 6, animated(12, Plays::Forever), &reads);
    let frames = terminal.frames();
    // Down at the end the first picture is out of the window; after the click
    // it is back, and `p`, which acts on the one selected, starts it.
    let back = frames
        .iter()
        .rposition(|frame| unstyled(frame).contains("line 0"))
        .expect("the top of the page was shown again");
    let end = frames
        .iter()
        .rposition(|frame| unstyled(frame).contains("line 11"))
        .unwrap();
    assert!(
        back > end,
        "the window went back to the picture after the click"
    );
    // Started again by `p`, which has the one selected to act on (the
    // selected picture is sent in a frame of its own, which is told too).
    let starts = told(&out)
        .iter()
        .filter(|c| c.starts_with("c=1,s=3"))
        .count();
    assert!(starts >= 2, "{:?}", told(&out));
    // The picture was selected by the click, before `p`: it has its frame.
    let images = sent_pixels(&out, "a=T,");
    assert!(
        images.iter().any(|image| image.chunks(4).any(is_frame)),
        "a selection frame was drawn"
    );
}

#[test]
fn p_with_no_animation_on_the_screen_goes_to_the_pill_s_picture_and_starts_it() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(8));
    reads.extend([b"G".as_slice(), b"", b"p", b"", b"", b"", b"q"]);
    let (out, terminal) = played_wide(100, 6, animated(12, Plays::Forever), &reads);
    let screens = terminal.screens();
    let starts = told(&out)
        .iter()
        .filter(|c| c.starts_with("c=1,s=3"))
        .count();
    assert!(starts >= 2, "{:?}", told(&out));
    assert!(
        unstyled(&screen_text(screens.last().unwrap())).contains("line 0") || {
            let n = screens.len();
            unstyled(&screen_text(&screens[n - 2])).contains("line 0")
        }
    );
}

#[test]
fn the_frame_goes_round_a_selected_picture_in_a_part_that_has_a_search_mark_on_it_too() {
    // A word searched for in the part of the page the picture is in: the
    // part is drawn with the mark over it, and the frame is round the picture
    // all the same.
    let source = "![first](anim.gif)\n\nThe needle is here.\n";
    let document = crate::typeset::parse::parse(source);
    let (_, mut inputs) = two_in_a_part();
    inputs
        .images
        .insert("anim.gif", Ok(big_clip(Plays::Forever)));
    let reads: [&[u8]; 6] = [b"", b"/needle\r", b"", b"\t", b"", b"q"];
    let (out, _) = played_in(30, (document, inputs), &reads);
    let images = sent_pixels(&out, "a=T,");
    let last = images.last().expect("an image");
    assert!(
        last.chunks(4).any(is_frame),
        "the picture has its frame, with the mark on the part"
    );
}

#[test]
fn a_selected_animation_that_is_also_the_pill_keeps_its_text_until_the_first_esc() {
    // Tab selects it, `p` starts it; played for its three seconds once.
    let mut reads: Vec<&[u8]> = vec![b"", b"\t", b"p"];
    reads.extend(idle(40));
    reads.push(b"q");
    let (_, terminal) = played_wide(100, 6, animated(0, Plays::Times(1)), &reads);
    let bars = statuses(&terminal);
    // The selected text and the pill's name: the name twice on the line.
    let twice =
        |bar: &str, mark: &str| bar.contains(mark) && bar.matches("an animation").count() == 2;
    assert!(
        bars.iter().any(|bar| twice(bar, "\u{25b6} an animation")),
        "playing: {bars:?}"
    );
    let last = bars.last().unwrap();
    assert!(twice(last, "\u{25a0} an animation"), "ended: {last:?}");
    // Started and stopped again by a second `p`: the pill stays, stopped.
    let reads: [&[u8]; 5] = [b"", b"\t", b"p", b"p", b"q"];
    let (_, terminal) = played_wide(100, 6, animated(0, Plays::Forever), &reads);
    let last = statuses(&terminal).pop().unwrap();
    assert!(twice(&last, "\u{25a0} an animation"), "stopped: {last:?}");
    // The first Esc takes the text with the frame; the second the pill.
    let reads: [&[u8]; 7] = [b"", b"\t", b"p", b"\x1b", b"\x1b", b"\x1b", b"q"];
    let (_, terminal) = played_wide(100, 6, animated(0, Plays::Forever), &reads);
    let bars = statuses(&terminal);
    let both = bars
        .iter()
        .rposition(|bar| twice(bar, "\u{25b6}"))
        .expect("both");
    let cleared = (both..bars.len())
        .find(|at| bars[*at].matches("an animation").count() == 1)
        .expect("the text goes");
    assert!(
        bars[cleared].contains("\u{25b6} an animation"),
        "{:?}",
        bars[cleared]
    );
    let off = (cleared..bars.len())
        .find(|at| !bars[*at].contains("an animation"))
        .unwrap_or_else(|| panic!("the pill goes: {:?}", &bars[cleared..]));
    assert!(bars[off..].iter().all(|bar| !bar.contains("an animation")));
}

#[test]
fn a_selected_animation_with_a_long_alt_text_is_told_whole_in_a_wide_window() {
    let alt = "a long alt text of thirty cell";
    let source = format!("![{alt}](anim.gif)\n\nline\n");
    let document = crate::typeset::parse::parse(&source);
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs
        .images
        .insert("anim.gif", Ok(clip(Duration::from_secs(1), Plays::Forever)));
    let reads: [&[u8]; 5] = [b"", b"\t", b"p", b"", b"q"];
    let (_, terminal) = played_wide(95, 6, (document, inputs), &reads);
    let last = statuses(&terminal).pop().unwrap();
    assert_eq!(last.matches(alt).count(), 1, "{last:?}");
    // The `…` is the pill's own name, cut at 14 cells, not the selected text.
    assert!(last.contains('\u{25b6}') && last.contains('…'), "{last:?}");
}

/// The focus leaving the pane under tmux, and coming back.
const FOCUS_OUT: &[u8] = b"\x1b[O";
const FOCUS_IN: &[u8] = b"\x1b[I";

/// `animated`, with the terminal behind tmux: kitty plays the frames,
/// which go through the passthrough.
fn behind_tmux(plays: Plays) -> (Document, Inputs) {
    let (document, mut inputs) = animated(0, plays);
    inputs.caps.tmux_passthrough = true;
    (document, inputs)
}

/// kitty's answer that it holds image `id`.
fn holds(id: u32) -> Vec<u8> {
    format!("\x1b_Gi={id},p=1263096393;OK\x1b\\").into_bytes()
}

/// The id of the image that an animation started by [`CLICK`] is sent in,
/// with its frames: the last image sent.
fn driven_id_of(plays: Plays) -> u32 {
    let reads: [&[u8]; 3] = [b"", CLICK, b"q"];
    let (out, _) = played(behind_tmux(plays), &reads);
    *sent_ids(&out).last().unwrap()
}

#[test]
fn behind_tmux_an_animation_of_the_text_layout_that_kitty_lost_is_sent_again() {
    // kitty's answer that it does not hold the image `id`.
    let lost = |id: u32| {
        let message = format!("ENOENT:Put command refers to non-existent image with id: {id}");
        format!("\x1b_Gi={id},p=1263096393;{message}\x1b\\").into_bytes()
    };
    let id = driven_id_of(Plays::Forever);
    let answer = lost(id);
    let mut reads: Vec<&[u8]> = vec![b"", CLICK, b"", &answer];
    reads.extend(idle(40));
    reads.push(b"q");
    let (out, _) = played(behind_tmux(Plays::Forever), &reads);
    // The animation with its frames is sent when it starts, and again when
    // the answer says kitty lost it.
    let sends = sent_ids(&out).iter().filter(|sent| **sent == id).count();
    assert_eq!(sends, 2, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 4, "{out:?}");
}

#[test]
fn behind_tmux_an_animation_is_sent_with_its_frames_and_told_to_play() {
    let reads: [&[u8]; 3] = [b"", CLICK, b"q"];
    let (out, _) = played(behind_tmux(Plays::Forever), &reads);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    assert_eq!(told(&out), ["c=1,s=3,v=4"], "{out:?}");
    // And the image with its frames is checked, as any image sent through
    // tmux.
    let id = driven_id_of(Plays::Forever);
    assert!(
        ids_after(&out, super::images::CHECK).contains(&id),
        "{out:?}"
    );
}

#[test]
fn an_animation_sent_just_before_the_focus_went_comes_back_stopped_on_its_first_frame() {
    // The focus-out is read right after the frames went out, and tmux
    // dropped what was written after it.
    let reads: [&[u8]; 6] = [b"", CLICK, FOCUS_OUT, b"", FOCUS_IN, b"q"];
    let (out, terminal) = played(behind_tmux(Plays::Forever), &reads);
    // What it comes back as is its first frame, a still image sent again;
    // the frames go when the reader starts it, and kitty is not told to play
    // what was sent again.
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    assert_eq!(plays(&out), 1, "{out:?}");
    let ids = sent_ids(&out);
    assert_eq!(ids.len(), 3, "{out:?}");
    assert_eq!(ids[0], ids[2], "{out:?}");
    let bars = statuses(&terminal);
    let last = bars.last().unwrap();
    assert!(last.contains("\u{25a0} an animation"), "stopped: {last:?}");
}

#[test]
fn an_animation_cut_by_the_focus_and_sent_again_is_not_played_by_the_send() {
    // The reader starts it again after the return: the frames go with the
    // start, and then kitty plays them.
    let reads: [&[u8]; 8] = [b"", CLICK, FOCUS_OUT, b"", FOCUS_IN, b"", CLICK, b"q"];
    let (out, _) = played(behind_tmux(Plays::Forever), &reads);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 4, "{out:?}");
    // One play for each start, and none for the send in between.
    assert_eq!(plays(&out), 2, "{out:?}");
}

#[test]
fn a_frame_drawn_after_the_send_does_not_keep_the_animation_from_being_stopped() {
    // The status line's time moves on while it plays: frames with nothing
    // sent come between, and the focus-out is read after them.
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(2));
    reads.extend([FOCUS_OUT, b"", FOCUS_IN, b"q"]);
    let (out, terminal) = played(behind_tmux(Plays::Forever), &reads);
    let frames = terminal.frames();
    let sending = frames.iter().position(|frame| frame.contains("a=f,"));
    let next = &frames[sending.unwrap() + 1];
    assert!(
        !next.contains("\x1b_Ga=T,"),
        "a frame with nothing sent: {next:?}"
    );
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    assert_eq!(plays(&out), 1, "{out:?}");
    let bars = statuses(&terminal);
    let last = bars.last().unwrap();
    assert!(last.contains("\u{25a0} an animation"), "stopped: {last:?}");
}

#[test]
fn an_animation_kitty_confirmed_before_the_focus_went_is_not_sent_again() {
    let held = holds(driven_id_of(Plays::Forever));
    let reads: [&[u8]; 7] = [b"", CLICK, &held, FOCUS_OUT, b"", FOCUS_IN, b"q"];
    let (out, _) = played(behind_tmux(Plays::Forever), &reads);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
}

/// How many times kitty was told to play the first image, from its first
/// frame: those of its controls that are play controls.
fn plays(out: &str) -> usize {
    told(out).iter().filter(|keys| keys.contains(",s=")).count()
}

/// The seconds that the status lines show of the animation's time, as
/// `1.5 / 3.0 s`, in order.
fn pill_times(terminal: &Fake) -> Vec<f64> {
    (statuses(terminal).iter())
        .filter_map(|bar| {
            let (before, _) = bar.split_once(" / ")?;
            before.rsplit(' ').next()?.parse().ok()
        })
        .collect()
}

#[test]
fn an_animation_that_played_when_the_focus_went_is_stopped_on_its_first_frame_when_it_comes_back() {
    // kitty confirmed the frames before the pane was hidden, so nothing is
    // sent again; kitty paused the animation meanwhile.
    let held = holds(driven_id_of(Plays::Forever));
    let mut reads: Vec<&[u8]> = vec![b"", CLICK, &held];
    reads.extend(idle(8));
    reads.push(FOCUS_OUT);
    reads.extend(idle(2));
    reads.push(FOCUS_IN);
    reads.extend(idle(2));
    reads.push(b"q");
    let (out, terminal) = played_wide(100, 6, behind_tmux(Plays::Forever), &reads);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    // Played once, and then stopped, back on its first frame, with nothing
    // that plays it again.
    assert_eq!(told(&out), ["c=1,s=3,v=4", "s=1", "c=1"], "{out:?}");
    // The status line shows it stopped with the time at 0, after the time
    // had gone past the first second.
    let times = pill_times(&terminal);
    assert!(times.iter().any(|&time| time >= 1.0), "{times:?}");
    assert_eq!(*times.last().unwrap(), 0.0, "{times:?}");
    let bars = statuses(&terminal);
    let last = bars.last().unwrap();
    assert!(last.contains("\u{25a0} an animation"), "stopped: {last:?}");
}

#[test]
fn an_animation_that_kiris_clock_ended_while_the_focus_was_away_is_stopped_on_its_first_frame_when_it_comes_back()
 {
    // One play takes three seconds.
    let held = holds(driven_id_of(Plays::Times(1)));
    let mut reads: Vec<&[u8]> = vec![b"", CLICK, &held];
    reads.extend(idle(4));
    reads.push(FOCUS_OUT);
    reads.extend(idle(40));
    reads.push(FOCUS_IN);
    reads.extend(idle(4));
    reads.push(b"q");
    let (out, terminal) = played_wide(100, 6, behind_tmux(Plays::Times(1)), &reads);
    assert_eq!(told(&out), ["c=1,s=3,v=2", "s=1", "c=1"], "{out:?}");
    let bars = statuses(&terminal);
    let last = bars.last().unwrap();
    assert!(last.contains("\u{25a0} an animation"), "stopped: {last:?}");
    assert_eq!(*pill_times(&terminal).last().unwrap(), 0.0);
}

#[test]
fn an_animation_stopped_by_the_focus_coming_back_starts_again_with_its_plays_from_the_start() {
    let held = holds(driven_id_of(Plays::Times(1)));
    let mut reads: Vec<&[u8]> = vec![b"", CLICK, &held];
    reads.extend(idle(4));
    reads.push(FOCUS_OUT);
    reads.push(FOCUS_IN);
    reads.extend(idle(2));
    reads.push(CLICK);
    reads.extend(idle(4));
    reads.push(b"q");
    let (out, terminal) = played_wide(100, 6, behind_tmux(Plays::Times(1)), &reads);
    // Played, stopped by the return, played again by the click: the same
    // play count, which starts over.
    let all = told(&out);
    assert_eq!(all.first().unwrap(), "c=1,s=3,v=2", "{out:?}");
    assert_eq!(all.last().unwrap(), "c=1,s=3,v=2", "{out:?}");
    let bars = statuses(&terminal);
    let last = bars.last().unwrap();
    assert!(last.contains("\u{25b6} an animation"), "playing: {last:?}");
}

#[test]
fn an_animation_that_was_stopped_or_ended_before_the_focus_went_is_not_started_by_it_coming_back() {
    let held = holds(driven_id_of(Plays::Forever));
    // Started, and stopped by a second click.
    let reads: [&[u8]; 8] = [b"", CLICK, &held, CLICK, FOCUS_OUT, b"", FOCUS_IN, b"q"];
    let (out, _) = played(behind_tmux(Plays::Forever), &reads);
    assert_eq!(told(&out), ["c=1,s=3,v=4", "s=1", "c=1"], "{out:?}");
    // Never started.
    let reads: [&[u8]; 5] = [b"", FOCUS_OUT, b"", FOCUS_IN, b"q"];
    let (out, _) = played(behind_tmux(Plays::Forever), &reads);
    assert_eq!(told(&out), Vec::<String>::new(), "{out:?}");
}

#[test]
fn outside_tmux_the_focus_going_and_coming_back_changes_nothing_for_an_animation() {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK];
    reads.extend(idle(8));
    reads.push(FOCUS_OUT);
    reads.extend(idle(2));
    reads.push(FOCUS_IN);
    reads.extend(idle(2));
    reads.push(b"q");
    let (out, terminal) = played_wide(100, 6, animated(0, Plays::Forever), &reads);
    assert_eq!(told(&out), ["c=1,s=3,v=4"], "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    let times = pill_times(&terminal);
    assert!(times.windows(2).all(|pair| pair[0] <= pair[1]), "{times:?}");
}

/// Two clips of 64 x 64 pixels, red, green and blue, one under the other in
/// one part of a page of reading mode: large enough for the marks.
fn two_big_in_a_part() -> (Document, Inputs) {
    let (document, mut inputs) = two_in_a_part();
    let clip = |side: u32| {
        let frames = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
            .into_iter()
            .map(|rgba| Frame {
                image: image::RgbaImage::from_pixel(side, side, image::Rgba(rgba)),
                gap: Duration::from_secs(1),
            })
            .collect();
        Source::Animated(std::sync::Arc::new(Animation {
            frames,
            plays: Plays::Forever,
            over_budget: false,
        }))
    };
    inputs.images.insert("a.gif", Ok(clip(64)));
    inputs.images.insert("b.gif", Ok(clip(64)));
    (document, inputs)
}

#[test]
fn the_frames_of_the_second_picture_keep_the_play_button_of_the_first_that_it_stopped() {
    // A click on a starts it; a click on b starts b while a plays, which
    // stops a. (A click selects nothing, so the frames are built for the
    // placement the page is drawn at.)
    let click_a: &[u8] = b"\x1b[<0;3;2M\x1b[<0;3;2m";
    let click_b: &[u8] = b"\x1b[<0;3;8M\x1b[<0;3;8m";
    let mut reads: Vec<&[u8]> = vec![b"", click_a];
    reads.extend(idle(12));
    reads.push(click_b);
    reads.extend(idle(12));
    reads.push(b"q");
    let (_, terminal) = played_in(20, two_big_in_a_part(), &reads);
    let shown = Shown::of(&terminal, (10, 20));
    // a's frames are on the screen when a starts, b's when b starts, and each
    // has the button of the picture that stands still, whose triangle is
    // white, as a picture that is not playing has one.
    let mut playing = Vec::new();
    for update in shown.with_images() {
        if shown.frames(update) < 3 {
            continue;
        }
        let ids = shown.ids(update);
        if !playing.contains(&ids) {
            playing.push(ids);
        }
        for frame in 0..shown.frames(update) {
            let picture = shown.picture(update, frame).unwrap();
            assert!(
                has_white(&picture.rgba),
                "screen {update}, frame {frame}: no play button"
            );
        }
    }
    assert!(
        playing.len() >= 2,
        "a played and b played: {} screens of playing",
        playing.len()
    );
}

#[test]
fn the_frames_of_the_second_picture_keep_the_first_as_it_was_stopped_when_it_was_waiting() {
    // b starts while a still waits for its frames, so the marks of before
    // the press hold a's turning mark; b's frames must show a as the still
    // of a stopped picture does. (A turning mark is white too, so the rows
    // of a are compared, not the white.)
    let click_a: &[u8] = b"\x1b[<0;3;2M\x1b[<0;3;2m";
    let click_b: &[u8] = b"\x1b[<0;3;8M\x1b[<0;3;8m";
    let mut reads: Vec<&[u8]> = vec![b"", click_a];
    reads.extend(idle(2));
    reads.push(click_b);
    reads.extend(idle(12));
    reads.push(b"q");
    let (_, terminal) = played_held(two_big_in_a_part(), &reads, 6);
    let shown = Shown::of(&terminal, (10, 20));
    // The first screen with a picture is the page as it stood before any
    // press, with the button on each picture; the top quarter of the page
    // holds a.
    let first = shown.picture(shown.with_images()[0], 0).unwrap();
    let top = first.height / 4;
    let last = *shown.with_images().last().unwrap();
    assert_eq!(shown.frames(last), 3, "b's frames were on the screen");
    for frame in 0..3 {
        let picture = shown.picture(last, frame).unwrap();
        assert_eq!(picture.width, first.width);
        assert!(
            picture.top(top) == first.top(top),
            "a in frame {frame} of b differs from a stopped"
        );
    }
}

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const CYAN: [u8; 4] = [0, 255, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];

/// Clicks on the upper picture of [`two_finite_in_a_part`] and on the
/// lower one.
const CLICK_A: &[u8] = b"\x1b[<0;3;2M\x1b[<0;3;2m";
const CLICK_B: &[u8] = b"\x1b[<0;3;8M\x1b[<0;3;8m";

/// Two clips of 64 x 64 pixels in one part of a page of reading mode, each
/// playing once: `a.gif` red, green and blue; `b.gif` cyan, magenta and
/// yellow.
fn two_finite_in_a_part() -> (Document, Inputs) {
    let (document, mut inputs) = two_in_a_part();
    let clip = |colours: [[u8; 4]; 3]| {
        let frames = colours
            .into_iter()
            .map(|rgba| Frame {
                image: image::RgbaImage::from_pixel(64, 64, image::Rgba(rgba)),
                gap: Duration::from_secs(1),
            })
            .collect();
        Source::Animated(std::sync::Arc::new(Animation {
            frames,
            plays: Plays::Times(1),
            over_budget: false,
        }))
    };
    let a = clip([RED, [0, 255, 0, 255], BLUE]);
    inputs.images.insert("a.gif", Ok(a));
    let b = clip([CYAN, [255, 0, 255, 255], YELLOW]);
    inputs.images.insert("b.gif", Ok(b));
    (document, inputs)
}

/// The pixels the screen shows at the end, as the first frame of each
/// image on it, and what each update of the run showed, one line each.
fn last_shown(terminal: &Fake) -> (Vec<u8>, Vec<String>) {
    let shown = Shown::of(terminal, (10, 20));
    let colours = [("R", RED), ("B", BLUE), ("C", CYAN), ("Y", YELLOW)];
    let timeline = (shown.with_images().into_iter())
        .map(|update| {
            let picture = shown.picture(update, 0).unwrap();
            let seen: String = (colours.iter())
                .filter(|(_, rgba)| has(&picture.rgba, *rgba))
                .map(|(name, _)| *name)
                .collect();
            format!("{update:03} {seen} white:{}", has_white(&picture.rgba))
        })
        .collect();
    (shown.last_picture(0).rgba, timeline)
}

/// a plays to its end, b plays to its end, and a is started again.
fn replayed() -> Vec<&'static [u8]> {
    replayed_with(CLICK_B)
}

/// [`replayed`] with `click_b` the click on b.
fn replayed_with(click_b: &'static [u8]) -> Vec<&'static [u8]> {
    let mut reads: Vec<&[u8]> = vec![b"", CLICK_A];
    reads.extend(idle(50));
    reads.push(click_b);
    reads.extend(idle(50));
    reads.push(CLICK_A);
    reads.extend(idle(12));
    reads.push(b"q");
    reads
}

#[test]
fn a_replay_shows_the_neighbour_as_it_is_now_not_as_it_was_at_the_first_play() {
    // The image of a's first play holds b on its first frame (cyan).
    let (_, terminal) = played_in(20, two_finite_in_a_part(), &replayed());
    let (shown, timeline) = last_shown(&terminal);
    assert!(
        has(&shown, YELLOW) && !has(&shown, CYAN),
        "b has ended and must be on its last frame while a plays again: {timeline:#?}"
    );
}

#[test]
fn in_ghostty_a_replay_shows_the_neighbour_as_it_is_now() {
    // The frames kiri plays itself were kept as they were at the first play.
    let (_, terminal) = played_in(20, ghostty(two_finite_in_a_part()), &replayed());
    let (shown, timeline) = last_shown(&terminal);
    assert!(
        has(&shown, YELLOW) && !has(&shown, CYAN),
        "b has ended and must be on its last frame while a plays again: {timeline:#?}"
    );
}

#[test]
fn a_stopped_neighbour_is_on_its_first_frame_while_the_other_plays_again() {
    // The reported steps after one play of b while a had ended: b's image of
    // that play holds a ended (blue).
    let mut reads: Vec<&[u8]> = vec![b"", CLICK_A];
    reads.extend(idle(50));
    reads.push(CLICK_B);
    reads.extend(idle(50));
    reads.push(CLICK_A);
    reads.extend(idle(12));
    reads.push(CLICK_B);
    reads.extend(idle(12));
    reads.push(b"q");
    let (_, terminal) = played_in(20, two_finite_in_a_part(), &reads);
    let (shown, timeline) = last_shown(&terminal);
    assert!(
        has(&shown, RED) && !has(&shown, BLUE),
        "a was stopped and must be on its first frame while b plays: {timeline:#?}"
    );
}

/// The ids of the images `out` sends with their frames, each once, in the
/// order they are first sent.
fn driven_ids(out: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    for id in ids_after(out, "\x1b_Ga=f,q=2,f=32,o=z,i=") {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// [`two_finite_in_a_part`] with `a.gif` 42 pixels high: the cells its
/// inset covers go down to 60 pixels, and `b.gif`, 16 pixels below it, starts
/// at 58, so that b's top is in a's inset. b is on the rows 3 to 7 of the
/// window, where [`CLICK_CLOSE_B`] clicks.
fn two_finite_close() -> (Document, Inputs) {
    let (document, mut inputs) = two_finite_in_a_part();
    let frames = [RED, [0, 255, 0, 255], BLUE]
        .into_iter()
        .map(|rgba| Frame {
            image: image::RgbaImage::from_pixel(42, 42, image::Rgba(rgba)),
            gap: Duration::from_secs(1),
        })
        .collect();
    let a = Source::Animated(std::sync::Arc::new(Animation {
        frames,
        plays: Plays::Times(1),
        over_budget: false,
    }));
    inputs.images.insert("a.gif", Ok(a));
    (document, inputs)
}

/// A click on b of [`two_finite_close`].
const CLICK_CLOSE_B: &[u8] = b"\x1b[<0;3;6M\x1b[<0;3;6m";

#[test]
fn a_replay_whose_inset_is_as_it_was_keeps_its_image_and_deletes_nothing() {
    // b is outside a's inset: a's second play shows what its first did.
    let (out, _) = played_in(20, two_finite_in_a_part(), &replayed());
    let driven = driven_ids(&out);
    // a's play, both times, and b's play.
    assert_eq!(driven.len(), 2, "{driven:?}");
    let first = format!("\x1b_Ga=T,U=1,q=2,f=32,o=z,i={},", driven[0]);
    assert_eq!(out.matches(&first).count(), 1, "sent once: {out:?}");
    let (running, _) = until_quit(&out);
    assert!(!deleted_ids(running).contains(&driven[0]), "{out:?}");
    assert_eq!(sorted(deleted_ids(&out)), sorted(sent_ids(&out)), "{out:?}");
}

#[test]
fn the_inset_of_the_first_play_that_a_replay_replaces_is_deleted() {
    // A replaced image that is forgotten stays in kitty, even after the quit.
    // b's top is in a's inset, and b has ended when a plays again.
    let (out, _) = played_in(20, two_finite_close(), &replayed_with(CLICK_CLOSE_B));
    let driven = driven_ids(&out);
    // a's first play, b's play, a's second play.
    assert_eq!(driven.len(), 3, "{driven:?}");
    let (running, _) = until_quit(&out);
    assert!(deleted_ids(running).contains(&driven[0]), "{out:?}");
    assert_eq!(sorted(deleted_ids(&out)), sorted(sent_ids(&out)), "{out:?}");
}

#[test]
fn in_ghostty_the_frames_a_replay_replaces_are_deleted() {
    // The same for the frames kiri plays itself: none stays in the terminal.
    let (out, _) = played_in(20, ghostty(two_finite_in_a_part()), &replayed());
    assert_eq!(sorted(deleted_ids(&out)), sorted(sent_ids(&out)), "{out:?}");
}

#[test]
fn behind_tmux_the_image_a_replay_replaces_goes_once_kitty_answers_for_the_new_one() {
    // tmux draws the cells of the new image only after the update that sends
    // it: a delete before then would leave them showing nothing.
    let behind_tmux = || {
        let (document, mut inputs) = two_finite_close();
        inputs.caps.tmux_passthrough = true;
        (document, inputs)
    };
    let (out, _) = played_in(20, behind_tmux(), &replayed_with(CLICK_CLOSE_B));
    let driven = driven_ids(&out);
    assert_eq!(driven.len(), 3, "{driven:?}");
    let (first, again) = (driven[0], driven[2]);
    // With no answer, it waits until the quit.
    let (running, _) = until_quit(&out);
    assert!(!deleted_ids(running).contains(&first), "{out:?}");
    assert_eq!(sorted(deleted_ids(&out)), sorted(sent_ids(&out)), "{out:?}");
    // With kitty's answer for the new one, it goes after the answer.
    let answer = holds(again);
    let mut reads: Vec<&[u8]> = replayed_with(CLICK_CLOSE_B);
    let at = reads.iter().rposition(|read| *read == CLICK_A).unwrap();
    reads.insert(at + 3, &answer);
    let (out, _) = played_in(20, behind_tmux(), &reads);
    let (running, _) = until_quit(&out);
    let sent = running.find(&format!("\x1b_Ga=T,U=1,q=2,f=32,o=z,i={again},"));
    let deleted = running.find(&format!("\x1b_Ga=d,d=I,i={first},"));
    assert!(sent.is_some() && sent < deleted, "{out:?}");
    assert_eq!(sorted(deleted_ids(&out)), sorted(sent_ids(&out)), "{out:?}");
}

/// A page of reading mode with a clip of 120 x 120 pixels at its top and
/// forty paragraphs under it; with `band`, a code block wider than the
/// window between them, which scrolls sideways as a band.
fn big_then_text(band: bool) -> (Document, Inputs) {
    let mut source = String::from("![first](anim.gif)\n\n");
    if band {
        source.push_str(&format!("```\n{}\n```\n\n", "wide ".repeat(60)));
    }
    for line in 0..40 {
        source.push_str(&format!("line {line}\n\n"));
    }
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    if let (true, Some(reading)) = (band, inputs.reading.as_mut()) {
        reading.wide = crate::typeset::reading::page::Wide::Scroll;
    }
    inputs
        .images
        .insert("anim.gif", Ok(big_clip(Plays::Forever)));
    (crate::typeset::parse::parse(&source), inputs)
}

/// How many times something was told to play in `document`, after a click,
/// `G`, and `p` with no animation on the screen; with `resize` the window
/// gets wider between `G` and `p`.
fn starts_after_going_back(
    (document, mut inputs): (Document, Inputs),
    resize: bool,
) -> (usize, Vec<String>) {
    let mut reads: Vec<&[u8]> = vec![b"", b"\x1b[<0;3;2M\x1b[<0;3;2m"];
    reads.extend(idle(8));
    reads.extend([b"G".as_slice(), b"", b"", b"", b"p"]);
    reads.extend(idle(12));
    reads.push(b"q");
    let mut terminal = Fake::new(60, 20, &reads);
    if resize {
        let mut sizes = vec![window(60, 20); 12];
        sizes.push(window(70, 20));
        terminal.sizes = sizes.into();
    }
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    let all: Vec<String> = controlled(&out).into_iter().map(|(_, keys)| keys).collect();
    let starts = all.iter().filter(|c| c.starts_with("c=1,s=3")).count();
    let bars = statuses(&terminal);
    let mut tail: Vec<String> = bars.iter().rev().take(3).cloned().collect();
    tail.extend(all);
    (starts, tail)
}

#[test]
fn p_goes_back_to_the_pill_s_picture_and_starts_it_after_a_resize() {
    let (starts, told) = starts_after_going_back(big_then_text(false), true);
    assert!(starts >= 2, "{told:#?}");
}

// The band that the scroll of `p` brings into view ends the layout before
// the draw that the deferred build of the picture waits for.
#[test]
fn p_starts_the_pill_s_picture_after_a_resize_when_a_band_comes_into_view() {
    let (starts, told) = starts_after_going_back(big_then_text(true), true);
    assert!(starts >= 2, "{told:#?}");
}
