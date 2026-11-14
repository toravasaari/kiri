//! Following a link to a Markdown file with Enter: the file opens as a view
//! over the one below, at the anchor of the link, and Backspace goes back
//! to the place the link was followed from. The files are those a `Then`
//! answers with, so no file system is read.

use super::*;
use crate::session::deck::Sheet;
use crate::session::history::History;
use crate::session::settings;
use crate::session::state::Change;

const BACKSPACE: &[u8] = b"\x7f";

/// `rows` paragraphs, `Row 1` to `Row n`, with `with` as the paragraph at
/// the top.
fn with_rows(with: &str, count: usize) -> Document {
    let rows: String = (1..=count).map(|n| format!("Row {n}\n\n")).collect();
    crate::typeset::parse::parse(&format!("{with}\n\n{rows}"))
}

/// `base` shown in a window of 60 × `rows`, with `inputs`, as the reader
/// gives `keys`, with `then` for the files; `after` looks at the loop once
/// the reader has quit.
fn keyed(
    base: &Document,
    (inputs, rows): (&mut Inputs, u16),
    then: &mut Then,
    keys: &[&[u8]],
    after: impl FnOnce(&Driver<'_, '_, '_, Fake>),
) -> Fake {
    let options = Options {
        files: Some(then),
        ..Options::new(SearchCase::Smart)
    };
    keyed_with(base, (inputs, rows), options, keys, after)
}

/// [`keyed`] with the `options` the test chooses, which have the files.
fn keyed_with(
    base: &Document,
    (inputs, rows): (&mut Inputs, u16),
    options: Options<'_>,
    keys: &[&[u8]],
    after: impl FnOnce(&Driver<'_, '_, '_, Fake>),
) -> Fake {
    let mut terminal = Fake::new(60, rows, keys);
    let mut loads = Loads::inline_builds();
    let session = session_for(&options);
    let content = Content::Document(base);
    let mut driver = Driver::new(
        content,
        None,
        session,
        inputs,
        options,
        &mut loads,
        &mut terminal,
    );
    while driver.round().unwrap() {}
    after(&driver);
    drop(driver);
    loads.halt();
    loads.wait(crate::images::STOPPING);
    terminal
}

fn ansi() -> Inputs {
    Inputs::new(Capabilities {
        hyperlinks: true,
        ..Capabilities::ansi(80)
    })
}

/// What the status line of `screen`, in a window of `rows`, says.
fn status_of(screen: &[String], rows: u16) -> String {
    unstyled(screen_row(screen, rows))
}

fn opens(documents: impl IntoIterator<Item = Document>) -> Then {
    Then::new([]).opening(documents.into_iter().map(Reloaded::Document))
}

#[test]
fn enter_on_a_markdown_link_opens_it_over_the_base_and_the_status_line_names_it() {
    let base = with_rows("[go](docs/other.md) to the other.", 20);
    let mut then = opens([rows(3)]);
    let mut views = Vec::new();
    let keys: [&[u8]; 3] = [b"\t", b"\r", b""];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |rig| {
        views = rig.state.deck.sheets().collect();
    });
    assert_eq!(then.opened, [(PathBuf::from("docs/other.md"), 1)]);
    let screens = terminal.screens();
    // The reader sees that the pager is reading before it has.
    let busy = screens.iter().map(|screen| status_of(screen, 8));
    assert!(
        busy.clone()
            .any(|bar| bar.contains("opening docs/other.md…"))
    );
    let last = screens.last().unwrap();
    let text = screen_text(last);
    assert!(
        text.contains("Row 1") && !text.contains("to the other"),
        "{text}"
    );
    assert!(status_of(last, 8).contains("docs/other.md"), "{last:?}");
    assert_eq!(views.len(), 2, "the base and the opened view: {views:?}");
    assert!(matches!(views[1], Sheet::View(_)));
}

#[test]
fn enter_on_a_link_with_an_anchor_opens_at_the_heading() {
    let base = with_rows("[go](docs/other.md#second)", 4);
    let other = {
        let rows: String = (1..=10).map(|n| format!("Row {n}\n\n")).collect();
        crate::typeset::parse::parse(&format!("# First\n\n{rows}# Second\n\nUnder it.\n\n{rows}"))
    };
    let mut then = opens([other]);
    let keys: [&[u8]; 3] = [b"\t", b"\r", b""];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |_| {});
    let screens = terminal.screens();
    let last = screens.last().unwrap();
    assert!(top_row_of(last).contains("Second"), "{last:?}");
    assert!(status_of(last, 8).contains("docs/other.md"), "{last:?}");
}

#[test]
fn an_anchor_missing_in_the_opened_file_is_told_and_the_view_is_at_its_start() {
    let base = with_rows("[go](docs/other.md#nowhere)", 4);
    let mut then = opens([rows(10)]);
    let keys: [&[u8]; 3] = [b"\t", b"\r", b""];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |_| {});
    let screens = terminal.screens();
    let last = screens.last().unwrap();
    assert!(top_row_of(last).contains("Row 1"), "{last:?}");
    let bar = status_of(last, 8);
    assert!(bar.contains("no #nowhere in docs/other.md"), "{bar:?}");
}

/// A base with the link on a paragraph of its own in the middle of rows.
fn linked_in_the_middle() -> Document {
    let before: String = (1..=12).map(|n| format!("Before {n}\n\n")).collect();
    let after: String = (1..=12).map(|n| format!("After {n}\n\n")).collect();
    crate::typeset::parse::parse(&format!("{before}[go](docs/other.md)\n\n{after}"))
}

/// The rows of `screen` above the status line, without styles.
fn page_of(screen: &[String], rows: u16) -> Vec<String> {
    (screen.iter().take(usize::from(rows) - 1))
        .map(|row| unstyled(row).trim_end().to_owned())
        .collect()
}

/// The screen the reader had when the pager said it was opening a file.
fn when_opening(screens: &[Vec<String>], rows: u16) -> Vec<String> {
    let busy = |screen: &&Vec<String>| status_of(screen, rows).contains("opening ");
    page_of(screens.iter().find(busy).expect("an opening frame"), rows)
}

#[test]
fn backspace_comes_back_to_the_same_place_in_the_base() {
    let base = linked_in_the_middle();
    let mut then = opens([rows(30)]);
    // `d` scrolls half a page; the link is on the screen after two.
    let keys: [&[u8]; 6] = [b"d", b"d", b"\t", b"\r", b"", BACKSPACE];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |_| {});
    let screens = terminal.screens();
    let place = when_opening(&screens, 8);
    assert_ne!(place, page_of(&screens[0], 8), "the keys scrolled the base");
    assert!(place.iter().any(|row| row.contains("go")), "{place:?}");
    let last = screens.last().unwrap();
    assert_eq!(page_of(last, 8), place);
    // The base's own name, and its link is still the one selected.
    assert!(status_of(last, 8).contains("notes.md"), "{last:?}");
}

#[test]
fn backspace_after_a_resize_comes_back_to_the_same_block() {
    let base = linked_in_the_middle();
    let mut then = opens([rows(30)]);
    let mut terminal = Fake::new(60, 8, &[b"d", b"d", b"\t", b"\r", b"", BACKSPACE]);
    // The window narrows while the opened view is on top.
    let wide = window(60, 8);
    terminal.sizes = [wide, wide, wide, wide, window(50, 8)].into();
    let mut inputs = ansi();
    let options = Options {
        files: Some(&mut then),
        ..Options::new(SearchCase::Smart)
    };
    run_document(&base, &mut inputs, options, &mut terminal);
    let screens = terminal.screens();
    let place = when_opening(&screens, 8);
    let last = screens.last().unwrap();
    assert_eq!(page_of(last, 8), place);
    let widest = (last.iter()).map(|row| unstyled(row).trim_end().chars().count());
    assert!(
        widest.max() <= Some(50),
        "laid out for the narrower window: {last:?}"
    );
}

#[test]
fn a_missing_file_is_told_and_nothing_is_pushed() {
    let base = with_rows("[go](docs/other.md)", 8);
    let mut then = Then::new([]).opening([Reloaded::Failed("No such file".to_owned())]);
    let (mut sheets, mut views) = (Vec::new(), 0);
    let keys: [&[u8]; 4] = [b"\t", b"\r", b"", BACKSPACE];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |rig| {
        sheets = rig.state.deck.sheets().collect();
        views = rig.session.sources().len();
    });
    let screens = terminal.screens();
    // The frame after the failed open: the base, and why.
    let told = (screens.iter()).find(|screen| status_of(screen, 8).contains("cannot open"));
    let told = told.expect("a frame that tells why");
    assert!(status_of(told, 8).contains("cannot open docs/other.md: No such file"));
    assert!(screen_text(told).contains("go"), "{told:?}");
    assert_eq!(sheets, [Sheet::Base]);
    assert_eq!(views, 1, "no source is left of the file");
    let last = screens.last().unwrap();
    let bar = status_of(last, 8);
    assert!(bar.contains("nothing to go back to"), "{bar:?}");
}

#[test]
fn opening_the_same_file_again_opens_another_view() {
    // `notes.md` is the base; `b.md` links back to it.
    let base = with_rows("[to b](b.md)", 5);
    let b = with_rows("[back to notes](notes.md)", 5);
    let mut then = opens([b, with_rows("Third", 5)]);
    let keys: [&[u8]; 7] = [b"\t", b"\r", b"\t", b"\r", b"", BACKSPACE, BACKSPACE];
    let mut deck = Vec::new();
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |rig| {
        deck = rig.state.deck.sheets().collect();
    });
    let opened: Vec<_> = then.opened.iter().collect();
    let expected = [(PathBuf::from("b.md"), 1), (PathBuf::from("notes.md"), 2)];
    assert_eq!(opened, expected.iter().collect::<Vec<_>>());
    let screens = terminal.screens();
    let texts: Vec<String> = screens.iter().map(|s| screen_text(s)).collect();
    let third = texts
        .iter()
        .position(|text| text.contains("Third"))
        .unwrap();
    assert!(
        texts[third..]
            .iter()
            .any(|text| text.contains("back to notes"))
    );
    // Two Backspaces bring the reader to the base, and not to a view.
    assert!(texts.last().unwrap().contains("to b"), "{:?}", texts.last());
    assert_eq!(deck, [Sheet::Base]);
}

#[test]
fn a_link_in_an_opened_view_is_resolved_against_its_own_directory() {
    let base = with_rows("[to docs](docs/b.md)", 3);
    let b = with_rows("[to c](c.md) and [up](../top.md)", 3);
    let mut then = opens([b, with_rows("C", 3)]);
    let keys: [&[u8]; 4] = [b"\t", b"\r", b"\t", b"\r"];
    keyed(&base, (&mut ansi(), 8), &mut then, &keys, |_| {});
    let opened: Vec<_> = then.opened.iter().map(|(path, _)| path.clone()).collect();
    assert_eq!(
        opened,
        [PathBuf::from("docs/b.md"), PathBuf::from("docs/c.md")]
    );
}

#[test]
fn jumps_inside_an_opened_view_come_back_before_the_view_does() {
    let base = with_rows("[to b](b.md)", 5);
    let rows: String = (1..=12).map(|n| format!("Row {n}\n\n")).collect();
    let b = crate::typeset::parse::parse(&format!(
        "[down](#target)\n\n{rows}# Target\n\nUnder it.\n\n{rows}"
    ));
    let mut then = opens([b]);
    // Open; jump down in the opened view; back there; back to the base.
    let keys: [&[u8]; 7] = [b"\t", b"\r", b"\t", b"\r", BACKSPACE, BACKSPACE, b""];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |_| {});
    let screens = terminal.screens();
    let tops: Vec<String> = screens.iter().map(|s| top_row_of(s)).collect();
    let target = tops.iter().position(|top| top.contains("Target")).unwrap();
    let after = &screens[target + 1];
    assert!(top_row_of(after).contains("down"), "back in the view");
    let bar = status_of(after, 8);
    // The path ends in the view that is shown.
    assert!(bar.contains(" notes.md › b.md "), "{bar:?}");
    let last = screens.last().unwrap();
    assert!(top_row_of(last).contains("to b"), "{last:?}");
    assert!(status_of(last, 8).contains("notes.md"), "{last:?}");
}

#[test]
fn escape_does_not_close_an_opened_view() {
    let base = with_rows("[go](docs/other.md)", 5);
    let mut then = opens([rows(5)]);
    let keys: [&[u8]; 5] = [b"\t", b"\r", b"", b"\x1b", b""];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |_| {});
    let screens = terminal.screens();
    let last = screens.last().unwrap();
    assert!(status_of(last, 8).contains("docs/other.md"), "{last:?}");
    assert!(screen_text(last).contains("Row 1"), "{last:?}");
}

#[test]
fn opening_and_returning_by_keys_sends_no_image_of_the_base_again() {
    let document = crate::typeset::parse::parse(
        "[go](docs/other.md)\n\nAbove\n\n![a square](square.png)\n\nBelow\n",
    );
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs
        .images
        .insert("square.png", Ok(square([255, 0, 0, 255])));
    let mut then = opens([rows(3)]);
    let keys: [&[u8]; 5] = [b"\t", b"\r", b"", BACKSPACE, b""];
    let terminal = keyed(&document, (&mut inputs, 12), &mut then, &keys, |_| {});
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    // The images go when the reader quits, which is not in question.
    let (out, _) = until_quit(&out);
    assert_eq!(sent_ids(out).len(), 1, "sent once, at first");
    assert!(deleted_ids(out).is_empty(), "{:?}", deleted_ids(out));
    let screens = terminal.screens();
    let last = screen_text(screens.last().unwrap());
    assert!(last.contains("go"), "back in the base: {last}");
}

#[test]
fn an_animation_started_by_tab_and_enter_is_stopped_when_a_link_opens_a_view() {
    use crate::typeset::images::animation::Plays;
    let (_, mut inputs) = animations::animated(0, Plays::Forever);
    let base =
        crate::typeset::parse::parse("![an animation](anim.gif)\n\n[go](docs/other.md)\n\nBelow\n");
    let mut then = opens([rows(3)]);
    // Tab to the picture, Enter to play it, Tab to the link, Enter to open.
    let keys: [&[u8]; 8] = [b"\t", b"\r", b"", b"", b"\t", b"\r", b"", b""];
    let terminal = keyed(&base, (&mut inputs, 14), &mut then, &keys, |_| {});
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    // The read of the Enter that opens the view; what is written after it
    // is the view's.
    let opened = terminal.marks[5].0;
    let started = animations::controlled(&out[..opened]);
    let plays = |(_, keys): &&(u32, String)| keys.starts_with("c=1,s=3");
    let ids: std::collections::BTreeSet<u32> =
        (started.iter().filter(plays)).map(|(id, _)| *id).collect();
    assert!(!ids.is_empty(), "the picture plays before the view opens");
    // What the view's first round leaves in the terminal is not playing:
    // each id the picture was started under is stopped, or deleted. Tab
    // and Enter send it under more than one id, as selecting it changes
    // how it is drawn, and the layout lets the stale one go.
    let end = until_quit(&out).0.len();
    let (under, deleted) = (&out[opened..end], deleted_ids(&out[opened..end]));
    let stopped = animations::controlled(under);
    for id in ids {
        let stop = stopped.contains(&(id, "s=1".to_owned()));
        assert!(
            stop || deleted.contains(&id),
            "{id}: {stopped:?} {deleted:?}"
        );
    }
}

#[test]
fn backspace_goes_back_to_the_base_when_the_history_has_run_out_in_an_opened_view() {
    let base = with_rows("[to b](b.md)", 5);
    let b = crate::typeset::parse::parse("[again](#top)\n\n# Top\n\nRow\n");
    let mut then = opens([b]);
    // More jumps than the history keeps push the place of the opening out.
    let mut keys: Vec<&[u8]> = vec![b"\t", b"\r"];
    for _ in 0..History::MOST + 1 {
        keys.extend([b"\t".as_slice(), b"\r"]);
    }
    keys.extend(std::iter::repeat_n(BACKSPACE, History::MOST));
    keys.extend([BACKSPACE, b""]);
    let mut deck = Vec::new();
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |rig| {
        deck = rig.state.deck.sheets().collect();
    });
    let screens = terminal.screens();
    let last = screens.last().unwrap();
    assert!(top_row_of(last).contains("to b"), "{last:?}");
    assert_eq!(deck, [Sheet::Base]);
}

/// `before` as the first word of each of the 12 paragraphs above the link
/// and of the 12 below it.
fn linked_with(word: &str) -> Document {
    let before: String = (1..=12).map(|n| format!("{word} {n}\n\n")).collect();
    let after: String = (1..=12).map(|n| format!("After {n}\n\n")).collect();
    crate::typeset::parse::parse(&format!("{before}[go](docs/other.md)\n\n{after}"))
}

fn look(length: u64) -> Change {
    Change::Changed(crate::session::state::Look {
        modified: None,
        length,
    })
}

/// Opens `docs/other.md` from the base `linked_with("Before")` with the
/// view scrolled, polls once, and goes back, and then polls `polls` times,
/// following as `follow` says, with `then` for the files.
fn following_a_round_trip(follow: settings::Follow, then: &mut Then, polls: usize) -> Fake {
    let base = linked_with("Before");
    let mut keys: Vec<&[u8]> = vec![b"d", b"d", b"\t", b"\r", b"", BACKSPACE];
    keys.extend(std::iter::repeat_n(b"".as_slice(), polls));
    let options = Options {
        files: Some(then),
        follow,
        ..Options::new(SearchCase::Smart)
    };
    keyed_with(&base, (&mut ansi(), 8), options, &keys, |_| {})
}

fn fresh_base() -> Document {
    linked_with("Fresh")
}

#[test]
fn r_reads_again_the_opened_file_only() {
    let base = with_rows("[go](docs/other.md)", 8);
    let newer = crate::typeset::parse::parse("Reloaded base\n\n[go](docs/other.md)\n");
    let mut then = Then::new([Reloaded::Document(rows(4)), Reloaded::Document(newer)])
        .opening([Reloaded::Document(rows(3))]);
    let keys: [&[u8]; 8] = [b"\t", b"\r", b"", b"r", b"", BACKSPACE, b"r", b""];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |_| {});
    let paths: Vec<_> = then.jobs.iter().map(|job| job.path.clone()).collect();
    assert_eq!(
        paths,
        [PathBuf::from("docs/other.md"), PathBuf::from("notes.md")]
    );
    let screens = terminal.screens();
    let busy: Vec<usize> = (0..screens.len())
        .filter(|&at| status_of(&screens[at], 8).contains("reloading"))
        .collect();
    assert_eq!(busy.len(), 2, "{busy:?}");
    // The base was as it was before its own `r`.
    let before = screen_text(&screens[busy[1] - 1]);
    assert!(
        before.contains("go") && !before.contains("Reloaded"),
        "{before}"
    );
    assert!(screen_text(screens.last().unwrap()).contains("Reloaded base"));
}

#[test]
fn returning_in_follow_mode_reads_the_base_again_when_its_file_changed() {
    let mut then = Then::new([Reloaded::Document(fresh_base())])
        .opening([Reloaded::Document(rows(3))])
        .changing_at("notes.md", 1, [look(7), look(7)]);
    let terminal = following_a_round_trip(settings::Follow::On, &mut then, 5);
    assert_eq!(then.asked, 1, "the base was read again, once");
    assert_eq!(then.jobs[0].path, Path::new("notes.md"));
    let screens = terminal.screens();
    let last = screen_text(screens.last().unwrap());
    // The place was kept: the paragraphs around the link, which is selected
    // still, are those of the new text.
    assert!(last.contains("Fresh 12") && last.contains("go"), "{last}");
    assert!(!last.contains("Before"), "{last}");
}

#[test]
fn returning_in_follow_mode_does_not_read_an_unchanged_base_again() {
    let mut then = Then::new([])
        .opening([Reloaded::Document(rows(3))])
        .changing_at("notes.md", 1, []);
    following_a_round_trip(settings::Follow::On, &mut then, 5);
    assert_eq!(then.asked, 0, "nothing was read again");
    let looked = then.looked_at.iter().filter(|path| *path == "notes.md");
    assert!(looked.count() > 0, "the base's file was looked at");
}

#[test]
fn returning_without_follow_mode_leaves_a_changed_base_as_it_was() {
    let mut then = Then::new([Reloaded::Document(fresh_base())])
        .opening([Reloaded::Document(rows(3))])
        .changing_at("notes.md", 1, [look(7), look(7)]);
    let terminal = following_a_round_trip(settings::Follow::Off, &mut then, 5);
    assert_eq!(then.asked, 0);
    assert!(then.looked_at.is_empty(), "{:?}", then.looked_at);
    let screens = terminal.screens();
    let last = screen_text(screens.last().unwrap());
    assert!(last.contains("Before") && !last.contains("Fresh"), "{last}");
}

/// The caps of a terminal with reading mode's pages, in a window of 80
/// columns.
fn paged() -> Inputs {
    let caps = Capabilities {
        cell_pixels: Some((10, 20)),
        hyperlinks: true,
        ..Capabilities::kitty(80)
    };
    Inputs::new(caps)
}

/// The base with a link, and the keys that open it and then come back
/// after `between`.
fn open_between(between: &[&'static [u8]]) -> Vec<&'static [u8]> {
    let mut keys: Vec<&'static [u8]> = vec![b"\t", b"\r", b""];
    keys.extend(between);
    keys.extend([BACKSPACE, b""]);
    keys
}

/// The frames of the base `with_rows` with a link, shown in 80 × 10,
/// where each frame's page or text is seen, as the keys say; reading mode
/// is on when `reading`.
fn seen_in(reading: bool, keys: &[&[u8]]) -> Fake {
    let base = with_rows("[go](docs/other.md)", 3);
    let mut then = opens([rows(3)]);
    let mut inputs = paged();
    let mode = reading::reading();
    let options = Options {
        files: Some(&mut then),
        reading: (!reading).then_some(mode.clone()),
        ..Options::new(SearchCase::Smart)
    };
    inputs.reading = reading.then_some(mode);
    keyed_with(&base, (&mut inputs, 10), options, keys, |_| {})
}

#[test]
fn the_opened_view_is_in_reading_mode_when_the_base_was() {
    let terminal = seen_in(true, &[b"\t", b"\r", b""]);
    let seen = terminal.seen();
    assert!(reading::shows_page(&seen[0]), "the base is in reading mode");
    let last = seen.last().unwrap();
    assert!(reading::shows_page(last), "{}", last.text());
    assert!(last.row(10).contains("docs/other.md"), "{}", last.row(10));
}

#[test]
fn the_opened_view_is_in_text_mode_when_the_base_was() {
    let terminal = seen_in(false, &[b"\t", b"\r", b""]);
    let seen = terminal.seen();
    assert!(!reading::shows_page(&seen[0]));
    let last = seen.last().unwrap();
    assert!(!reading::shows_page(last), "{}", last.text());
    assert!(last.text().contains("Row 1"), "{}", last.text());
    assert!(last.row(10).contains("docs/other.md"), "{}", last.row(10));
}

#[test]
fn reading_mode_turned_in_an_opened_view_holds_on_return() {
    // One setting for all the views: the base is laid out anew in text.
    let terminal = seen_in(true, &open_between(&[b"M", b""]));
    let seen = terminal.seen();
    let last = seen.last().unwrap();
    assert!(!reading::shows_page(last), "{}", last.text());
    assert!(
        last.text().contains("go"),
        "back in the base: {}",
        last.text()
    );
}

/// The left button pressed and let go at `column` of `row`, counted from 1.
fn click(column: u16, row: u16) -> Vec<u8> {
    format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m").into_bytes()
}

#[test]
fn a_click_on_a_markdown_link_opens_it_as_a_view_and_backspace_goes_back() {
    let base = with_rows("[go](docs/other.md) to the other.", 20);
    let mut then = opens([rows(3)]);
    let mut views = Vec::new();
    let click = click(4, 1);
    let keys: [&[u8]; 3] = [&click, BACKSPACE, b""];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |rig| {
        views = rig.state.deck.sheets().collect();
    });
    assert_eq!(then.opened, [(PathBuf::from("docs/other.md"), 1)]);
    let screens = terminal.screens();
    let opened = screen_text(&screens[screens.len() - 2]);
    assert!(opened.contains("Row 1"), "{opened}");
    let last = screen_text(screens.last().unwrap());
    assert!(last.contains("to the other"), "{last}");
    assert_eq!(views, [Sheet::Base], "the view is closed again");
}

#[test]
fn a_click_beside_a_markdown_link_opens_nothing() {
    let base = with_rows("[go](docs/other.md) to the other.", 20);
    let mut then = opens([rows(3)]);
    let click = click(30, 1);
    let keys: [&[u8]; 2] = [&click, b""];
    keyed(&base, (&mut ansi(), 8), &mut then, &keys, |_| {});
    assert!(then.opened.is_empty(), "{:?}", then.opened);
}

/// A base with a link to `docs/other.md` on row 5 once the view is at the
/// end, and what the other file and the third are: the second links to
/// `third.md`, which is `docs/third.md`.
fn three() -> (Document, Then) {
    let rows: String = (1..=20).map(|n| format!("Row {n}\n\n")).collect();
    let base = crate::typeset::parse::parse(&format!("{rows}[go](docs/other.md) at the end.\n"));
    let other = with_rows("[next](third.md) to the third.", 20);
    (base, opens([other, rows_of(3)]))
}

fn rows_of(count: usize) -> Document {
    with_rows("Third", count)
}

/// What the status line of `screen` says, in a window of 8 rows.
fn bar(screen: &[String]) -> String {
    status_of(screen, 8)
}

#[test]
fn the_status_line_has_no_path_with_one_view() {
    let (base, mut then) = three();
    let keys: [&[u8]; 2] = [b"G", b""];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |_| {});
    for screen in terminal.screens() {
        assert!(!bar(&screen).contains('›'), "{screen:?}");
    }
}

#[test]
fn the_status_line_tells_the_path_of_two_and_of_three_views() {
    let (base, mut then) = three();
    let (go, next) = (click(4, 5), click(4, 1));
    let keys = [b"G".as_slice(), &go, &next, b""];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |_| {});
    let screens = terminal.screens();
    let paths: Vec<String> = (screens.iter().map(|screen| bar(screen)))
        .filter(|bar| bar.contains('›'))
        .collect();
    assert!(
        paths
            .iter()
            .any(|bar| bar.contains(" notes.md › docs/other.md ")),
        "{paths:#?}"
    );
    let last = bar(screens.last().unwrap());
    assert!(
        last.contains(" notes.md › docs/other.md › docs/third.md "),
        "{last:?}"
    );
    // The last name is in bold, the others are not.
    let frame = terminal.frames().pop().unwrap();
    assert!(frame.contains("\x1b[0;1;7mdocs/third.md"), "{frame:?}");
}

#[test]
fn an_anchor_jump_in_a_view_adds_nothing_to_the_path() {
    let rows: String = (1..=12).map(|n| format!("Row {n}\n\n")).collect();
    let base = crate::typeset::parse::parse(&format!("[go](#target)\n\n{rows}# Target\n\n{rows}"));
    let mut then = opens([]);
    let go = click(4, 1);
    let keys: [&[u8]; 2] = [&go, b""];
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |_| {});
    let screens = terminal.screens();
    assert!(top_row_of(screens.last().unwrap()).contains("Target"));
    assert!(!bar(screens.last().unwrap()).contains('›'));
}

#[test]
fn a_narrow_window_cuts_the_start_of_the_path() {
    let (base, mut then) = three();
    let (go, next) = (click(4, 5), click(4, 1));
    let keys = [b"G".as_slice(), &go, &next, b""];
    let mut terminal = Fake::new(40, 8, &keys);
    let options = Options {
        files: Some(&mut then),
        ..Options::new(SearchCase::Smart)
    };
    let mut loads = Loads::inline_builds();
    let session = session_for(&options);
    let content = Content::Document(&base);
    let mut inputs = ansi();
    let mut driver = Driver::new(
        content,
        None,
        session,
        &mut inputs,
        options,
        &mut loads,
        &mut terminal,
    );
    while driver.round().unwrap() {}
    drop(driver);
    loads.halt();
    loads.wait(crate::images::STOPPING);
    let screens = terminal.screens();
    let last = status_of(screens.last().unwrap(), 8);
    assert!(last.contains("… › docs/third.md"), "{last:?}");
    assert!(!last.contains("notes.md"), "{last:?}");
}

/// The screens when `keys` follow the two links, and then `after`: the
/// states of the deck at the end, and the screens.
fn after_two(after: &[&[u8]]) -> (Vec<Sheet>, Vec<Vec<String>>) {
    let (base, mut then) = three();
    let (go, next) = (click(4, 5), click(4, 1));
    let mut keys = vec![b"G".as_slice(), &go, &next];
    keys.extend(after);
    keys.push(b"");
    let mut views = Vec::new();
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |rig| {
        views = rig.state.deck.sheets().collect();
    });
    (views, terminal.screens())
}

#[test]
fn a_click_on_the_first_name_goes_back_to_it_where_backspace_repeated_does() {
    let (by_keys, keyed) = after_two(&[BACKSPACE, BACKSPACE]);
    // `notes.md` is on columns 8 to 15 of the status line, on row 8.
    let name = click(10, 8);
    let (by_click, clicked) = after_two(&[&name]);
    assert_eq!(by_click, [Sheet::Base], "{by_click:?}");
    assert_eq!(by_keys, by_click);
    assert_eq!(clicked.last(), keyed.last(), "{clicked:#?}");
    let text = screen_text(clicked.last().unwrap());
    assert!(text.contains("go") && !text.contains("Third"), "{text}");
}

#[test]
fn a_click_on_a_name_in_the_middle_goes_back_to_that_view() {
    let (by_keys, keyed) = after_two(&[BACKSPACE]);
    // `docs/other.md` is on columns 19 to 31.
    let name = click(25, 8);
    let (by_click, clicked) = after_two(&[&name]);
    assert_eq!(by_click.len(), 2, "{by_click:?}");
    assert_eq!(by_keys, by_click);
    assert_eq!(clicked.last(), keyed.last(), "{clicked:#?}");
    let last = bar(clicked.last().unwrap());
    // The path ends in the view that is shown; the link that was followed
    // is still selected, and its address is told after it.
    assert!(last.contains("docs/other.md"), "{last:?}");
    assert!(!last.contains("docs/third.md"), "{last:?}");
}

#[test]
fn a_click_on_the_last_name_or_between_names_does_nothing() {
    let (_, kept) = after_two(&[]);
    // `docs/third.md` is on columns 35 to 47; ` › ` on 16 to 18.
    for column in [40, 17] {
        let click = click(column, 8);
        let (views, shown) = after_two(&[&click]);
        assert_eq!(views.len(), 3, "{column}: {views:?}");
        assert_eq!(shown.last(), kept.last(), "{column}");
    }
}

#[test]
fn a_click_on_the_first_name_passes_over_the_jumps_made_in_the_views_above() {
    let rows: String = (1..=12).map(|n| format!("Row {n}\n\n")).collect();
    let run = |after: &[&[u8]]| {
        let base = with_rows("[go](docs/other.md) at the start.", 20);
        let other = crate::typeset::parse::parse(&format!(
            "[down](#target) and [next](third.md)\n\n{rows}# Target\n\n{rows}"
        ));
        let mut then = opens([other, rows_of(3)]);
        let (go, down, next) = (click(4, 1), click(4, 1), click(13, 1));
        // Open; jump down in it, which puts the link above the screen; come
        // back there, and follow the second link to the third file.
        let mut keys = vec![go.as_slice(), &down, BACKSPACE, &next];
        keys.extend(after);
        keys.push(b"");
        let mut views = Vec::new();
        let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |rig| {
            views = rig.state.deck.sheets().collect();
        });
        (views, terminal.screens())
    };
    let (_, opened) = run(&[]);
    assert!(
        status_of(opened.last().unwrap(), 8).contains(" notes.md › docs/other.md › docs/third.md "),
        "{:?}",
        opened.last()
    );
    let (by_keys, keyed) = run(&[BACKSPACE, BACKSPACE]);
    let name = click(10, 8);
    let (by_click, clicked) = run(&[&name]);
    assert_eq!(by_click, [Sheet::Base]);
    assert_eq!(by_keys, by_click);
    assert_eq!(clicked.last(), keyed.last(), "{clicked:#?}");
}
