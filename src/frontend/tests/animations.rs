//! Animated images through the route: what the terminal is told follows
//! from who plays, and from the reader starting an animation.

use std::time::Duration;

use super::{paged, pictured};
use crate::frontend::sim::Sim;
use crate::images::Source;
use crate::session::settings::InitialLoad;
use crate::typeset::caps::{Capabilities, Graphics, KittyGraphics, Playback};
use crate::typeset::images::animation::{Animation, Frame, Plays};

/// A clip of three frames of a second each.
fn clip() -> Source {
    let frames = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
        .into_iter()
        .map(|rgba| Frame {
            image: image::RgbaImage::from_pixel(20, 20, image::Rgba(rgba)),
            gap: Duration::from_secs(1),
        })
        .collect();
    Source::Animated(std::sync::Arc::new(Animation {
        frames,
        plays: Plays::Forever,
        over_budget: None,
    }))
}

/// The pager on `caps` for a document with the animation `anim.gif`, until
/// it quits at 4 s; with `start`, a click on the animation at 1 s starts it.
fn played(caps: Capabilities, start: bool) -> String {
    let sim = Sim::new(40, 12)
        .caps(caps)
        .file("doc.md", &pictured("anim.gif"))
        .loads("anim.gif", Duration::ZERO, Ok(clip()));
    let sim = paged(sim, InitialLoad::Wait);
    // The image is on the third row, under the text and a blank row.
    let click = "\x1b[<0;3;3M\x1b[<0;3;3m";
    let sim = if start { sim.keys(1000, click) } else { sim };
    let ran = sim.keys(4000, "q").kiri(&["doc.md"]).quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    ran.frames().concat()
}

fn ghostty() -> Capabilities {
    let mut caps = Capabilities::kitty(40);
    caps.graphics = Graphics::Kitty(KittyGraphics {
        max_image_cells: crate::present::kitty::graphics::MAX_CELLS,
        playback: Playback::Redraw,
        terminal: Some("ghostty 1.1.3".into()),
    });
    caps
}

#[test]
fn kitty_gets_every_frame_and_nothing_plays_until_the_reader_starts_it() {
    let out = played(Capabilities::kitty(40), false);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 0, "{out:?}");
    assert!(!out.contains(",s=3"), "{out:?}");
    let out = played(Capabilities::kitty(40), true);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    // The file loops forever and the setting says 3 plays: a loop count of 4.
    assert_eq!(out.matches(",c=1,s=3,v=4\x1b\\").count(), 1, "{out:?}");
}

#[test]
fn ghostty_shows_the_first_frame_until_the_reader_starts_it_and_then_the_frames() {
    let still = played(ghostty(), false);
    assert_eq!(still.matches("\x1b_Ga=T,").count(), 1, "{still:?}");
    let out = played(ghostty(), true);
    assert!(out.matches("\x1b_Ga=T,").count() >= 3, "{out:?}");
    assert!(!out.contains("\x1b_Ga=a,") && !out.contains("\x1b_Ga=f,"));
}

#[test]
fn an_animation_that_plays_is_not_warned_of() {
    let mut caps = Capabilities::kitty(60);
    caps.cell_pixels = Some((10, 20));
    let sim = Sim::new(60, 12)
        .caps(caps)
        .reading_by_default()
        .file("doc.md", "![only](a.gif)\n")
        .loads("a.gif", Duration::ZERO, Ok(clip()));
    let ran = paged(sim, InitialLoad::Wait)
        .keys(1000, "\tp")
        .keys(2500, "q")
        .kiri(&["doc.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    // No frame has the warning mark or the note, and the animation was
    // started by the reader: it plays in reading mode.
    let frames = ran.frames();
    assert!(
        frames.iter().all(|frame| !frame.contains('⚠')),
        "{frames:?}"
    );
    assert!(
        frames
            .iter()
            .all(|frame| !frame.contains("only the first frame shows")),
        "{frames:?}"
    );
    let out = frames.concat();
    assert!(
        out.contains("\x1b_Ga=f,") && out.contains(",s=3,"),
        "{out:?}"
    );
}

/// 60 frames of 20 × 200 pixels: over the budget in a part of the page at 60
/// columns, within it at 30.
fn long_clip() -> Source {
    let frame = Frame {
        image: image::RgbaImage::from_pixel(20, 200, image::Rgba([255, 0, 0, 255])),
        gap: Duration::from_millis(100),
    };
    Source::Animated(std::sync::Arc::new(Animation {
        frames: vec![frame; 60],
        plays: Plays::Forever,
        over_budget: None,
    }))
}

/// The long animation in reading mode at 60 columns, the pager open until
/// 2.5 s, with `more` done to the run.
fn long_run(more: impl FnOnce(Sim) -> Sim) -> crate::frontend::sim::Ran {
    let mut caps = Capabilities::kitty(60);
    caps.cell_pixels = Some((10, 20));
    let text: String = (0..13).map(|n| format!("Paragraph {n}\n\n")).collect();
    let sim = Sim::new(60, 12)
        .caps(caps)
        .reading_by_default()
        .file("doc.md", &format!("{text}![long](a.gif)\n"))
        .loads("a.gif", Duration::ZERO, Ok(long_clip()));
    let sim = paged(sim, InitialLoad::Wait).keys(2500, "q");
    more(sim).kiri(&["doc.md"])
}

#[test]
fn a_picture_left_still_is_told_when_the_pager_quits_as_it_was_last_laid_out() {
    let ran = long_run(|sim| sim);
    assert!(ran.err.contains("its frames would take"), "{:?}", ran.err);
}

#[test]
fn turning_reading_mode_off_takes_the_note_back() {
    // Text layout plays every animation: nothing is told when it quits.
    let ran = long_run(|sim| sim.keys(1000, "M")).quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
}

#[test]
fn a_resize_after_which_the_picture_fits_takes_the_note_back() {
    let ran = long_run(|sim| sim.resize(1000, 30, 12)).quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
}
