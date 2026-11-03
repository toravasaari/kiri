//! Reading mode as the default: shown wherever the terminal draws images,
//! in the pager and when printing, and the text layout, without a word,
//! where it does not. The warning for a terminal that cannot show reading
//! mode when it was named is in `settings.rs`.

use super::{long, on_kitty, paragraphs};
use crate::frontend::sim::{Ran, Sim, plain};
use crate::session::settings::Paging;
use crate::typeset::caps::Capabilities;
use crate::typeset::input::Input;

/// The start of kitty's graphics sequences. The text layout draws none for
/// a document without pictures, and reading mode draws its text as images.
const GRAPHICS: &str = "\u{1b}_G";

fn ok(ran: &Ran) {
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
}

#[test]
fn the_pager_shows_reading_mode_by_default_on_a_terminal_that_draws_images() {
    let ran = (on_kitty(8).reading_by_default())
        .file("long.md", &long())
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .kiri(&["long.md"]);
    ok(&ran);
    assert!(ran.out.contains(GRAPHICS), "no image in {:?}", ran.out);
    assert_eq!(ran.err, "");
}

#[test]
fn printing_shows_reading_mode_by_default_on_a_terminal_that_draws_images() {
    let ran = (on_kitty(8).reading_by_default())
        .file("long.md", &long())
        .settings(|settings| settings.paging = Paging::Never)
        .kiri(&["long.md"]);
    ok(&ran);
    assert!(ran.frames().is_empty(), "paged: {:?}", ran.out);
    assert!(ran.out.contains(GRAPHICS), "no image in {:?}", ran.out);
    assert_eq!(ran.err, "");
}

#[test]
fn a_short_document_that_auto_paging_prints_is_shown_in_reading_mode_by_default() {
    let ran = (on_kitty(24).reading_by_default())
        .file("short.md", &paragraphs(2))
        .kiri(&["short.md"]);
    ok(&ran);
    assert!(ran.frames().is_empty(), "paged: {:?}", ran.out);
    assert!(ran.out.contains(GRAPHICS), "no image in {:?}", ran.out);
    assert_eq!(ran.err, "");
}

#[test]
fn a_terminal_without_images_shows_text_and_says_nothing_by_default() {
    let terminals = [
        ("ansi", Capabilities::ansi(40)),
        ("plain", Capabilities::plain(40)),
    ];
    for (name, caps) in terminals {
        for paging in [Paging::Always, Paging::Never] {
            let ran = (Sim::new(40, 8).caps(caps.clone()).reading_by_default())
                .file("long.md", &long())
                .settings(move |settings| settings.paging = paging)
                .keys(10, "q")
                .kiri(&["long.md"]);
            ok(&ran);
            assert_eq!(ran.err, "", "{name} {paging:?}");
            assert!(!ran.out.contains(GRAPHICS), "{name} {paging:?}");
            assert!(plain(&ran.out).contains("Line 1"), "{name} {paging:?}");
        }
    }
}

#[test]
fn output_into_a_pipe_is_text_and_says_nothing_by_default() {
    // Into a pipe the platform's capabilities are the plain ones.
    let ran = (Sim::new(40, 8).caps(Capabilities::plain(40)))
        .piped()
        .reading_by_default()
        .file("long.md", &long())
        .kiri(&["long.md"]);
    ok(&ran);
    assert_eq!(ran.err, "");
    assert!(!ran.out.contains(GRAPHICS), "{:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 10"), "{:?}", ran.out);
}

#[test]
fn reading_mode_that_is_asked_for_by_name_is_told_in_print_mode_too() {
    let ran = (Sim::new(40, 8).asks_reading())
        .file("long.md", &long())
        .settings(|settings| settings.paging = Paging::Never)
        .kiri(&["long.md"]);
    ok(&ran);
    assert_eq!(
        ran.err,
        "kiri: reading mode needs a terminal that draws images; showing the document as text\n"
    );
}

/// A report of `count` lines, as kiri writes its own.
fn report(count: usize) -> String {
    paragraphs(count)
}

#[test]
fn a_report_in_kitty_is_text_unless_reading_mode_is_asked_for() {
    let ran = (on_kitty(8).reading_by_default())
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .report(&report(10));
    ok(&ran);
    assert!(!ran.out.contains(GRAPHICS), "{:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 1"), "{:?}", ran.out);
    // Printed, not paged.
    let ran = (on_kitty(8).reading_by_default())
        .settings(|settings| settings.paging = Paging::Never)
        .report(&report(10));
    ok(&ran);
    assert!(!ran.out.contains(GRAPHICS), "{:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 10"), "{:?}", ran.out);
}

#[test]
fn a_report_in_kitty_is_reading_mode_when_it_is_asked_for_by_name() {
    for paging in [Paging::Always, Paging::Never] {
        let ran = (on_kitty(8).asks_reading())
            .settings(move |settings| settings.paging = paging)
            .keys(10, "q")
            .report(&report(10));
        ok(&ran);
        assert!(ran.out.contains(GRAPHICS), "{paging:?}: {:?}", ran.out);
    }
}

#[test]
fn a_report_asked_to_be_off_is_text() {
    let ran = (on_kitty(8).asks_no_reading())
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .report(&report(10));
    ok(&ran);
    assert!(!ran.out.contains(GRAPHICS), "{:?}", ran.out);
}

#[test]
fn m_turns_reading_mode_on_for_a_report_that_starts_as_text() {
    let ran = (on_kitty(8).reading_by_default())
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "M")
        .keys(30, "q")
        .report(&report(10));
    ok(&ran);
    let frames = ran.frames();
    assert!(frames.len() >= 2, "{frames:?}");
    assert!(
        !frames[0].contains(GRAPHICS),
        "first frame: {:?}",
        frames[0]
    );
    assert!(
        frames[1..].iter().any(|frame| frame.contains(GRAPHICS)),
        "M shows no reading mode: {:?}",
        ran.out
    );
}

const TABLE: &str = "name,count\nalpha,1\nbeta,2\n";

#[test]
fn a_csv_file_alone_stays_in_the_text_layout_by_default() {
    for paging in [Paging::Always, Paging::Never] {
        let ran = (on_kitty(8).reading_by_default())
            .file("data.csv", TABLE)
            .settings(move |settings| settings.paging = paging)
            .keys(10, "q")
            .kiri(&["data.csv"]);
        ok(&ran);
        assert!(!ran.out.contains(GRAPHICS), "{paging:?}: {:?}", ran.out);
        assert!(plain(&ran.out).contains("alpha"), "{paging:?}");
        assert_eq!(ran.err, "");
    }
}

#[test]
fn a_file_shown_as_text_stays_in_the_text_layout_by_default() {
    let ran = (on_kitty(8).reading_by_default())
        .file("notes.txt", "one\ntwo\n")
        .format(Input::Text)
        .settings(|settings| settings.paging = Paging::Never)
        .kiri(&["notes.txt"]);
    ok(&ran);
    assert!(!ran.out.contains(GRAPHICS), "{:?}", ran.out);
    assert!(plain(&ran.out).contains("two"));
}

#[test]
fn a_csv_file_is_in_reading_mode_when_reading_mode_is_named() {
    let ran = (on_kitty(8).asks_reading())
        .file("data.csv", TABLE)
        .settings(|settings| settings.paging = Paging::Never)
        .kiri(&["data.csv"]);
    ok(&ran);
    assert!(ran.out.contains(GRAPHICS), "no image in {:?}", ran.out);
}

#[test]
fn a_csv_file_beside_a_document_is_in_reading_mode_with_it_by_default() {
    // Reading mode is one setting of the run, so a document among the
    // files keeps it for all of them.
    let ran = (on_kitty(8).reading_by_default())
        .file("data.csv", TABLE)
        .file("doc.md", &paragraphs(2))
        .settings(|settings| settings.paging = Paging::Never)
        .kiri(&["data.csv", "doc.md"]);
    ok(&ran);
    assert!(ran.out.contains(GRAPHICS), "no image in {:?}", ran.out);
}
