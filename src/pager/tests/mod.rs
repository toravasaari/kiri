use std::collections::VecDeque;
use std::path::Path;

use super::*;
use crate::images::{ImageError, ImageKey, Images};
use crate::present::scene::Scene;
use crate::present::seen::Seen;
use crate::session::sources::{Session, SourceFile};
use crate::session::state::{Change, FRAME, Look, POLL};
use crate::typeset::color::Color;
use crate::typeset::input::Input;
use crate::typeset::model::{Document, File};

mod animations;
mod commands;
mod files;
mod following;
mod images;
mod layers;
mod links;
mod loading;
mod placement;
mod reading;
mod reload;
mod search;
mod simulated;
mod view;

thread_local! {
    /// Whether the test running draws its images as sixels.
    static SIXEL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the test running is on the sixel terminal, by [`on_both`].
fn on_sixel() -> bool {
    SIXEL.with(std::cell::Cell::get)
}

/// Runs a screen-level `test` on each terminal in turn, kitty and then
/// sixel: what is on the screen is the same on both, whatever bytes each
/// writes. A test that fails says which terminal it was on.
fn on_both(test: impl Fn()) {
    struct Terminal(bool);
    impl Drop for Terminal {
        fn drop(&mut self) {
            if std::thread::panicking() {
                eprintln!(
                    "(the test failed on {})",
                    ["kitty", "sixel"][usize::from(self.0)]
                );
            }
            SIXEL.with(|sixel| sixel.set(false));
        }
    }
    for sixel in [false, true] {
        SIXEL.with(|on| on.set(sixel));
        let _on = Terminal(sixel);
        test();
    }
}

/// The capabilities of the terminal a screen-level test is on: kitty's, or
/// sixel's with hyperlinks and a background it has told, which a sixel is
/// flattened onto, in a window `width` columns wide.
fn screen_caps(width: u16) -> Capabilities {
    match on_sixel() {
        true => Capabilities {
            hyperlinks: true,
            background: Some(Color::Rgb(16, 16, 16)),
            ..Capabilities::sixel(width)
        },
        false => Capabilities::kitty(width),
    }
}

/// What a frame draws of images, a piece for each: the id of each image
/// kitty is sent, or each sixel written, for the terminal [`on_both`] is on.
fn image_data(frame: &str) -> Vec<String> {
    match on_sixel() {
        true => (frame.split("\x1bP").skip(1))
            .map(|rest| rest.split("\x1b\\").next().unwrap_or("").to_owned())
            .collect(),
        false => sent_ids(frame).iter().map(u32::to_string).collect(),
    }
}

/// How the file behind `notes.md` looked when it was read, for a pager
/// that can read it again.
const READ: Look = Look {
    modified: None,
    length: 0,
};

/// The session of `notes.md`, read from a file when `options` has a way to
/// read it again.
fn session_for(options: &Options) -> Session {
    let mut session = Session::new(["notes.md"]);
    if options.files.is_some() {
        let (path, input, look) = ("notes.md".into(), Input::Markdown, Some(READ));
        session.read_from(session.sources()[0].id(), SourceFile { path, input, look });
    }
    session
}

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
    /// What had been written and what the time was at each read.
    marks: Vec<(usize, Instant)>,
    /// How long a read that gives keys takes, when not as long as it
    /// was asked to wait.
    typing: Option<Duration>,
    /// Called with the number of each read, counting from 0, before
    /// it gives its keys.
    hook: Option<Box<dyn FnMut(usize)>>,
    /// Called on each flush.
    flushed: Option<Box<dyn FnMut()>>,
    /// What each frame showed, as the scene meant it.
    seen: Vec<Seen>,
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
            marks: Vec::new(),
            typing: None,
            hook: None,
            flushed: None,
            seen: Vec::new(),
        }
    }

    /// The screen each frame showed, as the scene meant it: text, with an
    /// image as a marker, and the styles of the cells. One for each of
    /// [`Fake::frames`].
    fn seen(&self) -> &[Seen] {
        assert_eq!(self.seen.len(), self.frames().len(), "a screen a frame");
        &self.seen
    }

    /// The frames drawn, as text.
    fn frames(&self) -> Vec<String> {
        let out = String::from_utf8(self.out.clone()).unwrap();
        out.split("\x1b[?2026h")
            .skip(1)
            .map(str::to_owned)
            .collect()
    }

    /// The synchronized update of each frame, without what was written
    /// after it.
    fn updates(&self) -> Vec<String> {
        (self.frames().iter())
            .map(|frame| frame.split("\x1b[?2026l").next().unwrap_or("").to_owned())
            .collect()
    }

    /// The screen after each frame, a string for each row: the bytes
    /// written to the row since it was last erased, without cursor moves.
    fn screens(&self) -> Vec<Vec<String>> {
        replay(&self.frames())
    }
}

/// The screen after each of `frames`, as [`Fake::screens`] gives them.
fn replay(frames: &[String]) -> Vec<Vec<String>> {
    let mut rows: Vec<String> = Vec::new();
    let mut screens = Vec::new();
    for frame in frames {
        let mut cursor: Option<usize> = None;
        let mut rest = frame.as_str();
        while !rest.is_empty() {
            let Some(after) = rest.strip_prefix("\x1b[") else {
                let mut chars = rest.chars();
                let c = chars.next().unwrap();
                if let Some(row) = cursor {
                    rows[row].push(c);
                }
                rest = chars.as_str();
                continue;
            };
            let end = after
                .find(|c: char| !(c.is_ascii_digit() || ";:?".contains(c)))
                .unwrap_or(after.len());
            let (parameters, tail) = after.split_at(end);
            let Some(last) = tail.chars().next() else {
                break;
            };
            let sequence = &rest[..2 + end + last.len_utf8()];
            rest = &tail[last.len_utf8()..];
            match (parameters, last) {
                ("2", 'J') => rows.iter_mut().for_each(String::clear),
                ("2", 'K') => {
                    if let Some(row) = cursor {
                        rows[row].clear();
                    }
                }
                ("?2026", 'l') => {}
                (_, 'H') => {
                    let row = parameters.split(';').next().unwrap_or("");
                    let row: usize = row.parse().unwrap_or(1);
                    if rows.len() < row {
                        rows.resize(row, String::new());
                    }
                    cursor = Some(row - 1);
                }
                _ => {
                    if let Some(row) = cursor {
                        rows[row].push_str(sequence);
                    }
                }
            }
        }
        screens.push(rows.clone());
    }
    screens
}

/// `screen`'s rows one under the other.
fn screen_text(screen: &[String]) -> String {
    screen.join("\n")
}

/// The text on the top row of `screen`, without styles, trimmed.
fn top_row_of(screen: &[String]) -> String {
    unstyled(&screen[0]).trim().to_owned()
}

/// Whether `screen` shows `line` on its top row, in any style.
fn at_top_of(screen: &[String], line: &str) -> bool {
    let top = unstyled(&screen[0]);
    top.strip_prefix(&format!("  {line}"))
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('\x1b'))
}

/// Row `row`, from 1, of `screen`.
fn screen_row(screen: &[String], row: u16) -> &str {
    &screen[usize::from(row) - 1]
}

/// The rows of `screen` above row `row`, joined.
fn screen_above(screen: &[String], row: u16) -> String {
    screen_text(&screen[..screen.len().min(usize::from(row) - 1)])
}

/// The row of `screen` that shows `text`, counted from 1, of the
/// window's `rows`.
fn screen_row_of(screen: &[String], rows: u16, text: &str) -> Option<u16> {
    (1..rows).find(|&row| {
        screen
            .get(usize::from(row) - 1)
            .is_some_and(|shown| unstyled(shown).contains(text))
    })
}

/// The SGR codes before `text` in the row of `screen` that shows it.
fn screen_row_style(screen: &[String], text: &str) -> Vec<String> {
    style_before(&screen_above(screen, 21), text)
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
        self.marks.push((self.out.len(), self.clock));
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

    fn drew(&mut self, scene: &Scene<'_>, rows: &dyn Fn(&Line) -> u8) {
        self.seen.push(Seen::of(scene, rows));
    }
}

fn numbers(count: usize) -> Document {
    let source: String = (1..=count).map(|n| format!("Line {n}\n\n")).collect();
    crate::typeset::parse::parse(&source)
}

/// The settings of a run with nothing chosen.
fn plain_settings() -> crate::config::Settings {
    let choices = crate::config::choose(
        &Default::default(),
        &crate::config::Environment::default(),
        &crate::config::Config::default(),
    );
    choices.with_theme(crate::typeset::theme::Theme::default())
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
    let mut loads = Loads::inline_builds();
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
        session_for(&options),
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
/// when it looks at the file, what `changes` says, one a call, and
/// unchanged after them: a look as the file had when it was last
/// read, another look, or a file that is gone.
struct Then {
    then: VecDeque<Reloaded>,
    asked: usize,
    changes: VecDeque<Change>,
    /// How the file looks, as `Unchanged` looks.
    read: Look,
    /// The look that was last given, which the next reload gives as how the
    /// file looked when it was read.
    looked: Look,
    /// The jobs it was given to read again, one a reload.
    jobs: Vec<Reread>,
}

impl Then {
    fn new(then: impl IntoIterator<Item = Reloaded>) -> Self {
        Self {
            then: then.into_iter().collect(),
            asked: 0,
            changes: VecDeque::new(),
            read: READ,
            looked: READ,
            jobs: Vec::new(),
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

impl SourceFiles for Then {
    fn reread(
        &mut self,
        job: &Reread,
        _: &mut Images,
        _: &mut Loads,
        _: &Capabilities,
    ) -> (Option<Look>, Reloaded) {
        self.asked += 1;
        self.jobs.push(job.clone());
        let outcome = self.then.pop_front().expect("an outcome for each reload");
        self.read = self.looked;
        (Some(self.read), outcome)
    }

    fn look(&mut self, _: &Path) -> Result<Look, String> {
        let look = match self.changes.pop_front().unwrap_or(Change::Unchanged) {
            Change::Unchanged => Ok(self.read),
            Change::Changed(look) => Ok(look),
            Change::Gone(why) => Err(why),
        };
        self.looked = look.clone().unwrap_or(self.looked);
        look
    }
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
            files: Some(then),
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

#[test]
fn screens_keep_the_rows_a_frame_does_not_write() {
    let mut fake = Fake::new(10, 3, &[]);
    fake.out = concat!(
        "\x1b[?2026h\x1b[0m\x1b[H\x1b[2J\x1b[1;1Ha\x1b[2;1Hb\x1b[?2026l",
        "\x1b[?2026h\x1b_Ga=d,d=I,i=7,q=2\x1b\\\x1b[2;1H\x1b[2K\x1b[2;1Hc\x1b[2;4Hd\x1b[?2026l",
    )
    .as_bytes()
    .to_vec();
    assert_eq!(fake.screens(), [vec!["a", "b"], vec!["a", "cd"]]);
}
