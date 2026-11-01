//! The route's loads of images in the simulated frontend, as they run in
//! the background while the pager shows, and what the files of a run get of
//! them: each test here has a twin among the pty tests of
//! `tests/it/cli_pager.rs` that runs a real program for a diagram, and
//! scripts a picture's load where that one draws. The wait that the
//! reader can skip is in `wait.rs`.

use super::{missing, on_kitty, paged, pictured, red};
use crate::frontend::sim::{Ran, Sim};
use crate::session::settings::{Follow, FollowJump, InitialLoad};
use crate::typeset::caps::Capabilities;

/// Whether the frame `frame` has the picture sent, and not only the band
/// that stands for it.
fn sent(frame: &str) -> bool {
    frame.contains("\x1b_Ga=T,")
}

/// Asserts that `ran` ended well, showed the text and the band of the
/// loading first and the picture after, with `loading` in the band.
#[track_caller]
fn text_first(ran: &Ran, loading: &str) {
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let (frames, texts) = (ran.frames(), ran.texts());
    assert!(texts[0].contains("Above"), "{texts:?}");
    assert!(texts[0].contains(loading), "{texts:?}");
    // The band that stands for the picture is sent, and not the picture.
    assert!(frames[0].contains("\x1b_Ga=t,"), "{frames:?}");
    assert!(!sent(frames[0]), "{frames:?}");
    let at = frames.iter().position(|frame| sent(frame));
    let drawn = &texts[at.expect("the picture is sent")];
    assert!(!drawn.contains("[rendering"), "{texts:?}");
    assert!(drawn.contains(" doc.md "), "{texts:?}");
}

// Twin of an_initial_load_in_the_background_shows_the_text_before_the_diagram
#[test]
fn an_initial_load_in_the_background_shows_the_text_before_the_image() {
    // `-p` and `-f` with `--initial-load background`; `KIRI_INITIAL_LOAD`
    // is the same setting.
    let paging = |sim: Sim| paged(sim, InitialLoad::Background);
    let following = |sim: Sim| {
        (sim.follow(Follow::On, FollowJump::First))
            .settings(|settings| settings.initial_load = InitialLoad::Background)
    };
    for (name, how) in [("-p", &paging as &dyn Fn(Sim) -> Sim), ("-f", &following)] {
        let sim = red(
            on_kitty(12).file("doc.md", &pictured("a.png")),
            "a.png",
            300,
        );
        let ran = how(sim).keys(2000, "q").kiri(&["doc.md"]).quiet();
        text_first(&ran, "[rendering: 0/1]");
        assert!(ran.err.is_empty(), "{name}");
    }
}

// Twin of an_initial_load_from_standard_input_goes_in_the_background_with_p
#[test]
fn an_initial_load_from_standard_input_goes_in_the_background_with_p() {
    let sim = paged(on_kitty(12), InitialLoad::Background).stdin(&pictured("a.png"));
    let ran = red(sim, "a.png", 300).keys(2000, "q").kiri(&["-"]).quiet();
    let frames = ran.frames();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.texts()[0].contains("[rendering: 0/1]"), "{frames:?}");
    assert!(!sent(frames[0]), "{frames:?}");
    let drawn = frames.iter().find(|frame| sent(frame));
    assert!(!crate::frontend::sim::plain(drawn.unwrap()).contains("[rendering"));
}

// Twin of an_initial_load_in_the_background_with_nothing_to_load_shows_as_waiting
#[test]
fn an_initial_load_in_the_background_with_nothing_to_load_shows_as_waiting() {
    // Images off, a terminal that shows none, and a document without any.
    type Case = (fn() -> Sim, fn(Sim) -> Sim);
    let cases: [Case; 3] = [
        (|| on_kitty(12), |sim| sim.settings(|s| s.images = false)),
        (|| Sim::new(40, 12).caps(Capabilities::ansi(40)), |sim| sim),
        (|| on_kitty(12), |sim| sim),
    ];
    for (at, (start, change)) in cases.into_iter().enumerate() {
        let shown = |initial_load| {
            let document = match at {
                2 => "Above\n\nBelow\n".to_owned(),
                _ => pictured("a.png"),
            };
            let sim = change(paged(start().file("doc.md", &document), initial_load));
            // No load is scripted: one that started would never come.
            sim.keys(10, "q").kiri(&["doc.md"]).quiet()
        };
        let background = shown(InitialLoad::Background);
        assert!(!background.texts()[0].contains("[rendering"), "{at}");
        assert_eq!(background.frames()[0], shown(InitialLoad::Wait).frames()[0]);
    }
}

// Twin of an_initial_load_in_the_background_tells_a_failed_diagram_after_the_pager
#[test]
fn an_initial_load_in_the_background_tells_a_failed_image_after_the_pager() {
    let sim = paged(on_kitty(12), InitialLoad::Background);
    let ran = missing(sim.file("doc.md", &pictured("a.png")), "a.png", 300)
        .keys(2000, "q")
        .kiri(&["doc.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert!(texts[0].contains("[rendering: 0/1]"), "{texts:?}");
    assert!(!texts.last().unwrap().contains("[rendering"), "{texts:?}");
    // Where the pager's screen would hide it, it is told once the screen
    // is left.
    assert_eq!(ran.told_before_the_pager_left(), "");
    assert_eq!(
        ran.told_after_the_pager_left(),
        "kiri: a.png: file not found\n"
    );
    // Waiting, it is told before the pager opens.
    let sim = paged(on_kitty(12), InitialLoad::Wait);
    let ran = missing(sim.file("doc.md", &pictured("a.png")), "a.png", 300)
        .keys(2000, "q")
        .kiri(&["doc.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(!ran.texts()[0].contains("[rendering"), "{:?}", ran.texts());
    assert_eq!(
        ran.told_before_the_pager_left(),
        "kiri: a.png: file not found\n"
    );
    assert_eq!(ran.told_after_the_pager_left(), "");
}

/// `a.md` with the picture `a.png` and `missing.md` with `missing.png`,
/// which is not there.
fn two_files(sim: Sim) -> Sim {
    let sim = sim.file("a.md", &pictured("a.png"));
    sim.file("missing.md", "![gone](missing.png)\n")
}

// Twin of several_files_load_their_images_in_the_background_too
#[test]
fn several_files_load_their_images_in_the_background_too() {
    let told = "kiri: missing.md: missing.png: file not found\n";
    let sim = missing(
        red(
            two_files(paged(on_kitty(12), InitialLoad::Background)),
            "a.png",
            300,
        ),
        "missing.png",
        100,
    );
    let ran = sim.keys(2000, "q").kiri(&["a.md", "missing.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let (frames, texts) = (ran.frames(), ran.texts());
    assert!(texts[0].contains("Above"), "{texts:?}");
    assert!(texts[0].contains("[rendering: 0/2]"), "{texts:?}");
    assert!(!sent(frames[0]), "{frames:?}");
    assert!(frames.iter().any(|frame| sent(frame)), "{frames:?}");
    // Under the file's name, as waiting tells it before the pager opens.
    assert_eq!(ran.told_after_the_pager_left(), told);
    let sim = missing(
        red(
            two_files(paged(on_kitty(12), InitialLoad::Wait)),
            "a.png",
            300,
        ),
        "missing.png",
        100,
    );
    let waiting = sim.keys(2000, "q").kiri(&["a.md", "missing.md"]);
    assert_eq!(waiting.told_before_the_pager_left(), told);
    assert_eq!(waiting.err, ran.err);
}

// Twin of several_files_without_a_terminal_for_keys_print_as_when_waiting
#[test]
fn several_files_without_a_terminal_for_keys_print_as_when_waiting() {
    let shown = |initial_load| {
        let sim = missing(
            red(two_files(paged(on_kitty(12), initial_load)), "a.png", 300),
            "missing.png",
            100,
        );
        let ran = sim.no_keys().kiri(&["a.md", "missing.md"]);
        assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
        ran
    };
    let background = shown(InitialLoad::Background);
    assert!(background.frames().is_empty(), "{:?}", background.out);
    assert_eq!(
        background.out.matches("\x1b_Ga=T,").count(),
        1,
        "{:?}",
        background.out
    );
    assert!(
        background
            .err
            .contains("kiri: missing.md: missing.png: file not found\n"),
        "{}",
        background.err
    );
    let waiting = shown(InitialLoad::Wait);
    assert_eq!((background.out, background.err), (waiting.out, waiting.err));
}

/// A document of `top` lines in one paragraph, a picture and three
/// paragraphs of a line each: with the picture counted at three rows, it
/// takes `top` + 10 rows.
fn counted(top: usize, url: &str) -> String {
    let top: Vec<String> = (1..=top).map(|n| format!("Top {n}")).collect();
    let rest = "Line 1\n\nLine 2\n\nLine 3\n";
    format!("{}\n\n![a]({url})\n\n{rest}", top.join("  \n"))
}

// Twin of paging_on_auto_loads_in_the_background_what_is_longer_than_the_window_with_three_rows_an_image
#[test]
fn paging_on_auto_loads_in_the_background_what_is_longer_than_the_window_with_three_rows_an_image()
{
    let background = |sim: Sim| {
        (sim.settings(|settings| settings.initial_load = InitialLoad::Background)).keys(2000, "q")
    };
    // 13 rows in a window of 12: the pager opens at once.
    let sim = red(
        on_kitty(12).file("counted.md", &counted(3, "a.png")),
        "a.png",
        300,
    );
    let ran = background(sim).kiri(&["counted.md"]).quiet();
    assert!(
        !ran.frames().is_empty(),
        "printed, not paged: {:?}",
        ran.out
    );
    assert!(
        ran.texts()[0].contains("[rendering: 0/1]"),
        "{:?}",
        ran.texts()
    );
    assert!(ran.texts()[0].contains("Line 1"), "{:?}", ran.texts());
    // 12 rows: it waits, and then, the picture being shorter, prints.
    let sim = red(
        on_kitty(12).file("counted.md", &counted(2, "a.png")),
        "a.png",
        300,
    );
    let ran = background(sim).kiri(&["counted.md"]).quiet();
    assert!(ran.frames().is_empty(), "paged: {:?}", ran.out);
    assert_eq!(ran.out.matches("\x1b_Ga=T,").count(), 1, "{:?}", ran.out);
    // Several files, longer than the window as their images come.
    let sim = on_kitty(12).file("a.md", &pictured("a.png"));
    let sim = sim.file("counted.md", &counted(3, "b.png"));
    let sim = red(red(sim, "a.png", 300), "b.png", 300);
    let ran = background(sim).kiri(&["a.md", "counted.md"]).quiet();
    assert!(
        !ran.frames().is_empty(),
        "printed, not paged: {:?}",
        ran.out
    );
    assert!(
        ran.texts()[0].contains("[rendering: 0/2]"),
        "{:?}",
        ran.texts()
    );
}
