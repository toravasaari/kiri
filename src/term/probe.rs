//! Escape-sequence queries and their answers.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use rustix::process::{Pid, getpgrp};
use rustix::termios::tcgetpgrp;

use super::screen::RawMode;
use super::tmux;
use crate::present::answers::{PALETTE_ANSWER, device_attributes, sixel};
use crate::present::kitty::query;

/// Long enough for a terminal over SSH, short enough not to notice when
/// a terminal ignores the queries entirely.
pub(super) const TIMEOUT: Duration = Duration::from_millis(500);

/// Cursor position report.
const CURSOR_POSITION: &str = "\x1b[6n";
/// Foreground colour, OSC 10.
const FOREGROUND: &str = "\x1b]10;?\x1b\\";
/// Background colour, OSC 11.
const BACKGROUND: &str = "\x1b]11;?\x1b\\";
/// The terminal's name and version, XTVERSION.
const VERSION: &str = "\x1b[>q";
/// Primary device attributes.
const DEVICE_ATTRIBUTES: &str = "\x1b[c";

/// The widest and highest sixel the terminal takes, XTSMGRAPHICS "read
/// maximum" of item 2. "Read" would follow the window's size.
pub(crate) const SIXEL_GEOMETRY: &str = "\x1b[?2;4;0S";
/// The number of sixel colour registers, XTSMGRAPHICS "read" of item 1.
pub(crate) const SIXEL_REGISTERS: &str = "\x1b[?1;1;0S";
/// Sets the foreground to an RGB colour a 256-colour terminal would not
/// keep.
pub(crate) const TRUECOLOR_SET: &str = "\x1b[38;2;1;2;3m";
/// Asks for the graphic rendition, DECRQSS: a terminal with 24-bit colour
/// answers with the colour it was set.
pub(crate) const TRUECOLOR_ASK: &str = "\x1bP$qm\x1b\\";
/// Back to the default rendition after [`TRUECOLOR_SET`].
const TRUECOLOR_RESET: &str = "\x1b[m";

/// The 16 palette colours, OSC 4, one query each: not every terminal
/// answers more than one colour a query.
fn palette() -> String {
    (0..16)
        .map(|index| format!("{PALETTE_ANSWER}{index};?\x1b\\"))
        .collect()
}

/// How the probe sends the graphics protocol query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsQuery {
    /// As it is.
    Plain,
    /// Wrapped in tmux's passthrough, so that it reaches the terminal
    /// outside tmux.
    Wrapped,
    /// Not at all: tmux would take it for a pane title.
    Skip,
}

/// Cursor position reports around kitty's two text sizing tests, then a
/// graphics protocol query, the foreground, background and palette
/// colours, the terminal's version, whether a
/// colour set in 24 bits is kept, the cell size if `ask_cell_size`,
/// and primary device attributes. Every terminal answers the last one,
/// so its answer marks the end, also when a terminal ignores the
/// others. Behind tmux, tmux answers it at once, before kitty's answer to
/// the wrapped graphics query comes; the device attributes are then also
/// asked wrapped, from kitty, whose answer comes after its graphics
/// answer and marks the end. The cell size is asked wrapped there too:
/// tmux answers it from a pixel size it may only guess. The limits of
/// sixel are not asked here but in [`sixel_limits`], when the answer to
/// the device attributes lists sixel or sixel is forced: kitty prints a
/// parse error for each of them.
fn queries(ask_cell_size: bool, graphics: GraphicsQuery) -> String {
    let [width, scale] = query::text_sizing();
    let cell_size = if ask_cell_size { query::CELL_SIZE } else { "" };
    let (graphics, cell_size, end) = match graphics {
        GraphicsQuery::Plain => (query::GRAPHICS.to_owned(), cell_size, String::new()),
        GraphicsQuery::Wrapped => (
            tmux::passthrough(query::GRAPHICS),
            "",
            [cell_size, DEVICE_ATTRIBUTES]
                .into_iter()
                .filter(|query| !query.is_empty())
                .map(tmux::passthrough)
                .collect(),
        ),
        GraphicsQuery::Skip => (String::new(), cell_size, String::new()),
    };
    [
        "\r",
        CURSOR_POSITION,
        &width,
        CURSOR_POSITION,
        &scale,
        CURSOR_POSITION,
        &graphics,
        FOREGROUND,
        BACKGROUND,
        &palette(),
        VERSION,
        TRUECOLOR_SET,
        TRUECOLOR_ASK,
        TRUECOLOR_RESET,
        cell_size,
        DEVICE_ATTRIBUTES,
        &end,
    ]
    .concat()
}

/// The second round: the limits of sixel, and primary device attributes
/// again as the end, which every terminal answers, also when it ignores
/// the limits. Behind tmux this one is not wrapped: tmux answers it.
fn sixel_limits() -> String {
    [SIXEL_GEOMETRY, SIXEL_REGISTERS, DEVICE_ATTRIBUTES].concat()
}

/// Sends the queries on the controlling terminal and collects the
/// answers, asking for the cell size if `ask_cell_size`, and for the
/// limits of sixel whatever the device attributes say if
/// `always_ask_limits`, as with `--mode sixel`. `None` if the
/// terminal cannot be used or does not answer.
pub fn run(
    ask_cell_size: bool,
    graphics: GraphicsQuery,
    always_ask_limits: bool,
) -> Option<Vec<u8>> {
    ask(
        &controlling_terminal()?,
        ask_cell_size,
        graphics,
        always_ask_limits,
    )
}

/// The controlling terminal, open for reading and writing.
pub fn controlling_terminal() -> Option<File> {
    File::options().read(true).write(true).open("/dev/tty").ok()
}

/// Whether a process in the process group `own` is in the background of
/// a terminal whose foreground process group is `foreground`, if that
/// is known.
pub fn in_background(foreground: Option<Pid>, own: Pid) -> bool {
    foreground.is_some_and(|foreground| foreground != own)
}

/// Whether kiri runs in the background of `tty`, as in `kiri … &`.
/// There raw mode would stop it with SIGTTOU until it is brought to the
/// foreground.
pub fn runs_in_background(tty: &File) -> bool {
    in_background(tcgetpgrp(tty).ok(), getpgrp())
}

/// [`run`] on `tty`.
pub fn ask(
    tty: &File,
    ask_cell_size: bool,
    graphics: GraphicsQuery,
    always_ask_limits: bool,
) -> Option<Vec<u8>> {
    if runs_in_background(tty) {
        return None;
    }
    let raw = RawMode::enter(tty)?;
    let answers = exchange(&mut &*tty, ask_cell_size, graphics, always_ask_limits);
    // Erase whatever the OSC 66 tests drew before restoring the mode.
    let _ = (&*tty).write_all(query::erase_text_sizing_tests().as_bytes());
    drop(raw);
    answers
}

/// Reads into `answers` until it holds `wanted` device attributes
/// answers or `timeout` has passed. `None` if the terminal cannot be read.
fn read_until(
    tty: &mut (impl Read + AsFd),
    answers: &mut Vec<u8>,
    wanted: usize,
    timeout: Duration,
) -> Option<()> {
    let deadline = Instant::now() + timeout;
    let mut buffer = [0; 1024];
    while device_attributes(answers) < wanted {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return Some(());
        };
        if !super::readable(&*tty, left).ok()? {
            return Some(());
        }
        let read = tty.read(&mut buffer).ok()?;
        answers.extend_from_slice(&buffer[..read]);
    }
    Some(())
}

/// Sends the second round and reads until `wanted` device attributes
/// are in `answers`.
fn ask_limits(
    tty: &mut (impl Read + Write + AsFd),
    answers: &mut Vec<u8>,
    wanted: usize,
) -> Option<()> {
    tty.write_all(sixel_limits().as_bytes()).ok()?;
    tty.flush().ok()?;
    read_until(tty, answers, wanted, TIMEOUT)
}

/// Two rounds of queries. The first is [`queries`]. If its device
/// attributes list sixel, or `always_ask_limits` says that sixel is forced
/// and the terminal may not list it, a second asks for the limits of sixel
/// ([`sixel_limits`]), which a terminal without sixel, kitty among them,
/// would only log as errors. A terminal that answers no device attributes
/// at all gets no second round, forced or not. Behind tmux the second round
/// is not wrapped, so tmux answers it, limits included, whether or not the
/// terminal outside has sixel; its device attributes make the one more that
/// is counted. Both rounds wait for [`TIMEOUT`] at most, the
/// second from its start: a terminal that answered the first is there, and
/// answers the device attributes that end the second, so the wait is long
/// only for one that lists sixel and then goes silent.
fn exchange(
    tty: &mut (impl Read + Write + AsFd),
    ask_cell_size: bool,
    graphics: GraphicsQuery,
    always_ask_limits: bool,
) -> Option<Vec<u8>> {
    tty.write_all(queries(ask_cell_size, graphics).as_bytes())
        .ok()?;
    tty.flush().ok()?;
    let mut answers = Vec::new();
    // Behind tmux, kitty's answers count only when they come, as
    // passthrough may be off; tmux's own answers are there anyway.
    let (wanted, enough) = match graphics {
        GraphicsQuery::Wrapped => (2, 1),
        GraphicsQuery::Plain | GraphicsQuery::Skip => (1, 1),
    };
    read_until(tty, &mut answers, wanted, TIMEOUT)?;
    let heard = device_attributes(&answers);
    if heard < enough {
        return None;
    }
    // `sixel` reads only the first answers, so the second round's
    // device attributes change nothing there.
    if always_ask_limits || sixel(&String::from_utf8_lossy(&answers)) {
        // An error here leaves the first round's answers as they are,
        // as a timeout does.
        let _ = ask_limits(tty, &mut answers, heard + 1);
    }
    Some(answers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::present::answers::interpret;

    #[test]
    fn a_process_group_other_than_the_foreground_one_is_in_the_background() {
        let pid = |raw| rustix::process::Pid::from_raw(raw).unwrap();
        assert!(in_background(Some(pid(10)), pid(20)));
        assert!(!in_background(Some(pid(20)), pid(20)));
        // A terminal that is not the controlling one has no foreground
        // process group to compare with.
        assert!(!in_background(None, pid(20)));
    }

    #[test]
    fn sends_the_queries_in_order() {
        assert_eq!(
            queries(false, GraphicsQuery::Plain),
            [
                "\r\x1b[6n\x1b]66;w=2; \x1b\\\x1b[6n\x1b]66;s=2; \x1b\\\x1b[6n\
                 \x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b]10;?\x1b\\\x1b]11;?\x1b\\",
                &(0..16)
                    .map(|index| format!("\x1b]4;{index};?\x1b\\"))
                    .collect::<String>(),
                "\x1b[>q\x1b[38;2;1;2;3m\x1bP$qm\x1b\\\x1b[m\x1b[c",
            ]
            .concat()
        );
    }

    #[test]
    fn sends_the_graphics_query_wrapped_for_tmux() {
        let plain = queries(false, GraphicsQuery::Plain);
        let wrapped = "\x1bPtmux;\x1b\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\x1b\\\x1b\\";
        // tmux answers the device attributes itself, at once; kitty's
        // answer to the wrapped ones comes after its graphics answer.
        let expected = plain.replace(query::GRAPHICS, wrapped) + "\x1bPtmux;\x1b\x1b[c\x1b\\";
        assert_eq!(queries(false, GraphicsQuery::Wrapped), expected);
    }

    #[test]
    fn behind_tmux_the_cell_size_is_asked_from_the_terminal_outside() {
        // tmux answers it itself, from a pixel size it may not know.
        let plain = queries(false, GraphicsQuery::Plain);
        let wrapped = "\x1bPtmux;\x1b\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\x1b\\\x1b\\";
        let expected = plain.replace(query::GRAPHICS, wrapped)
            + "\x1bPtmux;\x1b\x1b[16t\x1b\\\x1bPtmux;\x1b\x1b[c\x1b\\";
        assert_eq!(queries(true, GraphicsQuery::Wrapped), expected);
    }

    #[test]
    fn skips_the_graphics_query_when_told() {
        let plain = queries(true, GraphicsQuery::Plain);
        let expected = plain.replace(query::GRAPHICS, "");
        assert_eq!(queries(true, GraphicsQuery::Skip), expected);
    }

    #[test]
    fn asks_for_the_cell_size_only_when_told() {
        let without = queries(false, GraphicsQuery::Plain);
        let end = without.len() - DEVICE_ATTRIBUTES.len();
        assert_eq!(
            queries(true, GraphicsQuery::Plain),
            [&without[..end], "\x1b[16t", DEVICE_ATTRIBUTES].concat()
        );
    }

    #[test]
    fn a_terminal_that_answers_the_sixel_and_colour_queries_is_heard() {
        use crate::term::testing::with_xterm;
        let (seen, answers) = with_xterm(|tty| ask(tty, false, GraphicsQuery::Plain, false));
        assert!(seen.contains(SIXEL_GEOMETRY), "{seen:?}");
        assert!(seen.contains(SIXEL_REGISTERS), "{seen:?}");
        assert!(seen.contains(TRUECOLOR_ASK), "{seen:?}");
        let probe = interpret(&answers.unwrap());
        assert!(probe.sixel);
        assert_eq!(probe.sixel_max_width, Some(1000));
        assert_eq!(probe.sixel_registers, Some(256));
        assert!(probe.truecolor);
    }

    /// Runs the probe on a pseudo-terminal whose other side answers
    /// like a kitty with cells of 9 × 19 pixels. Returns the queries the
    /// terminal saw and what the probe read.
    fn ask_kitty(ask_cell_size: bool) -> (String, Option<Vec<u8>>) {
        use crate::term::testing::{size, with_kitty};
        with_kitty(size(80, 24, 0, 0), |tty| {
            ask(tty, ask_cell_size, GraphicsQuery::Plain, false)
        })
    }

    #[test]
    fn behind_tmux_the_probe_waits_for_kittys_answers() {
        use crate::term::testing::with_tmux;
        let (seen, answers) = with_tmux(true, |tty| ask(tty, false, GraphicsQuery::Wrapped, false));
        assert!(seen.contains("\x1bPtmux;\x1b\x1b_G"), "{seen:?}");
        let probe = interpret(&answers.unwrap());
        assert!(probe.kitty_graphics);
    }

    #[test]
    fn behind_tmux_tmuxs_own_answers_count_when_kitty_does_not_answer() {
        use crate::term::testing::with_tmux;
        let (_, answers) = with_tmux(false, |tty| ask(tty, false, GraphicsQuery::Wrapped, false));
        let answers = String::from_utf8(answers.unwrap()).unwrap();
        assert!(answers.contains("\x1bP>|tmux 3.8"), "{answers:?}");
        assert!(!interpret(answers.as_bytes()).kitty_graphics);
    }

    #[test]
    fn a_terminal_that_lists_sixel_is_asked_for_the_limits_in_a_second_round() {
        use crate::term::testing::with_xterm;
        let (seen, answers) = with_xterm(|tty| ask(tty, false, GraphicsQuery::Plain, false));
        let limits = sixel_limits();
        assert!(seen.ends_with(&limits), "{seen:?}");
        assert_eq!(seen.matches(SIXEL_GEOMETRY).count(), 1, "{seen:?}");
        assert!(seen.contains(&format!("\x1b[m{DEVICE_ATTRIBUTES}{limits}")));
        assert_eq!(device_attributes(&answers.unwrap()), 2);
    }

    #[test]
    fn behind_tmux_a_terminal_that_lists_sixel_has_a_second_round_that_tmux_ends() {
        use crate::term::testing::with_tmux_and_sixel;
        let (seen, answers) =
            with_tmux_and_sixel(|tty| ask(tty, false, GraphicsQuery::Wrapped, false));
        assert!(seen.ends_with(&sixel_limits()), "{seen:?}");
        let answers = answers.unwrap();
        // tmux's, the terminal's and tmux's again to the second round.
        assert_eq!(device_attributes(&answers), 3);
        let probe = interpret(&answers);
        assert!(probe.sixel);
        // The limits are tmux's own, not the terminal's.
        assert_eq!(
            (probe.sixel_max_width, probe.sixel_registers),
            (None, Some(1024))
        );
    }

    #[test]
    fn behind_tmux_without_sixel_in_both_answers_there_is_no_second_round() {
        use crate::term::testing::with_tmux;
        let (seen, answers) = with_tmux(true, |tty| ask(tty, false, GraphicsQuery::Wrapped, false));
        assert!(!seen.contains(SIXEL_GEOMETRY), "{seen:?}");
        assert_eq!(device_attributes(&answers.unwrap()), 2);
    }

    #[test]
    fn a_terminal_that_lists_sixel_and_ignores_the_limits_does_not_hold_up_startup() {
        use crate::term::testing::with_mute_sixel_terminal;
        let start = Instant::now();
        let (seen, answers) =
            with_mute_sixel_terminal(|tty| ask(tty, false, GraphicsQuery::Plain, false));
        let waited = start.elapsed();
        assert!(seen.contains(SIXEL_GEOMETRY), "{seen:?}");
        assert!(waited >= TIMEOUT, "{waited:?}");
        assert!(waited < TIMEOUT * 2, "{waited:?}");
        let probe = interpret(&answers.unwrap());
        assert!(probe.sixel);
        assert_eq!((probe.sixel_max_width, probe.sixel_registers), (None, None));
    }

    /// A terminal whose writes fail after the first one: the queries of
    /// the first round go through, those of the second do not.
    struct FailsAfterTheFirstWrite<'a> {
        tty: &'a File,
        writes: usize,
    }

    impl Read for FailsAfterTheFirstWrite<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.tty.read(buffer)
        }
    }

    impl Write for FailsAfterTheFirstWrite<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.writes += 1;
            if self.writes > 1 {
                return Err(std::io::ErrorKind::BrokenPipe.into());
            }
            self.tty.write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.tty.flush()
        }
    }

    impl AsFd for FailsAfterTheFirstWrite<'_> {
        fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
            self.tty.as_fd()
        }
    }

    #[test]
    fn an_error_in_the_second_round_keeps_the_answers_of_the_first() {
        use crate::term::testing::with_xterm;
        let (seen, answers) = with_xterm(|tty| {
            let _raw = RawMode::enter(tty).unwrap();
            let mut broken = FailsAfterTheFirstWrite { tty, writes: 0 };
            exchange(&mut broken, false, GraphicsQuery::Plain, false)
        });
        assert!(!seen.contains(SIXEL_GEOMETRY), "{seen:?}");
        assert!(
            answers.is_some(),
            "the first round's answers are lost: {seen:?}"
        );
        let probe = interpret(&answers.unwrap());
        assert!(probe.sixel);
        assert!(probe.truecolor);
        assert_eq!((probe.sixel_max_width, probe.sixel_registers), (None, None));
    }

    #[test]
    fn a_terminal_without_sixel_is_not_asked_for_sixel_limits() {
        let (seen, answers) = ask_kitty(false);
        assert!(!seen.contains(SIXEL_GEOMETRY), "{seen:?}");
        assert!(!seen.contains(SIXEL_REGISTERS), "{seen:?}");
        assert!(!interpret(&answers.unwrap()).sixel);
    }

    #[test]
    fn forced_sixel_asks_for_the_limits_of_a_terminal_that_does_not_list_sixel() {
        use crate::term::testing::{XTERM_407_LIMITS, with_unlisted_sixel_terminal};
        let (seen, answers) = with_unlisted_sixel_terminal(XTERM_407_LIMITS, |tty| {
            ask(tty, false, GraphicsQuery::Plain, true)
        });
        assert!(seen.ends_with(&sixel_limits()), "{seen:?}");
        assert_eq!(seen.matches(SIXEL_GEOMETRY).count(), 1, "{seen:?}");
        let answers = answers.unwrap();
        assert_eq!(device_attributes(&answers), 2);
        let probe = interpret(&answers);
        assert_eq!(probe.sixel_max_width, Some(1000));
        assert_eq!(probe.sixel_registers, Some(256));
    }

    #[test]
    fn a_terminal_that_does_not_list_sixel_is_not_asked_unless_forced() {
        use crate::term::testing::{XTERM_407_LIMITS, with_unlisted_sixel_terminal};
        let (seen, answers) = with_unlisted_sixel_terminal(XTERM_407_LIMITS, |tty| {
            ask(tty, false, GraphicsQuery::Plain, false)
        });
        assert!(!seen.contains(SIXEL_GEOMETRY), "{seen:?}");
        assert_eq!(device_attributes(&answers.unwrap()), 1);
    }

    #[test]
    fn forced_sixel_on_kitty_asks_both_limits_and_ends_on_the_device_attributes() {
        use crate::term::testing::with_kitty_and_a_second_round;
        let (seen, (answers, waited)) = with_kitty_and_a_second_round(|tty| {
            let start = Instant::now();
            let answers = ask(tty, false, GraphicsQuery::Plain, true);
            (answers, start.elapsed())
        });
        assert!(waited < TIMEOUT, "{waited:?}");
        assert!(seen.ends_with(&sixel_limits()), "{seen:?}");
        let answers = answers.unwrap();
        assert_eq!(device_attributes(&answers), 2);
        let probe = interpret(&answers);
        assert!(!probe.sixel);
        assert_eq!((probe.sixel_max_width, probe.sixel_registers), (None, None));
    }

    #[test]
    fn behind_tmux_forced_sixel_takes_tmuxs_limits_and_counts_its_end() {
        use crate::term::testing::{
            KITTY_BEHIND_TMUX, TMUX_3_8, TMUX_3_8_SECOND_ROUND, size, with_terminal,
        };
        let (seen, answers) = with_terminal(
            size(80, 24, 0, 0),
            "\x1bPtmux;\x1b\x1b[c\x1b\\",
            |tty| ask(tty, false, GraphicsQuery::Wrapped, true),
            |_| {
                vec![
                    vec![
                        (Duration::ZERO, TMUX_3_8.to_owned()),
                        (Duration::from_millis(50), KITTY_BEHIND_TMUX.to_owned()),
                    ],
                    vec![(Duration::ZERO, TMUX_3_8_SECOND_ROUND.to_owned())],
                ]
            },
        );
        assert!(seen.ends_with(&sixel_limits()), "{seen:?}");
        let answers = answers.unwrap();
        // tmux's, the terminal's and tmux's again to the second round.
        assert_eq!(device_attributes(&answers), 3);
        let probe = interpret(&answers);
        assert_eq!(
            (probe.sixel_max_width, probe.sixel_registers),
            (None, Some(1024))
        );
    }

    #[test]
    fn a_terminal_tells_its_cell_size_when_asked() {
        let (seen, answers) = ask_kitty(true);
        assert!(seen.contains("\x1b[16t"), "{seen:?}");
        let probe = interpret(&answers.unwrap());
        assert_eq!(probe.cell_pixels, Some((9, 19)));
        assert!(probe.kitty_graphics);
    }

    #[test]
    fn the_cell_size_is_not_asked_unless_told() {
        let (seen, answers) = ask_kitty(false);
        assert!(!seen.contains("\x1b[16t"), "{seen:?}");
        assert_eq!(interpret(&answers.unwrap()).cell_pixels, None);
    }
}
