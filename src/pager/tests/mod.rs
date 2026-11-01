use std::collections::VecDeque;

use super::*;
use crate::images::{ImageError, ImageKey};
use crate::session::sources::Session;
use crate::session::state::{FRAME, POLL};
use crate::typeset::color::Color;
use crate::typeset::model::{Document, File};

mod commands;
mod files;
mod following;
mod images;
mod layers;
mod links;
mod loading;
mod reading;
mod reload;
mod search;
mod simulated;
mod view;

/// A terminal that gives the reads in `keys`, one a call, with the
/// window sizes in `sizes` after each read, and keeps what is written.
struct Fake {
    keys: VecDeque<Vec<u8>>,
    sizes: VecDeque<WindowSize>,
    size: WindowSize,
    out: Vec<u8>,
    /// The time now, which each read moves on by a poll.
    clock: Instant,
    /// How long each read was asked to wait.
    waits: Vec<Duration>,
    /// How long a read that gives keys takes, when not as long as it
    /// was asked to wait.
    typing: Option<Duration>,
    /// Called with the number of each read, counting from 0, before
    /// it gives its keys.
    hook: Option<Box<dyn FnMut(usize)>>,
    /// Called on each flush.
    flushed: Option<Box<dyn FnMut()>>,
}

impl Fake {
    fn new(columns: u16, rows: u16, keys: &[&[u8]]) -> Self {
        Self {
            keys: keys.iter().map(|keys| keys.to_vec()).collect(),
            sizes: VecDeque::new(),
            size: window(columns, rows),
            out: Vec::new(),
            clock: Instant::now(),
            waits: Vec::new(),
            typing: None,
            hook: None,
            flushed: None,
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
        if let Some(flushed) = &mut self.flushed {
            flushed();
        }
        Ok(())
    }
}

impl Terminal for Fake {
    fn size(&self) -> Option<WindowSize> {
        Some(self.size)
    }

    fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        if let Some(hook) = &mut self.hook {
            hook(self.waits.len());
        }
        self.waits.push(timeout);
        if let Some(size) = self.sizes.pop_front() {
            self.size = size;
        }
        // Out of keys, the reader quits, with Ctrl+C, which also
        // closes a prompt left open.
        let keys = self.keys.pop_front().unwrap_or_else(|| b"\x03".to_vec());
        self.clock += match self.typing {
            Some(typing) if !keys.is_empty() => typing,
            _ => timeout,
        };
        Ok(keys)
    }

    fn now(&self) -> Instant {
        self.clock
    }
}

fn numbers(count: usize) -> Document {
    let source: String = (1..=count).map(|n| format!("Line {n}\n\n")).collect();
    crate::typeset::parse::parse(&source)
}

/// Runs the pager on `document`, named `notes.md`, with `inputs` and
/// `options`, on `terminal`, with loads of its own, which are stopped and
/// waited for once it returns.
fn run_document(
    document: &Document,
    inputs: &mut Inputs,
    options: Options<'_>,
    terminal: &mut Fake,
) {
    let mut loads = Loads::new();
    run_document_in(document, inputs, options, &mut loads, terminal);
    loads.halt();
    loads.wait(crate::images::STOPPING);
}

/// [`run_document`], with the reloads' images loading in `loads`, which are
/// left running for the caller.
fn run_document_in(
    document: &Document,
    inputs: &mut Inputs,
    options: Options<'_>,
    loads: &mut Loads,
    terminal: &mut Fake,
) {
    run(
        Content::Document(document),
        None,
        Session::new(["notes.md"]),
        inputs,
        options,
        loads,
        terminal,
    )
    .unwrap();
}

fn show(document: &Document, terminal: &mut Fake) {
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    run_document(
        document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        terminal,
    );
}

/// The frames drawn for `document` in a window of `columns` × `rows`
/// when the terminal gives `keys`.
fn shown(document: &Document, columns: u16, rows: u16, keys: &[&[u8]]) -> Vec<String> {
    let mut terminal = Fake::new(columns, rows, keys);
    show(document, &mut terminal);
    terminal.frames()
}

/// `document` shown with its lines laid out beforehand from `before`,
/// with `caps`.
fn show_laid_out(document: &Document, before: &Document, caps: Capabilities) -> Vec<String> {
    let mut terminal = Fake::new(40, 6, &[b"q"]);
    let mut inputs = Inputs::new(caps);
    let stream = crate::typeset::layout::content(Content::Document(before), &inputs);
    run(
        Content::Document(document),
        Some(stream),
        Session::new(["notes.md"]),
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut Loads::new(),
        &mut terminal,
    )
    .unwrap();
    terminal.frames()
}

/// What the pager gets when it asks for the document again: the
/// outcomes in `then`, one a call, and how many times it asked; and
/// when it asks whether the document changed, the answers in
/// `changes`, one a call, and unchanged after them.
struct Then {
    then: VecDeque<Reloaded>,
    asked: usize,
    changes: VecDeque<Change>,
}

impl Then {
    fn new(then: impl IntoIterator<Item = Reloaded>) -> Self {
        Self {
            then: then.into_iter().collect(),
            asked: 0,
            changes: VecDeque::new(),
        }
    }

    /// As [`Then::new`], answering `changes` when asked whether the
    /// document changed.
    fn changing(
        then: impl IntoIterator<Item = Reloaded>,
        changes: impl IntoIterator<Item = Change>,
    ) -> Self {
        Self {
            changes: changes.into_iter().collect(),
            ..Self::new(then)
        }
    }
}

impl Reload for Then {
    fn reload(&mut self, _: &mut Images, _: &mut Loads, _: &Capabilities, _: bool) -> Reloaded {
        self.asked += 1;
        self.then.pop_front().expect("an outcome for each reload")
    }

    fn change(&mut self) -> Change {
        self.changes.pop_front().unwrap_or(Change::Unchanged)
    }
}

/// The row of `frame` that shows `text`, counted from 1, of the
/// window's `rows`.
fn row_of(frame: &str, rows: u16, text: &str) -> Option<u16> {
    let frame = unstyled(frame);
    (1..rows).find(|row| {
        let start = format!("\x1b[{row};1H");
        let end = format!("\x1b[{};1H", row + 1);
        frame
            .split(&start)
            .nth(1)
            .and_then(|rest| rest.split(&end).next())
            .is_some_and(|shown| shown.contains(text))
    })
}

/// The SGR codes before `text` in the row of `frame` that shows it.
fn row_style(frame: &str, text: &str) -> Vec<String> {
    style_before(document_part(frame, 21), text)
}

/// The background of the SGR codes `codes`, as `r;g;b`.
fn background(codes: &[String]) -> Option<String> {
    let at = codes
        .windows(2)
        .position(|pair| pair[0] == "48" && pair[1] == "2")?;
    Some(codes[at + 2..at + 5].join(";"))
}

/// As [`show_reloading`], on a terminal with `caps`, with `empty`
/// polls without keys after `reads`.
fn reloading_with(
    caps: Capabilities,
    document: &Document,
    then: &mut Then,
    reads: &[&[u8]],
    empty: usize,
) -> Vec<String> {
    reloading_in(6, caps, document, then, reads, empty).frames()
}

/// As [`reloading_with`], in a window of 60 × `rows`, giving the
/// terminal after the reader quit.
fn reloading_in(
    rows: u16,
    caps: Capabilities,
    document: &Document,
    then: &mut Then,
    reads: &[&[u8]],
    empty: usize,
) -> Fake {
    let mut reads = reads.to_vec();
    reads.extend(std::iter::repeat_n(b"".as_slice(), empty));
    let mut terminal = Fake::new(60, rows, &reads);
    let mut inputs = Inputs::new(caps);
    run_document(
        document,
        &mut inputs,
        Options {
            reload: Some(then),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    terminal
}

fn rows(count: usize) -> Document {
    let source: String = (1..=count).map(|n| format!("Row {n}\n\n")).collect();
    crate::typeset::parse::parse(&source)
}

/// The text on the top row of the document in `frame`.
fn top_row(frame: &str) -> String {
    let frame = unstyled(frame);
    let row = frame.split("\x1b[1;1H").nth(1).unwrap();
    row.split("\x1b[2;1H").next().unwrap().trim().to_owned()
}

/// The status line of `frame`, on row `row`.
fn status_row(frame: &str, row: u16) -> String {
    let at = format!("\x1b[{row};1H");
    frame[frame.find(&at).unwrap()..].to_owned()
}

/// Whether `frame` shows `line` on its top row, in any style.
fn at_top(frame: &str, line: &str) -> bool {
    unstyled(frame).contains(&format!("\x1b[1;1H  {line}\x1b"))
}

/// `frame` without its SGR sequences.
fn unstyled(frame: &str) -> String {
    let mut rest = frame;
    let mut kept = String::new();
    while let Some(at) = rest.find("\x1b[") {
        kept.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let parameters = after
            .find(|c: char| !c.is_ascii_digit() && c != ';' && c != ':')
            .unwrap_or(after.len());
        if after[parameters..].starts_with('m') {
            rest = &after[parameters + 1..];
        } else {
            kept.push_str("\x1b[");
            rest = after;
        }
    }
    kept.push_str(rest);
    kept
}

/// The codes of the last SGR sequence before `text` in `frame`; none
/// when there is none.
fn style_before(frame: &str, text: &str) -> Vec<String> {
    let at = frame
        .find(text)
        .unwrap_or_else(|| panic!("{text:?} in {frame:?}"));
    let Some(start) = frame[..at].rfind("\x1b[0") else {
        return Vec::new();
    };
    let start = start + 2;
    let end = start + frame[start..].find('m').unwrap();
    frame[start..end].split(';').map(str::to_owned).collect()
}

/// `frame` without its status line on row `row`.
fn document_part(frame: &str, row: u16) -> &str {
    &frame[..frame.find(&format!("\x1b[{row};1H")).unwrap()]
}

/// The pixels of the image the tests of images show: a square of
/// 20 pixels in `rgba`.
fn square(rgba: [u8; 4]) -> crate::images::Source {
    crate::images::Source::Raster(image::RgbaImage::from_pixel(20, 20, image::Rgba(rgba)))
}

/// A document that shows the image `square.png`, and inputs for kitty
/// with its pixels in `rgba`.
fn pictured(rgba: [u8; 4]) -> (Document, Inputs) {
    let document = crate::typeset::parse::parse("Above\n\n![a square](square.png)\n\nBelow\n");
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs.images.insert("square.png", Ok(square(rgba)));
    (document, inputs)
}

/// The ids of the images `out` sends, in order.
fn sent_ids(out: &str) -> Vec<u32> {
    ids_after(out, "\x1b_Ga=T,U=1,q=2,f=32,o=z,i=")
}

/// The ids of the bands `out` sends, in order.
fn band_ids(out: &str) -> Vec<u32> {
    ids_after(out, "\x1b_Ga=t,q=2,f=32,o=z,i=")
}

/// The ids of the images `out` deletes, in order.
fn deleted_ids(out: &str) -> Vec<u32> {
    ids_after(out, "\x1b_Ga=d,d=I,i=")
}

fn ids_after(out: &str, prefix: &str) -> Vec<u32> {
    (out.split(prefix).skip(1))
        .map(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().unwrap()
        })
        .collect()
}

/// `out` before and after the end of its last frame, where the pager
/// quits.
fn until_quit(out: &str) -> (&str, &str) {
    let end = out.rfind("\x1b[?2026l").unwrap();
    out.split_at(end)
}

fn sorted(mut ids: Vec<u32>) -> Vec<u32> {
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The terminal colours of the fade tests: grey text on black, with
/// black and yellow in the palette.
fn known_colours() -> Capabilities {
    let mut caps = Capabilities::ansi(80);
    caps.truecolor = true;
    caps.foreground = Some(Color::Rgb(200, 200, 200));
    caps.background = Some(Color::Rgb(0, 0, 0));
    caps.palette[0] = Some(Color::Rgb(0, 0, 0));
    caps.palette[3] = Some(Color::Rgb(200, 200, 0));
    caps
}

/// The frames drawn on a terminal with `caps` for `reads`, then
/// `empty` polls without keys.
fn frames_with(caps: Capabilities, reads: &[&[u8]], empty: usize) -> Vec<String> {
    frames_of(&numbers(30), Inputs::new(caps), reads, empty)
}

/// The frames drawn for `document` with `inputs` for `reads`, then
/// `empty` polls without keys.
fn frames_of(
    document: &Document,
    mut inputs: Inputs,
    reads: &[&[u8]],
    empty: usize,
) -> Vec<String> {
    let mut reads = reads.to_vec();
    reads.extend(std::iter::repeat_n(b"".as_slice(), empty));
    let mut terminal = Fake::new(60, 6, &reads);
    run_document(
        document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    terminal.frames()
}
