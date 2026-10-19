//! The interactive pager: the document on a screen of its own, scrolled
//! with keys.

pub mod keys;
pub mod status;
pub mod view;

use std::io::{self, Write};
use std::time::Duration;

use crate::config::Paging;
use crate::model::Document;
use crate::render::Inputs;
use crate::render::output::{Page, Writer};
use crate::render::style::{Line, Span};
use crate::term::{Capabilities, Screen, WindowSize};
use keys::Action;
use view::View;

/// Whether a document of `rows` rows is paged: never when the output is not
/// a terminal, and with `Auto` only when it is longer than the window's
/// `height`, when that is known.
pub fn pages(paging: Paging, terminal: bool, rows: usize, height: Option<u16>) -> bool {
    match paging {
        _ if !terminal => false,
        Paging::Always => true,
        Paging::Auto => height.is_some_and(|height| rows > usize::from(height)),
        Paging::Never => false,
    }
}

/// The rows `lines` take on a terminal with `caps`.
pub fn rows(lines: &[Line], caps: &Capabilities) -> usize {
    lines
        .iter()
        .map(|line| usize::from(crate::render::output::rows(line, caps)))
        .sum()
}

/// The terminal the pager shows a document on.
pub trait Terminal: Write {
    /// The size of the window now.
    fn size(&self) -> Option<WindowSize>;
    /// The bytes of the keys pressed, waiting at most `timeout` for the
    /// first; empty when none came.
    fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>>;
}

impl<W: Write> Terminal for Screen<W> {
    fn size(&self) -> Option<WindowSize> {
        Screen::size(self)
    }

    fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        Screen::read(self, timeout)
    }
}

/// Shows `document`, called `name`, on `terminal` until the reader quits,
/// laid out with `inputs` for the window's size, or `width` columns when
/// it is given.
pub fn run(
    document: &Document,
    name: &str,
    inputs: &mut Inputs,
    width: Option<u16>,
    terminal: &mut impl Terminal,
) -> io::Result<()> {
    let page = Page {
        background: inputs.theme.background,
        foreground: inputs.theme.foreground,
    };
    let mut size = terminal.size();
    let mut view: Option<View> = None;
    loop {
        fit(&mut inputs.caps, size, width);
        let window = usize::from(inputs.caps.height.unwrap_or(DEFAULT_ROWS));
        // The bottom row is the status line's.
        let rows = window.saturating_sub(1);
        let mut lines = crate::render::layout::layout(document, inputs);
        // The text sizing protocol cannot draw part of a line, so a line
        // taller than the window is drawn at normal size.
        for line in &mut lines {
            if usize::from(crate::render::output::rows(line, &inputs.caps)) > rows {
                line.scale = 1;
            }
        }
        let mut writer = Writer::new(&inputs.images, &inputs.caps, page);
        let heights = lines
            .iter()
            .map(|line| usize::from(writer.rows(line)))
            .collect();
        let mut shown = match view.take() {
            Some(mut view) => {
                view.resize(heights, rows);
                view
            }
            None => View::new(heights, rows),
        };
        let status = Status {
            name,
            styles: status::Styles::new(&inputs.theme, &inputs.caps),
            width: size.map_or(inputs.caps.width, |size| size.columns),
        };
        match show(&lines, &status, &mut shown, &mut writer, terminal, size)? {
            Outcome::Quit => return Ok(()),
            Outcome::Resized(new) => {
                size = new;
                view = Some(shown);
            }
        }
    }
}

/// The rows of a window whose size the terminal does not tell.
const DEFAULT_ROWS: u16 = 24;

/// How long the pager waits for a key before it looks at the window size
/// again.
const POLL: Duration = Duration::from_millis(100);

/// Why the pager stopped showing one layout of the document.
enum Outcome {
    Quit,
    /// The window is now of this size.
    Resized(Option<WindowSize>),
}

/// What the status line shows and how, apart from the position.
struct Status<'a> {
    name: &'a str,
    styles: status::Styles,
    /// The window's columns.
    width: u16,
}

impl Status<'_> {
    fn spans(&self, view: &View) -> Vec<Span> {
        let width = usize::from(self.width);
        status::spans(self.name, view.position(), width, &self.styles)
    }
}

/// `caps` for a window of `size`, `width` columns wide when it is given.
fn fit(caps: &mut Capabilities, size: Option<WindowSize>, width: Option<u16>) {
    let Some(size) = size else {
        return;
    };
    caps.width = width.unwrap_or(size.columns);
    caps.height = Some(size.rows).filter(|&rows| rows > 0);
    // A new font size changes the cells, when the window tells its pixels.
    if let Some(cell) = size.cell_pixels() {
        caps.cell_pixels = Some(cell);
    }
}

/// Shows `lines` in `view` with `status` below them on `terminal`, whose
/// window is of `size`, and scrolls them until the reader quits or the
/// window changes size.
fn show(
    lines: &[Line],
    status: &Status,
    view: &mut View,
    writer: &mut Writer,
    terminal: &mut impl Terminal,
    size: Option<WindowSize>,
) -> io::Result<Outcome> {
    draw(lines, view, &status.spans(view), writer, terminal)?;
    terminal.flush()?;
    let mut decoder = keys::Decoder::default();
    loop {
        let input = terminal.read(POLL)?;
        let top = view.top();
        // Nothing more came for a key the last read cut off.
        let keys = if input.is_empty() {
            decoder.flush()
        } else {
            decoder.keys(&input)
        };
        for key in keys {
            match keys::action(key) {
                Some(Action::Quit) => return Ok(Outcome::Quit),
                Some(Action::Move(motion)) => view.scroll(motion),
                None => {}
            }
        }
        let now = terminal.size();
        if now != size {
            return Ok(Outcome::Resized(now));
        }
        if view.top() != top {
            draw(lines, view, &status.spans(view), writer, terminal)?;
            terminal.flush()?;
        }
    }
}

/// Draws the lines `view` shows from the top of the screen, with the rows
/// below them empty and `status` on the row below the view, as one update
/// so the screen does not flicker.
pub fn draw(
    lines: &[Line],
    view: &View,
    status: &[Span],
    writer: &mut Writer,
    out: &mut impl Write,
) -> io::Result<()> {
    out.write_all(BEGIN_UPDATE)?;
    writer.clear(out)?;
    let mut top = 1;
    for line in &lines[view.visible()] {
        for row in 0..writer.rows(line) {
            write!(out, "\x1b[{};1H", top + usize::from(row))?;
            writer.row(line, row, out)?;
        }
        top += usize::from(writer.rows(line));
    }
    if !status.is_empty() {
        write!(out, "\x1b[{};1H", view.rows() + 1)?;
        writer.bar(status, out)?;
    }
    out.write_all(END_UPDATE)
}

/// Synchronized output: the terminal shows what comes between these at
/// once. Terminals without it ignore them.
const BEGIN_UPDATE: &[u8] = b"\x1b[?2026h";
const END_UPDATE: &[u8] = b"\x1b[?2026l";

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[test]
    fn auto_pages_a_document_longer_than_the_window() {
        assert!(pages(Paging::Auto, true, 25, Some(24)));
        assert!(!pages(Paging::Auto, true, 24, Some(24)));
        // Without a height, nothing tells that it is longer.
        assert!(!pages(Paging::Auto, true, 1000, None));
    }

    #[test]
    fn always_pages_even_a_short_document() {
        assert!(pages(Paging::Always, true, 1, Some(24)));
        assert!(pages(Paging::Always, true, 1, None));
    }

    #[test]
    fn nothing_is_paged_into_a_pipe_or_with_never() {
        for paging in [Paging::Always, Paging::Auto, Paging::Never] {
            assert!(!pages(paging, false, 100, Some(24)), "{paging:?}");
        }
        assert!(!pages(Paging::Never, true, 100, Some(24)));
    }

    /// A terminal that gives the reads in `keys`, one a call, with the
    /// window sizes in `sizes` after each read, and keeps what is written.
    struct Fake {
        keys: VecDeque<Vec<u8>>,
        sizes: VecDeque<WindowSize>,
        size: WindowSize,
        out: Vec<u8>,
    }

    impl Fake {
        fn new(columns: u16, rows: u16, keys: &[&[u8]]) -> Self {
            Self {
                keys: keys.iter().map(|keys| keys.to_vec()).collect(),
                sizes: VecDeque::new(),
                size: window(columns, rows),
                out: Vec::new(),
            }
        }

        /// The frames drawn, as text.
        fn frames(&self) -> Vec<String> {
            let out = String::from_utf8(self.out.clone()).unwrap();
            out.split("\x1b[?2026h")
                .skip(1)
                .map(str::to_owned)
                .collect()
        }
    }

    fn window(columns: u16, rows: u16) -> WindowSize {
        WindowSize {
            columns,
            rows,
            width: 0,
            height: 0,
        }
    }

    impl Write for Fake {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.out.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Terminal for Fake {
        fn size(&self) -> Option<WindowSize> {
            Some(self.size)
        }

        fn read(&mut self, _timeout: Duration) -> io::Result<Vec<u8>> {
            if let Some(size) = self.sizes.pop_front() {
                self.size = size;
            }
            // Out of keys, the reader quits.
            Ok(self.keys.pop_front().unwrap_or_else(|| b"q".to_vec()))
        }
    }

    fn numbers(count: usize) -> Document {
        let source: String = (1..=count).map(|n| format!("Line {n}\n\n")).collect();
        crate::parse::parse(&source)
    }

    fn show(document: &Document, terminal: &mut Fake) {
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        run(document, "notes.md", &mut inputs, None, terminal).unwrap();
    }

    #[test]
    fn draws_the_document_and_quits_on_q() {
        let mut terminal = Fake::new(40, 6, &[b"q"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 1, "{frames:?}");
        assert!(frames[0].contains("Line 1"), "{frames:?}");
        assert!(frames[0].contains("Line 3"), "{frames:?}");
        assert!(!frames[0].contains("Line 4"), "{frames:?}");
    }

    #[test]
    fn redraws_after_keys_that_move() {
        // Two lines down, then a key that does nothing, then the end.
        let mut terminal = Fake::new(40, 6, &[b"jj", b"x", b"", b"G"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 3, "{frames:?}");
        // Line 2 is at the top: the first row of the frame.
        assert!(frames[1].contains("\x1b[1;1H  Line 2"), "{frames:?}");
        assert!(frames[2].contains("Line 10"), "{frames:?}");
    }

    #[test]
    fn a_key_split_over_two_reads_moves() {
        // Page Down as an escape character and the rest of its sequence.
        let mut terminal = Fake::new(40, 6, &[b"\x1b", b"[6~"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 2, "{frames:?}");
        assert!(frames[1].contains("\x1b[2;1H  Line 4"), "{frames:?}");
    }

    #[test]
    fn the_status_line_is_on_the_bottom_row() {
        let mut terminal = Fake::new(40, 6, &[b"G"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 2, "{frames:?}");
        // The document takes the rows above it.
        assert!(frames[0].contains("\x1b[5;1H  Line 3"), "{frames:?}");
        assert!(!frames[0].contains("\x1b[6;1H  "), "{frames:?}");
        let status = |frame: &str| frame[frame.find("\x1b[6;1H").unwrap()..].to_owned();
        assert!(status(&frames[0]).contains(" kiri "), "{frames:?}");
        assert!(status(&frames[0]).contains(" notes.md "), "{frames:?}");
        assert!(status(&frames[0]).contains(" help"), "{frames:?}");
        assert!(status(&frames[1]).contains(" END "), "{frames:?}");
        assert!(status(&frames[1]).contains("100%"), "{frames:?}");
    }

    #[test]
    fn lays_the_document_out_again_for_a_new_window_size() {
        let mut terminal = Fake::new(40, 5, &[b"", b""]);
        terminal.sizes = [window(40, 5), window(8, 5)].into();
        let document = crate::parse::parse("A paragraph of several words\n");
        show(&document, &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 2, "{frames:?}");
        assert!(
            frames[0].contains("A paragraph of several words"),
            "{frames:?}"
        );
        assert!(!frames[1].contains("A paragraph of"), "{frames:?}");
    }

    #[test]
    fn a_heading_taller_than_the_window_is_drawn_at_normal_size() {
        let mut terminal = Fake::new(40, 3, &[b"G"]);
        let document = crate::parse::parse("# Title\n\nText\n");
        let mut inputs = Inputs::new(Capabilities::kitty(80));
        run(&document, "notes.md", &mut inputs, None, &mut terminal).unwrap();
        let frames = terminal.frames();
        assert!(frames[0].contains("Title"), "{frames:?}");
        assert!(!frames[0].contains("\x1b]66;"), "{frames:?}");
        // The last page shows the text below it.
        assert!(frames.last().unwrap().contains("Text"), "{frames:?}");
    }

    #[test]
    fn a_given_width_stays_when_the_window_changes() {
        let mut terminal = Fake::new(40, 5, &[b""]);
        terminal.sizes = [window(80, 5)].into();
        let document = crate::parse::parse("A paragraph of several words\n");
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        run(&document, "notes.md", &mut inputs, Some(10), &mut terminal).unwrap();
        // One frame before the window changed, and one after.
        assert_eq!(terminal.frames().len(), 2);
        for frame in terminal.frames() {
            assert!(!frame.contains("A paragraph of"), "{frame:?}");
        }
    }
}
