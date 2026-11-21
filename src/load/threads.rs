//! Starting the threads that the loads' queue and the CPU lane work on,
//! the one way: with the stack of the main thread, and with the work done
//! here when no thread can be had.

use std::io;

/// Starts a thread that does `work`. Its stack is as large as the main
/// thread's: a renderer built into kiri needs it.
pub(super) fn spawn_thread(work: Box<dyn FnOnce() + Send>) -> io::Result<()> {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(work)
        .map(drop)
}

/// Starts up to `count` threads, each made by `spawn`, that each do `work`
/// on a copy of `state`. When none of them could be made, the work is done
/// here, on the caller's thread, so that what waits for it is not left
/// waiting for ever; nothing is done when `count` is none. How many threads
/// were made: one that could not be made ends the starting, and the work is
/// not done here while another thread is at it.
pub(super) fn start_threads<S: Clone + Send + 'static>(
    count: usize,
    state: &S,
    work: fn(&S),
    mut spawn: impl FnMut(Box<dyn FnOnce() + Send>) -> io::Result<()>,
) -> usize {
    let mut started = 0;
    for _ in 0..count {
        let state = state.clone();
        if spawn(Box::new(move || work(&state))).is_err() {
            break;
        }
        started += 1;
    }
    if count > 0 && started == 0 {
        work(state);
    }
    started
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};

    use super::*;

    fn count(state: &Arc<AtomicUsize>) {
        state.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn each_thread_does_the_work_on_its_own_copy_of_the_state() {
        let done = Arc::new(AtomicUsize::new(0));
        let kept = std::cell::RefCell::new(Vec::new());
        let started = start_threads(3, &done, count, |work| {
            kept.borrow_mut().push(work);
            Ok(())
        });
        assert_eq!(started, 3);
        assert_eq!(done.load(Ordering::SeqCst), 0, "nothing is done here");
        for work in kept.into_inner() {
            work();
        }
        assert_eq!(done.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn work_is_done_here_when_no_thread_can_be_made() {
        let done = Arc::new(AtomicUsize::new(0));
        let started = start_threads(2, &done, count, |_| Err(io::Error::other("no thread")));
        assert_eq!(started, 0);
        assert_eq!(done.load(Ordering::SeqCst), 1, "once, here");
    }

    #[test]
    fn work_is_not_done_here_while_another_thread_is_at_it() {
        let done = Arc::new(AtomicUsize::new(0));
        let mut made = 0;
        let kept = std::cell::RefCell::new(Vec::new());
        let started = start_threads(3, &done, count, |work| {
            made += 1;
            if made == 2 {
                return Err(io::Error::other("no more threads"));
            }
            kept.borrow_mut().push(work);
            Ok(())
        });
        assert_eq!(started, 1, "the first failure ends the starting");
        assert_eq!(done.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn nothing_is_started_or_done_for_no_threads() {
        let done = Arc::new(AtomicUsize::new(0));
        let started = start_threads(0, &done, count, |_| unreachable!("no thread is asked for"));
        assert_eq!(started, 0);
        assert_eq!(done.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_real_thread_does_the_work_on_a_large_stack() {
        let (sender, received) = mpsc::channel();
        let work = Box::new(move || {
            // A thread has 2 MiB by default; this holds that, with copies of it.
            let stack = [1u8; 2 << 20];
            sender.send(std::hint::black_box(stack)[0]).unwrap();
        });
        spawn_thread(work).expect("a thread");
        assert_eq!(received.recv().unwrap(), 1);
    }
}
