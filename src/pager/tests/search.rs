use super::*;

#[test]
fn a_slash_opens_a_prompt_on_the_status_line() {
    let mut terminal = Fake::new(40, 6, &[b"/", "lä".as_bytes()]);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    for (frame, prompt) in [(&frames[1], "/\u{258f}"), (&frames[2], "/lä\u{258f}")] {
        let status = status_row(frame, 6);
        assert!(status.contains(" kiri "), "{status:?}");
        assert!(status.contains(prompt), "{status:?}");
        assert!(!status.contains("notes.md"), "{status:?}");
        assert!(!status.contains("help"), "{status:?}");
    }
}

#[test]
fn enter_moves_to_the_first_match_from_the_top_of_the_view() {
    // Line 3 is above the view after a page down, so Line 30 is the
    // first match.
    let mut terminal = Fake::new(40, 6, &[b"jjjjjjjj", b"/line 3", b"\r"]);
    show(&numbers(40), &mut terminal);
    let frames = terminal.frames();
    assert!(at_top(frames.last().unwrap(), "Line 30"), "{frames:#?}");
    let status = status_row(frames.last().unwrap(), 6);
    assert!(status.contains("notes.md"), "{status:?}");
    assert!(!status.contains('\u{258f}'), "{status:?}");
}

#[test]
fn a_match_already_in_view_does_not_move_it() {
    // Two lines down after the search shows Line 2 at the top.
    let mut terminal = Fake::new(40, 6, &[b"/line 2\r", b"jj"]);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert!(at_top(frames.last().unwrap(), "Line 2"), "{frames:#?}");
}

#[test]
fn escape_and_ctrl_c_cancel_the_prompt() {
    let reads: &[&[u8]] = &[b"/line 20", b"\x1b", b"", b"/line 20", b"\x03", b"jj"];
    let mut terminal = Fake::new(40, 6, reads);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    let last = frames.last().unwrap();
    // Ctrl+C did not quit, and j moved down.
    assert!(at_top(last, "Line 2"), "{frames:#?}");
    assert!(status_row(last, 6).contains("notes.md"), "{frames:#?}");
}

#[test]
fn backspace_deletes_and_on_an_empty_prompt_cancels() {
    let reads: &[&[u8]] = &[b"/line 2x", b"\x7f0\r", b"/a", b"\x7f\x08", b"jj"];
    let mut terminal = Fake::new(40, 6, reads);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert!(
        frames.iter().any(|frame| at_top(frame, "Line 20")),
        "{frames:#?}"
    );
    assert!(at_top(frames.last().unwrap(), "Line 21"), "{frames:#?}");
}

#[test]
fn enter_on_an_empty_prompt_goes_on_from_the_view_after_scrolling_away() {
    let reads: &[&[u8]] = &[b"/line 20\r", b"g", b"/\r"];
    let mut terminal = Fake::new(60, 6, reads);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 4, "{frames:#?}");
    assert!(at_top(&frames[2], "Line 1"), "{frames:#?}");
    let last = frames.last().unwrap();
    assert!(at_top(last, "Line 20"), "{frames:#?}");
    assert!(status_row(last, 6).contains("/line 20 1/1"), "{frames:#?}");
}

#[test]
fn enter_on_an_empty_prompt_goes_on_to_the_next_match_as_n_does() {
    // Line 2 is in view after the search, so the repeat moves on.
    let reads: &[&[u8]] = &[b"/line 2\r", b"/\r"];
    let mut terminal = Fake::new(60, 6, reads);
    show(&numbers(30), &mut terminal);
    let last = terminal.frames().pop().unwrap();
    assert!(status_row(&last, 6).contains("/line 2 2/11"), "{last:?}");
    assert!(at_top(&last, "Line 20"), "{last:?}");
}

#[test]
fn enter_on_an_empty_prompt_without_a_search_cancels() {
    let mut terminal = Fake::new(40, 6, &[b"/\r", b"jj"]);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    let last = frames.last().unwrap();
    assert!(at_top(last, "Line 2"), "{frames:#?}");
    assert!(status_row(last, 6).contains("notes.md"), "{frames:#?}");
}

#[test]
fn ctrl_u_clears_the_prompt() {
    let mut terminal = Fake::new(40, 6, &[b"/xyz", b"\x15line 20\r"]);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert!(at_top(frames.last().unwrap(), "Line 20"), "{frames:#?}");
}

#[test]
fn keys_go_to_the_prompt_while_it_is_open() {
    // q and G are typed, not bindings.
    let mut terminal = Fake::new(40, 6, &[b"/qG"]);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert!(status_row(&frames[1], 6).contains("/qG"), "{frames:#?}");
    assert!(at_top(&frames[1], "Line 1"), "{frames:#?}");
}

#[test]
fn the_status_line_counts_the_matches_after_a_search() {
    let mut terminal = Fake::new(60, 6, &[b"/line 2\r"]);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    let status = status_row(frames.last().unwrap(), 6);
    assert!(status.contains("notes.md"), "{status:?}");
    assert!(status.contains("/line 2 1/11"), "{status:?}");
}

#[test]
fn a_term_not_found_is_told_until_the_next_key() {
    let mut terminal = Fake::new(60, 6, &[b"/xyz\r", b"x"]);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 3, "{frames:#?}");
    assert!(
        status_row(&frames[1], 6).contains("not found: xyz"),
        "{frames:#?}"
    );
    assert!(!status_row(&frames[2], 6).contains("xyz"), "{frames:#?}");
}

#[test]
fn n_and_shift_n_move_between_matches_and_around_the_ends() {
    let reads: &[&[u8]] = &[b"/line 2\r", b"n", b"N", b"N", b"n"];
    let mut terminal = Fake::new(60, 6, reads);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    let shows =
        |frame: &str, line, count| at_top(frame, line) && status_row(frame, 6).contains(count);
    assert_eq!(frames.len(), 6, "{frames:#?}");
    assert!(shows(&frames[1], "Line 1", "/line 2 1/11"), "{frames:#?}");
    assert!(shows(&frames[2], "Line 20", "/line 2 2/11"), "{frames:#?}");
    assert!(shows(&frames[3], "Line 2", "/line 2 1/11"), "{frames:#?}");
    // Around the start to the last match, on the last page, and
    // around the end to the first, each with the number flashing.
    assert!(unstyled(&frames[4]).contains("Line 29"), "{frames:#?}");
    let status = status_row(&frames[4], 6);
    assert!(status.contains("11\x1b[0;7m/11"), "{frames:#?}");
    assert!(at_top(&frames[5], "Line 2"), "{frames:#?}");
    let status = status_row(&frames[5], 6);
    assert!(status.contains("m1\x1b[0;7m/11"), "{frames:#?}");
}

#[test]
fn n_after_scrolling_away_goes_on_from_the_view() {
    // At the first match, then to the end and back: n takes the first
    // match in the view, not the one after the match left behind.
    let reads: &[&[u8]] = &[b"/line 2\r", b"G", b"n"];
    let mut terminal = Fake::new(60, 6, reads);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 4, "{frames:#?}");
    let status = status_row(&frames[3], 6);
    assert!(status.contains("/line 2 10/11"), "{frames:#?}");
    assert!(unstyled(&frames[3]).contains("Line 29"), "{frames:#?}");
}

#[test]
fn n_without_a_search_does_nothing() {
    let mut terminal = Fake::new(60, 6, &[b"nN", b"jj"]);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 2, "{frames:#?}");
}

#[test]
fn n_after_a_term_not_found_tells_it_again() {
    let mut terminal = Fake::new(60, 6, &[b"/xyz\r", b"j", b"n"]);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    let last = status_row(frames.last().unwrap(), 6);
    assert!(last.contains("not found: xyz"), "{frames:#?}");
}

#[test]
fn escape_hides_the_search_and_n_takes_it_up_again() {
    let reads: &[&[u8]] = &[b"/line 2\r", b"\x1b", b"", b"n"];
    let mut terminal = Fake::new(60, 6, reads);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 4, "{frames:#?}");
    assert!(
        status_row(&frames[1], 6).contains("/line 2 1/11"),
        "{frames:#?}"
    );
    assert!(!status_row(&frames[2], 6).contains("line 2"), "{frames:#?}");
    assert!(at_top(&frames[3], "Line 20"), "{frames:#?}");
    assert!(
        status_row(&frames[3], 6).contains("/line 2 2/11"),
        "{frames:#?}"
    );
}

#[test]
fn a_new_window_size_finds_the_matches_again_from_the_top_of_the_view() {
    let reads: &[&[u8]] = &[b"/line\r", b"jjjjjjjj", b""];
    let mut terminal = Fake::new(60, 6, reads);
    terminal.sizes = [window(60, 6), window(60, 6), window(70, 6)].into();
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 4, "{frames:#?}");
    assert!(
        status_row(&frames[2], 6).contains("/line 1/30"),
        "{frames:#?}"
    );
    // Line 5 is at the top, and the view stays.
    assert!(at_top(&frames[3], "Line 5"), "{frames:#?}");
    assert!(
        status_row(&frames[3], 6).contains("/line 5/30"),
        "{frames:#?}"
    );
}

#[test]
fn without_colours_going_around_an_end_flashes_the_match_number() {
    // N goes around to the last match, then `empty` polls without keys.
    let frames = |empty| {
        let mut caps = Capabilities::ansi(80);
        caps.color = false;
        frames_with(caps, &[b"/line 2\r", b"N"], empty)
    };
    // Bold, without the bar's reverse video, for half a second.
    let flashed = "\x1b[0;1m11\x1b[0;7m/11";
    // While the match fades, each poll is a twentieth of a second.
    let before = frames(9);
    assert_eq!(before.len(), 3, "{before:#?}");
    assert!(status_row(&before[2], 6).contains(flashed), "{before:#?}");
    let after = frames(10);
    assert_eq!(after.len(), 4, "{after:#?}");
    assert!(!status_row(&after[3], 6).contains(flashed), "{after:#?}");
    assert!(status_row(&after[3], 6).contains(" 11/11"), "{after:#?}");
}

#[test]
fn matches_in_view_are_marked_and_the_current_one_apart() {
    // Without colours known as RGB, a jump marks the match at once.
    let wait = [b"".as_slice(); 30];
    let reads = [
        &[b"/line 2\r".as_slice()],
        &wait[..],
        &[b"n"],
        &wait,
        &[b"\x1b", b""],
    ];
    let mut terminal = Fake::new(60, 6, &reads.concat());
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 4, "{frames:#?}");
    // Line 2 is the current match, below Line 1.
    let found = document_part(&frames[1], 6);
    assert_eq!(style_before(found, "Line 2\x1b"), ["0", "1", "7"]);
    assert!(found.contains("  Line 1\x1b"), "{found:?}");
    // Line 20 is the current match, and Line 21 another.
    let next = document_part(&frames[2], 6);
    assert_eq!(style_before(next, "Line 2\x1b[0m0"), ["0", "1", "7"]);
    assert_eq!(style_before(next, "Line 2\x1b[0m1"), ["0", "7"]);
    // Escape takes the marks away.
    let hidden = document_part(&frames[3], 6);
    assert!(
        !hidden.contains(";7m") && !hidden.contains("[0;7"),
        "{hidden:?}"
    );
    assert!(at_top(&frames[3], "Line 20"), "{frames:#?}");
}

#[test]
fn matches_are_marked_in_large_headings_and_in_tables() {
    let mut reads = vec![b"/kiri\r".as_slice()];
    // After the fade of the jump.
    reads.extend([b"".as_slice(); 30]);
    let mut terminal = Fake::new(60, 12, &reads);
    let source = "# Kiri title\n\n| a | b |\n|---|---|\n| c | kiri |\n";
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    run(
        Content::Document(&crate::parse::parse(source)),
        None,
        "notes.md",
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    )
    .unwrap();
    let frames = terminal.frames();
    let found = document_part(frames.last().unwrap(), 12);
    // The heading is split where the match ends, both parts scaled.
    let heading = style_before(found, "\x1b]66;s=3;Kiri\x1b\\");
    assert!(heading.contains(&"1".into()) && heading.contains(&"7".into()));
    assert!(found.contains("\x1b]66;s=3; title\x1b"), "{found:?}");
    let cell = style_before(found, "kiri");
    assert!(cell.contains(&"7".into()) && !cell.contains(&"1".into()));
    // Without the search, neither is marked.
    assert!(!document_part(&frames[0], 12).contains(";7"), "{frames:#?}");
}

#[test]
fn n_without_going_around_does_not_flash() {
    let mut terminal = Fake::new(60, 6, &[b"/line 2\r", b"n"]);
    show(&numbers(30), &mut terminal);
    let frames = terminal.frames();
    assert!(
        status_row(&frames[2], 6).contains("/line 2 2/11"),
        "{frames:#?}"
    );
}

/// The SGR codes of bold text in `fg` on `bg`, each as `r;g;b`.
fn colours(fg: &str, bg: &str) -> Vec<String> {
    let mut codes = vec!["0".to_owned(), "1".to_owned()];
    codes.extend(format!("38;2;{fg};48;2;{bg}").split(';').map(str::to_owned));
    codes
}

#[test]
fn a_jump_fades_the_match_from_its_own_colours_into_reverse_video() {
    let frames = frames_with(known_colours(), &[b"/line 2\r"], 40);
    // The jump, a frame each twentieth of a second after it until
    // three quarters of a second, and no more after the fade.
    assert_eq!(frames.len(), 17, "{frames:#?}");
    let current = |frame: &str| style_before(document_part(frame, 6), "Line 2\x1b");
    // Grey on black, the text's own, into black on grey.
    assert_eq!(current(&frames[1]), colours("200;200;200", "0;0;0"));
    assert_eq!(current(&frames[7]), colours("120;120;120", "80;80;80"));
    assert_eq!(current(&frames[15]), colours("13;13;13", "187;187;187"));
    assert_eq!(current(&frames[16]), ["0", "1", "7"]);
}

#[test]
fn a_match_in_yellow_text_fades_into_its_reverse() {
    // The title of a warning is in palette colour 3, a yellow.
    let mut inputs = Inputs::new(known_colours());
    inputs.theme.alerts.colors = crate::theme::AlertColors::Terminal;
    let document = crate::parse::parse("> [!WARNING]\n> Mind the step.\n");
    let frames = frames_of(&document, inputs, &[b"/warning\r"], 40);
    assert_eq!(frames.len(), 17, "{frames:#?}");
    let current = |frame: &str| style_before(document_part(frame, 6), "Warning\x1b");
    // Yellow on the alert's yellow tint at the jump, into the tint on
    // yellow.
    assert_eq!(current(&frames[1]), colours("200;200;0", "30;30;0"));
    assert_eq!(current(&frames[7]), colours("132;132;0", "98;98;0"));
    assert_eq!(current(&frames[15]), colours("41;41;0", "189;189;0"));
    let end = current(&frames[16]);
    assert!(end.contains(&"7".to_owned()), "{end:?}");
}

#[test]
fn n_and_shift_n_fade_the_match_too() {
    let frames = frames_with(known_colours(), &[b"/line 2\r", b"n"], 0);
    let current = style_before(document_part(&frames[2], 6), "Line 2\x1b[0m0");
    assert_eq!(current, colours("200;200;200", "0;0;0"), "{frames:#?}");
}

#[test]
fn colours_not_known_as_rgb_mark_the_match_at_once() {
    // The terminal tells no colours: the current match's reverse
    // video at once, and no frame after it.
    let frames = frames_with(Capabilities::ansi(80), &[b"/line 2\r"], 30);
    assert_eq!(frames.len(), 2, "{frames:#?}");
    let current = |frame: &str| style_before(document_part(frame, 6), "Line 2\x1b");
    assert_eq!(current(&frames[1]), ["0", "1", "7"]);
    // Nor without colours.
    let mut caps = known_colours();
    caps.color = false;
    let frames = frames_with(caps, &[b"/line 2\r"], 30);
    assert_eq!(frames.len(), 2, "{frames:#?}");
    assert_eq!(current(&frames[1]), ["0", "1", "7"]);
}

#[test]
fn going_around_an_end_fades_the_match_number_from_the_bar_s_reverse() {
    // A grey bar, mixed from the black page and this grey.
    let mut caps = known_colours();
    caps.palette[8] = Some(Color::Rgb(100, 100, 100));
    let frames = frames_with(caps, &[b"/line 2\r", b"N"], 40);
    // The search, then N, which fades for three quarters of a second.
    assert_eq!(frames.len(), 18, "{frames:#?}");
    let number = |frame: &str| style_before(&status_row(frame, 6), "11\x1b");
    let colours = |fg: &str, bg: &str| {
        let mut codes = vec!["0".to_owned()];
        codes.extend(format!("38;2;{fg};48;2;{bg}").split(';').map(str::to_owned));
        codes
    };
    // Dark grey on grey back into the bar's grey on dark grey.
    assert_eq!(number(&frames[2]), colours("50;50;50", "200;200;200"));
    assert_eq!(number(&frames[8]), colours("110;110;110", "140;140;140"));
    assert_eq!(number(&frames[16]), colours("190;190;190", "60;60;60"));
    // At the end, in the bar's colours, with the rest of the search.
    let status = status_row(&frames[17], 6);
    assert_eq!(
        style_before(&status, "/line 2 11/11"),
        ["0", "48", "2", "50", "50", "50"]
    );
}

#[test]
fn going_around_an_end_flashes_the_match_number_where_colours_are_not_known() {
    // Bold, without the bar's reverse video, for half a second.
    let frames = frames_with(Capabilities::ansi(80), &[b"/line 2\r", b"N"], 30);
    assert_eq!(frames.len(), 4, "{frames:#?}");
    let number = |frame: &str| style_before(&status_row(frame, 6), "11\x1b");
    assert_eq!(number(&frames[2]), ["0", "1"]);
    assert!(status_row(&frames[3], 6).contains(" 11/11"), "{frames:#?}");
}
