//! The route's paging and printing decisions in the simulated frontend:
//! whether a run pages or prints, what the pager shows of several files
//! and of one that cannot be shown, and what standard error gets. The
//! keys come at their times on the virtual clock.

use super::{long, paragraphs};
use crate::frontend::Failure;
use crate::frontend::sim::{Sim, plain};
use crate::session::settings::Paging;
use crate::typeset::input::Input;

/// `front-matter-invalid.md`, whose front matter kiri warns about.
const INVALID: &str = include_str!("../../../tests/fixtures/front-matter-invalid.md");

#[test]
fn a_long_file_shown_as_text_is_paged_too() {
    let ran = Sim::new(40, 5)
        .file("long.md", &long())
        .format(Input::Text)
        .keys(10, "q")
        .kiri(&["long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(!ran.frames().is_empty(), "paged: {:?}", ran.out);
    assert!(ran.texts()[0].contains("Line 1"), "{:?}", ran.texts());
    assert_eq!(ran.err, "");
}

#[test]
fn a_short_document_is_printed() {
    let ran = Sim::new(40, 24)
        .file("document.md", &paragraphs(3))
        .kiri(&["document.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "not paged: {:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 3"), "{:?}", ran.out);
    assert_eq!(ran.err, "");
}

#[test]
fn p_pages_a_short_document() {
    let ran = Sim::new(40, 24)
        .file("document.md", &paragraphs(3))
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .kiri(&["document.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(!ran.frames().is_empty(), "paged: {:?}", ran.out);
}

#[test]
fn a_pipe_gets_the_document_printed_even_with_p() {
    let ran = Sim::new(40, 5)
        .file("long.md", &long())
        .piped()
        .settings(|settings| settings.paging = Paging::Always)
        .kiri(&["long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(
        ran.frames().is_empty(),
        "no frame of the pager: {:?}",
        ran.out
    );
    assert!(plain(&ran.out).contains("Line 10"), "{:?}", ran.out);
    assert_eq!(ran.err, "");
}

#[test]
fn without_a_terminal_for_keys_p_prints_with_a_warning() {
    let ran = Sim::new(40, 5)
        .file("long.md", &long())
        .no_keys()
        .settings(|settings| settings.paging = Paging::Always)
        .kiri(&["long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "{:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 10"), "{:?}", ran.out);
    assert_eq!(
        ran.err,
        "kiri: no terminal to read keys from, so the document is printed\n"
    );
}

#[test]
fn several_files_are_paged_as_one_stream_under_their_names() {
    let ran = Sim::new(40, 8)
        .file("document.md", &paragraphs(3))
        .file("long.md", &long())
        .keys(10, "G")
        .keys(20, "g")
        .keys(30, "/")
        .keys(40, "Line 10\r")
        .keys(50, "q")
        .kiri(&["document.md", "long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert!(texts[0].contains("document.md"), "{texts:?}");
    assert!(!texts[0].contains("long.md"), "{texts:?}");
    // The end of the stream is the end of the last file.
    assert!(texts[1].contains("Line 10"), "{texts:?}");
    // Search goes on from the first file into the second.
    let last = texts.last().expect("a frame");
    assert!(last.contains("Line 10"), "{last}");
    assert!(last.contains("/Line 10 1/1"), "{last}");
    assert_eq!(ran.err, "");
}

#[test]
fn colon_n_moves_to_the_next_file_and_the_status_line_names_it() {
    let ran = Sim::new(40, 8)
        .file("document.md", &paragraphs(3))
        .file("long.md", &long())
        .keys(10, ":n")
        .keys(20, "q")
        .kiri(&["document.md", "long.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(!texts[0].contains("long.md"), "{texts:?}");
    assert!(texts[1].contains("long.md"), "{texts:?}");
    assert!(!texts[1].contains("document.md"), "{texts:?}");
}

#[test]
fn a_file_that_cannot_be_shown_is_in_the_stream_under_its_header() {
    let ran = Sim::new(40, 8)
        .file("document.md", &paragraphs(3))
        .file("long.md", &long())
        .keys(10, ":n")
        .keys(20, "q")
        .kiri(&["document.md", "missing.md", "long.md"]);
    assert!(
        matches!(ran.result, Some(Err(Failure::Told))),
        "{:?}",
        ran.result
    );
    assert!(ran.err.starts_with("kiri: missing.md: "), "{:?}", ran.err);
    assert_eq!(ran.err.lines().count(), 1, "{:?}", ran.err);
    let texts = ran.texts();
    assert_eq!(texts.len(), 2, "{texts:?}");
    // :n goes to missing.md, its header and why it is not shown.
    assert!(texts[1].contains("missing.md"), "{texts:?}");
    assert!(texts[1].contains("No such file"), "{texts:?}");
    assert!(texts[1].contains(" missing.md 2/3 "), "{texts:?}");
}

#[test]
fn when_no_file_can_be_shown_the_pager_does_not_open() {
    let ran = Sim::new(40, 8)
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .kiri(&["nope1.md", "nope2.md"]);
    assert!(
        matches!(ran.result, Some(Err(Failure::Told))),
        "{:?}",
        ran.result
    );
    // An empty pager would wait for q.
    assert!(ran.frames().is_empty(), "{:?}", ran.out);
    assert!(ran.err.contains("nope1.md"), "{:?}", ran.err);
    assert!(ran.err.contains("nope2.md"), "{:?}", ran.err);
}

#[test]
fn a_front_matter_warning_is_on_stderr_and_in_the_pager_from_the_start() {
    let ran = Sim::new(40, 8)
        .file("invalid.md", INVALID)
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .kiri(&["invalid.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(
        ran.err.contains("kiri: front matter, line 4:"),
        "{:?}",
        ran.err
    );
    // The line is cut for a window of 40 columns, but starts as it does.
    let first = &ran.texts()[0];
    assert!(first.contains("\u{26a0} [:4] front matter: "), "{first}");
    assert!(first.contains("\u{26a0} 1"), "{first}");
}

#[test]
fn a_front_matter_warning_is_in_the_pager_for_a_file_alone_but_not_among_several() {
    let ran = Sim::new(40, 8)
        .file("invalid.md", INVALID)
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .kiri(&["invalid.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(
        ran.err.starts_with("kiri: front matter, line 4:"),
        "{:?}",
        ran.err
    );
    assert!(
        ran.texts()[0].contains("\u{26a0} [:4] front matter"),
        "{:?}",
        ran.texts()
    );

    let ran = Sim::new(40, 8)
        .file("invalid.md", INVALID)
        .file("document.md", &paragraphs(3))
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .kiri(&["invalid.md", "document.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(
        ran.err
            .starts_with("kiri: invalid.md: front matter, line 4:"),
        "{:?}",
        ran.err
    );
    assert!(!plain(&ran.out).contains('\u{26a0}'), "{:?}", ran.out);
}
