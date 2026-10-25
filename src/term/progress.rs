//! The line that tells how far loading a document's images has got,
//! `kiri: loading images 2/5…`, while it takes long enough for the
//! reader to wonder: with mermaid-cli, a second or more per diagram.

use std::io::Write;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long drawing may take before the line shows.
pub const DELAY: Duration = Duration::from_millis(500);

/// How far the work has got: `done` of `total`, and whether it is over.
#[derive(Debug, Default)]
struct State {
    done: usize,
    total: usize,
    finished: bool,
}

/// What the work tells the line.
#[derive(Debug, Default)]
pub struct Progress {
    state: Mutex<State>,
    changed: Condvar,
}

impl Progress {
    /// `done` of `total` images have loaded.
    pub fn update(&self, done: usize, total: usize) {
        let mut state = self.lock();
        state.done = done;
        state.total = total;
        self.changed.notify_all();
    }

    fn finish(&self) {
        self.lock().finished = true;
        self.changed.notify_all();
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Writes the line to `out` from when `delay` has passed until the
    /// work is finished, and then wipes it.
    fn show(&self, out: &mut impl Write, delay: Duration) {
        let start = Instant::now();
        let mut shown = None;
        let mut state = self.lock();
        while !state.finished {
            let waited = start.elapsed();
            if waited < delay {
                state = (self.changed.wait_timeout(state, delay - waited))
                    .unwrap_or_else(|error| error.into_inner())
                    .0;
                continue;
            }
            let text = (state.total > 0).then(|| line(state.done, state.total));
            if text.is_some() && text != shown {
                drop(state);
                // At the start of the line, over what was there.
                let _ = write!(out, "\r\x1b[2K{}", text.as_deref().unwrap_or(""));
                let _ = out.flush();
                shown = text;
                state = self.lock();
                continue;
            }
            state = (self.changed.wait(state)).unwrap_or_else(|error| error.into_inner());
        }
        drop(state);
        if shown.is_some() {
            let _ = write!(out, "\r\x1b[2K");
            let _ = out.flush();
        }
    }
}

/// The line for `done` of `total` images.
pub fn line(done: usize, total: usize) -> String {
    format!("kiri: loading images {done}/{total}…")
}

/// Runs `work`, which tells a `Progress` how far it has got. Once it has
/// run for `delay`, the line shows on `out`, a terminal, and changes as
/// the work goes on; it is wiped before `work` returns.
pub fn while_working<T>(
    mut out: impl Write + Send,
    delay: Duration,
    work: impl FnOnce(&Progress) -> T,
) -> T {
    let progress = Progress::default();
    std::thread::scope(|scope| {
        let shower = scope.spawn(|| progress.show(&mut out, delay));
        // Finished also when `work` panics: the scope waits for the line's
        // thread before it passes the panic on.
        let finish = Finish(&progress);
        let result = work(&progress);
        drop(finish);
        let _ = shower.join();
        result
    })
}

/// Waits with `wait`, which waits at most the time it is given and tells
/// whether what it waits for is over, for `limit` at most. When that
/// takes longer than `delay`, `text` shows on `out`, a terminal, until it
/// is over, and is then wiped.
pub fn while_waiting(
    mut out: impl Write,
    delay: Duration,
    limit: Duration,
    text: &str,
    mut wait: impl FnMut(Duration) -> bool,
) {
    if wait(delay.min(limit)) || limit <= delay {
        return;
    }
    let _ = write!(out, "\r\x1b[2K{text}");
    let _ = out.flush();
    wait(limit - delay);
    let _ = write!(out, "\r\x1b[2K");
    let _ = out.flush();
}

/// What the line says after the count where Space or Enter opens the
/// pager before the work is over.
pub const HINT: &str = " (space: open now)";

/// How long [`until_done_or_skipped`] waits for a key before it asks
/// again how far the work has got.
const STEP: Duration = Duration::from_millis(50);

/// Waits until `poll` tells that the work is over, `None`, or `skip`,
/// which waits for a key at most the time it is given, tells that the
/// reader pressed Space or Enter. Meanwhile `poll` tells how far the work
/// has got, done of total, which the line shows on `out`, a terminal, with
/// [`HINT`], once `delay` has passed; it is wiped before this returns.
/// Whether the reader skipped the rest of the wait.
pub fn until_done_or_skipped(
    mut out: impl Write,
    delay: Duration,
    mut poll: impl FnMut() -> Option<(usize, usize)>,
    mut skip: impl FnMut(Duration) -> bool,
) -> bool {
    let start = Instant::now();
    let mut shown = None;
    let skipped = loop {
        let Some((done, total)) = poll() else {
            break false;
        };
        if start.elapsed() >= delay && total > 0 {
            let text = format!("{}{HINT}", line(done, total));
            if shown.as_ref() != Some(&text) {
                // At the start of the line, over what was there.
                let _ = write!(out, "\r\x1b[2K{text}");
                let _ = out.flush();
                shown = Some(text);
            }
        }
        if skip(STEP) {
            break true;
        }
    };
    if shown.is_some() {
        let _ = write!(out, "\r\x1b[2K");
        let _ = out.flush();
    }
    skipped
}

/// Tells the line that the work is over when dropped.
struct Finish<'a>(&'a Progress);

impl Drop for Finish<'_> {
    fn drop(&mut self) {
        self.0.finish();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// Output that the test can read while it is written.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Shared {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }

        /// Waits until the output has `text` in it, for at most 5 s.
        fn wait_for(&self, text: &str) {
            let start = Instant::now();
            while !self.text().contains(text) {
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "{:?}",
                    self.text()
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    const WIPE: &str = "\r\x1b[2K";

    #[test]
    fn the_line_counts_the_images() {
        assert_eq!(line(2, 5), "kiri: loading images 2/5…");
    }

    #[test]
    fn shows_nothing_for_work_done_in_time() {
        let out = Shared::default();
        let result = while_working(out.clone(), Duration::from_secs(3600), |progress| {
            progress.update(0, 2);
            progress.update(1, 2);
            progress.update(2, 2);
            "done"
        });
        assert_eq!(result, "done");
        assert_eq!(out.text(), "");
    }

    #[test]
    fn shows_the_count_once_the_delay_has_passed_and_wipes_it() {
        let out = Shared::default();
        while_working(out.clone(), Duration::ZERO, |progress| {
            progress.update(0, 3);
            out.wait_for("0/3");
            progress.update(2, 3);
            out.wait_for("2/3");
        });
        let text = out.text();
        assert!(
            text.starts_with(&format!("{WIPE}{}", line(0, 3))),
            "{text:?}"
        );
        assert!(text.contains(&format!("{WIPE}{}", line(2, 3))), "{text:?}");
        assert!(text.ends_with(&format!("…{WIPE}")), "{text:?}");
    }

    #[test]
    fn work_that_panics_ends_the_line_and_passes_the_panic_on() {
        let out = Shared::default();
        let (sender, receiver) = std::sync::mpsc::channel();
        let shown = out.clone();
        // In a thread of its own, so that a hang fails the test.
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                while_working(shown.clone(), Duration::ZERO, |progress| {
                    progress.update(0, 1);
                    shown.wait_for("0/1");
                    panic!("drawing failed");
                })
            });
            let _ = sender.send(result.is_err());
        });
        let panicked = receiver.recv_timeout(Duration::from_secs(5));
        assert_eq!(panicked, Ok(true));
        assert!(
            out.text().ends_with(&format!("…{WIPE}")),
            "{:?}",
            out.text()
        );
    }

    #[test]
    fn shows_nothing_without_diagrams() {
        let out = Shared::default();
        while_working(out.clone(), Duration::ZERO, |_| {
            std::thread::sleep(Duration::from_millis(50));
        });
        assert_eq!(out.text(), "");
    }

    /// A wait that is over after `takes`, on a clock it keeps itself, and
    /// the times it was given.
    fn waiting(takes: Duration) -> (impl FnMut(Duration) -> bool, Arc<Mutex<Vec<Duration>>>) {
        let given = Arc::new(Mutex::new(Vec::new()));
        let mut passed = Duration::ZERO;
        let noted = Arc::clone(&given);
        let wait = move |most: Duration| {
            noted.lock().unwrap().push(most);
            passed += most.min(takes.saturating_sub(passed));
            passed >= takes
        };
        (wait, given)
    }

    /// `polls` for [`until_done_or_skipped`], in turn, and then `None`.
    fn polled(polls: &[(usize, usize)]) -> impl FnMut() -> Option<(usize, usize)> + '_ {
        let mut polls = polls.iter().copied();
        move || polls.next()
    }

    #[test]
    fn a_skippable_wait_over_in_time_shows_nothing() {
        let out = Shared::default();
        let hour = Duration::from_secs(3600);
        let skipped =
            until_done_or_skipped(out.clone(), hour, polled(&[(0, 2), (1, 2)]), |_| false);
        assert!(!skipped);
        assert_eq!(out.text(), "");
    }

    #[test]
    fn a_skippable_wait_shows_its_count_and_the_hint_and_wipes_it() {
        let out = Shared::default();
        let polls = polled(&[(0, 2), (0, 2), (1, 2)]);
        let skipped = until_done_or_skipped(out.clone(), Duration::ZERO, polls, |_| false);
        assert!(!skipped);
        assert_eq!(
            out.text(),
            format!("{WIPE}{}{HINT}{WIPE}{}{HINT}{WIPE}", line(0, 2), line(1, 2))
        );
    }

    #[test]
    fn space_or_enter_ends_a_skippable_wait_before_the_work_is_over() {
        let out = Shared::default();
        let mut keys = [false, true].into_iter();
        let skip = move |_| keys.next().unwrap_or(false);
        let polls = polled(&[(0, 3), (1, 3), (2, 3)]);
        assert!(until_done_or_skipped(
            out.clone(),
            Duration::ZERO,
            polls,
            skip
        ));
        assert!(out.text().contains(&line(1, 3)), "{:?}", out.text());
        assert!(!out.text().contains(&line(2, 3)), "{:?}", out.text());
        assert!(out.text().ends_with(WIPE), "{:?}", out.text());
    }

    #[test]
    fn a_wait_over_in_time_shows_nothing() {
        let out = Shared::default();
        let (wait, given) = waiting(Duration::from_millis(300));
        let (second, five) = (Duration::from_secs(1), Duration::from_secs(5));
        while_waiting(out.clone(), second, five, "closing", wait);
        assert_eq!(out.text(), "");
        assert_eq!(*given.lock().unwrap(), [second]);
    }

    #[test]
    fn a_longer_wait_shows_its_line_until_it_is_over_and_wipes_it() {
        let out = Shared::default();
        let (wait, given) = waiting(Duration::from_millis(1500));
        let (second, five) = (Duration::from_secs(1), Duration::from_secs(5));
        while_waiting(out.clone(), second, five, "closing", wait);
        assert_eq!(out.text(), format!("{WIPE}closing{WIPE}"));
        assert_eq!(*given.lock().unwrap(), [second, Duration::from_secs(4)]);
    }
}
