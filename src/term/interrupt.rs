//! Signals that would end kiri while programs outside kiri draw diagrams:
//! Ctrl-C (SIGINT), SIGTERM and SIGHUP. The programs run in process groups
//! of their own, which Ctrl-C does not reach, nor a signal sent to kiri
//! alone, so kiri catches these, stops the programs and removes their
//! files, and then ends as the signal would have ended it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use signal_hook::SigId;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

/// The signals an [`Interrupt`] catches.
const SIGNALS: [i32; 3] = [SIGINT, SIGTERM, SIGHUP];

/// Catches Ctrl-C, SIGTERM and SIGHUP while it lives, unless they are
/// ignored: one sets a flag instead of ending kiri, until
/// [`release`](Self::release). Dropping it takes its handlers away, which
/// leaves the signals ignored until another catches them, as the pager's
/// screen does; so it lives until kiri ends or the pager's screen catches
/// the signals, before the terminal is raw.
#[derive(Debug)]
pub struct Interrupt {
    /// The signal that came, 0 before one did.
    came: Arc<AtomicUsize>,
    /// Set when one came, for the work it is to stop.
    stop: Arc<AtomicBool>,
    /// Whether the signals end kiri as they do by default.
    ends: Arc<AtomicBool>,
    handlers: Vec<SigId>,
}

impl Interrupt {
    /// Catches the signals from now on; `None` when the handlers cannot be
    /// set.
    pub fn catch() -> Option<Self> {
        let mut interrupt = Interrupt {
            came: Arc::new(AtomicUsize::new(0)),
            stop: Arc::new(AtomicBool::new(false)),
            ends: Arc::new(AtomicBool::new(false)),
            handlers: Vec::new(),
        };
        // One that is ignored, as under nohup, stays so.
        for signal in SIGNALS.into_iter().filter(|&signal| !ignored(signal)) {
            // The handlers run in this order. Noting the signal before
            // looking at `ends`, as `release` sets `ends` before it looks
            // at the signal, means that a signal either ends kiri or is
            // seen by `release`.
            let came = Arc::clone(&interrupt.came);
            let came = signal_hook::flag::register_usize(signal, came, signal as usize);
            interrupt.handlers.push(came.ok()?);
            let stop = signal_hook::flag::register(signal, Arc::clone(&interrupt.stop));
            interrupt.handlers.push(stop.ok()?);
            let ends = Arc::clone(&interrupt.ends);
            let ends = signal_hook::flag::register_conditional_default(signal, ends);
            interrupt.handlers.push(ends.ok()?);
        }
        Some(interrupt)
    }

    /// The flag that a signal sets, for the work it is to stop.
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    /// Makes the signals end kiri again, as they do by default; the
    /// signal that came before, if one did, which kiri is then to end by
    /// with [`end_by`].
    pub fn release(&self) -> Option<i32> {
        self.ends.store(true, Ordering::SeqCst);
        match self.came.load(Ordering::SeqCst) {
            0 => None,
            signal => Some(signal as i32),
        }
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        for &id in &self.handlers {
            signal_hook::low_level::unregister(id);
        }
    }
}

/// Whether `signal` is ignored, as one stays that kiri's parent ignored;
/// false when that cannot be told.
fn ignored(signal: i32) -> bool {
    let mut action = std::mem::MaybeUninit::<libc::sigaction>::zeroed();
    // SAFETY: with no new action, sigaction only writes the current one
    // to `action`, which is valid for writes of its size.
    let result = unsafe { libc::sigaction(signal, std::ptr::null(), action.as_mut_ptr()) };
    if result != 0 {
        return false;
    }
    // SAFETY: sigaction succeeded, so it wrote the whole action.
    let action = unsafe { action.assume_init() };
    action.sa_sigaction == libc::SIG_IGN
}

/// Ends kiri as `signal` does by default, so that the shell that started
/// it sees that the signal ended it.
pub fn end_by(signal: i32) {
    let _ = signal_hook::low_level::emulate_default_handler(signal);
}
