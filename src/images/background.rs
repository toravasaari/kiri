//! Loading images on threads of their own, as the pager does after a
//! reload: the document shows at once, and each image comes as it is
//! ready. A load that a newer reload no longer wants stops, and one that
//! it still wants carries on.

use std::any::Any;
use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{self, AssertUnwindSafe};
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
/// Dropping it stops the loads, as [`Loads::stop`] does.
#[derive(Debug)]
pub struct Loads {
    sender: Sender<Done>,
    receiver: Receiver<Done>,
    /// The loads under way, by the key of their image.
    running: HashMap<ImageKey, Running>,
    /// The number of the next load.
    next: u64,
    /// The programs outside kiri drawing, across the loads of every
    /// reload.
    programs: Arc<Programs>,
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

/// The jobs of one reload, which its threads take one at a time.
struct Batch {
    queue: Mutex<VecDeque<(Job, u64, Arc<AtomicBool>)>>,
    context: Context,
    programs: Arc<Programs>,
    sender: Sender<Done>,
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
    /// Waits for room for one more program and counts it until the place
    /// it gives is dropped, also by a panic; `None`, with nothing counted,
    /// once `stop` is set.
    fn enter(&self, stop: &AtomicBool) -> Option<Place<'_>> {
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if stop.load(Ordering::SeqCst) {
                return None;
            }
            if *running < crate::diagrams::external::AT_ONCE {
                *running += 1;
                return Some(Place(self));
            }
            // Woken when one leaves; a stop is seen within the timeout.
            let wait = self
                .changed
                .wait_timeout(running, Duration::from_millis(20));
            running = wait.unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Waits until no program runs, for at most `limit`.
    fn wait_for_none(&self, limit: Duration) {
        let start = Instant::now();
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        while *running > 0 {
            let Some(left) = limit.checked_sub(start.elapsed()) else {
                return;
            };
            let wait = self.changed.wait_timeout(running, left);
            running = wait.unwrap_or_else(|e| e.into_inner()).0;
        }
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
const STOPPING: Duration = Duration::from_secs(5);

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
            programs: Arc::default(),
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
        self.keep(|_| false);
        self.programs.wait_for_none(STOPPING);
    }

    /// Loads `jobs` in the background as `settings` say; a load of the
    /// same image under way gives way to its job.
    pub(super) fn start(&mut self, jobs: Vec<Job>, context: Context) {
        if jobs.is_empty() {
            return;
        }
        let keys: HashSet<&ImageKey> = jobs.iter().map(|job| &job.key).collect();
        self.keep(|key| !keys.contains(key));
        let threads = match jobs.iter().any(|job| job.program) {
            true => crate::diagrams::external::AT_ONCE.min(jobs.len()),
            false => 1,
        };
        let mut queue = VecDeque::new();
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
            queue.push_back((job, id, stop));
        }
        let batch = Arc::new(Batch {
            queue: Mutex::new(queue),
            context,
            programs: Arc::clone(&self.programs),
            sender: self.sender.clone(),
        });
        let mut started = 0;
        for _ in 0..threads {
            // As large a stack as the main thread's, for a renderer built
            // into kiri.
            let thread = std::thread::Builder::new().stack_size(8 << 20);
            let batch = Arc::clone(&batch);
            if thread.spawn(move || batch.work()).is_err() {
                break;
            }
            started += 1;
        }
        // Without a thread of its own, the work is done here, as it would
        // be without loading in the background.
        if started == 0 {
            batch.work();
        }
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
    fn drop(&mut self) {
        self.stop();
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

impl Batch {
    /// Loads the jobs one after another until none is left, and sends
    /// each image that is still wanted.
    fn work(&self) {
        let fonts = Fonts::system();
        loop {
            let next = self
                .queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pop_front();
            let Some((job, id, stop)) = next else {
                break;
            };
            if stop.load(Ordering::SeqCst) {
                continue;
            }
            // Counted before the program can start, and so before it looks
            // at `stop`: stopping sets `stop` before it counts them.
            let place = match job.program {
                true => match self.programs.enter(&stop) {
                    Some(place) => Some(place),
                    None => continue,
                },
                false => None,
            };
            // A renderer or decoder that panics fails this load alone.
            let loaded =
                panic::catch_unwind(AssertUnwindSafe(|| self.load(&job.what, &stop, &fonts)));
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
            if self.sender.send(done).is_err() {
                break;
            }
        }
    }

    /// The image `what` names, loaded until `stop` is set.
    fn load(&self, what: &What, stop: &AtomicBool, fonts: &Fonts) -> Result<Source, ImageError> {
        let settings = &self.context;
        match what {
            What::Url { url, base } => load_url(url, base, settings.max_file_size, fonts),
            What::Diagram {
                language,
                source,
                line,
                by_line,
            } => {
                let renderer = settings.renderer.as_ref();
                let request = Request::new(language, source, settings.theme).stopped_by(stop);
                let drawn = renderer.render(&request);
                let options = LoadOptions {
                    renderer,
                    theme: settings.theme,
                    by_line: *by_line,
                    progress: None,
                    panel: settings.panel,
                    largest: settings.largest,
                };
                let max = settings.max_file_size;
                finish_diagram(language, source, *line, drawn, max, fonts, &options)
            }
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
}
