//! Following a link to a Markdown file with Enter: the file opens as a view
//! over the one below, at the anchor of the link, and Backspace goes back
//! to the place the link was followed from. The files are those a `Then`
//! answers with, so no file system is read.

use super::*;
use crate::session::deck::Sheet;

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
    let mut terminal = Fake::new(60, rows, keys);
    let options = Options {
        files: Some(then),
        ..Options::new(SearchCase::Smart)
    };
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
    assert!(bar.contains("b.md") && !bar.contains("notes.md"), "{bar:?}");
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
