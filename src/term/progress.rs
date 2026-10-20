//! The line that tells how far drawing a document's diagrams has got,
//! `kiri: rendering diagrams 2/5…`, while it takes long enough for the
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
    /// `done` of `total` diagrams are drawn.
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

/// The line for `done` of `total` diagrams.
pub fn line(done: usize, total: usize) -> String {
    format!("kiri: rendering diagrams {done}/{total}…")
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
        let result = work(&progress);
        progress.finish();
        let _ = shower.join();
        result
    })
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
    fn the_line_counts_the_diagrams() {
        assert_eq!(line(2, 5), "kiri: rendering diagrams 2/5…");
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
    fn shows_nothing_without_diagrams() {
        let out = Shared::default();
        while_working(out.clone(), Duration::ZERO, |_| {
            std::thread::sleep(Duration::from_millis(50));
        });
        assert_eq!(out.text(), "");
    }
}
