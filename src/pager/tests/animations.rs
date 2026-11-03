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
    Source::Animated(std::sync::Arc::new(Animation { frames, plays }))
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
fn played(
    (document, mut inputs): (Document, Inputs),
    setting: Animations,
    reads: &[&[u8]],
) -> (String, Fake) {
    let mut terminal = Fake::new(60, 6, reads);
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

#[test]
fn the_start_waits_the_start_delay_and_no_poll_longer() {
    let mut reads = idle(30);
    reads.push(b"q");
    let (out, terminal) = played(animated(0, Plays::Forever), Animations::Once, &reads);
    // The reads that wait until it is time: 100 ms polls, the last one
    // for what is left of the delay.
    let waits: Vec<_> = terminal.waits.iter().take(9).collect();
    let before: Duration = waits
        .iter()
        .take_while(|_| true)
        .map(|wait| **wait)
        .take(8)
        .sum();
    assert!(before <= crate::session::animation::START_DELAY + Duration::from_millis(100));
    assert_eq!(controls(&out), ["s=3,v=2"], "{out:?}");
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

#[test]
fn in_reading_mode_an_animation_is_still_its_first_frame() {
    // Reading mode draws a picture into the page's own image, so there is
    // nothing of it for the terminal to play.
    let (document, mut inputs) = animated(0, Plays::Forever);
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.reading = Some(super::reading::reading());
    let mut reads = idle(12);
    reads.push(b"q");
    let (out, _) = played((document, inputs), Animations::Always, &reads);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 0, "{out:?}");
    assert_eq!(controls(&out), Vec::<&str>::new(), "{out:?}");
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
