//! Loading images on threads of their own, as the pager does after a
//! reload: the document shows at once, and each image comes as it is
//! ready. A load that a newer reload no longer wants stops, and one that
//! it still wants carries on.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::drawn::finish_diagram;
use super::url::load_url;
use super::{Fonts, ImageError, ImageKey, LoadOptions, Source};
use crate::diagrams::{Renderer, Request, Theme};

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
    /// The jobs waiting, of every reload, and the programs outside kiri
    /// drawing for them.
    queue: Arc<Queue>,
    /// The threads to start for the jobs held until the order is told,
    /// while it is not.
    held: Option<usize>,
    /// How many loads have come since the loads were last all done, and
    /// of how many.
    done: usize,
    total: usize,
}

/// A load under way.
#[derive(Debug)]
struct Running {
    /// Its number, which tells it from an earlier load of the same image.
    id: u64,
    stop: Arc<AtomicBool>,
}

/// A load that has come.
#[derive(Debug)]
struct Done {
    id: u64,
    key: ImageKey,
    image: Result<Source, ImageError>,
}

/// An image to load, as owned data, so that a thread of its own can load
/// it.
#[derive(Debug, Clone)]
pub(super) enum What {
    /// An image file or `data:` URL, with relative paths from `base`.
    Url { url: String, base: PathBuf },
    /// A code block drawn as a diagram, as `Wanted::Diagram` is, named in a
    /// warning by its line or not.
    Diagram {
        language: String,
        source: String,
        line: usize,
        by_line: bool,
    },
}

/// One image to load: its key, what it is, and whether a program outside
/// kiri draws it.
#[derive(Debug)]
pub(super) struct Job {
    pub(super) key: ImageKey,
    pub(super) what: What,
    pub(super) program: bool,
}

/// What loading the jobs of one reload needs, besides each job.
pub(super) struct Context {
    pub(super) renderer: Box<dyn Renderer + Send + Sync>,
    pub(super) theme: Theme,
    pub(super) panel: bool,
    pub(super) largest: Option<(u32, u32)>,
    pub(super) max_file_size: u64,
}

/// The jobs waiting to load, of every reload, which the threads take
/// one at a time.
#[derive(Debug, Default)]
struct Queue {
    waiting: Mutex<Waiting>,
    programs: Programs,
}

/// The jobs waiting, in the order they were started, and which images go
/// first.
#[derive(Debug, Default)]
struct Waiting {
    jobs: Vec<Queued>,
    /// The images to load first, by their place in the order.
    first: HashMap<ImageKey, usize>,
}

/// A job waiting, with the number and the stop flag of its load and
/// what loading the jobs of its reload needs.
struct Queued {
    job: Job,
    id: u64,
    stop: Arc<AtomicBool>,
    context: Arc<Context>,
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

/// How long stopping waits at most for the programs to end: they get
/// SIGINT and half a second before they are killed.
pub const STOPPING: Duration = Duration::from_secs(5);

impl Default for Loads {
    fn default() -> Self {
        Self::new()
    }
}

impl Loads {
    /// No loads.
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            running: HashMap::new(),
            next: 0,
            queue: Arc::default(),
            held: None,
            done: 0,
            total: 0,
        }
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

    /// Stops the loads of the images that `keep` does not want.
    pub fn keep(&mut self, keep: impl Fn(&ImageKey) -> bool) {
        let before = self.running.len();
        self.running.retain(|key, running| {
            let kept = keep(key);
            if !kept {
                running.stop.store(true, Ordering::SeqCst);
            }
            kept
        });
        self.total -= before - self.running.len();
        if self.running.is_empty() {
            (self.done, self.total) = (0, 0);
        }
    }

    /// The images that have come since the last call, each once; those
    /// whose loads were stopped, or replaced by a newer one, are left out.
    pub fn poll(&mut self) -> Vec<(ImageKey, Result<Source, ImageError>)> {
        let mut came = Vec::new();
        while let Ok(done) = self.receiver.try_recv() {
            if self
                .running
                .get(&done.key)
                .is_some_and(|run| run.id == done.id)
            {
                self.running.remove(&done.key);
                self.done += 1;
                came.push((done.key, done.image));
            }
        }
        if self.running.is_empty() {
            (self.done, self.total) = (0, 0);
        }
        came
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
        self.keep(|_| false);
    }

    /// Waits for the programs outside kiri that draw to end, for at most
    /// `limit`; whether they have.
    pub fn wait(&self, limit: Duration) -> bool {
        self.queue.programs.wait_for_none(limit)
    }

    /// Holds the jobs started from now on until [`Loads::prefer`] tells
    /// which go first, so that the first ones do not start before.
    pub fn hold(&mut self) {
        self.held.get_or_insert(0);
    }

    /// Loads the images `keys`, of those still waiting, before the
    /// others, in that order, and the others in the order they were
    /// started; starts the jobs held.
    pub fn prefer(&mut self, keys: impl IntoIterator<Item = ImageKey>) {
        let first = (keys.into_iter().enumerate())
            .map(|(at, key)| (key, at))
            .collect();
        self.queue
            .waiting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .first = first;
        if let Some(threads) = self.held.take() {
            self.spawn(threads);
        }
    }

    /// Loads `jobs` in the background as `settings` say; a load of the
    /// same image under way gives way to its job.
    pub(super) fn start(&mut self, jobs: Vec<Job>, context: Context) {
        if jobs.is_empty() {
            return;
        }
        let keys: HashSet<&ImageKey> = jobs.iter().map(|job| &job.key).collect();
        self.keep(|key| !keys.contains(key));
        // As many threads as programs may draw at once, and one more for
        // the jobs that need none while they do.
        let programs = jobs.iter().filter(|job| job.program).count();
        let threads =
            programs.min(crate::diagrams::external::AT_ONCE) + usize::from(programs < jobs.len());
        let context = Arc::new(context);
        let mut waiting = self.queue.waiting.lock().unwrap_or_else(|e| e.into_inner());
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
            let context = Arc::clone(&context);
            waiting.jobs.push(Queued {
                job,
                id,
                stop,
                context,
            });
        }
        drop(waiting);
        match &mut self.held {
            Some(held) => *held += threads,
            None => self.spawn(threads),
        }
    }

    /// Starts `threads` threads to load the jobs waiting.
    fn spawn(&mut self, threads: usize) {
        let mut started = 0;
        for _ in 0..threads {
            // As large a stack as the main thread's, for a renderer built
            // into kiri.
            let thread = std::thread::Builder::new().stack_size(8 << 20);
            let (queue, sender) = (Arc::clone(&self.queue), self.sender.clone());
            if thread.spawn(move || queue.work(&sender)).is_err() {
                break;
            }
            started += 1;
        }
        // Without a thread of its own, the work is done here, as it would
        // be without loading in the background; none was asked for when
        // nothing new waits.
        if threads > 0 && started == 0 {
            self.queue.work(&self.sender);
        }
    }

    /// Has a program draw the diagram `key`, which ends `ending` after it
    /// is told to stop, and draws nothing; it draws when this returns.
    #[cfg(test)]
    pub(crate) fn program(&mut self, key: ImageKey, ending: Duration) {
        let job = Job {
            key,
            what: What::Diagram {
                language: "mermaid".to_owned(),
                source: "slow\n".to_owned(),
                line: 1,
                by_line: true,
            },
            program: true,
        };
        let context = Context {
            renderer: Box::new(tests::Slow(ending)),
            theme: Theme::Light,
            panel: true,
            largest: None,
            max_file_size: super::DEFAULT_MAX_FILE_SIZE,
        };
        self.start(vec![job], context);
        // At once, also while the loads are held.
        if let Some(threads) = self.held.take() {
            self.spawn(threads);
        }
        let start = Instant::now();
        while *self.queue.programs.running.lock().unwrap() == 0 && start.elapsed() < STOPPING {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Loads the diagrams `names`, each a Mermaid diagram of one line,
    /// with `gated`, one at a time, as no program draws them.
    #[cfg(test)]
    pub(crate) fn gated(&mut self, names: &[&str], gated: &Gated) {
        let jobs = names.iter().map(|name| diagram(name, false)).collect();
        self.start(jobs, context(gated.clone()));
    }

    /// Takes the image `key` as loading, to come when the test says.
    #[cfg(test)]
    pub(crate) fn expect(&mut self, key: impl Into<ImageKey>) -> Arrival {
        let key = key.into();
        self.keep(|loading| *loading != key);
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

/// An image a test has loading, which comes when it says.
#[cfg(test)]
pub(crate) struct Arrival {
    sender: Sender<Done>,
    id: u64,
    key: ImageKey,
    stop: Arc<AtomicBool>,
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
        };
        self.sender.send(done).expect("the loads to take it");
    }
}

/// Draws each diagram once `open` is set, or ends when its load is
/// stopped, and notes the source of each one it starts on.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct Gated {
    pub(crate) started: Arc<Mutex<Vec<String>>>,
    pub(crate) open: Arc<AtomicBool>,
}

#[cfg(test)]
impl Gated {
    /// The sources of the diagrams started, in the order they were.
    pub(crate) fn started(&self) -> Vec<String> {
        self.started.lock().unwrap().clone()
    }

    /// Waits until `count` diagrams have started, for at most two
    /// seconds; the sources of those that have.
    pub(crate) fn until(&self, count: usize) -> Vec<String> {
        let start = Instant::now();
        while self.started().len() < count && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.started()
    }
}

#[cfg(test)]
impl Renderer for Gated {
    fn draws(&self, _: &str) -> bool {
        true
    }

    fn render(&self, request: &Request) -> Result<crate::diagrams::Drawn, String> {
        let stop = request.stop.expect("a stop flag");
        self.started
            .lock()
            .unwrap()
            .push(request.source.trim().to_owned());
        while !self.open.load(Ordering::SeqCst) && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        Err("drawn".to_owned())
    }
}

/// A job for the diagram `source`, which a program draws or not.
#[cfg(test)]
fn diagram(source: &str, program: bool) -> Job {
    let source = format!("{source}\n");
    Job {
        key: ImageKey::block("mermaid", &source),
        what: What::Diagram {
            language: "mermaid".to_owned(),
            source,
            line: 1,
            by_line: true,
        },
        program,
    }
}

/// What loading with `renderer` needs.
#[cfg(test)]
fn context(renderer: impl Renderer + Send + Sync + 'static) -> Context {
    Context {
        renderer: Box::new(renderer),
        theme: Theme::Light,
        panel: true,
        largest: None,
        max_file_size: super::DEFAULT_MAX_FILE_SIZE,
    }
}

impl Queue {
    /// Loads the jobs one after another until none is left, and sends
    /// each image that is still wanted to `sender`.
    fn work(&self, sender: &Sender<Done>) {
        let fonts = Fonts::system();
        while let Some((queued, place)) = self.take() {
            let Queued {
                job,
                id,
                stop,
                context,
            } = queued;
            // A renderer or decoder that panics fails this load alone.
            let loaded = crate::panics::catch(|| load(&context, &job.what, &stop, &fonts));
            drop(place);
            let image = loaded.unwrap_or_else(|panic| Err(crashed(&job.what, panic.as_ref())));
            if stop.load(Ordering::SeqCst) {
                continue;
            }
            let done = Done {
                id,
                key: job.key,
                image,
            };
            // Nobody to take it once the loads are dropped.
            if sender.send(done).is_err() {
                break;
            }
        }
    }

    /// The next job to load, and the place of its program among the
    /// [`Programs`] if a program draws it: the first that waits, in the
    /// order [`Loads::prefer`] told, or while no program may start, the
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
            let Waiting { jobs, first } = &mut *waiting;
            jobs.retain(|queued| !queued.stop.load(Ordering::SeqCst));
            if jobs.is_empty() {
                return None;
            }
            let room = *running < crate::diagrams::external::AT_ONCE;
            let next = (jobs.iter().enumerate())
                .filter(|(_, queued)| room || !queued.job.program)
                .min_by_key(|(at, queued)| (first.get(&queued.job.key).unwrap_or(&usize::MAX), *at))
                .map(|(at, _)| at);
            if let Some(at) = next {
                let queued = jobs.remove(at);
                if !queued.job.program {
                    return Some((queued, None));
                }
                *running += 1;
                return Some((queued, Some(Place(&self.programs))));
            }
            drop(waiting);
            // Woken when a program ends; a stop, or a job that needs no
            // program, is seen within the timeout.
            let wait = (self.programs.changed).wait_timeout(running, Duration::from_millis(20));
            drop(wait);
        }
    }
}

/// The image `what` names, loaded as `context` says until `stop` is set.
fn load(
    context: &Context,
    what: &What,
    stop: &AtomicBool,
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    match what {
        What::Url { url, base } => load_url(url, base, context.max_file_size, fonts),
        What::Diagram {
            language,
            source,
            line,
            by_line,
        } => {
            let renderer = context.renderer.as_ref();
            let request = Request::new(language, source, context.theme).stopped_by(stop);
            let drawn = renderer.render(&request);
            let options = LoadOptions {
                renderer,
                theme: context.theme,
                by_line: *by_line,
                progress: None,
                panel: context.panel,
                largest: context.largest,
            };
            let max = context.max_file_size;
            finish_diagram(language, source, *line, drawn, max, fonts, &options)
        }
    }
}

/// Why loading `what` failed when it panicked with `panic`: a diagram
/// fails as one its renderer could not draw, an image as a broken one,
/// with the panic's message when it has one.
fn crashed(what: &What, panic: &(dyn Any + Send)) -> ImageError {
    let message = (panic.downcast_ref::<&str>().copied())
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str));
    let with = |crashed: &str| match message {
        Some(message) => format!("{crashed}: {message}"),
        None => crashed.to_owned(),
    };
    match what {
        What::Diagram {
            language,
            source,
            line,
            by_line,
        } => ImageError::Diagram {
            name: crate::diagrams::name(language, source, *line, *by_line),
            reason: with("the renderer crashed"),
        },
        What::Url { .. } => ImageError::Corrupt(with("the decoder crashed")),
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

        fn render(&self, request: &Request) -> Result<crate::diagrams::Drawn, String> {
            let stop = request.stop.expect("a stop flag");
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(self.0);
            Err("stopped".to_owned())
        }
    }

    /// Polls `loads` until the image `key` comes, for at most two
    /// seconds; whether it came.
    fn came(loads: &mut Loads, key: &ImageKey) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            if loads.poll().iter().any(|(came, _)| came == key) {
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
        let programs = |names: &[&str]| names.iter().map(|name| diagram(name, true)).collect();
        loads.start(programs(&["a1", "a2", "a3", "a4"]), context(gated.clone()));
        assert_eq!(gated.until(4).len(), 4);
        // A reload while the four draw, as many as may at once.
        let file = Job {
            key: "missing.png".into(),
            what: What::Url {
                url: "missing.png".to_owned(),
                base: PathBuf::from("/nonexistent"),
            },
            program: false,
        };
        let mut jobs: Vec<Job> = programs(&["b1", "b2", "b3", "b4"]);
        jobs.push(file);
        loads.start(jobs, context(gated.clone()));
        assert!(came(&mut loads, &"missing.png".into()));
        assert_eq!(gated.started().len(), 4);
        gated.open.store(true, Ordering::SeqCst);
        assert_eq!(gated.until(8).len(), 8);
    }

    /// Starts loading the diagrams `names` with `gated` in `loads`, which
    /// holds them, one at a time as no program draws them.
    fn held(loads: &mut Loads, gated: &Gated, names: &[&str]) {
        loads.hold();
        loads.gated(names, gated);
    }

    fn keys(names: &[&str]) -> Vec<ImageKey> {
        let key = |name: &&str| ImageKey::block("mermaid", &format!("{name}\n"));
        names.iter().map(key).collect()
    }

    #[test]
    fn the_images_preferred_load_first_and_then_the_rest_in_order() {
        let gated = Gated::default();
        gated.open.store(true, Ordering::SeqCst);
        let mut loads = Loads::new();
        held(&mut loads, &gated, &["a", "b", "c", "d"]);
        loads.prefer(keys(&["c", "a"]));
        assert_eq!(gated.until(4), ["c", "a", "b", "d"]);
    }

    #[test]
    fn preferring_others_reorders_what_waits_and_not_what_runs() {
        let gated = Gated::default();
        let mut loads = Loads::new();
        held(&mut loads, &gated, &["a", "b", "c", "d"]);
        loads.prefer(keys(&["b"]));
        assert_eq!(gated.until(1), ["b"]);
        // As the view moves on.
        loads.prefer(keys(&["d", "c"]));
        gated.open.store(true, Ordering::SeqCst);
        assert_eq!(gated.until(4), ["b", "d", "c", "a"]);
    }

    #[test]
    fn preferring_with_nothing_held_does_no_work_on_the_caller_s_thread() {
        let gated = Gated::default();
        let mut loads = Loads::new();
        // One draws, and one waits for the thread.
        loads.gated(&["a", "b"], &gated);
        assert_eq!(gated.until(1), ["a"]);
        // A reload that starts nothing.
        loads.hold();
        let open = gated.clone();
        let opener = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            open.open.store(true, Ordering::SeqCst);
        });
        let start = Instant::now();
        loads.prefer(Vec::new());
        let elapsed = start.elapsed();
        opener.join().unwrap();
        assert!(elapsed < Duration::from_millis(300), "{elapsed:?}");
    }

    #[test]
    fn jobs_held_and_stopped_before_the_order_is_told_never_start() {
        let gated = Gated::default();
        let mut loads = Loads::new();
        held(&mut loads, &gated, &["a", "b"]);
        loads.halt();
        let start = Instant::now();
        loads.prefer(keys(&["a"]));
        assert!(start.elapsed() < Duration::from_millis(300));
        std::thread::sleep(Duration::from_millis(100));
        assert!(gated.started().is_empty(), "{:?}", gated.started());
        assert!(loads.is_empty());
        assert!(loads.wait(Duration::ZERO));
        // Nothing is left held: a later load starts at once.
        gated.open.store(true, Ordering::SeqCst);
        loads.gated(&["c"], &gated);
        assert!(came(&mut loads, &keys(&["c"])[0]));
        assert_eq!(gated.started(), ["c"]);
    }

    #[test]
    fn a_test_s_program_draws_when_it_returns_also_while_held() {
        let mut loads = Loads::new();
        loads.hold();
        let start = Instant::now();
        loads.program(ImageKey::block("mermaid", "slow\n"), Duration::ZERO);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(*loads.queue.programs.running.lock().unwrap(), 1);
    }

    #[test]
    fn halting_does_not_wait_for_the_programs_and_waiting_does() {
        let mut loads = Loads::new();
        loads.program(ImageKey::block("mermaid", "slow\n"), Duration::from_secs(2));
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
        loads.program(ImageKey::block("mermaid", "slow\n"), Duration::from_secs(2));
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
        loads.keep(|key| *key != ImageKey::from("c.png"));
        assert_eq!(loads.progress(), Some((1, 2)));
        c.come(square());
        b.come(square());
        let came = loads.poll();
        assert_eq!(came.len(), 1);
        assert_eq!(came[0].0, ImageKey::from("b.png"));
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

        fn render(&self, _: &Request) -> Result<crate::diagrams::Drawn, String> {
            self.0.store(true, Ordering::SeqCst);
            panic!("a renderer that panics");
        }
    }

    #[test]
    fn a_load_that_panics_gives_its_program_s_place_back() {
        let mut loads = Loads::new();
        let started = Arc::new(AtomicBool::new(false));
        let job = Job {
            key: ImageKey::block("mermaid", "x\n"),
            what: What::Diagram {
                language: "mermaid".to_owned(),
                source: "x\n".to_owned(),
                line: 1,
                by_line: true,
            },
            program: true,
        };
        let context = Context {
            renderer: Box::new(Panicking(Arc::clone(&started))),
            theme: Theme::Light,
            panel: true,
            largest: None,
            max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        };
        loads.start(vec![job], context);
        let running = || {
            *loads
                .queue
                .programs
                .running
                .lock()
                .unwrap_or_else(|e| e.into_inner())
        };
        let start = Instant::now();
        while !started.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(started.load(Ordering::SeqCst));
        // The thread unwinds after the renderer panicked.
        while running() > 0 && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(running(), 0);
    }

    /// Panics on a diagram that says `boom`, and draws others as an empty
    /// SVG.
    struct Boom;

    impl Renderer for Boom {
        fn draws(&self, _: &str) -> bool {
            true
        }

        fn render(&self, request: &Request) -> Result<crate::diagrams::Drawn, String> {
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
        let job = |source: &str| Job {
            key: ImageKey::block("mermaid", source),
            what: What::Diagram {
                language: "mermaid".to_owned(),
                source: source.to_owned(),
                line: 3,
                by_line: true,
            },
            program: false,
        };
        let context = Context {
            renderer: Box::new(Boom),
            theme: Theme::Light,
            panel: true,
            largest: None,
            max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        };
        // One thread, as no program draws: the second job waits for it.
        loads.start(vec![job("boom\n"), job("fine\n")], context);
        let mut came = Vec::new();
        let start = Instant::now();
        while came.len() < 2 && start.elapsed() < Duration::from_secs(5) {
            came.extend(loads.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(came.len(), 2, "{came:?}");
        assert_eq!(came[0].0, ImageKey::block("mermaid", "boom\n"));
        match &came[0].1 {
            Err(ImageError::Diagram { name, reason }) => {
                assert_eq!(name, "mermaid diagram on line 3");
                assert_eq!(reason, "the renderer crashed: boom");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(came[1].0, ImageKey::block("mermaid", "fine\n"));
        assert!(came[1].1.is_ok(), "{came:?}");
        assert!(loads.is_empty());
    }

    #[test]
    fn a_panic_is_told_by_its_message_when_it_has_one() {
        let url = What::Url {
            url: "a.png".to_owned(),
            base: PathBuf::new(),
        };
        let diagram = What::Diagram {
            language: "mermaid".to_owned(),
            source: "x\n".to_owned(),
            line: 2,
            by_line: true,
        };
        let told =
            |what: &What, panic: Box<dyn Any + Send>| crashed(what, panic.as_ref()).to_string();
        assert_eq!(
            told(&url, Box::new("bad pixels")),
            "broken image: the decoder crashed: bad pixels"
        );
        assert_eq!(
            told(&url, Box::new(String::from("bad pixels"))),
            "broken image: the decoder crashed: bad pixels"
        );
        assert_eq!(told(&url, Box::new(7)), "broken image: the decoder crashed");
        assert_eq!(
            told(&diagram, Box::new(String::from("no layout"))),
            "mermaid diagram on line 2: the renderer crashed: no layout"
        );
        assert_eq!(
            told(&diagram, Box::new(7)),
            "mermaid diagram on line 2: the renderer crashed"
        );
    }
}
