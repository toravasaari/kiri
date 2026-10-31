//! Pseudo-terminals for the tests.

use std::fs::File;
use std::time::Duration;

pub(crate) use super::pty::pty;

/// A window size for [`rustix::termios::tcsetwinsize`].
pub(crate) fn size(columns: u16, rows: u16, width: u16, height: u16) -> rustix::termios::Winsize {
    rustix::termios::Winsize {
        ws_col: columns,
        ws_row: rows,
        ws_xpixel: width,
        ws_ypixel: height,
    }
}

/// What kitty 0.47.1 answers to the probe, one answer after the other as
/// it sends them: the cursor reports of the text sizing tests, the
/// graphics query's `OK`, the colours (foreground and background, and
/// blue of the palette), its name and version, `cell_size` if the probe
/// asked for it, and the device attributes that end the answers. The
/// cells are 9 × 19 pixels.
pub(crate) fn kitty_0_47_1(cell_size: bool) -> String {
    let cell_size = if cell_size { "\x1b[6;19;9t" } else { "" };
    format!(
        "\x1b[5;1R\x1b[5;3R\x1b[5;5R\x1b_Gi=31;OK\x1b\\\
         \x1b]10;rgb:dddd/dddd/dddd\x1b\\\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\\
         \x1b]4;4;rgb:0000/0000/eeee\x1b\\\
         \x1bP>|kitty(0.47.1)\x1b\\\
         {cell_size}\x1b[?62;c"
    )
}

/// What Ghostty 1.1.3 answers to the probe, in the order it sends them:
/// it ignores the text sizing tests, so each cursor report is the same,
/// and it answers the graphics query. The colours are white on a dark
/// grey, the cells 9 × 20 pixels (told when `cell_size`), and the
/// palette is left out.
pub(crate) fn ghostty_1_1_3(cell_size: bool) -> String {
    let cell_size = if cell_size { "\x1b[6;20;9t" } else { "" };
    format!(
        "\x1b[1;1R\x1b[1;1R\x1b[1;1R\x1b_Gi=31;OK\x1b\\\
         \x1b]10;rgb:ffff/ffff/ffff\x1b\\\x1b]11;rgb:2828/2c2c/3434\x1b\\\
         \x1bP>|ghostty 1.1.3-1.fc43\x1b\\\
         {cell_size}\x1b[?62;22c"
    )
}

/// What tmux 3.8 answers by itself to the probe: cursor reports, its name
/// and device attributes, and none of the terminal's colours.
pub(crate) const TMUX_3_8: &str = "\x1b[1;1R\x1b[1;1R\x1b[1;1R\x1bP>|tmux 3.8\x1b\\\x1b[?1;2;4c";

/// What kitty 0.47.1 behind tmux answers to the graphics query that came
/// through tmux's passthrough, which reaches the probe after tmux's own
/// answers.
pub(crate) const KITTY_BEHIND_TMUX: &str = "\x1b_Gi=31;OK\x1b\\\x1b[?62;52;c";

/// Runs `probe` on a pseudo-terminal with a window of `window`, whose
/// other side answers like a kitty 0.47.1 with cells of 9 × 19 pixels,
/// and tells the cell size only when asked, as [`kitty_0_47_1`] does.
/// Returns the queries the terminal saw and what `probe` returned. The
/// terminal waits for the queries for 2 s at most, so a probe that sends
/// none fails its test instead of hanging it.
pub(crate) fn with_kitty<T>(
    window: rustix::termios::Winsize,
    probe: impl FnOnce(&File) -> T,
) -> (String, T) {
    with_terminal(window, "\x1b[c", probe, |seen| {
        let answers = kitty_0_47_1(seen.contains("\x1b[16t"));
        vec![(Duration::ZERO, answers)]
    })
}

/// [`with_kitty`], for a Ghostty 1.1.3, as [`ghostty_1_1_3`] answers.
pub(crate) fn with_ghostty<T>(
    window: rustix::termios::Winsize,
    probe: impl FnOnce(&File) -> T,
) -> (String, T) {
    with_terminal(window, "\x1b[c", probe, |seen| {
        let answers = ghostty_1_1_3(seen.contains("\x1b[16t"));
        vec![(Duration::ZERO, answers)]
    })
}

/// Runs `probe` on a pseudo-terminal like [`with_kitty`], whose other side
/// answers like tmux 3.8 in a kitty 0.47.1: tmux's own answers at once
/// ([`TMUX_3_8`]), and kitty's ([`KITTY_BEHIND_TMUX`]) to the queries
/// wrapped in passthrough 50 ms later, if `kitty_answers`, as they come
/// back through tmux.
pub(crate) fn with_tmux<T>(kitty_answers: bool, probe: impl FnOnce(&File) -> T) -> (String, T) {
    let window = size(80, 24, 0, 0);
    with_terminal(window, "\x1bPtmux;\x1b\x1b[c\x1b\\", probe, move |_| {
        let mut answers = vec![(Duration::ZERO, TMUX_3_8.to_owned())];
        if kitty_answers {
            let kitty = KITTY_BEHIND_TMUX.to_owned();
            answers.push((Duration::from_millis(50), kitty));
        }
        answers
    })
}

/// Runs `probe` on a pseudo-terminal with a window of `window`, whose
/// other side reads until the queries end with `last`, then writes what
/// `answer` returns for them, each part after its wait. Returns the
/// queries the terminal saw and what `probe` returned. The terminal waits
/// for the queries for 2 s at most and then does not answer.
pub(crate) fn with_terminal<T>(
    window: rustix::termios::Winsize,
    last: &'static str,
    probe: impl FnOnce(&File) -> T,
    answer: impl FnOnce(&str) -> Vec<(Duration, String)> + Send + 'static,
) -> (String, T) {
    use std::io::{Read, Write};
    use std::time::Instant;

    use rustix::event::{PollFd, PollFlags, Timespec, poll};

    let (mut terminal, tty) = pty();
    rustix::termios::tcsetwinsize(&tty, window).unwrap();
    let kitty = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut seen = Vec::new();
        let mut buffer = [0; 1024];
        while !seen.ends_with(last.as_bytes()) {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return (String::from_utf8(seen).unwrap(), terminal);
            };
            let timeout = Timespec::try_from(left).unwrap();
            let mut fds = [PollFd::new(&terminal, PollFlags::IN)];
            if poll(&mut fds, Some(&timeout)).unwrap() == 0 {
                continue;
            }
            let read = terminal.read(&mut buffer).unwrap();
            seen.extend_from_slice(&buffer[..read]);
        }
        let seen = String::from_utf8(seen).unwrap();
        for (wait, part) in answer(&seen) {
            std::thread::sleep(wait);
            terminal.write_all(part.as_bytes()).unwrap();
        }
        (seen, terminal)
    });
    let result = probe(&tty);
    let (seen, _terminal) = kitty.join().unwrap();
    (seen, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::present::answers::interpret;
    use crate::typeset::color::Color;

    #[test]
    fn the_fake_kitty_gives_up_when_it_is_not_asked() {
        let (seen, ()) = with_kitty(size(80, 24, 0, 0), |_| ());
        assert_eq!(seen, "");
    }

    #[test]
    fn kittys_answers_tell_text_sizing_placeholders_colours_and_cells() {
        let probe = interpret(kitty_0_47_1(true).as_bytes());
        assert!(probe.text_sizing);
        assert!(probe.graphics);
        assert!(probe.image_placeholders);
        assert_eq!(probe.cell_pixels, Some((9, 19)));
        assert_eq!(probe.foreground, Some(Color::Rgb(0xdd, 0xdd, 0xdd)));
        assert_eq!(probe.dark_background, Some(true));
        assert_eq!(interpret(kitty_0_47_1(false).as_bytes()).cell_pixels, None);
    }

    #[test]
    fn ghosttys_answers_tell_images_without_text_sizing() {
        let probe = interpret(ghostty_1_1_3(true).as_bytes());
        assert!(!probe.text_sizing);
        assert!(probe.graphics);
        assert!(probe.image_placeholders);
        assert_eq!(probe.cell_pixels, Some((9, 20)));
        assert_eq!(probe.dark_background, Some(true));
        assert_eq!(interpret(ghostty_1_1_3(false).as_bytes()).cell_pixels, None);
    }

    #[test]
    fn tmuxs_answers_alone_tell_no_graphics_and_kittys_behind_it_do() {
        let alone = interpret(TMUX_3_8.as_bytes());
        assert!(!alone.graphics);
        assert!(!alone.text_sizing);
        let both = interpret(format!("{TMUX_3_8}{KITTY_BEHIND_TMUX}").as_bytes());
        assert!(both.graphics);
    }

    #[test]
    fn the_fake_ghostty_answers_the_probe_as_the_fixture_does() {
        let (seen, answered) = with_ghostty(size(80, 24, 0, 0), |tty| {
            crate::term::probe::ask(tty, true, crate::term::probe::GraphicsQuery::Plain)
        });
        assert!(seen.contains("\x1b[16t"), "{seen:?}");
        let answered = String::from_utf8(answered.unwrap()).unwrap();
        assert_eq!(answered, ghostty_1_1_3(true));
    }
}
