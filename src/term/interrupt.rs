//! Ctrl-C while programs outside kiri draw diagrams. They run in process
//! groups of their own, which Ctrl-C does not reach, so kiri catches it,
//! stops them and removes their files before it ends.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use signal_hook::SigId;
use signal_hook::consts::SIGINT;

/// Catches Ctrl-C (SIGINT) while it lives: one sets a flag instead of
/// ending kiri, until [`release`](Self::release). Dropping it takes its
/// handlers away, which leaves SIGINT ignored until another catches it, as
/// the pager's screen does; so it lives until kiri ends or the pager opens.
#[derive(Debug)]
pub struct Interrupt {
    /// Whether Ctrl-C came.
    came: Arc<AtomicBool>,
    /// Whether Ctrl-C ends kiri as it does by default.
    ends: Arc<AtomicBool>,
    handlers: Vec<SigId>,
}

impl Interrupt {
    /// Catches Ctrl-C from now on; `None` when the handlers cannot be set.
    pub fn catch() -> Option<Self> {
        let mut interrupt = Interrupt {
            came: Arc::new(AtomicBool::new(false)),
            ends: Arc::new(AtomicBool::new(false)),
            handlers: Vec::new(),
        };
        let ends = signal_hook::flag::register_conditional_default(SIGINT, interrupt.ends.clone());
        interrupt.handlers.push(ends.ok()?);
        let came = signal_hook::flag::register(SIGINT, interrupt.came.clone());
        interrupt.handlers.push(came.ok()?);
        Some(interrupt)
    }

    /// The flag that Ctrl-C sets, for the work it is to stop.
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.came)
    }

    /// Whether Ctrl-C came.
    pub fn came(&self) -> bool {
        self.came.load(Ordering::SeqCst)
    }

    /// Makes Ctrl-C end kiri again, as it does by default.
    pub fn release(&self) {
        self.ends.store(true, Ordering::SeqCst);
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        for &id in &self.handlers {
            signal_hook::low_level::unregister(id);
        }
    }
}
