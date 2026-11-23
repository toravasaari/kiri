//! The loads in the background: the document shows at once, and each
//! image comes as it is ready. `Loads::reconcile` is the one way to start
//! and stop them, for every entry point of `src/images/pipeline.rs`: a load
//! that is no longer wanted stops, and one that still is carries on. The
//! `Runner` of `runner.rs` runs what starts, and the jobs are in `jobs.rs`.
//!
//! `Loads` is what the pager polls. It starts and stops the loads through
//! one call, `reconcile`, given the images wanted (`Wanted`: kept, missing,
//! or to load again, with their `Job`), whose order in the list is the order
//! they load in; `want` is what the pager calls with a version's jobs, and
//! which of them `src/session/loading.rs` says are kept or missing. What
//! came is given as `Arrived`, with the look of the file it was read from;
//! `wait_by` waits until one comes or a deadline passes, so that a wait
//! without keys wakes when a load does rather than after a fixed sleep. Each
//! load has a stop flag and a number, so that the result of a load that was
//! stopped or replaced is dropped.
//!
//! The loads start through the `Runner` of `runner.rs`. The builds of an
//! animated picture's frames are kept in a `Builds` of `builds.rs`, which
//! `build`, `poll_builds` and `cancel_build` pass on to; the loop takes the
//! result as part of `Msg::Loads`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

#[cfg(test)]
use crate::diagrams::{Renderer, Request};
use crate::images::{ImageError, ImageKey, Source};
use crate::load::builds::{Builds, Finished};
pub use crate::load::jobs::Job;
use crate::load::runner::{Done, Queued, Runner, Threads};
#[cfg(test)]
use crate::load::testing::Arrival;
use crate::session::loading::Need;
use crate::session::state::Look;
use crate::typeset::images::build::FramesJob;
use crate::typeset::images::size::Placement;

/// The images loading in the background, and what has come of them.
/// Dropping it stops the loads, as [`Loads::halt`] does, without waiting
/// for their programs.
#[derive(Debug)]
pub struct Loads {
    sender: Sender<Done>,
    receiver: Receiver<Done>,
    /// The loads under way, by the key of their image.
    running: HashMap<ImageKey, Running>,
    /// The number of the next load.
    next: u64,
    /// What runs the loads started: threads, or as a test's script says.
    runner: Box<dyn Runner>,
    /// How many loads have come since the loads were last all done, and
    /// of how many.
    done: usize,
    total: usize,
    /// What [`Loads::wait_by`] took from the channel, for [`Loads::poll`].
    pending: Vec<Done>,
    /// The builds of the frames of animated pictures.
    pub(super) builds: Builds,
}

/// A load under way.
#[derive(Debug)]
struct Running {
    /// Its number, which tells it from an earlier load of the same image.
    id: u64,
    stop: Arc<AtomicBool>,
}

/// An image that came, as [`Loads::poll`] gives it.
#[derive(Debug)]
pub struct Arrived {
    pub key: ImageKey,
    pub image: Result<Source, ImageError>,
    /// How its file looked when it was read; `None` for what reads no
    /// file. Nothing reads it yet: a later step compares it.
    pub look: Option<Look>,
}

/// An image wanted, as [`Loads::reconcile`] takes it.
pub(crate) enum Wanted {
    /// Wanted, with nothing to start: a load of it under way carries on.
    Kept(ImageKey),
    /// To load, unless a load of it is under way, which carries on.
    Missing(Job),
    /// To load anew: a load of it under way stops, and what it brings is
    /// dropped.
    Again(Job),
}

impl Wanted {
    /// The key of the image.
    fn key(&self) -> &ImageKey {
        match self {
            Wanted::Kept(key) => key,
            Wanted::Missing(job) | Wanted::Again(job) => &job.key,
        }
    }
}

/// `wanted` with only the last entry of each key, each at its own place: a
/// later entry wins over an earlier one, as a newer job of a key wins over
/// a load of it under way.
fn last_per_key(wanted: Vec<Wanted>) -> Vec<Wanted> {
    let mut seen = HashSet::new();
    let mut last: Vec<Wanted> = wanted
        .into_iter()
        .rev()
        .filter(|entry| seen.insert(entry.key().clone()))
        .collect();
    last.reverse();
    last
}

/// How long stopping waits at most for the programs to end: they get
/// SIGINT and half a second before they are killed.
pub const STOPPING: Duration = Duration::from_secs(5);

impl Default for Loads {
    fn default() -> Self {
        Self::new()
    }
}

impl Loads {
    /// No loads, to run on threads.
    pub fn new() -> Self {
        Self::with(Box::<Threads>::default())
    }

    /// No loads, to run as `runner` says.
    pub(crate) fn with(runner: Box<dyn Runner>) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            running: HashMap::new(),
            next: 0,
            runner,
            done: 0,
            total: 0,
            pending: Vec::new(),
            builds: Builds::default(),
        }
    }

    /// Starts building the frames of `picture`: `jobs` are each the frames
    /// for a place it is shown at. A build of it that is under way gives way.
    /// Without jobs, the build comes to nothing at once.
    pub fn build(&mut self, picture: ImageKey, jobs: Vec<(Placement, FramesJob)>) {
        self.builds.start(picture, jobs, self.runner.as_mut());
    }

    /// Stops the build of the frames of `picture`, if one is under way.
    pub fn cancel_build(&mut self, picture: &ImageKey) {
        self.builds.cancel(picture);
    }

    /// Whether the frames of `picture` are being built.
    pub fn building(&self, picture: &ImageKey) -> bool {
        self.builds.contains(picture)
    }

    /// The builds that are done since the last call, each once; those that
    /// were stopped are left out.
    pub fn poll_builds(&mut self) -> Vec<Finished> {
        self.builds.poll()
    }

    /// Whether the image `key` is loading.
    pub fn loading(&self, key: &ImageKey) -> bool {
        self.running.contains_key(key)
    }

    /// The keys of the images loading.
    pub fn keys(&self) -> impl Iterator<Item = &ImageKey> {
        self.running.keys()
    }

    /// Whether nothing is loading.
    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    /// How many images have come since nothing was loading, and of how
    /// many; `None` once nothing is.
    pub fn progress(&self) -> Option<(usize, usize)> {
        (!self.running.is_empty()).then_some((self.done, self.total))
    }

    /// Stops the load of the image `key`, if one is under way.
    fn stop_one(&mut self, key: &ImageKey) {
        if let Some(running) = self.running.remove(key) {
            running.stop.store(true, Ordering::SeqCst);
            self.total -= 1;
        }
        if self.running.is_empty() {
            (self.done, self.total) = (0, 0);
        }
    }

    /// Starts and stops the loads so that those of `wanted` are under way,
    /// and no others: the loads of the images that no entry names stop,
    /// those that are `Kept` or `Missing` and under way carry on, and those
    /// that are `Again` start anew, the one under way giving way. The new
    /// ones wait behind the jobs that wait, and everything that waits
    /// loads in the order of the list.
    ///
    /// A key is named once. Nobody relies on more: in a debug build a list
    /// that names a key twice panics, and in a release build only the last
    /// entry of the key counts, at its place; the earlier jobs are dropped
    /// unstarted.
    pub(crate) fn reconcile(&mut self, wanted: Vec<Wanted>) {
        let count = wanted.len();
        let wanted = last_per_key(wanted);
        debug_assert!(wanted.len() == count, "a reconcile list names a key twice");
        let first: HashMap<ImageKey, usize> = (wanted.iter().enumerate())
            .map(|(at, entry)| (entry.key().clone(), at))
            .collect();
        let named: HashSet<&ImageKey> = first.keys().collect();
        let before = self.running.len();
        let mut stopped = Vec::new();
        self.running.retain(|key, running| {
            let kept = named.contains(key);
            if !kept {
                stopped.push(Arc::clone(&running.stop));
            }
            kept
        });
        self.total -= before - self.running.len();
        self.runner.stop(&stopped);
        let mut jobs = Vec::new();
        for entry in wanted {
            match entry {
                Wanted::Kept(_) => {}
                Wanted::Missing(job) => {
                    if !self.running.contains_key(&job.key) {
                        jobs.push(job);
                    }
                }
                Wanted::Again(job) => {
                    self.stop_one(&job.key);
                    jobs.push(job);
                }
            }
        }
        if self.running.is_empty() {
            (self.done, self.total) = (0, 0);
        }
        self.runner.prefer(first);
        self.begin(jobs);
    }

    /// Starts and stops the loads so that those of the images `wanted`
    /// names are under way: each is a place in `jobs` and what it needs.
    /// An image that is `Kept`, or `Missing` with a load of it under way,
    /// carries on; a `Missing` one with none starts, from a copy of its
    /// job; and the loads of the images that `wanted` does not name stop.
    /// The images of `jobs` that `wanted` leaves out are not wanted.
    pub fn want(&mut self, jobs: &[Job], wanted: Vec<(usize, Need)>) {
        let entries = (wanted.into_iter())
            .map(|(at, need)| match need {
                Need::Missing if !self.running.contains_key(&jobs[at].key) => {
                    Wanted::Missing(jobs[at].clone())
                }
                Need::Kept | Need::Missing => Wanted::Kept(jobs[at].key.clone()),
            })
            .collect();
        self.reconcile(entries);
    }

    /// `wanted`, and a `Kept` entry for every load under way that it does
    /// not name, so that reconciling stops none of them.
    pub(crate) fn adding(&self, mut wanted: Vec<Wanted>) -> Vec<Wanted> {
        let named: HashSet<&ImageKey> = wanted.iter().map(Wanted::key).collect();
        let kept: Vec<Wanted> = (self.running.keys())
            .filter(|key| !named.contains(key))
            .map(|key| Wanted::Kept(key.clone()))
            .collect();
        wanted.extend(kept);
        wanted
    }

    /// The images that have come since the last call, each once; those
    /// whose loads were stopped, or replaced by a newer one, are left out.
    pub fn poll(&mut self) -> Vec<Arrived> {
        let mut came = Vec::new();
        let mut dones = std::mem::take(&mut self.pending);
        dones.extend(self.receiver.try_iter());
        for done in dones {
            if self
                .running
                .get(&done.key)
                .is_some_and(|run| run.id == done.id)
            {
                self.running.remove(&done.key);
                self.done += 1;
                let Done {
                    key, image, look, ..
                } = done;
                came.push(Arrived { key, image, look });
            }
        }
        if self.running.is_empty() {
            (self.done, self.total) = (0, 0);
        }
        came
    }

    /// Waits until a load comes or `deadline` passes, whichever is first;
    /// whether one came. What came stays for [`Loads::poll`], which gives it
    /// as it gives the rest; a result of a load that was stopped or replaced
    /// may wake it, and is then dropped there.
    pub fn wait_by(&mut self, deadline: Instant) -> bool {
        if !self.pending.is_empty() {
            return true;
        }
        let wait = deadline.saturating_duration_since(Instant::now());
        // The loads hold a sender themselves, so the channel does not end.
        match self.receiver.recv_timeout(wait) {
            Ok(done) => {
                self.pending.push(done);
                true
            }
            Err(_) => false,
        }
    }

    /// Stops every load, and waits for the programs outside kiri that
    /// drew to end, as they do when their time is up, but not for a
    /// renderer built into kiri, which cannot be stopped: what it draws
    /// is dropped.
    pub fn stop(&mut self) {
        self.halt();
        self.wait(STOPPING);
    }

    /// Stops every load, as [`Loads::stop`] does, without waiting for the
    /// programs to end: [`Loads::wait`] does, such as once the pager has
    /// left its screen.
    pub fn halt(&mut self) {
        self.reconcile(Vec::new());
    }

    /// Waits for the programs outside kiri that draw to end, for at most
    /// `limit`; whether they have.
    pub fn wait(&self, limit: Duration) -> bool {
        self.runner.wait(limit)
    }

    /// Gives each of `jobs` the number and the stop flag of a new load,
    /// and has the runner run them.
    fn begin(&mut self, jobs: Vec<Job>) {
        if jobs.is_empty() {
            return;
        }
        let mut queued = Vec::with_capacity(jobs.len());
        for job in jobs {
            let stop = Arc::new(AtomicBool::new(false));
            let id = self.next;
            self.next += 1;
            let running = Running {
                id,
                stop: Arc::clone(&stop),
            };
            self.running.insert(job.key.clone(), running);
            self.total += 1;
            queued.push(Queued { job, id, stop });
        }
        self.runner.run(queued, &self.sender);
    }

    /// Takes the image `key` as loading, to come when the test says.
    #[cfg(test)]
    pub(crate) fn expect(&mut self, key: impl Into<ImageKey>) -> Arrival {
        let key = key.into();
        self.stop_one(&key);
        let id = self.next;
        self.next += 1;
        let stop = Arc::new(AtomicBool::new(false));
        let running = Running {
            id,
            stop: Arc::clone(&stop),
        };
        self.running.insert(key.clone(), running);
        self.total += 1;
        Arrival {
            sender: self.sender.clone(),
            id,
            key,
            stop,
        }
    }
}

impl Drop for Loads {
    /// Stops the loads without waiting for their programs: a caller that
    /// is to wait for them calls [`Loads::wait`] first.
    fn drop(&mut self) {
        self.halt();
    }
}

#[cfg(test)]
mod tests;
