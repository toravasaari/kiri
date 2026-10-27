//! Asking kitty, behind tmux, whether it holds the images kiri sent: tmux
//! drops output for a client that cannot keep up, and the images in it
//! with it.

use std::fs::File;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use super::probe::{self, TIMEOUT};
use super::screen::RawMode;
use super::tmux::Wrapping;
use crate::kitty::graphics::{self, Cells, ImageId};
use crate::kitty::query;

/// The images of `sent` that kitty, behind tmux, does not hold, asked on
/// the controlling terminal. kiri waits for kitty's answers as the probe
/// does, [`TIMEOUT`] at most after the last that came. `None` when kitty
/// does not answer at all, as when tmux shows the pane in no window, and
/// when it cannot be asked, as when kiri runs in the background.
pub fn lost_images(sent: &[(ImageId, Cells)]) -> Option<Vec<ImageId>> {
    if sent.is_empty() {
        return Some(Vec::new());
    }
    let tty = probe::controlling_terminal()?;
    if probe::runs_in_background(&tty) {
        return None;
    }
    let raw = RawMode::enter(&tty)?;
    let answers = ask(&tty, sent, TIMEOUT);
    drop(raw);
    lost(&answers, sent)
}

/// Asks kitty through `tty`, in raw mode, wrapped for tmux, whether it
/// holds the images of `sent`, and then the graphics query, whose answer
/// comes last: kitty's answers, waiting `timeout` at most after each.
fn ask(tty: &File, sent: &[(ImageId, Cells)], timeout: Duration) -> Vec<u8> {
    let mut answers = Vec::new();
    let mut out = Wrapping::new(tty, true);
    let asked = (sent.iter())
        .try_for_each(|&(id, cells)| graphics::check(&mut out, id, cells))
        .and_then(|()| out.write_all(query::GRAPHICS.as_bytes()))
        .and_then(|()| out.flush());
    if asked.is_err() {
        return answers;
    }
    let mut buffer = [0; 1024];
    let mut deadline = Instant::now() + timeout;
    while !ended(&answers) {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        match super::readable(tty, left) {
            Ok(true) => {}
            _ => break,
        }
        match (&*tty).read(&mut buffer) {
            Ok(read) if read > 0 => answers.extend_from_slice(&buffer[..read]),
            _ => break,
        }
        deadline = Instant::now() + timeout;
    }
    answers
}

/// Whether `answers` hold kitty's answer to the graphics query, its last.
fn ended(answers: &[u8]) -> bool {
    let end = query::GRAPHICS_ANSWER.as_bytes();
    answers.windows(end.len()).any(|window| window == end)
}

/// The images of `sent` that `answers` tell kitty does not hold: those
/// it answered so, and, when its answer to the graphics query came, also
/// those it did not answer, as tmux dropped what asked about them.
/// `None` when kitty did not answer at all.
fn lost(answers: &[u8], sent: &[(ImageId, Cells)]) -> Option<Vec<ImageId>> {
    let checked: Vec<(ImageId, bool)> =
        (sequences(answers)).filter_map(graphics::checked).collect();
    let all = ended(answers);
    if checked.is_empty() && !all {
        return None;
    }
    let lost = (sent.iter())
        .map(|&(id, _)| id)
        .filter(
            |id| match checked.iter().find(|(checked, _)| checked == id) {
                Some(&(_, held)) => !held,
                None => all,
            },
        )
        .collect();
    Some(lost)
}

/// The APC sequences in `answers`, each from `ESC _` to `ESC \`.
fn sequences(answers: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = answers;
    std::iter::from_fn(move || {
        let start = rest.windows(2).position(|pair| pair == b"\x1b_")?;
        let end = rest[start..]
            .windows(2)
            .position(|pair| pair == b"\x1b\\")?;
        let sequence = &rest[start..start + end + 2];
        rest = &rest[start + end + 2..];
        Some(sequence)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::testing::{size, with_terminal};

    /// Image `id` as kitty answers for it.
    fn image(id: u32) -> ImageId {
        let answer = format!("\x1b_Gi={id},p=1263096393;OK\x1b\\");
        graphics::checked(answer.as_bytes()).unwrap().0
    }

    fn sent(ids: &[u32]) -> Vec<(ImageId, Cells)> {
        let cells = Cells {
            columns: 4,
            rows: 2,
        };
        ids.iter().map(|&id| (image(id), cells)).collect()
    }

    const END: &str = "\x1b_Gi=31;OK\x1b\\";

    fn held(id: u32) -> String {
        format!("\x1b_Gi={id},p=1263096393;OK\x1b\\")
    }

    fn missing(id: u32) -> String {
        format!("\x1b_Gi={id},p=1263096393;ENOENT:Put command refers to non-existent image\x1b\\")
    }

    #[test]
    fn the_images_kitty_does_not_hold_are_lost() {
        let answers = [held(1), missing(2), held(3), END.into()].concat();
        let lost = lost(answers.as_bytes(), &sent(&[1, 2, 3]));
        assert_eq!(lost, Some(vec![image(2)]));
    }

    #[test]
    fn with_all_the_answers_in_an_image_without_one_is_lost() {
        // tmux dropped the check with the image.
        let answers = [held(1), END.into()].concat();
        let lost = lost(answers.as_bytes(), &sent(&[1, 2]));
        assert_eq!(lost, Some(vec![image(2)]));
    }

    #[test]
    fn without_the_last_answer_only_what_kitty_said_counts() {
        let answers = [held(1), missing(2)].concat();
        let lost_one = lost(answers.as_bytes(), &sent(&[1, 2, 3]));
        assert_eq!(lost_one, Some(vec![image(2)]));
        // No answer at all, as from a pane tmux does not show, tells
        // nothing.
        assert_eq!(lost(b"", &sent(&[1, 2])), None);
        assert_eq!(lost(b"\x1b[?62;c", &sent(&[1, 2])), None);
        // Only the last answer: tmux dropped every check.
        assert_eq!(lost(END.as_bytes(), &sent(&[1])), Some(vec![image(1)]));
    }

    #[test]
    fn kitty_is_asked_through_tmux_and_its_answers_read() {
        let checks = sent(&[1, 2]);
        let last = "\x1bPtmux;\x1b\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\x1b\\\x1b\\";
        let (seen, answers) = with_terminal(
            size(80, 24, 0, 0),
            last,
            |tty| {
                let _raw = RawMode::enter(tty).unwrap();
                ask(tty, &checks, Duration::from_secs(2))
            },
            |_| {
                let wait = Duration::from_millis(20);
                vec![(wait, held(1)), (wait, missing(2)), (wait, END.into())]
            },
        );
        let check = "\x1bPtmux;\x1b\x1b_Ga=p,U=1,i=1,p=1263096393,c=4,r=2\x1b\x1b\\\x1b\\\
                     \x1bPtmux;\x1b\x1b_Ga=d,d=i,i=1,p=1263096393,q=2\x1b\x1b\\\x1b\\";
        assert!(seen.starts_with(check), "{seen:?}");
        assert!(seen.ends_with(last), "{seen:?}");
        let expected = [held(1), missing(2), END.into()].concat();
        assert_eq!(String::from_utf8(answers).unwrap(), expected);
    }

    #[test]
    fn kiri_does_not_wait_long_for_a_kitty_that_does_not_answer() {
        let (_, (answers, took)) = with_terminal(
            size(80, 24, 0, 0),
            "never",
            |tty| {
                let start = Instant::now();
                let answers = ask(tty, &sent(&[1]), Duration::from_millis(100));
                (answers, start.elapsed())
            },
            |_| Vec::new(),
        );
        assert!(answers.is_empty());
        assert!(took < Duration::from_secs(1), "{took:?}");
    }
}
