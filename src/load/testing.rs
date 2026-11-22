//! The test doubles of the loads, which the tests of other modules use too: a
//! runner that only notes the jobs (`Expecting`) and the loads it brings
//! (`Arrival`), a renderer that waits (`Gated`), threads that hold or run
//! builds at once, and the constructors of jobs and loads that use them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::diagrams::{FormulaColors, Renderer, Request, Theme};
use crate::images::{ImageError, ImageKey, Source};
use crate::load::jobs::{Context, Fetch, Job, What};
use crate::load::loads::{Loads, STOPPING, Wanted};
use crate::load::runner::{Done, Held, Queued, Runner, Threads};

impl Job {
    /// The job of the image file `url`, relative to a directory that does
    /// not exist, so that it fails to load at once, as a reload builds it
    /// with a plain context.
    pub(crate) fn file(url: &str) -> Job {
        Job::image(url, std::path::Path::new("/nonexistent"))
    }

    /// A job that must never run: the loads it is given to are run by an
    /// [`Expecting`] runner, which only notes it.
    pub(crate) fn named(key: impl Into<ImageKey>) -> Job {
        Job {
            key: key.into(),
            fetch: None,
            compute: What::Decode,
            program: false,
            context: Arc::new(context(crate::diagrams::NoRenderer)),
        }
    }

    /// The job of the image at `url`, relative to `base`.
    pub(crate) fn image(url: &str, base: &std::path::Path) -> Job {
        Job {
            key: url.into(),
            fetch: Some(Fetch::Url {
                url: url.to_owned(),
                base: base.to_path_buf(),
            }),
            compute: What::Decode,
            program: false,
            context: Arc::new(context(crate::diagrams::NoRenderer)),
        }
    }
}

/// Draws each diagram once `open` is set, or ends when its load is
/// stopped, and notes the source of each one it starts on.
#[derive(Clone, Default)]
pub(crate) struct Gated {
    pub(crate) started: Arc<Mutex<Vec<String>>>,
    pub(crate) open: Arc<AtomicBool>,
}

impl Gated {
    /// The jobs of the Mermaid diagrams `names`, each of one line, which
    /// this draws one at a time as no program does, keyed as diagrams drawn
    /// as `look` says are.
    pub(crate) fn jobs(&self, names: &[&str], look: u64) -> Vec<Job> {
        let context = Arc::new(context(self.clone()));
        (names.iter())
            .map(|name| {
                let job = diagram(name, false, &context);
                let key = job.key.with_look(look);
                Job { key, ..job }
            })
            .collect()
    }

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

impl Renderer for Gated {
    fn draws(&self, _: &str) -> bool {
        true
    }

    fn render(
        &self,
        request: &Request,
    ) -> Result<crate::diagrams::Drawn, crate::diagrams::DiagramError> {
        let stop = request.stop.expect("a stop flag");
        self.started
            .lock()
            .unwrap()
            .push(request.source.trim().to_owned());
        while !self.open.load(Ordering::SeqCst) && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        Err("drawn".into())
    }
}

/// A job for the diagram `source`, which a program draws or not, loaded as
/// `context` says.
pub(super) fn diagram(source: &str, program: bool, context: &Arc<Context>) -> Job {
    let source = format!("{source}\n");
    Job {
        key: ImageKey::rendered("mermaid", &source),
        fetch: None,
        compute: What::Diagram {
            language: "mermaid".to_owned(),
            source,
            line: 1,
            by_line: true,
        },
        program,
        context: Arc::clone(context),
    }
}

/// The jobs of the diagrams `names`, which `gated` draws one at a
/// time as no program draws them.
pub(super) fn diagrams(gated: &Gated, names: &[&str]) -> Vec<Job> {
    let context = Arc::new(context(gated.clone()));
    names
        .iter()
        .map(|name| diagram(name, false, &context))
        .collect()
}

/// What loading with `renderer` needs.
pub(super) fn context(renderer: impl Renderer + Send + Sync + 'static) -> Context {
    Context {
        renderer: Box::new(renderer),
        theme: Theme::Light,
        formula: FormulaColors::default(),
        panel: true,
        largest: None,
        max_file_size: crate::images::DEFAULT_MAX_FILE_SIZE,
        animated: false,
    }
}

impl Queued {
    /// The key of the image the job loads.
    pub(crate) fn key(&self) -> &ImageKey {
        &self.job.key
    }

    /// The image file the job reads, and the directory a relative path is
    /// read from.
    pub(crate) fn fetches(&self) -> Option<(&str, &std::path::Path)> {
        match self.job.fetch.as_ref()? {
            Fetch::Url { url, base } => Some((url.as_str(), base.as_path())),
        }
    }

    /// The load as a test's script brings it, to come on `done`.
    pub(crate) fn arrival(&self, done: &Sender<Done>) -> Arrival {
        Arrival {
            sender: done.clone(),
            id: self.id,
            key: self.job.key.clone(),
            stop: Arc::clone(&self.stop),
        }
    }
}

/// An image a test has loading, which comes when it says.
#[derive(Debug)]
pub(crate) struct Arrival {
    pub(super) sender: Sender<Done>,
    pub(super) id: u64,
    pub(super) key: ImageKey,
    pub(super) stop: Arc<AtomicBool>,
}

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
#[derive(Debug, Default, Clone)]
pub(crate) struct Expecting(pub(crate) std::rc::Rc<std::cell::RefCell<Vec<(ImageKey, Arrival)>>>);

impl Expecting {
    /// The first load noted that has not been taken, with its key.
    pub(crate) fn take(&self) -> (ImageKey, Arrival) {
        self.0.borrow_mut().remove(0)
    }
}

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

impl Loads {
    /// No loads, to run on threads, with the builds of frames run where they
    /// are started: a test cannot wait for a thread.
    pub(crate) fn inline_builds() -> Self {
        Self::with(Box::new(Threads::inline()))
    }

    /// No loads, to run on threads, with the builds of frames held back until
    /// the [`Held`] that comes with them releases them.
    pub(crate) fn held_builds() -> (Self, Held) {
        let held = Held::default();
        (Self::with(Box::new(Threads::holding(held.clone()))), held)
    }

    /// Has a program draw the diagram `key`, which ends `ending` after it
    /// is told to stop, and draws nothing; it draws when this returns.
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
            context: Arc::new(context(Slow(ending))),
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
    pub(crate) fn gated(&mut self, names: &[&str], gated: &Gated, look: u64) {
        let jobs = gated.jobs(names, look).into_iter().map(Wanted::Again);
        self.reconcile(self.adding(jobs.collect()));
    }
}

/// Draws until it is told to stop, and ends that long after, as a
/// program given time to close does.
struct Slow(Duration);

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
