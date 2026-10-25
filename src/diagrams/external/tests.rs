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
    let config: serde_json::Value = serde_json::from_str(&mermaid::config(Output::Svg)).unwrap();
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
fn a_request_s_own_stop_ends_its_program_and_leaves_the_others() {
    let scratch = scratch("own-stop");
    let started = scratch.join("started");
    // The diagram that says `slow` waits; the others are drawn at once.
    let script = format!(
        "case \"$(cat \"$1\")\" in slow*) touch '{}'; sleep 30 ;; esac\n\
             printf '<svg/>' > \"$2\"\n",
        started.to_string_lossy()
    );
    let renderer = script_renderer(&scratch, &script);
    let stop = AtomicBool::new(false);
    let start = Instant::now();
    let request = Request::new("mermaid", "slow\n", Theme::Light).stopped_by(&stop);
    let (slow, other) = std::thread::scope(|scope| {
        let drawing = scope.spawn(|| renderer.render(&request));
        while !started.exists() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        stop.store(true, Ordering::SeqCst);
        let other = renderer.render(&Request::new("mermaid", SOURCE, Theme::Light));
        (drawing.join().unwrap(), other)
    });
    assert_eq!(slow, Err("sh was stopped: interrupted".to_owned()));
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(other.map(|drawn| drawn.text()).as_deref(), Ok("<svg/>"));
    // A request stopped before its program starts does not start it.
    fs::remove_file(&started).unwrap();
    assert_eq!(
        renderer.render(&request),
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
