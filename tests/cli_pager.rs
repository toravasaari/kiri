//! The pager end to end: kiri in a pseudo-terminal, with keys typed into
//! it.

use std::io::{Read, Write};
use std::os::fd::BorrowedFd;
use std::os::unix::process::CommandExt;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use rustix::process::Resource;

mod common;

use common::{Scratch, command, fixtures, read_terminal, text};

/// The alternate screen, which the pager draws on.
const ENTER: &str = "\x1b[?1049h";
const LEAVE: &str = "\x1b[?1049l";

/// A document of `count` paragraphs, one row each.
fn paragraphs(count: usize) -> String {
    (1..=count).map(|n| format!("Line {n}\n\n")).collect()
}

/// Runs kiri with `args` in a terminal of 40 × `rows` cells, in a scratch
/// directory called `name` that holds `document.md` of 3 rows and `long.md`
/// of [`long`]. Pipes `stdin` in when given, and types each of `keys` after
/// a frame of the pager, the first after the first frame. `--mode ansi`
/// unless `args` choose a mode. Returns how kiri exited and everything it
/// wrote to the terminal.
fn in_terminal(
    name: &str,
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    keys: &[&[u8]],
) -> (ExitStatus, String) {
    let mut typed = 0;
    in_terminal_with(name, args, rows, stdin, |frames, terminal, _| {
        if typed < frames.min(keys.len()) {
            terminal.write_all(keys[typed]).unwrap();
            typed += 1;
        }
    })
}

/// [`in_terminal`] with `act` called after each read of the terminal,
/// with the number of frames drawn so far, the terminal side and kiri's
/// process.
fn in_terminal_with(
    name: &str,
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    act: impl FnMut(usize, &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String) {
    in_terminal_after(name, |_| {}, args, rows, stdin, act)
}

/// [`in_terminal_with`], with `prepare` called with the scratch directory
/// before kiri starts.
fn in_terminal_after(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    act: impl FnMut(usize, &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String) {
    let (status, seen, stderr) = in_terminal_told(name, prepare, args, rows, stdin, act);
    assert_eq!(stderr, "");
    (status, seen)
}

/// [`in_terminal_after`], with what kiri wrote to standard error returned
/// last instead of expected to be empty.
fn in_terminal_told(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    act: impl FnMut(usize, &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String, String) {
    in_terminal_run(name, prepare, args, rows, stdin, Setup::default(), act)
}

/// [`in_terminal_after`], with the environment variables `env` set for
/// kiri.
fn in_terminal_env(
    name: &str,
    env: &[(&str, &str)],
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    act: impl FnMut(usize, &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String) {
    let setup = Setup { both: false, env };
    let (status, seen, stderr) = in_terminal_run(name, |_| {}, args, rows, stdin, setup, act);
    assert_eq!(stderr, "");
    (status, seen)
}

/// [`in_terminal_after`], with what kiri writes to standard error also on
/// the terminal, among what it writes to standard output.
fn in_terminal_both(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    rows: u16,
    act: impl FnMut(usize, &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String) {
    let setup = Setup {
        both: true,
        env: &[],
    };
    let (status, seen, _) = in_terminal_run(name, prepare, args, rows, None, setup, act);
    (status, seen)
}

/// How [`in_terminal_run`] sets kiri up besides its arguments.
#[derive(Default)]
struct Setup<'a> {
    /// Standard error on the terminal too; what kiri writes to it is then
    /// on the terminal instead of returned.
    both: bool,
    /// Environment variables set for kiri.
    env: &'a [(&'a str, &'a str)],
}

/// [`in_terminal_told`], set up as `setup` says.
fn in_terminal_run(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    setup: Setup,
    mut act: impl FnMut(usize, &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String, String) {
    let dir = Scratch::new(name);
    std::fs::write(dir.join("document.md"), paragraphs(3)).unwrap();
    std::fs::write(dir.join("long.md"), long()).unwrap();
    prepare(&dir);
    let (mut terminal, tty) = common::pty();
    let window = rustix::termios::Winsize {
        ws_col: 40,
        ws_row: rows,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    rustix::termios::tcsetwinsize(&tty, window).unwrap();
    let mut kiri = command(&dir);
    if !args.contains(&"--mode") {
        kiri.args(["--mode", "ansi"]);
    }
    kiri.current_dir(&*dir)
        .envs(setup.env.iter().copied())
        .args(args)
        .stdin(match stdin {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        })
        .stderr(match setup.both {
            true => Stdio::from(tty.try_clone().unwrap()),
            false => Stdio::piped(),
        })
        .stdout(Stdio::from(tty));
    // SAFETY: setsid, ioctl, getrlimit and setrlimit are async-signal-safe
    // and touch no memory.
    unsafe {
        kiri.pre_exec(|| {
            rustix::process::setsid()?;
            // Standard output is the terminal by now.
            rustix::process::ioctl_tiocsctty(BorrowedFd::borrow_raw(1))?;
            // No core dump when a test ends kiri with SIGQUIT.
            let mut core = rustix::process::getrlimit(Resource::Core);
            core.current = Some(0);
            rustix::process::setrlimit(Resource::Core, core)?;
            Ok(())
        });
    }
    let mut child = kiri.spawn().unwrap();
    // The command holds the program's side of the terminal. Close it, so
    // that reading the terminal side ends once kiri exits.
    drop(kiri);
    if let Some(document) = stdin {
        let mut input = child.stdin.take().unwrap();
        input.write_all(document.as_bytes()).unwrap();
    }
    // kiri gets 10 s at most.
    let seen = read_terminal(
        &mut terminal,
        &mut child,
        Duration::from_secs(10),
        |seen, terminal, child| {
            let frames = text(seen).matches("\x1b[?2026l").count();
            act(frames, terminal, child);
        },
    );
    let status = child.wait().unwrap();
    let mut stderr = String::new();
    if let Some(mut told) = child.stderr.take() {
        told.read_to_string(&mut stderr).unwrap();
    }
    (status, text(&seen), stderr)
}

/// A document longer than a window of 5 rows.
fn long() -> String {
    paragraphs(10)
}

#[test]
fn a_long_document_is_paged_until_q() {
    let (status, seen) = in_terminal("pager-long", &["long.md"], 5, None, &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(seen.ends_with(LEAVE), "{seen:?}");
    // The bottom row is the status line's.
    assert!(seen.contains("Line 2"), "{seen:?}");
    assert!(!seen.contains("Line 3"), "{seen:?}");
    assert!(seen.contains("\x1b[5;1H\x1b[0;1m kiri "), "{seen:?}");
    assert!(seen.contains(" long.md "), "{seen:?}");
}

#[test]
fn keys_scroll_the_document() {
    let (status, seen) = in_terminal("pager-keys", &["long.md"], 5, None, &[b"G", b"q"]);
    assert!(status.success());
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    assert_eq!(frames.len(), 2, "{seen:?}");
    assert!(!frames[0].contains("Line 10"), "{seen:?}");
    assert!(frames[1].contains("Line 10"), "{seen:?}");
}

#[test]
fn ctrl_c_quits_and_leaves_the_screen() {
    let (status, seen) = in_terminal("pager-ctrl-c", &["long.md"], 5, None, &[b"\x03"]);
    assert!(status.success());
    assert!(seen.ends_with(LEAVE), "{seen:?}");
}

#[test]
fn a_long_file_shown_as_text_is_paged_too() {
    let (status, seen) = in_terminal(
        "pager-text",
        &["--format", "text", "long.md"],
        5,
        None,
        &[b"q"],
    );
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(seen.contains("Line 1"), "{seen:?}");
    assert!(seen.ends_with(LEAVE), "{seen:?}");
}

#[test]
fn a_short_document_is_printed() {
    let (status, seen) = in_terminal("pager-short", &["document.md"], 24, None, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert!(seen.contains("Line 3"), "{seen:?}");
}

#[test]
fn p_pages_a_short_document() {
    let (status, seen) = in_terminal("pager-p", &["-p", "document.md"], 24, None, &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
}

#[test]
fn following_pages_a_short_file() {
    let config = |text: &'static str| {
        move |dir: &std::path::Path| {
            if !text.is_empty() {
                common::write(&dir.join("config/kiri/config.toml"), text);
            }
        }
    };
    for (args, file) in [
        (&["-f", "document.md"][..], ""),
        (&["--follow", "on", "document.md"][..], ""),
        (&["--fa", "document.md"][..], ""),
        (&["--follow", "auto", "document.md"][..], ""),
        (&["document.md"][..], "follow = \"on\"\n"),
        (&["--paging", "auto", "-f", "document.md"][..], ""),
    ] {
        let (status, seen) = in_terminal_after(
            "pager-follow-short",
            config(file),
            args,
            24,
            None,
            |frames, terminal, _| {
                if frames == 1 {
                    terminal.write_all(b"q").unwrap();
                }
            },
        );
        assert!(status.success(), "{args:?} {file:?}");
        assert!(seen.starts_with(ENTER), "{args:?} {file:?}: {seen:?}");
        assert!(
            text_of(&seen).contains(" follow "),
            "{args:?} {file:?}: {seen:?}"
        );
    }
}

#[test]
fn following_still_prints_with_paging_never() {
    for (args, file) in [
        (&["-f", "-P", "document.md"][..], ""),
        (&["-f", "document.md"][..], "paging = \"never\"\n"),
        (
            &["document.md"][..],
            "paging = \"never\"\nfollow = \"on\"\n",
        ),
    ] {
        let prepare = |dir: &std::path::Path| {
            if !file.is_empty() {
                common::write(&dir.join("config/kiri/config.toml"), file);
            }
        };
        let (status, seen) =
            in_terminal_after("pager-follow-never", prepare, args, 24, None, |_, _, _| {});
        assert!(status.success(), "{args:?} {file:?}");
        assert!(!seen.contains(ENTER), "{args:?} {file:?}: {seen:?}");
        assert!(seen.contains("Line 3"), "{args:?} {file:?}: {seen:?}");
    }
}

#[test]
fn kiri_follow_pages_a_short_file_and_leaves_standard_input_quiet() {
    let env = [("KIRI_FOLLOW", "on")];
    let quit = |frames: usize, terminal: &mut std::fs::File, _: &std::process::Child| {
        if frames == 1 {
            terminal.write_all(b"q").unwrap();
        }
    };
    let args = ["document.md"];
    let (status, seen) = in_terminal_env("pager-follow-variable", &env, &args, 24, None, quit);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(text_of(&seen).contains(" follow "), "{seen:?}");
    let stdin = Some(long());
    let (status, seen) = in_terminal_env(
        "pager-follow-variable-stdin",
        &env,
        &["-p"],
        5,
        stdin.as_deref(),
        quit,
    );
    assert!(status.success());
    assert!(text_of(&seen).contains("Line 1"), "{seen:?}");
    assert!(!text_of(&seen).contains("follow"), "{seen:?}");
}

#[test]
fn capital_p_prints_a_long_document() {
    for args in [
        &["-P", "long.md"][..],
        &["--paging", "never", "long.md"][..],
    ] {
        let (status, seen) = in_terminal("pager-capital-p", args, 5, None, &[]);
        assert!(status.success());
        assert!(!seen.contains(ENTER), "{args:?}: {seen:?}");
        assert!(seen.contains("Line 10"), "{args:?}: {seen:?}");
    }
}

#[test]
fn plain_mode_prints_without_the_pager() {
    let (status, seen) = in_terminal(
        "pager-plain",
        &["-p", "--mode", "plain", "-"],
        5,
        Some(&long()),
        &[],
    );
    assert!(status.success());
    assert!(!seen.contains('\x1b'), "{seen:?}");
}

#[test]
fn a_pipe_gets_the_document_printed_even_with_p() {
    let dir = Scratch::new("pager-pipe");
    let out = common::kiri(&dir, &long(), &["-p", "--mode", "ansi"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(!stdout.contains(ENTER), "{stdout:?}");
    assert!(stdout.contains("Line 10"), "{stdout:?}");
}

#[test]
fn a_pipe_gets_an_image_file_printed_even_with_p() {
    let dir = Scratch::new("pager-pipe-image");
    let icon = fixtures().join("images/icon.png");
    let out = common::kiri_on(&dir, &icon, &["-p", "--mode", "kitty"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(!stdout.contains(ENTER), "{stdout:?}");
    assert_eq!(stdout.matches("\x1b_Ga=T,").count(), 1, "{stdout:?}");
    let without = common::kiri_on(&dir, &icon, &["--mode", "kitty"]);
    assert_eq!(stdout, text(&without.stdout));
}

#[test]
fn a_piped_document_is_printed_unless_paging_is_always() {
    let (status, seen) = in_terminal("pager-stdin-auto", &["-"], 5, Some(&long()), &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert!(seen.contains("Line 10"), "{seen:?}");
    let (status, seen) = in_terminal("pager-stdin-p", &["-p", "-"], 5, Some(&long()), &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
}

#[test]
fn the_doctor_report_is_paged_only_when_paging_is_always() {
    let (status, seen) = in_terminal("pager-doctor-auto", &["--doctor"], 5, None, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert!(seen.contains("Settings"), "{seen:?}");
    let (status, seen) = in_terminal("pager-doctor-p", &["--doctor", "-p"], 5, None, &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    let raw = ["--doctor", "--raw", "-p"];
    let (status, seen) = in_terminal("pager-doctor-raw", &raw, 5, None, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
}

/// Runs kiri with `args` in a scratch directory called `name`, after
/// `prepare` is called with it, with standard output a terminal of 40 × 5
/// cells of 10 × 20 pixels but no terminal to read keys from. Returns how
/// kiri exited, everything it wrote to the terminal and what it wrote to
/// standard error.
fn without_keys(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
) -> (ExitStatus, String, String) {
    let dir = Scratch::new(name);
    prepare(&dir);
    let (mut terminal, tty) = common::pty();
    let window = rustix::termios::Winsize {
        ws_col: 40,
        ws_row: 5,
        ws_xpixel: 400,
        ws_ypixel: 100,
    };
    rustix::termios::tcsetwinsize(&tty, window).unwrap();
    let mut kiri = command(&dir);
    kiri.current_dir(&*dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(tty))
        .stderr(Stdio::piped());
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        // A session of its own and no controlling terminal: standard
        // output is a terminal, but there is none to read keys from.
        kiri.pre_exec(|| {
            rustix::process::setsid()?;
            Ok(())
        });
    }
    let mut child = kiri.spawn().unwrap();
    drop(kiri);
    let output = read_terminal(
        &mut terminal,
        &mut child,
        Duration::from_secs(10),
        |_, _, _| {},
    );
    let status = child.wait().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    (status, text(&output), stderr)
}

#[test]
fn without_a_terminal_for_keys_p_prints_with_a_warning() {
    let prepare = |dir: &std::path::Path| std::fs::write(dir.join("long.md"), long()).unwrap();
    let args = ["--mode", "ansi", "-p", "long.md"];
    let (status, seen, stderr) = without_keys("pager-no-keys", prepare, &args);
    assert!(status.success(), "{stderr}");
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert!(seen.contains("Line 10"), "{seen:?}");
    assert_eq!(
        stderr,
        "kiri: no terminal to read keys from, so the document is printed\n"
    );
}

#[test]
fn without_a_terminal_for_keys_p_prints_an_image_file_as_without_it() {
    // Larger than the window, so that the window decides its size.
    let prepare = |dir: &std::path::Path| {
        image::RgbaImage::from_pixel(800, 400, image::Rgba([255, 0, 0, 255]))
            .save(dir.join("a.png"))
            .unwrap();
    };
    let args = ["--mode", "kitty", "-p", "a.png"];
    let (status, seen, stderr) = without_keys("pager-no-keys-image", prepare, &args);
    assert!(status.success(), "{stderr}");
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    assert_eq!(
        stderr,
        "kiri: no terminal to read keys from, so the document is printed\n"
    );
    let args = ["--mode", "kitty", "a.png"];
    let (status, printed, stderr) = without_keys("pager-no-keys-image-auto", prepare, &args);
    assert!(status.success(), "{stderr}");
    assert_eq!(seen, printed);
}

#[test]
fn a_signal_to_end_kiri_leaves_the_screen_first() {
    use std::os::unix::process::ExitStatusExt;

    use rustix::process::{Pid, Signal, kill_process};

    for signal in [Signal::TERM, Signal::HUP, Signal::INT, Signal::QUIT] {
        let mut sent = false;
        let (status, seen) =
            in_terminal_with("pager-signal", &["long.md"], 5, None, |frames, _, child| {
                if frames > 0 && !sent {
                    let pid = Pid::from_raw(child.id() as i32).unwrap();
                    kill_process(pid, signal).unwrap();
                    sent = true;
                }
            });
        assert!(seen.ends_with(LEAVE), "{signal:?}: {seen:?}");
        assert_eq!(status.signal(), Some(signal.as_raw()), "{signal:?}");
    }
}

#[test]
fn a_signal_after_a_program_drew_the_diagrams_leaves_the_screen_first() {
    use std::os::unix::process::ExitStatusExt;

    use rustix::process::{Pid, Signal, kill_process};

    // kiri catches Ctrl-C while the program draws, and the pager catches
    // it once the document shows.
    let prepare = |dir: &std::path::Path| {
        let script = dir.join("render.sh");
        std::fs::write(
            &script,
            "echo '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"20\"/>' > \"$2\"\n",
        )
        .unwrap();
        let config = format!(
            "[diagrams.mermaid]\ncommand = [\"/bin/sh\", \"{}\", \"{{input}}\", \"{{output}}\"]\n",
            script.display()
        );
        common::write(&dir.join("config/kiri/config.toml"), &config);
        let document = format!("```mermaid\nflowchart TD\n```\n\n{}", long());
        std::fs::write(dir.join("diagram.md"), document).unwrap();
    };
    let mut sent = false;
    let (status, seen) = in_terminal_after(
        "pager-signal-diagrams",
        prepare,
        &["--mode", "kitty", "diagram.md"],
        5,
        None,
        |frames, _, child| {
            if frames > 0 && !sent {
                let pid = Pid::from_raw(child.id() as i32).unwrap();
                kill_process(pid, Signal::INT).unwrap();
                sent = true;
            }
        },
    );
    assert!(seen.contains("\x1b_G"), "the diagram is drawn: {seen:?}");
    assert!(seen.ends_with(LEAVE), "{seen:?}");
    assert_eq!(status.signal(), Some(Signal::INT.as_raw()));
}

#[test]
fn the_pager_shows_why_a_diagram_was_not_drawn_and_finds_it() {
    let prepare = |dir: &std::path::Path| {
        let script = dir.join("render.sh");
        std::fs::write(&script, "echo 'Error: bad' >&2\nexit 1\n").unwrap();
        let config = format!(
            "render_errors = \"inline\"\n\
             [diagrams.mermaid]\ncommand = [\"/bin/sh\", \"{}\", \"{{input}}\", \"{{output}}\"]\n",
            script.display()
        );
        common::write(&dir.join("config/kiri/config.toml"), &config);
        let document = format!("{}```mermaid\nflowchart TD\n```\n", long());
        std::fs::write(dir.join("diagram.md"), document).unwrap();
    };
    let keys: [&[u8]; 8] = [b"/", b"d", b"r", b"a", b"w", b"n", b"\r", b"q"];
    let mut typed = 0;
    let (status, seen) = in_terminal_after(
        "pager-render-error",
        prepare,
        &["--mode", "kitty", "diagram.md"],
        5,
        None,
        |frames, terminal, _| {
            if typed < frames.min(keys.len()) {
                terminal.write_all(keys[typed]).unwrap();
                typed += 1;
            }
        },
    );
    assert!(status.success());
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    // Below the end of the first page, and shown once found.
    assert!(!frames[0].contains("could not be"), "{seen:?}");
    let found = frames.last().unwrap();
    assert!(found.contains("✗ mermaid diagram could not be"), "{seen:?}");
    assert!(found.contains("/drawn 1/1"), "{seen:?}");
}

#[test]
fn several_files_are_paged_as_one_stream_under_their_names() {
    let keys: [&[u8]; 4] = [b"G", b"g", b"/", b"Line 10\r"];
    let mut typed = 0;
    let (status, seen) = in_terminal_with(
        "pager-files",
        &["document.md", "long.md"],
        8,
        None,
        |frames, terminal, _| {
            if typed < frames.min(keys.len()) {
                terminal.write_all(keys[typed]).unwrap();
                typed += 1;
            } else if typed == keys.len() && frames > keys.len() {
                terminal.write_all(b"q").unwrap();
                typed += 1;
            }
        },
    );
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(seen.ends_with(LEAVE), "{seen:?}");
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    assert!(frames[0].contains("document.md"), "{seen:?}");
    assert!(!frames[0].contains("long.md"), "{seen:?}");
    // The end of the stream is the end of the last file.
    assert!(frames[1].contains("Line 10"), "{seen:?}");
    // Search goes on from the first file into the second.
    assert!(frames.last().unwrap().contains("Line 10"), "{seen:?}");
    assert!(frames.last().unwrap().contains("/Line 10 1/1"), "{seen:?}");
}

#[test]
fn colon_n_moves_to_the_next_file_and_the_status_line_names_it() {
    let (status, seen) = in_terminal(
        "pager-next-file",
        &["document.md", "long.md"],
        8,
        None,
        &[b":n", b"q"],
    );
    assert!(status.success());
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    assert_eq!(frames.len(), 2, "{seen:?}");
    assert!(!frames[0].contains("long.md"), "{seen:?}");
    assert!(frames[1].contains("long.md"), "{seen:?}");
    assert!(!frames[1].contains("document.md"), "{seen:?}");
}

#[test]
fn a_file_that_cannot_be_shown_is_in_the_stream_under_its_header() {
    let (status, seen, stderr) = in_terminal_told(
        "pager-missing",
        |_| {},
        &["document.md", "missing.md", "long.md"],
        8,
        None,
        |frames, terminal, _| match frames {
            1 => terminal.write_all(b":n").unwrap(),
            2 => terminal.write_all(b"q").unwrap(),
            _ => {}
        },
    );
    assert_eq!(status.code(), Some(1), "{seen:?}");
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(stderr.starts_with("kiri: missing.md: "), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    assert_eq!(frames.len(), 2, "{seen:?}");
    // :n goes to missing.md, its header and why it is not shown.
    assert!(frames[1].contains("missing.md"), "{seen:?}");
    assert!(frames[1].contains("No such file"), "{seen:?}");
    assert!(frames[1].contains(" missing.md 2/3 "), "{seen:?}");
}

#[test]
fn a_file_that_cannot_be_shown_is_told_at_its_place_when_the_files_fit() {
    let prepare = |dir: &std::path::Path| {
        std::fs::write(dir.join("a.md"), "Alpha text.\n").unwrap();
        std::fs::write(dir.join("b.md"), "Beta text.\n").unwrap();
    };
    let args = ["a.md", "missing.md", "b.md"];
    let (status, seen) = in_terminal_both("pager-missing-fits", prepare, &args, 24, |_, _, _| {});
    assert_eq!(status.code(), Some(1), "{seen:?}");
    assert!(!seen.contains(ENTER), "{seen:?}");
    // As printed: told once, between the files around it, and not shown.
    let mut rest = seen.as_str();
    for part in ["Alpha text.", "kiri: missing.md: ", "b.md", "Beta text."] {
        let at = rest
            .find(part)
            .unwrap_or_else(|| panic!("{part}: {seen:?}"));
        rest = &rest[at + part.len()..];
    }
    assert_eq!(seen.matches("missing.md").count(), 1, "{seen:?}");
    assert!(!seen.contains('✗'), "{seen:?}");
}

#[test]
fn a_front_matter_warning_is_told_at_its_place_when_the_files_fit() {
    let invalid = std::fs::read_to_string(fixtures().join("front-matter-invalid.md")).unwrap();
    let prepare = |dir: &std::path::Path| {
        std::fs::write(dir.join("a.md"), "Alpha text.\n").unwrap();
        std::fs::write(dir.join("c.md"), invalid).unwrap();
        std::fs::write(dir.join("b.md"), "Beta text.\n").unwrap();
    };
    let args = ["a.md", "c.md", "b.md"];
    let (status, seen) = in_terminal_both("pager-warning-fits", prepare, &args, 40, |_, _, _| {});
    assert_eq!(status.code(), Some(0), "{seen:?}");
    assert!(!seen.contains(ENTER), "{seen:?}");
    // As printed: the warning comes after the file before it and before
    // its own file's header.
    let mut rest = seen.as_str();
    for part in ["Alpha text.", "kiri: c.md: front matter", "c.md", "b.md"] {
        let at = rest
            .find(part)
            .unwrap_or_else(|| panic!("{part}: {seen:?}"));
        rest = &rest[at + part.len()..];
    }
}

#[test]
fn when_no_file_can_be_shown_the_pager_does_not_open() {
    let mut typed = false;
    let args = ["-p", "nope1.md", "nope2.md"];
    let (status, seen, stderr) = in_terminal_told(
        "pager-none",
        |_| {},
        &args,
        8,
        None,
        |frames, terminal, _| {
            // An empty pager would wait for q.
            if frames > 0 && !typed {
                terminal.write_all(b"q").unwrap();
                typed = true;
            }
        },
    );
    assert_eq!(status.code(), Some(1), "{seen:?}");
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert!(stderr.contains("nope1.md"), "{stderr}");
    assert!(stderr.contains("nope2.md"), "{stderr}");
}

/// [`in_terminal`] in a window of 5 rows, with copies of an image beside
/// the documents, `a.png` and `b.png`.
fn with_images(name: &str, args: &[&str], keys: &[&[u8]]) -> (ExitStatus, String) {
    let icon = common::fixtures().join("images/icon.png");
    let prepare = |dir: &std::path::Path| {
        for name in ["a.png", "b.png"] {
            std::fs::copy(&icon, dir.join(name)).unwrap();
        }
    };
    let mut typed = 0;
    in_terminal_after(name, prepare, args, 5, None, |frames, terminal, _| {
        if typed < frames.min(keys.len()) {
            terminal.write_all(keys[typed]).unwrap();
            typed += 1;
        }
    })
}

#[test]
fn image_files_alone_are_paged_only_when_paging_is_always() {
    let args = ["--mode", "kitty", "a.png", "b.png"];
    let (status, seen) = with_images("pager-images", &args, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 2, "{seen:?}");
    let args = ["--mode", "kitty", "-p", "a.png", "b.png"];
    let (status, seen) = with_images("pager-images-p", &args, &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(seen.contains("a.png"), "{seen:?}");
}

#[test]
fn image_files_with_a_document_are_paged_when_they_are_long() {
    let args = ["--mode", "kitty", "a.png", "document.md", "b.png"];
    let (status, seen) = with_images("pager-images-document", &args, &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
}

#[test]
fn an_image_file_on_its_own_is_paged_only_when_paging_is_always() {
    let args = ["--mode", "kitty", "a.png"];
    let (status, seen) = with_images("pager-image", &args, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    let args = ["--mode", "kitty", "-p", "a.png"];
    let (status, seen) = with_images("pager-image-p", &args, &[b"r", b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(seen.ends_with(LEAVE), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    // The status line of a file on its own, without its place among files.
    assert!(text_of(frames[0]).contains(" a.png "), "{seen:?}");
    assert!(!text_of(frames[0]).contains("1/1"), "{seen:?}");
    assert!(text_of(&seen).contains("nothing to reload"), "{seen:?}");
}

#[test]
fn paging_always_in_the_configuration_pages_an_image_file_on_its_own() {
    let prepare = |dir: &std::path::Path| {
        std::fs::copy(fixtures().join("images/icon.png"), dir.join("a.png")).unwrap();
        common::write(
            &dir.join("config/kiri/config.toml"),
            "paging = \"always\"\n",
        );
    };
    let (status, seen) = in_terminal_after(
        "pager-image-config",
        prepare,
        &["--mode", "kitty", "a.png"],
        5,
        None,
        |frames, terminal, _| {
            if frames == 1 {
                terminal.write_all(b"q").unwrap();
            }
        },
    );
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
}

#[test]
fn p_pages_an_image_from_standard_input() {
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"20\"/>\n";
    let args = ["--mode", "kitty", "--format", "svg"];
    let (status, seen) = in_terminal("pager-image-stdin", &args, 5, Some(svg), &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    let args = ["--mode", "kitty", "--format", "svg", "-p"];
    let (status, seen) = in_terminal("pager-image-stdin-p", &args, 5, Some(svg), &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(text_of(&seen).contains(" <stdin> "), "{seen:?}");
}

/// Writes a configuration in `dir` whose program draws every Mermaid
/// diagram as a square SVG, and `diagram.mmd`.
fn drawn_diagram(dir: &std::path::Path) {
    let script = dir.join("render.sh");
    std::fs::write(
        &script,
        "echo '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"20\"/>' > \"$2\"\n",
    )
    .unwrap();
    let config = format!(
        "[diagrams.mermaid]\ncommand = [\"/bin/sh\", \"{}\", \"{{input}}\", \"{{output}}\"]\n",
        script.display()
    );
    common::write(&dir.join("config/kiri/config.toml"), &config);
    std::fs::write(dir.join("diagram.mmd"), "flowchart TD\n").unwrap();
}

#[test]
fn a_diagram_file_on_its_own_is_paged_only_when_paging_is_always() {
    let args = ["--mode", "kitty", "diagram.mmd"];
    let (status, seen) = in_steps_with("pager-diagram", drawn_diagram, &args, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    let reload = Step {
        frames: 1,
        keys: &|_| b"r",
    };
    let steps = [reload, quit_after(2)];
    let args = ["--mode", "kitty", "-p", "diagram.mmd"];
    let (status, seen) = in_steps_with("pager-diagram-p", drawn_diagram, &args, &steps);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(seen.ends_with(LEAVE), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    assert!(text_of(frames[0]).contains(" diagram.mmd "), "{seen:?}");
    assert!(!text_of(frames[0]).contains("1/1"), "{seen:?}");
    // Not the diagram's code: it is drawn.
    assert!(!text_of(frames[0]).contains("flowchart"), "{seen:?}");
    assert!(text_of(&seen).contains("nothing to reload"), "{seen:?}");
}

/// What a test types into the pager and when: the keys that `keys` returns
/// once `frames` frames are drawn, after it may have changed the files in
/// the scratch directory it is given.
struct Step<'a> {
    frames: usize,
    keys: &'a dyn Fn(&std::path::Path) -> &'static [u8],
}

/// kiri on `args` in a terminal of 5 rows, in a scratch directory called
/// `name`, with `steps` done in order.
fn in_steps(
    name: &str,
    args: &[&str],
    stdin: Option<&str>,
    steps: &[Step],
) -> (ExitStatus, String) {
    in_steps_after(name, |_| {}, args, stdin, steps)
}

/// [`in_steps`] on files `prepare` makes.
fn in_steps_with(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    steps: &[Step],
) -> (ExitStatus, String) {
    in_steps_after(name, prepare, args, None, steps)
}

fn in_steps_after(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    stdin: Option<&str>,
    steps: &[Step],
) -> (ExitStatus, String) {
    let dir = std::cell::RefCell::new(std::path::PathBuf::new());
    let prepare = |scratch: &std::path::Path| {
        *dir.borrow_mut() = scratch.to_owned();
        prepare(scratch);
    };
    let mut done = 0;
    in_terminal_after(name, prepare, args, 5, stdin, |frames, terminal, _| {
        if let Some(step) = steps.get(done).filter(|step| frames >= step.frames) {
            terminal.write_all((step.keys)(&dir.borrow())).unwrap();
            done += 1;
        }
    })
}

fn quit_after(frames: usize) -> Step<'static> {
    Step {
        frames,
        keys: &|_| b"q",
    }
}

#[test]
fn r_reads_the_file_again() {
    let rewrite = |dir: &std::path::Path| {
        let fresh: String = (1..=10).map(|n| format!("Fresh {n}\n\n")).collect();
        std::fs::write(dir.join("long.md"), fresh).unwrap();
        b"r" as &'static [u8]
    };
    // The first frame, then the one that says it is reloading; the one
    // of the new document is drawn before the pager reads the next key.
    let steps = [
        Step {
            frames: 1,
            keys: &rewrite,
        },
        quit_after(2),
    ];
    let (status, seen) = in_steps("pager-reload", &["long.md"], None, &steps);
    assert!(status.success());
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    assert!(frames[0].contains("Line 1"), "{seen:?}");
    assert!(text_of(frames[1]).contains("reloading"), "{seen:?}");
    let last = frames.last().unwrap();
    assert!(last.contains("Fresh 1"), "{seen:?}");
    assert!(text_of(last).contains("reloaded"), "{seen:?}");
}

#[test]
fn a_file_that_is_gone_stays_as_it_was_and_the_status_line_says_why() {
    let remove = |dir: &std::path::Path| {
        std::fs::remove_file(dir.join("long.md")).unwrap();
        b"r" as &'static [u8]
    };
    let steps = [
        Step {
            frames: 1,
            keys: &remove,
        },
        quit_after(2),
    ];
    let (status, seen) = in_steps("pager-reload-gone", &["long.md"], None, &steps);
    assert!(status.success());
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    let last = frames.last().unwrap();
    assert!(last.contains("Line 1"), "{seen:?}");
    assert!(text_of(last).contains("No such file"), "{seen:?}");
    assert!(!text_of(last).contains("reloaded"), "{seen:?}");
}

#[test]
fn standard_input_and_several_files_reload_nothing() {
    let reload = Step {
        frames: 1,
        keys: &|_| b"r",
    };
    let steps = [reload, quit_after(2)];
    let (status, seen) = in_steps("pager-reload-stdin", &["-p"], Some(&long()), &steps);
    assert!(status.success());
    assert!(text_of(&seen).contains("nothing to reload"), "{seen:?}");
    let args = ["long.md", "document.md"];
    let (status, seen) = in_steps("pager-reload-files", &args, None, &steps);
    assert!(status.success());
    assert!(text_of(&seen).contains("nothing to reload"), "{seen:?}");
}

/// The text of `long.md` written anew: ten paragraphs of `Fresh`.
fn fresh() -> String {
    (1..=10).map(|n| format!("Fresh {n}\n\n")).collect()
}

/// kiri following `long.md` with `args` in a terminal of 5 rows, with
/// `change` done to the scratch directory after the first frame and no
/// key until the second, after which it quits. Returns the frames.
fn following(name: &str, args: &[&str], change: &dyn Fn(&std::path::Path)) -> Vec<String> {
    let changed = |dir: &std::path::Path| {
        change(dir);
        b"" as &'static [u8]
    };
    let steps = [
        Step {
            frames: 1,
            keys: &changed,
        },
        quit_after(2),
    ];
    let (status, seen) = in_steps(name, args, None, &steps);
    assert!(status.success());
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).map(text_of).collect();
    assert!(frames[0].contains("Line 1"), "{seen:?}");
    assert!(frames[0].contains(" follow "), "{seen:?}");
    frames
}

#[test]
fn follow_reads_the_file_again_when_it_changes_without_a_key() {
    let rewrite = |dir: &std::path::Path| std::fs::write(dir.join("long.md"), fresh()).unwrap();
    for args in [&["--follow", "on", "long.md"][..], &["-f", "long.md"]] {
        let frames = following("pager-follow", args, &rewrite);
        let last = frames.last().unwrap();
        assert!(last.contains("Fresh 1"), "{args:?}: {frames:?}");
        assert!(last.contains("reloaded"), "{args:?}: {frames:?}");
        // Without a key, the pager does not say it is reloading.
        assert!(!frames.iter().any(|frame| frame.contains("reloading")));
    }
}

#[test]
fn follow_reads_a_file_replaced_by_a_rename() {
    // As editors save: a new file written beside it and moved over it.
    let replace = |dir: &std::path::Path| {
        std::fs::write(dir.join(".long.md.swp"), fresh()).unwrap();
        std::fs::rename(dir.join(".long.md.swp"), dir.join("long.md")).unwrap();
    };
    let frames = following("pager-follow-rename", &["-f", "long.md"], &replace);
    let last = frames.last().unwrap();
    assert!(last.contains("Fresh 1"), "{frames:?}");
    assert!(last.contains("reloaded"), "{frames:?}");
}

#[test]
fn follow_keeps_a_file_that_is_gone_and_the_status_line_says_why() {
    let remove = |dir: &std::path::Path| std::fs::remove_file(dir.join("long.md")).unwrap();
    let frames = following("pager-follow-gone", &["-f", "long.md"], &remove);
    let last = frames.last().unwrap();
    assert!(last.contains("Line 1"), "{frames:?}");
    assert!(last.contains("No such file"), "{frames:?}");
}

#[test]
fn standard_input_and_several_files_have_nothing_to_follow() {
    let steps = [quit_after(1)];
    let args = ["-p", "--follow", "on"];
    let (status, seen) = in_steps("pager-follow-stdin", &args, Some(&long()), &steps);
    assert!(status.success());
    assert!(text_of(&seen).contains("nothing to follow"), "{seen:?}");
    let args = ["-f", "long.md", "document.md"];
    let (status, seen) = in_steps("pager-follow-files", &args, None, &steps);
    assert!(status.success());
    assert!(text_of(&seen).contains("nothing to follow"), "{seen:?}");
    let args = ["-p", "--fa"];
    let (status, seen) = in_steps("pager-follow-auto-stdin", &args, Some(&long()), &steps);
    assert!(status.success());
    assert!(text_of(&seen).contains("nothing to follow"), "{seen:?}");
}

#[test]
fn following_from_the_configuration_file_is_quiet_where_there_is_nothing_to_follow() {
    let follow = |dir: &std::path::Path| {
        common::write(&dir.join("config/kiri/config.toml"), "follow = \"on\"\n");
    };
    let steps = [quit_after(1)];
    let (status, seen) = in_steps_after(
        "pager-follow-config-stdin",
        follow,
        &["-p"],
        Some(&long()),
        &steps,
    );
    assert!(status.success());
    assert!(text_of(&seen).contains("Line 1"), "{seen:?}");
    assert!(!text_of(&seen).contains("nothing to follow"), "{seen:?}");
    let args = ["long.md", "document.md"];
    let (status, seen) = in_steps_with("pager-follow-config-files", follow, &args, &steps);
    assert!(status.success());
    assert!(text_of(&seen).contains("long.md 1/2"), "{seen:?}");
    assert!(!text_of(&seen).contains("nothing to follow"), "{seen:?}");
}

#[test]
fn follow_takes_on_off_or_auto_and_one_flag_of_them() {
    let dir = common::Scratch::new("follow-values");
    for value in ["yes", "Auto"] {
        let out = command(&dir)
            .args(["--follow", value, "-"])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{value}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!("invalid value '{value}'")),
            "{stderr}"
        );
    }
    for flags in [
        ["-f", "-F"],
        ["--fa", "-F"],
        ["-f", "--fa"],
        ["--fa", "--follow=on"],
    ] {
        let out = command(&dir).args(flags).arg("-").output().unwrap();
        assert!(!out.status.success(), "{flags:?}");
    }
}

/// `frame` without its escape sequences.
fn text_of(frame: &str) -> String {
    let mut plain = String::new();
    let mut chars = frame.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            plain.push(c);
            continue;
        }
        // A CSI sequence ends at a letter; the graphics ones at the
        // string terminator.
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() || c == '\\' {
                break;
            }
        }
    }
    plain
}

/// The width in pixels of each image sent to the terminal in `seen`, in
/// the order they were sent.
fn sent_widths(seen: &str) -> Vec<String> {
    seen.split("\x1b_Ga=T,")
        .skip(1)
        .filter_map(|image| image.split(',').find(|key| key.starts_with("s=")))
        .map(str::to_owned)
        .collect()
}

/// A red PNG of `width` × 16 pixels at `path`, last modified at `time`.
fn png(path: &std::path::Path, width: u32, time: std::time::SystemTime) {
    image::RgbaImage::from_pixel(width, 16, image::Rgba([255, 0, 0, 255]))
        .save(path)
        .unwrap();
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(time).unwrap();
}

#[test]
fn a_reload_shows_an_image_whose_file_changed_and_keeps_the_others() {
    let long_ago = std::time::UNIX_EPOCH + Duration::from_secs(1000);
    let prepare = |dir: &std::path::Path| {
        png(&dir.join("a.png"), 16, long_ago);
        png(&dir.join("b.png"), 16, long_ago);
        let document = format!("![a](a.png)\n\n![b](b.png)\n\n{}", long());
        std::fs::write(dir.join("images.md"), document).unwrap();
    };
    // `a.png` is wider, and later than it was; `b.png` is wider too, but
    // as late as it was.
    let change = |dir: &std::path::Path| {
        png(&dir.join("a.png"), 32, std::time::SystemTime::now());
        png(&dir.join("b.png"), 32, long_ago);
        b"r" as &'static [u8]
    };
    let steps = [
        Step {
            frames: 1,
            keys: &change,
        },
        quit_after(2),
    ];
    let args = ["--mode", "kitty", "images.md"];
    let (status, seen) = in_steps_with("pager-reload-images", prepare, &args, &steps);
    assert!(status.success());
    // Both images are sent again to the new layout, in the order of the
    // document.
    let widths = sent_widths(&seen);
    assert_eq!(widths, ["s=20", "s=20", "s=40", "s=20"], "{seen:?}");
}

/// Writes `document` into the named pipe at `path` once a reader opens it,
/// on a thread of its own.
fn write_pipe(path: &std::path::Path, document: String) {
    let path = path.to_owned();
    std::thread::spawn(move || {
        let mut pipe = std::fs::File::options().write(true).open(path).unwrap();
        let _ = pipe.write_all(document.as_bytes());
    });
}

#[test]
fn a_named_pipe_reloads_nothing() {
    use std::os::unix::fs::OpenOptionsExt;

    let prepare = |dir: &std::path::Path| {
        let path = dir.join("pipe.md");
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `name` is a C string that lives through the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        write_pipe(&path, long());
    };
    // Another document waits in the pipe, should kiri open it again.
    let again = |dir: &std::path::Path| {
        write_pipe(&dir.join("pipe.md"), "Again\n".to_owned());
        b"r" as &'static [u8]
    };
    let quit = |dir: &std::path::Path| {
        // Lets the writer's open go through, should kiri not have opened
        // the pipe again.
        let _ = std::fs::File::options()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(dir.join("pipe.md"));
        b"q" as &'static [u8]
    };
    let steps = [
        Step {
            frames: 1,
            keys: &again,
        },
        Step {
            frames: 2,
            keys: &quit,
        },
    ];
    let (status, seen) = in_steps_with("pager-reload-fifo", prepare, &["pipe.md"], &steps);
    assert!(status.success());
    assert!(text_of(&seen).contains("nothing to reload"), "{seen:?}");
    assert!(!seen.contains("Again"), "{seen:?}");
}
