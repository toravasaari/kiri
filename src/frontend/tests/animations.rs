//! Animated images through the route: the setting reaches the pager, and
//! what the terminal is told follows from it and from who plays.

use std::time::Duration;

use super::{paged, pictured};
use crate::frontend::sim::Sim;
use crate::images::Source;
use crate::session::settings::{Animations, InitialLoad};
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

/// The pager on `caps` for a document with the animation `anim.gif`, which
/// plays as `setting` says, until it quits at 4 s.
fn played(caps: Capabilities, setting: Animations) -> String {
    let sim = Sim::new(40, 12)
        .caps(caps)
        .file("doc.md", &pictured("anim.gif"))
        .loads("anim.gif", Duration::ZERO, Ok(clip()))
        .settings(move |settings| settings.animations = setting);
    let ran = paged(sim, InitialLoad::Wait)
        .keys(4000, "q")
        .kiri(&["doc.md"])
        .quiet();
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
fn the_setting_makes_the_pager_play_an_animation_in_kitty() {
    let out = played(Capabilities::kitty(40), Animations::Always);
    assert_eq!(out.matches("\x1b_Ga=f,").count(), 2, "{out:?}");
    assert_eq!(out.matches(",s=3,v=1\x1b\\").count(), 1, "{out:?}");
    let once = played(Capabilities::kitty(40), Animations::Once);
    assert_eq!(once.matches(",s=3,v=2\x1b\\").count(), 1, "{once:?}");
}

#[test]
fn with_animations_never_kitty_gets_the_first_frame_only() {
    let out = played(Capabilities::kitty(40), Animations::Never);
    assert_eq!(out.matches("\x1b_Ga=T,").count(), 1, "{out:?}");
    assert!(!out.contains("\x1b_Ga=f,") && !out.contains("\x1b_Ga=a,"));
}

#[test]
fn the_setting_makes_the_pager_draw_the_frames_in_ghostty() {
    let out = played(ghostty(), Animations::Always);
    assert!(out.matches("\x1b_Ga=T,").count() >= 3, "{out:?}");
    assert!(!out.contains("\x1b_Ga=a,") && !out.contains("\x1b_Ga=f,"));
}

/// A reading-mode document of two animations in one part of the page, so that
/// the second stays on its first frame, with the loads of both.
fn two_in_one_part(sim: Sim) -> Sim {
    let source = "![first](a.gif)\n\n![second](b.gif)\n";
    sim.reading_by_default()
        .file("doc.md", source)
        .loads("a.gif", Duration::ZERO, Ok(clip()))
        .loads("b.gif", Duration::ZERO, Ok(clip()))
        .settings(|settings| settings.animations = Animations::Always)
}

const SHARED: &str =
    "b.gif: another animation is first in a part of the page it is in: only its first frame shows";

#[test]
fn printing_tells_an_animation_left_on_its_first_frame_once() {
    let mut caps = Capabilities::kitty(60);
    caps.cell_pixels = Some((10, 20));
    let sim = two_in_one_part(Sim::new(60, 12).caps(caps)).asks_animations();
    let ran = sim
        .settings(|settings| settings.paging = crate::session::settings::Paging::Never)
        .kiri(&["doc.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let told = ran
        .err
        .lines()
        .filter(|line| line.ends_with(SHARED))
        .count();
    assert_eq!(told, 1, "{:?}", ran.err);
}

#[test]
fn the_pager_shows_it_and_tells_it_when_it_quits() {
    let mut caps = Capabilities::kitty(60);
    caps.cell_pixels = Some((10, 20));
    let ran = paged(
        two_in_one_part(Sim::new(60, 12).caps(caps)),
        InitialLoad::Wait,
    )
    .keys(1500, "q")
    .kiri(&["doc.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    // In the warning place while it runs, and on standard error after.
    assert!(
        ran.texts().iter().any(|text| text.contains("b.gif")),
        "{:?}",
        ran.texts()
    );
    assert_eq!(
        ran.err
            .lines()
            .filter(|line| line.ends_with(SHARED))
            .count(),
        1,
        "{:?}",
        ran.err
    );
}

#[test]
fn an_animation_that_plays_is_not_warned_of() {
    let mut caps = Capabilities::kitty(60);
    caps.cell_pixels = Some((10, 20));
    let sim = Sim::new(60, 12)
        .caps(caps)
        .reading_by_default()
        .file("doc.md", "![only](a.gif)\n")
        .loads("a.gif", Duration::ZERO, Ok(clip()))
        .settings(|settings| settings.animations = Animations::Always);
    let ran = paged(sim, InitialLoad::Wait)
        .keys(1500, "q")
        .kiri(&["doc.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
}
