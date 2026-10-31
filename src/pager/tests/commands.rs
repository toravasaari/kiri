//! What the keys do together: how Escape answers whatever is open, and
//! which way the sideways keys scroll. They hold the pager's answers
//! the same while keys and commands are data.

use std::rc::Rc;

use super::*;
use crate::typeset::reading::Fonts;
use crate::typeset::reading::engine::Reading;

fn kitty() -> Capabilities {
    Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    }
}

/// The frames drawn for `files`, in a window of 60 × 6, when the
/// terminal gives `reads`.
fn files_shown(files: &[File], reads: &[&[u8]]) -> Vec<String> {
    let mut terminal = Fake::new(60, 6, reads);
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    run(
        Content::Files(files),
        None,
        "",
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    )
    .unwrap();
    terminal.frames()
}

fn named(files: &[(&str, usize)]) -> Vec<File> {
    (files.iter())
        .map(|&(name, count)| File {
            name: name.to_owned(),
            document: numbers(count),
        })
        .collect()
}

#[test]
fn escape_closes_the_help_and_leaves_the_search_shown() {
    let frames = shown(&numbers(30), 60, 6, &[b"/line 2\r", b"?", b"\x1b", b""]);
    assert_eq!(frames.len(), 4, "{frames:#?}");
    assert!(frames[2].contains(" Keys "), "{frames:#?}");
    assert!(!frames[3].contains(" Keys "), "{frames:#?}");
    assert!(
        status_row(&frames[3], 6).contains("/line 2 1/11"),
        "{frames:#?}"
    );
}

#[test]
fn escape_closes_the_prompt_and_leaves_the_search_shown() {
    let frames = shown(&numbers(30), 60, 6, &[b"/line 2\r", b"/x", b"\x1b", b""]);
    let last = frames.last().unwrap();
    assert!(!status_row(last, 6).contains('\u{258f}'), "{frames:#?}");
    assert!(status_row(last, 6).contains("/line 2 1/11"), "{frames:#?}");
}

#[test]
fn escape_closes_the_colon_and_leaves_the_search_shown() {
    // The n after the closed colon is a search's, not :n's.
    let reads: &[&[u8]] = &[b"/line 2\r", b":", b"\x1b", b"", b"n"];
    let frames = files_shown(&named(&[("a.md", 3), ("b.md", 3)]), reads);
    let last = frames.last().unwrap();
    assert!(status_row(last, 6).contains("/line 2 2/2"), "{frames:#?}");
    assert!(status_row(last, 6).contains(" b.md "), "{frames:#?}");
}

#[test]
fn escape_with_nothing_open_clears_the_marks_and_the_selected_link_together() {
    let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
    let mut terminal = Fake::new(60, 6, &[b"/link\r", b"\t", b"\x1b", b""]);
    let mut inputs = Inputs::new(kitty());
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    let bar = |frame: &String| status_row(frame, 6);
    assert!(
        bar(&frames[2]).contains("https://one.example/"),
        "{frames:#?}"
    );
    assert!(bar(&frames[1]).contains("/link"), "{frames:#?}");
    let last = frames.last().unwrap();
    assert!(!bar(last).contains("https://one.example/"), "{frames:#?}");
    assert!(!bar(last).contains("/link"), "{frames:#?}");
}

#[test]
fn escape_does_not_forget_the_last_move_to_a_file() {
    // c.md is one line, so the view stops above its start, in b.md; the
    // move to c.md is why the next :n has no file to go to.
    let reads: &[&[u8]] = &[b":n", b":n", b"\x1b", b"", b":n"];
    let frames = files_shown(&named(&[("a.md", 3), ("b.md", 10), ("c.md", 1)]), reads);
    let last = status_row(frames.last().unwrap(), 6);
    assert!(last.contains(" no next file "), "{frames:#?}");
}

/// The frames drawn for a wide code block scrolled in reading mode, in a
/// window of 80 × 10, when the terminal gives `reads`.
fn band_frames(reads: &[&[u8]]) -> Vec<String> {
    let long = "a".repeat(200);
    let document = crate::typeset::parse::parse(&format!("```\n{long}\n```\n"));
    let mut terminal = Fake::new(80, 10, reads);
    let mut inputs = Inputs {
        reading: Some(Reading {
            pictures: Default::default(),
            fonts: Rc::new(Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Scroll,
        }),
        ..Inputs::new(kitty())
    };
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    terminal.frames()
}

#[test]
fn the_sideways_keys_scroll_the_way_they_point() {
    // The band starts at its left end: a key to the left has nowhere to
    // go and draws nothing, one to the right draws the band again, and
    // the opposite key then draws it again, moved back.
    let drawn = |reads: &[&[u8]]| band_frames(reads).len();
    assert_eq!(drawn(&[b"\x1b[D"]), 1);
    assert_eq!(drawn(&[b"H"]), 1);
    assert_eq!(drawn(&[b"\x1b[C"]), 2);
    assert_eq!(drawn(&[b"L"]), 2);
    for (there, back) in [(&b"\x1b[C"[..], &b"\x1b[D"[..]), (b"L", b"H")] {
        let frames = band_frames(&[there, back]);
        assert_eq!(frames.len(), 3, "{frames:#?}");
        // The image the band is drawn as; the ids around it change.
        let image = |frame: &str| frame.split("m=0;").nth(1).map(str::to_owned);
        assert!(image(&frames[0]).is_some(), "{frames:#?}");
        assert_ne!(image(&frames[1]), image(&frames[0]));
        assert_ne!(image(&frames[2]), image(&frames[1]));
    }
}
