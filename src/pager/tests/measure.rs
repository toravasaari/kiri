//! A measurement and no test: what the beach ball of `examples/S06a-images.md`
//! sends to the terminal when the reader starts it in reading mode, for the
//! work on B25. Run it by hand, before and after a change:
//!
//! ```sh
//! cargo test --lib the_bytes_the_beach_ball_sends -- --ignored --nocapture
//! ```
//!
//! The bytes are those of the graphics sequences as written: the image data
//! compressed and in base64, as the terminal gets it. The part sent after
//! the wait is the part as it stood before the click, which the waiting
//! steps replaced, sent again under the picture's inset.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use super::*;
use crate::session::animation::Pacing;
use crate::typeset::images::Source;
use crate::typeset::images::animation::{Decoded, FRAME_BUDGET, decode_animation};

const BALL: &[u8] = include_bytes!("../../../tests/fixtures/images/beachball-apng.png");

/// A click on the first row of the window, where the ball is.
const CLICK: &[u8] = b"\x1b[<0;3;1M\x1b[<0;3;1m";

/// One graphics command in the output: what it does, to which image, how
/// large the image is (`a=T` and `a=f`), and how many bytes it takes with
/// its chunks.
struct Sent {
    action: String,
    id: String,
    size: Option<(u32, u32)>,
    bytes: usize,
}

/// The commands that put pixels on the terminal in `out`, in order.
fn sent(out: &str) -> Vec<Sent> {
    let mut all: Vec<Sent> = Vec::new();
    for sequence in out.split("\x1b_G").skip(1) {
        let body = sequence.split("\x1b\\").next().unwrap_or("");
        let bytes = 3 + body.len() + 2;
        let keys = body.split(';').next().unwrap_or("");
        let value =
            |key: &str| (keys.split(',')).find_map(|k| k.strip_prefix(key)?.strip_prefix('='));
        // A chunk after the first has no id.
        let (Some(id), Some(action)) = (value("i"), value("a")) else {
            if let Some(last) = all.last_mut().filter(|_| keys.contains("m=")) {
                last.bytes += bytes;
            }
            continue;
        };
        if !["T", "f"].contains(&action) {
            continue;
        }
        let size = value("s")
            .zip(value("v"))
            .map(|(s, v)| (s.parse().unwrap(), v.parse().unwrap()));
        all.push(Sent {
            action: action.to_owned(),
            id: id.to_owned(),
            size,
            bytes,
        });
    }
    all
}

/// The commands sent after the click, `later`, split: the steps of the
/// wait, the part sent after it (one that `first`, the first draw, sent
/// too), and the animation.
fn split<'a>(first: &[Sent], later: &'a [Sent]) -> (&'a [Sent], Option<&'a Sent>, &'a [Sent]) {
    // The animation is the last image sent with frames after it, and the
    // frames; what is sent before it came with the waiting.
    let at = (later.iter().rposition(|s| s.action == "f")).map(|last| {
        (0..=last)
            .rev()
            .find(|n| later[*n].action == "T")
            .expect("an image the frames belong to")
    });
    let (waiting, animation) = match at {
        Some(at) => (&later[..at], &later[at..]),
        None => (later, &later[later.len()..]),
    };
    // The part as it stood before the click, sent again once the wait
    // is over, under the picture's inset: not a step of the wait.
    let (waiting, after) = match waiting.split_last() {
        Some((last, steps)) if first.iter().any(|s| s.id == last.id) => (steps, Some(last)),
        _ => (waiting, None),
    };
    (waiting, after, animation)
}

fn kib(bytes: usize) -> String {
    format!("{:.1} KiB", bytes as f64 / 1024.0)
}

#[test]
#[ignore = "a measurement to run by hand, see the module's comment"]
fn the_bytes_the_beach_ball_sends_when_it_starts() {
    let Ok(Decoded::Animated(ball)) = decode_animation(BALL, FRAME_BUDGET) else {
        panic!("the beach ball is an animation");
    };
    let frames = ball.frames.len();
    let ball = Source::Animated(std::sync::Arc::new(ball));
    // The ball, and paragraphs under it that fill the first part of the page.
    let mut source = String::from("![Beach ball](beachball.png)\n\n");
    for line in 0..40 {
        source.push_str(&format!("Paragraph {line}.\n\n"));
    }
    for cell in [(10u16, 20u16), (16, 32)] {
        let document = crate::typeset::parse::parse(&source);
        let mut inputs = Inputs::new(Capabilities::kitty(123));
        inputs.caps.cell_pixels = Some(cell);
        inputs.reading = Some(super::reading::reading());
        inputs.images.insert("beachball.png", Ok(ball.clone()));
        let mut reads: Vec<&[u8]> = vec![b"", CLICK];
        reads.extend(std::iter::repeat_n(b"".as_slice(), 80));
        reads.push(b"q");
        let mut terminal = Fake::new(123, 40, &reads);
        // The real time at each read, to tell how long the frames took to build:
        // they are built where they are started, between the click's read and
        // the next.
        let stamps: Rc<RefCell<Vec<Instant>>> = Rc::default();
        let seen = Rc::clone(&stamps);
        terminal.hook = Some(Box::new(move |_| seen.borrow_mut().push(Instant::now())));
        run_document(
            &document,
            &mut inputs,
            Options {
                // As the program has it: the waiting mark shows for its least
                // time even when the frames are ready.
                animation: Pacing::new(3),
                ..Options::new(SearchCase::Smart)
            },
            &mut terminal,
        );
        let out = String::from_utf8(terminal.out.clone()).unwrap();
        let (before, after) = out.split_at(terminal.marks[1].0);
        let first = sent(before);
        let later = sent(after);
        let stamps = stamps.borrow();
        let took = stamps[2].duration_since(stamps[1]);
        let (waiting, after, animation) = split(&first, &later);
        let size = |s: &Sent| s.size.map_or(String::new(), |(w, h)| format!("{w}x{h}"));
        println!("\ncells {cell:?}, window 123 x 40, {frames} frames in the ball");
        for s in &first {
            println!(
                "  the first draw: {} {} {} {}",
                s.action,
                s.id,
                size(s),
                kib(s.bytes)
            );
        }
        println!(
            "  waiting steps after the click: {}, {}",
            waiting.len(),
            kib(waiting.iter().map(|s| s.bytes).sum())
        );
        if let Some(step) = waiting.first() {
            println!("    each is an image of {}", size(step));
        }
        match after {
            Some(s) => println!(
                "  the part sent after the wait: {} {} {}",
                s.id,
                size(s),
                kib(s.bytes)
            ),
            None => println!("  the part sent after the wait: none"),
        }
        for s in animation.iter().take(2) {
            println!(
                "  the animation: {} {} {} {}",
                s.action,
                s.id,
                size(s),
                kib(s.bytes)
            );
        }
        let rest = animation.iter().filter(|s| s.action == "f");
        println!(
            "  the animation: first frame {}, {} more frames {}",
            kib(animation.first().map_or(0, |s| s.bytes)),
            rest.clone().count(),
            kib(rest.map(|s| s.bytes).sum())
        );
        println!(
            "  all sent after the click: {}",
            kib(later.iter().map(|s| s.bytes).sum())
        );
        println!("  real time from the click's read to the next: {took:?}");
    }
}
