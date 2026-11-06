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

/// An SVG image file `width` by `height` pixels, which kiri shows as an
/// image on its own.
fn svg(width: u32, height: u32) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\">\
         <rect width=\"{width}\" height=\"{height}\" fill=\"red\"/></svg>"
    )
}

/// A window of `rows` rows on kitty, which draws images, with the image
/// files `a.svg`, `b.svg` and `c.svg` of `height` pixels each.
fn images(rows: u16, height: u32) -> Sim {
    let sim = super::on_kitty(rows);
    ["a.svg", "b.svg", "c.svg"]
        .iter()
        .fold(sim, |sim, name| sim.file(name, &svg(64, height)))
}

#[test]
fn image_files_together_longer_than_the_window_are_paged() {
    let ran = images(12, 64)
        .keys(10, "q")
        .kiri(&["a.svg", "b.svg", "c.svg"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(!ran.frames().is_empty(), "paged: {:?}", plain(&ran.out));
    assert!(ran.texts()[0].contains("a.svg"), "{:?}", ran.texts());
}

#[test]
fn image_files_that_fit_the_window_together_are_printed() {
    let ran = images(40, 16).kiri(&["a.svg", "b.svg", "c.svg"]).quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "not paged: {:?}", plain(&ran.out));
    assert!(plain(&ran.out).contains("c.svg"), "{:?}", ran.out);
}

#[test]
fn image_files_are_not_paged_with_paging_never() {
    let ran = images(12, 64)
        .settings(|settings| settings.paging = Paging::Never)
        .kiri(&["a.svg", "b.svg", "c.svg"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "not paged: {:?}", plain(&ran.out));
}

#[test]
fn an_image_and_a_code_file_are_paged_by_their_rows_together() {
    let code = "fn main() {}\n".repeat(6);
    let both = |window| {
        images(window, 32)
            .file("code.rs", &code)
            .keys(10, "q")
            .kiri(&["a.svg", "code.rs"])
            .quiet()
    };
    // Neither alone is longer than the window of 14 rows, together they
    // are.
    assert!(!both(14).frames().is_empty(), "paged");
    assert!(both(40).frames().is_empty(), "printed");
}

#[test]
fn an_image_alone_is_scaled_to_the_window_so_it_is_printed() {
    let ran = images(12, 4000).kiri(&["a.svg"]).quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "not paged: {:?}", plain(&ran.out));
}

#[test]
fn an_image_from_standard_input_is_paged_only_when_paging_is_always() {
    let ran = super::on_kitty(12).stdin(&svg(64, 64)).kiri(&["-"]).quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "not paged: {:?}", plain(&ran.out));
}

#[test]
fn an_image_that_is_printed_after_the_pager_did_not_open_looks_as_with_paging_never() {
    let print = |paging| {
        let ran = images(12, 64)
            .settings(move |settings| settings.paging = paging)
            .kiri(&["a.svg"])
            .quiet();
        assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
        ran.out
    };
    assert_eq!(print(Paging::Auto), print(Paging::Never));
}

#[test]
fn an_image_alone_printed_after_the_pager_did_not_open_has_no_page_of_a_paper_theme() {
    // A theme that paints its page, as the dark and light themes do.
    let print = |paging| {
        let ran = images(12, 64)
            .settings(move |settings| {
                settings.paging = paging;
                settings.theme.background = Some(crate::typeset::color::Color::Rgb(10, 20, 30));
            })
            .kiri(&["a.svg"])
            .quiet();
        assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
        ran.out
    };
    assert_eq!(print(Paging::Auto), print(Paging::Never));
}

/// A PNG image file `width` by `height` pixels.
fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::new(width, height);
    let mut bytes = std::io::Cursor::new(Vec::new());
    (image.write_to(&mut bytes, image::ImageFormat::Png)).expect("a PNG encodes");
    bytes.into_inner()
}

/// The making of an image file of a width and a height in pixels.
type Make = fn(u32, u32) -> Vec<u8>;

/// Whether the image files of `heights` pixels, each 64 pixels wide, made
/// by `make` and named with the extension `kind`, are paged in a window
/// of `rows` rows.
fn pages(kind: &str, make: Make, heights: &[u32], rows: u16) -> bool {
    let names: Vec<String> = (0..heights.len())
        .map(|at| format!("{at}.{kind}"))
        .collect();
    let sim = super::on_kitty(rows).keys(10, "q");
    let sim = names.iter().zip(heights).fold(sim, |sim, (name, height)| {
        sim.file_of(name, &make(64, *height))
    });
    let files: Vec<&str> = names.iter().map(String::as_str).collect();
    let ran = sim.kiri(&files).quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    !ran.frames().is_empty()
}

#[test]
fn image_files_are_paged_exactly_when_their_rows_together_are_longer_than_the_window() {
    // For each set of image heights, the fewest rows of a window that
    // prints them: the pager opens when the window is one row less. The
    // rows of an SVG and a raster image of the same size are the same.
    let sets: [(&[u32], u16); 4] = [
        (&[16, 16], 11),
        (&[64, 64, 64], 26),
        (&[1, 1], 11),
        (&[32, 16, 8], 18),
    ];
    let kinds: [(&str, Make); 2] = [("svg", |w, h| svg(w, h).into_bytes()), ("png", png)];
    for (kind, make) in kinds {
        for (heights, rows) in sets {
            assert!(
                !pages(kind, make, heights, rows),
                "{kind} {heights:?} in {rows} rows"
            );
            assert!(
                pages(kind, make, heights, rows - 1),
                "{kind} {heights:?} in {} rows",
                rows - 1
            );
        }
    }
}

#[test]
fn image_files_are_read_one_at_a_time_when_they_are_printed() {
    let ran = images(40, 16).kiri(&["a.svg", "b.svg", "c.svg"]).quiet();
    assert!(ran.frames().is_empty(), "printed: {:?}", plain(&ran.out));
    // The last time each file is opened is to show it: only after the
    // image before it was written, so that no more than one is held.
    let last = |name: &str| ran.opened.iter().rfind(|(path, _)| path == name).unwrap().1;
    assert_eq!(last("a.svg"), 0, "{:?}", ran.opened);
    assert!(last("b.svg") > last("a.svg"), "{:?}", ran.opened);
    assert!(last("c.svg") > last("b.svg"), "{:?}", ran.opened);
}

#[test]
fn the_pager_reads_the_image_files_when_it_opens() {
    let ran = images(12, 64)
        .keys(10, "q")
        .kiri(&["a.svg", "b.svg", "c.svg"])
        .quiet();
    assert!(!ran.frames().is_empty(), "paged");
    // The image on the first screen is sent to the terminal, so it was read.
    assert!(ran.out.contains("\x1b_Ga=T,"), "{:?}", ran.out);
}

#[test]
fn image_files_that_were_to_be_paged_but_cannot_are_printed_as_with_paging_never() {
    let print = |paging, keys: bool| {
        let sim = match keys {
            true => images(12, 64),
            false => images(12, 64).no_keys(),
        };
        let ran = sim
            .settings(move |settings| settings.paging = paging)
            .kiri(&["a.svg", "b.svg", "c.svg"])
            .quiet();
        assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
        ran.out
    };
    // Auto with no terminal to read keys from: the rows say to page, and
    // the files are printed as they are with paging never. That the images
    // are read one at a time is checked by the test of the files' reads.
    assert_eq!(print(Paging::Auto, false), print(Paging::Never, true));
}

/// A PNG image file `width` by `height` pixels whose end is missing: its
/// header can be read, but it cannot be decoded.
fn broken(width: u32, height: u32) -> Vec<u8> {
    let bytes = png(width, height);
    bytes[..bytes.len() - 20].to_vec()
}

#[test]
fn image_files_are_read_one_at_a_time_when_there_is_no_terminal_to_page_in() {
    let ran = images(12, 64)
        .no_keys()
        .kiri(&["a.svg", "b.svg", "c.svg"])
        .quiet();
    assert!(ran.frames().is_empty(), "printed: {:?}", plain(&ran.out));
    let last = |name: &str| ran.opened.iter().rfind(|(path, _)| path == name).unwrap().1;
    assert_eq!(last("a.svg"), 0, "{:?}", ran.opened);
    assert!(last("b.svg") > last("a.svg"), "{:?}", ran.opened);
    assert!(last("c.svg") > last("b.svg"), "{:?}", ran.opened);
}

#[test]
fn a_broken_image_file_is_told_once_when_the_files_are_printed_after_all() {
    let ran = super::on_kitty(12)
        .no_keys()
        .file_of("a.png", &broken(640, 640))
        .file_of("b.png", &broken(640, 640))
        .kiri(&["a.png", "b.png"]);
    assert!(ran.frames().is_empty(), "printed: {:?}", plain(&ran.out));
    assert_eq!(ran.err.matches("a.png").count(), 1, "{:?}", ran.err);
    assert_eq!(ran.err.matches("b.png").count(), 1, "{:?}", ran.err);
}

#[test]
fn an_image_file_that_cannot_be_read_is_an_error_in_the_pager_as_with_paging_always() {
    // The header of a broken file can be read, so the pager does not know
    // that it is broken until it opens; it then shows what it would have
    // shown had it known.
    let shown = |paging| {
        let ran = super::on_kitty(12)
            .settings(move |settings| settings.paging = paging)
            .keys(10, "q")
            .file("a.svg", &svg(64, 64))
            .file_of("b.png", &broken(640, 640))
            .kiri(&["b.png", "a.svg"]);
        // A file that failed ends kiri with a failure.
        assert!(matches!(ran.result, Some(Err(_))), "{:?}", ran.result);
        assert!(!ran.frames().is_empty(), "paged");
        assert_eq!(ran.err, "kiri: b.png: the image data is cut short\n");
        ran
    };
    let auto = shown(Paging::Auto);
    let always = shown(Paging::Always);
    assert!(
        auto.texts()
            .iter()
            .any(|text| text.contains("\u{2717} the image data is cut short")),
        "{:?}",
        auto.texts()
    );
    assert_eq!(auto.texts(), always.texts());
}

/// The animated GIF of the fixtures.
const GIF: &[u8] = include_bytes!("../../../tests/fixtures/images/animated.gif");

#[test]
fn animated_image_files_that_are_printed_after_the_pager_did_not_open_show_one_frame() {
    // The pager measured the files and decided not to open: they are
    // printed, each as its first frame alone. The output is the same
    // whether the files were decoded with all their frames or not; that
    // they are not is only a saving, which no output tells.
    let ran = super::on_kitty(60)
        .file_of("a.gif", GIF)
        .file_of("b.gif", GIF)
        .kiri(&["a.gif", "b.gif"])
        .quiet();
    assert!(ran.frames().is_empty(), "printed");
    assert_eq!(ran.out.matches("\x1b_Ga=T,").count(), 2, "{:?}", ran.out);
    assert!(!ran.out.contains("\x1b_Ga=f,"), "{:?}", ran.out);
    assert!(!ran.out.contains("\x1b_Ga=a,"), "{:?}", ran.out);
}

#[test]
fn what_is_told_of_the_files_when_the_pager_opens_comes_in_the_order_of_the_files() {
    // The warning of the first file, and then why the second, an image
    // file that is broken beyond its header, cannot be shown; as when the
    // files are printed.
    let told = |paging| {
        let ran = super::on_kitty(12)
            .settings(move |settings| settings.paging = paging)
            .keys(10, "q")
            .file("front.md", INVALID)
            .file_of("b.png", &broken(640, 640))
            .kiri(&["front.md", "b.png"]);
        assert!(!ran.frames().is_empty(), "paged");
        ran.err
    };
    let err = told(Paging::Auto);
    let (warning, error) = (err.find("front.md"), err.find("b.png"));
    assert!(warning.is_some() && warning < error, "{err:?}");
    assert_eq!(err, told(Paging::Always));
}
