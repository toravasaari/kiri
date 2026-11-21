//! The loads in the background: the document shows at once, and each
//! image comes as it is ready. `Loads::reconcile` is the one way to start
//! and stop them, for every entry point of `src/images/pipeline.rs`: a load
//! that is no longer wanted stops, and one that still is carries on. The
//! `Runner` of `runner.rs` runs what starts, and the jobs are in `jobs.rs`.

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
use crate::load::jobs::Job;
#[cfg(test)]
use crate::load::jobs::{Gated, What, context, diagram, diagrams};
#[cfg(test)]
use crate::load::runner::{Arrival, Held};
use crate::load::runner::{Done, Queued, Runner, Threads};
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

    /// No loads, to run on threads, with the builds of frames run where they
    /// are started: a test cannot wait for a thread.
    #[cfg(test)]
    pub(crate) fn inline_builds() -> Self {
        Self::with(Box::new(Threads::inline()))
    }

    /// No loads, to run on threads, with the builds of frames held back until
    /// the [`Held`] that comes with them releases them.
    #[cfg(test)]
    pub(crate) fn held_builds() -> (Self, Held) {
        let held = Held::default();
        (Self::with(Box::new(Threads::holding(held.clone()))), held)
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

    /// Has a program draw the diagram `key`, which ends `ending` after it
    /// is told to stop, and draws nothing; it draws when this returns.
    #[cfg(test)]
    pub(crate) fn program(&mut self, key: ImageKey, ending: Duration) {
        let job = Job {
            key: key.clone(),
            fetch: None,
            compute: What::Diagram {
                language: "mermaid".to_owned(),
                source: "slow\n".to_owned(),
                line: 1,
                by_line: true,
            },
            program: true,
            context: Arc::new(context(tests::Slow(ending))),
        };
        self.reconcile(self.adding(vec![Wanted::Again(job)]));
        let start = Instant::now();
        while self.wait(Duration::ZERO) && start.elapsed() < STOPPING {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Loads the diagrams `names`, each a Mermaid diagram of one line,
    /// with `gated`, one at a time, as no program draws them, keyed as
    /// diagrams drawn as `look` says are.
    #[cfg(test)]
    pub(crate) fn gated(&mut self, names: &[&str], gated: &Gated, look: u64) {
        let jobs = gated.jobs(names, look).into_iter().map(Wanted::Again);
        self.reconcile(self.adding(jobs.collect()));
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
mod tests {
    use super::*;

    /// Draws until it is told to stop, and ends that long after, as a
    /// program given time to close does.
    pub(super) struct Slow(pub(super) Duration);

    impl Renderer for Slow {
        fn draws(&self, _: &str) -> bool {
            true
        }

        fn render(
            &self,
            request: &Request,
        ) -> Result<crate::diagrams::Drawn, crate::diagrams::DiagramError> {
            let stop = request.stop.expect("a stop flag");
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(self.0);
            Err("stopped".into())
        }
    }

    /// What `loads` has come to, polled until `count` images have, for at
    /// most two seconds.
    fn arrivals(loads: &mut Loads, count: usize) -> Vec<Arrived> {
        let start = Instant::now();
        let mut arrived = Vec::new();
        while arrived.len() < count && start.elapsed() < Duration::from_secs(2) {
            arrived.extend(loads.poll());
            std::thread::sleep(Duration::from_millis(5));
        }
        arrived
    }

    /// The number of the load of the image `key`, which is under way.
    fn id_of(loads: &Loads, key: &str) -> u64 {
        loads.running[&ImageKey::from(key)].id
    }

    #[test]
    fn reconciling_keeps_what_is_kept_and_stops_the_load_that_no_entry_names() {
        let mut loads = Loads::new();
        let a = loads.expect("a.png");
        let b = loads.expect("b.png");
        let id = id_of(&loads, "a.png");
        loads.reconcile(vec![Wanted::Kept("a.png".into())]);
        assert!(!a.stopped());
        assert!(b.stopped());
        assert!(loads.loading(&"a.png".into()));
        assert!(!loads.loading(&"b.png".into()));
        assert_eq!(id_of(&loads, "a.png"), id);
        assert_eq!(loads.progress(), Some((0, 1)));
        // What the load that stopped brings is dropped.
        b.come(square());
        assert!(loads.poll().is_empty());
        a.come(square());
        assert_eq!(loads.poll().len(), 1);
        assert_eq!(loads.progress(), None);
    }

    #[test]
    fn wanting_keeps_loads_under_way_starts_the_missing_and_stops_the_rest() {
        let mut loads = Loads::new();
        let (under_way, other, gone) = (
            loads.expect("under-way.png"),
            loads.expect("other.png"),
            loads.expect("gone.png"),
        );
        let id = id_of(&loads, "under-way.png");
        let jobs = [
            Job::file("held.png"),
            Job::file("under-way.png"),
            Job::file("new.png"),
            Job::file("other.png"),
        ];
        let wanted = vec![
            (0, Need::Kept),
            (1, Need::Missing),
            (2, Need::Missing),
            (3, Need::Kept),
        ];
        loads.want(&jobs, wanted);
        // A load under way carries on whether it is kept or missing, with
        // the number it had; the one that nothing names stops.
        assert!(!under_way.stopped() && !other.stopped());
        assert!(gone.stopped());
        assert_eq!(id_of(&loads, "under-way.png"), id);
        assert!(loads.loading(&"other.png".into()));
        assert!(!loads.loading(&"gone.png".into()));
        // What is kept and not under way does not start; what is missing
        // does.
        assert!(!loads.loading(&"held.png".into()));
        assert!(loads.loading(&"new.png".into()));
        assert_eq!(loads.progress(), Some((0, 3)));
    }

    #[test]
    fn wanting_nothing_stops_every_load() {
        let mut loads = Loads::new();
        let a = loads.expect("a.png");
        loads.want(&[Job::file("a.png")], Vec::new());
        assert!(a.stopped());
        assert!(loads.is_empty());
    }

    #[test]
    fn a_missing_image_loads_unless_a_load_of_it_is_under_way() {
        let mut loads = Loads::new();
        let a = loads.expect("a.png");
        let id = id_of(&loads, "a.png");
        let wanted = vec![
            Wanted::Missing(Job::file("a.png")),
            Wanted::Missing(Job::file("m.png")),
        ];
        loads.reconcile(wanted);
        // The load under way carries on, as the same one.
        assert!(!a.stopped());
        assert_eq!(id_of(&loads, "a.png"), id);
        assert!(loads.loading(&"m.png".into()));
        assert_eq!(loads.progress(), Some((0, 2)));
        a.come(square());
        let arrived = arrivals(&mut loads, 2);
        assert_eq!(arrived.len(), 2, "{arrived:?}");
        let by = |key: &str| arrived.iter().find(|came| came.key == ImageKey::from(key));
        assert!(
            matches!(by("a.png"), Some(Arrived { image: Ok(_), .. })),
            "{arrived:?}"
        );
        assert!(
            matches!(by("m.png"), Some(Arrived { image: Err(_), .. })),
            "{arrived:?}"
        );
        assert!(loads.is_empty());
    }

    #[test]
    fn an_image_to_load_again_replaces_the_load_under_way_and_drops_what_it_brings() {
        let mut loads = Loads::new();
        let old = loads.expect("a.png");
        let id = id_of(&loads, "a.png");
        loads.reconcile(vec![Wanted::Again(Job::file("a.png"))]);
        assert!(old.stopped());
        assert_ne!(id_of(&loads, "a.png"), id);
        assert_eq!(loads.progress(), Some((0, 1)));
        // The older result is dropped; the file's, which is missing, comes.
        old.come(square());
        let arrived = arrivals(&mut loads, 1);
        assert!(
            matches!(&arrived[..], [Arrived { image: Err(_), .. }]),
            "{arrived:?}"
        );
        assert!(loads.is_empty());
    }

    /// The entries as (kind, key), to compare lists with.
    fn shape(wanted: &[Wanted]) -> Vec<(&'static str, String)> {
        let kind = |entry: &Wanted| match entry {
            Wanted::Kept(_) => "kept",
            Wanted::Missing(_) => "missing",
            Wanted::Again(_) => "again",
        };
        (wanted.iter())
            .map(|entry| (kind(entry), entry.key().to_string()))
            .collect()
    }

    #[test]
    fn the_last_entry_of_a_key_stays_at_its_own_place() {
        let wanted = vec![
            Wanted::Missing(Job::file("a.png")),
            Wanted::Missing(Job::file("b.png")),
            Wanted::Again(Job::file("a.png")),
            Wanted::Kept("c.png".into()),
        ];
        let kept = last_per_key(wanted);
        assert_eq!(
            shape(&kept),
            [
                ("missing", "b.png".to_string()),
                ("again", "a.png".to_string()),
                ("kept", "c.png".to_string()),
            ]
        );
    }

    #[test]
    fn a_list_without_a_key_twice_is_left_as_it_is() {
        let wanted = vec![
            Wanted::Kept("b.png".into()),
            Wanted::Missing(Job::file("a.png")),
        ];
        let kept = last_per_key(wanted);
        assert_eq!(
            shape(&kept),
            [
                ("kept", "b.png".to_string()),
                ("missing", "a.png".to_string()),
            ]
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "twice")]
    fn reconciling_a_list_that_names_a_key_twice_is_a_mistake_of_the_caller() {
        let mut loads = Loads::new();
        loads.reconcile(vec![
            Wanted::Missing(Job::file("a.png")),
            Wanted::Missing(Job::file("a.png")),
        ]);
    }

    #[test]
    fn expecting_an_image_counts_from_nothing_when_it_empties_the_loads_as_reconciling_does() {
        let count = |again: bool| {
            let mut loads = Loads::new();
            let a = loads.expect("a.png");
            let b = loads.expect("b.png");
            b.come(square());
            assert_eq!(loads.poll().len(), 1);
            assert_eq!(loads.progress(), Some((1, 2)));
            // The only load left is replaced by a new one.
            let _new = match again {
                true => {
                    loads.reconcile(loads.adding(vec![Wanted::Again(Job::file("a.png"))]));
                    None
                }
                false => Some(loads.expect("a.png")),
            };
            assert!(a.stopped());
            loads.progress()
        };
        assert_eq!(count(true), Some((0, 1)));
        assert_eq!(count(false), Some((0, 1)));
    }

    #[test]
    fn the_total_follows_what_reconciling_starts_and_stops() {
        let mut loads = Loads::new();
        let a = loads.expect("a.png");
        let b = loads.expect("b.png");
        assert_eq!(loads.progress(), Some((0, 2)));
        let wanted = vec![
            Wanted::Kept("a.png".into()),
            Wanted::Again(Job::file("b.png")),
            Wanted::Missing(Job::file("c.png")),
        ];
        loads.reconcile(wanted);
        assert!(b.stopped());
        assert!(!a.stopped());
        assert_eq!(loads.progress(), Some((0, 3)));
        loads.reconcile(vec![Wanted::Kept("a.png".into())]);
        assert_eq!(loads.progress(), Some((0, 1)));
        loads.reconcile(Vec::new());
        assert!(a.stopped());
        assert!(loads.is_empty());
        assert_eq!(loads.progress(), None);
    }

    #[test]
    fn reconciling_nothing_stops_every_load_and_starts_none() {
        let gated = Gated::default();
        let mut loads = Loads::new();
        let a = loads.expect("a.png");
        loads.gated(&["x"], &gated, 0);
        assert_eq!(gated.until(1), ["x"]);
        loads.reconcile(Vec::new());
        assert!(a.stopped());
        assert!(loads.is_empty());
        assert_eq!(loads.progress(), None);
        // Nothing is left to start.
        loads.reconcile(Vec::new());
        assert_eq!(gated.started(), ["x"]);
    }

    #[test]
    fn adding_names_the_loads_under_way_so_that_reconciling_stops_none_of_them() {
        let mut loads = Loads::new();
        let a = loads.expect("a.png");
        let b = loads.expect("b.png");
        let wanted = loads.adding(vec![Wanted::Again(Job::file("a.png"))]);
        // `a.png` is named already.
        assert_eq!(wanted.len(), 2);
        loads.reconcile(wanted);
        assert!(a.stopped());
        assert!(!b.stopped());
        assert_eq!(loads.progress(), Some((0, 2)));
        let wanted = loads.adding(vec![Wanted::Missing(Job::file("m.png"))]);
        loads.reconcile(wanted);
        assert!(!b.stopped());
        assert_eq!(loads.progress(), Some((0, 3)));
    }

    /// A runner that runs nothing, and keeps what it is told.
    #[derive(Debug, Default, Clone)]
    struct Recorder {
        told: Arc<Mutex<Vec<String>>>,
        jobs: Arc<Mutex<Vec<Queued>>>,
        done: Arc<Mutex<Option<Sender<Done>>>>,
    }

    impl Recorder {
        fn told(&self) -> Vec<String> {
            self.told.lock().unwrap().clone()
        }
    }

    impl Runner for Recorder {
        fn run(&mut self, jobs: Vec<Queued>, done: &Sender<Done>) {
            let keys: Vec<String> = jobs.iter().map(|queued| queued.key().to_string()).collect();
            self.told
                .lock()
                .unwrap()
                .push(format!("run {}", keys.join(" ")));
            self.jobs.lock().unwrap().extend(jobs);
            *self.done.lock().unwrap() = Some(done.clone());
        }

        fn prefer(&mut self, first: HashMap<ImageKey, usize>) {
            let mut first: Vec<_> = first.into_iter().map(|(key, at)| (at, key)).collect();
            first.sort_by_key(|(at, _)| *at);
            let keys: Vec<String> = first.iter().map(|(_, key)| key.to_string()).collect();
            self.told
                .lock()
                .unwrap()
                .push(format!("prefer {}", keys.join(" ")));
        }

        fn wait(&self, _: Duration) -> bool {
            self.told.lock().unwrap().push("wait".to_owned());
            true
        }
    }

    #[test]
    fn the_runner_gets_the_new_jobs_in_the_order_of_the_list_and_never_none() {
        let recorder = Recorder::default();
        let mut loads = Loads::with(Box::new(recorder.clone()));
        let wanted = vec![
            Wanted::Missing(Job::file("b.png")),
            Wanted::Again(Job::file("a.png")),
            Wanted::Kept("k.png".into()),
        ];
        loads.reconcile(wanted);
        // The order of the list is told before the jobs run.
        assert_eq!(
            recorder.told(),
            ["prefer b.png a.png k.png", "run b.png a.png"]
        );
        assert!(loads.loading(&"b.png".into()));
        assert_eq!(loads.progress(), Some((0, 2)));
        // Nothing new: the loads under way carry on, and halting is not
        // a job.
        loads.reconcile(vec![Wanted::Missing(Job::file("b.png"))]);
        loads.halt();
        let told = recorder.told();
        assert_eq!(told[2..], ["prefer b.png", "prefer "], "{told:?}");
        assert_eq!(
            told.iter().filter(|told| told.starts_with("run")).count(),
            1
        );
        assert!(loads.is_empty());
    }

    #[test]
    fn the_runner_is_told_the_order_of_a_list_with_its_kept_images_and_to_wait() {
        let recorder = Recorder::default();
        let mut loads = Loads::with(Box::new(recorder.clone()));
        let wanted = vec![
            Wanted::Kept("c.png".into()),
            Wanted::Missing(Job::file("a.png")),
        ];
        loads.reconcile(wanted);
        assert!(loads.wait(Duration::ZERO));
        assert_eq!(recorder.told(), ["prefer c.png a.png", "run a.png", "wait"]);
    }

    #[test]
    fn a_result_of_a_load_the_runner_ran_comes_only_while_the_load_is_wanted() {
        let recorder = Recorder::default();
        let mut loads = Loads::with(Box::new(recorder.clone()));
        loads.reconcile(vec![Wanted::Missing(Job::file("a.png"))]);
        loads.reconcile(vec![Wanted::Again(Job::file("a.png"))]);
        let jobs = recorder.jobs.lock().unwrap();
        let [old, new] = &jobs[..] else {
            panic!("{jobs:?}")
        };
        // The runner is given a stop flag of each, and the old one is set.
        assert!(old.stop.load(Ordering::SeqCst));
        assert!(!new.stop.load(Ordering::SeqCst));
        assert_ne!(old.id, new.id);
        let done = recorder.done.lock().unwrap().clone().expect("a sender");
        let send = |queued: &Queued, image| {
            let key = queued.job.key.clone();
            let id = queued.id;
            done.send(Done {
                id,
                key,
                image,
                look: None,
            })
            .unwrap();
        };
        send(old, Ok(Source::Raster(image::RgbaImage::new(9, 9))));
        send(new, square());
        let came = loads.poll();
        assert!(
            matches!(&came[..], [Arrived { image: Ok(image), .. }] if image.size() == (2, 2)),
            "{came:?}"
        );
        assert!(loads.is_empty());
    }

    #[test]
    fn a_file_s_image_arrives_with_the_look_of_the_file() {
        let scratch = crate::testing::Scratch::new("queue-look");
        let png = || {
            let mut png = std::io::Cursor::new(Vec::new());
            let image = image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 255]));
            image.write_to(&mut png, image::ImageFormat::Png).unwrap();
            png.into_inner()
        };
        std::fs::write(scratch.join("a.png"), png()).unwrap();
        let time = std::time::UNIX_EPOCH + Duration::from_secs(1234);
        let file = std::fs::File::options()
            .write(true)
            .open(scratch.join("a.png"))
            .unwrap();
        file.set_modified(time).unwrap();
        let length = std::fs::metadata(scratch.join("a.png")).unwrap().len();
        let mut loads = Loads::new();
        loads.reconcile(vec![Wanted::Missing(Job::image("a.png", &scratch))]);
        let arrived = arrivals(&mut loads, 1);
        let [Arrived { image, look, .. }] = &arrived[..] else {
            panic!("{arrived:?}")
        };
        assert!(image.is_ok(), "{arrived:?}");
        assert_eq!(
            *look,
            Some(Look {
                modified: Some(time),
                length
            })
        );
    }

    #[test]
    fn what_reads_no_file_arrives_without_a_look() {
        let bytes = {
            let mut png = std::io::Cursor::new(Vec::new());
            let image = image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 255]));
            image.write_to(&mut png, image::ImageFormat::Png).unwrap();
            png.into_inner()
        };
        let data = {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            format!("data:image/png;base64,{encoded}")
        };
        let mut loads = Loads::new();
        let gated = Gated::default();
        gated.open.store(true, Ordering::SeqCst);
        loads.gated(&["x"], &gated, 0);
        let wanted = vec![
            Wanted::Missing(Job::file(&data)),
            Wanted::Missing(Job::file("missing.png")),
        ];
        loads.reconcile(loads.adding(wanted));
        let arrived = arrivals(&mut loads, 3);
        assert_eq!(arrived.len(), 3, "{arrived:?}");
        let data_image = arrived
            .iter()
            .find(|came| came.key == ImageKey::from(data.as_str()));
        assert!(
            matches!(
                data_image,
                Some(Arrived {
                    image: Ok(_),
                    look: None,
                    ..
                })
            ),
            "{arrived:?}"
        );
        assert!(
            arrived.iter().all(|came| came.look.is_none()),
            "{arrived:?}"
        );
    }

    /// Starts `jobs` in `loads`, each giving way to a load of its image
    /// under way, and stopping no other load.
    fn start(loads: &mut Loads, jobs: Vec<Job>) {
        let jobs = jobs.into_iter().map(Wanted::Again).collect();
        loads.reconcile(loads.adding(jobs));
    }

    /// Polls `loads` until the image `key` comes, for at most two
    /// seconds; whether it came.
    fn came(loads: &mut Loads, key: &ImageKey) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            if loads.poll().iter().any(|came| came.key == *key) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn an_image_file_passes_diagrams_waiting_for_a_program() {
        let gated = Gated::default();
        let mut loads = Loads::new();
        let context = Arc::new(context(gated.clone()));
        let programs = |names: &[&str]| {
            (names.iter())
                .map(|name| diagram(name, true, &context))
                .collect()
        };
        start(&mut loads, programs(&["a1", "a2", "a3", "a4"]));
        assert_eq!(gated.until(4).len(), 4);
        // A reload while the four draw, as many as may at once.
        let file = Job {
            context: Arc::clone(&context),
            ..Job::file("missing.png")
        };
        let mut jobs: Vec<Job> = programs(&["b1", "b2", "b3", "b4"]);
        jobs.push(file);
        start(&mut loads, jobs);
        assert!(came(&mut loads, &"missing.png".into()));
        assert_eq!(gated.started().len(), 4);
        gated.open.store(true, Ordering::SeqCst);
        assert_eq!(gated.until(8).len(), 8);
    }

    fn keys(names: &[&str]) -> Vec<ImageKey> {
        let key = |name: &&str| ImageKey::rendered("mermaid", &format!("{name}\n"));
        names.iter().map(key).collect()
    }

    /// Wants the diagrams `names` of `jobs`, in that order, those that
    /// are under way staying as they are.
    fn want_in_order(loads: &mut Loads, jobs: &[Job], names: &[&str]) {
        let place = |key: ImageKey| jobs.iter().position(|job| job.key == key).unwrap();
        let wanted = keys(names)
            .into_iter()
            .map(|key| (place(key), Need::Missing));
        loads.want(jobs, wanted.collect());
    }

    #[test]
    fn the_jobs_of_a_list_load_in_its_order() {
        let gated = Gated::default();
        gated.open.store(true, Ordering::SeqCst);
        let mut loads = Loads::new();
        let jobs = diagrams(&gated, &["a", "b", "c", "d"]);
        want_in_order(&mut loads, &jobs, &["c", "a", "b", "d"]);
        assert_eq!(gated.until(4), ["c", "a", "b", "d"]);
    }

    #[test]
    fn wanting_again_in_another_order_reorders_what_waits_and_not_what_runs() {
        let gated = Gated::default();
        let mut loads = Loads::new();
        let jobs = diagrams(&gated, &["a", "b", "c", "d"]);
        want_in_order(&mut loads, &jobs, &["b", "a", "c", "d"]);
        assert_eq!(gated.until(1), ["b"]);
        // As the view moves on.
        want_in_order(&mut loads, &jobs, &["b", "d", "c", "a"]);
        gated.open.store(true, Ordering::SeqCst);
        assert_eq!(gated.until(4), ["b", "d", "c", "a"]);
    }

    #[test]
    fn wanting_what_is_there_does_no_work_on_the_caller_s_thread() {
        let gated = Gated::default();
        let mut loads = Loads::new();
        // One draws, and one waits for the thread.
        let jobs = diagrams(&gated, &["a", "b"]);
        want_in_order(&mut loads, &jobs, &["a", "b"]);
        assert_eq!(gated.until(1), ["a"]);
        // A reconcile that starts nothing.
        let open = gated.clone();
        let opener = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            open.open.store(true, Ordering::SeqCst);
        });
        let start = Instant::now();
        want_in_order(&mut loads, &jobs, &["a", "b"]);
        let elapsed = start.elapsed();
        opener.join().unwrap();
        assert!(elapsed < Duration::from_millis(300), "{elapsed:?}");
    }

    #[test]
    fn jobs_stopped_while_they_wait_never_start() {
        let gated = Gated::default();
        let mut loads = Loads::new();
        let jobs = diagrams(&gated, &["a", "b"]);
        want_in_order(&mut loads, &jobs, &["a", "b"]);
        assert_eq!(gated.until(1), ["a"]);
        loads.halt();
        assert!(loads.is_empty());
        gated.open.store(true, Ordering::SeqCst);
        assert!(loads.wait(Duration::ZERO));
        // A later load starts at once; the one that waited never did.
        loads.gated(&["c"], &gated, 0);
        assert!(came(&mut loads, &keys(&["c"])[0]));
        assert_eq!(gated.started(), ["a", "c"]);
    }

    /// A runner that runs nothing, and notes how many flags it is given to
    /// set at once, and whether they were all unset.
    #[derive(Debug, Default, Clone)]
    struct Stops(Arc<Mutex<Vec<(usize, bool)>>>);

    impl Runner for Stops {
        fn run(&mut self, _: Vec<Queued>, _: &Sender<Done>) {}

        fn stop(&mut self, flags: &[Arc<AtomicBool>]) {
            let unset = flags.iter().all(|flag| !flag.load(Ordering::SeqCst));
            self.0.lock().unwrap().push((flags.len(), unset));
            for flag in flags {
                flag.store(true, Ordering::SeqCst);
            }
        }

        fn prefer(&mut self, _: HashMap<ImageKey, usize>) {}

        fn wait(&self, _: Duration) -> bool {
            true
        }
    }

    #[test]
    fn halting_has_the_runner_stop_every_load_in_one_go() {
        let stops = Stops::default();
        let mut loads = Loads::with(Box::new(stops.clone()));
        let jobs = ["a.png", "b.png", "c.png"].map(Job::file);
        loads.reconcile(jobs.iter().cloned().map(Wanted::Missing).collect());
        loads.halt();
        assert!(loads.is_empty());
        // The runner is also told when nothing stops.
        assert_eq!(*stops.0.lock().unwrap(), [(0, true), (3, true)]);
    }

    #[test]
    fn a_test_s_program_draws_when_it_returns() {
        let mut loads = Loads::new();
        let start = Instant::now();
        loads.program(ImageKey::rendered("mermaid", "slow\n"), Duration::ZERO);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "{:?}",
            start.elapsed()
        );
        assert!(!loads.wait(Duration::ZERO));
    }

    #[test]
    fn halting_does_not_wait_for_the_programs_and_waiting_does() {
        let mut loads = Loads::new();
        loads.program(
            ImageKey::rendered("mermaid", "slow\n"),
            Duration::from_secs(2),
        );
        let start = Instant::now();
        loads.halt();
        assert!(loads.is_empty());
        assert!(start.elapsed() < Duration::from_millis(200));
        assert!(!loads.wait(Duration::ZERO));
        assert!(loads.wait(Duration::from_secs(5)));
    }

    #[test]
    fn dropping_the_loads_stops_them_without_waiting() {
        let mut loads = Loads::new();
        loads.program(
            ImageKey::rendered("mermaid", "slow\n"),
            Duration::from_secs(2),
        );
        let start = Instant::now();
        drop(loads);
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "{:?}",
            start.elapsed()
        );
    }

    fn square() -> Result<Source, ImageError> {
        Ok(Source::Raster(image::RgbaImage::new(2, 2)))
    }

    #[test]
    fn counts_what_came_until_nothing_is_loading() {
        let mut loads = Loads::new();
        let a = loads.expect("a.png");
        let b = loads.expect("b.png");
        assert_eq!(loads.progress(), Some((0, 2)));
        a.come(square());
        assert_eq!(loads.poll().len(), 1);
        assert_eq!(loads.progress(), Some((1, 2)));
        let c = loads.expect("c.png");
        assert_eq!(loads.progress(), Some((1, 3)));
        // A load no longer wanted no longer counts.
        let rest = (loads
            .keys()
            .filter(|key| **key != ImageKey::from("c.png"))
            .cloned())
        .collect::<Vec<_>>();
        loads.reconcile(rest.into_iter().map(Wanted::Kept).collect());
        assert_eq!(loads.progress(), Some((1, 2)));
        c.come(square());
        b.come(square());
        let came = loads.poll();
        assert_eq!(came.len(), 1);
        assert_eq!(came[0].key, ImageKey::from("b.png"));
        assert_eq!(loads.progress(), None);
        // Counted from nothing again.
        let _d = loads.expect("d.png");
        assert_eq!(loads.progress(), Some((0, 1)));
    }

    #[test]
    fn an_older_load_of_an_image_gives_way_to_a_newer_one() {
        let mut loads = Loads::new();
        let old = loads.expect("a.png");
        let new = loads.expect("a.png");
        old.come(square());
        assert!(loads.poll().is_empty());
        assert!(loads.loading(&"a.png".into()));
        new.come(square());
        assert_eq!(loads.poll().len(), 1);
        assert!(loads.is_empty());
    }

    /// Draws Mermaid, as a program would, by panicking, once it has
    /// noted that it started.
    struct Panicking(Arc<AtomicBool>);

    impl Renderer for Panicking {
        fn draws(&self, _: &str) -> bool {
            true
        }

        fn render(
            &self,
            _: &Request,
        ) -> Result<crate::diagrams::Drawn, crate::diagrams::DiagramError> {
            self.0.store(true, Ordering::SeqCst);
            panic!("a renderer that panics");
        }
    }

    #[test]
    fn a_load_that_panics_gives_its_program_s_place_back() {
        let mut loads = Loads::new();
        let started = Arc::new(AtomicBool::new(false));
        let job = Job {
            key: ImageKey::rendered("mermaid", "x\n"),
            fetch: None,
            compute: What::Diagram {
                language: "mermaid".to_owned(),
                source: "x\n".to_owned(),
                line: 1,
                by_line: true,
            },
            program: true,
            context: Arc::new(context(Panicking(Arc::clone(&started)))),
        };
        start(&mut loads, vec![job]);
        let start = Instant::now();
        while !started.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(started.load(Ordering::SeqCst));
        // The thread unwinds after the renderer panicked.
        assert!(loads.wait(Duration::from_secs(2)));
    }

    /// Panics on a diagram that says `boom`, and draws others as an empty
    /// SVG.
    struct Boom;

    impl Renderer for Boom {
        fn draws(&self, _: &str) -> bool {
            true
        }

        fn render(
            &self,
            request: &Request,
        ) -> Result<crate::diagrams::Drawn, crate::diagrams::DiagramError> {
            if request.source.starts_with("boom") {
                panic!("boom");
            }
            let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"/>"#;
            Ok(crate::diagrams::Drawn::svg(svg, request.theme, None))
        }
    }

    #[test]
    fn a_load_that_panics_fails_and_the_others_still_load() {
        let mut loads = Loads::new();
        let context = Arc::new(context(Boom));
        let job = |source: &str| Job {
            key: ImageKey::rendered("mermaid", source),
            fetch: None,
            compute: What::Diagram {
                language: "mermaid".to_owned(),
                source: source.to_owned(),
                line: 3,
                by_line: true,
            },
            program: false,
            context: Arc::clone(&context),
        };
        // One thread, as no program draws: the second job waits for it.
        start(&mut loads, vec![job("boom\n"), job("fine\n")]);
        let mut came = Vec::new();
        let start = Instant::now();
        while came.len() < 2 && start.elapsed() < Duration::from_secs(5) {
            came.extend(loads.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(came.len(), 2, "{came:?}");
        assert_eq!(came[0].key, ImageKey::rendered("mermaid", "boom\n"));
        match &came[0].image {
            Err(ImageError::Diagram { name, reason }) => {
                assert_eq!(name, "mermaid diagram on line 3");
                assert_eq!(reason.to_string(), "the renderer crashed: boom");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(came[1].key, ImageKey::rendered("mermaid", "fine\n"));
        assert!(came[1].image.is_ok(), "{came:?}");
        assert!(loads.is_empty());
    }

    #[test]
    fn a_load_that_comes_wakes_the_wait_well_before_its_deadline_and_poll_gives_it() {
        let mut loads = Loads::new();
        let a = loads.expect("a.png");
        let start = Instant::now();
        let producer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            a.come(square());
        });
        let came = loads.wait_by(start + Duration::from_secs(10));
        let waited = start.elapsed();
        producer.join().unwrap();
        assert!(came);
        assert!(waited < Duration::from_secs(5), "{waited:?}");
        let arrived = loads.poll();
        assert!(
            matches!(&arrived[..], [Arrived { key, image: Ok(_), .. }] if *key == "a.png".into()),
            "{arrived:?}"
        );
        assert!(loads.is_empty());
        assert!(loads.poll().is_empty());
    }

    #[test]
    fn a_wait_with_nothing_coming_ends_at_its_deadline() {
        let mut loads = Loads::new();
        let _a = loads.expect("a.png");
        let wait = Duration::from_millis(60);
        let start = Instant::now();
        assert!(!loads.wait_by(start + wait));
        assert!(start.elapsed() >= wait, "{:?}", start.elapsed());
        // What is still loading stays so.
        assert!(loads.loading(&"a.png".into()));
        assert!(loads.poll().is_empty());
    }

    #[test]
    fn a_load_that_came_before_the_wait_ends_it_at_once_and_a_deadline_gone_still_looks() {
        let mut loads = Loads::new();
        let a = loads.expect("a.png");
        a.come(square());
        let gone = Instant::now();
        assert!(loads.wait_by(gone));
        // Asked again before `poll`, what came is still there.
        assert!(loads.wait_by(gone));
        assert_eq!(loads.poll().len(), 1);
        assert!(!loads.wait_by(gone));
    }

    #[test]
    fn the_result_of_a_load_that_was_stopped_wakes_the_wait_and_poll_drops_it() {
        let mut loads = Loads::new();
        let a = loads.expect("a.png");
        loads.reconcile(Vec::new());
        assert!(a.stopped());
        a.come(square());
        assert!(loads.wait_by(Instant::now() + Duration::from_secs(10)));
        assert!(loads.poll().is_empty());
        assert!(loads.is_empty());
    }
}
