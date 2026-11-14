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
fn the_help_opens_at_the_links_while_one_is_selected() {
    let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
    let (frames, _) = shown_with(&document, 24, &[b"\t", b"?"]);
    let screens = replay(&frames);
    assert_eq!(screens.len(), 3, "{screens:#?}");
    assert!(screen_row(&screens[2], 2).contains("Links"), "{screens:#?}");
}

#[test]
fn tab_selects_a_link_in_the_text_in_reverse_video_and_the_status_line_says_where_it_goes() {
    let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
    let (frames, _) = shown_with(&document, 6, &[b"\t", b"\x1b", b""]);
    let screens = replay(&frames);
    assert_eq!(frames.len(), 3, "{frames:#?}");
    let reversed =
        |screen: &[String]| style_before(&screen_above(screen, 6), "first").contains(&"7".into());
    assert!(!reversed(&screens[0]), "{:?}", screens[0]);
    assert!(reversed(&screens[1]), "{:?}", screens[1]);
    assert!(!reversed(&screens[2]), "{:?}", screens[2]);
    assert!(
        screen_row(&screens[1], 6).contains("https://one.example/"),
        "{screens:#?}"
    );
    assert!(!screen_row(&screens[2], 6).contains("https://one.example/"));
    // Only the link is in reverse video.
    assert!(!style_before(&screen_above(&screens[1], 6), "link.").contains(&"7".into()));
}

#[test]
fn a_wrapped_link_is_in_reverse_video_on_each_of_its_lines() {
    let document = crate::typeset::parse::parse(
        "[a long link text that wraps over lines in the window](https://one.example/)\n",
    );
    let (frames, _) = shown_with(&document, 6, &[b"\t", b""]);
    let screens = replay(&frames);
    assert_eq!(frames.len(), 2, "{frames:#?}");
    let frame = screen_above(&screens[1], 6);
    assert!(
        style_before(&frame, "a long").contains(&"7".into()),
        "{frame:?}"
    );
    assert!(
        style_before(&frame, "window").contains(&"7".into()),
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
    let key = inputs.drawn(ImageKey::heading_of(*level, words, (10, 20)).in_document(0));
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

#[test]
fn tab_shows_a_link_that_was_wholly_out_of_the_view_in_the_middle_of_it() {
    let mut source: String = (0..20).map(|n| format!("Paragraph {n}\n\n")).collect();
    source.push_str("A [first](https://one.example/) link.\n\n");
    source.push_str(&"More text.\n\n".repeat(10));
    let document = crate::typeset::parse::parse(&source);
    let (frames, _) = shown_with(&document, 8, &[b"\t", b""]);
    let screens = replay(&frames);
    // Three rows of the document's seven above it, and three below.
    assert_eq!(
        screen_row_of(&screens[1], 8, "first"),
        Some(4),
        "{:?}",
        screens[1]
    );
}

#[test]
fn tab_shows_a_wrapped_link_that_was_wholly_out_of_the_view_in_the_middle() {
    let mut source: String = (0..20).map(|n| format!("Paragraph {n}\n\n")).collect();
    source.push_str(&format!(
        "A [alpha{} omega](https://one.example/) link.\n\n",
        " word".repeat(18)
    ));
    source.push_str(&"More text.\n\n".repeat(10));
    let document = crate::typeset::parse::parse(&source);
    let (frames, _) = shown_with(&document, 8, &[b"\t", b""]);
    let screens = replay(&frames);
    // The link has three lines: two rows above them, and two below, of the
    // seven rows of the document.
    let frame = &screens[1];
    assert_eq!(screen_row_of(frame, 8, "alpha"), Some(3), "{frame:?}");
    assert_eq!(screen_row_of(frame, 8, "omega"), Some(5), "{frame:?}");
}

#[test]
fn a_linked_block_image_in_text_is_no_tab_stop() {
    // The image is drawn as rows with no link on them, so there is nothing
    // to follow and nothing to show as selected: Tab goes to the link after
    // it, as it always did.
    let document = crate::typeset::parse::parse(
        "[![alt](img.png)](https://x.example/)\n\nA [link](https://one.example/) here.\n",
    );
    let mut inputs = Inputs::new(kitty());
    let image = image::RgbaImage::from_pixel(40, 40, image::Rgba([9; 4]));
    inputs
        .images
        .insert("img.png", Ok(crate::images::Source::Raster(image)));
    let mut terminal = Fake::new(40, 14, &[b"\t", b""]);
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    let status = bar(&frames[1], 14);
    assert!(status.contains("https://one.example/"), "{status:?}");
    assert!(!status.contains("https://x.example/"), "{status:?}");
}

/// A link to `#target` that `with` is the text of, 12 paragraphs, the
/// heading `# Target`, and 12 more paragraphs.
fn jumping(with: &str) -> Document {
    let before: String = (1..=12).map(|n| format!("Before {n}.\n\n")).collect();
    let after: String = (1..=12).map(|n| format!("After {n}.\n\n")).collect();
    crate::typeset::parse::parse(&format!(
        "{with}\n\n{before}# Target\n\nUnder it.\n\n{after}"
    ))
}

const BACKSPACE: &[u8] = b"\x7f";

/// The text of the top row of `screen`, which the document starts on.
fn top(screen: &[String]) -> String {
    unstyled(screen_row(screen, 1))
}

#[test]
fn enter_on_an_anchor_link_jumps_to_the_heading_and_backspace_comes_back() {
    let document = jumping("[go](#target)");
    let (frames, out) = shown_with(&document, 6, &[b"\t", b"\r", BACKSPACE, b""]);
    let screens = replay(&frames);
    assert!(top(&screens[1]).contains("go"), "{screens:#?}");
    assert!(top(&screens[2]).contains("Target"), "{screens:#?}");
    let last = screens.last().unwrap();
    assert!(top(last).contains("go"), "{screens:#?}");
    // The jump let the link go, and Backspace does not select it again.
    assert!(
        !screen_row(&screens[2], 6).contains("#target"),
        "{screens:#?}"
    );
    assert!(!screen_row(last, 6).contains("#target"), "{screens:#?}");
    assert!(!out.contains("\x1b]52;c;I3RhcmdldA"), "copied: {out:?}");
}

#[test]
fn an_anchor_link_is_not_copied() {
    let document = jumping("[go](#target)");
    let (frames, out) = shown_with(&document, 6, &[b"\t", b"\r", b""]);
    assert!(!out.contains("\x1b]52;"), "{out:?}");
    let bar = bar(frames.last().unwrap(), 6);
    assert!(!bar.contains("link copied"), "{bar:?}");
}

#[test]
fn an_address_away_is_still_copied_next_to_an_anchor_link() {
    let document = jumping("[go](#target) and [away](https://one.example/)");
    let (_, out) = shown_with(&document, 6, &[b"\t", b"\t", b"\r", b""]);
    assert_eq!(out.matches(COPY_ONE).count(), 1, "{out:?}");
}

#[test]
fn an_anchor_that_is_not_there_is_told_and_backspace_has_nothing_to_go_back_to() {
    let document = jumping("[go](#nowhere)");
    let (frames, _) = shown_with(&document, 6, &[b"\t", b"\r", BACKSPACE, b""]);
    let screens = replay(&frames);
    assert!(top(&screens[2]).contains("go"), "{screens:#?}");
    assert!(
        screen_row(&screens[2], 6).contains("no #nowhere here"),
        "{screens:#?}"
    );
    let last = screens.last().unwrap();
    assert!(
        screen_row(last, 6).contains("nothing to go back to"),
        "{screens:#?}"
    );
}

#[test]
fn backspace_with_no_history_says_so() {
    let document = jumping("text");
    let (frames, _) = shown_with(&document, 6, &[BACKSPACE, b""]);
    let screens = replay(&frames);
    let last = screens.last().unwrap();
    assert!(
        screen_row(last, 6).contains("nothing to go back to"),
        "{screens:#?}"
    );
    assert!(top(last).contains("text"), "{screens:#?}");
}

#[test]
fn a_jump_to_an_a_name_above_a_heading_shows_the_heading() {
    let before: String = (1..=12).map(|n| format!("Before {n}.\n\n")).collect();
    let document = crate::typeset::parse::parse(&format!(
        "[go](#old-name)\n\n{before}<a name=\"old-name\"></a>\n\n# Target\n\nUnder it.\n\n{before}"
    ));
    let (frames, _) = shown_with(&document, 6, &[b"\t", b"\r", b""]);
    let screens = replay(&frames);
    let text = screen_text(screens.last().unwrap());
    assert!(text.contains("Target"), "{text}");
    assert!(!text.contains("Before 12"), "{text}");
}

#[test]
fn a_jump_puts_the_target_on_the_top_line_also_when_it_is_in_view() {
    let after: String = (1..=20).map(|n| format!("After {n}.\n\n")).collect();
    let document = crate::typeset::parse::parse(&format!(
        "[go](#target)\n\nOne.\n\n# Target\n\nUnder it.\n\n{after}"
    ));
    let (frames, _) = shown_with(&document, 12, &[b"\t", b"\r", BACKSPACE, b""]);
    let screens = replay(&frames);
    assert!(top(&screens[2]).contains("Target"), "{screens:#?}");
    assert!(top(screens.last().unwrap()).contains("go"), "{screens:#?}");
}

#[test]
fn two_jumps_come_back_one_at_a_time() {
    let before: String = (1..=12).map(|n| format!("Before {n}.\n\n")).collect();
    let document = crate::typeset::parse::parse(&format!(
        "[one](#first)\n\n{before}# First\n\n[two](#second)\n\n{before}# Second\n\nEnd.\n\n{before}"
    ));
    let keys: &[&[u8]] = &[b"\t", b"\r", b"\t", b"\r", BACKSPACE, BACKSPACE, b""];
    let (frames, _) = shown_with(&document, 6, keys);
    let screens = replay(&frames);
    let tops: Vec<String> = screens.iter().map(|screen| top(screen)).collect();
    let has = |at: usize, word: &str| tops[at].contains(word);
    assert!(has(2, "First"), "{tops:#?}");
    assert!(has(4, "Second"), "{tops:#?}");
    assert!(has(5, "First"), "{tops:#?}");
    assert!(has(6, "one"), "{tops:#?}");
}

#[test]
fn backspace_in_the_prompt_still_deletes_and_does_not_go_back() {
    let document = jumping("[go](#target)");
    // After a jump, `/` and Backspace on the empty prompt cancel it: the
    // view stays at the target.
    let keys: &[&[u8]] = &[b"\t", b"\r", b"/", BACKSPACE, b""];
    let (frames, _) = shown_with(&document, 6, keys);
    let screens = replay(&frames);
    let last = screens.last().unwrap();
    assert!(top(last).contains("Target"), "{screens:#?}");
    // The one Backspace after that goes back.
    let keys: &[&[u8]] = &[b"\t", b"\r", b"/", BACKSPACE, BACKSPACE, b""];
    let (frames, _) = shown_with(&document, 6, keys);
    let screens = replay(&frames);
    assert!(top(screens.last().unwrap()).contains("go"), "{screens:#?}");
}

#[test]
fn the_help_lists_backspace_with_the_links() {
    let document = jumping("[go](#target)");
    let (frames, _) = shown_with(&document, 24, &[b"\t", b"?"]);
    let screens = replay(&frames);
    let text = screen_text(screens.last().unwrap());
    assert!(text.contains("Backspace"), "{text}");
}

#[test]
fn a_selected_link_with_a_title_shows_the_title_and_one_without_its_address() {
    let document = crate::typeset::parse::parse(
        "A [first](https://one.example/ \"The first one\") and [second](https://two.example/).\n",
    );
    let (frames, _) = shown_with(&document, 6, &[b"\t", b"\t", b""]);
    let screens = replay(&frames);
    let first = screen_row(&screens[1], 6);
    assert!(first.contains("The first one"), "{screens:#?}");
    assert!(!first.contains("one.example"), "{screens:#?}");
    let second = screen_row(&screens[2], 6);
    assert!(second.contains("https://two.example/"), "{screens:#?}");
    // Enter still copies the address of the link with a title.
    let (_, out) = shown_with(&document, 6, &[b"\t", b"\r", b""]);
    assert_eq!(out.matches(COPY_ONE).count(), 1, "{out:?}");
}

#[test]
fn tab_after_a_jump_takes_the_first_stop_on_the_screen_at_the_target() {
    let before: String = (1..=12).map(|n| format!("Before {n}.\n\n")).collect();
    let after: String = (1..=12).map(|n| format!("After {n}.\n\n")).collect();
    let document = crate::typeset::parse::parse(&format!(
        "[go](#target) [other](#nowhere)\n\n{before}# Target\n\n[inside](https://two.example/)\n\n{after}"
    ));
    let (frames, _) = shown_with(&document, 8, &[b"\t", b"\r", b"\t", b""]);
    let screens = replay(&frames);
    let last = screens.last().unwrap();
    assert!(
        top(last).contains("Target"),
        "the view went back: {screens:#?}"
    );
    assert!(
        screen_row(last, 8).contains("https://two.example/"),
        "{screens:#?}"
    );
}

#[test]
fn a_heading_drawn_as_an_image_is_a_target_too() {
    use crate::images::{ImageKey, Source, heading_words};
    use crate::typeset::model::Block;
    let before: String = (1..=12).map(|n| format!("Before {n}.\n\n")).collect();
    let after: String = (1..=12).map(|n| format!("After {n}.\n\n")).collect();
    let document = crate::typeset::parse::parse(&format!(
        "[go](#a-heading)\n\n{before}# A heading\n\nBody\n\n{after}"
    ));
    let Some(Block::Heading { level, content }) = document
        .blocks
        .iter()
        .find(|block| matches!(block, Block::Heading { .. }))
    else {
        panic!("a heading");
    };
    let words = heading_words(content);
    let count = u16::try_from(words.len()).unwrap();
    let mut inputs = Inputs::new(Capabilities {
        text_sizing: false,
        ..kitty()
    });
    let key = inputs.drawn(ImageKey::heading_of(*level, words, (10, 20)).in_document(0));
    let image = image::RgbaImage::from_pixel(u32::from(count) * 40, 20, image::Rgba([9; 4]));
    let text = Source::Text {
        image,
        breaks: (1..count).map(|word| word * 40).collect(),
        ends: (1..=count).map(|word| word * 40 - 10).collect(),
    };
    inputs.images.insert(key, Ok(text));
    let mut terminal = Fake::new(40, 8, &[b"\t", b"\r", BACKSPACE, b""]);
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let screens = replay(&terminal.frames());
    let bar = screen_row(&screens[2], 8);
    assert!(!bar.contains("is not shown"), "{screens:#?}");
    assert!(!top(&screens[2]).contains("go"), "{screens:#?}");
    assert!(top(screens.last().unwrap()).contains("go"), "{screens:#?}");
}
