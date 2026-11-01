//! Settings that change the route, as the simulated front end gets them
//! resolved: paging from the flag, the variable or the file; diagrams off;
//! reading mode on a terminal that draws no images. Which flag or variable
//! sets which setting is `config`'s tests (`src/config/tests.rs`).

use crate::frontend::sim::{Sim, plain};
use crate::session::settings::Paging;

/// `count` paragraphs of one row each, `Line 1` to `Line count`.
fn paragraphs(count: usize) -> String {
    (1..=count).map(|n| format!("Line {n}\n\n")).collect()
}

/// A document longer than a window of 5 rows.
fn long() -> String {
    paragraphs(10)
}

/// What kiri tells when reading mode is asked for on a terminal that does
/// not draw images.
const READING: &str =
    "kiri: reading mode needs a terminal that draws images; showing the document as text\n";

#[test]
fn capital_p_prints_a_long_document() {
    let ran = Sim::new(40, 5)
        .file("long.md", &long())
        .settings(|settings| settings.paging = Paging::Never)
        .kiri(&["long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "not paged: {:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 10"), "{:?}", ran.out);
}

#[test]
fn a_diagram_file_alone_that_is_not_drawn_is_paged_when_it_is_long_unlike_several() {
    let source: String = (1..=10).map(|n| format!("    A{n} --> B{n}\n")).collect();
    let source = format!("flowchart TD\n{source}");
    let ran = Sim::new(40, 5)
        .file("diagram.mmd", &source)
        .settings(|settings| settings.diagrams = false)
        .keys(10, "q")
        .kiri(&["diagram.mmd"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(!ran.frames().is_empty(), "paged: {:?}", ran.out);
    assert!(ran.texts()[0].contains("A1 --> B1"), "{:?}", ran.texts());

    // Several of them are each shown on their own: paged only with `always`.
    let ran = Sim::new(40, 5)
        .file("diagram.mmd", &source)
        .file("second.mmd", &source)
        .settings(|settings| settings.diagrams = false)
        .kiri(&["diagram.mmd", "second.mmd"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "not paged: {:?}", ran.out);
    assert_eq!(
        plain(&ran.out).matches("A1 --> B1").count(),
        2,
        "{:?}",
        ran.out
    );
}

#[test]
fn the_pager_tells_that_reading_mode_is_not_shown_for_a_file_alone_and_for_several() {
    for files in [&["long.md"][..], &["long.md", "document.md"][..]] {
        let ran = Sim::new(40, 8)
            .file("long.md", &long())
            .file("document.md", &paragraphs(3))
            .settings(|settings| {
                settings.reading = true;
                settings.paging = Paging::Always;
            })
            .keys(10, "q")
            .kiri(files);
        assert!(
            matches!(ran.result, Some(Ok(()))),
            "{files:?}: {:?}",
            ran.result
        );
        assert_eq!(ran.err, READING, "{files:?}");
        let first = &ran.texts()[0];
        assert!(
            first.contains("\u{26a0} reading mode"),
            "{files:?}: {first}"
        );
    }
}

#[test]
fn reading_mode_is_told_for_a_diagram_alone_paged_with_always() {
    let ran = Sim::new(40, 8)
        .file("flow.mmd", "flowchart TD\n")
        .settings(|settings| {
            settings.reading = true;
            settings.paging = Paging::Always;
        })
        .keys(10, "q")
        .kiri(&["flow.mmd"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert_eq!(ran.err, READING);
    assert!(
        ran.texts()[0].contains("\u{26a0} reading mode"),
        "{:?}",
        ran.texts()
    );
}

#[test]
fn reading_mode_is_not_told_for_a_diagram_alone_that_is_printed_in_a_terminal() {
    // With paging on `auto`, the diagram fits the window and is printed.
    let ran = Sim::new(40, 24)
        .file("flow.mmd", "flowchart TD\n")
        .settings(|settings| settings.reading = true)
        .kiri(&["flow.mmd"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "{:?}", ran.out);
    assert_eq!(ran.err, "");
    assert!(plain(&ran.out).contains("flowchart TD"), "{:?}", ran.out);
}
