//! Animated images in the pager: when the terminal is told to play them,
//! stop them and start them again, and what is sent for them.

use super::*;
use crate::session::settings::Animations;
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
        over_budget: None,
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

/// All the pager writes for `document` and `inputs` when it plays
/// animations as `setting` says and the keys `reads` come, one a read.
fn played(inputs: (Document, Inputs), setting: Animations, reads: &[&[u8]]) -> (String, Fake) {
    played_in(6, inputs, setting, reads)
}

/// [`played`] in a window of `rows` rows.
fn played_in(
    rows: u16,
    (document, mut inputs): (Document, Inputs),
    setting: Animations,
    reads: &[&[u8]],
) -> (String, Fake) {
    let mut terminal = Fake::new(60, rows, reads);
    run_document(
        &document,
        &mut inputs,
        Options {
            animations: setting,
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    (String::from_utf8(terminal.out.clone()).unwrap(), terminal)
}

/// `n` reads that give no key.
fn idle(n: usize) -> Vec<&'static [u8]> {
    vec![b"".as_slice(); n]
}

/// The controls `out` sends, as the keys that follow `a=a,q=2,i=ID,`.
fn controls(out: &str) -> Vec<&str> {
    out.split("\x1b_Ga=a,q=2,i=")
        .skip(1)
        .map(|rest| rest.split_once("\x1b\\").unwrap().0)
        .map(|keys| keys.split_once(',').unwrap().1)
        .filter(|keys| !keys.starts_with("r=1,"))
        .collect()
}

#[test]
fn an_animation_in_view_is_sent_with_its_frames_and_starts_after_the_delay() {
    let mut reads = idle(12);
    reads.push(b"q");
    let (out, terminal) = played(animated(0, Plays::Forever), Animations::Once, &reads);
    // Once, with every frame and the first one's gap.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 1, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    assert!(out.contains(",r=1,z=1000\x1b\\"), "{out:?}");
    // Started once, as one play: a loop count of two.
    assert_eq!(controls(&out), ["s=3,v=2"], "{out:?}");
    // Not in the first frame: the delay is over some reads later.
    let first = terminal.frames().remove(0);
    let drawn = first.split("\x1b[?2026l").next().unwrap();
    assert!(!drawn.contains("s=3,v=2"), "{drawn:?}");
}

/// How long after the first read, on the virtual clock, the pager had
/// written what the text `after` is the end of in `out`.
fn written_at(terminal: &Fake, out: &str, after: &str) -> Duration {
    let end = out.find(after).expect("it was written") + after.len();
    let first = terminal.marks[0].1;
    // The read after which the pager wrote it is the next to start.
    let (_, at) = terminal.marks.iter().find(|(len, _)| *len >= end).unwrap();
    *at - first
}

#[test]
fn the_start_waits_what_is_left_of_the_delay_and_comes_exactly_at_it() {
    use crate::session::animation::START_DELAY;
    // A key that does nothing, 30 ms in: the polls then go on from there,
    // and the last wait is what is left of the delay, not a whole poll.
    let mut terminal_reads: Vec<&[u8]> = vec![b"x"];
    terminal_reads.extend(idle(20));
    terminal_reads.push(b"q");
    let (document, mut inputs) = animated(0, Plays::Forever);
    let mut terminal = Fake::new(60, 6, &terminal_reads);
    terminal.typing = Some(Duration::from_millis(30));
    run_document(
        &document,
        &mut inputs,
        Options {
            animations: Animations::Once,
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    assert_eq!(controls(&out), ["s=3,v=2"], "{out:?}");
    assert_eq!(written_at(&terminal, &out, "s=3,v=2\x1b\\"), START_DELAY);
    let waits: Vec<_> = terminal.waits.iter().map(Duration::as_millis).collect();
    assert!(
        waits.contains(&70),
        "the last wait is what is left: {waits:?}"
    );
    assert!(waits.iter().all(|wait| *wait <= 100), "{waits:?}");
}

#[test]
fn with_animations_never_only_the_first_frame_is_sent_and_nothing_is_told() {
    let mut reads = idle(12);
    reads.push(b"q");
    let (out, _) = played(animated(0, Plays::Forever), Animations::Never, &reads);
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 1, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 0, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=a,").count(), 0, "{out:?}");
}

#[test]
fn always_plays_as_often_as_the_file_says() {
    let mut reads = idle(12);
    reads.push(b"q");
    let (out, _) = played(animated(0, Plays::Forever), Animations::Always, &reads);
    assert_eq!(controls(&out), ["s=3,v=1"], "{out:?}");
    let (out, _) = played(animated(0, Plays::Times(3)), Animations::Always, &reads);
    assert_eq!(controls(&out), ["s=3,v=4"], "{out:?}");
}

#[test]
fn an_animation_scrolled_out_of_view_stops_and_goes_on_when_it_is_back() {
    // Twelve lines below it, and a window of five: `G` scrolls it away.
    let mut reads = idle(10);
    reads.push(b"G");
    reads.extend(idle(3));
    reads.push(b"g");
    reads.extend(idle(3));
    reads.push(b"q");
    let (out, _) = played(animated(12, Plays::Forever), Animations::Always, &reads);
    assert_eq!(controls(&out), ["s=3,v=1", "s=1", "s=3,v=1"], "{out:?}");
}

#[test]
fn an_image_that_has_not_been_in_view_for_the_delay_does_not_start() {
    // In view for 300 ms only.
    let mut reads = idle(3);
    reads.push(b"G");
    reads.extend(idle(12));
    reads.push(b"q");
    let (out, _) = played(animated(12, Plays::Forever), Animations::Always, &reads);
    assert_eq!(controls(&out), Vec::<&str>::new(), "{out:?}");
}

#[test]
fn a_finished_animation_is_left_on_its_last_frame_when_it_comes_back() {
    // One play takes three seconds.
    let mut reads = idle(45);
    reads.push(b"G");
    reads.extend(idle(3));
    reads.push(b"g");
    reads.extend(idle(3));
    reads.push(b"q");
    let (out, _) = played(animated(12, Plays::Forever), Animations::Once, &reads);
    assert_eq!(controls(&out), ["s=3,v=2"], "{out:?}");
}

#[test]
fn an_animation_sent_again_is_started_again() {
    // A resize makes another layout of the same image: not sent again.
    let mut reads = idle(10);
    reads.push(b"q");
    let (out, mut terminal) = {
        let (document, mut inputs) = animated(0, Plays::Forever);
        let mut terminal = Fake::new(60, 6, &reads);
        terminal.sizes.push_back(window(70, 6));
        run_document(
            &document,
            &mut inputs,
            Options {
                animations: Animations::Always,
                ..Options::new(SearchCase::Smart)
            },
            &mut terminal,
        );
        (String::from_utf8(terminal.out.clone()).unwrap(), terminal)
    };
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 1, "{out:?}");
    assert_eq!(controls(&out), ["s=3,v=1"], "{out:?}");
    let _ = &mut terminal;
}

#[test]
fn an_image_file_shown_on_its_own_plays_too() {
    let document = Document::new(vec![crate::typeset::model::Block::ImageFile(
        "anim.gif".into(),
    )]);
    let (_, inputs) = animated(0, Plays::Forever);
    let mut reads = idle(12);
    reads.push(b"q");
    let (out, _) = played((document, inputs), Animations::Always, &reads);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    assert_eq!(controls(&out), ["s=3,v=1"], "{out:?}");
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

/// An animation in a paragraph of reading mode, played as `setting` says.
fn in_reading_mode(setting: Animations, reads: &[&[u8]]) -> (String, Fake) {
    let (document, mut inputs) = animated(0, Plays::Forever);
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    played((document, inputs), setting, reads)
}

#[test]
fn in_reading_mode_the_part_that_holds_an_animation_plays_it() {
    let mut reads = idle(12);
    reads.push(b"q");
    let (out, _) = in_reading_mode(Animations::Always, &reads);
    // One part of the page, sent with the frames of the picture in it.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 1, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    assert_eq!(controls(&out), ["s=3,v=1"], "{out:?}");
    // The frames are the part drawn again with the picture's green and
    // blue frames in it.
    let frames = frame_pixels(&out);
    assert!(has(&frames[0], [0, 255, 0, 255]) && !has(&frames[0], [0, 0, 255, 255]));
    assert!(has(&frames[1], [0, 0, 255, 255]) && !has(&frames[1], [0, 255, 0, 255]));
}

#[test]
fn in_reading_mode_the_first_frame_of_the_part_has_the_first_frame_of_the_picture() {
    let (out, _) = in_reading_mode(Animations::Never, &[b"q"]);
    // Not played: the first frame, in the part, as a picture is.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 1, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 0, "{out:?}");
}

#[test]
fn p_pauses_what_plays_in_view_and_a_second_p_plays_it_again() {
    let mut reads = idle(10);
    reads.extend([b"p".as_slice()]);
    reads.extend(idle(3));
    reads.push(b"p");
    reads.extend(idle(2));
    reads.push(b"q");
    let (out, terminal) = played(animated(0, Plays::Forever), Animations::Always, &reads);
    assert_eq!(controls(&out), ["s=3,v=1", "s=1", "s=3,v=1"], "{out:?}");
    // The status line says what p did.
    let frames = terminal.frames();
    assert!(frames.iter().any(|frame| frame.contains("paused")));
    assert!(frames.iter().any(|frame| frame.contains("playing")));
}

#[test]
fn p_with_no_animation_in_view_says_so() {
    let (document, inputs) = (
        crate::typeset::parse::parse("just text\n"),
        Inputs::new(Capabilities::kitty(80)),
    );
    let (out, terminal) = played((document, inputs), Animations::Always, &[b"p", b"q"]);
    assert_eq!(controls(&out), Vec::<&str>::new());
    assert!(
        terminal
            .frames()
            .iter()
            .any(|frame| frame.contains("no animation")),
        "{out:?}"
    );
}

#[test]
fn a_click_on_an_animation_plays_it_again_from_the_first_frame() {
    // The image is on the first rows of the window.
    let click: &[u8] = b"\x1b[<0;3;1M\x1b[<0;3;1m";
    let mut reads = idle(10);
    reads.push(click);
    reads.extend(idle(2));
    reads.push(b"q");
    let (out, _) = played(animated(0, Plays::Forever), Animations::Always, &reads);
    assert_eq!(controls(&out), ["s=3,v=1", "c=1,s=3,v=1"], "{out:?}");
}

#[test]
fn a_click_elsewhere_replays_nothing() {
    let click: &[u8] = b"\x1b[<0;3;5M\x1b[<0;3;5m";
    let mut reads = idle(10);
    reads.push(click);
    reads.push(b"q");
    let (out, _) = played(animated(12, Plays::Forever), Animations::Always, &reads);
    assert_eq!(controls(&out), ["s=3,v=1"], "{out:?}");
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

/// The id of the image that each frame of `out` shows in its placeholders,
/// from the foreground colour that carries it.
fn shown_ids(terminal: &Fake) -> Vec<u32> {
    terminal
        .frames()
        .iter()
        .filter_map(|frame| {
            let at = frame.find("\x1b[38;2;")? + "\x1b[38;2;".len();
            let rgb: Vec<u32> = (frame[at..].split('m').next()?)
                .split(';')
                .filter_map(|n| n.parse().ok())
                .collect();
            Some(rgb[0] << 16 | rgb[1] << 8 | rgb[2])
        })
        .collect()
}

#[test]
fn in_ghostty_kiri_redraws_the_image_with_each_frame_at_its_gap() {
    let mut reads = idle(60);
    reads.push(b"q");
    let (out, terminal) = played(
        ghostty(animated(0, Plays::Forever)),
        Animations::Always,
        &reads,
    );
    // Every frame is a still image of its own, sent when it is first shown,
    // and the terminal is told nothing else.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 3, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 0, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=a,").count(), 0, "{out:?}");
    let ids = shown_ids(&terminal);
    let mut changes = ids.clone();
    changes.dedup();
    // Frame 0 until 1 s + 1 s, then 1, 2, 0, 1 ... a second each.
    assert!(changes.len() >= 5, "{changes:?}");
    assert_eq!(changes[0], changes[3], "{changes:?}");
    assert_ne!(changes[0], changes[1]);
    assert_ne!(changes[1], changes[2]);
    // Drawn when a frame changes, not at every poll.
    assert!(ids.len() < 20, "{} frames drawn", ids.len());
}

#[test]
fn in_ghostty_what_plays_once_rests_on_its_last_frame() {
    let mut reads = idle(80);
    reads.push(b"q");
    let (out, terminal) = played(
        ghostty(animated(0, Plays::Forever)),
        Animations::Once,
        &reads,
    );
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 3, "{out:?}");
    let ids = shown_ids(&terminal);
    let mut changes = ids.clone();
    changes.dedup();
    assert_eq!(changes.len(), 3, "{changes:?}");
    // And draws no more once it has rested, whatever the polls do: the last
    // frame is drawn once.
    let rested = ids.iter().rposition(|id| *id != changes[2]).unwrap() + 1;
    assert_eq!(ids.len(), rested + 1, "{ids:?}");
}

#[test]
fn in_ghostty_p_holds_the_frame_and_plays_on_from_it() {
    let mut reads = idle(14);
    reads.push(b"p");
    reads.extend(idle(40));
    reads.push(b"p");
    reads.extend(idle(15));
    reads.push(b"q");
    let (_, terminal) = played(
        ghostty(animated(0, Plays::Forever)),
        Animations::Always,
        &reads,
    );
    let ids = shown_ids(&terminal);
    let mut changes = ids.clone();
    changes.dedup();
    // Paused on the frame it was on for 4 s, then it goes on.
    assert!(changes.len() >= 2, "{changes:?}");
    let told = |text: &str| terminal.frames().iter().any(|f| f.contains(text));
    assert!(told("paused") && told("playing"));
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

/// The status line of each frame, as text.
fn statuses(terminal: &Fake) -> Vec<String> {
    terminal
        .frames()
        .iter()
        .map(|frame| {
            frame
                .rsplit_once("\x1b[0;1m kiri")
                .map_or("", |(_, bar)| bar)
                .to_owned()
        })
        .collect()
}

#[test]
fn tab_stops_at_animated_images_and_the_status_line_names_the_one_selected() {
    let (_, terminal) = played_in(
        14,
        stops(),
        Animations::Always,
        &[b"\t", b"\t", b"\t", b"q"],
    );
    let bars = statuses(&terminal);
    assert!(!bars[0].contains(".gif"), "{bars:?}");
    assert!(bars[1].contains("anim.gif"), "{bars:?}");
    // Not at the still image, which has no stop: the next is the second
    // animation, and then the first again.
    assert!(bars[2].contains("other.gif"), "{bars:?}");
    assert!(bars[3].contains("anim.gif"), "{bars:?}");
}

#[test]
fn shift_tab_goes_back_from_the_last_animated_image() {
    let (_, terminal) = played_in(
        14,
        stops(),
        Animations::Always,
        &[b"\x1b[Z", b"\x1b[Z", b"q"],
    );
    let bars = statuses(&terminal);
    assert!(bars[1].contains("other.gif"), "{bars:?}");
    assert!(bars[2].contains("anim.gif"), "{bars:?}");
}

#[test]
fn the_selected_animated_image_is_sent_again_with_a_highlighted_frame() {
    let (out, _) = played_in(14, stops(), Animations::Always, &[b"\t", b"q"]);
    // Plain, and again with the frame; the first has the frame in no pixel
    // of its edge. Both go with their frames.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 4, "{out:?}");
}

#[test]
fn enter_on_the_selected_animation_plays_it_from_the_first_frame() {
    let mut reads = idle(10);
    reads.extend([b"\t".as_slice(), b"\r"]);
    reads.push(b"q");
    let (out, terminal) = played_in(14, stops(), Animations::Always, &reads);
    assert!(
        controls(&out).iter().any(|c| c.starts_with("c=1,s=3")),
        "{out:?}"
    );
    // And does not copy anything.
    assert!(!out.contains("\x1b]52;"), "{out:?}");
    assert!(
        !statuses(&terminal)
            .iter()
            .any(|bar| bar.contains("link copied"))
    );
}

#[test]
fn a_linked_animation_is_one_stop_and_enter_copies_its_address() {
    let source = "Text [![linked](anim.gif)](https://example.com/x) more.\n";
    let document = crate::typeset::parse::parse(source);
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs
        .images
        .insert("anim.gif", Ok(clip(Duration::from_secs(1), Plays::Forever)));
    let (out, terminal) = played(
        (document, inputs),
        Animations::Always,
        &[b"\t", b"\t", b"\r", b"q"],
    );
    let bars = statuses(&terminal);
    // One stop: the address, not the file, and a second Tab stays on it.
    assert!(bars[1].contains("https://example.com/x"), "{bars:?}");
    assert!(!bars.iter().any(|bar| bar.contains("anim.gif")), "{bars:?}");
    assert!(
        out.contains("\x1b]52;"),
        "enter copies the address: {out:?}"
    );
}

#[test]
fn a_click_on_a_linked_animation_does_not_replay_it() {
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
        Animations::Always,
        &[
            b"\x1b[<0;3;1M\x1b[<0;3;1m",
            b"\x1b[<0;3;5M\x1b[<0;3;5m",
            b"q",
        ],
    );
    let replays = controls(&out)
        .iter()
        .filter(|c| c.starts_with("c=1"))
        .count();
    assert_eq!(replays, 1, "{out:?}");
}

#[test]
fn in_reading_mode_the_animation_stops_when_its_picture_is_scrolled_out_of_view() {
    let reading = |lines| {
        let (document, mut inputs) = animated(lines, Plays::Forever);
        inputs.caps.cell_pixels = Some((10, 20));
        inputs.reading = Some(super::reading::reading());
        (document, inputs)
    };
    let mut reads = idle(10);
    reads.push(b"G");
    reads.extend(idle(3));
    reads.push(b"g");
    reads.extend(idle(3));
    reads.push(b"q");
    // Lines below it fill the window once scrolled to the end, with the
    // text of the same part of the page on the screen and not the picture.
    let (out, _) = played(reading(12), Animations::Always, &reads);
    assert_eq!(controls(&out), ["s=3,v=1", "s=1", "s=3,v=1"], "{out:?}");
}

#[test]
fn in_reading_mode_p_tab_and_enter_work_on_the_picture() {
    let mut reads = idle(10);
    reads.extend([b"p".as_slice(), b"p", b"\t", b"\r"]);
    reads.push(b"q");
    let (document, mut inputs) = animated(0, Plays::Forever);
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    let (out, terminal) = played((document, inputs), Animations::Always, &reads);
    let all = controls(&out);
    assert_eq!(&all[..3], ["s=3,v=1", "s=1", "s=3,v=1"], "{out:?}");
    // Tab selects it, named by its file, and Enter plays it again.
    assert!(
        statuses(&terminal)
            .iter()
            .any(|bar| bar.contains("anim.gif"))
    );
    assert!(all.iter().any(|c| c.starts_with("c=1,s=3")), "{all:?}");
    // The selected part is sent again with the picture framed in yellow.
    assert!(out.matches("\x1b_Ga=T,").count() >= 2, "{out:?}");
}

#[test]
fn in_reading_mode_a_click_replays_on_the_picture_and_not_on_the_text_of_its_part() {
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
    let mut reads = idle(10);
    reads.extend([text, picture]);
    reads.push(b"q");
    let (out, _) = played_in(10, (document, inputs), Animations::Always, &reads);
    let replays = controls(&out)
        .iter()
        .filter(|c| c.starts_with("c=1"))
        .count();
    assert_eq!(replays, 1, "{out:?}");
}

#[test]
fn in_ghostty_reading_mode_redraws_the_part_with_each_frame() {
    let (document, mut inputs) = ghostty(animated(0, Plays::Forever));
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    let mut reads = idle(60);
    reads.push(b"q");
    let (out, terminal) = played((document, inputs), Animations::Always, &reads);
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 3, "{out:?}");
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 0, "{out:?}");
    let mut changes = shown_ids(&terminal);
    changes.dedup();
    assert!(changes.len() >= 5, "{changes:?}");
}

#[test]
fn p_acts_on_the_selected_animation_only_when_one_is_selected() {
    // Two animations in view, the first selected: only it is paused, under
    // the two ids it has, plain and framed.
    let mut reads = idle(12);
    reads.extend([b"\t".as_slice(), b"p"]);
    reads.push(b"q");
    let (out, _) = played_in(14, stops(), Animations::Always, &reads);
    let stops = controls(&out).iter().filter(|c| **c == "s=1").count();
    assert_eq!(stops, 2, "{out:?}");
}

#[test]
fn in_ghostty_the_redraws_come_at_the_gaps_when_a_gap_is_not_a_poll() {
    let (document, mut inputs) = ghostty(animated(0, Plays::Forever));
    let slow = clip(Duration::from_millis(130), Plays::Forever);
    inputs.images.insert("anim.gif", Ok(slow));
    let mut reads = idle(60);
    reads.push(b"q");
    let (out, terminal) = played((document, inputs), Animations::Always, &reads);
    // Each frame is drawn when its 130 ms are over: the draw after the
    // start (at 1 s) and then every 130 ms, not at a multiple of a poll.
    let draws: Vec<Duration> = terminal
        .frames()
        .iter()
        .scan(0usize, |at, frame| {
            *at += frame.len() + "\x1b[?2026h".len();
            Some(*at)
        })
        .filter_map(|end| {
            let (_, at) = terminal.marks.iter().find(|(len, _)| *len >= end)?;
            Some(*at - terminal.marks[0].1)
        })
        .collect();
    let gaps: Vec<u128> = draws
        .windows(2)
        .map(|w| (w[1] - w[0]).as_millis())
        .collect();
    assert!(
        gaps.iter().skip(2).take(6).all(|gap| *gap == 130),
        "{gaps:?} {out:.0}"
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
    // Scrolled so that only the upper half is in view, then so that the
    // lower half comes in too: `d` is half a window, five rows.
    let mut reads: Vec<&[u8]> = vec![b"d", b"d", b"d"];
    reads.extend(idle(12));
    reads.push(b"d");
    reads.extend(idle(3));
    reads.push(b"q");
    let (out, _) = played_in(12, jiggy_after(12), Animations::Always, &reads);
    let starts = controlled(&out);
    let ids: std::collections::BTreeSet<u32> = starts.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids.len(), 2, "two parts show it: {starts:?}");
    // The lower half comes in at the last `d`, and 0.3 s later it plays with
    // the upper half, not a start delay after it.
    let last_two: Vec<_> = starts[starts.len() - 2..]
        .iter()
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(
        last_two
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2,
        "both halves are started together: {starts:?}"
    );
}

#[test]
fn in_reading_mode_a_click_on_either_half_of_a_picture_replays_all_of_it() {
    // `d` three times shows the upper half only; the click is on its last
    // row, which is the picture's.
    let mut reads: Vec<&[u8]> = vec![b"d", b"d", b"d", b"d"];
    reads.extend(idle(12));
    reads.push(b"\x1b[<0;5;6M\x1b[<0;5;6m");
    reads.extend(idle(2));
    reads.push(b"q");
    let (out, _) = played_in(12, jiggy_after(12), Animations::Always, &reads);
    let replays: Vec<u32> = (controlled(&out).into_iter())
        .filter(|(_, keys)| keys.starts_with("c=1,s=3"))
        .map(|(id, _)| id)
        .collect();
    let ids: std::collections::BTreeSet<_> = replays.iter().collect();
    assert_eq!(ids.len(), 2, "{replays:?}");
}

#[test]
fn in_reading_mode_a_picture_across_two_parts_has_one_tab_stop_and_stops_as_one() {
    let mut reads: Vec<&[u8]> = vec![b"d", b"d", b"d", b"d"];
    reads.extend(idle(12));
    reads.extend([b"\t".as_slice(), b"\t", b"p"]);
    reads.push(b"q");
    let (out, terminal) = played_in(12, jiggy_after(12), Animations::Always, &reads);
    // Two Tabs stay on the one stop, and `p` stops the picture in both parts,
    // each under its plain and its framed id at most.
    let named = statuses(&terminal)
        .iter()
        .filter(|bar| bar.contains("jiggy.gif"))
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
    let mut reads: Vec<&[u8]> = vec![b"d", b"d", b"d", b"d"];
    reads.extend(idle(60));
    reads.push(b"q");
    let (_, terminal) = played_in(12, (document, inputs), Animations::Always, &reads);
    // Both parts are on the screen; every frame has an id for each, and
    // the pairs are those of the two frames of the picture, never mixed.
    let pairs: std::collections::BTreeSet<Vec<String>> = terminal
        .frames()
        .iter()
        .filter_map(|frame| {
            let mut ids: Vec<String> = frame
                .split("\x1b[38;2;")
                .skip(1)
                .map(|rest| rest.split('m').next().unwrap().to_owned())
                .collect();
            ids.dedup();
            (ids.len() == 2).then_some(ids)
        })
        .collect();
    assert_eq!(pairs.len(), 2, "{pairs:?}");
}

#[test]
fn in_the_text_layout_a_picture_half_in_view_plays_whole_and_is_not_started_again() {
    // 512 × 512 pixels in cells of the default size: taller than the window,
    // scrolled so that part of it is above the window, then more.
    let bytes = std::fs::read(crate::load::tests::fixtures().join("images/jiggy.gif")).unwrap();
    let fonts = &crate::images::Fonts::none();
    let source =
        crate::typeset::images::decode::decode_named_as(&bytes, None, 10_000_000, fonts, true);
    let document = crate::typeset::parse::parse("![jiggy](jiggy.gif)\n\nAfter\n");
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.images.insert("jiggy.gif", source);
    let mut reads: Vec<&[u8]> = vec![b"d"];
    reads.extend(idle(12));
    reads.extend([b"d".as_slice(), b"d"]);
    reads.extend(idle(8));
    reads.push(b"q");
    let (out, _) = played_in(10, (document, inputs), Animations::Always, &reads);
    // One image: sent once, with its frames, started once, and nothing
    // sent or started again as it scrolls.
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 1, "{out:?}");
    assert_eq!(controls(&out), ["s=3,v=1"], "{out:?}");
}

#[test]
fn in_reading_mode_a_picture_that_would_play_in_only_some_of_its_parts_sends_no_frames() {
    // 60 frames, 20 × 200 pixels: over the budget in a part of 30 rows, under
    // it in a short last part that the picture crosses into.
    let frame = Frame {
        image: image::RgbaImage::from_pixel(20, 200, image::Rgba([255, 0, 0, 255])),
        gap: Duration::from_millis(100),
    };
    let animation = Animation {
        frames: vec![frame; 60],
        plays: Plays::Forever,
        over_budget: None,
    };
    let text: String = (0..13).map(|n| format!("Paragraph {n}\n\n")).collect();
    let document = crate::typeset::parse::parse(&format!("{text}![long](long.gif)\n"));
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    inputs.images.insert(
        "long.gif",
        Ok(Source::Animated(std::sync::Arc::new(animation))),
    );
    let mut reads = idle(14);
    reads.push(b"q");
    let (out, _) = played_in(40, (document, inputs), Animations::Always, &reads);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 0, "no frame is sent");
    assert!(controls(&out).is_empty(), "{out:?}");
}
