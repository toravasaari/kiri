//! Diagrams drawn by a program outside kiri, such as mermaid-cli's `mmdc`.
//!
//! kiri writes the diagram's source to a file in a directory of its own,
//! and for Mermaid the program's settings beside it. It runs the command
//! that kiri's configuration file gives, or that kiri found, without a
//! shell and with that directory as its temporary directory, and reads
//! the image the program wrote, SVG or raster. The directory goes, with whatever the
//! program left in it, when the program has finished, failed, run out of
//! time or been stopped; one that a kiri which was killed left goes the
//! next time a kiri draws there.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, DirBuilder, File};
use std::io::{self, ErrorKind};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use super::{Drawn, Renderer, Theme};

/// How long a program may take to draw one diagram before kiri stops it,
/// unless the configuration file says otherwise.
pub const TIME_LIMIT: Duration = Duration::from_secs(20);

/// How many programs draw diagrams at once.
pub const AT_ONCE: usize = 4;

/// How many pixels to the CSS pixel a command draws a raster image with
/// when it takes the scale as `{scale}`.
pub const SCALE: u32 = 3;

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
/// [`External::with_work_dir`] uses it, without making it; `None` when it
/// can, as far as can be told before trying.
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
/// that a program which takes it draws a raster image with. `{output}`
/// ends in `.png` for such a program and in `.svg` for others.
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

/// Draws the diagrams of some languages with external commands.
#[derive(Debug)]
pub struct External {
    /// The command for each language, by its lower-case name.
    commands: BTreeMap<String, Command>,
    time_limit: Duration,
    /// Where the directories for the files go, unless it cannot be made
    /// or its path is too long; then they go in `temp`.
    work: Option<PathBuf>,
    /// The system's temporary directory.
    temp: PathBuf,
    /// How many directories this renderer has made, to name the next one.
    made: AtomicUsize,
    /// Run once the work directory is there, to remove what an earlier
    /// kiri left in it.
    swept: Once,
    /// Set when the programs are to stop, such as on Ctrl-C.
    stop: Option<Arc<AtomicBool>>,
}

impl External {
    /// Draws each language in `commands` with its command.
    pub fn new(commands: BTreeMap<String, Command>) -> Self {
        External {
            commands: (commands.into_iter())
                .map(|(language, command)| (language.to_ascii_lowercase(), command))
                .collect(),
            time_limit: TIME_LIMIT,
            work: None,
            temp: crate::config::temp_directory(&crate::config::Environment {
                tmpdir: std::env::var_os("TMPDIR"),
                ..Default::default()
            }),
            made: AtomicUsize::new(0),
            swept: Once::new(),
            stop: None,
        }
    }

    /// This renderer stopping its programs, and starting no more, once
    /// `stop` is set.
    pub fn stopped_by(self, stop: Arc<AtomicBool>) -> Self {
        External {
            stop: Some(stop),
            ..self
        }
    }

    /// Whether the programs are to stop.
    fn stopped(&self) -> bool {
        (self.stop.as_ref()).is_some_and(|stop| stop.load(Ordering::SeqCst))
    }

    /// This renderer stopping each program after `limit`.
    pub fn with_time_limit(self, limit: Duration) -> Self {
        External {
            time_limit: limit,
            ..self
        }
    }

    /// This renderer with its files in directories under `dir`, which is
    /// made when it is needed, for the user alone, such as `diagrams` in
    /// kiri's cache directory. Without it, or when it cannot be made or
    /// its path is too long, they go in the system's temporary directory.
    pub fn with_work_dir(self, dir: PathBuf) -> Self {
        External {
            work: Some(dir),
            ..self
        }
    }

    /// This renderer with `dir` as the system's temporary directory,
    /// such as `config::temp_directory` gives; by default, that of this
    /// process's `TMPDIR`.
    pub fn with_temp_dir(self, dir: PathBuf) -> Self {
        External { temp: dir, ..self }
    }

    /// Whether no language has a command.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
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
        let work = (self.work.as_deref())
            .filter(|work| work.as_os_str().len() <= LONGEST_WORK_DIR)
            .and_then(|work| {
                let made = DirBuilder::new().recursive(true).mode(0o700).create(work);
                made.ok()?;
                self.swept.call_once(|| remove_stale(work));
                self.new_dir(work, "").ok()
            });
        match work {
            Some(dir) => Ok(dir),
            None => self.new_dir(&self.temp, "kiri-diagram-"),
        }
    }

    /// A new directory in `parent`, its name `prefix` followed by kiri's
    /// process id and a number.
    fn new_dir(&self, parent: &Path, prefix: &str) -> io::Result<TempDir> {
        loop {
            let n = self.made.fetch_add(1, Ordering::Relaxed);
            let dir = parent.join(format!("{prefix}{}-{n}", process::id()));
            match DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => return Ok(TempDir(dir)),
                // Left over from an earlier kiri with the same process id.
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    /// The image file that `command` draws of `source`, in `language`, in
    /// `theme`.
    fn run(
        &self,
        command: &Command,
        language: &str,
        source: &str,
        theme: Theme,
    ) -> Result<Vec<u8>, String> {
        let program = command.program();
        if self.stopped() {
            return Err(format!("{program} was not started: interrupted"));
        }
        let dir = self
            .temp_dir()
            .map_err(|error| format!("cannot make a directory for {program}: {error}"))?;
        let input = dir
            .0
            .join(format!("diagram.{}", super::extension(language)));
        let writes = command.writes();
        let output = dir.0.join(match writes {
            Output::Svg => "diagram.svg",
            Output::Raster => "diagram.png",
        });
        let errors = dir.0.join("errors.txt");
        let config = config(language, writes).map(|(text, extension)| {
            let path = dir.0.join(format!("config.{extension}"));
            (path, text)
        });
        let written = (fs::write(&input, source))
            .and_then(|()| match &config {
                Some((path, text)) => fs::write(path, text),
                None => Ok(()),
            })
            .and_then(|()| File::create(&errors));
        let errors_file =
            written.map_err(|error| format!("cannot write the files for {program}: {error}"))?;
        let theme = super::theme_name(language, theme);
        let argv: Vec<String> = (command.argv.iter())
            .map(|arg| {
                let arg = (arg.replace("{input}", &input.to_string_lossy()))
                    .replace("{output}", &output.to_string_lossy())
                    .replace("{theme}", theme)
                    .replace("{scale}", &SCALE.to_string());
                match &config {
                    Some((path, _)) => arg.replace("{config}", &path.to_string_lossy()),
                    None => arg,
                }
            })
            .collect();
        let mut child = process::Command::new(&argv[0]);
        child
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(errors_file)
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
        let stop = || self.stopped();
        let status = wait(child, self.time_limit, &stop).map_err(|error| match error {
            Waited::TimedOut => format!(
                "{program} did not finish in {} s",
                self.time_limit.as_secs_f32()
            ),
            Waited::Stopped => format!("{program} was stopped: interrupted"),
            Waited::Failed(error) => format!("{program}: {error}"),
        })?;
        if !status.success() {
            let said = fs::read_to_string(&errors).unwrap_or_default();
            let ended = match status.code() {
                Some(code) => format!("{program} exited with status {code}"),
                None => format!("{program} was stopped by a signal"),
            };
            return Err(match first_line(&said) {
                Some(line) => format!("{ended}: {line}"),
                None => ended,
            });
        }
        match fs::read(&output) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Err(format!("{program} wrote no output file"))
            }
            Err(error) => Err(format!("cannot read what {program} wrote: {error}")),
        }
    }
}

impl Renderer for External {
    fn draws(&self, language: &str) -> bool {
        self.command(language).is_some()
    }

    fn render(&self, language: &str, source: &str, theme: Theme) -> Result<Drawn, String> {
        let Some(command) = self.command(language) else {
            return Err(format!("{language} diagrams have no command"));
        };
        let image = self.run(command, language, source, theme)?;
        // The program cannot tell which theme the diagram chose itself.
        let theme = super::chosen_theme(language, source).unwrap_or(theme);
        let scale = match command.writes() {
            Output::Svg => 1,
            Output::Raster => SCALE,
        };
        Ok(Drawn {
            image,
            scale,
            theme,
            background: None,
        })
    }
}

/// The kinds of Mermaid diagram whose configuration has `useMaxWidth`,
/// as Mermaid 12 names them.
const MERMAID_DIAGRAMS: &[&str] = &[
    "flowchart",
    "swimlane",
    "agentflow",
    "sequence",
    "gantt",
    "journey",
    "timeline",
    "class",
    "state",
    "er",
    "pie",
    "quadrantChart",
    "xyChart",
    "requirement",
    "architecture",
    "mindmap",
    "ishikawa",
    "kanban",
    "gitGraph",
    "c4",
    "sankey",
    "packet",
    "block",
    "eventmodeling",
    "treeView",
    "radar",
    "usecase",
    "venn",
    "wardley-beta",
    "cynefin",
    "railroad",
];

/// The Mermaid configuration that a command gets as `{config}`, which a
/// diagram's own directive or front matter overrides, for a command that
/// writes `output`.
///
/// It has each diagram drawn at its own width, rather than at most as wide
/// as the page it is drawn on, which in mmdc's browser is 800 pixels: kiri
/// shrinks a diagram to the window itself. It has the look and layout that
/// the built-in renderer gives too, so that both draw a diagram alike.
///
/// For SVG, it has the labels drawn as SVG text, which kiri can draw,
/// rather than as HTML, which it cannot: Mermaid 11 and later read the
/// first `htmlLabels`, Mermaid 10 the others. A raster image is a
/// browser's drawing, which has the labels as Mermaid draws them.
fn mermaid_config(output: Output) -> String {
    let mut config = serde_json::json!({});
    for diagram in MERMAID_DIAGRAMS {
        config[diagram] = serde_json::json!({"useMaxWidth": false});
    }
    if output == Output::Svg {
        config["htmlLabels"] = false.into();
        for diagram in ["flowchart", "class", "state"] {
            config[diagram]["htmlLabels"] = false.into();
        }
    }
    for (key, value) in super::MERMAID_SITE {
        config[key] = value.into();
    }
    format!("{config}\n")
}

/// The configuration a command for `language` that writes `output` gets
/// as `{config}`, and the extension of its file.
fn config(language: &str, output: Output) -> Option<(String, &'static str)> {
    (language.eq_ignore_ascii_case("mermaid")).then(|| (mermaid_config(output), "json"))
}

/// What a command writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Output {
    /// SVG, or a raster image at one pixel to the CSS pixel.
    Svg,
    /// A raster image at [`SCALE`], when the command takes `{scale}`.
    Raster,
}

/// A directory that is removed with everything in it when dropped.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Removes the directories in `work` that a kiri which has ended left
/// there, such as one killed while a program drew: those named, as
/// `new_dir` names them, by the id of a process that no longer runs. A
/// directory whose process still runs, or might, is left alone, as is
/// anything else in `work`.
fn remove_stale(work: &Path) {
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
        if is_dir && test_kill_process(pid) == Err(Errno::SRCH) {
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

/// The first line of `text` with something on it, shortened to a length
/// that fits in a warning.
fn first_line(text: &str) -> Option<String> {
    const LONGEST: usize = 200;
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(match line.char_indices().nth(LONGEST) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of its own for one test, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("kiri-external-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("temp")).unwrap();
            Scratch(dir)
        }

        /// A renderer that runs the shell script `script` for Mermaid,
        /// with the input, output, theme and configuration as its
        /// arguments.
        fn renderer(&self, script: &str) -> External {
            let path = self.0.join("render.sh");
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
            External::new(BTreeMap::from([("mermaid".to_owned(), command)]))
                .with_work_dir(self.0.join("temp"))
        }

        /// What the directory for the files has in it.
        fn left(&self) -> Vec<PathBuf> {
            (fs::read_dir(self.0.join("temp")).unwrap())
                .map(|entry| entry.unwrap().path())
                .collect()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const SOURCE: &str = "flowchart TD\n  A --> B\n";

    #[test]
    fn draws_with_the_command_and_reads_its_svg() {
        let scratch = Scratch::new("draws");
        let renderer = scratch.renderer(
            "case \"$1\" in *.mmd) ;; *) exit 3 ;; esac\n\
             printf '<svg theme=\"%s\">%s</svg>' \"$3\" \"$(cat \"$1\")\" > \"$2\"\n",
        );
        assert!(renderer.draws("mermaid") && renderer.draws("Mermaid"));
        assert!(!renderer.draws("dot"));
        for (theme, name) in [(Theme::Light, "default"), (Theme::Dark, "dark")] {
            let drawn = renderer.render("Mermaid", SOURCE, theme).unwrap();
            let svg = format!("<svg theme=\"{name}\">flowchart TD\n  A --> B</svg>");
            assert_eq!(drawn, Drawn::svg(svg, theme, None));
        }
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_diagram_that_chooses_its_theme_is_drawn_for_that_page() {
        let scratch = Scratch::new("own-theme");
        let renderer = scratch.renderer("echo '<svg/>' > \"$2\"\n");
        let forest = format!("%%{{init: {{\"theme\": \"forest\"}}}}%%\n{SOURCE}");
        let drawn = renderer.render("mermaid", &forest, Theme::Dark).unwrap();
        assert_eq!(drawn.theme, Theme::Light);
        let drawn = renderer.render("mermaid", SOURCE, Theme::Dark).unwrap();
        assert_eq!(drawn.theme, Theme::Dark);
    }

    #[test]
    fn mermaid_gets_its_source_unchanged_and_a_configuration_for_svg_text() {
        let scratch = Scratch::new("labels");
        let written = |script: &str, source: &str| {
            let renderer = scratch.renderer(script);
            renderer
                .render("mermaid", source, Theme::Light)
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
        assert_eq!(config, Ok(mermaid_config(Output::Svg)));
        let config: serde_json::Value = serde_json::from_str(&mermaid_config(Output::Svg)).unwrap();
        assert_eq!(config["htmlLabels"], false);
        for diagram in ["flowchart", "class", "state"] {
            assert_eq!(config[diagram]["htmlLabels"], false, "{diagram}");
        }
        // The look and layout that the built-in renderer gives a diagram
        // that does not choose its own, so that both draw alike.
        assert_eq!(config["look"], "classic");
        assert_eq!(config["layout"], "dagre");
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_command_that_takes_the_scale_draws_a_png_at_it() {
        // It writes the name of its output file, its scale and its
        // configuration.
        let scratch = Scratch::new("scale");
        let path = scratch.0.join("render.sh");
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
            External::new(BTreeMap::from([("mermaid".to_owned(), command)]))
                .with_work_dir(scratch.0.join("temp"))
        };
        let drawn = command("-s{scale}")
            .render("mermaid", SOURCE, Theme::Light)
            .unwrap();
        let text = drawn.text();
        let (start, config) = text.split_at(text.find('{').unwrap());
        assert_eq!(start, "diagram.png -s3 ");
        assert_eq!(drawn.scale, SCALE);
        // Labels as a browser draws them, in HTML.
        assert_eq!(config, mermaid_config(Output::Raster));
        let config: serde_json::Value = serde_json::from_str(config).unwrap();
        assert_eq!(config.get("htmlLabels"), None);
        assert_eq!(config["flowchart"].get("htmlLabels"), None);
        // A command without it writes SVG, or a raster image at one pixel
        // to the CSS pixel, and gets labels as SVG text.
        let drawn = command("1")
            .render("mermaid", SOURCE, Theme::Light)
            .unwrap();
        let text = drawn.text();
        let (start, config) = text.split_at(text.find('{').unwrap());
        assert_eq!(start, "diagram.svg 1 ");
        assert_eq!(drawn.scale, 1);
        assert_eq!(config, mermaid_config(Output::Svg));
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn mermaid_draws_diagrams_at_their_own_width() {
        // Not as wide as the page at most, which is 800 pixels in mmdc's
        // browser.
        for output in [Output::Svg, Output::Raster] {
            let config: serde_json::Value = serde_json::from_str(&mermaid_config(output)).unwrap();
            for diagram in ["flowchart", "sequence", "class", "state", "er", "gitGraph"] {
                assert_eq!(config[diagram]["useMaxWidth"], false, "{diagram}");
            }
        }
    }

    #[test]
    fn a_command_may_leave_the_configuration_out() {
        let scratch = Scratch::new("no-config");
        let path = scratch.0.join("render.sh");
        fs::write(&path, "cat \"$1\" > \"$2\"\n").unwrap();
        let argv = ["/bin/sh", &path.to_string_lossy(), "{input}", "{output}"];
        let command = Command::new(argv.map(String::from).to_vec());
        let renderer = External::new(BTreeMap::from([("dot".to_owned(), command)]))
            .with_work_dir(scratch.0.join("temp"));
        let svg = renderer.render("dot", "digraph {}", Theme::Light);
        assert_eq!(svg.map(|drawn| drawn.text()), Ok("digraph {}".to_owned()));
    }

    #[test]
    fn a_command_that_fails_tells_why_in_its_first_line() {
        let scratch = Scratch::new("fails");
        let renderer = scratch.renderer(
            "echo > \"$2\"\n\
             printf '\\n  Error: Parse error on line 2:\\n...A[Unclosed\\n' >&2\n\
             exit 1\n",
        );
        assert_eq!(
            renderer.render("mermaid", SOURCE, Theme::Light),
            Err("sh exited with status 1: Error: Parse error on line 2:".to_owned())
        );
        let renderer = scratch.renderer("exit 2\n");
        assert_eq!(
            renderer.render("mermaid", SOURCE, Theme::Light),
            Err("sh exited with status 2".to_owned())
        );
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_command_that_writes_nothing_fails() {
        let scratch = Scratch::new("writes-nothing");
        let renderer = scratch.renderer("exit 0\n");
        assert_eq!(
            renderer.render("mermaid", SOURCE, Theme::Light),
            Err("sh wrote no output file".to_owned())
        );
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_command_may_write_a_raster_image() {
        // The image loader tells what it is by its content.
        let scratch = Scratch::new("raster");
        let renderer = scratch.renderer("printf '\\211PNG\\r\\n\\032\\n\\377' > \"$2\"\n");
        let drawn = renderer.render("mermaid", SOURCE, Theme::Light).unwrap();
        assert_eq!(drawn.image, b"\x89PNG\r\n\x1a\n\xff");
        assert_eq!(drawn.scale, 1);
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_program_that_is_not_there_fails() {
        let scratch = Scratch::new("missing");
        let command = Command::new(vec![
            "/no/such/mmdc".into(),
            "{input}".into(),
            "{output}".into(),
        ]);
        let renderer = External::new(BTreeMap::from([("mermaid".to_owned(), command)]))
            .with_work_dir(scratch.0.join("temp"));
        let error = renderer
            .render("mermaid", SOURCE, Theme::Light)
            .unwrap_err();
        assert!(error.starts_with("/no/such/mmdc: "), "{error}");
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_command_that_takes_too_long_is_stopped_with_what_it_started() {
        let scratch = Scratch::new("too-long");
        let started = scratch.0.join("started");
        let script = format!(
            "sleep 30 &\necho $! > '{}'\nwait\n",
            started.to_string_lossy()
        );
        let renderer = scratch
            .renderer(&script)
            .with_time_limit(Duration::from_millis(300));
        let start = Instant::now();
        assert_eq!(
            renderer.render("mermaid", SOURCE, Theme::Light),
            Err("sh did not finish in 0.3 s".to_owned())
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
        // The sleep the script started is gone too, or a zombie that is
        // being reaped.
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
    }

    #[test]
    fn a_time_limit_too_long_for_the_clock_means_no_limit() {
        // The configuration file takes any number of seconds that a
        // Duration holds, more than an Instant can be moved by.
        let scratch = Scratch::new("long-limit");
        let renderer = scratch
            .renderer("echo '<svg/>' > \"$2\"\n")
            .with_time_limit(Duration::from_secs_f64(1e19));
        let drawn = renderer.render("mermaid", SOURCE, Theme::Light).unwrap();
        assert_eq!(drawn.theme, Theme::Light);
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_program_that_is_stopped_may_clean_up_first() {
        // mermaid-cli closes its browser on SIGINT, which runs in a session
        // of its own that killing mmdc's process group does not reach.
        let scratch = Scratch::new("clean-up");
        let cleaned = scratch.0.join("cleaned");
        let script = format!(
            "trap \"echo yes > '{}'; exit 130\" INT\nsleep 30 &\nwait\n",
            cleaned.to_string_lossy()
        );
        let renderer = scratch
            .renderer(&script)
            .with_time_limit(Duration::from_millis(300));
        let start = Instant::now();
        assert_eq!(
            renderer.render("mermaid", SOURCE, Theme::Light),
            Err("sh did not finish in 0.3 s".to_owned())
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(fs::read_to_string(&cleaned).ok().as_deref(), Some("yes\n"));
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_stop_kills_the_program_with_what_it_started_and_starts_no_more() {
        let scratch = Scratch::new("stop");
        let started = scratch.0.join("started");
        let script = format!(
            "sleep 30 &\necho $! > '{}'\nwait\n",
            started.to_string_lossy()
        );
        let stop = Arc::new(AtomicBool::new(false));
        let renderer = scratch.renderer(&script).stopped_by(Arc::clone(&stop));
        let start = Instant::now();
        let drawn = std::thread::scope(|scope| {
            let drawing = scope.spawn(|| renderer.render("mermaid", SOURCE, Theme::Light));
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
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
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
            renderer.render("mermaid", SOURCE, Theme::Light),
            Err("sh was not started: interrupted".to_owned())
        );
        assert!(!started.exists());
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn the_placeholders_may_be_inside_arguments() {
        let scratch = Scratch::new("inside");
        let path = scratch.0.join("render.sh");
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
        let renderer = External::new(BTreeMap::from([("mermaid".to_owned(), command)]))
            .with_work_dir(scratch.0.join("temp"));
        let svg = renderer
            .render("mermaid", SOURCE, Theme::Dark)
            .unwrap()
            .text();
        let dir = scratch.0.join("temp").join(format!("{}-0", process::id()));
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
        let scratch = Scratch::new("tmpdir");
        let renderer = scratch.renderer(MAKES_A_PROFILE);
        let tmpdir = renderer
            .render("mermaid", SOURCE, Theme::Light)
            .unwrap()
            .text();
        let expected = scratch.0.join("temp").join(format!("{}-0", process::id()));
        assert_eq!(Path::new(&tmpdir), expected);
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn the_directory_for_the_files_is_made_for_the_user_alone() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = Scratch::new("made");
        let work = scratch.0.join("c/kiri/diagrams");
        let renderer = scratch
            .renderer(MAKES_A_PROFILE)
            .with_work_dir(work.clone());
        let tmpdir = renderer
            .render("mermaid", SOURCE, Theme::Light)
            .unwrap()
            .text();
        assert_eq!(Path::new(&tmpdir).parent(), Some(work.as_path()));
        for dir in [scratch.0.join("c"), scratch.0.join("c/kiri"), work.clone()] {
            let mode = fs::metadata(&dir).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "{}", dir.display());
        }
        assert_eq!(fs::read_dir(&work).unwrap().count(), 0);
    }

    #[test]
    fn directories_left_by_a_kiri_that_has_ended_are_removed() {
        let scratch = Scratch::new("stale");
        let work = scratch.0.join("temp");
        // A process id that no process has now.
        let mut child = process::Command::new("true").spawn().unwrap();
        let ended = child.id();
        child.wait().unwrap();
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
        }
        // A file with such a name is not a directory of kiri's.
        let file = format!("{ended}-1");
        fs::write(work.join(&file), "").unwrap();
        let renderer = scratch.renderer("echo '<svg/>' > \"$2\"\n");
        renderer.render("mermaid", SOURCE, Theme::Light).unwrap();
        let mut left: Vec<String> = (scratch.left().iter())
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        left.sort();
        let mut expected: Vec<String> = kept.into_iter().chain([file]).collect();
        expected.sort();
        assert_eq!(left, expected);
    }

    #[test]
    fn without_a_directory_of_its_own_the_files_go_in_the_temporary_one() {
        let scratch = Scratch::new("fallback");
        fs::write(scratch.0.join("file"), "").unwrap();
        // One that cannot be made, and one whose path would be too long for
        // the sockets a browser makes in it.
        let long = scratch.0.join("x".repeat(LONGEST_WORK_DIR));
        for work in [scratch.0.join("file/diagrams"), long.clone()] {
            let renderer = scratch
                .renderer(MAKES_A_PROFILE)
                .with_work_dir(work.clone());
            let tmpdir = renderer
                .render("mermaid", SOURCE, Theme::Light)
                .unwrap()
                .text();
            let tmpdir = Path::new(&tmpdir);
            assert_eq!(tmpdir.parent(), Some(std::env::temp_dir().as_path()));
            let name = tmpdir.file_name().unwrap().to_string_lossy();
            assert!(name.starts_with("kiri-diagram-"), "{name}");
            assert!(!tmpdir.exists());
        }
        assert!(!long.exists());
    }

    #[test]
    fn tells_why_a_directory_cannot_hold_the_files() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = Scratch::new("unusable");
        let file = scratch.0.join("file");
        fs::write(&file, "").unwrap();
        let locked = scratch.0.join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
        let work = scratch.0.join("temp/kiri/diagrams");
        assert_eq!(not_writable(&work), None);
        assert_eq!(
            not_writable(&file.join("kiri/diagrams")),
            Some(Unusable::NotADirectory(file.clone()))
        );
        let long = scratch.0.join("x".repeat(LONGEST_WORK_DIR));
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
        let scratch = Scratch::new("path");
        let path = scratch.0.join("render.sh");
        fs::write(&path, "printf '%s' \"$PATH\" > \"$2\"\n").unwrap();
        let argv = ["/bin/sh", &path.to_string_lossy(), "{input}", "{output}"];
        let command = Command {
            argv: argv.map(String::from).to_vec(),
            path: Some("/opt/node/bin:/usr/bin".into()),
        };
        let renderer = External::new(BTreeMap::from([("mermaid".to_owned(), command)]))
            .with_work_dir(scratch.0.join("temp"));
        let svg = renderer
            .render("mermaid", SOURCE, Theme::Dark)
            .unwrap()
            .text();
        assert_eq!(svg, "/opt/node/bin:/usr/bin");
    }

    #[test]
    fn a_long_first_line_is_shortened() {
        let line = "x".repeat(300);
        assert_eq!(first_line(&line), Some(format!("{}…", "x".repeat(200))));
        assert_eq!(first_line(" \n\n"), None);
    }
}
