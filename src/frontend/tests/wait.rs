//! The wait for the images before the pager opens, in the simulated
//! frontend: the progress line and its hint, Space and Enter that open
//! the pager before the images have come, and what the wait does without a
//! standard error or a terminal for keys. Each test has a twin among the
//! pty tests of `tests/it/cli_pager.rs`, which stay for Ctrl-C and for a
//! program that runs.

use super::{long, missing, on_kitty, paged, pictured, red};
use crate::frontend::Failure;
use crate::frontend::sim::{Raise, Ran};
use crate::session::settings::{InitialLoad, Paging};
use crate::term::progress::HINT;

/// The picture `a.png` comes after a second: the reader has time to type.
const AFTER: u64 = 1000;

fn sent(frame: &str) -> bool {
    frame.contains("\x1b_Ga=T,")
}

/// Asserts that the pager opened at once, with the picture still loading,
/// and the picture came later.
#[track_caller]
fn opened_before_the_picture(ran: &Ran) {
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let (frames, texts) = (ran.frames(), ran.texts());
    assert!(texts[0].contains("Above"), "{texts:?}");
    assert!(texts[0].contains("[rendering: 0/1]"), "{texts:?}");
    assert!(!sent(frames[0]), "{frames:?}");
    assert!(frames.iter().any(|frame| sent(frame)), "{frames:?}");
}

// Twin of space_or_enter_opens_the_pager_before_the_slow_diagram_is_drawn
#[test]
fn space_or_enter_opens_the_pager_before_the_slow_image_comes() {
    for key in [" ", "\r"] {
        let sim = paged(on_kitty(12), InitialLoad::Wait).stderr_terminal();
        let sim = red(sim.file("doc.md", &pictured("a.png")), "a.png", AFTER);
        let ran = sim.keys(200, key).keys(3000, "q").kiri(&["doc.md"]);
        opened_before_the_picture(&ran);
        assert!(ran.err.contains(HINT), "{key:?}: {:?}", ran.err);
        // The line is wiped, and the pager opened before the picture came.
        assert!(ran.err.ends_with("\r\x1b[2K"), "{key:?}: {:?}", ran.err);
    }
}

// Twin of space_opens_the_pager_for_several_files_before_the_slow_diagram_is_drawn
#[test]
fn space_opens_the_pager_for_several_files_before_the_slow_image_comes() {
    let sim = paged(on_kitty(12), InitialLoad::Wait).stderr_terminal();
    let sim = sim.file("a.md", &pictured("a.png")).file("b.md", "Other\n");
    let ran = red(sim, "a.png", AFTER)
        .keys(200, " ")
        .keys(3000, "q")
        .kiri(&["a.md", "b.md"]);
    opened_before_the_picture(&ran);
    assert!(ran.err.contains(HINT), "{:?}", ran.err);
}

// Twin of a_diagram_that_fails_after_a_skipped_wait_is_told_after_the_pager
#[test]
fn an_image_that_fails_after_a_skipped_wait_is_told_after_the_pager() {
    for (files, told) in [
        (&["a.md"][..], "kiri: a.png: file not found\n"),
        (&["a.md", "b.md"], "kiri: a.md: a.png: file not found\n"),
    ] {
        let sim = paged(on_kitty(12), InitialLoad::Wait).stderr_terminal();
        let sim = sim.file("a.md", &pictured("a.png")).file("b.md", "Other\n");
        let ran = missing(sim, "a.png", AFTER)
            .keys(200, " ")
            .keys(3000, "q")
            .kiri(files);
        assert!(matches!(ran.result, Some(Ok(()))), "{files:?}");
        assert!(
            ran.texts()[0].contains("[rendering: 0/1]"),
            "{:?}",
            ran.texts()
        );
        assert!(ran.err.contains(HINT), "{files:?}: {:?}", ran.err);
        // Not while the pager shows, but once it has left its screen.
        assert!(!ran.told_before_the_pager_left().contains("file not found"));
        assert_eq!(ran.told_after_the_pager_left(), told, "{files:?}");
    }
}

// Twin of without_standard_error_on_a_terminal_space_does_not_skip_the_wait
#[test]
fn without_standard_error_on_a_terminal_space_does_not_skip_the_wait() {
    // Space while the picture loads, which nothing then shows.
    let sim = paged(on_kitty(12), InitialLoad::Wait).keys(200, " ");
    let ran = red(sim.file("doc.md", &pictured("a.png")), "a.png", AFTER)
        .keys(3000, "q")
        .kiri(&["doc.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let frames = ran.frames();
    // Waited for the picture.
    assert!(sent(frames[0]), "{frames:?}");
    assert!(!ran.texts()[0].contains("[rendering"), "{:?}", ran.texts());
}

// Twin of another_key_does_nothing_while_the_images_load_and_is_not_kept
#[test]
fn another_key_does_nothing_while_the_images_load_and_is_not_kept() {
    // To the end, were it kept for the pager.
    let sim = paged(on_kitty(12), InitialLoad::Wait).stderr_terminal();
    let document = format!("![a](a.png)\n\n{}", long());
    let ran = red(sim.file("doc.md", &document), "a.png", AFTER)
        .keys(200, "G")
        .keys(3000, "q")
        .kiri(&["doc.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.err.contains(HINT), "no hint: {:?}", ran.err);
    let frames = ran.frames();
    // Waited for the picture, at the top.
    assert!(sent(frames[0]), "{frames:?}");
    let first = &ran.texts()[0];
    assert!(!first.contains("[rendering"), "{first}");
    assert!(first.contains("Line 2"), "{first}");
    assert!(!first.contains("Line 10"), "{first}");
}

// Twin of the_hint_shows_only_where_the_pager_surely_opens
#[test]
fn the_hint_shows_only_where_the_pager_surely_opens() {
    // The document, whether it fits a window of 12 rows, and the paging.
    let short = pictured("a.png");
    let longer = format!("![a](a.png)\n\n{}", long());
    for (document, paging, hint) in [
        (&short, Paging::Always, true),
        (&longer, Paging::Auto, true),
        (&short, Paging::Auto, false),
        (&longer, Paging::Never, false),
    ] {
        let sim = on_kitty(12).file("doc.md", document).stderr_terminal();
        let sim = sim.settings(move |settings| settings.paging = paging);
        let ran = red(sim, "a.png", 600).keys(3000, "q").kiri(&["doc.md"]);
        assert!(matches!(ran.result, Some(Ok(()))), "{paging:?}");
        assert_eq!(ran.err.contains(HINT), hint, "{paging:?}: {:?}", ran.err);
        // Where it does not, the images load first, and that line counts
        // only diagrams, which no picture is: nothing shows.
        assert_eq!(ran.err.contains("loading images"), hint, "{paging:?}");
    }
}

/// SIGHUP, caught for real, arriving at the moment `when` says while the
/// reader skips the wait for the picture with Space and quits the pager.
fn sighup(when: Raise) -> Ran {
    let sim = paged(on_kitty(12), InitialLoad::Wait).stderr_terminal();
    let sim = red(sim.file("doc.md", &pictured("a.png")), "a.png", AFTER);
    (sim.raising(signal_hook::consts::SIGHUP, when))
        .keys(200, " ")
        .keys(3000, "q")
        .kiri(&["doc.md"])
}

/// Runs the ignored test `name` of this module in a process of its own,
/// as the signals reach the whole test binary, and asserts that it passed.
#[track_caller]
fn passes_alone(name: &str) {
    use std::process::{Command, Stdio};

    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &format!("frontend::tests::wait::{name}")])
        .args(["--ignored", "--test-threads", "1"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("running 1 test"), "{stdout}");
    assert!(output.status.success(), "{:?}: {stdout}", output.status);
}

/// Run on its own by [`a_signal_in_the_read_that_skips_the_wait_is_the_failure`]:
/// SIGHUP arrives in the very read of keys in which the reader presses
/// Space.
#[test]
#[ignore = "run in a process of its own"]
fn sighup_as_the_wait_is_skipped() {
    let ran = sighup(Raise::Skip);
    assert!(
        matches!(ran.result, Some(Err(Failure::Signal(1)))),
        "{:?}",
        ran.result
    );
}

#[test]
fn a_signal_in_the_read_that_skips_the_wait_is_the_failure() {
    passes_alone("sighup_as_the_wait_is_skipped");
}

/// Run on its own by [`a_signal_as_the_wait_opens_its_reader_is_the_failure`]:
/// SIGHUP arrives as the wait opens its reader of keys, which a real
/// terminal sets to read keys without echo.
#[test]
#[ignore = "run in a process of its own"]
fn sighup_as_the_wait_opens_its_reader() {
    let ran = sighup(Raise::Open);
    assert!(
        matches!(ran.result, Some(Err(Failure::Signal(1)))),
        "{:?}",
        ran.result
    );
}

// B19: the signals were caught only after the reader opened, so a signal
// in between ended kiri with the terminal left without echo.
#[test]
fn a_signal_as_the_wait_opens_its_reader_is_the_failure() {
    passes_alone("sighup_as_the_wait_opens_its_reader");
}

/// Run on its own by [`a_signal_before_the_screen_catches_ends_the_pager`]:
/// SIGHUP arrives after the reader skipped the wait, before the pager's
/// screen catches the signals.
#[test]
#[ignore = "run in a process of its own"]
fn sighup_before_the_screen_catches() {
    let ran = sighup(Raise::Screen);
    let Some(Err(Failure::Error(error))) = &ran.result else {
        panic!("{:?}", ran.result);
    };
    assert!(error.contains("signal"), "{error}");
}

// B19: the guard noted the signal and was dropped with it, so the pager
// opened and kiri ended with status 0 after `q`.
#[test]
fn a_signal_before_the_screen_catches_ends_the_pager() {
    passes_alone("sighup_before_the_screen_catches");
}
