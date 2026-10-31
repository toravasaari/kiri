use super::*;

/// The files `a.md` of 3 lines and `b.md` of 10, as [`numbers`] has
/// them. In a stream, `a.md` takes lines 0 to 8, its header and blank
/// line included, and `b.md` starts on line 10.
fn two_files() -> Vec<File> {
    [("a.md", 3), ("b.md", 10)]
        .into_iter()
        .map(|(name, count)| File {
            name: name.to_owned(),
            document: numbers(count),
        })
        .collect()
}

/// Shows `files` on `terminal` as the binary does, without a name of
/// its own.
fn show_files(files: &[File], terminal: &mut Fake) {
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    run(
        Content::Files(files),
        None,
        Session::new(files.iter().map(|file| file.name.as_str())),
        &mut inputs,
        Options::new(SearchCase::Smart),
        terminal,
    )
    .unwrap();
}

#[test]
fn the_status_line_takes_the_names_and_the_count_from_the_session() {
    // The documents carry other names than the session's sources, which
    // are the ones shown.
    let mut terminal = Fake::new(40, 6, &[b":n"]);
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    let session = Session::new(["x.md", "y.md"]);
    let files = two_files();
    let options = Options::new(SearchCase::Smart);
    run(
        Content::Files(&files),
        None,
        session,
        &mut inputs,
        options,
        &mut terminal,
    )
    .unwrap();
    let frames = terminal.frames();
    assert!(
        status_row(&frames[0], 6).contains(" x.md 1/2 "),
        "{frames:#?}"
    );
    assert!(
        status_row(&frames[1], 6).contains(" y.md 2/2 "),
        "{frames:#?}"
    );
}

#[test]
fn the_status_line_names_the_file_at_the_top_of_the_view() {
    // Down to the blank line above b.md's header, onto the header, and
    // back up.
    let mut terminal = Fake::new(40, 6, &[b"jjjjjjjjj", b"j", b"k"]);
    show_files(&two_files(), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 4, "{frames:#?}");
    let names = |frame: &str| status_row(frame, 6).contains(" a.md ");
    assert!(names(&frames[0]), "{frames:#?}");
    assert!(names(&frames[1]), "{frames:#?}");
    // b.md's header at the top: an empty row of its band, then its name.
    assert!(
        unstyled(&frames[2]).contains("\x1b[2;1H▌ b.md"),
        "{frames:#?}"
    );
    assert!(status_row(&frames[2], 6).contains(" b.md "), "{frames:#?}");
    assert!(!names(&frames[2]), "{frames:#?}");
    assert!(names(&frames[3]), "{frames:#?}");
}

#[test]
fn the_status_line_tells_the_file_s_place_among_the_files() {
    let mut terminal = Fake::new(40, 6, &[b":n"]);
    show_files(&two_files(), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 2, "{frames:#?}");
    assert!(
        status_row(&frames[0], 6).contains(" a.md 1/2 "),
        "{frames:#?}"
    );
    assert!(
        status_row(&frames[1], 6).contains(" b.md 2/2 "),
        "{frames:#?}"
    );
    // A lone document has no place among others.
    let frames = shown(&numbers(3), 40, 6, &[]);
    let status = status_row(&frames[0], 6);
    assert!(status.contains(" notes.md  "), "{status:?}");
    assert!(!status.contains("1/1"), "{status:?}");
}

/// Files called `name` with `count` lines each, as [`numbers`] has
/// them.
fn files(counts: &[(&str, usize)]) -> Vec<File> {
    counts
        .iter()
        .map(|&(name, count)| File {
            name: name.to_owned(),
            document: numbers(count),
        })
        .collect()
}

/// Whether `frame` shows the header of `name` at the top of the view
/// of `rows` rows, and `name` on the status line below it.
fn at_file(frame: &str, name: &str, rows: u16) -> bool {
    unstyled(frame).contains(&format!("\x1b[2;1H▌ {name}\x1b"))
        && status_row(frame, rows + 1).contains(&format!(" {name} "))
}

#[test]
fn colon_n_and_colon_p_move_to_the_start_of_the_next_and_previous_file() {
    // b.md starts on line 10 and c.md on line 34. The colon on its own,
    // then n; :n at the last file, which only tells there is none; :p
    // from the start of c.md, and from inside b.md, first to its start.
    let reads: &[&[u8]] = &[b":", b"n", b":n", b":n", b":p", b"jjj", b":p", b":p"];
    let mut terminal = Fake::new(40, 6, reads);
    show_files(
        &files(&[("a.md", 3), ("b.md", 10), ("c.md", 10)]),
        &mut terminal,
    );
    let frames = terminal.frames();
    assert_eq!(frames.len(), 9, "{frames:#?}");
    assert!(at_file(&frames[0], "a.md", 5), "{frames:#?}");
    assert!(
        status_row(&frames[1], 6).contains(" :\u{258f}"),
        "{frames:#?}"
    );
    assert!(at_file(&frames[2], "b.md", 5), "{frames:#?}");
    assert!(at_file(&frames[3], "c.md", 5), "{frames:#?}");
    assert_eq!(document_part(&frames[4], 6), document_part(&frames[3], 6));
    assert!(at_file(&frames[5], "b.md", 5), "{frames:#?}");
    assert!(status_row(&frames[6], 6).contains(" b.md "), "{frames:#?}");
    assert!(at_file(&frames[7], "b.md", 5), "{frames:#?}");
    assert!(at_file(&frames[8], "a.md", 5), "{frames:#?}");
}

#[test]
fn a_new_window_size_moves_the_blank_line_between_files_to_the_next_file() {
    // Line 9 is the blank line between a.md and b.md.
    let reads: &[&[u8]] = &[b"jjjjjjjjj", b""];
    let mut terminal = Fake::new(40, 6, reads);
    terminal.sizes = [window(40, 6), window(50, 6)].into();
    show_files(&two_files(), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 3, "{frames:#?}");
    assert!(at_file(&frames[2], "b.md", 5), "{frames:#?}");
}

#[test]
fn shift_page_down_and_shift_page_up_move_as_colon_n_and_colon_p_do() {
    // As in the test above: past the last file, Shift+PgDn only tells
    // there is none; Shift+PgUp from inside b.md goes first to its
    // start.
    let (down, up): (&[u8], &[u8]) = (b"\x1b[6;2~", b"\x1b[5;2~");
    let reads = &[down, down, down, up, b"jjj", up, up];
    let mut terminal = Fake::new(40, 6, reads);
    show_files(
        &files(&[("a.md", 3), ("b.md", 10), ("c.md", 10)]),
        &mut terminal,
    );
    let frames = terminal.frames();
    assert_eq!(frames.len(), 8, "{frames:#?}");
    assert!(at_file(&frames[0], "a.md", 5), "{frames:#?}");
    assert!(at_file(&frames[1], "b.md", 5), "{frames:#?}");
    assert!(at_file(&frames[2], "c.md", 5), "{frames:#?}");
    assert!(
        status_row(&frames[3], 6).contains("no next file"),
        "{frames:#?}"
    );
    assert!(at_file(&frames[4], "b.md", 5), "{frames:#?}");
    assert!(status_row(&frames[5], 6).contains(" b.md "), "{frames:#?}");
    assert!(at_file(&frames[6], "b.md", 5), "{frames:#?}");
    assert!(at_file(&frames[7], "a.md", 5), "{frames:#?}");
}

#[test]
fn colon_n_names_a_file_on_the_last_page_though_the_view_stops_short() {
    // In 13 rows, with the margin below it, the last page starts at b.md,
    // the line 24, and also shows c.md, which starts on line 30.
    let reads: &[&[u8]] = &[b":n", b":n", b":p", b":n", b"k"];
    let mut terminal = Fake::new(40, 14, reads);
    show_files(
        &files(&[("a.md", 10), ("b.md", 1), ("c.md", 1)]),
        &mut terminal,
    );
    let frames = terminal.frames();
    assert_eq!(frames.len(), 6, "{frames:#?}");
    let named = |frame: &str, name: &str| status_row(frame, 14).contains(&format!(" {name} "));
    assert!(at_file(&frames[1], "b.md", 13), "{frames:#?}");
    assert!(named(&frames[2], "c.md"), "{frames:#?}");
    // The view stays at b.md's header.
    assert!(
        unstyled(&frames[2]).contains("\x1b[2;1H▌ b.md\x1b"),
        "{frames:#?}"
    );
    assert!(at_file(&frames[3], "b.md", 13), "{frames:#?}");
    assert!(named(&frames[4], "c.md"), "{frames:#?}");
    // Once the view moves, the file at its top again.
    assert!(named(&frames[5], "a.md"), "{frames:#?}");
}

#[test]
fn colon_n_still_names_its_file_after_a_new_window_size() {
    // As above, but the window then grows by two rows, so the last
    // page starts higher up, and the view still stops short of c.md's
    // header.
    let reads: &[&[u8]] = &[b":n", b":n", b""];
    let mut terminal = Fake::new(40, 14, reads);
    terminal.sizes = [window(40, 14), window(40, 14), window(40, 16)].into();
    show_files(
        &files(&[("a.md", 10), ("b.md", 1), ("c.md", 1)]),
        &mut terminal,
    );
    let frames = terminal.frames();
    assert_eq!(frames.len(), 4, "{frames:#?}");
    let named =
        |frame: &str, name: &str, rows| status_row(frame, rows).contains(&format!(" {name} "));
    assert!(named(&frames[2], "c.md", 14), "{frames:#?}");
    assert!(named(&frames[3], "c.md", 16), "{frames:#?}");
}

#[test]
fn a_key_after_a_new_window_size_makes_colon_n_forget_its_file() {
    // As above, then k: the view moves, so the status line names the
    // file above again.
    let reads: &[&[u8]] = &[b":n", b":n", b"", b"k"];
    let mut terminal = Fake::new(40, 14, reads);
    terminal.sizes = [window(40, 14), window(40, 14), window(40, 16)].into();
    show_files(
        &files(&[("a.md", 10), ("b.md", 1), ("c.md", 1)]),
        &mut terminal,
    );
    let frames = terminal.frames();
    assert_eq!(frames.len(), 5, "{frames:#?}");
    let named = |frame: &str, name: &str| status_row(frame, 16).contains(&format!(" {name} "));
    assert!(named(&frames[3], "c.md"), "{frames:#?}");
    assert!(!named(&frames[4], "c.md"), "{frames:#?}");
}

#[test]
fn a_key_after_the_colon_that_is_no_command_only_closes_it() {
    let reads: &[&[u8]] = &[b":", b"j", b":", b"\x1b", b"", b"j"];
    let mut terminal = Fake::new(40, 6, reads);
    show_files(&two_files(), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 6, "{frames:#?}");
    // j did not move, and the colon is gone.
    assert_eq!(frames[2], frames[0]);
    assert_eq!(frames[4], frames[0]);
    // Down a line, onto the name in a.md's header.
    let top = "\x1b[1;1H▌ a.md\x1b";
    assert!(unstyled(&frames[5]).contains(top), "{frames:#?}");
}

#[test]
fn at_the_ends_of_the_files_the_status_line_tells_there_is_none_until_the_next_key() {
    // :p at the very start, then j; :n to b.md and :n again at the
    // last file, then k; Shift+PgDn back to b.md and once more.
    let down: &[u8] = b"\x1b[6;2~";
    let reads: &[&[u8]] = &[b":p", b"j", b":n", b":n", b"k", down, down];
    let mut terminal = Fake::new(40, 6, reads);
    show_files(&two_files(), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 8, "{frames:#?}");
    let tells = |frame: &str, text: &str| status_row(frame, 6).contains(text);
    assert!(tells(&frames[1], " no previous file "), "{frames:#?}");
    // The view did not move.
    assert_eq!(document_part(&frames[1], 6), document_part(&frames[0], 6));
    assert!(!tells(&frames[2], "no previous file"), "{frames:#?}");
    assert!(at_file(&frames[3], "b.md", 5), "{frames:#?}");
    assert!(!tells(&frames[3], "no next file"), "{frames:#?}");
    assert!(tells(&frames[4], " no next file "), "{frames:#?}");
    assert!(at_file(&frames[4], "b.md", 5), "{frames:#?}");
    assert!(!tells(&frames[5], "no next file"), "{frames:#?}");
    assert!(at_file(&frames[6], "b.md", 5), "{frames:#?}");
    assert!(tells(&frames[7], " no next file "), "{frames:#?}");
    assert!(at_file(&frames[7], "b.md", 5), "{frames:#?}");
}

#[test]
fn a_colon_in_a_lone_document_does_nothing_and_lets_the_next_key_through() {
    // :n there is n, which goes on to the next match.
    let with_colon: &[&[u8]] = &[b":", b"j", b"/Line\r", b":n", b":"];
    let without: &[&[u8]] = &[b"j", b"/Line\r", b"n"];
    let frames = |reads| shown(&numbers(30), 40, 6, reads);
    let shown = frames(with_colon);
    assert_eq!(shown, frames(without));
    assert_eq!(shown.len(), 4, "{shown:#?}");
}

#[test]
fn the_help_lists_colon_n_and_colon_p_only_with_several_files() {
    let mut terminal = Fake::new(60, 24, &[b"?"]);
    show_files(&two_files(), &mut terminal);
    let help = terminal.frames()[1].clone();
    assert!(help.contains(":n"), "{help:?}");
    assert!(help.contains("next file"), "{help:?}");
    assert!(help.contains(":p"), "{help:?}");
    assert!(help.contains("previous file"), "{help:?}");
    assert!(help.contains("Shift+PgDn"), "{help:?}");
    assert!(help.contains("Shift+PgUp"), "{help:?}");
    let help = shown(&numbers(30), 60, 24, &[b"?"])[1].clone();
    assert!(help.contains("down a line"), "{help:?}");
    assert!(!help.contains("next file"), "{help:?}");
    assert!(!help.contains("Shift+PgDn"), "{help:?}");
}
