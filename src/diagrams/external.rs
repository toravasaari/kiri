//! Diagrams drawn by a program outside kiri, such as mermaid-cli's `mmdc`.
//!
//! kiri writes the diagram's source to a file in a directory of its own,
//! and for Mermaid the program's settings beside it. It runs the command
//! that kiri's configuration file gives, or that kiri found, without a
//! shell and with that directory as its temporary directory, and reads
//! the image the program wrote, SVG or raster. The directory goes, with
//! whatever the program left in it, when the program has finished,
//! failed, run out of time or been stopped; one that a kiri which was
//! killed left goes the next time a kiri draws there.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, DirBuilder, File};
use std::io::{self, ErrorKind, Read};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use super::{Drawn, Renderer, Request};

/// How long a program may take to draw one diagram before kiri stops it,
/// unless the configuration file says otherwise.
pub const TIME_LIMIT: Duration = Duration::from_secs(20);

/// The most bytes of an image that a program writes that kiri reads: as
/// many as the largest image file `--max-image-size` lets it read, a guard
/// only against a program that runs away. That option limits image files
/// alone; the pixel limit and the drawing at a smaller scale take care of
/// a large PNG.
pub const MAX_IMAGE_SIZE: u64 = 100 * crate::images::MB;

/// How many programs draw diagrams at once.
pub const AT_ONCE: usize = 4;

/// How many pixels to the CSS pixel a command draws a raster image with
/// when it takes the scale as `{scale}`, unless the image is then too
/// large to load; see `smaller_scale`.
const SCALE: u32 = 3;

/// The longest path of the directory that the directories for diagrams'
/// files go in. A program gets one of them as its temporary directory, and
/// Chromium, which `mmdc` runs, may make a socket in that: a socket's path
/// takes at most 107 bytes, and a longer one would fail.
pub const LONGEST_WORK_DIR: usize =
    107 - "/.org.chromium.Chromium.XXXXXX/SingletonSocket".len() - "/4194304-999".len();

/// Why a directory cannot hold the directories for diagrams' files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unusable {
    /// Its path is longer than [`LONGEST_WORK_DIR`] bytes.
    TooLong,
    /// This, on its path, is a file rather than a directory.
    NotADirectory(PathBuf),
    /// This directory on its path cannot be written to or searched.
    NotWritable(PathBuf),
}

/// Why `work` cannot hold the directories for diagrams' files, as
/// [`ExternalSettings::work_dir`] is used, without making it; `None`
/// when it can, as far as can be told before trying.
pub fn unusable(work: &Path) -> Option<Unusable> {
    if work.as_os_str().len() > LONGEST_WORK_DIR {
        return Some(Unusable::TooLong);
    }
    not_writable(work)
}

/// Why no directory can be made in `dir`, or in what of it can be made;
/// `None` when one can.
pub fn not_writable(dir: &Path) -> Option<Unusable> {
    use rustix::fs::{Access, access};

    // The nearest part of its path that is there decides.
    for part in dir.ancestors().filter(|part| !part.as_os_str().is_empty()) {
        match fs::metadata(part) {
            Ok(found) if found.is_dir() => {
                let writable = access(part, Access::WRITE_OK | Access::EXEC_OK).is_ok();
                return (!writable).then(|| Unusable::NotWritable(part.to_owned()));
            }
            Ok(_) => return Some(Unusable::NotADirectory(part.to_owned())),
            // A file further up the path tells.
            Err(error)
                if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) =>
            {
                continue;
            }
            Err(_) => return Some(Unusable::NotWritable(part.to_owned())),
        }
    }
    None
}

/// A program that draws diagrams and its arguments, in which `{input}`
/// stands for the file of the diagram's source, `{output}` for the image
/// file the program writes, `{theme}` for the theme kiri chose, `{config}`
/// for a configuration file kiri writes for the language, such as
/// Mermaid's, and `{scale}` for the number of pixels to the CSS pixel
/// that a program which takes it draws a raster image with, a whole
/// number. `{output}` ends in `.png` for such a program and in `.svg` for
/// others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub argv: Vec<String>,
    /// The `PATH` the program runs with, when it is not kiri's own.
    pub path: Option<OsString>,
}

impl Command {
    pub fn new(argv: Vec<String>) -> Self {
        Command { argv, path: None }
    }

    /// What the program writes: a raster image at [`SCALE`] when it takes
    /// `{scale}`, and otherwise SVG, or a raster image at one pixel to the
    /// CSS pixel.
    fn writes(&self) -> Output {
        match self.argv.iter().any(|arg| arg.contains("{scale}")) {
            true => Output::Raster,
            false => Output::Svg,
        }
    }

    /// The program's name for messages: the last part of its path.
    fn program(&self) -> String {
        let program = self.argv.first().map_or("", String::as_str);
        Path::new(program)
            .file_name()
            .map_or_else(|| program.to_owned(), |name| name.to_string_lossy().into())
    }
}

/// How an [`External`] runs its programs.
#[derive(Debug, Clone)]
pub struct ExternalSettings {
    /// How long a program may take to draw one diagram before it is
    /// stopped.
    pub time_limit: Duration,
    /// The most bytes of an image that is read; by default
    /// [`MAX_IMAGE_SIZE`].
    pub max_size: u64,
    /// Where the directories for the programs' files go, made when it is
    /// needed, for the user alone, such as `diagrams` in kiri's cache
    /// directory. Without it, or when it cannot be made or its path is too
    /// long, they go in `temp_dir`.
    pub work_dir: Option<PathBuf>,
    /// The system's temporary directory, such as `config::temp_directory`
    /// gives.
    pub temp_dir: PathBuf,
    /// Set when the programs are to stop, and no more are to start, such
    /// as on Ctrl-C.
    pub stop: Option<Arc<AtomicBool>>,
}

impl ExternalSettings {
    /// The settings kiri has unless told otherwise, with `temp_dir` as the
    /// system's temporary directory.
    pub fn new(temp_dir: PathBuf) -> Self {
        ExternalSettings {
            time_limit: TIME_LIMIT,
            max_size: MAX_IMAGE_SIZE,
            work_dir: None,
            temp_dir,
            stop: None,
        }
    }
}

/// Draws the diagrams of some languages with external commands.
#[derive(Debug)]
pub struct External {
    /// The command for each language, by its lower-case name.
    commands: BTreeMap<String, Command>,
    settings: ExternalSettings,
    /// How many directories this renderer has made, to name the next one.
    made: AtomicUsize,
    /// Run once the work directory is there, to remove what an earlier
    /// kiri left in it.
    swept: Once,
}

impl External {
    /// Draws each language in `commands` with its command, as `settings`
    /// say.
    pub fn new(commands: BTreeMap<String, Command>, settings: ExternalSettings) -> Self {
        External {
            commands: (commands.into_iter())
                .map(|(language, command)| (language.to_ascii_lowercase(), command))
                .collect(),
            settings,
            made: AtomicUsize::new(0),
            swept: Once::new(),
        }
    }

    /// Whether the programs are to stop.
    fn stopped(&self) -> bool {
        (self.settings.stop.as_ref()).is_some_and(|stop| stop.load(Ordering::SeqCst))
    }

    fn command(&self, language: &str) -> Option<&Command> {
        self.commands.get(&language.to_ascii_lowercase())
    }

    /// A new directory of its own for one diagram's files: in the work
    /// directory when it can be, and otherwise in the system's temporary
    /// directory, where its name says whose it is. The first time, the
    /// directories that a kiri which has ended left in the work directory
    /// go.
    fn temp_dir(&self) -> io::Result<TempDir> {
        let work = (self.settings.work_dir.as_deref())
            .filter(|work| work.as_os_str().len() <= LONGEST_WORK_DIR)
            .and_then(|work| {
                let made = DirBuilder::new().recursive(true).mode(0o700).create(work);
                made.ok()?;
                self.swept.call_once(|| remove_stale(work, &host_name()));
                self.new_dir(work, "").ok()
            });
        match work {
            Some(dir) => Ok(dir),
            None => self.new_dir(&self.settings.temp_dir, "kiri-diagram-"),
        }
    }

    /// A new directory in `parent`, its name `prefix` followed by kiri's
    /// process id and a number, with a file in it that names this machine.
    /// It gets its name once the file is there, so that no kiri on another
    /// machine that shares the directory takes it for one of its own.
    fn new_dir(&self, parent: &Path, prefix: &str) -> io::Result<TempDir> {
        let host = format!("{}\n", host_name());
        loop {
            let n = self.made.fetch_add(1, Ordering::Relaxed);
            let name = format!("{prefix}{}-{n}", process::id());
            let dir = parent.join(&name);
            let making = parent.join(format!("{name}{MAKING}"));
            // Left over from an earlier kiri with the same process id, or
            // made by one on another machine.
            let taken = |error: &io::Error| {
                matches!(
                    error.kind(),
                    ErrorKind::AlreadyExists | ErrorKind::DirectoryNotEmpty
                )
            };
            if fs::symlink_metadata(&dir).is_ok() {
                continue;
            }
            match DirBuilder::new().mode(0o700).create(&making) {
                Ok(()) => {}
                Err(error) if taken(&error) => continue,
                Err(error) => return Err(error),
            }
            // Removed, should anything below fail.
            let making = TempDir(making);
            fs::write(making.0.join(HOST_FILE), &host)?;
            match making.renamed(dir) {
                Ok(dir) => return Ok(dir),
                Err(error) if taken(&error) => continue,
                Err(error) => return Err(error),
            }
        }
    }

    /// The image file that `command`, which writes `writes`, draws of the
    /// diagram `request` asks for, at `scale` pixels to the CSS pixel when
    /// it takes `{scale}`.
    fn run(
        &self,
        command: &Command,
        writes: Output,
        request: &Request,
        scale: u32,
    ) -> Result<Vec<u8>, String> {
        let program = command.program();
        if self.stopped() {
            return Err(format!("{program} was not started: interrupted"));
        }
        let dir = self
            .temp_dir()
            .map_err(|error| format!("cannot make a directory for {program}: {error}"))?;
        let (files, errors) = Files::write(&dir.0, request, writes)
            .map_err(|error| format!("cannot write the files for {program}: {error}"))?;
        let theme = super::theme_name(request.language, request.theme);
        let argv = files.arguments(&command.argv, theme, scale);
        let mut child = process::Command::new(&argv[0]);
        child
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(errors)
            // Its temporary files, such as the profile of mermaid-cli's
            // browser, in its directory, so that they go with it, also when
            // it is killed.
            .env("TMPDIR", &dir.0)
            // A group of its own, so that stopping it stops what it
            // started, such as mermaid-cli's browser.
            .process_group(0);
        if let Some(path) = &command.path {
            child.env("PATH", path);
        }
        let child = child
            .spawn()
            .map_err(|error| format!("{}: {error}", argv[0]))?;
        self.finish(child, &program, &files)?;
        self.read_output(&program, &files.output)
    }

    /// Waits for `child`, the program `program` that draws into `files`,
    /// to end, until the time limit or a stop; why it failed, as the first
    /// lines it wrote on standard error tell, when it did.
    fn finish(&self, child: process::Child, program: &str, files: &Files) -> Result<(), String> {
        let limit = self.settings.time_limit;
        let stop = || self.stopped();
        let status = wait(child, limit, &stop).map_err(|error| match error {
            Waited::TimedOut => format!("{program} did not finish in {} s", limit.as_secs_f32()),
            Waited::Stopped => format!("{program} was stopped: interrupted"),
            Waited::Failed(error) => format!("{program}: {error}"),
        })?;
        if status.success() {
            return Ok(());
        }
        let said = read_start(&files.errors, SAID);
        let ended = match status.code() {
            Some(code) => format!("{program} exited with status {code}"),
            None => format!("{program} was stopped by a signal"),
        };
        Err(match first_lines(&String::from_utf8_lossy(&said)) {
            Some(line) => format!("{ended}: {line}"),
            None => ended,
        })
    }

    /// The image file at `output` that `program` wrote, up to the size
    /// limit.
    fn read_output(&self, program: &str, output: &Path) -> Result<Vec<u8>, String> {
        let limit = self.settings.max_size;
        match File::open(output).and_then(|file| crate::images::read_at_most(file, limit)) {
            Ok(Some(bytes)) => Ok(bytes),
            Ok(None) => Err(format!(
                "{program} wrote an image larger than the {} limit",
                crate::images::size(limit)
            )),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Err(format!("{program} wrote no output file"))
            }
            Err(error) => Err(format!("cannot read what {program} wrote: {error}")),
        }
    }
}

/// The files of one drawing, in its directory.
struct Files {
    /// The diagram's source.
    input: PathBuf,
    /// The image the program writes.
    output: PathBuf,
    /// What the program says on standard error.
    errors: PathBuf,
    /// The configuration for its language, if it has one.
    config: Option<PathBuf>,
}

impl Files {
    /// The files in `dir` for drawing the diagram `request` asks for with
    /// a command that writes `writes`: its source and its language's
    /// configuration written, and the file for what the program says
    /// made, open for writing.
    fn write(dir: &Path, request: &Request, writes: Output) -> io::Result<(Files, File)> {
        let input = dir.join(format!("diagram.{}", super::extension(request.language)));
        let output = dir.join(match writes {
            Output::Svg => "diagram.svg",
            Output::Raster => "diagram.png",
        });
        fs::write(&input, request.source)?;
        let config = match config(request.language, writes) {
            Some((text, extension)) => {
                let path = dir.join(format!("config.{extension}"));
                fs::write(&path, text)?;
                Some(path)
            }
            None => None,
        };
        let errors = dir.join("errors.txt");
        let errors_file = File::create(&errors)?;
        let files = Files {
            input,
            output,
            errors,
            config,
        };
        Ok((files, errors_file))
    }

    /// The arguments `argv` with the placeholders in them filled in, the
    /// theme by the name `theme` and the scale as `scale`.
    fn arguments(&self, argv: &[String], theme: &str, scale: u32) -> Vec<String> {
        (argv.iter())
            .map(|arg| {
                let arg = (arg.replace("{input}", &self.input.to_string_lossy()))
                    .replace("{output}", &self.output.to_string_lossy())
                    .replace("{theme}", theme)
                    .replace("{scale}", &scale.to_string());
                match &self.config {
                    Some(path) => arg.replace("{config}", &path.to_string_lossy()),
                    None => arg,
                }
            })
            .collect()
    }
}

impl Renderer for External {
    fn draws(&self, language: &str) -> bool {
        self.command(language).is_some()
    }

    fn render(&self, request: &Request) -> Result<Drawn, String> {
        let Some(command) = self.command(request.language) else {
            return Err(format!("{} diagrams have no command", request.language));
        };
        let writes = command.writes();
        let mut scale = match writes {
            Output::Svg => 1,
            Output::Raster => SCALE,
        };
        let mut image = self.run(command, writes, request, scale)?;
        // Once more at a scale that fits, rather than not at all.
        if writes == Output::Raster
            && let Some(smaller) = smaller_scale(&image, scale)
        {
            image = self.run(command, writes, request, smaller)?;
            scale = smaller;
        }
        // The program cannot tell which theme the diagram chose itself.
        let theme = super::chosen_theme(request.language, request.source);
        Ok(Drawn {
            image,
            scale,
            theme: theme.unwrap_or(request.theme),
            background: None,
        })
    }
}

/// A scale smaller than `scale`, at least one pixel to the CSS pixel, at
/// which the PNG `image`, drawn at `scale`, would have no more pixels than
/// kiri loads, when it has more: as many pixels to the CSS pixel as fit,
/// in whole pixels, as a program may take no other. `None` when it fits,
/// when it would not fit even at one, and when it is no PNG, whose size
/// cannot be told without decoding it.
fn smaller_scale(image: &[u8], scale: u32) -> Option<u32> {
    let (width, height) = crate::images::png_size(image)?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels <= crate::images::MAX_PIXELS {
        return None;
    }
    // The pixels grow with the square of the scale.
    let fits = f64::from(scale) * (crate::images::MAX_PIXELS as f64 / pixels as f64).sqrt();
    let smaller = (fits.floor() as u32).min(scale.saturating_sub(1));
    (smaller >= 1).then_some(smaller)
}

/// The configuration a command for `language` that writes `output` gets
/// as `{config}`, and the extension of its file.
fn config(language: &str, output: Output) -> Option<(String, &'static str)> {
    let file = super::language(language)?.config?;
    Some(((file.text)(output), file.extension))
}

/// What a command writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// SVG, or a raster image at one pixel to the CSS pixel.
    Svg,
    /// A raster image at [`SCALE`], when the command takes `{scale}`.
    Raster,
}

/// The file in each directory for a diagram's files that names the machine
/// it was made on.
const HOST_FILE: &str = "host";

/// What the name of a directory for a diagram's files ends in until the
/// file that names the machine is in it.
const MAKING: &str = ".new";

/// This machine's name, as `uname -n` gives it.
fn host_name() -> String {
    (rustix::system::uname().nodename().to_string_lossy()).into_owned()
}

/// A directory that is removed with everything in it when dropped.
struct TempDir(PathBuf);

impl TempDir {
    /// This directory under the path `to`, unless it cannot be renamed;
    /// then it is removed.
    fn renamed(mut self, to: PathBuf) -> io::Result<TempDir> {
        fs::rename(&self.0, &to)?;
        self.0 = to;
        Ok(self)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Removes the directories in `work` that a kiri on this machine, named
/// `host`, left there when it ended, such as one killed while a program
/// drew: those named, as `new_dir` names them, by the id of a process that
/// no longer runs, and whose file names this machine. A directory whose
/// process still runs, or might, is left alone, as is one that names
/// another machine, whose processes kiri cannot see, and anything else in
/// `work`. So is one that names no machine: no kiri made it, or one was
/// killed after making it and before writing the file. Then it is empty
/// and its name ends in `.new`, and kiri cannot tell whether a kiri on
/// another machine is still writing the file.
fn remove_stale(work: &Path, host: &str) {
    use rustix::io::Errno;
    use rustix::process::{Pid, test_kill_process};

    let Ok(entries) = fs::read_dir(work) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        // Also one that a kiri was making when it was killed.
        let name = name.strip_suffix(MAKING).unwrap_or(name);
        let Some((pid, n)) = name.split_once('-') else {
            continue;
        };
        let (Ok(id), Ok(number)) = (pid.parse::<i32>(), n.parse::<usize>()) else {
            continue;
        };
        // Only the names kiri gives, without signs or leading zeros.
        if format!("{id}-{number}") != name {
            continue;
        }
        let Some(pid) = Pid::from_raw(id) else {
            continue;
        };
        let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        let made_here = || {
            let named = fs::read_to_string(entry.path().join(HOST_FILE));
            !host.is_empty() && named.is_ok_and(|named| named.trim_end_matches('\n') == host)
        };
        if is_dir && made_here() && test_kill_process(pid) == Err(Errno::SRCH) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Why waiting for a program did not give its exit status.
enum Waited {
    TimedOut,
    /// `stop` said so.
    Stopped,
    Failed(io::Error),
}

/// How `child` exited, waiting at most `limit` and until `stop` says
/// otherwise; then `child` and the rest of its process group are killed.
/// A limit longer than the clock reaches means no limit.
fn wait(
    mut child: process::Child,
    limit: Duration,
    stop: &dyn Fn() -> bool,
) -> Result<ExitStatus, Waited> {
    let deadline = Instant::now().checked_add(limit);
    let passed = || deadline.is_some_and(|deadline| Instant::now() >= deadline);
    let mut pause = Duration::from_millis(1);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if stop() || passed() => {
                end(&mut child);
                return Err(match stop() {
                    true => Waited::Stopped,
                    false => Waited::TimedOut,
                });
            }
            Ok(None) => {
                let left =
                    deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
                std::thread::sleep(left.map_or(pause, |left| pause.min(left)));
                pause = (pause * 2).min(Duration::from_millis(20));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Waited::Failed(error));
            }
        }
    }
}

/// How long a program that is to stop may take to end by itself.
const GRACE: Duration = Duration::from_millis(500);

/// Ends `child` and the rest of its process group. It gets SIGINT first,
/// as from Ctrl-C, and `GRACE` to end by itself: mermaid-cli then closes
/// its browser, which runs in a session of its own, out of the group's
/// reach. Then the group is killed.
fn end(child: &mut process::Child) {
    use rustix::process::{Signal, WaitId, WaitIdOptions, kill_process_group, waitid};

    let group = rustix::process::Pid::from_child(child);
    let _ = kill_process_group(group, Signal::INT);
    let deadline = Instant::now() + GRACE;
    // Left unreaped, so that the group's id cannot go to another process
    // before it is killed.
    let ended = || {
        let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        !matches!(waitid(WaitId::Pid(group), options), Ok(None))
    };
    while !ended() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = kill_process_group(group, Signal::KILL);
    let _ = child.wait();
}

/// How many bytes of what a program says on standard error kiri reads:
/// far more than the first lines it quotes take.
const SAID: u64 = 64 * 1024;

/// The first `limit` bytes of the file at `path`, or as many as it has;
/// none when it cannot be read.
fn read_start(path: &Path, limit: u64) -> Vec<u8> {
    let mut start = Vec::new();
    if let Ok(file) = File::open(path) {
        let _ = file.take(limit).read_to_end(&mut start);
    }
    start
}

/// The first lines of `text` with something on them, at most five, each
/// shortened to a length that fits in a warning. They end where a stack
/// trace starts, as after mmdc's message, its pointer to the mistake and
/// what it expected. Lines after the first keep their indent, so that a
/// pointer still points at its column.
fn first_lines(text: &str) -> Option<String> {
    const MOST: usize = 5;
    const LONGEST: usize = 200;
    let shortened = |line: &str| match line.char_indices().nth(LONGEST) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line.to_owned(),
    };
    let mut lines = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty());
    let mut told = vec![shortened(lines.next()?.trim_start())];
    told.extend(
        lines
            .take_while(|line| !is_stack_frame(line))
            .take(MOST - 1)
            .map(shortened),
    );
    Some(told.join("\n"))
}

/// Whether `line` is a frame of a JavaScript stack trace: one that ends in
/// a place in a file, `…:line:column` or `…:line:column)`, as in
/// `at f (file.js:1:2)` or `Parser.parse (https://…/a.js:1:2)`.
fn is_stack_frame(line: &str) -> bool {
    let place = line.trim_end().trim_end_matches(')');
    let mut parts = place.rsplitn(3, ':');
    let number = |part: Option<&str>| {
        part.is_some_and(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    };
    number(parts.next()) && number(parts.next()) && parts.next().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagrams::{Theme, mermaid};
    use crate::testing::Scratch;

    /// A directory of its own for the test that calls it `name`, with
    /// `temp` in it for the files. Some tests make the directory for the
    /// files in it, whose path must be short enough for the sockets a
    /// browser makes there: so it is in `/tmp`, whatever the length of
    /// `TMPDIR`, and its name is short.
    fn scratch(name: &str) -> Scratch {
        let scratch = Scratch::in_dir(Path::new("/tmp"), &format!("x-{name}"));
        fs::create_dir_all(scratch.join("temp")).unwrap();
        fs::create_dir_all(scratch.join("tmp")).unwrap();
        scratch
    }

    /// The settings for a renderer with its files in `temp` under `dir`,
    /// and `tmp` there as the system's temporary directory.
    fn settings(dir: &Path) -> ExternalSettings {
        ExternalSettings {
            work_dir: Some(dir.join("temp")),
            ..ExternalSettings::new(dir.join("tmp"))
        }
    }

    /// A renderer that runs the shell script `script` for Mermaid, with
    /// the input, output, theme and configuration as its arguments, and
    /// its files in `temp` under `dir`.
    fn script_renderer(dir: &Path, script: &str) -> External {
        script_renderer_with(dir, script, settings(dir))
    }

    /// `script_renderer` with `settings`.
    fn script_renderer_with(dir: &Path, script: &str, settings: ExternalSettings) -> External {
        let path = dir.join("render.sh");
        fs::write(&path, script).unwrap();
        // Through sh, so the test does not run a file it has just
        // written, which can fail with "text file busy" while another
        // test starts a program.
        let argv = [
            "/bin/sh",
            &path.to_string_lossy(),
            "{input}",
            "{output}",
            "{theme}",
            "{config}",
        ];
        let command = Command::new(argv.map(String::from).to_vec());
        External::new(BTreeMap::from([("mermaid".to_owned(), command)]), settings)
    }

    /// What the directory for the files under `dir` has in it.
    fn left(dir: &Path) -> Vec<PathBuf> {
        (fs::read_dir(dir.join("temp")).unwrap())
            .map(|entry| entry.unwrap().path())
            .collect()
    }

    const SOURCE: &str = "flowchart TD\n  A --> B\n";

    #[test]
    fn draws_with_the_command_and_reads_its_svg() {
        let scratch = scratch("draws");
        let renderer = script_renderer(
            &scratch,
            "case \"$1\" in *.mmd) ;; *) exit 3 ;; esac\n\
             printf '<svg theme=\"%s\">%s</svg>' \"$3\" \"$(cat \"$1\")\" > \"$2\"\n",
        );
        assert!(renderer.draws("mermaid") && renderer.draws("Mermaid"));
        assert!(!renderer.draws("dot"));
        for (theme, name) in [(Theme::Light, "default"), (Theme::Dark, "dark")] {
            let drawn = renderer
                .render(&Request::new("Mermaid", SOURCE, theme))
                .unwrap();
            let svg = format!("<svg theme=\"{name}\">flowchart TD\n  A --> B</svg>");
            assert_eq!(drawn, Drawn::svg(svg, theme, None));
        }
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_diagram_that_chooses_its_theme_is_drawn_for_that_page() {
        let scratch = scratch("own-theme");
        let renderer = script_renderer(&scratch, "echo '<svg/>' > \"$2\"\n");
        let forest = format!("%%{{init: {{\"theme\": \"forest\"}}}}%%\n{SOURCE}");
        let drawn = renderer
            .render(&Request::new("mermaid", &forest, Theme::Dark))
            .unwrap();
        assert_eq!(drawn.theme, Theme::Light);
        let drawn = renderer
            .render(&Request::new("mermaid", SOURCE, Theme::Dark))
            .unwrap();
        assert_eq!(drawn.theme, Theme::Dark);
    }

    #[test]
    fn mermaid_gets_its_source_unchanged_and_a_configuration_for_svg_text() {
        let scratch = scratch("labels");
        let written = |script: &str, source: &str| {
            let renderer = script_renderer(&scratch, script);
            renderer
                .render(&Request::new("mermaid", source, Theme::Light))
                .map(|drawn| drawn.text())
        };
        // The source as it is, so that the program's line numbers are the
        // diagram's.
        let front_matter = "\n---\ntitle: T\n---\nflowchart TD\n";
        for source in [SOURCE, front_matter] {
            let source_file = written("cat \"$1\" > \"$2\"\n", source);
            assert_eq!(source_file, Ok(source.to_owned()));
        }
        let config = written("cat \"$4\" > \"$2\"\n", SOURCE);
        assert_eq!(config, Ok(mermaid::config(Output::Svg)));
        let config: serde_json::Value =
            serde_json::from_str(&mermaid::config(Output::Svg)).unwrap();
        assert_eq!(config["htmlLabels"], false);
        for diagram in ["flowchart", "class", "state"] {
            assert_eq!(config[diagram]["htmlLabels"], false, "{diagram}");
        }
        // The look and layout that the built-in renderer gives a diagram
        // that does not choose its own, so that both draw alike.
        assert_eq!(config["look"], "classic");
        assert_eq!(config["layout"], "dagre");
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_command_that_takes_the_scale_draws_a_png_at_it() {
        // It writes the name of its output file, its scale and its
        // configuration.
        let scratch = scratch("scale");
        let path = scratch.join("render.sh");
        fs::write(
            &path,
            "printf '%s %s ' \"${2##*/}\" \"$3\" > \"$2\"\ncat \"$4\" >> \"$2\"\n",
        )
        .unwrap();
        let command = |scale: &str| {
            let argv = [
                "/bin/sh",
                &path.to_string_lossy(),
                "{input}",
                "{output}",
                scale,
                "{config}",
            ];
            let command = Command::new(argv.map(String::from).to_vec());
            External::new(
                BTreeMap::from([("mermaid".to_owned(), command)]),
                settings(&scratch),
            )
        };
        let drawn = command("-s{scale}")
            .render(&Request::new("mermaid", SOURCE, Theme::Light))
            .unwrap();
        let text = drawn.text();
        let (start, config) = text.split_at(text.find('{').unwrap());
        assert_eq!(start, "diagram.png -s3 ");
        assert_eq!(drawn.scale, SCALE);
        // Labels as a browser draws them, in HTML.
        assert_eq!(config, mermaid::config(Output::Raster));
        let config: serde_json::Value = serde_json::from_str(config).unwrap();
        assert_eq!(config.get("htmlLabels"), None);
        assert_eq!(config["flowchart"].get("htmlLabels"), None);
        // A command without it writes SVG, or a raster image at one pixel
        // to the CSS pixel, and gets labels as SVG text.
        let drawn = command("1")
            .render(&Request::new("mermaid", SOURCE, Theme::Light))
            .unwrap();
        let text = drawn.text();
        let (start, config) = text.split_at(text.find('{').unwrap());
        assert_eq!(start, "diagram.svg 1 ");
        assert_eq!(drawn.scale, 1);
        assert_eq!(config, mermaid::config(Output::Svg));
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_command_may_leave_the_configuration_out() {
        let scratch = scratch("no-config");
        let path = scratch.join("render.sh");
        fs::write(&path, "cat \"$1\" > \"$2\"\n").unwrap();
        let argv = ["/bin/sh", &path.to_string_lossy(), "{input}", "{output}"];
        let command = Command::new(argv.map(String::from).to_vec());
        let renderer = External::new(
            BTreeMap::from([("dot".to_owned(), command)]),
            settings(&scratch),
        );
        let svg = renderer.render(&Request::new("dot", "digraph {}", Theme::Light));
        assert_eq!(svg.map(|drawn| drawn.text()), Ok("digraph {}".to_owned()));
    }

    #[test]
    fn a_command_that_fails_tells_why_in_its_first_lines() {
        let scratch = scratch("fails");
        // As mmdc tells a parse error: the message, the line with the
        // mistake, a pointer to it and what was expected, then the
        // JavaScript stack.
        let renderer = script_renderer(
            &scratch,
            "echo > \"$2\"\n\
             printf '\\n  Error: Parse error on line 2:\\n...A[Unclosed\\n\
             -----^\\n\\nExpecting SQE, got EOF\\n\
             Parser.parseError (https://x.invalid/a.js:1:2)\\n\
             \\x20   at #evaluate (file:///x/b.js:3:4)\\n' >&2\n\
             exit 1\n",
        );
        assert_eq!(
            renderer.render(&Request::new("mermaid", SOURCE, Theme::Light)),
            Err("sh exited with status 1: Error: Parse error on line 2:\n\
                 ...A[Unclosed\n\
                 -----^\n\
                 Expecting SQE, got EOF"
                .to_owned())
        );
        let renderer = script_renderer(&scratch, "exit 2\n");
        assert_eq!(
            renderer.render(&Request::new("mermaid", SOURCE, Theme::Light)),
            Err("sh exited with status 2".to_owned())
        );
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn an_image_larger_than_the_limit_is_not_read() {
        let scratch = scratch("too-large");
        let limited = |script: &str| {
            let settings = ExternalSettings {
                max_size: 100,
                ..settings(&scratch)
            };
            script_renderer_with(&scratch, script, settings)
        };
        let renderer = limited("printf '%0100d' 0 > \"$2\"\n");
        let drawn = renderer.render(&Request::new("mermaid", SOURCE, Theme::Light));
        assert_eq!(drawn.map(|drawn| drawn.image.len()), Ok(100));
        let renderer = limited("printf '%0101d' 0 > \"$2\"\n");
        assert_eq!(
            renderer.render(&Request::new("mermaid", SOURCE, Theme::Light)),
            Err("sh wrote an image larger than the 100-byte limit".to_owned())
        );
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn what_a_command_says_is_read_only_as_far_as_its_first_lines() {
        // Past a megabyte, its first lines still tell why.
        let scratch = scratch("says-much");
        let renderer = script_renderer(
            &scratch,
            "echo 'Error: it failed' >&2\nyes 'more' | head -c 2000000 >&2\nexit 1\n",
        );
        assert_eq!(
            renderer.render(&Request::new("mermaid", SOURCE, Theme::Light)),
            Err("sh exited with status 1: Error: it failed\nmore\nmore\nmore\nmore".to_owned())
        );
    }

    #[test]
    fn a_command_that_writes_nothing_fails() {
        let scratch = scratch("writes-nothing");
        let renderer = script_renderer(&scratch, "exit 0\n");
        assert_eq!(
            renderer.render(&Request::new("mermaid", SOURCE, Theme::Light)),
            Err("sh wrote no output file".to_owned())
        );
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_command_may_write_a_raster_image() {
        // The image loader tells what it is by its content.
        let scratch = scratch("raster");
        let renderer = script_renderer(&scratch, "printf '\\211PNG\\r\\n\\032\\n\\377' > \"$2\"\n");
        let drawn = renderer
            .render(&Request::new("mermaid", SOURCE, Theme::Light))
            .unwrap();
        assert_eq!(drawn.image, b"\x89PNG\r\n\x1a\n\xff");
        assert_eq!(drawn.scale, 1);
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_program_that_is_not_there_fails() {
        let scratch = scratch("missing");
        let command = Command::new(vec![
            "/no/such/mmdc".into(),
            "{input}".into(),
            "{output}".into(),
        ]);
        let renderer = External::new(
            BTreeMap::from([("mermaid".to_owned(), command)]),
            settings(&scratch),
        );
        let error = renderer
            .render(&Request::new("mermaid", SOURCE, Theme::Light))
            .unwrap_err();
        assert!(error.starts_with("/no/such/mmdc: "), "{error}");
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_command_that_takes_too_long_is_stopped_with_what_it_started() {
        let scratch = scratch("too-long");
        let started = scratch.join("started");
        let script = format!(
            "sleep 30 &\necho $! > '{}'\nwait\n",
            started.to_string_lossy()
        );
        // The limit starts with sh, so on a busy machine sh may not get as
        // far as the sleep in time. Then it gets a longer limit.
        let mut limit = Duration::from_millis(300);
        let pid = loop {
            let settings = ExternalSettings {
                time_limit: limit,
                ..settings(&scratch)
            };
            let renderer = script_renderer_with(&scratch, &script, settings);
            let start = Instant::now();
            assert_eq!(
                renderer.render(&Request::new("mermaid", SOURCE, Theme::Light)),
                Err(format!("sh did not finish in {} s", limit.as_secs_f32()))
            );
            assert!(
                start.elapsed() < limit + Duration::from_secs(5),
                "{:?}",
                start.elapsed()
            );
            assert_eq!(left(&scratch), Vec::<PathBuf>::new());
            match fs::read_to_string(&started) {
                Ok(pid) if pid.ends_with('\n') => break pid,
                _ if limit < Duration::from_secs(10) => limit *= 2,
                read => panic!("sh did not start the sleep: {read:?}"),
            }
        };
        // The sleep the script started is gone too, or a zombie that is
        // being reaped.
        let stat = Path::new("/proc").join(pid.trim()).join("stat");
        let gone = (0..100).any(|_| {
            let alive = fs::read_to_string(&stat).is_ok_and(|stat| {
                stat.rsplit_once(") ")
                    .is_some_and(|(_, s)| !s.starts_with('Z'))
            });
            if alive {
                std::thread::sleep(Duration::from_millis(20));
            }
            !alive
        });
        assert!(gone, "sleep {pid} still runs");
    }

    #[test]
    fn a_time_limit_too_long_for_the_clock_means_no_limit() {
        // The configuration file takes any number of seconds that a
        // Duration holds, more than an Instant can be moved by.
        let scratch = scratch("long-limit");
        let settings = ExternalSettings {
            time_limit: Duration::from_secs_f64(1e19),
            ..settings(&scratch)
        };
        let renderer = script_renderer_with(&scratch, "echo '<svg/>' > \"$2\"\n", settings);
        let drawn = renderer
            .render(&Request::new("mermaid", SOURCE, Theme::Light))
            .unwrap();
        assert_eq!(drawn.theme, Theme::Light);
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_program_that_is_stopped_may_clean_up_first() {
        // mermaid-cli closes its browser on SIGINT, which runs in a session
        // of its own that killing mmdc's process group does not reach.
        let scratch = scratch("clean-up");
        let cleaned = scratch.join("cleaned");
        let ready = scratch.join("ready");
        let script = format!(
            "trap \"echo yes > '{}'; exit 130\" INT\nsleep 30 &\ntouch '{}'\nwait\n",
            cleaned.to_string_lossy(),
            ready.to_string_lossy()
        );
        let stop = Arc::new(AtomicBool::new(false));
        let settings = ExternalSettings {
            stop: Some(Arc::clone(&stop)),
            ..settings(&scratch)
        };
        let renderer = script_renderer_with(&scratch, &script, settings);
        let start = Instant::now();
        // Stopped once the trap is set, however long sh takes to start.
        let drawn = std::thread::scope(|scope| {
            let drawing =
                scope.spawn(|| renderer.render(&Request::new("mermaid", SOURCE, Theme::Light)));
            while !ready.exists() && start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(10));
            }
            stop.store(true, Ordering::SeqCst);
            drawing.join().unwrap()
        });
        assert_eq!(drawn, Err("sh was stopped: interrupted".to_owned()));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(fs::read_to_string(&cleaned).ok().as_deref(), Some("yes\n"));
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_stop_kills_the_program_with_what_it_started_and_starts_no_more() {
        let scratch = scratch("stop");
        let started = scratch.join("started");
        let script = format!(
            "sleep 30 &\necho $! > '{}'\nwait\n",
            started.to_string_lossy()
        );
        let stop = Arc::new(AtomicBool::new(false));
        let settings = ExternalSettings {
            stop: Some(Arc::clone(&stop)),
            ..settings(&scratch)
        };
        let renderer = script_renderer_with(&scratch, &script, settings);
        let start = Instant::now();
        let drawn = std::thread::scope(|scope| {
            let drawing =
                scope.spawn(|| renderer.render(&Request::new("mermaid", SOURCE, Theme::Light)));
            while !started.exists() && start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(10));
            }
            stop.store(true, Ordering::SeqCst);
            drawing.join().unwrap()
        });
        assert_eq!(drawn, Err("sh was stopped: interrupted".to_owned()));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
        let pid = fs::read_to_string(&started).unwrap();
        let stat = Path::new("/proc").join(pid.trim()).join("stat");
        let gone = (0..100).any(|_| {
            let alive = fs::read_to_string(&stat).is_ok_and(|stat| {
                stat.rsplit_once(") ")
                    .is_some_and(|(_, s)| !s.starts_with('Z'))
            });
            if alive {
                std::thread::sleep(Duration::from_millis(20));
            }
            !alive
        });
        assert!(gone, "sleep {pid} still runs");
        // Once stopped, nothing more starts.
        fs::remove_file(&started).unwrap();
        assert_eq!(
            renderer.render(&Request::new("mermaid", SOURCE, Theme::Light)),
            Err("sh was not started: interrupted".to_owned())
        );
        assert!(!started.exists());
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn the_placeholders_may_be_inside_arguments() {
        let scratch = scratch("inside");
        let path = scratch.join("render.sh");
        fs::write(
            &path,
            "for arg; do case $arg in --out=*) out=${arg#--out=} ;; esac; done\n\
             printf '%s ' \"$@\" > \"$out\"\n",
        )
        .unwrap();
        let argv = [
            "/bin/sh",
            &path.to_string_lossy(),
            "--theme={theme}",
            "--out={output}",
            "{input}",
        ];
        let command = Command::new(argv.map(String::from).to_vec());
        let renderer = External::new(
            BTreeMap::from([("mermaid".to_owned(), command)]),
            settings(&scratch),
        );
        let svg = renderer
            .render(&Request::new("mermaid", SOURCE, Theme::Dark))
            .unwrap()
            .text();
        let dir = scratch.join("temp").join(format!("{}-0", process::id()));
        let dir = dir.to_string_lossy();
        assert_eq!(
            svg,
            format!("--theme=dark --out={dir}/diagram.svg {dir}/diagram.mmd ")
        );
    }

    /// A program that makes a directory in its temporary directory, as
    /// puppeteer does for a browser's profile, and writes the path of its
    /// temporary directory as its SVG.
    const MAKES_A_PROFILE: &str = "mkdir \"$TMPDIR/profile\"\nprintf '%s' \"$TMPDIR\" > \"$2\"\n";

    #[test]
    fn the_program_s_temporary_files_are_in_its_directory_and_go_with_it() {
        let scratch = scratch("tmpdir");
        let renderer = script_renderer(&scratch, MAKES_A_PROFILE);
        let tmpdir = renderer
            .render(&Request::new("mermaid", SOURCE, Theme::Light))
            .unwrap()
            .text();
        let expected = scratch.join("temp").join(format!("{}-0", process::id()));
        assert_eq!(Path::new(&tmpdir), expected);
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
    }

    #[test]
    fn the_directory_for_the_files_is_made_for_the_user_alone() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = scratch("made");
        let work = scratch.join("c/kiri/diagrams");
        let settings = ExternalSettings {
            work_dir: Some(work.clone()),
            ..settings(&scratch)
        };
        let renderer = script_renderer_with(&scratch, MAKES_A_PROFILE, settings);
        let tmpdir = renderer
            .render(&Request::new("mermaid", SOURCE, Theme::Light))
            .unwrap()
            .text();
        assert_eq!(Path::new(&tmpdir).parent(), Some(work.as_path()));
        for dir in [scratch.join("c"), scratch.join("c/kiri"), work.clone()] {
            let mode = fs::metadata(&dir).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "{}", dir.display());
        }
        assert_eq!(fs::read_dir(&work).unwrap().count(), 0);
    }

    /// A process id that no process has now.
    fn ended_process() -> u32 {
        let mut child = process::Command::new("true").spawn().unwrap();
        let ended = child.id();
        child.wait().unwrap();
        ended
    }

    #[test]
    fn directories_left_by_a_kiri_that_has_ended_are_removed() {
        let scratch = scratch("stale");
        let work = scratch.join("temp");
        let ended = ended_process();
        let alive = process::id();
        let stale = [format!("{ended}-0"), format!("{ended}-17")];
        let kept = [
            format!("{alive}-90"),
            "1-0".to_owned(),
            format!("{ended}"),
            format!("{ended}-"),
            format!("{ended}-x"),
            format!("0{ended}-0"),
            format!("kiri-diagram-{ended}-0"),
            "notes".to_owned(),
        ];
        for name in stale.iter().chain(&kept) {
            fs::create_dir_all(work.join(name).join("profile")).unwrap();
            fs::write(
                work.join(name).join(HOST_FILE),
                format!("{}\n", host_name()),
            )
            .unwrap();
        }
        // A file with such a name is not a directory of kiri's.
        let file = format!("{ended}-1");
        fs::write(work.join(&file), "").unwrap();
        let renderer = script_renderer(&scratch, "echo '<svg/>' > \"$2\"\n");
        renderer
            .render(&Request::new("mermaid", SOURCE, Theme::Light))
            .unwrap();
        let mut left: Vec<String> = (left(&scratch).iter())
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        left.sort();
        let mut expected: Vec<String> = kept.into_iter().chain([file]).collect();
        expected.sort();
        assert_eq!(left, expected);
    }

    #[test]
    fn directories_of_other_machines_are_left_alone() {
        // A cache directory that machines or containers share: a process
        // id tells nothing about another machine's processes.
        let scratch = scratch("hosts");
        let work = scratch.join("temp");
        let ended = ended_process();
        let dir = |name: &str, host: Option<&str>| {
            fs::create_dir_all(work.join(name)).unwrap();
            if let Some(host) = host {
                fs::write(work.join(name).join(HOST_FILE), format!("{host}\n")).unwrap();
            }
        };
        dir(&format!("{ended}-0"), Some("this"));
        dir(&format!("{ended}-1"), Some("other"));
        dir(&format!("{ended}-2"), Some(""));
        // One that names no machine: not one that kiri made.
        dir(&format!("{ended}-3"), None);
        // One that a kiri made and was killed before it got its name.
        dir(&format!("{ended}-4.new"), Some("this"));
        dir(&format!("{ended}-5.new"), Some("other"));
        // One that a kiri was killed while making, before the file was in
        // it, or that a kiri on another machine is making now: left, empty.
        dir(&format!("{ended}-6.new"), None);
        remove_stale(&work, "this");
        let mut left: Vec<String> = (left(&scratch).iter())
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        left.sort();
        let expected = [1, 2, 3].map(|n| format!("{ended}-{n}"));
        let mut expected = expected.to_vec();
        expected.extend([5, 6].map(|n| format!("{ended}-{n}.new")));
        assert_eq!(left, expected);
    }

    #[test]
    fn each_directory_names_the_machine_it_was_made_on() {
        let scratch = scratch("host-file");
        let renderer = script_renderer(&scratch, "cat \"$TMPDIR/host\" > \"$2\"\n");
        let drawn = renderer
            .render(&Request::new("mermaid", SOURCE, Theme::Light))
            .unwrap();
        assert_eq!(drawn.text(), format!("{}\n", host_name()));
        assert!(!host_name().is_empty());
    }

    #[test]
    fn without_a_directory_of_its_own_the_files_go_in_the_temporary_one() {
        let scratch = scratch("fallback");
        fs::write(scratch.join("file"), "").unwrap();
        // One that cannot be made, and one whose path would be too long for
        // the sockets a browser makes in it.
        let long = scratch.join("x".repeat(LONGEST_WORK_DIR));
        for work in [scratch.join("file/diagrams"), long.clone()] {
            let settings = ExternalSettings {
                work_dir: Some(work.clone()),
                ..settings(&scratch)
            };
            let renderer = script_renderer_with(&scratch, MAKES_A_PROFILE, settings);
            let tmpdir = renderer
                .render(&Request::new("mermaid", SOURCE, Theme::Light))
                .unwrap()
                .text();
            let tmpdir = Path::new(&tmpdir);
            assert_eq!(tmpdir.parent(), Some(scratch.join("tmp").as_path()));
            let name = tmpdir.file_name().unwrap().to_string_lossy();
            assert!(name.starts_with("kiri-diagram-"), "{name}");
            assert!(!tmpdir.exists());
        }
        assert!(!long.exists());
    }

    #[test]
    fn tells_why_a_directory_cannot_hold_the_files() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = scratch("unusable");
        let file = scratch.join("file");
        fs::write(&file, "").unwrap();
        let locked = scratch.join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
        let work = scratch.join("temp/kiri/diagrams");
        assert_eq!(not_writable(&work), None);
        assert_eq!(
            not_writable(&file.join("kiri/diagrams")),
            Some(Unusable::NotADirectory(file.clone()))
        );
        let long = scratch.join("x".repeat(LONGEST_WORK_DIR));
        assert_eq!(unusable(&long), Some(Unusable::TooLong));
        // Unless the tests run as root, who may write anywhere.
        if rustix::fs::access(&locked, rustix::fs::Access::WRITE_OK).is_err() {
            assert_eq!(
                not_writable(&locked.join("kiri")),
                Some(Unusable::NotWritable(locked.clone()))
            );
        }
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn the_command_runs_with_its_own_path() {
        let scratch = scratch("path");
        let path = scratch.join("render.sh");
        fs::write(&path, "printf '%s' \"$PATH\" > \"$2\"\n").unwrap();
        let argv = ["/bin/sh", &path.to_string_lossy(), "{input}", "{output}"];
        let command = Command {
            argv: argv.map(String::from).to_vec(),
            path: Some("/opt/node/bin:/usr/bin".into()),
        };
        let renderer = External::new(
            BTreeMap::from([("mermaid".to_owned(), command)]),
            settings(&scratch),
        );
        let svg = renderer
            .render(&Request::new("mermaid", SOURCE, Theme::Dark))
            .unwrap()
            .text();
        assert_eq!(svg, "/opt/node/bin:/usr/bin");
    }

    /// The start of a PNG file of `width` × `height` pixels, as far as its
    /// header, which is all that tells its size.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(width.to_be_bytes());
        png.extend(height.to_be_bytes());
        png.extend([8, 6, 0, 0, 0]);
        png
    }

    /// What a command that takes `{scale}` draws of a diagram as large as
    /// `sizes` gives for each scale, and the scales it was asked for.
    fn drawn_at_scales(name: &str, sizes: &[(u32, Vec<u8>)]) -> (Result<Drawn, String>, String) {
        let scratch = scratch(&format!("scales-{name}"));
        let calls = scratch.join("calls");
        let mut script = format!("echo \"$3\" >> '{}'\ncase \"$3\" in\n", calls.display());
        for (scale, image) in sizes {
            let octal: String = image.iter().map(|byte| format!("\\{byte:03o}")).collect();
            script.push_str(&format!("  {scale}) printf '{octal}' > \"$2\" ;;\n"));
        }
        script.push_str("  *) exit 9 ;;\nesac\n");
        let path = scratch.join("render.sh");
        fs::write(&path, script).unwrap();
        let argv = [
            "/bin/sh",
            &path.to_string_lossy(),
            "{input}",
            "{output}",
            "{scale}",
        ];
        let command = Command::new(argv.map(String::from).to_vec());
        let renderer = External::new(
            BTreeMap::from([("mermaid".to_owned(), command)]),
            settings(&scratch),
        );
        let drawn = renderer.render(&Request::new("mermaid", SOURCE, Theme::Light));
        let calls = fs::read_to_string(&calls).unwrap_or_default();
        assert_eq!(left(&scratch), Vec::<PathBuf>::new());
        (
            drawn,
            calls.split_whitespace().collect::<Vec<_>>().join(" "),
        )
    }

    #[test]
    fn a_png_too_large_to_load_is_drawn_again_at_a_scale_that_fits() {
        // 64 megapixels at most: 9000 × 9000 at three pixels to the CSS
        // pixel is too many, 6000 × 6000 at two is not.
        let large = png_header(9000, 9000);
        let smaller = png_header(6000, 6000);
        let sizes = [(3, large.clone()), (2, smaller.clone())];
        let (drawn, calls) = drawn_at_scales("smaller", &sizes);
        let drawn = drawn.unwrap();
        assert_eq!((&drawn.image, drawn.scale), (&smaller, 2));
        assert_eq!(calls, "3 2");
        // One that fits is drawn once.
        let (drawn, calls) = drawn_at_scales("fits", &[(3, smaller.clone())]);
        assert_eq!(drawn.unwrap().scale, 3);
        assert_eq!(calls, "3");
        // Only once more: one still too large is left for the image loader
        // to refuse, which tells its size.
        let sizes = [(3, large.clone()), (2, large.clone())];
        let (drawn, calls) = drawn_at_scales("still-large", &sizes);
        let drawn = drawn.unwrap();
        assert_eq!((drawn.image, drawn.scale), (large, 2));
        assert_eq!(calls, "3 2");
        // One that would not fit even at one pixel to the CSS pixel is not
        // drawn again.
        let huge = png_header(30000, 30000);
        let (drawn, calls) = drawn_at_scales("huge", &[(3, huge.clone())]);
        assert_eq!(drawn.unwrap().image, huge);
        assert_eq!(calls, "3");
        // Nor is what is no PNG, whose size kiri cannot tell before
        // decoding it.
        let (drawn, calls) = drawn_at_scales("not-png", &[(3, b"GIF89a".to_vec())]);
        assert_eq!(drawn.unwrap().scale, 3);
        assert_eq!(calls, "3");
        // A program that fails the second time fails.
        let (drawn, calls) = drawn_at_scales("fails", &[(3, png_header(9000, 9000))]);
        assert_eq!(drawn, Err("sh exited with status 9".to_owned()));
        assert_eq!(calls, "3 2");
    }

    #[test]
    fn a_long_line_is_shortened() {
        let line = "x".repeat(300);
        assert_eq!(first_lines(&line), Some(format!("{}…", "x".repeat(200))));
        assert_eq!(first_lines(" \n\n"), None);
    }

    #[test]
    fn a_stack_frame_ends_with_its_place_in_a_file() {
        for frame in [
            "Parser.parseError (https://x.invalid/a.js:1:2)",
            "    at #evaluate (file:///x/b.js:3:4)",
            "Parser.parse (/usr/lib/x.js:10:20)",
            "    at async run (node:internal/main:5:7)",
            "    at /usr/lib/x.js:1:2",
        ] {
            assert!(is_stack_frame(frame), "{frame}");
        }
        // Lines of a message that look a little like one.
        for line in [
            "at least one node is needed",
            "at position 5",
            "...B(Docs http://x.org)",
            "Expecting 'SQE', got 'PE' (line 3)",
        ] {
            assert!(!is_stack_frame(line), "{line}");
        }
    }

    #[test]
    fn at_most_five_lines_tell_why() {
        let said = "\n one\ntwo\n\n  three\nfour\nfive\nsix\n";
        assert_eq!(
            first_lines(said),
            Some("one\ntwo\n  three\nfour\nfive".to_owned())
        );
    }
}
