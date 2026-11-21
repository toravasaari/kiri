//! The CPU lane: the work that takes the processor and waits for no file or
//! program, which is the building of an animated picture's frames. It runs
//! on a small pool of threads beside the loads' own, which wait for files
//! and programs. The loads' threads are not held up by it, nor it by them.
//!
//! The work is cut into pieces, and each piece goes back to the lane before
//! the next, so that a build that the loads prefer does not wait behind a
//! long one for more than a piece. What waits runs in the order the loads
//! prefer ([`Order`], which the runner's `prefer` tells), and by when it came
//! among the pictures of the same place in it.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use image::RgbaImage;

use crate::load::order::{Order, rank, take_first};
use crate::typeset::images::ImageKey;
use crate::typeset::images::build::{Build, FramesJob, Piece};

/// How many threads the lane works on at most.
pub(crate) const THREADS: usize = 2;

/// What a piece of work came to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// There is more to do: the work goes back to wait for its turn.
    More,
    /// The work is done, or gave up.
    Done,
}

/// Work for the lane, done a piece at a time, for the picture `picture`.
pub(crate) struct Task {
    picture: ImageKey,
    stop: Arc<AtomicBool>,
    piece: Box<dyn FnMut() -> Step + Send>,
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a build of {:?}", self.picture)
    }
}

impl Task {
    /// Work that `piece` does a piece of at each call, until it says it is
    /// done, as long as `stop` is not set.
    pub(crate) fn new(
        picture: ImageKey,
        stop: Arc<AtomicBool>,
        piece: impl FnMut() -> Step + Send + 'static,
    ) -> Self {
        Task {
            picture,
            stop,
            piece: Box::new(piece),
        }
    }

    /// The building of the frames of `job` for `picture`, which gives
    /// `done` the frames when they are all built: `None` when they would
    /// take too much, or when `stop` was set first.
    pub(crate) fn frames(
        picture: ImageKey,
        stop: Arc<AtomicBool>,
        job: FramesJob,
        done: impl FnOnce(Option<Vec<RgbaImage>>) + Send + 'static,
    ) -> Self {
        let (mut build, mut done) = (Build::new(job), Some(done));
        let flag = Arc::clone(&stop);
        Task::new(picture, stop, move || {
            // A piece that panics fails this build alone, as a load that
            // panics fails alone: the frames do not come, and the thread
            // goes on with the next.
            let piece = crate::panics::catch(|| build.as_mut().map(|build| build.piece(&flag)));
            let frames = match piece {
                Ok(Some(Piece::More)) => return Step::More,
                Ok(Some(Piece::Done(frames))) => Some(frames),
                Ok(Some(Piece::Stopped) | None) | Err(_) => None,
            };
            if let Some(done) = done.take() {
                done(frames);
            }
            Step::Done
        })
    }

    /// Whether it was stopped.
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Does it all here, piece after piece, unless it was stopped: for a
    /// runner without threads.
    pub(crate) fn finish(mut self) {
        while !self.stopped() && (self.piece)() == Step::More {}
    }
}

/// The threads that do the work, and the work that waits for them.
#[derive(Debug)]
pub(crate) struct Lane {
    shared: Arc<Mutex<Waiting>>,
    threads: usize,
    /// The order of the images that the loads prefer, which what waits goes
    /// in.
    pub(super) order: Order,
}

/// The work that waits, and how many threads are at it.
#[derive(Debug, Default)]
struct Waiting {
    /// Each with the number it came with.
    tasks: Vec<(u64, Task)>,
    next: u64,
    /// The threads that have not found it empty.
    workers: usize,
}

impl Lane {
    /// A lane of at most `threads` threads, whose work goes in the order
    /// `order` tells.
    pub(crate) fn new(threads: usize, order: Order) -> Self {
        Lane {
            shared: Arc::default(),
            threads,
            order,
        }
    }

    /// Has `task` done, on a thread of the lane's, or here when no thread
    /// can be had: a build that never came would leave the picture waiting
    /// for ever.
    pub(crate) fn add(&self, task: Task) {
        self.add_with(task, |work| {
            // As large a stack as the main thread's, as a load has.
            let thread = std::thread::Builder::new().stack_size(8 << 20);
            thread.spawn(work).map(drop)
        });
    }

    /// [`Lane::add`], with `spawn` making the threads.
    fn add_with(&self, task: Task, spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<()>) {
        let start = {
            let mut waiting = lock(&self.shared);
            let number = waiting.next;
            waiting.next += 1;
            waiting.tasks.push((number, task));
            let start = waiting.workers < self.threads;
            waiting.workers += usize::from(start);
            start
        };
        if start {
            let (shared, order) = (Arc::clone(&self.shared), self.order.clone());
            if spawn(Box::new(move || work(&shared, &order))).is_err() {
                work(&self.shared, &self.order);
            }
        }
    }

    /// Does the work that waits on the thread that calls this, as a thread
    /// of the lane does, without waiting for one.
    #[cfg(test)]
    pub(crate) fn work_here(&self) {
        lock(&self.shared).workers += 1;
        work(&self.shared, &self.order);
    }
}

/// A lock of `waiting`, which a panic elsewhere does not poison.
fn lock(waiting: &Mutex<Waiting>) -> MutexGuard<'_, Waiting> {
    waiting.lock().unwrap_or_else(|e| e.into_inner())
}

/// A thread of the lane that is counted among those at work: when it ends
/// by anything but finding the work empty, such as a piece that panics
/// without being caught, it is not counted any more, so that the lane can
/// start another thread in its place.
struct Counted<'a> {
    shared: &'a Mutex<Waiting>,
    counted: bool,
}

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        if self.counted {
            let mut waiting = lock(self.shared);
            waiting.workers = waiting.workers.saturating_sub(1);
        }
    }
}

/// Does the pieces of the work that waits, in the order `order` tells,
/// until none is left that was not stopped.
fn work(shared: &Mutex<Waiting>, order: &Order) {
    let mut thread = Counted {
        shared,
        counted: true,
    };
    while let Some((number, mut task)) = next(shared, order) {
        if (task.piece)() == Step::More {
            lock(shared).tasks.push((number, task));
        }
    }
    // `next` found it empty, and no longer counted this thread.
    thread.counted = false;
}

/// The work to do a piece of next: the first in the order `order` tells,
/// of the same place the one that came first. The work that was stopped is
/// dropped unstarted. `None` when none is left, which this thread is then no
/// longer counted among the lane's.
fn next(shared: &Mutex<Waiting>, order: &Order) -> Option<(u64, Task)> {
    let first = order.get();
    let mut waiting = lock(shared);
    let Waiting { tasks, workers, .. } = &mut *waiting;
    let stopped = |(_, task): &(u64, Task)| task.stopped();
    let place = |_, (number, task): &(u64, Task)| Some((rank(&first, &task.picture), *number));
    let taken = take_first(tasks, stopped, place);
    if taken.is_none() {
        *workers -= 1;
    }
    taken
}

#[cfg(test)]
mod tests;
