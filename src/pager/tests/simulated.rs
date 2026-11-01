//! The pager run in the simulated frontend (`src/frontend/sim.rs`): a virtual
//! clock, files in memory and a script of keys and edits, in place of the
//! pseudo-terminal tests that tested the pager and not the terminal. They
//! look at the bytes the pager writes, as those did.

use super::*;
use crate::frontend::sim::Sim;
use crate::session::settings::Follow;

/// A document longer than a window of 5 rows: `Line 1` to `Line 10`, one
/// row each and a blank row between.
fn long() -> String {
    (1..=10).map(|n| format!("Line {n}\n\n")).collect()
}

/// The text of `long.md` written anew: ten paragraphs of `Fresh`.
fn fresh() -> String {
    (1..=10).map(|n| format!("Fresh {n}\n\n")).collect()
}

/// A window of 40 × 5 showing `long.md`, as `kiri long.md` does in one.
fn on_long() -> Sim {
    Sim::new(40, 5).file("long.md", &long()).show("long.md")
}

#[test]
fn the_script_runs_on_the_virtual_clock_and_the_reader_quits_when_it_ends() {
    // Ten minutes pass before the key, a poll at a time: none is waited
    // for.
    let ten_minutes = 10 * 60 * 1000;
    let ran = on_long().keys(ten_minutes, "G").run();
    assert!(
        ran.elapsed >= Duration::from_secs(10 * 60),
        "{:?}",
        ran.elapsed
    );
    // The frame of the start, and the one `G` draws.
    assert_eq!(ran.frames().len(), 2, "{}", ran.out);
    // Every wait is a poll while nothing fades.
    assert!(
        ran.waits.iter().all(|wait| *wait == POLL),
        "{:?}",
        &ran.waits[..3]
    );
}

#[test]
fn keys_scroll_the_document() {
    let ran = on_long().keys(10, "G").keys(20, "q").run();
    let frames = ran.frames();
    assert_eq!(frames.len(), 2, "{}", ran.out);
    assert!(!frames[0].contains("Line 10"), "{}", ran.out);
    assert!(frames[1].contains("Line 10"), "{}", ran.out);
}

#[test]
fn r_reads_the_file_again() {
    let ran = on_long()
        .write(5, "long.md", &fresh())
        .keys(10, "r")
        .keys(500, "q")
        .run();
    assert_eq!(ran.reloads, 1);
    // The first frame, then the one that says it is reloading, then the
    // new document's.
    let texts = ran.texts();
    assert!(texts[0].contains("Line 1"), "{texts:?}");
    assert!(texts[1].contains("reloading"), "{texts:?}");
    let last = texts.last().unwrap();
    assert!(last.contains("Fresh 1"), "{texts:?}");
    assert!(last.contains("reloaded"), "{texts:?}");
}

#[test]
fn a_file_that_is_gone_stays_as_it_was_and_the_status_line_says_why() {
    let ran = on_long()
        .remove(5, "long.md")
        .keys(10, "r")
        .keys(500, "q")
        .run();
    let texts = ran.texts();
    let last = texts.last().unwrap();
    assert!(last.contains("Line 1"), "{texts:?}");
    assert!(last.contains("No such file"), "{texts:?}");
    assert!(!last.contains("reloaded"), "{texts:?}");
}

/// A window of 40 × 5 following `long.md` as `follow` says, with
/// `change` made to the disk 10 ms in and no key until 1 s, after which
/// the reader quits. Returns the frames' text.
fn following(follow: Follow, change: impl FnOnce(Sim) -> Sim) -> Vec<String> {
    let sim = on_long().follow(follow, FollowJump::First);
    let ran = change(sim).keys(1000, "q").run();
    let texts = ran.texts();
    assert!(texts[0].contains("Line 1"), "{texts:?}");
    let follows = [" follow ", " auto "];
    assert!(
        texts[0].contains(follows[usize::from(follow == Follow::Auto)]),
        "{texts:?}"
    );
    texts
}

#[test]
fn follow_reads_the_file_again_when_it_changes_without_a_key() {
    let texts = following(Follow::On, |sim| sim.write(10, "long.md", &fresh()));
    let last = texts.last().unwrap();
    assert!(last.contains("Fresh 1"), "{texts:?}");
    assert!(last.contains("reloaded"), "{texts:?}");
    // Without a key, the pager does not say it is reloading.
    assert!(
        !texts.iter().any(|frame| frame.contains("reloading")),
        "{texts:?}"
    );
}

#[test]
fn auto_follow_moves_the_view_to_a_change_in_the_file() {
    let text = long().replace("Line 9\n", "Line 9 changed\n");
    let texts = following(Follow::Auto, |sim| sim.write(10, "long.md", &text));
    let last = texts.last().unwrap();
    assert!(last.contains("Line 9 changed"), "{texts:?}");
    assert!(!last.contains("Line 1 "), "{texts:?}");
    assert!(last.contains(" auto "), "{texts:?}");
}

#[test]
fn follow_reads_a_file_replaced_by_a_rename() {
    // As editors save: a new file written beside it and moved over it.
    let texts = following(Follow::On, |sim| {
        sim.file(".long.md.swp", &fresh())
            .rename(10, ".long.md.swp", "long.md")
    });
    let last = texts.last().unwrap();
    assert!(last.contains("Fresh 1"), "{texts:?}");
    assert!(last.contains("reloaded"), "{texts:?}");
}

#[test]
fn follow_keeps_a_file_that_is_gone_and_the_status_line_says_why() {
    let texts = following(Follow::On, |sim| sim.remove(10, "long.md"));
    let last = texts.last().unwrap();
    assert!(last.contains("Line 1"), "{texts:?}");
    assert!(last.contains("No such file"), "{texts:?}");
}

#[test]
fn follow_waits_for_a_change_to_look_the_same_twice_a_poll_apart() {
    // Written at 10 ms: seen on the poll at 100 ms, and read again on the
    // one at 200 ms, when it still looks the same.
    let follow = |quit| {
        let ran = on_long()
            .follow(Follow::On, FollowJump::First)
            .write(10, "long.md", &fresh())
            .keys(quit, "q")
            .run();
        ran.reloads
    };
    assert_eq!(follow(150), 0);
    assert_eq!(follow(250), 1);
}

#[test]
fn a_window_resized_while_the_pager_runs_shows_the_document_in_its_rows() {
    let ran = on_long().resize(10, 40, 9).keys(500, "q").run();
    let texts = ran.texts();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(!texts[0].contains("Line 4"), "{texts:?}");
    assert!(texts[1].contains("Line 4"), "{texts:?}");
}

#[test]
fn an_image_a_reload_brings_comes_when_the_script_says() {
    // The wait is also the clock: the image comes 300 ms after the
    // reload, and the pager draws a frame of its band each 50 ms until.
    let red = crate::images::Source::Raster(image::RgbaImage::from_pixel(
        20,
        20,
        image::Rgba([255, 0, 0, 255]),
    ));
    let text = "Above\n\n![a square](square.png)\n\nBelow\n";
    let ran = Sim::new(60, 12)
        .caps(Capabilities::kitty(60))
        .file("doc.md", text)
        .show("doc.md")
        .reload_brings("square.png", Duration::from_millis(300), Ok(red))
        .keys(10, "r")
        .keys(2000, "q")
        .run();
    assert_eq!(ran.reloads, 1);
    assert!(ran.waits.contains(&FRAME), "{:?}", ran.waits);
    assert!(ran.elapsed >= Duration::from_millis(310));
    // The picture is sent once it has come, and not before: the frames
    // before it have the band that stands for it, which is another kind
    // of image.
    let (before, after) = ran
        .out
        .split_at(ran.out.find("\x1b_Ga=T").expect("the picture is sent"));
    assert_eq!(before.matches("\x1b_Ga=T").count(), 0);
    assert!(
        before.contains("\x1b_Ga=t"),
        "the band shows while it loads"
    );
    assert_eq!(after.matches("\x1b_Ga=T").count(), 1, "sent once");
    // With nothing to load and nothing fading the pager polls again.
    assert_eq!(ran.waits.last(), Some(&POLL), "{:?}", ran.waits);
}

/// A window of 80 × 10 on kitty, with cells of 10 × 20 pixels, in text
/// first with reading mode for `M`, showing `doc.md`: a line with a badge
/// in it, which loads when `M` turns reading mode on.
fn badge() -> Sim {
    let caps = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    };
    let quadrants = image::RgbaImage::from_fn(8, 8, |x, y| {
        image::Rgba(match (x < 4, y < 4) {
            (true, true) => [255, 0, 0, 255],
            (false, true) => [0, 255, 0, 255],
            (true, false) => [0, 0, 255, 255],
            (false, false) => [255, 255, 0, 255],
        })
    });
    let text = "A line with a badge <img src=\"p.png\" width=\"24\" alt=\"badge\"> in it.\n";
    Sim::new(80, 10)
        .caps(caps)
        .file("doc.md", text)
        .show("doc.md")
        .reading()
        .inline_brings(
            "p.png",
            Duration::from_millis(50),
            crate::images::Source::Raster(quadrants),
        )
}

/// The ids of the pages sent in `out`, in order.
fn pages(out: &str) -> Vec<String> {
    out.split("\x1b_Ga=T,U=1,")
        .skip(1)
        .filter_map(|rest| {
            let id = rest.split(',').find_map(|part| part.strip_prefix("i="))?;
            Some(id.to_owned())
        })
        .collect()
}

#[test]
fn m_loads_the_pictures_in_the_middle_of_text_that_reading_mode_shows() {
    let ran = badge().keys(10, "M").keys(1000, "q").run();
    // The page without the picture when `M` turns reading mode on, and
    // again with it when it has come: sent as two images, as it is two
    // different pages.
    let ids = pages(&ran.out);
    assert_eq!(ids.len(), 2, "the pages sent: {ids:?}");
}

#[test]
fn a_reload_after_m_keeps_the_pictures_m_loaded() {
    let ran = badge().keys(10, "M").keys(500, "r").keys(1000, "q").run();
    assert_eq!(ran.reloads, 1);
    // The page without the picture, then with it; a reload leaves the
    // page with the picture, as it is when `--reading` asked for it.
    let ids = pages(&ran.out);
    assert!(ids.len() >= 2, "{ids:?}");
    assert_ne!(ids[0], ids[1], "{ids:?}");
    assert_eq!(ids.last(), Some(&ids[1]), "{ids:?}");
}

#[test]
fn m_on_again_loads_a_picture_it_holds_no_second_time() {
    // The script has one outcome for the picture: a second load of it
    // would never come, and the page would show without it.
    let ran = badge()
        .keys(10, "M")
        .keys(200, "M")
        .keys(300, "M")
        .keys(1000, "q")
        .run();
    let ids = pages(&ran.out);
    // Without the picture, with it; reading mode off and on again shows
    // those two pages again.
    assert!(ids.len() >= 2, "{ids:?}");
    assert_ne!(ids[0], ids[1], "{ids:?}");
    assert_eq!(ids.last(), Some(&ids[1]), "{ids:?}");
}
