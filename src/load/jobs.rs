//! The kinds of work the loads do: what is fetched, what is computed from
//! it, and how an image is loaded from a job. A job is owned data, so that
//! a thread of its own can run it; the runner (`runner.rs`) decides where.

use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
#[cfg(test)]
use std::sync::{Mutex, atomic::Ordering};
#[cfg(test)]
use std::time::{Duration, Instant};

use crate::diagrams::{FormulaColors, Renderer, Request, Theme};
use crate::images::{
    Fonts, HeadingWord, ImageError, ImageKey, LevelLook, LoadOptions, Source, draw_heading,
};
use crate::load::url::{Fetched, fetch_url};
use crate::session::state::Look;
use crate::typeset::images::decode::decode_named_as;
use crate::typeset::images::drawn::finish_diagram;

/// What is read before an image is computed, as owned data, so that a
/// thread of its own can read it.
#[derive(Debug, Clone)]
pub(crate) enum Fetch {
    /// An image file or `data:` URL, with relative paths from `base`.
    Url { url: String, base: PathBuf },
}

impl Fetch {
    /// Reads it, refusing a file larger than `max_file_size` bytes.
    fn run(&self, max_file_size: u64) -> Result<Fetched, ImageError> {
        match self {
            Fetch::Url { url, base } => fetch_url(url, base, max_file_size),
        }
    }
}

/// An image to compute from what was fetched, as owned data, so that a
/// thread of its own can compute it.
#[derive(Debug, Clone)]
pub(crate) enum What {
    /// The image that the bytes fetched hold.
    Decode,
    /// A code block drawn as a diagram, as `Wanted::Diagram` is, named in a
    /// warning by its line or not.
    Diagram {
        language: String,
        source: String,
        line: usize,
        by_line: bool,
    },
    /// A heading drawn as an image as `look` says, in cells of `cell`
    /// pixels.
    Heading {
        words: Vec<HeadingWord>,
        look: LevelLook,
        cell: (u16, u16),
    },
}

/// One image to load: its key, what is read before computing it, what it
/// is, whether a program outside kiri draws it, and what loading it needs
/// besides, which the jobs of one call share.
#[derive(Clone)]
pub struct Job {
    pub(crate) key: ImageKey,
    /// What is read first; `None` when the job holds all it needs, as a
    /// diagram's source or a heading's words.
    pub(crate) fetch: Option<Fetch>,
    pub(crate) compute: What,
    /// Whether a program outside kiri draws it (a program place).
    pub(crate) program: bool,
    pub(crate) context: Arc<Context>,
}

impl Job {
    /// Whether it draws a code block as a diagram.
    pub fn draws_diagram(&self) -> bool {
        matches!(self.compute, What::Diagram { .. })
    }

    /// The job of the image file `url`, relative to a directory that does
    /// not exist, so that it fails to load at once, as a reload builds it
    /// with a plain context.
    #[cfg(test)]
    pub(crate) fn file(url: &str) -> Job {
        Job::image(url, std::path::Path::new("/nonexistent"))
    }

    /// A job that must never run: the loads it is given to are run by an
    /// [`Expecting`] runner, which only notes it.
    #[cfg(test)]
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
    #[cfg(test)]
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

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job")
            .field("key", &self.key)
            .field("fetch", &self.fetch)
            .field("compute", &self.compute)
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

/// What loading the jobs of one call of an entry point needs, besides each
/// job.
pub(crate) struct Context {
    pub(crate) renderer: Box<dyn Renderer + Send + Sync>,
    pub(crate) theme: Theme,
    pub(crate) formula: FormulaColors,
    pub(crate) panel: bool,
    pub(crate) largest: Option<(u32, u32)>,
    pub(crate) max_file_size: u64,
    /// Whether an animated image is decoded with all its frames.
    pub(crate) animated: bool,
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

#[cfg(test)]
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
#[cfg(test)]
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

/// What loading with `renderer` needs.
#[cfg(test)]
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

/// The image that `job` is: what it fetches read, and then computed as its
/// context says until `stop` is set; and how the file read looked.
pub(super) fn load(
    job: &Job,
    stop: &AtomicBool,
    fonts: &Fonts,
) -> (Result<Source, ImageError>, Option<Look>) {
    let context = &job.context;
    let fetched = match (job.fetch.as_ref()).map(|fetch| fetch.run(context.max_file_size)) {
        Some(Err(error)) => return (Err(error), None),
        Some(Ok(fetched)) => Some(fetched),
        None => None,
    };
    let look = fetched.as_ref().and_then(|fetched| fetched.look);
    (compute(context, &job.compute, fetched, stop, fonts), look)
}

/// The image `what` names, computed as `context` says from what was
/// `fetched`, until `stop` is set.
pub(super) fn compute(
    context: &Context,
    what: &What,
    fetched: Option<Fetched>,
    stop: &AtomicBool,
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    match what {
        What::Decode => {
            let Some(fetched) = fetched else {
                unreachable!("a job that decodes fetches what it decodes");
            };
            let (bytes, named) = (&fetched.bytes, fetched.named);
            decode_named_as(bytes, named, context.max_file_size, fonts, context.animated)
        }
        What::Diagram {
            language,
            source,
            line,
            by_line,
        } => {
            let renderer = context.renderer.as_ref();
            let request = Request::new(language, source, context.theme)
                .in_colors(context.formula)
                .stopped_by(stop);
            let drawn = renderer.render(&request);
            let options = LoadOptions {
                renderer,
                theme: context.theme,
                formula: context.formula,
                by_line: *by_line,
                progress: None,
                panel: context.panel,
                largest: context.largest,
                headings: None,
            };
            let max = context.max_file_size;
            finish_diagram(language, source, *line, drawn, max, fonts, &options)
        }
        What::Heading { words, look, cell } => draw_heading(words, look, *cell, fonts),
    }
}

/// Why loading `what` failed when it panicked with `panic`: a diagram
/// fails as one its renderer could not draw, an image as a broken one,
/// with the panic's message when it has one.
pub(super) fn crashed(what: &What, panic: &(dyn Any + Send)) -> ImageError {
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
            reason: with("the renderer crashed").into(),
        },
        What::Decode | What::Heading { .. } => ImageError::Corrupt(with("the decoder crashed")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_formula_loaded_in_the_background_is_in_the_text_colour() {
        let mut context = context(crate::diagrams::BuiltIn);
        context.formula.text = Some([0x12, 0x34, 0x56]);
        let what = What::Diagram {
            language: "math".to_owned(),
            source: "\\rule{1em}{1em}\n".to_owned(),
            line: 1,
            by_line: true,
        };
        let loaded = compute(
            &context,
            &what,
            None,
            &AtomicBool::new(false),
            &Fonts::none(),
        );
        let image = loaded.unwrap().render(8, 8);
        assert_eq!(image.get_pixel(4, 4).0, [0x12, 0x34, 0x56, 255]);
    }

    #[test]
    fn a_panic_is_told_by_its_message_when_it_has_one() {
        let url = What::Decode;
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
