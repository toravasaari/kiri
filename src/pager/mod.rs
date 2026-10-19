//! The interactive pager: the document on a screen of its own, scrolled
//! with keys.

pub mod help;
pub mod keys;
pub mod prompt;
pub mod search;
pub mod status;
pub mod view;

use std::io::{self, Write};
use std::time::Duration;

use crate::config::{Paging, SearchCase};
use crate::model::Document;
use crate::render::Inputs;
use crate::render::output::{Page, Writer};
use crate::render::style::{Line, Span, Style};
use crate::term::{Capabilities, Screen, WindowSize};
use help::Placed;
use keys::{Action, Key};
use prompt::{Edit, Prompt};
use search::Search;
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
/// it is given. A search treats case as `case` says.
pub fn run(
    document: &Document,
    name: &str,
    inputs: &mut Inputs,
    width: Option<u16>,
    case: SearchCase,
    terminal: &mut impl Terminal,
) -> io::Result<()> {
    let page = Page {
        background: inputs.theme.background,
        foreground: inputs.theme.foreground,
    };
    let mut size = terminal.size();
    let mut view: Option<View> = None;
    let mut state = State::new(case);
    loop {
        fit(&mut inputs.caps, size, width);
        let window = usize::from(inputs.caps.height.unwrap_or(DEFAULT_ROWS));
        // The bottom row is the status line's, unless the document would
        // have no row left.
        let bar = window > 1;
        let rows = if bar { window - 1 } else { window };
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
        state.lay_out(&lines, shown.top());
        let status = Status {
            name,
            styles: status::Styles::new(&inputs.theme, &inputs.caps),
            width: size.map_or(inputs.caps.width, |size| size.columns),
            bar,
            panel: Style {
                fg: inputs.theme.foreground,
                bg: inputs.theme.background,
                ..Style::default()
            },
            muted: Style {
                // Without a colour of its own, muted text is dimmed.
                dim: inputs.theme.muted.is_none(),
                fg: inputs.theme.muted.or(inputs.theme.foreground),
                bg: inputs.theme.background,
                ..Style::default()
            },
        };
        let mut laid_out = Layout {
            lines: &lines,
            status: &status,
            view: &mut shown,
        };
        match show(&mut laid_out, &mut state, &mut writer, terminal, size)? {
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

/// What the status line and the help panel show and how, apart from the
/// position.
struct Status<'a> {
    name: &'a str,
    styles: status::Styles,
    /// The window's columns.
    width: u16,
    /// Whether the window has a row for the status line besides the
    /// document's.
    bar: bool,
    /// The help panel's style, and that of its notes.
    panel: Style,
    muted: Style,
}

impl Status<'_> {
    /// The status line for `view` in `state`: the prompt while it is open.
    fn spans(&self, view: &View, state: &State) -> Vec<Span> {
        if !self.bar {
            return Vec::new();
        }
        let width = usize::from(self.width);
        match &state.prompt {
            Some(prompt) => status::prompt(prompt.term(), width, &self.styles),
            None => {
                let found = state.found();
                status::spans(self.name, view.position(), found, width, &self.styles)
            }
        }
    }

    /// The help panel over `view`, when `open`.
    fn panel(&self, view: &View, open: bool) -> Vec<Placed> {
        if open {
            help::panel(
                usize::from(self.width),
                view.rows(),
                &self.panel,
                &self.muted,
            )
        } else {
            Vec::new()
        }
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

/// What the reader does in the pager besides scrolling, which stays when
/// the window changes size.
#[derive(Debug)]
struct State {
    /// How a search treats case.
    case: SearchCase,
    /// Whether the help is open.
    help: bool,
    /// The search prompt, while it is open.
    prompt: Option<Prompt>,
    /// The last search.
    search: Option<Search>,
    /// Whether the last search is shown; Escape hides it until `n` or
    /// `N`.
    shown: bool,
    /// Whether the status line tells that the last search found nothing,
    /// which it does until the next key.
    nowhere: bool,
}

impl State {
    /// Nothing done yet, with searches that treat case as `case` says.
    fn new(case: SearchCase) -> Self {
        Self {
            case,
            help: false,
            prompt: None,
            search: None,
            shown: false,
            nowhere: false,
        }
    }

    /// Does what `key` asks in `layout`; true when it quits.
    fn press(&mut self, key: Key, layout: &mut Layout) -> bool {
        self.nowhere = false;
        // Any key closes the help and does nothing else, but Ctrl+C
        // still quits.
        if self.help && key != Key::Ctrl('c') {
            self.help = false;
            return false;
        }
        // While the prompt is open, keys edit it.
        if let Some(prompt) = &mut self.prompt {
            match prompt.edit(key) {
                Edit::Typing => {}
                Edit::Cancel => self.prompt = None,
                Edit::Search(term) => {
                    self.prompt = None;
                    self.find(&term, layout);
                }
            }
            return false;
        }
        match keys::action(key) {
            Some(Action::Quit) => return true,
            Some(Action::Move(motion)) => layout.view.scroll(motion),
            // The help and the prompt need a row besides the status line's.
            Some(Action::Help) => self.help = layout.status.bar,
            Some(Action::Search) => {
                self.prompt = layout.status.bar.then(Prompt::default);
            }
            Some(Action::Next) => self.step(Search::forward, layout),
            Some(Action::Previous) => self.step(Search::backward, layout),
            Some(Action::Clear) => self.shown = false,
            None => {}
        }
        false
    }

    /// Searches for `term` from the top of the view, and shows the first
    /// match.
    fn find(&mut self, term: &str, layout: &mut Layout) {
        let search = Search::new(layout.lines, term, self.case, layout.view.top());
        match search.current() {
            Some(found) => layout.view.reveal(found.line),
            None => self.nowhere = true,
        }
        self.search = Some(search);
        self.shown = true;
    }

    /// Finds the last search again in `lines`, laid out anew, from the
    /// line `top` of the view.
    fn lay_out(&mut self, lines: &[Line], top: usize) {
        if let Some(search) = &mut self.search {
            *search = Search::new(lines, search.term(), self.case, top);
        }
    }

    /// Moves the last search to another match by `step`, and shows it.
    fn step(&mut self, step: fn(&mut Search) -> bool, layout: &mut Layout) {
        let Some(search) = &mut self.search else {
            return;
        };
        step(search);
        match search.current() {
            Some(found) => layout.view.reveal(found.line),
            None => self.nowhere = true,
        }
        self.shown = true;
    }

    /// What the status line tells about the last search.
    fn found(&self) -> Option<status::Found<'_>> {
        let search = self.search.as_ref()?;
        if self.nowhere {
            return Some(status::Found::Nowhere(search.term()));
        }
        if !self.shown {
            return None;
        }
        let (current, total) = search.count()?;
        Some(status::Found::At {
            term: search.term(),
            current,
            total,
        })
    }
}

/// One layout of the document: its lines, the view of them and the status
/// line below it.
struct Layout<'a> {
    lines: &'a [Line],
    status: &'a Status<'a>,
    view: &'a mut View,
}

impl Layout<'_> {
    /// What the screen shows in `state`: the top line, the status line and
    /// the help panel.
    fn frame(&self, state: &State) -> (usize, Vec<Span>, Vec<Placed>) {
        (
            self.view.top(),
            self.status.spans(self.view, state),
            self.status.panel(self.view, state.help),
        )
    }
}

/// Shows `layout` in `state` on `terminal`, whose window is of `size`, and
/// scrolls it until the reader quits or the window changes size.
fn show(
    layout: &mut Layout,
    state: &mut State,
    writer: &mut Writer,
    terminal: &mut impl Terminal,
    size: Option<WindowSize>,
) -> io::Result<Outcome> {
    let mut shown = layout.frame(state);
    draw(
        layout.lines,
        layout.view,
        &shown.1,
        &shown.2,
        writer,
        terminal,
    )?;
    terminal.flush()?;
    let mut decoder = keys::Decoder::default();
    loop {
        let input = terminal.read(POLL)?;
        // Nothing more came for a key the last read cut off.
        let keys = if input.is_empty() {
            decoder.flush()
        } else {
            decoder.keys(&input)
        };
        for key in keys {
            if state.press(key, layout) {
                return Ok(Outcome::Quit);
            }
        }
        let now = terminal.size();
        if now != size {
            return Ok(Outcome::Resized(now));
        }
        let frame = layout.frame(state);
        if frame != shown {
            shown = frame;
            draw(
                layout.lines,
                layout.view,
                &shown.1,
                &shown.2,
                writer,
                terminal,
            )?;
            terminal.flush()?;
        }
    }
}

/// Draws the lines `view` shows from the top of the screen, with the rows
/// below them empty, `status` on the row below the view and `panel` over
/// the lines, which it dims, as one update so the screen does not flicker.
pub fn draw(
    lines: &[Line],
    view: &View,
    status: &[Span],
    panel: &[Placed],
    writer: &mut Writer,
    out: &mut impl Write,
) -> io::Result<()> {
    out.write_all(BEGIN_UPDATE)?;
    writer.clear(out)?;
    writer.dim(!panel.is_empty());
    let mut top = 1;
    for line in &lines[view.visible()] {
        for row in 0..writer.rows(line) {
            write!(out, "\x1b[{};1H", top + usize::from(row))?;
            writer.row(line, row, out)?;
        }
        top += usize::from(writer.rows(line));
    }
    writer.dim(false);
    if !status.is_empty() {
        write!(out, "\x1b[{};1H", view.rows() + 1)?;
        writer.bar(status, out)?;
    }
    for placed in panel {
        write!(out, "\x1b[{};{}H", placed.row, placed.column)?;
        writer.bar(&placed.spans, out)?;
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
            // Out of keys, the reader quits, with Ctrl+C, which also
            // closes a prompt left open.
            Ok(self.keys.pop_front().unwrap_or_else(|| b"\x03".to_vec()))
        }
    }

    fn numbers(count: usize) -> Document {
        let source: String = (1..=count).map(|n| format!("Line {n}\n\n")).collect();
        crate::parse::parse(&source)
    }

    fn show(document: &Document, terminal: &mut Fake) {
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        run(
            document,
            "notes.md",
            &mut inputs,
            None,
            SearchCase::Smart,
            terminal,
        )
        .unwrap();
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
        assert!(status(&frames[1]).contains(" kiri "), "{frames:?}");
        assert!(status(&frames[1]).contains("100%"), "{frames:?}");
    }

    #[test]
    fn the_help_opens_over_the_dimmed_document_and_any_key_closes_it() {
        // The help, a key that closes it without moving, the help again,
        // and q, which closes it without quitting, then the end.
        let mut terminal = Fake::new(60, 24, &[b"?", b"j", b"h", b"q", b"G"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 6, "{frames:#?}");
        let help = |frame: &str| frame.contains("down a line");
        assert!(!help(&frames[0]), "{frames:#?}");
        assert!(help(&frames[1]), "{frames:#?}");
        assert!(frames[1].contains("[0;2m  Line 1"), "{frames:#?}");
        assert!(!help(&frames[2]), "{frames:#?}");
        assert!(!frames[2].contains("[0;2m"), "{frames:#?}");
        assert_eq!(frames[0], frames[2]);
        assert!(help(&frames[3]), "{frames:#?}");
        assert_eq!(frames[0], frames[4]);
        assert!(frames[5].contains("100%"), "{frames:#?}");
    }

    #[test]
    fn keys_without_a_binding_close_the_help_too() {
        // F5 closes the help, then j moves.
        let mut terminal = Fake::new(60, 24, &[b"?", b"\x1b[15~", b"j"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert!(frames[1].contains("down a line"), "{frames:#?}");
        assert_eq!(frames[2], frames[0]);
        assert_ne!(frames[3], frames[0]);
    }

    #[test]
    fn ctrl_c_quits_even_from_the_help() {
        let mut terminal = Fake::new(60, 24, &[b"?", b"", b"G"]);
        show(&numbers(30), &mut terminal);
        assert_eq!(terminal.frames().len(), 2);
    }

    #[test]
    fn a_window_of_one_row_shows_the_document_without_a_status_line() {
        // The help has no room, so q quits at once.
        let mut terminal = Fake::new(40, 1, &[b"?", b"q", b"G"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 1, "{frames:#?}");
        assert!(frames[0].contains("Line 1"), "{frames:#?}");
        assert!(!frames[0].contains(" kiri "), "{frames:#?}");
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
        run(
            &document,
            "notes.md",
            &mut inputs,
            None,
            SearchCase::Smart,
            &mut terminal,
        )
        .unwrap();
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
        run(
            &document,
            "notes.md",
            &mut inputs,
            Some(10),
            SearchCase::Smart,
            &mut terminal,
        )
        .unwrap();
        // One frame before the window changed, and one after.
        assert_eq!(terminal.frames().len(), 2);
        for frame in terminal.frames() {
            assert!(!frame.contains("A paragraph of"), "{frame:?}");
        }
    }

    /// The status line of `frame`, on row `row`.
    fn status_row(frame: &str, row: u16) -> String {
        let at = format!("\x1b[{row};1H");
        frame[frame.find(&at).unwrap()..].to_owned()
    }

    /// Whether `frame` shows `line` on its top row.
    fn at_top(frame: &str, line: &str) -> bool {
        frame.contains(&format!("\x1b[1;1H  {line}\x1b"))
    }

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
        // Around the start to the last match, on the last page.
        assert!(frames[4].contains("Line 29"), "{frames:#?}");
        assert!(status_row(&frames[4], 6).contains("11/11"), "{frames:#?}");
        assert!(shows(&frames[5], "Line 2", "/line 2 1/11"), "{frames:#?}");
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
}
