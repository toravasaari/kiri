use super::*;

/// The screens `document` shows in a window of 20 × 8 when the terminal
/// gives `keys`, with the terminal's capabilities saying it scrolls or not, and the
/// scrolls the frames hold: `(rows, 'S' or 'T')`.
fn screens_of(
    document: &Document,
    scroll: bool,
    keys: &[&[u8]],
) -> (Vec<Vec<String>>, Vec<(usize, char)>) {
    let mut terminal = Fake::new(20, 8, keys);
    let mut inputs = Inputs::new(Capabilities {
        scroll,
        ..Capabilities::ansi(80)
    });
    let options = Options::new(SearchCase::Smart);
    run_document(document, &mut inputs, options, &mut terminal);
    let mut scrolls = Vec::new();
    for frame in terminal.frames() {
        let mut rest = frame.as_str();
        while let Some(at) = rest.find("\x1b[") {
            rest = &rest[at + 2..];
            let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
            if let Some(kind @ ('S' | 'T')) = rest[digits..].chars().next() {
                scrolls.push((rest[..digits].parse().unwrap_or(1), kind));
            }
        }
    }
    (terminal.screens(), scrolls)
}

#[test]
fn a_document_scrolled_both_ways_shows_the_same_screens_with_the_terminal_scrolling() {
    let document = numbers(40);
    let keys: [&[u8]; 12] = [
        b"j", b"j", b"j", b" ", b"j", b"k", b"k", b"k", b"b", b"k", b"j", b"q",
    ];
    let (off, none) = screens_of(&document, false, &keys);
    let (on, scrolls) = screens_of(&document, true, &keys);
    assert!(none.is_empty(), "{none:?}");
    assert!(scrolls.iter().any(|(_, kind)| *kind == 'S'), "{scrolls:?}");
    assert!(scrolls.iter().any(|(_, kind)| *kind == 'T'), "{scrolls:?}");
    assert_eq!(on.len(), off.len());
    for (n, (on, off)) in on.iter().zip(&off).enumerate() {
        assert_eq!(on, off, "the screen after frame {n}");
    }
}

#[test]
fn a_terminal_that_does_not_scroll_is_written_frames_without_a_scroll() {
    let document = numbers(40);
    let keys: [&[u8]; 6] = [b"j", b"j", b" ", b"k", b"b", b"q"];
    let (screens, scrolls) = screens_of(&document, false, &keys);
    assert!(scrolls.is_empty(), "{scrolls:?}");
    assert!(screens.len() > keys.len() / 2);
}

/// The screens of `source` in a window of 30 columns and `rows` rows when
/// the terminal gives `keys`, on kitty with level one headings at scale 3,
/// scrolling the terminal or not.
fn headed(source: &str, rows: u16, scroll: bool, keys: &[&[u8]]) -> Vec<Vec<String>> {
    headed_in("[h1]\nscale = 3\n", source, rows, scroll, keys)
}

/// As [`headed`], with the theme `theme`.
fn headed_in(
    theme: &str,
    source: &str,
    rows: u16,
    scroll: bool,
    keys: &[&[u8]],
) -> Vec<Vec<String>> {
    let mut terminal = Fake::new(30, rows, keys);
    let mut inputs = Inputs {
        theme: crate::typeset::theme::parse(theme).unwrap(),
        ..Inputs::new(Capabilities {
            scroll,
            ..Capabilities::kitty(80)
        })
    };
    let document = crate::typeset::parse::parse(source);
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    terminal.screens()
}

/// The scale of the text sized on each row of `screen`: `None` for a row
/// with none.
fn row_scales(screen: &[String]) -> Vec<Option<usize>> {
    screen
        .iter()
        .map(|row| {
            let at = row.find("\x1b]66;s=")? + "\x1b]66;s=".len();
            let digits = row[at..].find(|c: char| !c.is_ascii_digit())?;
            row[at..at + digits].parse().ok()
        })
        .collect()
}

#[test]
fn a_heading_that_does_not_fit_the_rows_left_at_the_bottom_is_drawn_smaller() {
    let source = "one\n\ntwo\n\n# Big\n\nthree\n";
    // The rows above the status line: 6 leave 2 below "two" and its
    // blank line, 5 leave 1, 8 leave room for the heading as it is.
    for (rows, scale) in [(7, Some(2)), (6, None), (9, Some(3))] {
        let screens = headed(source, rows, false, &[b"q"]);
        let screen = &screens[0];
        let at = screen
            .iter()
            .position(|row| row.contains("Big"))
            .unwrap_or_else(|| panic!("the heading is missing with {rows} rows: {screen:#?}"));
        assert_eq!(
            row_scales(screen)[at],
            scale,
            "the heading with {rows} rows: {screen:#?}"
        );
        // Wherever it is, it ends above the status line.
        let status = screen.len() - 1;
        let reach = at + scale.unwrap_or(1);
        assert!(reach <= status, "{rows} rows: {screen:#?}");
    }
}

#[test]
fn a_heading_scrolled_past_the_bottom_edge_line_by_line_and_back_never_reaches_the_status_line() {
    let source = "one\n\ntwo\n\n# Big\n\nthree\n\nfour\n\nfive\n\nsix\n\nseven\n";
    let mut keys: Vec<&[u8]> = vec![b"j"; 8];
    keys.extend(vec![b"k".as_slice(); 8]);
    let off = headed(source, 7, false, &keys);
    let on = headed(source, 7, true, &keys);
    assert_eq!(on.len(), off.len());
    let mut shown = 0;
    for (n, (on, off)) in on.iter().zip(&off).enumerate() {
        assert_eq!(on, off, "the screen after frame {n}");
        let status = on.len() - 1;
        for (row, scale) in row_scales(on).iter().enumerate() {
            if let Some(scale) = scale {
                shown += 1;
                assert!(row + scale <= status, "frame {n}: {on:#?}");
            }
        }
        // Only the status line is on the status row.
        assert!(!on[status].contains("\x1b]66;"), "frame {n}: {on:#?}");
    }
    assert!(shown > 0, "the heading was never sized text: {on:#?}");
}

#[test]
fn a_heading_ending_in_code_has_no_band_after_it_when_drawn_smaller() {
    let source = "one\n\ntwo\n\n# Big `x`\n\nthree\n";
    // 7 rows leave 2 below "two": the heading is drawn at 2.
    let screens = headed(source, 7, false, &[b"q"]);
    let screen = &screens[0];
    let at = screen.iter().position(|row| row.contains("Big")).unwrap();
    assert_eq!(row_scales(screen)[at], Some(2), "{screen:#?}");
    let row = &screen[at];
    let after = &row[row.rfind("\x1b]66;").unwrap()..];
    let after = &after[after.find("\x1b\\").unwrap() + 2..];
    // Nothing is written after the code: no spaces in its colours.
    assert!(!after.contains(' '), "{row:?}");
}

/// The cells a screen row written with sized text and spaces covers.
fn row_cells(row: &str) -> usize {
    let mut cells = 0;
    let mut rest = row;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("\x1b]66;") {
            let (head, after) = after.split_once(';').unwrap();
            let scale: usize = head
                .trim_start_matches("s=")
                .split(':')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            let (text, after) = after.split_once("\x1b\\").unwrap();
            cells += scale * text.chars().count();
            rest = after;
        } else if let Some(after) = rest.strip_prefix("\x1b[") {
            let end = after.find(|c: char| c.is_ascii_alphabetic()).unwrap();
            rest = &after[end + 1..];
        } else {
            cells += 1;
            let mut chars = rest.chars();
            chars.next();
            rest = chars.as_str();
        }
    }
    cells
}

/// The cells the row with `word` covers on the first screen of `source`,
/// in a window of `rows` rows and the theme `theme`.
fn heading_cells(theme: &str, source: &str, rows: u16, word: &str) -> (usize, Option<usize>) {
    let screens = headed_in(theme, source, rows, false, &[b"q"]);
    let screen = &screens[0];
    let at = screen.iter().position(|row| row.contains(word)).unwrap();
    (row_cells(&screen[at]), row_scales(screen)[at])
}

#[test]
fn a_band_that_merged_with_the_text_reaches_the_edge_when_drawn_smaller() {
    // Text with no style of its own: the layout joins the fill to it.
    let theme = "[h1]\nscale = 3\nbold = false\ncolor = \"none\"\nbackground = \"#203040\"\n";
    let source = "one\n\ntwo\n\n# Big\n\nthree\n";
    assert_eq!(heading_cells(theme, source, 9, "Big"), (30, Some(3)));
    assert_eq!(heading_cells(theme, source, 7, "Big"), (30, Some(2)));
    assert_eq!(heading_cells(theme, source, 6, "Big").0, 30);
}

#[test]
fn a_band_with_no_fill_to_make_reaches_the_edge_when_drawn_smaller() {
    // The text takes the whole row, so the layout adds no spaces.
    let theme = "[h1]\nscale = 3\nbackground = \"#203040\"\nindent = 0\n";
    let source = "one\n\ntwo\n\n# Bigwordsab cd\n\nthree\n";
    assert_eq!(heading_cells(theme, source, 7, "Bigwordsab"), (30, Some(2)));
}
