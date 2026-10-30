//! Tab and Enter on the links of the text layout.

use super::*;

fn kitty() -> Capabilities {
    Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    }
}

/// The frames and everything written for `document` in text, in a window
/// of 40 × `rows`, when the terminal gives `keys`.
fn shown_with(document: &Document, rows: u16, keys: &[&[u8]]) -> (Vec<String>, String) {
    let mut terminal = Fake::new(40, rows, keys);
    let mut inputs = Inputs::new(kitty());
    run_document(
        document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    (frames, String::from_utf8(terminal.out).unwrap())
}

/// OSC 52 for the clipboard, with `https://one.example/` in base64.
const COPY_ONE: &str = "\x1b]52;c;aHR0cHM6Ly9vbmUuZXhhbXBsZS8=\x1b\\";

/// The status line of `frame`, a window of `rows` high.
fn bar(frame: &str, rows: u16) -> String {
    let at = format!("\x1b[{rows};1H");
    frame.rsplit(&at).next().unwrap_or("").to_owned()
}

#[test]
fn tab_selects_a_link_in_the_text_in_reverse_video_and_the_status_line_says_where_it_goes() {
    let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
    let (frames, _) = shown_with(&document, 6, &[b"\t", b"\x1b", b""]);
    assert_eq!(frames.len(), 3, "{frames:#?}");
    let reversed =
        |frame: &str| style_before(document_part(frame, 6), "first").contains(&"7".into());
    assert!(!reversed(&frames[0]), "{:?}", frames[0]);
    assert!(reversed(&frames[1]), "{:?}", frames[1]);
    assert!(!reversed(&frames[2]), "{:?}", frames[2]);
    assert!(
        bar(&frames[1], 6).contains("https://one.example/"),
        "{frames:#?}"
    );
    assert!(!bar(&frames[2], 6).contains("https://one.example/"));
    // Only the link is in reverse video.
    assert!(!style_before(document_part(&frames[1], 6), "link.").contains(&"7".into()));
}

#[test]
fn a_wrapped_link_is_in_reverse_video_on_each_of_its_lines() {
    let document = crate::typeset::parse::parse(
        "[a long link text that wraps over lines in the window](https://one.example/)\n",
    );
    let (frames, _) = shown_with(&document, 6, &[b"\t", b""]);
    assert_eq!(frames.len(), 2, "{frames:#?}");
    let frame = document_part(&frames[1], 6);
    assert!(
        style_before(frame, "a long").contains(&"7".into()),
        "{frame:?}"
    );
    assert!(
        style_before(frame, "window").contains(&"7".into()),
        "{frame:?}"
    );
}

#[test]
fn enter_copies_the_selected_link_s_address_in_the_text_too() {
    let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
    let (frames, out) = shown_with(&document, 6, &[b"\t", b"\r", b""]);
    assert_eq!(out.matches(COPY_ONE).count(), 1, "{out:?}");
    assert!(
        bar(frames.last().unwrap(), 6).contains("link copied"),
        "{frames:#?}"
    );
}

#[test]
fn enter_with_a_link_that_has_no_address_copies_nothing_in_the_text() {
    let document = crate::typeset::parse::parse("A [first]() link.\n");
    let (_, out) = shown_with(&document, 6, &[b"\t", b"\r", b""]);
    assert!(!out.contains("\x1b]52;"), "{out:?}");
}

#[test]
fn a_wrapped_link_is_copied_while_one_of_its_lines_is_on_the_screen() {
    let filler = "A paragraph of filler.\n\n".repeat(10);
    let document = crate::typeset::parse::parse(&format!(
        "[a long link text that wraps over lines in the window](https://one.example/)\n\n{filler}"
    ));
    // The first line scrolls off the screen, the second stays; then the
    // second goes too.
    let (_, out) = shown_with(&document, 6, &[b"\t", b"j", b"\r", b""]);
    assert_eq!(out.matches(COPY_ONE).count(), 1, "{out:?}");
    let (_, out) = shown_with(&document, 6, &[b"\t", b"j", b"j", b"\r", b""]);
    assert!(!out.contains("\x1b]52;"), "{out:?}");
}

#[test]
fn a_link_in_a_heading_drawn_as_an_image_is_skipped_by_tab() {
    use crate::images::{ImageKey, Source, heading_words};
    use crate::typeset::model::Block;
    let document = crate::typeset::parse::parse(
        "# A [link](https://one.example/) here\n\nBody [other](https://two.example/)\n",
    );
    let Block::Heading { level, content } = &document.blocks[0] else {
        panic!("a heading");
    };
    let words = heading_words(content);
    let count = u16::try_from(words.len()).unwrap();
    let mut inputs = Inputs::new(Capabilities {
        text_sizing: false,
        ..kitty()
    });
    let key = ImageKey::heading_of(*level, words, (10, 20)).in_document(0);
    let image = image::RgbaImage::from_pixel(u32::from(count) * 40, 20, image::Rgba([9; 4]));
    let text = Source::Text {
        image,
        breaks: (1..count).map(|word| word * 40).collect(),
        ends: (1..=count).map(|word| word * 40 - 10).collect(),
    };
    inputs.images.insert(key, Ok(text));
    let mut terminal = Fake::new(40, 8, &[b"\t", b""]);
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    assert!(
        bar(&frames[1], 8).contains("https://two.example/"),
        "{frames:#?}"
    );
}
