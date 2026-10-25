use super::*;

fn others() -> Document {
    crate::parse::parse("Other 1\n\nOther 2\n")
}

#[test]
fn shows_the_lines_laid_out_for_the_window() {
    let caps = Capabilities {
        height: Some(6),
        ..Capabilities::ansi(40)
    };
    let frames = show_laid_out(&numbers(10), &others(), caps);
    assert!(frames[0].contains("Other 1"), "{frames:?}");
    assert!(!frames[0].contains("Line 1"), "{frames:?}");
}

#[test]
fn lays_the_document_out_again_for_another_window() {
    let frames = show_laid_out(&numbers(10), &others(), Capabilities::ansi(80));
    assert!(frames[0].contains("Line 1"), "{frames:?}");
    assert!(!frames[0].contains("Other 1"), "{frames:?}");
}

#[test]
fn draws_the_document_and_quits_on_q() {
    let frames = shown(&numbers(10), 40, 6, &[b"q"]);
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert!(frames[0].contains("Line 1"), "{frames:?}");
    assert!(frames[0].contains("Line 3"), "{frames:?}");
    assert!(!frames[0].contains("Line 4"), "{frames:?}");
}

#[test]
fn redraws_after_keys_that_move() {
    // Two lines down, then a key that does nothing, then the end.
    let frames = shown(&numbers(10), 40, 6, &[b"jj", b"x", b"", b"G"]);
    assert_eq!(frames.len(), 3, "{frames:?}");
    // Line 2 is at the top: the first row of the frame.
    assert!(frames[1].contains("\x1b[1;1H  Line 2"), "{frames:?}");
    assert!(frames[2].contains("Line 10"), "{frames:?}");
}

#[test]
fn a_key_split_over_two_reads_moves() {
    // Page Down as an escape character and the rest of its sequence.
    let frames = shown(&numbers(10), 40, 6, &[b"\x1b", b"[6~"]);
    assert_eq!(frames.len(), 2, "{frames:?}");
    assert!(frames[1].contains("\x1b[2;1H  Line 4"), "{frames:?}");
}

#[test]
fn the_status_line_is_on_the_bottom_row() {
    let frames = shown(&numbers(10), 40, 6, &[b"G"]);
    assert_eq!(frames.len(), 2, "{frames:?}");
    // The document takes the rows above it.
    assert!(frames[0].contains("\x1b[5;1H  Line 3"), "{frames:?}");
    assert!(!frames[0].contains("\x1b[6;1H  "), "{frames:?}");
    let status = |frame: &str| frame[frame.find("\x1b[6;1H").unwrap()..].to_owned();
    assert!(status(&frames[0]).contains(" kiri "), "{frames:?}");
    assert!(status(&frames[0]).contains(" notes.md "), "{frames:?}");
    assert!(status(&frames[0]).contains(" help"), "{frames:?}");
    assert!(status(&frames[1]).contains(" kiri "), "{frames:?}");
    assert!(status(&frames[1]).contains("100%"), "{frames:?}");
}

#[test]
fn the_help_opens_over_the_dimmed_document_and_any_key_closes_it() {
    // The help, a key that closes it without moving, the help again,
    // and q, which closes it without quitting, then the end.
    let frames = shown(&numbers(30), 60, 24, &[b"?", b"j", b"h", b"q", b"G"]);
    assert_eq!(frames.len(), 6, "{frames:#?}");
    let help = |frame: &str| frame.contains("down a line");
    assert!(!help(&frames[0]), "{frames:#?}");
    assert!(help(&frames[1]), "{frames:#?}");
    assert!(frames[1].contains("[0;2m  Line 1"), "{frames:#?}");
    assert!(!help(&frames[2]), "{frames:#?}");
    assert!(!frames[2].contains("[0;2m"), "{frames:#?}");
    assert_eq!(frames[0], frames[2]);
    assert!(help(&frames[3]), "{frames:#?}");
    assert_eq!(frames[0], frames[4]);
    assert!(frames[5].contains("100%"), "{frames:#?}");
}

#[test]
fn the_help_s_footer_is_muted_text_on_the_page() {
    let footer = |theme: &str| {
        let mut terminal = Fake::new(60, 24, &[b"?"]);
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        inputs.theme = crate::theme::parse(theme).unwrap();
        let document = numbers(30);
        run(
            Content::Document(&document),
            None,
            "notes.md",
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        )
        .unwrap();
        let help = terminal.frames()[1].clone();
        let end = help.find("Any key closes this.").unwrap();
        help[help[..end].rfind('\x1b').unwrap()..end].to_owned()
    };
    // In the muted colour, on the page's background, as the 256-colour
    // palette has them.
    let page = "background = \"#ffffff\"\nforeground = \"#000000\"\n";
    let muted = footer(&format!("{page}muted = \"#767676\"\n"));
    assert_eq!(muted, "\x1b[0;38;5;243;48;5;231m");
    // Without a muted colour, the text's colour dimmed.
    let dimmed = footer(&format!("{page}muted = \"none\"\n"));
    assert_eq!(dimmed, "\x1b[0;2;38;5;16;48;5;231m");
}

#[test]
fn keys_without_a_binding_close_the_help_too() {
    // F5 closes the help, then j moves.
    let frames = shown(&numbers(30), 60, 24, &[b"?", b"\x1b[15~", b"j"]);
    assert_eq!(frames.len(), 4, "{frames:#?}");
    assert!(frames[1].contains("down a line"), "{frames:#?}");
    assert_eq!(frames[2], frames[0]);
    assert_ne!(frames[3], frames[0]);
}

#[test]
fn ctrl_c_quits_even_from_the_help() {
    let frames = shown(&numbers(30), 60, 24, &[b"?", b"", b"G"]);
    assert_eq!(frames.len(), 2);
}

#[test]
fn a_window_of_one_row_shows_the_document_without_a_status_line() {
    // The help has no room, so q quits at once.
    let frames = shown(&numbers(10), 40, 1, &[b"?", b"q", b"G"]);
    assert_eq!(frames.len(), 1, "{frames:#?}");
    assert!(frames[0].contains("Line 1"), "{frames:#?}");
    assert!(!frames[0].contains(" kiri "), "{frames:#?}");
}

#[test]
fn a_window_shrunk_to_one_row_closes_the_prompt() {
    // Without a status line the prompt cannot show, so G is a
    // binding again and moves to the end.
    let mut terminal = Fake::new(40, 6, &[b"/", b"", b"G"]);
    terminal.sizes = [window(40, 6), window(40, 1)].into();
    show(&numbers(10), &mut terminal);
    let frames = terminal.frames();
    assert!(frames.last().unwrap().contains("Line 10"), "{frames:#?}");
}

#[test]
fn lays_the_document_out_again_for_a_new_window_size() {
    let mut terminal = Fake::new(40, 5, &[b"", b""]);
    terminal.sizes = [window(40, 5), window(8, 5)].into();
    let document = crate::parse::parse("A paragraph of several words\n");
    show(&document, &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 2, "{frames:?}");
    assert!(
        frames[0].contains("A paragraph of several words"),
        "{frames:?}"
    );
    assert!(!frames[1].contains("A paragraph of"), "{frames:?}");
}

#[test]
fn a_heading_taller_than_the_window_is_drawn_at_normal_size() {
    let mut terminal = Fake::new(40, 3, &[b"G"]);
    let document = crate::parse::parse("# Title\n\nText\n");
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    run(
        Content::Document(&document),
        None,
        "notes.md",
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    )
    .unwrap();
    let frames = terminal.frames();
    assert!(frames[0].contains("Title"), "{frames:?}");
    assert!(!frames[0].contains("\x1b]66;"), "{frames:?}");
    // The last page shows the text below it.
    assert!(frames.last().unwrap().contains("Text"), "{frames:?}");
}

#[test]
fn a_given_width_stays_when_the_window_changes() {
    let mut terminal = Fake::new(40, 5, &[b""]);
    terminal.sizes = [window(80, 5)].into();
    let document = crate::parse::parse("A paragraph of several words\n");
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    run(
        Content::Document(&document),
        None,
        "notes.md",
        &mut inputs,
        Options {
            width: Some(10),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    )
    .unwrap();
    // One frame before the window changed, and one after.
    assert_eq!(terminal.frames().len(), 2);
    for frame in terminal.frames() {
        assert!(!frame.contains("A paragraph of"), "{frame:?}");
    }
}
