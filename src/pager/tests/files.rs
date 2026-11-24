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
        &mut Loads::new(),
        terminal,
    )
    .unwrap();
}

#[test]
fn the_status_line_names_the_files_after_the_sources_of_the_session() {
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
        &mut Loads::new(),
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
#[should_panic(expected = "the session has 1 sources but the content has 2 files")]
fn a_session_with_fewer_sources_than_files_is_refused_at_the_start() {
    let mut terminal = Fake::new(40, 6, &[b"jjjjjjjjjj"]);
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    let files = two_files();
    let options = Options::new(SearchCase::Smart);
    let session = Session::new(["only.md"]);
    let _ = run(
        Content::Files(&files),
        None,
        session,
        &mut inputs,
        options,
        &mut Loads::new(),
        &mut terminal,
    );
}

#[test]
#[should_panic(expected = "a lone document needs a source in the session")]
fn a_lone_document_with_an_empty_session_is_refused_at_the_start() {
    let mut terminal = Fake::new(40, 6, &[b"q"]);
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    let options = Options::new(SearchCase::Smart);
    let session = Session::new(Vec::<String>::new());
    let document = numbers(3);
    let content = Content::Document(&document);
    let _ = run(
        content,
        None,
        session,
        &mut inputs,
        options,
        &mut Loads::new(),
        &mut terminal,
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

/// Whether `screen` shows the header of `name` at the top of the view
/// of `rows` rows, and `name` on the status line below it.
fn at_file(screen: &[String], name: &str, rows: u16) -> bool {
    unstyled(screen_row(screen, 2)).starts_with(&format!("▌ {name}"))
        && screen_row(screen, rows + 1).contains(&format!(" {name} "))
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
    let screens = terminal.screens();
    assert_eq!(screens.len(), 9, "{screens:#?}");
    assert!(at_file(&screens[0], "a.md", 5), "{screens:#?}");
    assert!(
        screen_row(&screens[1], 6).contains(" :\u{258f}"),
        "{screens:#?}"
    );
    assert!(at_file(&screens[2], "b.md", 5), "{screens:#?}");
    assert!(at_file(&screens[3], "c.md", 5), "{screens:#?}");
    assert_eq!(screen_above(&screens[4], 6), screen_above(&screens[3], 6));
    assert!(at_file(&screens[5], "b.md", 5), "{screens:#?}");
    assert!(
        screen_row(&screens[6], 6).contains(" b.md "),
        "{screens:#?}"
    );
    assert!(at_file(&screens[7], "b.md", 5), "{screens:#?}");
    assert!(at_file(&screens[8], "a.md", 5), "{screens:#?}");
}

#[test]
fn a_new_window_size_moves_the_blank_line_between_files_to_the_next_file() {
    // Line 9 is the blank line between a.md and b.md.
    let reads: &[&[u8]] = &[b"jjjjjjjjj", b""];
    let mut terminal = Fake::new(40, 6, reads);
    terminal.sizes = [window(40, 6), window(50, 6)].into();
    show_files(&two_files(), &mut terminal);
    let screens = terminal.screens();
    assert_eq!(screens.len(), 3, "{screens:#?}");
    assert!(at_file(&screens[2], "b.md", 5), "{screens:#?}");
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
    let screens = terminal.screens();
    assert_eq!(screens.len(), 8, "{screens:#?}");
    assert!(at_file(&screens[0], "a.md", 5), "{screens:#?}");
    assert!(at_file(&screens[1], "b.md", 5), "{screens:#?}");
    assert!(at_file(&screens[2], "c.md", 5), "{screens:#?}");
    assert!(
        screen_row(&screens[3], 6).contains("no next file"),
        "{screens:#?}"
    );
    assert!(at_file(&screens[4], "b.md", 5), "{screens:#?}");
    assert!(
        screen_row(&screens[5], 6).contains(" b.md "),
        "{screens:#?}"
    );
    assert!(at_file(&screens[6], "b.md", 5), "{screens:#?}");
    assert!(at_file(&screens[7], "a.md", 5), "{screens:#?}");
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
    let screens = terminal.screens();
    assert_eq!(screens.len(), 6, "{screens:#?}");
    let named =
        |screen: &[String], name: &str| screen_row(screen, 14).contains(&format!(" {name} "));
    assert!(at_file(&screens[1], "b.md", 13), "{screens:#?}");
    assert!(named(&screens[2], "c.md"), "{screens:#?}");
    // The view stays at b.md's header.
    assert!(
        unstyled(screen_row(&screens[2], 2)).starts_with("▌ b.md"),
        "{screens:#?}"
    );
    assert!(at_file(&screens[3], "b.md", 13), "{screens:#?}");
    assert!(named(&screens[4], "c.md"), "{screens:#?}");
    // Once the view moves, the file at its top again.
    assert!(named(&screens[5], "a.md"), "{screens:#?}");
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
    let screens = terminal.screens();
    assert_eq!(screens.len(), 6, "{screens:#?}");
    // j did not move, and the colon is gone.
    assert_eq!(screens[2], screens[0]);
    assert_eq!(screens[4], screens[0]);
    // Down a line, onto the name in a.md's header.
    let top = "▌ a.md";
    assert!(
        unstyled(screen_row(&screens[5], 1)).starts_with(top),
        "{screens:#?}"
    );
}

#[test]
fn a_click_while_the_colon_waits_reaches_the_document_and_the_colon_stays() {
    // The click goes to the document, which has nothing under it, and
    // does not close the colon: the n after it is `:n`, to the next file.
    let click: &[u8] = b"\x1b[<0;10;3M\x1b[<0;10;3m";
    let mut terminal = Fake::new(40, 6, &[b":", click, b"n"]);
    show_files(&two_files(), &mut terminal);
    let screens = terminal.screens();
    assert!(at_file(screens.last().unwrap(), "b.md", 5), "{screens:#?}");
}

#[test]
fn the_wheel_with_shift_while_the_colon_waits_only_closes_the_colon() {
    // The wheel with Shift becomes a move, and the colon takes that as
    // its key: it closes, the view does not move, and n is the search's.
    let wheel: &[u8] = b"\x1b[<69;10;3M";
    let mut terminal = Fake::new(40, 6, &[b":", wheel, b"n"]);
    show_files(&two_files(), &mut terminal);
    let screens = terminal.screens();
    assert_eq!(screens.len(), 3, "{screens:#?}");
    assert_eq!(screens[2], screens[0], "{screens:#?}");
}

#[test]
fn a_colon_then_a_question_mark_opens_no_help() {
    let mut terminal = Fake::new(60, 24, &[b":", b"?"]);
    show_files(&two_files(), &mut terminal);
    let screens = terminal.screens();
    assert!(
        !screen_text(screens.last().unwrap()).contains("down a line"),
        "{screens:#?}"
    );
}

#[test]
fn ctrl_c_after_a_colon_does_not_quit() {
    // Ctrl+C only closes the colon; the q after it quits.
    let mut terminal = Fake::new(40, 6, &[b":", b"\x03", b"j", b"q"]);
    show_files(&two_files(), &mut terminal);
    let screens = terminal.screens();
    assert_eq!(screens.len(), 4, "{screens:#?}");
}

#[test]
fn at_the_ends_of_the_files_the_status_line_tells_there_is_none_until_the_next_key() {
    // :p at the very start, then j; :n to b.md and :n again at the
    // last file, then k; Shift+PgDn back to b.md and once more.
    let down: &[u8] = b"\x1b[6;2~";
    let reads: &[&[u8]] = &[b":p", b"j", b":n", b":n", b"k", down, down];
    let mut terminal = Fake::new(40, 6, reads);
    show_files(&two_files(), &mut terminal);
    let screens = terminal.screens();
    assert_eq!(screens.len(), 8, "{screens:#?}");
    let tells = |screen: &[String], text: &str| screen_row(screen, 6).contains(text);
    assert!(tells(&screens[1], " no previous file "), "{screens:#?}");
    // The view did not move.
    assert_eq!(screen_above(&screens[1], 6), screen_above(&screens[0], 6));
    assert!(!tells(&screens[2], "no previous file"), "{screens:#?}");
    assert!(at_file(&screens[3], "b.md", 5), "{screens:#?}");
    assert!(!tells(&screens[3], "no next file"), "{screens:#?}");
    assert!(tells(&screens[4], " no next file "), "{screens:#?}");
    assert!(at_file(&screens[4], "b.md", 5), "{screens:#?}");
    assert!(!tells(&screens[5], "no next file"), "{screens:#?}");
    assert!(at_file(&screens[6], "b.md", 5), "{screens:#?}");
    assert!(tells(&screens[7], " no next file "), "{screens:#?}");
    assert!(at_file(&screens[7], "b.md", 5), "{screens:#?}");
}

#[test]
fn a_colon_in_a_lone_document_lets_a_key_that_is_no_command_for_it_through() {
    // The colon shows on the status line, and the key after it acts as it
    // would alone: :n there is n, which goes on to the next match, as a
    // lone document has no next file.
    let with_colon: &[&[u8]] = &[b":", b"j", b"/Line\r", b":n", b":"];
    let without: &[&[u8]] = &[b"j", b"/Line\r", b"n"];
    let frames = |reads| shown(&numbers(30), 40, 6, reads);
    let shown = frames(with_colon);
    let (colons, rest): (Vec<String>, Vec<String>) =
        shown.into_iter().partition(|frame| frame.contains(" :▏"));
    // `:n` comes in one read, so only the first colon and the last one
    // show.
    assert_eq!(colons.len(), 2, "{colons:#?}");
    // Without the colons, the frames are those of the keys alone.
    assert_eq!(rest, frames(without), "{rest:#?}");
}

#[test]
fn the_help_lists_the_keys_between_files_with_one_file_too() {
    // Scrolled to the Files topic, the help of one document has it.
    let mut terminal = Fake::new(60, 90, &[b"?"]);
    show(&numbers(30), &mut terminal);
    let help = terminal.screens().pop().unwrap();
    let help = screen_text(&help);
    for text in [
        "Files",
        ":n",
        "next file",
        ":p",
        "previous file",
        "Shift+PgDn",
    ] {
        assert!(help.contains(text), "{text}: {help}");
    }
}

#[test]
fn the_help_opens_at_the_files_with_several_files() {
    let mut terminal = Fake::new(60, 24, &[b"?"]);
    show_files(&two_files(), &mut terminal);
    let screens = terminal.screens();
    // The topic is on screen, and the help is no further down than its
    // last page lets it be.
    let text = screen_text(&screens[1]);
    assert!(
        text.contains("Files") && text.contains(":n Shift+PgDn"),
        "{screens:#?}"
    );
    assert!(!text.contains("Searching"), "{screens:#?}");
}

#[test]
fn the_help_of_several_files_fits_a_window_of_24_rows_by_scrolling() {
    // The top of the help, and then its end.
    let mut terminal = Fake::new(60, 24, &[b"?", b"g", b"G"]);
    show_files(&two_files(), &mut terminal);
    let screens = terminal.screens();
    for screen in &screens[1..] {
        let text = screen_text(screen);
        assert!(text.contains(" Help "), "{screens:#?}");
    }
    let text = |at: usize| screen_text(&screens[at]);
    assert!(!text(2).contains("this help"), "{screens:#?}");
    assert!(text(3).contains("this help"), "{screens:#?}");
}

/// `docs/a.md`, which has `link`, and `docs/b.md` with a heading
/// `# Second` far down, shown in a window of 40 × 6 on kitty, with paths,
/// when the terminal gives `keys`.
fn linked(link: &str, keys: &[&[u8]]) -> Vec<Vec<String>> {
    let filler: String = (1..=10).map(|n| format!("Filler {n}.\n\n")).collect();
    let files = [
        File {
            name: "docs/a.md".to_owned(),
            document: crate::typeset::parse::parse(&format!("{link}\n\nAlpha.\n")),
        },
        File {
            name: "docs/b.md".to_owned(),
            document: crate::typeset::parse::parse(&format!(
                "Bravo.\n\n{filler}# Second\n\nUnder it.\n\n{filler}"
            )),
        },
    ];
    let mut session = Session::new(files.iter().map(|file| file.name.as_str()));
    for (number, file) in files.iter().enumerate() {
        session.came_from(session.sources()[number].id(), file.name.clone().into());
    }
    let mut terminal = Fake::new(40, 6, keys);
    let caps = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    };
    let mut inputs = Inputs::new(caps);
    let options = Options::new(SearchCase::Smart);
    let loads = &mut Loads::new();
    run(
        Content::Files(&files),
        None,
        session,
        &mut inputs,
        options,
        loads,
        &mut terminal,
    )
    .unwrap();
    terminal.screens()
}

fn text_above_the_bar(screen: &[String]) -> String {
    unstyled(&screen_above(screen, 6))
}

#[test]
fn a_link_to_another_shown_file_jumps_to_it_and_backspace_comes_back() {
    let screens = linked("[b](b.md#second)", &[b"\t", b"\r", b"\x7f", b""]);
    assert!(screens.len() >= 4, "{screens:#?}");
    // The heading lands below the sticky row, on the second row.
    assert!(names(&screens[2], 1, "docs/b.md"), "{screens:#?}");
    assert!(row_text(&screens[2], 2).contains("Second"), "{screens:#?}");
    assert!(
        screen_row(&screens[2], 6).contains("docs/b.md"),
        "{screens:#?}"
    );
    let last = screens.last().unwrap();
    assert_eq!(
        text_above_the_bar(last),
        text_above_the_bar(&screens[0]),
        "{screens:#?}"
    );
    // The jump let the link go: Tab goes on from the target.
    assert!(!screen_row(last, 6).contains("b.md#second"), "{screens:#?}");
    assert!(screen_row(last, 6).contains("docs/a.md"), "{screens:#?}");
}

#[test]
fn a_link_to_a_shown_file_without_an_id_goes_to_its_start() {
    let screens = linked("[b](./b.md)", &[b"\t", b"\r", b""]);
    let text = screen_text(screens.last().unwrap());
    assert!(text.contains("docs/b.md"), "{screens:#?}");
    assert!(text.contains("Bravo."), "{screens:#?}");
}

#[test]
fn an_anchor_is_looked_for_in_the_link_s_own_file() {
    // `# Second` is in b.md only: a.md has none.
    let screens = linked("[x](#second)", &[b"\t", b"\r", b""]);
    let last = screens.last().unwrap();
    assert!(
        screen_row(last, 6).contains("no #second in docs/a.md"),
        "{screens:#?}"
    );
    assert!(
        text_above_the_bar(last).contains("[x]") || text_above_the_bar(last).contains('x'),
        "{screens:#?}"
    );
}

#[test]
fn a_link_to_a_file_that_is_not_markdown_is_still_copied() {
    let screens = linked("[o](other.txt)", &[b"\t", b"\r", b""]);
    let last = screens.last().unwrap();
    assert!(screen_row(last, 6).contains("link copied"), "{screens:#?}");
}

/// The text of row `row`, from 1, of `screen`, without styles and
/// without the band's trailing spaces.
fn row_text(screen: &[String], row: u16) -> String {
    unstyled(screen_row(screen, row)).trim_end().to_owned()
}

/// Whether row `row` of `screen` is the empty row of a header's band: its
/// bar and nothing else.
fn empty_band(screen: &[String], row: u16) -> bool {
    row_text(screen, row) == "▌"
}

/// Whether row `row` of `screen` shows the name `name` as a header's name
/// row does.
fn names(screen: &[String], row: u16, name: &str) -> bool {
    row_text(screen, row) == format!("▌ {name}")
}

#[test]
fn a_header_scrolled_past_leaves_its_name_on_the_top_row() {
    // b.md's header is on lines 10 to 12, a blank line below it. Down to
    // the header's top row, one row at a time, then back up.
    let reads: &[&[u8]] = &[b"jjjjjjjjjj", b"j", b"j", b"j", b"k", b"k", b"k"];
    let mut terminal = Fake::new(40, 7, reads);
    show_files(&two_files(), &mut terminal);
    let screens = terminal.screens();
    assert_eq!(screens.len(), 8, "{screens:#?}");
    // The header fully in view: an empty row, the name, an empty row.
    let screen = &screens[1];
    assert!(empty_band(screen, 1), "{screens:#?}");
    assert!(names(screen, 2, "b.md"), "{screens:#?}");
    assert!(empty_band(screen, 3), "{screens:#?}");
    // Its top row just above the view: the name and the empty row below
    // it, with nothing drawn over.
    let screen = &screens[2];
    assert!(names(screen, 1, "b.md"), "{screens:#?}");
    assert!(empty_band(screen, 2), "{screens:#?}");
    // One row further: the name stays on the top row.
    let screen = &screens[3];
    assert!(names(screen, 1, "b.md"), "{screens:#?}");
    assert_eq!(row_text(screen, 2), "", "{screens:#?}");
    // Further down, in the file's text.
    let screen = &screens[4];
    assert!(names(screen, 1, "b.md"), "{screens:#?}");
    assert_eq!(row_text(screen, 2), "  Line 1", "{screens:#?}");
    // And back: the name on the top row, then the empty row returns
    // above the name.
    assert!(names(&screens[5], 1, "b.md"), "{screens:#?}");
    assert!(names(&screens[6], 1, "b.md"), "{screens:#?}");
    assert!(empty_band(&screens[6], 2), "{screens:#?}");
    assert!(empty_band(&screens[7], 1), "{screens:#?}");
    assert!(names(&screens[7], 2, "b.md"), "{screens:#?}");
}

#[test]
fn the_next_files_header_replaces_the_sticky_name_when_it_reaches_the_top() {
    // b.md has 3 lines, so c.md's header starts on line 20, after the
    // blank line 19 that is b.md's.
    let reads: &[&[u8]] = &[b"jjjjjjjjjjjjjjjjjjj", b"j", b"j", b"j"];
    let mut terminal = Fake::new(40, 7, reads);
    show_files(
        &files(&[("a.md", 3), ("b.md", 3), ("c.md", 10)]),
        &mut terminal,
    );
    let screens = terminal.screens();
    assert_eq!(screens.len(), 5, "{screens:#?}");
    // The blank line before c.md's header is b.md's: its name stays.
    assert!(names(&screens[1], 1, "b.md"), "{screens:#?}");
    // c.md's header at the top: its empty row, no sticky row over it.
    assert!(empty_band(&screens[2], 1), "{screens:#?}");
    assert!(names(&screens[2], 2, "c.md"), "{screens:#?}");
    assert!(names(&screens[3], 1, "c.md"), "{screens:#?}");
    assert!(names(&screens[4], 1, "c.md"), "{screens:#?}");
}

#[test]
fn a_lone_document_and_the_first_screen_have_no_sticky_row() {
    let frames = shown(&numbers(30), 40, 7, &[b"jjjjjjjjjj"]);
    assert!(!unstyled(&frames[1]).contains("▌"), "{frames:#?}");
}

#[test]
fn reading_mode_has_the_sticky_name_too() {
    // Reading mode lays the header out with the same code: scrolling row
    // by row, the name stays on the top row after the empty row above it
    // has gone, and the empty row returns above the name going back.
    on_both(|| {
        let mut keys: Vec<&[u8]> = vec![b"M"];
        keys.extend([b"j" as &[u8]; 30]);
        keys.extend([b"k" as &[u8]; 30]);
        let mut terminal = Fake::new(40, 8, &keys);
        let mut inputs = Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..screen_caps(80)
        });
        let files = two_files();
        let options = Options {
            reading: Some(super::reading::reading()),
            ..Options::new(SearchCase::Smart)
        };
        let session = Session::new(files.iter().map(|file| file.name.as_str()));
        let loads = &mut Loads::new();
        let content = Content::Files(&files);
        run(
            content,
            None,
            session,
            &mut inputs,
            options,
            loads,
            &mut terminal,
        )
        .unwrap();
        let seen = terminal.seen();
        let row = |at: usize, row: usize| seen[at].rows[row - 1].text().trim_end().to_owned();
        let down = |at: usize| row(at, 1) == "▌ b.md" && row(at, 2) == "▌";
        let at = (1..seen.len())
            .find(|&at| down(at))
            .expect("the header's top row above the view");
        let name_on_top = |at: usize| row(at, 1) == "▌ b.md";
        assert!(
            name_on_top(at) && row(at, 2) == "▌",
            "{:?}",
            seen[at].text()
        );
        assert!(
            row(at - 1, 1) == "▌" && row(at - 1, 2) == "▌ b.md",
            "{:?}",
            seen[at - 1].text()
        );
        // One row further: the empty row has gone, the name stays.
        assert!(
            name_on_top(at + 1) && row(at + 1, 2) != "▌",
            "{:?}",
            seen[at + 1].text()
        );
        // And back up: the empty row returns above the name.
        let back = (at + 2..seen.len()).find(|&at| row(at, 1) == "▌" && row(at, 2) == "▌ b.md");
        assert!(back.is_some(), "{:?}", seen.last().map(Seen::text));
    });
}

#[test]
fn a_long_path_is_cut_to_one_sticky_row_from_the_left() {
    let named = [("some/long/directory/b.md", 10)];
    let mut shown = files(&[("a.md", 3)]);
    shown.extend(files(&named));
    let reads: &[&[u8]] = &[b"jjjjjjjjjjjj"];
    let mut terminal = Fake::new(20, 7, reads);
    show_files(&shown, &mut terminal);
    let screens = terminal.screens();
    let top = row_text(&screens[1], 1);
    assert!(
        top.starts_with("▌ …") && top.ends_with("ory/b.md"),
        "{screens:#?}"
    );
    assert!(
        unstyled(screen_row(&screens[1], 1)).chars().count() <= 20,
        "{screens:#?}"
    );
    // The header itself wraps its name over rows instead.
    let header = row_text(&screens[0], 4);
    assert!(!header.contains('…'), "{screens:#?}");
}

/// The screens after `reads` of two files, the second `markdown`, on a
/// terminal that draws images, kitty's, which reports the mouse.
fn second_is(markdown: &str, reads: &[&[u8]]) -> Vec<Vec<String>> {
    let shown = [
        File {
            name: "a.md".to_owned(),
            document: numbers(3),
        },
        File {
            name: "b.md".to_owned(),
            document: crate::typeset::parse::parse(markdown),
        },
    ];
    let mut terminal = Fake::new(40, 7, reads);
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    let session = Session::new(shown.iter().map(|file| file.name.as_str()));
    let loads = &mut Loads::new();
    let options = Options::new(SearchCase::Smart);
    let content = Content::Files(&shown);
    run(
        content,
        None,
        session,
        &mut inputs,
        options,
        loads,
        &mut terminal,
    )
    .unwrap();
    terminal.screens()
}

/// The screens after `reads` of two files, the second with a link on its
/// first line to its last, which is far below.
fn link_in_second(reads: &[&[u8]]) -> Vec<Vec<String>> {
    let filler: String = (1..=8).map(|n| format!("Filler {n}.\n\n")).collect();
    second_is(
        &format!("[go](#target)\n\n{filler}# Target\n\nEnd.\n"),
        reads,
    )
}

#[test]
fn a_click_on_the_sticky_row_does_nothing() {
    // b.md's header is on lines 10 to 12, its blank line is 13, and the
    // link is line 14: the top line when the view is 14 lines down, under
    // the sticky row.
    let click_on = |row: u16| format!("\x1b[<0;3;{row}M\x1b[<0;3;{row}m").into_bytes();
    // The link on the second row is followed, as the click on it should.
    let down = b"jjjjjjjjjjjjj";
    let screens = link_in_second(&[down, &click_on(2)]);
    assert!(names(&screens[1], 1, "b.md"), "{screens:#?}");
    assert!(row_text(&screens[1], 2).contains("go"), "{screens:#?}");
    assert!(
        screens.last().unwrap() != &screens[1],
        "the click followed the link: {screens:#?}"
    );
    // With the link on the top line, the sticky row covers it: the click
    // on that row reaches nothing.
    let down = b"jjjjjjjjjjjjjj";
    let screens = link_in_second(&[down, &click_on(1)]);
    assert!(names(&screens[1], 1, "b.md"), "{screens:#?}");
    assert_eq!(row_text(&screens[1], 3), "  Filler 1.", "{screens:#?}");
    assert_eq!(screens.last().unwrap(), &screens[1], "{screens:#?}");
}

#[test]
fn a_search_match_lands_on_the_second_row_below_the_sticky_row() {
    let reads: &[&[u8]] = &[b"/Line 7\r"];
    let mut terminal = Fake::new(40, 7, reads);
    show_files(&two_files(), &mut terminal);
    let screens = terminal.screens();
    let screen = screens.last().unwrap();
    assert!(names(screen, 1, "b.md"), "{screens:#?}");
    assert_eq!(row_text(screen, 2), "  Line 7", "{screens:#?}");
}

#[test]
fn tab_skips_a_link_under_the_sticky_row() {
    // The first link is the top line, under the sticky row: the stop is
    // the second.
    let filler: String = (1..=4).map(|n| format!("Filler {n}.\n\n")).collect();
    let markdown = format!("[one](#x)\n\n{filler}[two](#y)\n\n{filler}# X\n\n# Y\n");
    let reads: &[&[u8]] = &[b"jjjjjjjjjjjjjj", b"\t"];
    let screens = second_is(&markdown, reads);
    assert!(names(&screens[1], 1, "b.md"), "{screens:#?}");
    let status = unstyled(screen_row(screens.last().unwrap(), 7));
    assert!(status.contains("#y"), "{screens:#?}");
}

#[test]
fn colon_n_still_lands_on_the_header_s_top_row_with_the_sticky_row_in_use() {
    // From inside b.md, :n goes to c.md's header, whose empty top row is
    // the first row.
    let reads: &[&[u8]] = &[b"jjjjjjjjjjjjjjj", b":n"];
    let mut terminal = Fake::new(40, 7, reads);
    show_files(
        &files(&[("a.md", 3), ("b.md", 6), ("c.md", 10)]),
        &mut terminal,
    );
    let screens = terminal.screens();
    let screen = screens.last().unwrap();
    assert!(names(&screens[1], 1, "b.md"), "{screens:#?}");
    assert!(empty_band(screen, 1), "{screens:#?}");
    assert!(names(screen, 2, "c.md"), "{screens:#?}");
}
