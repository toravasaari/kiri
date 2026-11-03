//! Reading mode as the default: shown wherever the terminal draws images,
//! in the pager and when printing, and the text layout, without a word,
//! where it does not. The warning for a terminal that cannot show reading
//! mode when it was asked for by name is in `settings.rs`.

use super::{long, on_kitty, paragraphs};
use crate::frontend::sim::{Ran, Sim, plain};
use crate::session::settings::Paging;
use crate::typeset::caps::Capabilities;

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
