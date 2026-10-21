//! Diagrams drawn by a program outside kiri, such as mermaid-cli's `mmdc`.
//!
//! kiri writes the diagram's source to a file in a directory of its own,
//! and for Mermaid the program's settings beside it. It runs the command
//! that kiri's configuration file gives, or that kiri found, without a
//! shell, and reads the SVG the program wrote. The directory goes when
//! the program has finished, failed or run out of time.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, DirBuilder, File};
use std::io::{self, ErrorKind};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::{Drawn, Renderer, Theme};

/// How long a program may take to draw one diagram before kiri stops it,
/// unless the configuration file says otherwise.
pub const TIME_LIMIT: Duration = Duration::from_secs(20);

/// How many programs draw diagrams at once.
pub const AT_ONCE: usize = 4;

/// A program that draws diagrams and its arguments, in which `{input}`
/// stands for the file of the diagram's source, `{output}` for the SVG
/// file the program writes, `{theme}` for the theme kiri chose, and
/// `{config}` for a configuration file kiri writes for the language, such
/// as Mermaid's.
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
    /// Where the directories for the files go.
    temp: PathBuf,
    /// How many directories this renderer has made, to name the next one.
    made: AtomicUsize,
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
            temp: std::env::temp_dir(),
            made: AtomicUsize::new(0),
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

    /// This renderer with its files in directories under `dir`.
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

    /// A new directory of its own for one diagram's files.
    fn temp_dir(&self) -> io::Result<TempDir> {
        loop {
            let n = self.made.fetch_add(1, Ordering::Relaxed);
            let dir = (self.temp).join(format!("kiri-diagram-{}-{n}", process::id()));
            match DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => return Ok(TempDir(dir)),
                // Left over from an earlier kiri with the same process id.
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    /// The SVG that `command` draws of `source`, in `language`, in `theme`.
    fn run(
        &self,
        command: &Command,
        language: &str,
        source: &str,
        theme: Theme,
    ) -> Result<String, String> {
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
        let output = dir.0.join("diagram.svg");
        let errors = dir.0.join("errors.txt");
        let config = config(language).map(|(text, extension)| {
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
                    .replace("{theme}", theme);
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
            Ok(bytes) => String::from_utf8(bytes).map_err(|_| format!("{program} wrote no SVG")),
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
        let svg = self.run(command, language, source, theme)?;
        // The program cannot tell which theme the diagram chose itself.
        let theme = super::chosen_theme(language, source).unwrap_or(theme);
        Ok(Drawn { svg, theme })
    }
}

/// The Mermaid configuration that a command gets as `{config}`: it has a
/// diagram's labels drawn as SVG text, which kiri can draw, rather than as
/// HTML, which it cannot. Mermaid 11 and later read the first setting;
/// Mermaid 10 the others.
const MERMAID_CONFIG: &str = "{\"htmlLabels\": false, \"flowchart\": {\"htmlLabels\": false}, \
                              \"class\": {\"htmlLabels\": false}, \"state\": {\"htmlLabels\": false}}\n";

/// The configuration a command for `language` gets as `{config}`, and
/// the extension of its file.
fn config(language: &str) -> Option<(&'static str, &'static str)> {
    language
        .eq_ignore_ascii_case("mermaid")
        .then_some((MERMAID_CONFIG, "json"))
}

/// A directory that is removed with everything in it when dropped.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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
fn wait(
    mut child: process::Child,
    limit: Duration,
    stop: &dyn Fn() -> bool,
) -> Result<ExitStatus, Waited> {
    let deadline = Instant::now() + limit;
    let mut pause = Duration::from_millis(1);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if stop() || Instant::now() >= deadline => {
                end(&mut child);
                return Err(match stop() {
                    true => Waited::Stopped,
                    false => Waited::TimedOut,
                });
            }
            Ok(None) => {
                std::thread::sleep(pause.min(deadline.saturating_duration_since(Instant::now())));
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
                .with_temp_dir(self.0.join("temp"))
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
            assert_eq!(drawn, Drawn { svg, theme });
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
                .map(|drawn| drawn.svg)
        };
        // The source as it is, so that the program's line numbers are the
        // diagram's.
        let front_matter = "\n---\ntitle: T\n---\nflowchart TD\n";
        for source in [SOURCE, front_matter] {
            let source_file = written("cat \"$1\" > \"$2\"\n", source);
            assert_eq!(source_file, Ok(source.to_owned()));
        }
        let config = written("cat \"$4\" > \"$2\"\n", SOURCE);
        assert_eq!(config, Ok(MERMAID_CONFIG.to_owned()));
        let config: serde_json::Value = serde_json::from_str(MERMAID_CONFIG).unwrap();
        assert_eq!(config["htmlLabels"], false);
        for diagram in ["flowchart", "class", "state"] {
            assert_eq!(config[diagram]["htmlLabels"], false, "{diagram}");
        }
        assert_eq!(scratch.left(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_command_may_leave_the_configuration_out() {
        let scratch = Scratch::new("no-config");
        let path = scratch.0.join("render.sh");
        fs::write(&path, "cat \"$1\" > \"$2\"\n").unwrap();
        let argv = ["/bin/sh", &path.to_string_lossy(), "{input}", "{output}"];
        let command = Command::new(argv.map(String::from).to_vec());
        let renderer = External::new(BTreeMap::from([("dot".to_owned(), command)]))
            .with_temp_dir(scratch.0.join("temp"));
        let svg = renderer.render("dot", "digraph {}", Theme::Light);
        assert_eq!(svg.map(|drawn| drawn.svg), Ok("digraph {}".to_owned()));
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
        let renderer = scratch.renderer("printf '\\377\\376' > \"$2\"\n");
        assert_eq!(
            renderer.render("mermaid", SOURCE, Theme::Light),
            Err("sh wrote no SVG".to_owned())
        );
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
            .with_temp_dir(scratch.0.join("temp"));
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
            .with_temp_dir(scratch.0.join("temp"));
        let svg = renderer.render("mermaid", SOURCE, Theme::Dark).unwrap().svg;
        let dir = scratch
            .0
            .join("temp")
            .join(format!("kiri-diagram-{}-0", process::id()));
        let dir = dir.to_string_lossy();
        assert_eq!(
            svg,
            format!("--theme=dark --out={dir}/diagram.svg {dir}/diagram.mmd ")
        );
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
            .with_temp_dir(scratch.0.join("temp"));
        let svg = renderer.render("mermaid", SOURCE, Theme::Dark).unwrap().svg;
        assert_eq!(svg, "/opt/node/bin:/usr/bin");
    }

    #[test]
    fn a_long_first_line_is_shortened() {
        let line = "x".repeat(300);
        assert_eq!(first_line(&line), Some(format!("{}…", "x".repeat(200))));
        assert_eq!(first_line(" \n\n"), None);
    }
}
