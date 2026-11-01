//! A simulated frontend for the pager's tests: the terminal, the clock, the
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
//! [`Sim::run`] runs the pager alone, on a document it is given.
//! [`Sim::kiri`] runs the route as `kiri <files>` does, through
//! `SimPlatform`, which implements the [`Platform`] the binary implements
//! for the process: the disk above, standard input and output and error
//! kept in memory, and a terminal that is the script's. The loads of images
//! come as the script says ([`Sim::loads`]), on the virtual clock: the
//! platform's loads run a `Runner` that runs nothing, and its waits move the
//! clock, so no load reads the real disk or runs a program, and no signal
//! handler is installed. Only a reload's stamps still look at the image
//! files' times on the real disk.
//!
//! This is the frontend of the tests and, like the real ones, belongs to the
//! platform. What it does not do is what the terminal does: raw mode, the
//! alternate screen, signals, programs that draw, and the probe. Those stay on a
//! pseudo-terminal, in `tests/it/cli_tmux.rs` and the others.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use crate::config::{Config, Environment, Options as Choices, Passthrough, Settings};
use crate::frontend::pager::close;
use crate::frontend::{Asked, Configured, Failure, Pause, Platform, Screen, show_files};
use crate::images::{ImageError, ImageKey, Images, Loads, Source};
use crate::load::queue::{Done, Job, Queued, Runner};
use crate::load::watch::{Reloaded, SourceFiles};
use crate::pager::{Options, Terminal, run};
use crate::present::kitty::graphics::{Cells, ImageId};
use crate::render::Inputs;
use crate::session::plan::{self, Route};
use crate::session::settings::{Follow, FollowJump, SearchCase};
use crate::session::sources::{Reread, Session, SourceFile};
use crate::session::state::Look;
use crate::term::{Mode, WindowSize};
use crate::typeset::caps::Capabilities;
use crate::typeset::input::Input;
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

    fn has(&self, path: &str) -> bool {
        self.files.contains_key(path)
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
    /// The images on their way: when each comes, and as what.
    coming: RefCell<Vec<Coming>>,
    /// What each load that starts comes to, as the script says.
    loading: RefCell<Script>,
    /// What was written to the terminal: the screen and standard output
    /// are one stream, as they are on a terminal. Each write borrows it
    /// for that call only, as the route holds two handles to it at once.
    out: RefCell<Vec<u8>>,
    /// What was written to standard error. Behind a lock and not a
    /// `RefCell`: the platform's standard error is `Send`.
    err: Arc<Mutex<Vec<u8>>>,
    /// The window's size now.
    size: Cell<WindowSize>,
    /// How long each read of keys was asked to wait.
    waits: RefCell<Vec<Duration>>,
    /// When the screen was left, on the real clock.
    left: Cell<Option<Instant>>,
    /// How much had been written to standard error when the screen was
    /// left.
    told_before_leaving: Cell<Option<usize>>,
}

impl World {
    /// A world with `disk`, in a window of `columns` × `rows` cells.
    fn new(disk: Disk, columns: u16, rows: u16) -> Self {
        World {
            start: Instant::now(),
            elapsed: Cell::new(Duration::ZERO),
            disk: RefCell::new(disk),
            coming: RefCell::new(Vec::new()),
            loading: RefCell::new(Script::new()),
            out: RefCell::new(Vec::new()),
            err: Arc::default(),
            size: Cell::new(WindowSize {
                columns,
                rows,
                width: 0,
                height: 0,
            }),
            waits: RefCell::new(Vec::new()),
            left: Cell::new(None),
            told_before_leaving: Cell::new(None),
        }
    }

    /// The clock moves on to `at`, if that is later, and the images due by
    /// then come.
    fn arrive(&self, at: Duration) {
        self.elapsed.set(self.elapsed.get().max(at));
        let due: Vec<Coming> = {
            let mut coming = self.coming.borrow_mut();
            let (due, later) = std::mem::take(&mut *coming)
                .into_iter()
                .partition(|coming| coming.at <= at);
            *coming = later;
            due
        };
        for coming in due {
            coming.arrival.come(coming.image);
        }
    }

    /// Lets the clock run for `most` or until an image comes, which it
    /// does as it passes its time.
    fn pass(&self, most: Duration) {
        let end = self.elapsed.get() + most;
        let next = (self.coming.borrow().iter()).map(|coming| coming.at).min();
        match next.filter(|at| *at <= end) {
            Some(at) => self.arrive(at),
            None => self.elapsed.set(end),
        }
    }

    /// What a run came to.
    fn ran(&self, reloads: usize, result: Option<Result<(), Failure>>) -> Ran {
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).expect("kiri writes UTF-8");
        let before_leaving = self.told_before_leaving.get();
        Ran {
            out: text(self.out.take()),
            err: text(std::mem::take(
                &mut *self.err.lock().expect("no panic holds it"),
            )),
            waits: self.waits.take(),
            elapsed: self.elapsed.get(),
            reloads,
            result,
            before_leaving,
        }
    }
}

struct Coming {
    at: Duration,
    arrival: crate::images::Arrival,
    image: Result<Source, ImageError>,
}

/// What the script says a load of an image comes to, one for each time the
/// image starts loading: after how long, and as what.
type Script = HashMap<ImageKey, VecDeque<(Duration, Result<Source, ImageError>)>>;

/// Runs the loads as the script says: each comes on the virtual clock, in
/// [`World::coming`], and one the script says nothing of never comes. The
/// script is by the url of an image, whatever the document it is in.
struct SimRunner {
    world: Rc<World>,
}

impl std::fmt::Debug for SimRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimRunner").finish_non_exhaustive()
    }
}

impl Runner for SimRunner {
    fn run(&mut self, jobs: Vec<Queued>, done: &std::sync::mpsc::Sender<Done>) {
        for queued in jobs {
            let key = queued.key().clone().in_document(0);
            let next =
                (self.world.loading.borrow_mut().get_mut(&key)).and_then(VecDeque::pop_front);
            let Some((after, image)) = next else {
                continue;
            };
            let at = self.world.elapsed.get() + after;
            let arrival = queued.arrival(done);
            self.world
                .coming
                .borrow_mut()
                .push(Coming { at, arrival, image });
        }
    }

    fn prefer(&mut self, _: HashMap<ImageKey, usize>) {}

    /// No program draws.
    fn wait(&self, _: Duration) -> bool {
        true
    }
}

/// A change to the settings that [`Sim::kiri`] runs with.
type ChangeSettings = Box<dyn FnOnce(&mut Settings)>;

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
    /// The loads that kiri's route starts: for each url, what each load
    /// comes to, and after how long.
    loads: Vec<(String, Duration, Result<Source, ImageError>)>,
    /// Whether standard error is a terminal, which shows the progress line.
    stderr_terminal: bool,
    /// What standard input holds, when it is not a terminal.
    stdin: Option<String>,
    /// Whether standard output is a pipe and not a terminal.
    piped: bool,
    /// Whether there is no terminal to read keys from.
    keyless: bool,
    /// What changes the settings that `kiri` runs with.
    changes: Vec<ChangeSettings>,
    /// The input format the files are read as, as `--format` asks; by
    /// their names when `None`.
    format: Option<Input>,
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
            loads: Vec::new(),
            stderr_terminal: false,
            stdin: None,
            piped: false,
            keyless: false,
            changes: Vec::new(),
            format: None,
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

    /// Has the next load of the image at `url` that kiri's route starts
    /// come `after` it started, as `image`; one load for each call, in the
    /// order they start. A load the script says nothing of never comes,
    /// and a wait for it ends the test.
    pub(crate) fn loads(
        mut self,
        url: &str,
        after: Duration,
        image: Result<Source, ImageError>,
    ) -> Self {
        self.loads.push((url.to_owned(), after, image));
        self
    }

    /// Standard error is a terminal, so that the progress line of a wait
    /// shows there, and Space can skip it.
    pub(crate) fn stderr_terminal(mut self) -> Self {
        self.stderr_terminal = true;
        self
    }

    /// Standard input holding `text`; it is not a terminal.
    pub(crate) fn stdin(mut self, text: &str) -> Self {
        self.stdin = Some(text.to_owned());
        self
    }

    /// Standard output is a pipe, not a terminal. The caps stay as given:
    /// pass [`Sim::caps`] with plain ones for what `--mode auto` gets on a
    /// pipe.
    pub(crate) fn piped(mut self) -> Self {
        self.piped = true;
        self
    }

    /// No terminal to read keys from, as in the background.
    pub(crate) fn no_keys(mut self) -> Self {
        self.keyless = true;
        self
    }

    /// The settings, from the defaults with [`fixed_theme`], changed by
    /// `change`.
    pub(crate) fn settings(mut self, change: impl FnOnce(&mut Settings) + 'static) -> Self {
        self.changes.push(Box::new(change));
        self
    }

    /// Reads the files as `format` says, as `--format` does.
    pub(crate) fn format(mut self, format: Input) -> Self {
        self.format = Some(format);
        self
    }

    /// Runs kiri on `files` (`-` for standard input) through the route, as
    /// `kiri <files>` does: [`plan::route`], then [`show_files`]. Its loads
    /// of images are the script's ([`Sim::loads`]): nothing reads the real
    /// disk or runs a program.
    pub(crate) fn kiri(self, files: &[&str]) -> Ran {
        assert!(
            self.brings.is_empty() && self.inline.is_empty(),
            ".reload_brings and .inline_brings are for Sim::run: \
             kiri's route takes its loads from .loads"
        );
        assert!(
            self.shown.is_none(),
            ".show is for Sim::run: kiri takes its files as arguments"
        );
        assert!(
            !self.reading,
            ".reading is for Sim::run: the route always has a reading mode"
        );
        let Sim {
            columns,
            rows,
            caps,
            follow,
            jump,
            disk,
            mut script,
            loads,
            stderr_terminal,
            stdin,
            piped,
            keyless,
            changes,
            format,
            ..
        } = self;
        script.sort_by_key(|(at, _)| *at);
        let world = Rc::new(World::new(disk, columns, rows));
        for (url, after, image) in loads {
            let key = ImageKey::from(url.as_str());
            (world.loading.borrow_mut().entry(key).or_default()).push_back((after, image));
        }
        let terminal = SimTerminal::new(Rc::clone(&world), script);
        let platform = SimPlatform {
            stdin: RefCell::new(stdin.as_deref().map(|text| text.as_bytes().to_vec())),
            piped,
            stderr_terminal,
            ..SimPlatform::new(&world, caps, (!keyless).then_some(terminal))
        };
        let env = platform.environment();
        let mut settings = crate::config::choose(&Choices::default(), env, &Config::default())
            .with_theme(fixed_theme());
        settings.follow = follow;
        settings.follow_jump = jump;
        changes.into_iter().for_each(|change| change(&mut settings));
        let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
        let route = plan::route(&paths, stdin.is_none());
        assert!(
            !matches!(route, Route::Missing | Route::RepeatedStdin),
            "the command line is wrong: {route:?}"
        );
        let asked = Asked {
            files: &paths,
            format,
            follow,
        };
        let configured = Configured {
            warnings: Vec::new(),
            settings: Ok(settings),
        };
        let result = show_files(&platform, &asked, route, configured);
        world.ran(0, Some(result))
    }

    /// Runs the pager through the script, and gives what it did.
    pub(crate) fn run(self) -> Ran {
        assert!(
            self.stdin.is_none()
                && !self.piped
                && !self.keyless
                && self.changes.is_empty()
                && self.loads.is_empty()
                && !self.stderr_terminal,
            ".stdin, .piped, .no_keys and .settings are for Sim::kiri, as are \
             .loads and .stderr_terminal: run has its own terminal and settings"
        );
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
            ..
        } = self;
        script.sort_by_key(|(at, _)| *at);
        let world = Rc::new(World::new(disk, columns, rows));
        // What each image comes to, in the order it starts loading.
        let script_of = |key: &str, after, image| {
            let key = ImageKey::from(key);
            (world.loading.borrow_mut().entry(key).or_default()).push_back((after, image));
        };
        let brought: Vec<String> = brings.iter().map(|(key, ..)| key.clone()).collect();
        for (key, after, image) in brings {
            script_of(&key, after, image);
        }
        let inline_keys: Vec<String> = inline.iter().map(|(key, ..)| key.clone()).collect();
        for (key, after, image) in inline {
            script_of(&key, after, Ok(image));
        }
        let mut inputs = Inputs {
            theme: fixed_theme(),
            ..Inputs::new(caps.clone())
        };
        let document = match &shown {
            Some(path) => parse(&world.disk.borrow().read(path).expect("the file shown")),
            None => parse(""),
        };
        // How many times the document was read again, which the planner
        // asks for: the first reload makes what it brings wanted.
        let asked = Rc::new(Cell::new(0));
        let reloads = Rc::clone(&asked);
        let mut files = shown.as_ref().map(|_| SimFiles {
            world: Rc::clone(&world),
            asked: Rc::clone(&asked),
        });
        let terminal = SimTerminal::new(Rc::clone(&world), script);
        let options = Options {
            files: files.as_mut().map(|files| files as &mut dyn SourceFiles),
            follow,
            jump,
            reading: reading.then(|| Reading {
                pictures: Default::default(),
                fonts: Rc::new(Fonts::fixture()),
                wide: Wide::Wrap,
            }),
            // What a reload brings, and the pictures in text that reading
            // mode shows, are what the document wants.
            planner: Some(Box::new(move |_, _, reading| {
                let brought = (reloads.get() > 0).then_some(&brought);
                let inline = reading.then_some(&inline_keys);
                (brought.into_iter().chain(inline).flatten())
                    .map(|key| Job::file(key))
                    .collect()
            })),
            ..Options::new(SearchCase::Smart)
        };
        let mut session = Session::new([shown.as_deref().unwrap_or("notes.md")]);
        if let Some(path) = &shown {
            let look = world.disk.borrow().look(path).ok();
            let (path, input) = (PathBuf::from(path), Input::Markdown);
            session.read_from(session.sources()[0].id(), SourceFile { path, input, look });
        }
        let runner = SimRunner {
            world: Rc::clone(&world),
        };
        let mut loads = Loads::with(Box::new(runner));
        let mut screen: Box<dyn Screen> = Box::new(terminal);
        run(
            Content::Document(&document),
            None,
            session,
            &mut inputs,
            options,
            &mut loads,
            &mut screen,
        )
        .expect("the simulated terminal does not fail");
        let platform = SimPlatform::new(&world, caps, None);
        close(&platform, screen, &mut loads);
        world.ran(asked.get(), None)
    }
}

fn parse(text: &str) -> Document {
    crate::typeset::parse::parse(text)
}

/// What a run of the pager, or of kiri, did.
pub(crate) struct Ran {
    /// Everything it wrote to the terminal, the screen and standard output.
    pub(crate) out: String,
    /// Everything it wrote to standard error.
    pub(crate) err: String,
    /// How long each read of keys was asked to wait.
    pub(crate) waits: Vec<Duration>,
    /// The time on the virtual clock when the pager quit.
    pub(crate) elapsed: Duration,
    /// How many times the pager read the document again. Always 0 after
    /// [`Sim::kiri`], whose reloads read the disk through `PlatformFiles`.
    pub(crate) reloads: usize,
    /// How kiri ended, after [`Sim::kiri`]; `None` for [`Sim::run`].
    pub(crate) result: Option<Result<(), Failure>>,
    /// How much of `err` was written before the pager left its screen;
    /// `None` when it never opened.
    before_leaving: Option<usize>,
}

impl Ran {
    /// Asserts that nothing was written to standard error, and gives the
    /// run back. A test that expects a warning reads `err` instead.
    #[track_caller]
    pub(crate) fn quiet(self) -> Self {
        assert_eq!(self.err, "", "standard error is not empty");
        self
    }

    /// What was told on standard error while the pager showed or before
    /// it opened: all of `err` when it never opened.
    pub(crate) fn told_before_the_pager_left(&self) -> &str {
        &self.err[..self.before_leaving.unwrap_or(self.err.len())]
    }

    /// What was told on standard error once the pager had left its screen,
    /// where it stays on the terminal; nothing when it never opened.
    pub(crate) fn told_after_the_pager_left(&self) -> &str {
        &self.err[self.before_leaving.unwrap_or(self.err.len())..]
    }

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
pub(crate) fn plain(text: &str) -> String {
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

/// The files behind the pager's sources: the simulated disk.
struct SimFiles {
    world: Rc<World>,
    /// How many times the pager asked for the file again, which the
    /// planner reads too.
    asked: Rc<Cell<usize>>,
}

impl SourceFiles for SimFiles {
    fn look(&mut self, path: &Path) -> Result<Look, String> {
        self.world.disk.borrow().look(&path.to_string_lossy())
    }

    fn reread(
        &mut self,
        job: &Reread,
        _images: &mut Images,
        _loads: &mut Loads,
        _caps: &Capabilities,
        _reading: bool,
    ) -> (Option<Look>, Reloaded) {
        self.asked.set(self.asked.get() + 1);
        let path = job.path.to_string_lossy();
        // Before the file is read, so that a change meanwhile counts.
        let read = self.world.disk.borrow().look(&path).ok();
        let text = match self.world.disk.borrow().read(&path) {
            Ok(text) => text,
            Err(why) => return (read, Reloaded::Failed(why)),
        };
        (read, Reloaded::Document(parse(&text)))
    }
}

/// The terminal: the keys and edits of the script at their times, on the
/// virtual clock, and what the pager writes kept.
struct SimTerminal {
    world: Rc<World>,
    script: std::collections::VecDeque<(Duration, Act)>,
}

impl SimTerminal {
    fn new(world: Rc<World>, script: Vec<(Duration, Act)>) -> Self {
        SimTerminal {
            world,
            script: script.into(),
        }
    }

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
                let size = self.world.size.get();
                self.world.size.set(WindowSize {
                    columns,
                    rows,
                    ..size
                });
            }
        }
        Vec::new()
    }
}

impl SimTerminal {
    /// Lets the clock run for `timeout` or until keys come, as the script
    /// and the arrivals say, and gives the keys typed; `None` once nothing
    /// more happens, with the clock where it stopped.
    fn pass(&mut self, timeout: Duration) -> Option<Vec<u8>> {
        let end = self.world.elapsed.get() + timeout;
        loop {
            let next_act = self.script.front().map(|(at, _)| *at);
            let next_image = (self.world.coming.borrow().iter())
                .map(|coming| coming.at)
                .min();
            let at = [next_act, next_image].into_iter().flatten().min()?;
            if at > end {
                self.world.elapsed.set(end);
                return Some(Vec::new());
            }
            // What is due arrives, the images first.
            self.world.arrive(at);
            let mut keys = Vec::new();
            while self.script.front().is_some_and(|(when, _)| *when <= at) {
                let (_, act) = self.script.pop_front().expect("a step that is due");
                keys.extend(self.happen(act));
            }
            if !keys.is_empty() {
                return Some(keys);
            }
        }
    }
}

impl Write for SimTerminal {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.world.out.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Terminal for SimTerminal {
    fn size(&self) -> Option<WindowSize> {
        Some(self.world.size.get())
    }

    /// Lets the clock run for `timeout` or until keys come, with the
    /// script's edits and the images' arrivals happening as it passes
    /// their times. Out of script, the reader quits.
    fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        self.world.waits.borrow_mut().push(timeout);
        // Nothing more will happen: the reader quits.
        Ok(self.pass(timeout).unwrap_or_else(|| b"\x03".to_vec()))
    }

    fn now(&self) -> Instant {
        self.world.start + self.world.elapsed.get()
    }
}

/// When [`close_over_screen`] started, left the screen, and returned, on
/// the real clock.
pub(crate) struct Closed {
    pub(crate) started: Instant,
    pub(crate) left: Instant,
    pub(crate) returned: Instant,
}

/// Ends a pager with [`close`] over a simulated screen that nothing is
/// shown on, with `loads` for its loads, and tells when the screen was left.
pub(crate) fn close_over_screen(loads: &mut Loads) -> Closed {
    let world = Rc::new(World::new(Disk::default(), 40, 5));
    let platform = SimPlatform::new(&world, Capabilities::ansi(40), None);
    let screen = Box::new(SimTerminal::new(Rc::clone(&world), Vec::new()));
    let started = Instant::now();
    close(&platform, screen, loads);
    let returned = Instant::now();
    let left = world.left.get().expect("close left the screen");
    Closed {
        started,
        left,
        returned,
    }
}

impl Screen for SimTerminal {
    fn close_then(self: Box<Self>, after: Box<dyn FnOnce() + '_>) {
        self.world.left.set(Some(Instant::now()));
        let told = self.world.err.lock().expect("no panic holds it").len();
        self.world.told_before_leaving.set(Some(told));
        after();
    }
}

/// What kiri's waits for loads wait with in the simulated frontend: the
/// virtual clock moves to the next arrival, or by the time asked, and with
/// keys the script's keys are read from the one terminal, which the pager
/// takes later.
struct SimPause<'a> {
    platform: &'a SimPlatform,
    keys: bool,
}

impl Pause for SimPause<'_> {
    fn until(&mut self, loads: &mut Loads, most: Duration) -> bool {
        let world = &self.platform.world;
        let mut terminal = self.platform.terminal.borrow_mut();
        let terminal = terminal.as_mut().filter(|_| self.keys);
        let comes = !world.coming.borrow().is_empty();
        let types = terminal
            .as_ref()
            .is_some_and(|terminal| !terminal.script.is_empty());
        assert!(
            comes || types,
            "the wait for {:?} would never end: the script brings no load and types no key",
            loads.keys().collect::<Vec<_>>()
        );
        let Some(terminal) = terminal else {
            world.pass(most);
            return false;
        };
        // Arrivals and keys alike pass the time on the one clock.
        match terminal.pass(most) {
            Some(typed) => crate::term::skips(&typed),
            None => {
                world.pass(most);
                false
            }
        }
    }

    /// The virtual clock has no time to wait for a line to show: it shows
    /// at once.
    fn delay(&self) -> Duration {
        Duration::ZERO
    }
}

/// Standard output: the terminal's stream, one handle among several.
struct SimOut(Rc<World>);

impl Write for SimOut {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.out.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Standard error.
struct SimErr(Arc<Mutex<Vec<u8>>>);

impl Write for SimErr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (self.0.lock().expect("no panic holds it")).extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// What the frontends reach of the world, in memory: the simulated
/// disk, the streams of [`World`], and a terminal on the virtual clock.
struct SimPlatform {
    world: Rc<World>,
    env: Environment,
    caps: Capabilities,
    /// What standard input holds, until it is read.
    stdin: RefCell<Option<Vec<u8>>>,
    /// Whether standard output is a pipe.
    piped: bool,
    /// Whether standard error is a terminal.
    stderr_terminal: bool,
    /// The one terminal, until the pager takes it; `None` without keys.
    terminal: RefCell<Option<SimTerminal>>,
}

impl SimPlatform {
    /// The platform of `world`, with `caps` and the one `terminal`, and
    /// standard input holding nothing.
    fn new(world: &Rc<World>, caps: Capabilities, terminal: Option<SimTerminal>) -> Self {
        SimPlatform {
            world: Rc::clone(world),
            env: Environment::default(),
            caps,
            stdin: RefCell::new(None),
            piped: false,
            stderr_terminal: false,
            terminal: RefCell::new(terminal),
        }
    }
}

impl Platform for SimPlatform {
    fn is_dir(&self, _: &Path) -> bool {
        false
    }

    fn is_file(&self, path: &Path) -> bool {
        self.world.disk.borrow().has(&path.to_string_lossy())
    }

    fn look(&self, path: &Path) -> Result<Look, String> {
        self.world.disk.borrow().look(&path.to_string_lossy())
    }

    fn open(&self, path: &Path) -> io::Result<Box<dyn Read>> {
        match self.world.disk.borrow().read(&path.to_string_lossy()) {
            Ok(text) => Ok(Box::new(Cursor::new(text.into_bytes()))),
            Err(_) => Err(io::Error::from_raw_os_error(2)),
        }
    }

    fn stdin(&self) -> Box<dyn Read> {
        let bytes = self.stdin.borrow_mut().take().unwrap_or_default();
        Box::new(Cursor::new(bytes))
    }

    fn stdout(&self) -> Box<dyn Write + '_> {
        Box::new(SimOut(Rc::clone(&self.world)))
    }

    fn stderr(&self) -> Box<dyn Write + Send + '_> {
        Box::new(SimErr(Arc::clone(&self.world.err)))
    }

    fn environment(&self) -> &Environment {
        &self.env
    }

    fn reading_fonts(&self) -> Fonts {
        Fonts::fixture()
    }

    fn stdout_is_terminal(&self) -> bool {
        !self.piped
    }

    fn stderr_is_terminal(&self) -> bool {
        self.stderr_terminal
    }

    fn capabilities(&self, _: Mode, width: Option<u16>, _: Passthrough) -> Capabilities {
        let mut caps = self.caps.clone();
        caps.width = width.unwrap_or(caps.width);
        caps.height = self.window().map(|window| window.rows);
        caps
    }

    fn window(&self) -> Option<WindowSize> {
        Some(self.world.size.get())
    }

    fn lost_images(&self, _: &[(ImageId, Cells)]) -> Option<Vec<ImageId>> {
        Some(Vec::new())
    }

    /// The loads run as the script says, on the virtual clock.
    fn loads(&self) -> Loads {
        let world = Rc::clone(&self.world);
        Loads::with(Box::new(SimRunner { world }))
    }

    fn pause(&self, keys: bool) -> Option<Box<dyn Pause + '_>> {
        // Without a terminal there are no keys to read, as for the real one.
        if keys && self.terminal.borrow().is_none() {
            return None;
        }
        Some(Box::new(SimPause {
            platform: self,
            keys,
        }))
    }

    /// None: no signal handler in the test process; Ctrl-C stays on the
    /// pseudo-terminal.
    fn interrupt(&self) -> Option<crate::term::Interrupt> {
        None
    }

    fn screen(
        &self,
        caught: Box<dyn FnOnce() + '_>,
        _: bool,
        _: bool,
    ) -> Option<Box<dyn Screen + '_>> {
        let terminal = self.terminal.borrow_mut().take()?;
        caught();
        Some(Box::new(terminal))
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

    /// A window of 40 × 12 on kitty, with `doc.md` holding `Above`, the
    /// image `a.png` and `Below`, paged, with its image loading in the
    /// background.
    fn pictured() -> Sim {
        Sim::new(40, 12)
            .caps(Capabilities::kitty(40))
            .file("doc.md", "Above\n\n![a](a.png)\n\nBelow\n")
            .settings(|settings| {
                settings.paging = crate::session::settings::Paging::Always;
                settings.initial_load = crate::session::settings::InitialLoad::Background;
            })
    }

    fn red() -> Result<Source, ImageError> {
        let pixel = image::Rgba([255, 0, 0, 255]);
        Ok(Source::Raster(image::RgbaImage::from_pixel(20, 20, pixel)))
    }

    #[test]
    fn a_load_the_script_brings_to_the_route_comes_on_the_virtual_clock() {
        let ran = pictured()
            .loads("a.png", Duration::from_millis(300), red())
            .keys(2000, "q")
            .kiri(&["doc.md"]);
        assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
        // The band that stands for the picture, and then the picture, sent
        // once it has come.
        let (before, after) = ran.out.split_at(ran.out.find("\x1b_Ga=T").expect("sent"));
        assert!(before.contains("\x1b_Ga=t"), "{before:?}");
        assert_eq!(after.matches("\x1b_Ga=T").count(), 1, "{after:?}");
        assert!(
            ran.elapsed >= Duration::from_millis(300),
            "{:?}",
            ran.elapsed
        );
        assert_eq!(ran.err, "");
    }

    #[test]
    fn what_is_told_is_split_at_the_time_the_pager_left_its_screen() {
        let missing = Err(ImageError::Missing("a.png".into()));
        let ran = pictured()
            .loads("a.png", Duration::from_millis(300), missing)
            .keys(2000, "q")
            .kiri(&["doc.md"]);
        assert_eq!(ran.told_before_the_pager_left(), "");
        assert_eq!(
            ran.told_after_the_pager_left(),
            "kiri: a.png: file not found\n"
        );
        // Printed, it is all told, and nothing after.
        let ran = Sim::new(40, 12).file("doc.md", "A\n").kiri(&["doc.md"]);
        assert_eq!(ran.told_after_the_pager_left(), "");
    }

    #[test]
    #[should_panic(expected = "would never end")]
    fn a_wait_for_a_load_the_script_says_nothing_of_ends_the_test() {
        let _ = pictured()
            .settings(|settings| {
                settings.initial_load = crate::session::settings::InitialLoad::Wait;
            })
            .stderr_terminal()
            .kiri(&["doc.md"]);
    }

    #[test]
    fn the_progress_line_shows_on_a_standard_error_that_is_a_terminal() {
        let ran = pictured()
            .settings(|settings| {
                settings.initial_load = crate::session::settings::InitialLoad::Wait;
            })
            .loads("a.png", Duration::from_millis(300), red())
            .stderr_terminal()
            .keys(2000, "q")
            .kiri(&["doc.md"]);
        assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
        let line = crate::term::progress::line(0, 1);
        let hint = crate::term::progress::HINT;
        assert_eq!(
            ran.err,
            format!("\r\x1b[2K{line}{hint}\r\x1b[2K"),
            "shown, and wiped"
        );
    }

    #[test]
    #[should_panic(expected = ".stdin, .piped, .no_keys and .settings are for Sim::kiri")]
    fn run_refuses_stdin() {
        Sim::new(40, 5).stdin("x").keys(10, "q").run();
    }

    #[test]
    #[should_panic(expected = ".stdin, .piped, .no_keys and .settings are for Sim::kiri")]
    fn run_refuses_piped() {
        Sim::new(40, 5).piped().run();
    }

    #[test]
    #[should_panic(expected = ".stdin, .piped, .no_keys and .settings are for Sim::kiri")]
    fn run_refuses_no_keys() {
        Sim::new(40, 5).no_keys().run();
    }

    #[test]
    #[should_panic(expected = ".stdin, .piped, .no_keys and .settings are for Sim::kiri")]
    fn run_refuses_settings() {
        Sim::new(40, 5).settings(|_| {}).run();
    }

    #[test]
    #[should_panic(expected = "as are .loads and .stderr_terminal")]
    fn run_refuses_loads() {
        let image = Err(ImageError::Remote);
        Sim::new(40, 5).loads("a.png", Duration::ZERO, image).run();
    }

    #[test]
    #[should_panic(expected = "as are .loads and .stderr_terminal")]
    fn run_refuses_stderr_terminal() {
        Sim::new(40, 5).stderr_terminal().run();
    }
}
