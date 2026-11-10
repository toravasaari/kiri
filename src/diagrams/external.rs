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

use super::{DiagramError, Drawn, Renderer, Request};
use crate::typeset::diagrams::output::Output;

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
/// Chromium, which `mmdc` runs, may make a socket in that, and a socket's
/// path longer than [`LONGEST_SOCKET_PATH`] would fail.
pub const LONGEST_WORK_DIR: usize = LONGEST_SOCKET_PATH
    - "/.org.chromium.Chromium.XXXXXX/SingletonSocket".len()
    - "/4194304-999".len();

/// The longest path a socket can have: its address holds 108 bytes on
/// Linux and 104 on macOS, the zero that ends the path included.
const LONGEST_SOCKET_PATH: usize = if cfg!(target_os = "macos") { 103 } else { 107 };

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

    /// Whether the program drawing `request` is to stop: all of them
    /// are, or this one alone.
    fn stopped(&self, request: &Request) -> bool {
        let stop = |stop: &AtomicBool| stop.load(Ordering::SeqCst);
        self.settings.stop.as_deref().is_some_and(stop) || request.stop.is_some_and(stop)
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
    ) -> Result<Vec<u8>, DiagramError> {
        let program = command.program();
        if self.stopped(request) {
            return Err(DiagramError::NotStarted { program });
        }
        let dir = self.temp_dir().map_err(|error| DiagramError::NoDirectory {
            program: program.clone(),
            error: error.to_string(),
        })?;
        let (files, errors) =
            Files::write(&dir.0, request, writes).map_err(|error| DiagramError::NoFiles {
                program: program.clone(),
                error: error.to_string(),
            })?;
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
        let child = child.spawn().map_err(|error| DiagramError::CannotStart {
            program: argv[0].clone(),
            error: error.to_string(),
        })?;
        self.finish(child, &program, &files, request)?;
        self.read_output(&program, &files.output)
    }

    /// Waits for `child`, the program `program` that draws into `files`
    /// what `request` asks for, to end, until the time limit or a stop;
    /// why it failed, as the first lines it wrote on standard error tell,
    /// when it did.
    fn finish(
        &self,
        child: process::Child,
        program: &str,
        files: &Files,
        request: &Request,
    ) -> Result<(), DiagramError> {
        let limit = self.settings.time_limit;
        let stop = || self.stopped(request);
        let named = program.to_owned();
        let status = wait(child, limit, &stop).map_err(|error| match error {
            Waited::TimedOut => DiagramError::TimedOut {
                program: named,
                limit,
            },
            Waited::Stopped => DiagramError::Interrupted { program: named },
            Waited::Failed(error) => DiagramError::WaitFailed {
                program: named,
                error: error.to_string(),
            },
        })?;
        if status.success() {
            return Ok(());
        }
        let said = read_start(&files.errors, SAID);
        let said = first_lines(&String::from_utf8_lossy(&said));
        Err(match status.code() {
            Some(code) => DiagramError::Exited {
                program: program.to_owned(),
                code,
                said,
            },
            None => DiagramError::Signalled {
                program: program.to_owned(),
                said,
            },
        })
    }

    /// The image file at `output` that `program` wrote, up to the size
    /// limit.
    fn read_output(&self, program: &str, output: &Path) -> Result<Vec<u8>, DiagramError> {
        let program = program.to_owned();
        let limit = self.settings.max_size;
        match File::open(output).and_then(|file| crate::images::read_at_most(file, limit)) {
            Ok(Some(bytes)) => Ok(bytes),
            Ok(None) => Err(DiagramError::TooLarge {
                program,
                limit: crate::images::size(limit),
            }),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Err(DiagramError::NoOutput { program })
            }
            Err(error) => Err(DiagramError::Unreadable {
                program,
                error: error.to_string(),
            }),
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

    fn render(&self, request: &Request) -> Result<Drawn, DiagramError> {
        let Some(command) = self.command(request.language) else {
            return Err(DiagramError::NoCommand(request.language.to_owned()));
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
mod tests;
