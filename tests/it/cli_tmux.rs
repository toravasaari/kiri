//! Printing through tmux: kiri asks kitty whether it holds the images it
//! sent, and sends again those tmux dropped on the way.

use std::io::Write;
use std::os::fd::BorrowedFd;
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::common::{Scratch, command, read_terminal, text};

/// The graphics query, wrapped for tmux, which kiri sends after its
/// checks: kitty's answer to it comes last.
const END: &str = "\x1bPtmux;\x1b\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\x1b\\\x1b\\";

/// The start of a check, wrapped for tmux, up to the image id.
const CHECK: &str = "\x1bPtmux;\x1b\x1b_Ga=p,U=1,i=";

/// The start of an image's transmission, wrapped for tmux, up to its id.
const SENT: &str = "\x1bPtmux;\x1b\x1b_Ga=T,U=1,q=2,f=32,o=z,i=";

/// The ids after each `prefix` in `seen`.
fn ids_after(seen: &str, prefix: &str) -> Vec<String> {
    (seen.split(prefix).skip(1))
        .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect())
        .collect()
}

/// Prints `documents` documents with one image each, `-P`, on a
/// terminal of its own, with `args`, while the terminal answers each round of checks with
/// what `answer` returns for its number, counting from 0, and the ids
/// checked; nothing when it returns `None`. Returns everything kiri
/// wrote, and how long it took.
fn printed(
    name: &str,
    documents: usize,
    args: &[&str],
    mut answer: impl FnMut(usize, &[String]) -> Option<String>,
) -> (String, Duration) {
    let dir = Scratch::new(name);
    image::RgbaImage::from_pixel(40, 40, image::Rgba([255, 0, 0, 255]))
        .save(dir.join("red.png"))
        .unwrap();
    let names: Vec<String> = (1..=documents)
        .map(|n| format!("document-{n}.md"))
        .collect();
    for name in &names {
        std::fs::write(dir.join(name), "Above\n\n![red](red.png)\n").unwrap();
    }
    let (mut terminal, tty) = crate::common::pty();
    let window = rustix::termios::Winsize {
        ws_col: 40,
        ws_row: 10,
        ws_xpixel: 400,
        ws_ypixel: 200,
    };
    rustix::termios::tcsetwinsize(&tty, window).unwrap();
    let mut kiri = command(&dir);
    kiri.current_dir(&*dir)
        .args(["--mode", "kitty", "-P"])
        .args(args)
        .args(&names)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::from(tty.try_clone().unwrap()));
    // SAFETY: setsid and ioctl are async-signal-safe and touch no memory.
    unsafe {
        kiri.pre_exec(|| {
            rustix::process::setsid()?;
            // Standard output is the terminal by now.
            rustix::process::ioctl_tiocsctty(BorrowedFd::borrow_raw(1))?;
            Ok(())
        });
    }
    let start = Instant::now();
    let mut child = kiri.spawn().unwrap();
    drop(kiri);
    let mut rounds = 0;
    let act = |seen: &[u8], terminal: &mut std::fs::File, _: &std::process::Child| {
        let seen = text(seen);
        let asked = seen.matches(END).count();
        while rounds < asked {
            let round = &seen[..nth_end(&seen, rounds)];
            let start = match rounds {
                0 => 0,
                _ => nth_end(&seen, rounds - 1),
            };
            let checked = ids_after(&round[start..], CHECK);
            if let Some(answer) = answer(rounds, &checked) {
                terminal.write_all(answer.as_bytes()).unwrap();
            }
            rounds += 1;
        }
    };
    let seen = read_terminal(&mut terminal, tty, &mut child, Duration::from_secs(20), act);
    assert!(child.wait().unwrap().success());
    (text(&seen), start.elapsed())
}

/// Where the `n`th round of checks ends in `seen`, counting from 0.
fn nth_end(seen: &str, n: usize) -> usize {
    let mut at = 0;
    for _ in 0..=n {
        at += seen[at..].find(END).unwrap() + END.len();
    }
    at
}

/// kitty's answer to a check for `id`: `OK` when it holds the image.
fn checked(id: &str, held: bool) -> String {
    let message = match held {
        true => "OK".to_owned(),
        false => format!("ENOENT:Put command refers to non-existent image with id: {id}"),
    };
    format!("\x1b_Gi={id},p=1263096393;{message}\x1b\\")
}

const ANSWERED: &str = "\x1b_Gi=31;OK\x1b\\";

#[test]
fn through_tmux_an_image_kitty_lost_is_sent_again() {
    let (seen, _) = printed("tmux-lost", 1, &["--passthrough", "tmux"], |round, ids| {
        let held = round > 0;
        let answers: String = ids.iter().map(|id| checked(id, held)).collect();
        Some(answers + ANSWERED)
    });
    let sent = ids_after(&seen, SENT);
    assert_eq!(sent.len(), 2, "{seen:?}");
    assert_eq!(sent[0], sent[1]);
    // Checked after each sending, and no more once kitty holds it.
    assert_eq!(ids_after(&seen, CHECK), [sent[0].clone(), sent[0].clone()]);
    // A transmission only: the placeholders are not written again.
    let placeholders = seen.matches('\u{10EEEE}').count();
    let resent = seen.rfind(SENT).unwrap();
    assert!(!seen[resent..].contains('\u{10EEEE}'), "{seen:?}");
    assert!(placeholders > 0);
}

#[test]
fn through_tmux_an_image_kitty_holds_is_sent_once() {
    let (seen, _) = printed("tmux-held", 1, &["--passthrough", "tmux"], |_, ids| {
        let answers: String = ids.iter().map(|id| checked(id, true)).collect();
        Some(answers + ANSWERED)
    });
    assert_eq!(ids_after(&seen, SENT).len(), 1, "{seen:?}");
    assert_eq!(seen.matches(END).count(), 1, "{seen:?}");
}

#[test]
fn through_tmux_an_image_kitty_never_holds_is_sent_a_few_times_at_most() {
    let (seen, _) = printed("tmux-never", 1, &["--passthrough", "tmux"], |_, ids| {
        let answers: String = ids.iter().map(|id| checked(id, false)).collect();
        Some(answers + ANSWERED)
    });
    // Sent once, and three times again.
    assert_eq!(ids_after(&seen, SENT).len(), 4, "{seen:?}");
}

#[test]
fn through_tmux_kiri_does_not_hang_when_kitty_does_not_answer() {
    // As when tmux shows the pane in no window.
    let (seen, took) = printed("tmux-silent", 1, &["--passthrough", "tmux"], |_, _| None);
    assert_eq!(ids_after(&seen, SENT).len(), 1, "{seen:?}");
    assert_eq!(seen.matches(END).count(), 1, "{seen:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn once_kitty_did_not_answer_nothing_more_is_checked() {
    // Each file would wait half a second for nothing.
    let (seen, took) = printed(
        "tmux-silent-files",
        3,
        &["--passthrough", "tmux"],
        |_, _| None,
    );
    assert_eq!(ids_after(&seen, SENT).len(), 3, "{seen:?}");
    assert_eq!(seen.matches(END).count(), 1, "{seen:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn kitty_is_asked_after_each_file_while_it_answers() {
    let (seen, _) = printed("tmux-files", 3, &["--passthrough", "tmux"], |_, ids| {
        let answers: String = ids.iter().map(|id| checked(id, true)).collect();
        Some(answers + ANSWERED)
    });
    assert_eq!(seen.matches(END).count(), 3, "{seen:?}");
}

#[test]
fn without_tmux_nothing_is_checked() {
    let (seen, _) = printed("tmux-none", 1, &["--passthrough", "none"], |_, _| None);
    assert!(seen.contains("\x1b_Ga=T,"), "{seen:?}");
    assert!(!seen.contains("a=p,U=1"), "{seen:?}");
    assert!(!seen.contains("\x1b_Gi=31,"), "{seen:?}");
}
