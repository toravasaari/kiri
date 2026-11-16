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
