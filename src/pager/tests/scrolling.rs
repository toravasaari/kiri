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
    let mut terminal = Fake::new(30, rows, keys);
    let mut inputs = Inputs {
        theme: crate::typeset::theme::parse("[h1]\nscale = 3\n").unwrap(),
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
