//! Reloading in the simulated front end: `r` reads the shown file again
//! from the simulated disk, and standard input and several files have
//! nothing to reload.

use crate::frontend::sim::Sim;
use crate::session::settings::Paging;

/// `count` paragraphs of one row each, `Line 1` to `Line count`.
fn paragraphs(count: usize) -> String {
    (1..=count).map(|n| format!("Line {n}\n\n")).collect()
}

/// A document longer than a window of 5 rows.
fn long() -> String {
    paragraphs(10)
}

/// The text of `long.md` written anew: ten paragraphs of `Fresh`.
fn fresh() -> String {
    (1..=10).map(|n| format!("Fresh {n}\n\n")).collect()
}

#[test]
fn r_reads_the_file_again() {
    let ran = Sim::new(40, 5)
        .file("long.md", &long())
        .write(50, "long.md", &fresh())
        .keys(100, "r")
        .keys(200, "q")
        .kiri(&["long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert!(texts[0].contains("Line 1"), "{texts:?}");
    // The frame that says it is reloading, and the one of the new document.
    assert!(
        texts.iter().any(|text| text.contains("reloading")),
        "{texts:?}"
    );
    let last = texts.last().expect("a frame");
    assert!(last.contains("Fresh 1"), "{texts:?}");
    assert!(last.contains("reloaded"), "{texts:?}");
}

#[test]
fn standard_input_and_several_files_reload_nothing() {
    let ran = Sim::new(40, 5)
        .stdin(&long())
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "r")
        .keys(20, "q")
        .kiri(&["-"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(
        ran.texts()
            .iter()
            .any(|text| text.contains("nothing to reload")),
        "{:?}",
        ran.texts()
    );

    let ran = Sim::new(40, 5)
        .file("long.md", &long())
        .file("document.md", &paragraphs(3))
        .keys(10, "r")
        .keys(20, "q")
        .kiri(&["long.md", "document.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(
        ran.texts()
            .iter()
            .any(|text| text.contains("nothing to reload")),
        "{:?}",
        ran.texts()
    );
}
