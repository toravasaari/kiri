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
    let (seen, answers) = with_tmux_and_sixel(|tty| ask(tty, false, GraphicsQuery::Wrapped, false));
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
    assert!(!probe.sixel);
    assert_eq!(probe.sixel_max_width, Some(1000));
    assert_eq!(probe.sixel_registers, Some(256));
}

#[test]
fn a_terminal_that_hangs_up_ends_the_reading_at_once() {
    // poll reports a stream at its end as readable, and a read then
    // returns no bytes, as it does from a hung-up terminal.
    let mut ended = File::open("/dev/null").unwrap();
    let mut answers = b"\x1b[?62;c".to_vec();
    let start = Instant::now();
    let result = read_until(&mut ended, &mut answers, 2, TIMEOUT);
    let waited = start.elapsed();
    assert!(waited < TIMEOUT / 2, "{waited:?}");
    // The answers heard stay, as after a timeout.
    assert_eq!(result, Some(()));
    assert_eq!(answers, b"\x1b[?62;c");
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
    use crate::term::testing::{TMUX_3_8, TMUX_3_8_SECOND_ROUND, size, with_terminal};
    let (seen, answers) = with_terminal(
        size(80, 24, 0, 0),
        "\x1b[c",
        |tty| ask(tty, false, GraphicsQuery::Skip, true),
        |_| {
            vec![
                vec![(Duration::ZERO, TMUX_3_8.to_owned())],
                vec![(Duration::ZERO, TMUX_3_8_SECOND_ROUND.to_owned())],
            ]
        },
    );
    assert!(seen.ends_with(&sixel_limits()), "{seen:?}");
    let answers = answers.unwrap();
    // tmux's to the first round and tmux's to the second.
    assert_eq!(device_attributes(&answers), 2);
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
