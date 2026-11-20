//! Settings that change the route, as the simulated frontend gets them
//! resolved: paging from the flag, the variable or the file; diagrams off;
//! reading mode on a terminal that draws no images. Which flag or variable
//! sets which setting is `config`'s tests (`src/config/tests.rs`).

use super::{long, paragraphs};
use crate::frontend::sim::{Sim, plain};
use crate::session::settings::{Paging, Toc};

/// What kiri tells when reading mode is asked for on a terminal that does
/// not draw images.
const READING: &str =
    "kiri: reading mode needs a terminal that draws images; showing the document as text\n";

#[test]
fn capital_p_prints_a_long_document() {
    let ran = Sim::new(40, 5)
        .file("long.md", &long())
        .settings(|settings| settings.paging = Paging::Never)
        .kiri(&["long.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "not paged: {:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 10"), "{:?}", ran.out);
}

#[test]
fn diagram_files_that_are_not_drawn_are_paged_when_they_are_long_alone_or_together() {
    let source: String = (1..=10).map(|n| format!("    A{n} --> B{n}\n")).collect();
    let source = format!("flowchart TD\n{source}");
    for files in [&["diagram.mmd"][..], &["diagram.mmd", "second.mmd"][..]] {
        let ran = Sim::new(40, 5)
            .file("diagram.mmd", &source)
            .file("second.mmd", &source)
            .settings(|settings| settings.diagrams = false)
            .keys(10, "q")
            .kiri(files)
            .quiet();
        assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
        assert!(!ran.frames().is_empty(), "{files:?} paged: {:?}", ran.out);
        // The first of several is under its header, which fills the window.
        assert!(ran.texts()[0].contains("diagram.mmd"), "{:?}", ran.texts());
    }

    // Short enough for the window, they are printed.
    let ran = Sim::new(40, 60)
        .file("diagram.mmd", &source)
        .file("second.mmd", &source)
        .settings(|settings| settings.diagrams = false)
        .kiri(&["diagram.mmd", "second.mmd"])
        .quiet();
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
            .asks_reading()
            .settings(|settings| settings.paging = Paging::Always)
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
        .asks_reading()
        .settings(|settings| settings.paging = Paging::Always)
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
        .asks_reading()
        .kiri(&["flow.mmd"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "{:?}", ran.out);
    assert_eq!(ran.err, "");
    assert!(plain(&ran.out).contains("flowchart TD"), "{:?}", ran.out);
}

#[test]
fn reading_mode_that_is_not_asked_for_is_not_told_when_it_is_not_shown() {
    // The setting is on, as it is by default, but nobody asked for it.
    for paging in [Paging::Always, Paging::Never] {
        let ran = Sim::new(40, 8)
            .file("long.md", &long())
            .settings(move |settings| {
                settings.reading = true;
                settings.paging = paging;
            })
            .keys(10, "q")
            .kiri(&["long.md"]);
        assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
        assert_eq!(ran.err, "", "{paging:?}");
        if paging == Paging::Always {
            let first = &ran.texts()[0];
            assert!(!first.contains("reading mode"), "{first}");
        }
    }
}

#[test]
fn reading_mode_that_is_asked_to_be_off_is_not_told_when_it_is_not_shown() {
    let ran = Sim::new(40, 8)
        .file("long.md", &long())
        .asks_no_reading()
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .kiri(&["long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert_eq!(ran.err, "");
}

/// Two headings, one with a heading under it, and a paragraph each.
const HEADED: &str = "# Alpha\n\n## Beta\n\ntext\n\n# Gamma\n\nmore\n";

#[test]
fn toc_off_hides_the_contents_bar_in_a_wide_window_until_t_shows_it() {
    let ran = Sim::new(120, 12)
        .file("a.md", HEADED)
        .settings(|settings| {
            settings.toc = Toc::Off;
            settings.paging = Paging::Always;
        })
        .keys(10, "T")
        .keys(20, "q")
        .kiri(&["a.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let screens = ran.shown();
    assert_eq!(screens.len(), 2, "{:?}", ran.out);
    assert!(!screens[0].text().contains('▾'), "{}", screens[0].text());
    assert!(screens[1].text().contains('▾'), "{}", screens[1].text());
}

#[test]
fn toc_on_shows_the_bar_where_the_document_keeps_40_columns() {
    // At 70 columns, auto leaves the document too narrow for the bar; on
    // shows it, since the document keeps 47 columns.
    for toc in [Toc::Auto, Toc::On] {
        let ran = Sim::new(70, 12)
            .file("a.md", HEADED)
            .settings(move |settings| {
                settings.toc = toc;
                settings.paging = Paging::Always;
            })
            .keys(10, "q")
            .kiri(&["a.md"])
            .quiet();
        assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
        let first = ran.shown()[0].text();
        assert_eq!(first.contains('▾'), toc == Toc::On, "{toc:?}: {first}");
    }
}

/// `count` sections, each of a heading and a line, the line of `changed`
/// being `edited`.
fn sections(count: usize, changed: usize, edited: &str) -> String {
    (0..count)
        .map(|n| {
            let line = if n == changed { edited } else { "line" };
            format!("# H{n:02}\n\n{line}\n\n")
        })
        .collect()
}

#[test]
fn a_reload_while_the_bar_previews_leaves_esc_putting_the_place_back() {
    use crate::session::settings::{Follow, FollowJump};
    let run = |keys: &[(u64, &str)]| {
        let mut sim = Sim::new(120, 12)
            .file("a.md", &sections(30, 99, ""))
            .follow(Follow::On, FollowJump::First)
            .settings(|settings| {
                settings.toc_preview = true;
                settings.paging = Paging::Always;
            })
            .write(500, "a.md", &sections(30, 20, "edited line"));
        for &(ms, keys) in keys {
            sim = sim.keys(ms, keys);
        }
        sim.kiri(&["a.md"]).quiet()
    };
    // The reload comes while the selection has moved the view to the end.
    let back = run(&[
        (10, "jjj"),
        (20, "t"),
        (30, "G"),
        (3000, "\x1b"),
        (4000, "q"),
    ]);
    let kept = run(&[(10, "jjj"), (4000, "q")]);
    assert!(matches!(back.result, Some(Ok(()))), "{:?}", back.result);
    // The document's rows above the status line, which says "reloaded" for
    // a while. The bar's list is not compared: the selection moved it, and
    // it stays where it is until the heading the reader is in needs it to
    // move.
    let rows = |ran: &crate::frontend::sim::Ran| {
        let text = ran.shown().last().unwrap().text();
        let lines: Vec<&str> = text.lines().collect();
        let page = lines[..lines.len() - 1].iter();
        let page = page.map(|line| line.split_once('▕').map_or(*line, |(_, page)| page));
        page.collect::<Vec<_>>().join("\n")
    };
    assert_eq!(rows(&back), rows(&kept));
}
