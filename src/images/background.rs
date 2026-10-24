//! Loading images on threads of their own, as the pager does after a
//! reload: the document shows at once, and each image comes as it is
//! ready. A load that a newer reload no longer wants stops, and one that
//! it still wants carries on.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
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
    /// How many programs outside kiri are drawing.
    programs: Arc<AtomicUsize>,
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
    programs: Arc<AtomicUsize>,
    sender: Sender<Done>,
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
            programs: Arc::new(AtomicUsize::new(0)),
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
        let start = Instant::now();
        while self.programs.load(Ordering::SeqCst) > 0 && start.elapsed() < STOPPING {
            std::thread::sleep(Duration::from_millis(5));
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
            if job.program {
                self.programs.fetch_add(1, Ordering::SeqCst);
            }
            let image = self.load(&job.what, &stop, &fonts);
            if job.program {
                self.programs.fetch_sub(1, Ordering::SeqCst);
            }
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
}
