//! The runner of the loads: where the jobs that `Loads::reconcile` starts
//! run. A `Runner` runs them on threads (`Threads`: the jobs wait in a queue
//! that the threads take from, with the programs outside kiri limited) or as
//! a test's script says, and has the builds of frames done by the CPU lane.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::images::{Fonts, ImageError, ImageKey, Source};
#[cfg(test)]
use crate::load::jobs::Fetch;
use crate::load::jobs::{Job, crashed, load};
use crate::load::lane::{Lane, THREADS, Task};
use crate::load::order::{Order, rank, take_first};
use crate::session::state::Look;

/// A load that has come.
#[derive(Debug)]
pub(crate) struct Done {
    pub(super) id: u64,
    pub(super) key: ImageKey,
    pub(super) image: Result<Source, ImageError>,
    /// How the file it was read from looked then, if one was read.
    pub(super) look: Option<Look>,
}

/// Where the loads that [`Loads::reconcile`] starts run: on threads of
/// their own ([`Threads`]), or as a test's script says.
pub(crate) trait Runner: std::fmt::Debug {
    /// Runs `jobs`, newly started, each sending what it came to on `done`
    /// unless its stop flag is set by then.
    fn run(&mut self, jobs: Vec<Queued>, done: &Sender<Done>);
    /// Sets the stop flags `flags`, those of the loads that are stopped
    /// together: no job that waits starts between one flag set and the
    /// next.
    fn stop(&mut self, flags: &[Arc<AtomicBool>]) {
        for flag in flags {
            flag.store(true, Ordering::SeqCst);
        }
    }
    /// Runs the waiting jobs that `first` names before the others, in its
    /// order; the others in the order they waited. Told before the jobs
    /// that are run next, so that those wait in that order too.
    fn prefer(&mut self, first: HashMap<ImageKey, usize>);
    /// Waits for the programs outside kiri to end, for at most `limit`;
    /// whether they have.
    fn wait(&self, limit: Duration) -> bool;
    /// Has `task`, which builds frames, done on the CPU lane's threads, or
    /// at once when the runner has none.
    fn work(&mut self, task: Task) {
        task.finish();
    }
}

/// Runs the loads on threads of their own: the jobs wait in a queue that
/// the threads take from, with the programs outside kiri drawing for them
/// limited.
#[derive(Debug)]
pub(super) struct Threads {
    /// The jobs waiting, of every reload, and the programs outside kiri
    /// drawing for them.
    queue: Arc<Queue>,
    /// Whether builds of frames run where they are started, for a test that
    /// has no time to wait for a thread.
    inline: bool,
    /// Builds of frames kept to be run when a test says.
    held: Option<Held>,
    /// The threads that build frames, apart from those that load.
    lane: Lane,
}

impl Default for Threads {
    fn default() -> Self {
        // The loads and the lane go in the one order the runner is told.
        let order = Order::default();
        Threads {
            queue: Arc::new(Queue {
                order: order.clone(),
                ..Queue::default()
            }),
            inline: false,
            held: None,
            lane: Lane::new(THREADS, order),
        }
    }
}

#[cfg(test)]
impl Threads {
    /// Threads that run builds of frames where they are started.
    pub(super) fn inline() -> Self {
        Threads {
            inline: true,
            ..Threads::default()
        }
    }

    /// Threads that keep builds of frames in `held`, to run when a test
    /// says.
    pub(super) fn holding(held: Held) -> Self {
        Threads {
            held: Some(held),
            ..Threads::default()
        }
    }
}

/// Builds of frames that a test holds back, and runs when it says.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Default)]
pub(crate) struct Held(Arc<Mutex<Vec<Task>>>);

#[cfg_attr(not(test), allow(dead_code))]
impl Held {
    /// Runs the builds held, in the order they came, each piece after piece
    /// to its end.
    pub(crate) fn release(&self) {
        let tasks = std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()));
        for task in tasks {
            task.finish();
        }
    }

    /// How many builds are held.
    pub(crate) fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// The jobs waiting to load, of every reload, which the threads take
/// one at a time.
#[derive(Debug, Default)]
struct Queue {
    waiting: Mutex<Waiting>,
    programs: Programs,
    /// Which images go first, by their place in the order.
    order: Order,
}

/// The jobs waiting, in the order they were started.
#[derive(Debug, Default)]
struct Waiting {
    jobs: Vec<Queued>,
}

/// A job waiting, with the number and the stop flag of its load.
pub(crate) struct Queued {
    pub(super) job: Job,
    pub(super) id: u64,
    pub(super) stop: Arc<AtomicBool>,
}

impl Queued {
    /// The key of the image the job loads.
    #[cfg(test)]
    pub(crate) fn key(&self) -> &ImageKey {
        &self.job.key
    }

    /// The image file the job reads, and the directory a relative path is
    /// read from.
    #[cfg(test)]
    pub(crate) fn fetches(&self) -> Option<(&str, &std::path::Path)> {
        match self.job.fetch.as_ref()? {
            Fetch::Url { url, base } => Some((url.as_str(), base.as_path())),
        }
    }

    /// The load as a test's script brings it, to come on `done`.
    #[cfg(test)]
    pub(crate) fn arrival(&self, done: &Sender<Done>) -> Arrival {
        Arrival {
            sender: done.clone(),
            id: self.id,
            key: self.job.key.clone(),
            stop: Arc::clone(&self.stop),
        }
    }
}

impl std::fmt::Debug for Queued {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Queued")
            .field("job", &self.job)
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// The programs outside kiri that draw for the loads: at most
/// [`AT_ONCE`](crate::diagrams::external::AT_ONCE) at a time, however many
/// reloads started loads, as each one may be a browser.
#[derive(Debug, Default)]
struct Programs {
    running: Mutex<usize>,
    changed: Condvar,
}

impl Programs {
    /// Waits until no program runs, for at most `limit`; whether none
    /// does.
    fn wait_for_none(&self, limit: Duration) -> bool {
        let start = Instant::now();
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        while *running > 0 {
            let Some(left) = limit.checked_sub(start.elapsed()) else {
                return false;
            };
            let wait = self.changed.wait_timeout(running, left);
            running = wait.unwrap_or_else(|e| e.into_inner()).0;
        }
        true
    }
}

/// A program's place among [`Programs`], given back when it is dropped.
struct Place<'a>(&'a Programs);

impl Drop for Place<'_> {
    fn drop(&mut self) {
        *self.0.running.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        self.0.changed.notify_all();
    }
}

/// An image a test has loading, which comes when it says.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct Arrival {
    pub(super) sender: Sender<Done>,
    pub(super) id: u64,
    pub(super) key: ImageKey,
    pub(super) stop: Arc<AtomicBool>,
}

#[cfg(test)]
impl Arrival {
    /// Whether the load was stopped.
    pub(crate) fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// The image comes as `image`.
    pub(crate) fn come(self, image: Result<Source, ImageError>) {
        let done = Done {
            id: self.id,
            key: self.key,
            image,
            look: None,
        };
        self.sender.send(done).expect("the loads to take it");
    }
}

/// A runner that runs nothing and notes each job it is given, in the
/// order it is given them, as an `Arrival` that a test brings to come when
/// it says.
#[cfg(test)]
#[derive(Debug, Default, Clone)]
pub(crate) struct Expecting(pub(crate) std::rc::Rc<std::cell::RefCell<Vec<(ImageKey, Arrival)>>>);

#[cfg(test)]
impl Expecting {
    /// The first load noted that has not been taken, with its key.
    pub(crate) fn take(&self) -> (ImageKey, Arrival) {
        self.0.borrow_mut().remove(0)
    }
}

#[cfg(test)]
impl Runner for Expecting {
    fn run(&mut self, jobs: Vec<Queued>, done: &Sender<Done>) {
        let mut noted = self.0.borrow_mut();
        noted.extend(
            jobs.iter()
                .map(|queued| (queued.key().clone(), queued.arrival(done))),
        );
    }

    fn prefer(&mut self, _: HashMap<ImageKey, usize>) {}

    fn wait(&self, _: Duration) -> bool {
        true
    }
}

impl Runner for Threads {
    fn run(&mut self, jobs: Vec<Queued>, done: &Sender<Done>) {
        // As many threads as programs may draw at once, and one more for
        // the jobs that need none while they do.
        let programs = jobs.iter().filter(|queued| queued.job.program).count();
        let threads =
            programs.min(crate::diagrams::external::AT_ONCE) + usize::from(programs < jobs.len());
        let mut waiting = self.queue.waiting.lock().unwrap_or_else(|e| e.into_inner());
        waiting.jobs.extend(jobs);
        drop(waiting);
        self.spawn(threads, done);
    }

    fn prefer(&mut self, first: HashMap<ImageKey, usize>) {
        self.queue.order.set(first);
    }

    fn wait(&self, limit: Duration) -> bool {
        self.queue.programs.wait_for_none(limit)
    }

    fn stop(&mut self, flags: &[Arc<AtomicBool>]) {
        self.queue.stop(flags, || {});
    }

    fn work(&mut self, task: Task) {
        if let Some(held) = &self.held {
            held.0.lock().unwrap_or_else(|e| e.into_inner()).push(task);
        } else if self.inline {
            task.finish();
        } else {
            self.lane.add(task);
        }
    }
}

impl Threads {
    /// Starts `threads` threads to load the jobs waiting, which send what
    /// they come to on `done`.
    fn spawn(&mut self, threads: usize, done: &Sender<Done>) {
        let mut started = 0;
        for _ in 0..threads {
            // As large a stack as the main thread's, for a renderer built
            // into kiri.
            let thread = std::thread::Builder::new().stack_size(8 << 20);
            let (queue, sender) = (Arc::clone(&self.queue), done.clone());
            if thread.spawn(move || queue.work(&sender)).is_err() {
                break;
            }
            started += 1;
        }
        // Without a thread of its own, the work is done here, as it would
        // be without loading in the background; none was asked for when
        // nothing new waits.
        if threads > 0 && started == 0 {
            self.queue.work(done);
        }
    }
}

impl Queue {
    /// Sets the stop flags `flags`, calling `between` after each, with the
    /// jobs that wait held: a thread that is freed by the first flag finds
    /// the others set when it looks for its next job. Without it a job
    /// that was stopped with the rest could be taken between two flags and
    /// start.
    fn stop(&self, flags: &[Arc<AtomicBool>], mut between: impl FnMut()) {
        let _waiting = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
        for flag in flags {
            flag.store(true, Ordering::SeqCst);
            between();
        }
    }

    /// Loads the jobs one after another until none is left, and sends
    /// each image that is still wanted to `sender`.
    fn work(&self, sender: &Sender<Done>) {
        let fonts = Fonts::system();
        while let Some((queued, place)) = self.take() {
            let Queued { job, id, stop } = queued;
            // A renderer or decoder that panics fails this load alone.
            let loaded = crate::panics::catch(|| load(&job, &stop, &fonts));
            drop(place);
            let (image, look) =
                loaded.unwrap_or_else(|panic| (Err(crashed(&job.compute, panic.as_ref())), None));
            if stop.load(Ordering::SeqCst) {
                continue;
            }
            let done = Done {
                id,
                key: job.key,
                image,
                look,
            };
            // Nobody to take it once the loads are dropped.
            if sender.send(done).is_err() {
                break;
            }
        }
    }

    /// The next job to load, and the place of its program among the
    /// [`Programs`] if a program draws it: the first that waits, in the
    /// order [`Runner::prefer`] told, or while no program may start, the
    /// first that needs none. Waits while every job that waits needs a
    /// program and none may start; `None` once no job that was not
    /// stopped waits.
    fn take(&self) -> Option<(Queued, Option<Place<'_>>)> {
        loop {
            let mut waiting = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
            // Counted before the program can start, and so before it looks
            // at its stop flag: stopping sets the flag before it counts
            // the programs, and a flag set before this lock is seen here.
            let mut running = (self.programs.running.lock()).unwrap_or_else(|e| e.into_inner());
            let first = self.order.get();
            let jobs = &mut waiting.jobs;
            let room = *running < crate::diagrams::external::AT_ONCE;
            let stopped = |queued: &Queued| queued.stop.load(Ordering::SeqCst);
            let place = |at, queued: &Queued| {
                (room || !queued.job.program).then(|| (rank(&first, &queued.job.key), at))
            };
            if let Some(queued) = take_first(jobs, stopped, place) {
                if !queued.job.program {
                    return Some((queued, None));
                }
                *running += 1;
                return Some((queued, Some(Place(&self.programs))));
            }
            if jobs.is_empty() {
                return None;
            }
            drop(waiting);
            // Woken when a program ends; a stop, or a job that needs no
            // program, is seen within the timeout.
            let wait = (self.programs.changed).wait_timeout(running, Duration::from_millis(20));
            drop(wait);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::jobs::{Gated, diagrams};
    use std::sync::mpsc;

    #[test]
    fn a_job_that_waits_does_not_start_between_the_stop_flags_of_one_stop() {
        let gated = Gated::default();
        let (sender, _receiver) = mpsc::channel();
        let mut threads = Threads::default();
        let flags: Vec<Arc<AtomicBool>> = (0..2).map(|_| Arc::default()).collect();
        let queued = (diagrams(&gated, &["a", "b"]).into_iter().zip(&flags))
            .enumerate()
            .map(|(id, (job, stop))| Queued {
                job,
                id: id as u64,
                stop: Arc::clone(stop),
            })
            .collect();
        threads.run(queued, &sender);
        assert_eq!(gated.until(1), ["a"]);
        // The stop of `a` frees the thread, which looks for the next job as
        // soon as it can; the stop of `b` comes after it has had its time.
        let mut first = true;
        threads.queue.stop(&flags, || {
            if std::mem::take(&mut first) {
                let start = Instant::now();
                while gated.started().len() < 2 && start.elapsed() < Duration::from_millis(300) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        gated.open.store(true, Ordering::SeqCst);
        assert_eq!(gated.started(), ["a"]);
    }

    #[test]
    fn the_loads_and_the_lane_share_the_one_order_the_runner_is_told() {
        let mut threads = Threads::default();
        threads.prefer(HashMap::from([(ImageKey::from("a.gif"), 0)]));
        let (queue, lane) = (threads.queue.order.get(), threads.lane.order.get());
        assert!(Arc::ptr_eq(&queue, &lane));
        assert_eq!(lane.get(&ImageKey::from("a.gif")), Some(&0));
    }
}
