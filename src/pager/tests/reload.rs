use super::*;
use crate::session::sources::Version;
use crate::session::state::Look;
use crate::typeset::diagnostic::{Code, Diagnostic};

/// `document` shown with `then` to reload it, in a window of 60 × 6.
fn show_reloading(document: &Document, then: &mut Then, keys: &[&[u8]]) -> Vec<String> {
    reloading_with(Capabilities::ansi(80), document, then, keys, 0)
}

/// As [`show_reloading`], with every read in `reads` as it is, empty
/// ones included.
fn show_reloading_with(document: &Document, then: &mut Then, reads: &[&[u8]]) -> Vec<String> {
    let mut terminal = Fake::new(60, 6, reads);
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    let options = Options {
        files: Some(then),
        ..Options::new(SearchCase::Smart)
    };
    let content = Content::Document(document);
    run(
        content,
        None,
        session_for(&options),
        &mut inputs,
        options,
        &mut Loads::new(),
        &mut terminal,
    )
    .unwrap();
    terminal.frames()
}

/// The session as it ended, when `document` was shown with `then` to
/// reload it and the reader pressed `reads`.
fn session_after(document: &Document, then: &mut Then, reads: &[&[u8]]) -> Session {
    let mut terminal = Fake::new(60, 6, reads);
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    let options = Options {
        files: Some(then),
        ..Options::new(SearchCase::Smart)
    };
    let session = session_for(&options);
    let content = Content::Document(document);
    run(
        content,
        None,
        session,
        &mut inputs,
        options,
        &mut Loads::new(),
        &mut terminal,
    )
    .unwrap()
}

#[test]
fn each_reload_that_reads_the_document_brings_its_source_to_the_next_version() {
    let docs = [Reloaded::Document(rows(3)), Reloaded::Document(rows(4))];
    let mut then = Then::new(docs);
    let session = session_after(&numbers(10), &mut then, &[b"r", b"", b"r", b"", b"q"]);
    assert_eq!(then.asked, 2);
    let version = session.sources()[0].version();
    assert_eq!(version, Version::FIRST.next().next(), "{session:?}");
}

#[test]
fn a_reload_that_fails_leaves_the_version_of_its_source() {
    let why = Reloaded::Failed("notes.md: No such file or directory".to_owned());
    let mut then = Then::new([why]);
    let session = session_after(&numbers(10), &mut then, &[b"r", b"", b"q"]);
    assert_eq!(then.asked, 1);
    assert_eq!(session.sources()[0].version(), Version::FIRST);
}

#[test]
fn a_reload_reads_the_job_the_session_describes() {
    let docs = [Reloaded::Document(rows(3)), Reloaded::Document(rows(4))];
    let mut then = Then::new(docs);
    let session = session_after(&numbers(10), &mut then, &[b"r", b"", b"r", b"", b"q"]);
    let job = Reread {
        source: session.sources()[0].id(),
        path: "notes.md".into(),
        input: Input::Markdown,
    };
    assert_eq!(then.jobs, [job.clone(), job]);
}

/// How a file looked at a reload, told apart by `length`.
fn look_of(length: u64) -> Look {
    Look {
        modified: None,
        length,
    }
}

/// The look the session has of the file of its only source.
fn session_look(session: &Session) -> Option<Option<Look>> {
    session.sources()[0].file().map(|file| file.look)
}

#[test]
fn a_reload_that_reads_the_document_gives_its_source_the_look_it_was_fetched_at() {
    let mut then = Then::new([Reloaded::Document(rows(3))]);
    then.looked = look_of(7);
    let session = session_after(&numbers(10), &mut then, &[b"r", b"", b"q"]);
    assert_eq!(session.sources()[0].version(), Version::FIRST.next());
    assert_eq!(session_look(&session), Some(Some(look_of(7))));
}

#[test]
fn a_reload_that_fails_gives_its_source_the_look_and_leaves_the_version() {
    let why = Reloaded::Failed("notes.md: not UTF-8 text".to_owned());
    let mut then = Then::new([why]);
    then.looked = look_of(7);
    let session = session_after(&numbers(10), &mut then, &[b"r", b"", b"q"]);
    assert_eq!(session.sources()[0].version(), Version::FIRST);
    assert_eq!(session_look(&session), Some(Some(look_of(7))));
}

#[test]
fn r_shows_the_document_read_again_and_tells_so_until_the_next_key() {
    let mut then = Then::new([Reloaded::Document(rows(10))]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"j"]);
    let screens = replay(&frames);
    assert_eq!(screens.len(), 4, "{screens:#?}");
    assert_eq!(then.asked, 1);
    assert!(at_top_of(&screens[0], "Line 1"), "{screens:#?}");
    // The old document stays on the screen while it is reloaded.
    assert!(at_top_of(&screens[1], "Line 1"), "{screens:#?}");
    assert!(
        screen_row(&screens[1], 6).contains("reloading"),
        "{screens:#?}"
    );
    assert!(at_top_of(&screens[2], "Row 1"), "{screens:#?}");
    assert!(
        screen_row(&screens[2], 6).contains("reloaded"),
        "{screens:#?}"
    );
    assert!(
        !screen_row(&screens[3], 6).contains("reloaded"),
        "{screens:#?}"
    );
}

#[test]
fn a_reload_that_fails_keeps_the_old_document_and_tells_why_until_the_next_key() {
    let why = Reloaded::Failed("notes.md: No such file or directory".to_owned());
    let mut then = Then::new([why]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"j"]);
    let screens = replay(&frames);
    assert_eq!(screens.len(), 4, "{screens:#?}");
    assert!(at_top_of(&screens[2], "Line 1"), "{screens:#?}");
    let status = screen_row(&screens[2], 6);
    assert!(status.contains("No such file or directory"), "{screens:#?}");
    assert!(!status.contains("reloaded"), "{screens:#?}");
    assert!(
        !screen_row(&screens[3], 6).contains("No such file"),
        "{screens:#?}"
    );
}

/// A warning at line `line` of the file.
fn warning(line: usize, message: &str) -> Diagnostic {
    Diagnostic {
        line: Some(line),
        ..Diagnostic::new(Code::Image, message)
    }
}

/// A front matter warning at line `line`, as the parser's error makes it.
fn front_matter(line: usize, message: &str) -> Diagnostic {
    Diagnostic::from(&crate::typeset::model::FrontMatterError {
        line: Some(line),
        message: message.to_owned(),
    })
}

#[test]
fn a_reload_with_warnings_says_reloaded_and_marks_how_many() {
    let warned = Reloaded::Warned(rows(10), vec![warning(3, "bad"), warning(9, "worse")]);
    let mut then = Then::new([warned]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"j"]);
    assert_eq!(frames.len(), 4, "{frames:#?}");
    assert!(at_top(&frames[2], "Row 1"), "{frames:#?}");
    let status = unstyled(&status_row(&frames[2], 6));
    assert!(status.contains("reloaded"), "{frames:#?}");
    assert!(status.contains("\u{26a0} 2"), "{frames:#?}");
    // The mark stays after the next key, and `reloaded` goes.
    let status = unstyled(&status_row(&frames[3], 6));
    assert!(status.contains("\u{26a0} 2"), "{frames:#?}");
    assert!(!status.contains("reloaded"), "{frames:#?}");
}

#[test]
fn a_document_without_warnings_has_no_mark() {
    let mut then = Then::new([Reloaded::Document(rows(10))]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"j"]);
    assert!(!unstyled(&frames[2]).contains('\u{26a0}'), "{frames:#?}");
}

#[test]
fn a_reload_without_warnings_takes_the_mark_away() {
    let warned = Reloaded::Warned(rows(10), vec![warning(3, "bad")]);
    let mut then = Then::new([warned, Reloaded::Document(rows(10))]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"r"]);
    let last = frames.len() - 1;
    let status = unstyled(&status_row(&frames[2], 6));
    assert!(status.contains("\u{26a0} 1"), "{frames:#?}");
    let status = unstyled(&status_row(&frames[last], 6));
    assert!(!status.contains('\u{26a0}'), "{frames:#?}");
    assert!(status.contains("reloaded"), "{frames:#?}");
}

#[test]
fn the_number_of_the_mark_flashes_when_it_changes_and_not_when_it_stays() {
    // Without known colours the flash is bold for half a second.
    let flashing = |frame: &str| status_row(frame, 6).contains("\x1b[0;1m1");
    let warned = || Reloaded::Warned(rows(10), vec![warning(3, "bad")]);
    let mut then = Then::new([warned(), warned()]);
    let reads: Vec<&[u8]> = vec![b"r", b"", b"", b"", b"", b"", b"", b"", b"", b"", b"", b"r"];
    let frames = show_reloading_with(&numbers(10), &mut then, &reads);
    let first = frames.iter().position(|f| f.contains("\u{26a0}"));
    let first = first.unwrap_or(usize::MAX);
    assert!(first < frames.len(), "no mark: {frames:#?}");
    assert!(flashing(&frames[first]), "{frames:#?}");
    let calm = frames
        .iter()
        .position(|f| f.contains("\u{26a0}") && !flashing(f));
    let calm = calm.expect("the flash ends");
    assert!(calm > first, "{frames:#?}");
    // The second reload leaves the count as it was.
    let last = frames.last().unwrap();
    assert!(last.contains("\u{26a0}") && !flashing(last), "{frames:#?}");
}

/// The text a frame shows in the warning line, on row 5 of a window
/// of 6 rows, with the column it starts at; `None` when the frame
/// writes nothing there but the document's own row.
fn warning_line(frame: &str) -> Option<(usize, String)> {
    let plain = unstyled(frame);
    let at = plain.rfind("\x1b[5;")?;
    let rest = &plain[at + 4..];
    let (column, text) = rest.split_once('H')?;
    let text = text.split("\x1b").next()?;
    // The document's own row 5 starts in column 1.
    (column != "1").then(|| (column.parse().unwrap(), text.to_owned()))
}

/// The red of the warning line in `frame` as RGB, which is where it
/// has begun to fade.
fn fade_red(frame: &str) -> Option<u8> {
    let line = &frame[frame.rfind("\x1b[5;")?..];
    let rgb = line.split_once("38;2;")?.1;
    rgb.split(';').next()?.parse().ok()
}

fn warned_with(warnings: Vec<Diagnostic>) -> Reloaded {
    Reloaded::Warned(rows(10), warnings)
}

#[test]
fn a_reload_with_warnings_shows_the_first_above_the_status_line_at_the_right_edge() {
    let mut then = Then::new([warned_with(vec![front_matter(3, "bad")])]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"r"]);
    let (column, text) = warning_line(&frames[2]).unwrap_or_default();
    assert_eq!(text, "\u{26a0} [:3] front matter: bad", "{frames:#?}");
    // Ends a cell before the edge, as the status line does.
    assert_eq!(column + text.chars().count(), 60, "{frames:#?}");
    // The document's row stays where it is, and the status line too.
    assert!(
        unstyled(&frames[2]).contains("\x1b[5;1H  Row 3"),
        "{frames:#?}"
    );
    assert!(
        status_row(&frames[2], 6).contains("reloaded"),
        "{frames:#?}"
    );
    // Red, as errors are drawn in a document, on the page.
    assert!(frames[2].contains("\x1b[0;31m\u{26a0}"), "{frames:#?}");
}

#[test]
fn the_warning_line_leaves_out_the_line_when_it_is_not_known() {
    let warning = Diagnostic::new(Code::Image, "no line");
    let mut then = Then::new([warned_with(vec![warning])]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"r"]);
    let (_, text) = warning_line(&frames[2]).unwrap_or_default();
    assert_eq!(text, "\u{26a0} no line", "{frames:#?}");
}

#[test]
fn the_warning_line_counts_the_other_warnings_in_italics() {
    let many = vec![
        warning(3, "first"),
        warning(4, "second"),
        warning(5, "third"),
    ];
    let mut then = Then::new([warned_with(many)]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"r"]);
    let (_, text) = warning_line(&frames[2]).unwrap_or_default();
    assert_eq!(
        text, "\u{26a0} [:3] first  and 2 more warnings\u{2026}",
        "{frames:#?}"
    );
    assert!(frames[2].contains("\x1b[0;2;3m  and 2"), "{frames:#?}");
    assert!(
        unstyled(&status_row(&frames[2], 6)).contains("\u{26a0} 3"),
        "{frames:#?}"
    );
}

#[test]
fn a_key_does_not_cancel_the_warning_line() {
    let mut then = Then::new([warned_with(vec![warning(3, "bad")])]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"j"]);
    assert!(warning_line(&frames[2]).is_some(), "{frames:#?}");
    assert!(
        warning_line(frames.last().unwrap()).is_some(),
        "{frames:#?}"
    );
}

#[test]
fn the_warning_line_stays_five_seconds_and_then_goes() {
    let mut then = Then::new([warned_with(vec![warning(3, "bad")])]);
    let terminal = reloading_in(
        6,
        Capabilities::ansi(80),
        &numbers(10),
        &mut then,
        &[b"r"],
        80,
    );
    let screens = terminal.screens();
    // The warning's own text is only in the warning line, on row 5.
    let warned = |screen: &[String]| unstyled(screen_row(screen, 5)).contains("bad");
    assert!(warned(&screens[2]), "{screens:#?}");
    // Seen until the fade out ends, where it cannot fade.
    let last = screens.last().unwrap();
    assert!(!warned(last), "{last:?}");
    // The mark stays, as the document still has the warning.
    assert!(
        unstyled(screen_row(last, 6)).contains("\u{26a0} 1"),
        "{last:?}"
    );
}

#[test]
fn the_warning_line_fades_into_the_page_where_colours_are_known() {
    let mut caps = Capabilities::kitty(80);
    caps.foreground = Some(Color::Rgb(200, 200, 200));
    caps.background = Some(Color::Rgb(0, 0, 0));
    caps.palette[1] = Some(Color::Rgb(200, 0, 0));
    let mut then = Then::new([warned_with(vec![warning(3, "bad")])]);
    let terminal = reloading_in(6, caps, &numbers(10), &mut then, &[b"r"], 90);
    let frames: Vec<String> = terminal
        .frames()
        .into_iter()
        .filter(|frame| warning_line(frame).is_some())
        .collect();
    assert!(frames.len() > 3, "no warning line: {frames:#?}");
    // Red while it stays; the palette's red goes as RGB once it fades.
    assert!(frames[0].contains("\x1b[0;31m\u{26a0}"), "{:?}", frames[0]);
    let reds: Vec<u8> = frames.iter().filter_map(|frame| fade_red(frame)).collect();
    assert!(reds.len() > 5, "{reds:?}\n{frames:#?}");
    assert!(reds[0] > 150 && reds.contains(&120), "{reds:?}");
    assert!(reds.windows(2).all(|pair| pair[1] <= pair[0]), "{reds:?}");
    assert!(*reds.last().unwrap() < 30, "{reds:?}");
    // Fading is drawn at frame rate, and only then.
    let fast = terminal.waits.iter().filter(|wait| **wait == FRAME).count();
    assert!(fast >= 10, "{:?}", terminal.waits);
}

#[test]
fn a_new_reload_replaces_the_warning_line_or_takes_it_away() {
    let mut then = Then::new([
        warned_with(vec![warning(3, "first")]),
        warned_with(vec![warning(4, "second")]),
        Reloaded::Document(rows(10)),
    ]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"r", b"r"]);
    let lines: Vec<_> = frames
        .iter()
        .filter_map(|frame| warning_line(frame))
        .collect();
    assert!(
        lines.iter().any(|(_, text)| text.contains("first")),
        "{frames:#?}"
    );
    assert!(lines.last().unwrap().1.contains("second"), "{frames:#?}");
    // The replayed screen shows the warning line gone: a frame that does
    // not write the row would not show it.
    let screens = replay(&frames);
    let last = unstyled(screen_row(screens.last().unwrap(), 5));
    assert!(
        !last.contains("first") && !last.contains("second"),
        "{last:?}"
    );
}

#[test]
fn a_window_without_a_row_above_the_status_line_has_no_warning_line() {
    let mut then = Then::new([warned_with(vec![warning(3, "bad")])]);
    let terminal = reloading_in(
        1,
        Capabilities::ansi(80),
        &numbers(10),
        &mut then,
        &[b"r"],
        0,
    );
    for frame in terminal.frames() {
        assert!(!unstyled(&frame).contains("bad"), "{frame:?}");
    }
}

#[test]
fn warnings_given_at_the_start_show_the_mark_and_the_line_at_once() {
    let mut terminal = Fake::new(60, 6, &[b"j"]);
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    let document = numbers(10);
    let options = Options {
        warnings: vec![front_matter(3, "bad")],
        ..Options::new(SearchCase::Smart)
    };
    let content = Content::Document(&document);
    run(
        content,
        None,
        Session::new(["notes.md"]),
        &mut inputs,
        options,
        &mut Loads::new(),
        &mut terminal,
    )
    .unwrap();
    let frames = terminal.frames();
    let (_, text) = warning_line(&frames[0]).unwrap_or_default();
    assert_eq!(text, "\u{26a0} [:3] front matter: bad", "{frames:#?}");
    assert!(
        unstyled(&status_row(&frames[0], 6)).contains("\u{26a0} 1"),
        "{frames:#?}"
    );
}

#[test]
fn without_a_file_to_reload_r_says_so_until_the_next_key() {
    let frames = shown(&numbers(10), 60, 6, &[b"r", b"j"]);
    assert_eq!(frames.len(), 3, "{frames:#?}");
    let status = status_row(&frames[1], 6);
    assert!(status.contains("nothing to reload"), "{frames:#?}");
    assert!(!status_row(&frames[2], 6).contains("nothing to reload"));
}

#[test]
fn reloaded_stays_four_seconds_then_fades_out_into_the_bar() {
    // A grey bar, mixed from the black page and this grey.
    let mut caps = known_colours();
    caps.palette[8] = Some(Color::Rgb(100, 100, 100));
    let mut then = Then::new([Reloaded::Document(rows(10))]);
    let frames = reloading_with(caps, &numbers(10), &mut then, &[b"r"], 200);
    // The reload, nothing new for four seconds, a frame each
    // twentieth of a second for the three seconds of the fade, and
    // the status line without the message after it.
    assert_eq!(frames.len(), 64, "{frames:#?}");
    let message = |frame: &str| style_before(&status_row(frame, 6), "reloaded");
    let colours = |fg: &str| {
        let mut codes = vec!["0".to_owned()];
        codes.extend(
            format!("38;2;{fg};48;2;50;50;50")
                .split(';')
                .map(str::to_owned),
        );
        codes
    };
    // In the bar's colours at first, the grey of the page's text on
    // dark grey, then into the dark grey.
    assert_eq!(message(&frames[2]), ["0", "48", "2", "50", "50", "50"]);
    assert_eq!(message(&frames[3]), colours("200;200;200"));
    assert_eq!(message(&frames[33]), colours("125;125;125"));
    assert_eq!(message(&frames[61]), colours("55;55;55"));
    assert!(
        !status_row(&frames[63], 6).contains("reloaded"),
        "{frames:#?}"
    );
}

#[test]
fn reloaded_goes_at_once_after_seven_seconds_where_colours_are_not_known() {
    // Seven seconds are seventy polls; the reload's read is the first.
    let reloaded = |empty| {
        let mut then = Then::new([Reloaded::Document(rows(10))]);
        let caps = Capabilities::ansi(80);
        reloading_with(caps, &numbers(10), &mut then, &[b"r"], empty)
    };
    let frames = reloaded(69);
    assert_eq!(frames.len(), 3, "{frames:#?}");
    assert!(status_row(&frames[2], 6).contains("reloaded"));
    let frames = reloaded(70);
    assert_eq!(frames.len(), 4, "{frames:#?}");
    assert!(
        !status_row(&frames[3], 6).contains("reloaded"),
        "{frames:#?}"
    );
}

#[test]
fn the_pager_waits_a_frame_only_while_reloaded_fades_on_a_shown_status_line() {
    let mut caps = known_colours();
    caps.palette[8] = Some(Color::Rgb(100, 100, 100));
    let waits = |height| {
        let mut then = Then::new([Reloaded::Document(rows(10))]);
        let caps = caps.clone();
        reloading_in(height, caps, &numbers(10), &mut then, &[b"r"], 120).waits
    };
    // The read of r, forty polls of the stay, sixty frames of the
    // fade, and polls again up to the read of Ctrl+C that quits.
    let mut expected = vec![POLL; 41];
    expected.extend([FRAME; 60]);
    expected.extend([POLL; 21]);
    assert_eq!(waits(6), expected);
    // A window of one row has no status line to fade.
    assert_eq!(waits(1), [POLL; 122]);
}

#[test]
fn a_key_clears_reloaded_at_once_while_it_fades() {
    // Forty polls are the four seconds the message stays, and ten
    // frames half a second of its fade.
    let mut caps = known_colours();
    caps.palette[8] = Some(Color::Rgb(100, 100, 100));
    let mut then = Then::new([Reloaded::Document(rows(10))]);
    let mut reads: Vec<&[u8]> = vec![b"r"];
    reads.extend(std::iter::repeat_n(b"".as_slice(), 50));
    reads.push(b"j");
    let frames = reloading_with(caps, &numbers(10), &mut then, &reads, 0);
    let last = frames.last().unwrap();
    // The key moved the view and cleared the message in one frame.
    let screens = replay(&frames);
    assert!(!at_top_of(screens.last().unwrap(), "Row 1"), "{frames:#?}");
    assert!(!status_row(last, 6).contains("reloaded"), "{frames:#?}");
    // The frame before the key showed the message fading.
    let before = status_row(&frames[frames.len() - 2], 6);
    let fading = style_before(&before, "reloaded");
    assert_eq!(fading[..2], ["0", "38"], "{frames:#?}");
}

#[test]
fn other_messages_stay_until_the_next_key() {
    let why = Reloaded::Failed("notes.md: No such file or directory".to_owned());
    let mut then = Then::new([why]);
    let frames = reloading_with(known_colours(), &numbers(10), &mut then, &[b"r"], 200);
    assert_eq!(frames.len(), 3, "{frames:#?}");
    assert!(status_row(&frames[2], 6).contains("No such file"));
    let frames = frames_with(known_colours(), &[b"r"], 200);
    assert_eq!(frames.len(), 2, "{frames:#?}");
    assert!(status_row(&frames[1], 6).contains("nothing to reload"));
}

#[test]
fn a_reload_keeps_the_block_at_the_top() {
    let mut then = Then::new([Reloaded::Document(rows(10))]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"jjjjjj", b"r"]);
    assert_eq!(frames.len(), 4, "{frames:#?}");
    assert!(at_top(&frames[1], "Line 4"), "{frames:#?}");
    assert!(at_top(&frames[3], "Row 4"), "{frames:#?}");
}

#[test]
fn a_reload_at_the_end_goes_to_the_new_end() {
    // Ten lines end with Line 9 at the top; twenty rows end with Row
    // 19 there, not Row 9, as a document that grew is followed.
    let mut then = Then::new([Reloaded::Document(rows(20))]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"G", b"r"]);
    assert_eq!(frames.len(), 4, "{frames:#?}");
    assert!(at_top(&frames[1], "Line 9"), "{frames:#?}");
    assert!(at_top(&frames[3], "Row 19"), "{frames:#?}");
}

#[test]
fn a_reload_finds_the_search_again_in_the_new_lines() {
    // Line 3 and Line 30 match in 30 lines; Line 3 and Line 30 to
    // Line 35 do in 35.
    let mut then = Then::new([Reloaded::Document(numbers(35))]);
    let keys: &[&[u8]] = &[b"/Line 3\r", b"r", b"n"];
    let frames = show_reloading(&numbers(30), &mut then, keys);
    assert_eq!(frames.len(), 5, "{frames:#?}");
    assert!(
        status_row(&frames[1], 6).contains("/Line 3 1/2"),
        "{frames:#?}"
    );
    assert!(
        status_row(&frames[3], 6).contains("reloaded"),
        "{frames:#?}"
    );
    assert!(
        status_row(&frames[4], 6).contains("/Line 3 2/7"),
        "{frames:#?}"
    );
}

#[test]
fn a_reload_keeps_the_place_when_a_block_is_added_above_it() {
    // Ten lines, scrolled to Line 4; then a block before all of them.
    let mut then = Then::new([Reloaded::Document(crate::typeset::parse::parse(&format!(
        "Added\n\n{}",
        (1..=10)
            .map(|n| format!("Line {n}\n\n"))
            .collect::<String>()
    )))]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"jjjjjj", b"r"]);
    let screens = replay(&frames);
    assert!(at_top_of(&screens[1], "Line 4"), "{screens:#?}");
    assert!(at_top_of(&screens[3], "Line 4"), "{screens:#?}");
}

/// A document of `blocks`, one after the other.
fn blocks(blocks: &[&str]) -> Document {
    crate::typeset::parse::parse(&blocks.join("\n\n"))
}

#[test]
fn a_reload_goes_to_the_next_block_when_the_top_block_is_removed() {
    // Scrolled to Line 4; then Line 4 is gone and a block comes first.
    let old: Vec<String> = (1..=10).map(|n| format!("Line {n}")).collect();
    let mut new: Vec<&str> = vec!["Added"];
    new.extend(
        old.iter()
            .map(String::as_str)
            .filter(|&line| line != "Line 4"),
    );
    let mut then = Then::new([Reloaded::Document(blocks(&new))]);
    let frames = show_reloading(&numbers(10), &mut then, &[b"jjjjjj", b"r"]);
    assert!(at_top(&frames[1], "Line 4"), "{frames:#?}");
    assert!(at_top(&frames[3], "Line 5"), "{frames:#?}");
}

#[test]
fn a_reload_keeps_the_offset_in_a_block_that_changed() {
    // A code block of eight lines, scrolled three lines into it; then
    // two of its lines change and a block is added above it.
    let code = |first: &str, last: &str| format!("```\n{first}\nb\nc\nd\ne\nf\ng\n{last}\n```");
    let old = blocks(&["Intro", &code("a", "h"), "Outro"]);
    let new = blocks(&["Added", "Intro", &code("A", "H"), "Outro"]);
    let mut then = Then::new([Reloaded::Document(new)]);
    let frames = show_reloading(&old, &mut then, &[b"jjjjj", b"r"]);
    let screens = replay(&frames);
    assert_eq!(screens.len(), 4, "{screens:#?}");
    assert_eq!(top_row_of(&screens[1]), "c", "{screens:#?}");
    assert_eq!(top_row_of(&screens[3]), "c", "{screens:#?}");
}

#[test]
fn a_reload_keeps_the_place_in_a_details_block() {
    let details = |added: &str| {
        let inner: String = (1..=6).map(|n| format!("Inner {n}\n\n")).collect();
        format!("{added}<details>\n<summary>More</summary>\n\n{inner}</details>\n\nEnd")
    };
    let old = crate::typeset::parse::parse(&details(""));
    let new = crate::typeset::parse::parse(&details("Added\n\n"));
    let mut then = Then::new([Reloaded::Document(new)]);
    let frames = show_reloading(&old, &mut then, &[b"jjjjjj", b"r"]);
    let screens = replay(&frames);
    assert_eq!(screens.len(), 4, "{screens:#?}");
    let inner = top_row_of(&screens[1]);
    assert!(inner.starts_with("Inner"), "{screens:#?}");
    assert_eq!(top_row_of(&screens[3]), inner, "{screens:#?}");
}

#[test]
fn a_reload_goes_to_the_next_block_from_a_blank_top_line() {
    // One line down, the blank line after Line 1 is at the top; then a
    // block is added above. As on a resize, the view goes to the start
    // of the next block. A header line only exists with several files,
    // and those cannot be reloaded, so that case is not tested here.
    let mut then = Then::new([Reloaded::Document(blocks(&[
        "Added", "Line 1", "Line 2", "Line 3", "Line 4", "Line 5", "Line 6",
    ]))]);
    let frames = show_reloading(&numbers(6), &mut then, &[b"j", b"r"]);
    let screens = replay(&frames);
    assert_eq!(screens.len(), 4, "{screens:#?}");
    assert_eq!(top_row_of(&screens[1]), "", "{screens:#?}");
    assert!(at_top_of(&screens[3], "Line 2"), "{screens:#?}");
}

#[test]
fn a_reload_goes_to_the_next_block_when_the_top_block_in_a_details_is_removed() {
    let details = |inner: &[&str]| {
        let inner: String = inner.iter().map(|line| format!("{line}\n\n")).collect();
        crate::typeset::parse::parse(&format!(
            "<details>\n<summary>More</summary>\n\n{inner}</details>\n\nEnd"
        ))
    };
    let old = details(&[
        "Inner 1", "Inner 2", "Inner 3", "Inner 4", "Inner 5", "Inner 6",
    ]);
    // A block before Inner 1 as well, so that the block numbers alone
    // would put Inner 2 at the top.
    let new = details(&["New", "Inner 1", "Inner 2", "Inner 4", "Inner 5", "Inner 6"]);
    let mut then = Then::new([Reloaded::Document(new)]);
    let frames = show_reloading(&old, &mut then, &[b"jjjjjj", b"r"]);
    let screens = replay(&frames);
    assert_eq!(screens.len(), 4, "{screens:#?}");
    assert_eq!(top_row_of(&screens[1]), "Inner 3", "{screens:#?}");
    assert_eq!(top_row_of(&screens[3]), "Inner 4", "{screens:#?}");
}
