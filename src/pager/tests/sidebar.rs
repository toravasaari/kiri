//! The contents bar beside the document: when the window shows it, how
//! wide the document's layout is then, and that the place is kept when the
//! bar comes or goes with the window's size.

use super::*;
use crate::session::sidebar::Visibility;

const SOURCE: &str =
    "# Alpha\n\nfirst paragraph\n\n## Beta\n\nsecond paragraph\n\n## Gamma\n\nthird\n";

/// The screens of a run of `source` in a window of `columns` × 12 with the
/// bar `visibility` asks for, the terminal giving `keys`, and `sizes` for
/// the reads after the first.
fn ran(source: &str, columns: u16, visibility: Visibility, keys: &[&[u8]]) -> Fake {
    let document = crate::typeset::parse::parse(source);
    let mut terminal = Fake::new(columns, 12, keys);
    let mut inputs = Inputs::new(screen_caps(columns));
    let options = Options {
        sidebar: visibility,
        ..Options::new(SearchCase::Smart)
    };
    run_document(&document, &mut inputs, options, &mut terminal);
    terminal
}

/// The column, counted from 0, where `needle` starts in the first row of
/// `seen` that has it.
fn column_of(seen: &Seen, needle: &str) -> Option<usize> {
    seen.rows.iter().find_map(|row| row.find(needle))
}

#[test]
fn a_window_of_120_columns_shows_the_bar_beside_the_document() {
    on_both(|| {
        let terminal = ran(SOURCE, 120, Visibility::Auto, &[b"q"]);
        let seen = &terminal.seen()[0];
        // The bar: Alpha has children, so it has a fold glyph; the entries
        // are at the left, in the first 30 columns.
        let glyph = column_of(seen, "▾").expect("a fold glyph");
        assert!(glyph < 30, "{}", seen.text());
        assert!(column_of(seen, "Beta").is_some_and(|c| c < 30));
        // The document starts right of the bar.
        let first = column_of(seen, "first paragraph").expect("the document");
        assert!(first >= 30, "{first}: {}", seen.text());
    });
}

#[test]
fn a_window_of_100_columns_shows_no_bar() {
    on_both(|| {
        let terminal = ran(SOURCE, 100, Visibility::Auto, &[b"q"]);
        let seen = &terminal.seen()[0];
        assert_eq!(column_of(seen, "▾"), None, "{}", seen.text());
        assert!(column_of(seen, "first paragraph").is_some_and(|c| c < 10));
    });
}

#[test]
fn a_closed_bar_is_not_shown_in_a_wide_window() {
    on_both(|| {
        let terminal = ran(SOURCE, 140, Visibility::Closed, &[b"q"]);
        let seen = &terminal.seen()[0];
        assert_eq!(column_of(seen, "▾"), None, "{}", seen.text());
        assert!(column_of(seen, "first paragraph").is_some_and(|c| c < 10));
    });
}

#[test]
fn a_document_without_headings_has_no_bar_in_a_wide_window() {
    on_both(|| {
        let terminal = ran("just text\n\nmore text\n", 140, Visibility::Auto, &[b"q"]);
        let seen = &terminal.seen()[0];
        assert!(column_of(seen, "just text").is_some_and(|c| c < 10));
    });
}

/// A paragraph of `words` words, `word` and its number: one row in a wide
/// window and several in a narrow one.
fn long_paragraph(words: usize) -> String {
    (1..=words).map(|n| format!("w{n:03} ")).collect()
}

#[test]
fn the_layout_is_the_window_less_the_bar_open_and_the_window_when_closed_or_narrow() {
    on_both(|| {
        // 19 words of 5 columns: 94 columns of text, which the theme's
        // widest text of 100 holds, and which the 90 columns that a window
        // of 120 leaves beside the bar do not.
        let source = format!("# Top\n\n## Sub\n\n{}\n", long_paragraph(19));
        let rows_of_paragraph = |columns: u16, visibility: Visibility| {
            let terminal = ran(&source, columns, visibility, &[b"q"]);
            let seen = &terminal.seen()[0];
            (seen.rows.iter())
                .filter(|row| row.find("w0").is_some())
                .count()
        };
        // Open: the window less 30 columns of the bar.
        assert_eq!(rows_of_paragraph(120, Visibility::Auto), 2);
        assert_eq!(rows_of_paragraph(110, Visibility::Auto), 2);
        // Closed, and in a window too narrow for the bar: the window's.
        assert_eq!(rows_of_paragraph(120, Visibility::Closed), 1);
        assert_eq!(rows_of_paragraph(109, Visibility::Auto), 1);
        assert_eq!(rows_of_paragraph(100, Visibility::Auto), 1);
        // Wide enough for the theme's widest text beside the bar too.
        assert_eq!(rows_of_paragraph(140, Visibility::Auto), 1);
    });
}

#[test]
fn the_place_is_kept_when_a_resize_opens_the_bar() {
    on_both(|| {
        let source: String = std::iter::once("# Top\n\n## Sub\n\n".to_owned())
            .chain((1..=40).map(|n| format!("Paragraph {n}.\n\n")))
            .collect();
        let document = crate::typeset::parse::parse(&source);
        // Down 10 lines at 100 columns, then a window of 120, which opens
        // the bar and narrows the layout.
        let keys: &[&[u8]] = &[b"", b"jjjjjjjjjj", b"", b""];
        let mut terminal = Fake::new(100, 12, keys);
        terminal.sizes = [window(100, 12), window(100, 12), window(120, 12)].into();
        let mut inputs = Inputs::new(screen_caps(100));
        run_document(
            &document,
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        );
        let screens = terminal.seen();
        let opened = (screens
            .iter()
            .position(|seen| column_of(seen, "▾").is_some()))
        .expect("a frame with the bar");
        let (before, after) = (&screens[opened - 1], &screens[opened]);
        // The first line of the document is the same paragraph.
        let first = |seen: &Seen| {
            let text = seen.text();
            let at = text.find("Paragraph").expect("a paragraph");
            text[at..].split('.').next().unwrap_or_default().to_owned()
        };
        assert_eq!(
            first(before),
            first(after),
            "{}\n{}",
            before.text(),
            after.text()
        );
        assert_ne!(first(before), "Paragraph 1");
    });
}

#[test]
fn the_pager_shows_the_bar_beside_a_short_document() {
    // Headings of three levels, none folded, the view at the top: the
    // first heading is the one the reader is in.
    let source = "# Notes\n\nIntro text.\n\n## Setup\n\nSetup text.\n\n### Details\n\nMore.\n\n## Usage\n\nUsage text.\n";
    let terminal = ran(source, 120, Visibility::Auto, &[b"q"]);
    insta::assert_snapshot!(terminal.seen()[0].text());
}

#[test]
fn the_mark_of_the_bar_follows_the_view() {
    on_both(|| {
        let source: String = (1..=6)
            .map(|n| {
                let text: String = (1..=6).map(|p| format!("Text {n}.{p}\n\n")).collect();
                format!("# S{n}\n\n{text}")
            })
            .collect();
        let terminal = ran(&source, 120, Visibility::Auto, &[b"G", b"q"]);
        let screens = terminal.seen();
        let (first, last) = (&screens[0], screens.last().unwrap());
        // Entry `n` is on row `n` of the bar.
        let style = |seen: &Seen, n: u16| seen.style_in(n, &format!("S{n}")).cloned();
        let marked = style(first, 1);
        assert_ne!(
            marked,
            style(first, 2),
            "the first entry is marked at the top"
        );
        assert_eq!(style(first, 2), style(first, 3));
        assert_eq!(style(last, 6), marked, "the last at the end");
        assert_ne!(style(last, 1), marked);
    });
}

/// A document whose link to `#gamma` is on the first screen and whose
/// heading `Gamma` is far below it.
fn linked() -> String {
    let filler: String = (1..=40).map(|n| format!("Filler {n}.\n\n")).collect();
    format!("# Alpha\n\n[go to gamma](#gamma)\n\n## Beta\n\n{filler}## Gamma\n\nThe end.\n")
}

/// A click of the left button at `column` and `row`, from 1.
fn click(column: usize, row: usize) -> Vec<u8> {
    format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m").into_bytes()
}

#[test]
fn a_click_on_a_link_beside_the_bar_follows_it_and_a_click_on_the_bar_does_nothing() {
    on_both(|| {
        let source = linked();
        let first = ran(&source, 120, Visibility::Auto, &[b"q"]);
        let seen = &first.seen()[0];
        let row = (seen.rows.iter())
            .position(|row| row.find("go to gamma").is_some())
            .expect("the link on the screen");
        let column = seen.rows[row].find("go to gamma").unwrap();
        assert!(column >= 30, "the link is beside the bar: {column}");
        // On the link, in the document's own area: it jumps to `Gamma`.
        let on_link = click(column + 3, row + 1);
        let jumped = ran(&source, 120, Visibility::Auto, &[&on_link, b"q"]);
        let last = jumped.seen().last().unwrap();
        assert!(last.text().contains("The end."), "{}", last.text());
        assert!(!last.text().contains("go to gamma"), "{}", last.text());
        // On the bar, on the same row: nothing happens.
        let on_bar = click(5, row + 1);
        let still = ran(&source, 120, Visibility::Auto, &[&on_bar, b"q"]);
        let last = still.seen().last().unwrap();
        assert!(last.text().contains("go to gamma"), "{}", last.text());
        assert!(!last.text().contains("The end."), "{}", last.text());
    });
}

/// The screens of a run of `source` that starts at `columns` and is
/// resized to each of `sizes` after the reads, with `visibility` and `keys`.
fn ran_resized(source: &str, columns: u16, sizes: &[u16], keys: &[&[u8]]) -> Fake {
    let document = crate::typeset::parse::parse(source);
    let mut terminal = Fake::new(columns, 12, keys);
    terminal.sizes = sizes.iter().map(|&columns| window(columns, 12)).collect();
    let mut inputs = Inputs::new(screen_caps(columns));
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    terminal
}

/// Whether the last screen of `terminal` shows the bar.
fn bar_shown(terminal: &Fake) -> bool {
    column_of(terminal.seen().last().unwrap(), "▾").is_some()
}

#[test]
fn capital_t_closes_the_bar_of_a_wide_window_and_opens_it_again() {
    on_both(|| {
        let closed = ran(SOURCE, 120, Visibility::Auto, &[b"T", b"q"]);
        assert!(
            !bar_shown(&closed),
            "{}",
            closed.seen().last().unwrap().text()
        );
        let again = ran(SOURCE, 120, Visibility::Auto, &[b"T", b"T", b"q"]);
        assert!(bar_shown(&again));
    });
}

#[test]
fn capital_t_and_small_t_open_the_bar_of_a_narrow_window() {
    on_both(|| {
        for key in [b"T", b"t"] {
            let terminal = ran(SOURCE, 100, Visibility::Auto, &[key, b"q"]);
            let seen = terminal.seen().last().unwrap();
            assert!(bar_shown(&terminal), "{key:?}: {}", seen.text());
            // The document is right of the bar.
            let first = column_of(seen, "first paragraph").expect("the document");
            assert!(first >= 25, "{first}");
        }
    });
}

#[test]
fn a_bar_opened_by_hand_needs_40_columns_for_the_document_and_otherwise_the_status_line_says_why() {
    on_both(|| {
        // 59 columns: a bar of 19 leaves 40.
        let fits = ran(SOURCE, 59, Visibility::Auto, &[b"T", b"q"]);
        assert!(bar_shown(&fits), "{}", fits.seen().last().unwrap().text());
        // 58 columns: it would leave 39.
        for key in [b"T", b"t"] {
            let terminal = ran(SOURCE, 58, Visibility::Auto, &[key, b"q"]);
            let seen = terminal.seen().last().unwrap();
            assert!(!bar_shown(&terminal), "{}", seen.text());
            assert!(
                seen.text().contains("too narrow for the contents"),
                "{}",
                seen.text()
            );
        }
    });
}

#[test]
fn a_resize_does_not_undo_what_the_reader_chose() {
    on_both(|| {
        // Opened by hand in a narrow window, which then grows and shrinks.
        let opened = ran_resized(SOURCE, 100, &[100, 140, 90], &[b"T", b"", b"", b"q"]);
        let screens = opened.seen();
        assert!(column_of(&screens[screens.len() - 1], "▾").is_some());
        assert!(
            screens
                .iter()
                .skip(1)
                .all(|seen| column_of(seen, "▾").is_some())
        );
        // Closed in a wide window, which then grows.
        let closed = ran_resized(SOURCE, 120, &[120, 160, 200], &[b"T", b"", b"", b"q"]);
        assert!(!bar_shown(&closed));
        assert!(
            closed
                .seen()
                .iter()
                .skip(1)
                .all(|seen| column_of(seen, "▾").is_none())
        );
    });
}

/// The bar's cells of each row of `seen`: the first 30 columns.
fn bar_of(seen: &Seen) -> String {
    let rows: Vec<String> = seen
        .rows
        .iter()
        .map(|row| row.cells.iter().take(30).map(|c| c.text.as_str()).collect())
        .collect();
    rows.join("\n")
}

/// The document's cells of each row of `seen`: what is right of the bar.
fn document_of(seen: &Seen) -> String {
    let rows: Vec<String> = seen
        .rows
        .iter()
        .map(|row| row.cells.iter().skip(30).map(|c| c.text.as_str()).collect())
        .collect();
    rows.join("\n")
}

#[test]
fn with_the_focus_in_the_bar_the_keys_of_the_bar_do_not_scroll_the_document() {
    on_both(|| {
        let source = linked();
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        let keyed = ran(&source, 120, Visibility::Auto, &[b"t", b"jjk", b"q"]);
        let screens = keyed.seen();
        assert_eq!(
            document_of(screens.last().unwrap()),
            document_of(&top.seen()[0]),
            "{}",
            screens.last().unwrap().text()
        );
        // With the focus in the document the same keys scroll it.
        let scrolled = ran(&source, 120, Visibility::Auto, &[b"jjk", b"q"]);
        assert_ne!(
            document_of(scrolled.seen().last().unwrap()),
            document_of(&top.seen()[0])
        );
    });
}

#[test]
fn after_zero_the_keys_are_the_documents_again_and_the_bar_stays() {
    on_both(|| {
        let source = linked();
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        for back in [&b"0"[..], b"\x1b"] {
            let keyed = ran(
                &source,
                120,
                Visibility::Auto,
                &[b"t", back, b"", b"jj", b"q"],
            );
            let last = keyed.seen().last().unwrap();
            assert!(column_of(last, "▾").is_some(), "{}", last.text());
            assert_ne!(
                document_of(last),
                document_of(&top.seen()[0]),
                "{back:?}: j scrolled the document"
            );
        }
    });
}

#[test]
fn left_folds_the_entry_in_the_bar_and_right_opens_it_again() {
    on_both(|| {
        let folded = ran(SOURCE, 120, Visibility::Auto, &[b"t", b"h", b"q"]);
        let bar = bar_of(folded.seen().last().unwrap());
        assert!(bar.contains('▸'), "{bar}");
        assert!(!bar.contains("Beta"), "{bar}");
        let opened = ran(SOURCE, 120, Visibility::Auto, &[b"t", b"h", b"l", b"q"]);
        let bar = bar_of(opened.seen().last().unwrap());
        assert!(bar.contains('▾') && bar.contains("Beta"), "{bar}");
        // The arrows do the same.
        let arrows = ran(SOURCE, 120, Visibility::Auto, &[b"t", b"\x1b[D", b"q"]);
        assert!(bar_of(arrows.seen().last().unwrap()).contains('▸'));
    });
}

/// `count` sections, each of a heading and a line.
fn many(count: usize) -> String {
    (0..count)
        .map(|n| format!("# H{n:02}\n\nline\n\n"))
        .collect()
}

#[test]
fn a_bar_longer_than_the_window_scrolls_to_keep_the_selection_in_view() {
    on_both(|| {
        let source = many(30);
        let last = ran(&source, 120, Visibility::Auto, &[b"t", b"G", b"q"]);
        let bar = bar_of(last.seen().last().unwrap());
        assert!(bar.contains("H29") && !bar.contains("H00"), "{bar}");
        let first = ran(&source, 120, Visibility::Auto, &[b"t", b"G", b"g", b"q"]);
        assert!(bar_of(first.seen().last().unwrap()).contains("H00"));
        // Down by down, the selection never leaves the window.
        let mut downs: Vec<&[u8]> = vec![b"t"];
        downs.extend(vec![&b"j"[..]; 20]);
        downs.push(b"q");
        let moved = ran(&source, 120, Visibility::Auto, &downs);
        let bar = bar_of(moved.seen().last().unwrap());
        assert!(bar.contains("H20"), "{bar}");
    });
}

#[test]
fn keys_that_are_not_the_bars_go_to_the_document_while_the_bar_has_the_focus() {
    on_both(|| {
        let source = linked();
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        let paged = ran(&source, 120, Visibility::Auto, &[b"t", b" ", b"q"]);
        assert_ne!(
            document_of(paged.seen().last().unwrap()),
            document_of(&top.seen()[0])
        );
    });
}

#[test]
fn the_selection_in_the_bar_shows_apart_from_the_entry_the_reader_is_in() {
    on_both(|| {
        let source: String = (1..=6)
            .map(|n| format!("# S{n}\n\nText {n}.\n\n"))
            .collect();
        let style = |seen: &Seen, n: u16| seen.style_in(n, &format!("S{n}")).cloned();
        let screens = ran(&source, 120, Visibility::Auto, &[b"t", b"jj", b"q"]);
        let (before, focused) = (&screens.seen()[0], screens.seen().last().unwrap());
        // Unfocused, the first entry has the mark; focused, the third has it
        // and the first has a quieter look, which the others do not have.
        let (mark, quiet, page) = (style(before, 1), style(focused, 1), style(focused, 4));
        assert_eq!(style(focused, 3), mark);
        assert_ne!(quiet, mark);
        assert_ne!(quiet, page);
        assert_ne!(mark, page);
    });
}

/// The entries of the bar, `names`, that have a look of their own: the
/// mark, which the one the reader is in has.
fn marked(seen: &Seen, names: &[&str]) -> Vec<String> {
    // A blank row of the bar, below the entries, has the page's look.
    let page = seen.rows[10].cells.get(5).map(|cell| cell.style.clone());
    let styles = (1u16..)
        .zip(names)
        .map(|(row, name)| seen.style_in(row, name).cloned());
    names
        .iter()
        .zip(styles)
        .filter(|(_, style)| *style != page)
        .map(|(name, _)| (*name).to_owned())
        .collect()
}

/// `# Aaa`, `paragraphs` paragraphs and then `tail`.
fn ending(paragraphs: usize, tail: &str) -> String {
    let body: String = (1..=paragraphs)
        .map(|n| format!("Paragraph {n}.\n\n"))
        .collect();
    format!("# Aaa\n\n{body}{tail}")
}

const BACKSPACE: &[u8] = b"\x7f";

#[test]
fn enter_in_the_bar_goes_to_the_heading_and_gives_the_focus_back() {
    on_both(|| {
        let source = linked();
        let jumped = ran(&source, 120, Visibility::Auto, &[b"t", b"G", b"\r", b"q"]);
        let last = jumped.seen().last().unwrap();
        assert!(document_of(last).contains("The end."), "{}", last.text());
        assert!(
            !document_of(last).contains("go to gamma"),
            "{}",
            last.text()
        );
        // The focus is the document's: `k` scrolls it, the bar keeps its selection.
        let up = ran(
            &source,
            120,
            Visibility::Auto,
            &[b"t", b"G", b"\r", b"k", b"q"],
        );
        assert_ne!(
            document_of(up.seen().last().unwrap()),
            document_of(last),
            "k moved the document"
        );
    });
}

#[test]
fn backspace_after_a_jump_from_the_bar_returns_to_where_it_was() {
    on_both(|| {
        let source = linked();
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        let keys: &[&[u8]] = &[b"t", b"G", b"\r", BACKSPACE, b"q"];
        let back = ran(&source, 120, Visibility::Auto, keys);
        assert_eq!(
            document_of(back.seen().last().unwrap()),
            document_of(&top.seen()[0]),
            "{}",
            back.seen().last().unwrap().text()
        );
    });
}

#[test]
fn the_target_of_a_jump_is_the_current_heading_and_the_mark_follows_the_view_after() {
    on_both(|| {
        let source: String = (1..=6)
            .map(|n| {
                let text: String = (1..=6).map(|p| format!("Text {n}.{p}\n\n")).collect();
                format!("# S{n}\n\n{text}")
            })
            .collect();
        let names = ["S1", "S2", "S3", "S4", "S5", "S6"];
        // Down to the fourth entry and Enter.
        let jumped = ran(&source, 120, Visibility::Auto, &[b"t", b"jjj", b"\r", b"q"]);
        assert_eq!(marked(jumped.seen().last().unwrap(), &names), ["S4"]);
        // Scrolling up moves the mark as the view moves.
        let scrolled = ran(
            &source,
            120,
            Visibility::Auto,
            &[b"t", b"jjj", b"\r", b"bb", b"q"],
        );
        let after = marked(scrolled.seen().last().unwrap(), &names);
        assert_eq!(after, ["S2"], "two pages up from S4");
    });
}

#[test]
fn at_the_end_of_the_document_the_last_heading_on_the_screen_is_the_current_one() {
    on_both(|| {
        // The review's example: the last heading is a screen from the first,
        // with a line of its own.
        let source = ending(40, "## Bbb\n\nOne line.\n");
        let names = ["Aaa", "Bbb"];
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        assert_eq!(marked(&top.seen()[0], &names), ["Aaa"]);
        let end = ran(&source, 120, Visibility::Auto, &[b"G", b"q"]);
        let seen = end.seen().last().unwrap();
        assert_eq!(marked(seen, &names), ["Bbb"], "{}", seen.text());
    });
}

#[test]
fn a_jump_can_choose_the_second_to_last_heading_of_a_dense_ending() {
    on_both(|| {
        // At the end of the document `C` is the heading last on the screen,
        // but a jump to `B`, which the view cannot put at its top, marks `B`
        // until the view moves.
        let source = ending(40, "## B\n\nb\n\n## C\n\nc\n");
        let names = ["Aaa", "B", "C"];
        let end = ran(&source, 120, Visibility::Auto, &[b"G", b"q"]);
        assert_eq!(marked(end.seen().last().unwrap(), &names), ["C"]);
        let jumped = ran(
            &source,
            120,
            Visibility::Auto,
            &[b"t", b"G", b"k", b"\r", b"q"],
        );
        let seen = jumped.seen().last().unwrap();
        assert_eq!(marked(seen, &names), ["B"], "{}", seen.text());
    });
}

#[test]
fn a_bar_longer_than_the_window_follows_the_reader_while_the_focus_is_in_the_document() {
    on_both(|| {
        let source = many(30);
        let names: Vec<String> = (0..30).map(|n| format!("H{n:02}")).collect();
        // Page by page to the end of the document: the entry the reader is in
        // is always in the bar's window, and the bar moves down with it.
        let mut keys: Vec<&[u8]> = vec![&b" "[..]; 12];
        keys.push(b"q");
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let mut lowest = 0;
        for seen in run.seen() {
            let bar = bar_of(seen);
            let rows: Vec<&str> = bar.lines().collect();
            // The marked entry is the one whose cell style differs from a blank row's.
            let page = seen.rows[10].cells.get(5).map(|cell| cell.style.clone());
            let row = (1u16..=11).find(|&row| {
                let name = names
                    .iter()
                    .find(|name| rows[usize::from(row) - 1].contains(name.as_str()));
                name.is_some_and(|name| seen.style_in(row, name).cloned() != page)
            });
            let row = row.unwrap_or_else(|| panic!("a marked entry in view:\n{bar}"));
            let name = names
                .iter()
                .find(|name| rows[usize::from(row) - 1].contains(name.as_str()))
                .unwrap();
            let at: usize = name[1..].parse().unwrap();
            assert!(at >= lowest, "the mark went up from {lowest} to {at}");
            lowest = at;
        }
        assert!(lowest > 10, "the bar followed the document down: {lowest}");
    });
}

#[test]
fn the_help_opens_at_the_contents_when_the_bar_has_the_focus() {
    on_both(|| {
        let document = crate::typeset::parse::parse(SOURCE);
        // The text of the frames, the help being open in one of them.
        let frames = |keys: &[&[u8]]| -> Vec<String> {
            let mut terminal = Fake::new(120, 24, keys);
            let mut inputs = Inputs::new(screen_caps(120));
            let options = Options::new(SearchCase::Smart);
            run_document(&document, &mut inputs, options, &mut terminal);
            terminal.seen().iter().map(Seen::text).collect()
        };
        let helped = |frames: &[String], text: &str| frames.iter().any(|f| f.contains(text));
        let focused = frames(&[b"t", b"?", b"q"]);
        assert!(helped(&focused, "go to the heading"), "{focused:?}");
        let unfocused = frames(&[b"?", b"q"]);
        assert!(!helped(&unfocused, "go to the heading"), "{unfocused:?}");
        assert!(helped(&unfocused, "down a line"), "{unfocused:?}");
    });
}

#[test]
fn enter_after_t_at_the_end_goes_to_the_heading_the_mark_shows() {
    on_both(|| {
        let source = ending(40, "## Bbb\n\nOne line.\n");
        let keys: &[&[u8]] = &[b"G", b"t", b"\r", b"q"];
        let terminal = ran(&source, 120, Visibility::Auto, keys);
        let seen = terminal.seen().last().unwrap();
        // `Bbb` is the mark at the end, so Enter stays there: the first
        // paragraph is not on the screen.
        assert!(!seen.text().contains("Paragraph 1."), "{}", seen.text());
        assert!(document_of(seen).contains("Bbb"), "{}", seen.text());
    });
}

/// `a.md` and `b.md`, each a heading and `count` lines, shown together
/// in a window of 120 × 12 with `keys` typed.
fn ran_files(count: usize, keys: &[&[u8]]) -> Fake {
    let file = |name: &str, head: &str| File {
        name: name.to_owned(),
        document: crate::typeset::parse::parse(&format!(
            "# {head}\n\n{}",
            (1..=count)
                .map(|n| format!("{head} line {n}.\n\n"))
                .collect::<String>()
        )),
    };
    let files = [file("a.md", "Alpha"), file("b.md", "Beta")];
    let mut terminal = Fake::new(120, 12, keys);
    let mut inputs = Inputs::new(screen_caps(120));
    run(
        Content::Files(&files),
        None,
        Session::new(["a.md", "b.md"]),
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut Loads::new(),
        &mut terminal,
    )
    .unwrap();
    terminal
}

#[test]
fn enter_on_a_file_entry_goes_to_the_start_of_the_file_and_backspace_returns() {
    on_both(|| {
        let top = ran_files(12, &[b"q"]);
        // File `a.md`, its heading, file `b.md`: `t jj` selects `b.md`.
        let keys: &[&[u8]] = &[b"t", b"jj", b"\r", b"q"];
        let jumped = ran_files(12, keys);
        let seen = jumped.seen().last().unwrap();
        assert!(document_of(seen).contains("b.md"), "{}", seen.text());
        assert!(!document_of(seen).contains("Alpha line"), "{}", seen.text());
        let keys: &[&[u8]] = &[b"t", b"jj", b"\r", BACKSPACE, b"q"];
        let back = ran_files(12, keys);
        assert_eq!(
            document_of(back.seen().last().unwrap()),
            document_of(&top.seen()[0]),
            "{}",
            back.seen().last().unwrap().text()
        );
    });
}
