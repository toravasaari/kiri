//! A simulated front end for the pager's tests: the terminal, the clock, the
//! files and the loads that the loop in `src/pager/mod.rs` reaches, all in
//! memory, so that a test runs the pager as kiri runs it without a
//! pseudo-terminal and without waiting on the wall clock.
//!
//! A [`Sim`] is a script. Each step of it happens at a time since the
//! start on a virtual clock: keys typed, a file written, moved or
//! removed, the window resized. The pager waits for keys as it does on a
//! terminal, for as long as [`Pager::wait`](crate::session::step) says,
//! and the clock moves on by that, or to the next step, whichever is
//! first; nothing sleeps. When the script has nothing more, the reader
//! quits, as with Ctrl+C. What the pager wrote comes back as bytes, the
//! same that go to a terminal, in [`Ran`].
//!
//! The files are in a map in memory and look the way a file's `stat`
//! does, by when it was last written and its length, so that following
//! one sees a change as it does on a disk. Images that a reload brings
//! come as the script says, as a background load would.
//!
//! This is the front end of the tests and, like `main.rs`, belongs to the
//! platform. What it does not do is what the terminal does: raw mode, the
//! alternate screen, signals and the probe. Those stay on a
//! pseudo-terminal, in `tests/it/cli_tmux.rs` and the others.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use crate::images::{Arrival, ImageError, ImageKey, Images, Loads, Placeholder, Source};
use crate::pager::{InlineLoader, Options, Reload, Reloaded, Terminal, run};
use crate::render::Inputs;
use crate::session::settings::{Follow, FollowJump, SearchCase};
use crate::session::sources::Session;
use crate::session::state::{Change, Look};
use crate::term::WindowSize;
use crate::typeset::caps::Capabilities;
use crate::typeset::layout::Content;
use crate::typeset::model::Document;
use crate::typeset::reading::Fonts;
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page::Wide;
use crate::typeset::theme::Theme;

/// The theme the simulated pager draws with: the one the snapshots of
/// features use, `tests/fixtures/test-theme.toml`, which stays as it is
/// when the built-in theme changes. Like a theme loaded from a file, as
/// the snapshots load it, it is not the built-in one (`built_in` is
/// false), so the greys are not lightened for the terminal's colours.
pub(crate) fn fixed_theme() -> Theme {
    let mut theme =
        crate::typeset::theme::parse(include_str!("../../tests/fixtures/test-theme.toml"))
            .expect("the test theme parses");
    theme.built_in = false;
    theme
}

/// What a file looks like to `stat`, as the virtual clock saw it.
#[derive(Clone)]
struct Stored {
    text: String,
    /// When it was last written, since the start.
    written: Duration,
}

/// The files of a simulated disk, by path.
#[derive(Default)]
struct Disk {
    files: BTreeMap<String, Stored>,
}

/// The error text of a file that is not there, as the operating system
/// words it.
const NO_SUCH_FILE: &str = "No such file or directory (os error 2)";

impl Disk {
    fn look(&self, path: &str) -> Result<Look, String> {
        let file = self
            .files
            .get(path)
            .ok_or_else(|| NO_SUCH_FILE.to_owned())?;
        Ok(Look {
            modified: Some(SystemTime::UNIX_EPOCH + file.written),
            length: file.text.len() as u64,
        })
    }

    fn read(&self, path: &str) -> Result<String, String> {
        let file = self
            .files
            .get(path)
            .ok_or_else(|| NO_SUCH_FILE.to_owned())?;
        Ok(file.text.clone())
    }
}

/// What happens at a time of the script.
enum Act {
    /// These bytes are typed.
    Keys(Vec<u8>),
    /// A file is written, which makes it newer.
    Write(String, String),
    /// A file is removed.
    Remove(String),
    /// A file is moved over another, as an editor saves.
    Rename(String, String),
    /// The window is resized.
    Resize(u16, u16),
}

/// What the script and the virtual world share with the reload and the
/// terminal: the disk, the clock and the images on their way.
struct World {
    start: Instant,
    elapsed: Cell<Duration>,
    disk: RefCell<Disk>,
    /// The images a reload started loading: when each comes, and as what.
    coming: RefCell<Vec<Coming>>,
}

struct Coming {
    at: Duration,
    arrival: Arrival,
    image: Result<Source, ImageError>,
}

/// A script for the pager: the document, the terminal it runs on and what
/// happens while it does.
pub(crate) struct Sim {
    columns: u16,
    rows: u16,
    caps: Capabilities,
    follow: Follow,
    jump: FollowJump,
    /// The file the pager shows, if it is a file.
    shown: Option<String>,
    disk: Disk,
    script: Vec<(Duration, Act)>,
    brings: Vec<(String, Duration, Result<Source, ImageError>)>,
    /// Whether `M` has a reading mode to turn on.
    reading: bool,
    /// The images the start of loading what reading mode shows brings.
    inline: Vec<(String, Duration, Source)>,
}

impl Sim {
    /// A script in a window of `columns` × `rows` cells, on a terminal
    /// that takes ANSI styling.
    pub(crate) fn new(columns: u16, rows: u16) -> Self {
        Self {
            columns,
            rows,
            caps: Capabilities::ansi(columns),
            follow: Follow::Off,
            jump: FollowJump::First,
            shown: None,
            disk: Disk::default(),
            script: Vec::new(),
            brings: Vec::new(),
            reading: false,
            inline: Vec::new(),
        }
    }

    /// On a terminal with `caps`.
    pub(crate) fn caps(mut self, caps: Capabilities) -> Self {
        self.caps = caps;
        self
    }

    /// With the file `path` holding `text` from the start.
    pub(crate) fn file(mut self, path: &str, text: &str) -> Self {
        let stored = Stored {
            text: text.to_owned(),
            written: Duration::ZERO,
        };
        self.disk.files.insert(path.to_owned(), stored);
        self
    }

    /// Shows the file `path`, which can be read again.
    pub(crate) fn show(mut self, path: &str) -> Self {
        self.shown = Some(path.to_owned());
        self
    }

    /// Follows the file as `follow` says, jumping to the change `jump`
    /// chooses.
    pub(crate) fn follow(mut self, follow: Follow, jump: FollowJump) -> Self {
        self.follow = follow;
        self.jump = jump;
        self
    }

    /// Has a reload start loading the image `key`, which comes `after`
    /// the reload as `image`.
    pub(crate) fn reload_brings(
        mut self,
        key: &str,
        after: Duration,
        image: Result<Source, ImageError>,
    ) -> Self {
        self.brings.push((key.to_owned(), after, image));
        self
    }

    /// Gives `M` a reading mode to turn on, laid out with the test
    /// fonts.
    pub(crate) fn reading(mut self) -> Self {
        self.reading = true;
        self
    }

    /// Has turning reading mode on start loading the image `key`, which
    /// comes `after` as `image`, as the pictures in the middle of text
    /// do.
    pub(crate) fn inline_brings(mut self, key: &str, after: Duration, image: Source) -> Self {
        self.inline.push((key.to_owned(), after, image));
        self
    }

    fn at(mut self, ms: u64, act: Act) -> Self {
        self.script.push((Duration::from_millis(ms), act));
        self
    }

    /// Types `keys` `ms` milliseconds after the start.
    pub(crate) fn keys(self, ms: u64, keys: &str) -> Self {
        self.at(ms, Act::Keys(keys.as_bytes().to_vec()))
    }

    /// Writes `text` to the file `path`.
    pub(crate) fn write(self, ms: u64, path: &str, text: &str) -> Self {
        self.at(ms, Act::Write(path.to_owned(), text.to_owned()))
    }

    /// Removes the file `path`.
    pub(crate) fn remove(self, ms: u64, path: &str) -> Self {
        self.at(ms, Act::Remove(path.to_owned()))
    }

    /// Moves the file `from` over `to`.
    pub(crate) fn rename(self, ms: u64, from: &str, to: &str) -> Self {
        self.at(ms, Act::Rename(from.to_owned(), to.to_owned()))
    }

    /// Resizes the window.
    pub(crate) fn resize(self, ms: u64, columns: u16, rows: u16) -> Self {
        self.at(ms, Act::Resize(columns, rows))
    }

    /// Runs the pager through the script, and gives what it did.
    pub(crate) fn run(self) -> Ran {
        let Sim {
            columns,
            rows,
            caps,
            follow,
            jump,
            shown,
            disk,
            mut script,
            brings,
            reading,
            inline,
        } = self;
        script.sort_by_key(|(at, _)| *at);
        let world = Rc::new(World {
            start: Instant::now(),
            elapsed: Cell::new(Duration::ZERO),
            disk: RefCell::new(disk),
            coming: RefCell::new(Vec::new()),
        });
        let mut inputs = Inputs {
            theme: fixed_theme(),
            ..Inputs::new(caps)
        };
        let document = match &shown {
            Some(path) => parse(&world.disk.borrow().read(path).expect("the file shown")),
            None => parse(""),
        };
        let mut reload = shown.as_ref().map(|path| SimReload {
            world: Rc::clone(&world),
            path: path.clone(),
            read: world.disk.borrow().look(path).ok(),
            brings,
            asked: 0,
        });
        let mut terminal = SimTerminal {
            world: Rc::clone(&world),
            script: script.into(),
            size: WindowSize {
                columns,
                rows,
                width: 0,
                height: 0,
            },
            out: Vec::new(),
            waits: Vec::new(),
        };
        let mut inline = Some(inline);
        let options = Options {
            reload: reload.as_mut().map(|reload| reload as &mut dyn Reload),
            follow,
            jump,
            reading: reading.then(|| Reading {
                pictures: Default::default(),
                fonts: Rc::new(Fonts::fixture()),
                wide: Wide::Wrap,
            }),
            inline: reading.then(|| -> InlineLoader {
                let world = Rc::clone(&world);
                Box::new(move |_, images, loads, _| {
                    // What a load of the pictures in text starts, once.
                    for (key, after, image) in inline.take().unwrap_or_default() {
                        let key = ImageKey::from(key.as_str());
                        images.wait(key.clone(), Placeholder::Unsized);
                        let arrival = loads.expect(key);
                        let at = world.elapsed.get() + after;
                        let image = Ok(image);
                        world
                            .coming
                            .borrow_mut()
                            .push(Coming { at, arrival, image });
                    }
                })
            }),
            ..Options::new(SearchCase::Smart)
        };
        let session = Session::new([shown.as_deref().unwrap_or("notes.md")]);
        run(
            Content::Document(&document),
            None,
            session,
            &mut inputs,
            options,
            &mut terminal,
        )
        .expect("the simulated terminal does not fail");
        Ran {
            out: String::from_utf8(terminal.out).expect("the pager writes UTF-8"),
            waits: terminal.waits,
            elapsed: world.elapsed.get(),
            reloads: reload.map_or(0, |reload| reload.asked),
        }
    }
}

fn parse(text: &str) -> Document {
    crate::typeset::parse::parse(text)
}

/// What a run of the pager did.
pub(crate) struct Ran {
    /// Everything it wrote to the terminal.
    pub(crate) out: String,
    /// How long each read of keys was asked to wait.
    pub(crate) waits: Vec<Duration>,
    /// The time on the virtual clock when the pager quit.
    pub(crate) elapsed: Duration,
    /// How many times the pager read the document again.
    pub(crate) reloads: usize,
}

impl Ran {
    /// The frames drawn, each from the start of its synchronized update to
    /// the next one, as text with its escape sequences.
    pub(crate) fn frames(&self) -> Vec<&str> {
        self.out.split("\x1b[?2026h").skip(1).collect()
    }

    /// The frames drawn, without their escape sequences. Cursor moves
    /// are gone with them, so the rows run together: use it to tell
    /// whether something is shown, not where.
    pub(crate) fn texts(&self) -> Vec<String> {
        self.frames().into_iter().map(plain).collect()
    }
}

/// `text` without its escape sequences: a CSI sequence ends at its final
/// byte, and an OSC, DCS, APC, SOS or PM string at BEL or the string
/// terminator, with everything in it.
fn plain(text: &str) -> String {
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            plain.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            Some(']' | 'P' | '_' | 'X' | '^') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || c == '\x1b' && chars.next_if_eq(&'\\').is_some() {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    plain
}

/// Where the pager gets the shown file again from: the simulated disk.
struct SimReload {
    world: Rc<World>,
    path: String,
    /// How the file looked when it was last read.
    read: Option<Look>,
    /// The images a reload starts loading.
    brings: Vec<(String, Duration, Result<Source, ImageError>)>,
    asked: usize,
}

impl Reload for SimReload {
    fn reload(
        &mut self,
        _images: &mut Images,
        loads: &mut Loads,
        _caps: &Capabilities,
        _reading: bool,
    ) -> Reloaded {
        self.asked += 1;
        // Before the file is read, so that a change meanwhile counts.
        self.read = self.world.disk.borrow().look(&self.path).ok();
        let text = match self.world.disk.borrow().read(&self.path) {
            Ok(text) => text,
            Err(why) => return Reloaded::Failed(why),
        };
        let now = self.world.elapsed.get();
        for (key, after, image) in self.brings.drain(..) {
            let arrival = loads.expect(ImageKey::from(key.as_str()));
            let coming = Coming {
                at: now + after,
                arrival,
                image,
            };
            self.world.coming.borrow_mut().push(coming);
        }
        Reloaded::Document(parse(&text))
    }

    fn change(&mut self) -> Change {
        match self.world.disk.borrow().look(&self.path) {
            Ok(look) if Some(look) == self.read => Change::Unchanged,
            Ok(look) => Change::Changed(look),
            Err(why) => Change::Gone(why),
        }
    }
}

/// The terminal: the keys and edits of the script at their times, on the
/// virtual clock, and what the pager writes kept.
struct SimTerminal {
    world: Rc<World>,
    script: std::collections::VecDeque<(Duration, Act)>,
    size: WindowSize,
    out: Vec<u8>,
    waits: Vec<Duration>,
}

impl SimTerminal {
    /// Does what the script says is due at `at`, and gives the keys it
    /// types.
    fn happen(&mut self, act: Act) -> Vec<u8> {
        let elapsed = self.world.elapsed.get();
        let mut disk = self.world.disk.borrow_mut();
        match act {
            Act::Keys(keys) => return keys,
            Act::Write(path, text) => {
                let stored = Stored {
                    text,
                    written: elapsed,
                };
                disk.files.insert(path, stored);
            }
            Act::Remove(path) => {
                let removed = disk.files.remove(&path);
                assert!(
                    removed.is_some(),
                    "the script removes {path}, which is not there"
                );
            }
            Act::Rename(from, to) => {
                let mut file = (disk.files.remove(&from))
                    .unwrap_or_else(|| panic!("the script moves {from}, which is not there"));
                file.written = elapsed;
                disk.files.insert(to, file);
            }
            Act::Resize(columns, rows) => {
                self.size = WindowSize {
                    columns,
                    rows,
                    ..self.size
                };
            }
        }
        Vec::new()
    }
}

impl Write for SimTerminal {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Terminal for SimTerminal {
    fn size(&self) -> Option<WindowSize> {
        Some(self.size)
    }

    /// Lets the clock run for `timeout` or until keys come, with the
    /// script's edits and the images' arrivals happening as it passes
    /// their times. Out of script, the reader quits.
    fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        self.waits.push(timeout);
        let end = self.world.elapsed.get() + timeout;
        loop {
            let next_act = self.script.front().map(|(at, _)| *at);
            let next_image = (self.world.coming.borrow().iter())
                .map(|coming| coming.at)
                .min();
            let Some(at) = [next_act, next_image].into_iter().flatten().min() else {
                // Nothing more will happen: the reader quits.
                return Ok(b"\x03".to_vec());
            };
            if at > end {
                self.world.elapsed.set(end);
                return Ok(Vec::new());
            }
            self.world.elapsed.set(self.world.elapsed.get().max(at));
            // What is due arrives, the images first.
            let due: Vec<Coming> = {
                let mut coming = self.world.coming.borrow_mut();
                let (due, later) = std::mem::take(&mut *coming)
                    .into_iter()
                    .partition(|coming| coming.at <= at);
                *coming = later;
                due
            };
            for coming in due {
                coming.arrival.come(coming.image);
            }
            let mut keys = Vec::new();
            while self.script.front().is_some_and(|(when, _)| *when <= at) {
                let (_, act) = self.script.pop_front().expect("a step that is due");
                keys.extend(self.happen(act));
            }
            if !keys.is_empty() {
                return Ok(keys);
            }
        }
    }

    fn now(&self) -> Instant {
        self.world.start + self.world.elapsed.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_is_its_text_and_not_its_target() {
        let text = plain("\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\");
        assert_eq!(text, "link");
    }

    #[test]
    fn a_graphics_sequence_and_a_bell_ended_string_leave_no_text() {
        assert_eq!(plain("a\x1b_Gi=31;OK\x1b\\b"), "ab");
        assert_eq!(plain("a\x1b]0;title\x07b"), "ab");
        assert_eq!(plain("a\x1bP>|kitty(0.47.1)\x1b\\b"), "ab");
    }

    #[test]
    fn a_csi_sequence_ends_at_its_final_byte() {
        assert_eq!(plain("a\x1b[0;1mb\x1b[5;1Hc\x1b[?2026hd"), "abcd");
    }
}
