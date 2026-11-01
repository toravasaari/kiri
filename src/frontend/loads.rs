//! The frontends' loading of a document's images and diagrams, and the
//! reloading of its file for the pager. Images load in the foreground while
//! the reader waits for the pager, on threads while the pager shows, and in
//! the background when the pager finds images it does not hold, which its
//! planner (`src/frontend/pager.rs`) tells it a document wants. A reload
//! reads the file again, brings the images up to date for it, and tells the
//! pager whether the file has changed since it was read.
//!
//! What does not belong here is the choice of what to show and when, which
//! `route.rs` makes from the settings, and the state of a session. The
//! functions here do the I/O and run the programs that draw diagrams; they
//! take the settings and the terminal's capabilities as input.
//!
//! This is the seed of the platform's work runner. The session that roadmap
//! item G7.3.3 adds will fill it in and replace the parts that do not fit it.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::config::Settings;
use crate::images::{
    Arrived, Fonts, Format, ImageError, ImageKey, Images, Loading, Loads, Part, Placeholder,
    Source, Stamps, reload_in_background,
};
use crate::pager::{Reload, Reloaded};
use crate::render::Inputs;
use crate::session::loading::Need;
use crate::session::plan;
use crate::session::state::{Change, Look};
use crate::term::progress::{DELAY, Step, until_over};
use crate::term::{Interrupt, WaitKeys};
use crate::typeset::caps::Capabilities;
use crate::typeset::input::Input;
use crate::typeset::model::Document;

use super::open::document;
use super::route::Context;
use super::{Failure, Platform};

/// The rows an image or diagram that has not loaded yet counts as, to
/// tell whether a document is longer than the window before it loads.
pub(crate) const ROWS_UNLOADED: u16 = 3;

/// Whether `content`, whose `parts` are to load their images in the
/// background, surely goes to the pager with `settings`: always with
/// paging `always`; with paging on `auto`, when it is longer than the
/// window laid out with `inputs` and each image of `parts` counted at
/// [`ROWS_UNLOADED`] rows, which then stand in for them in `inputs`
/// until they start loading.
pub(crate) fn surely_paged(
    context: &Context,
    content: crate::typeset::layout::Content,
    parts: &[Part],
    settings: &Settings,
    inputs: &mut Inputs,
) -> bool {
    let longer = plan::surely_paged(settings.paging, || {
        let loading = loading(context, settings, &inputs.caps);
        for key in crate::images::planned_keys(parts, &loading) {
            inputs.images.wait(key, Placeholder::Rows(ROWS_UNLOADED));
        }
        let stream = crate::typeset::layout::content(content, inputs);
        let rows = crate::pager::rows(&stream.lines, &inputs.caps);
        let height = context.platform.window().map(|window| window.rows);
        plan::Measured { rows, height }
    });
    if !longer {
        inputs.images.take_placeholders();
    }
    longer
}

/// What waiting for the images before the pager opens came to.
pub(crate) struct Waited {
    /// The guard that caught the signals meanwhile, to go once the
    /// pager's screen catches them.
    pub(crate) interrupt: Option<Interrupt>,
    /// The loads still under way, when the reader opened the pager
    /// before all had come.
    pub(crate) loads: Option<Loads>,
}

/// What the progress line counts while [`wait`] waits.
enum Line<'k> {
    /// Nothing is shown, as the wait finishes what the reader skipped.
    Quiet,
    /// The diagrams among the loads, by key: the line of the images
    /// loaded first, which counts only them.
    Diagrams(HashSet<ImageKey>),
    /// Every load, with the hint, while `keys` may skip the rest.
    Skippable(&'k mut WaitKeys),
}

/// How a [`wait`] ended.
enum Over {
    /// Nothing loads any more.
    Done,
    /// The reader pressed Space or Enter; the rest loads on.
    Skipped,
    /// A signal came; the loads are not stopped yet.
    Signal(i32),
}

/// Waits for what `loads` load, giving each image to `came` as it comes,
/// until nothing loads, `signal` tells of one, or with [`Line::Skippable`]
/// the reader presses Space or Enter. The line is wiped before it
/// returns; it shows after `delay`. `wait` stops nothing itself, and the caller owns the keys: it
/// drops them, so that the terminal is as it was, before it stops the
/// loads on a signal.
fn wait(
    (out, delay): (impl Write, Duration),
    loads: &mut Loads,
    mut came: impl FnMut(ImageKey, Result<Source, ImageError>),
    mut line: Line,
    signal: impl Fn() -> Option<i32>,
) -> Over {
    let hint = matches!(line, Line::Skippable(_));
    let mut over = Over::Done;
    let mut diagrams = 0;
    let step = |most: Duration| {
        match &mut line {
            // What comes while it waits for a key stays in the loads.
            Line::Skippable(keys) => {
                if keys.skip(most) {
                    over = Over::Skipped;
                    return Step::Skipped;
                }
            }
            Line::Quiet | Line::Diagrams(_) => {
                if !most.is_zero() {
                    loads.wait_by(Instant::now() + most);
                }
            }
        }
        for Arrived { key, image, .. } in loads.poll() {
            if let Line::Diagrams(keys) = &line {
                diagrams += usize::from(keys.contains(&key));
            }
            came(key, image);
        }
        if let Some(signal) = signal() {
            over = Over::Signal(signal);
            return Step::Over;
        }
        match (loads.progress(), &line) {
            (None, _) => Step::Over,
            (Some((done, total)), Line::Skippable(_)) => Step::Working(done, total),
            (Some(_), Line::Diagrams(keys)) => Step::Working(diagrams, keys.len()),
            (Some(_), Line::Quiet) => Step::Working(0, 0),
        }
    };
    until_over(out, delay, hint, step);
    over
}

/// Loads the images of `parts` into `inputs` while the reader waits for
/// the pager, with the progress line on standard error, a terminal, and
/// the hint that Space opens the pager now: until they have all come, or
/// until the reader presses Space or Enter, which leaves the rest loading
/// in the background. Other keys go unheeded. Ctrl-C, SIGTERM, SIGHUP and
/// Ctrl-\ stop the programs meanwhile, as when the images load first,
/// and are then the failure. `None` without a terminal to read keys
/// from, or without the line to tell of them, standard error not being
/// a terminal, where nothing has started.
pub(crate) fn wait_or_skip(
    context: &Context,
    parts: &[Part],
    settings: &Settings,
    inputs: &mut Inputs,
) -> Option<Result<Waited, Failure>> {
    if !context.platform.stderr_is_terminal() {
        return None;
    }
    let mut keys = WaitKeys::open()?;
    let interrupt = Interrupt::catch();
    let signal = || interrupt.as_ref().and_then(Interrupt::signal);
    let mut loads = Loads::new();
    let loading = loading(context, settings, &inputs.caps);
    crate::images::load_in_background(parts, &loading, &mut inputs.images, &mut loads);
    let images = &mut inputs.images;
    let came = |key, image| images.insert(key, image);
    let line = Line::Skippable(&mut keys);
    let out = (context.platform.stderr(), DELAY);
    let over = wait(out, &mut loads, came, line, signal);
    // The terminal as it was, before the pager or the shell takes it.
    drop(keys);
    if let Over::Signal(signal) = over {
        loads.halt();
        loads.wait(crate::images::STOPPING);
        let signal = interrupt
            .as_ref()
            .and_then(Interrupt::release)
            .unwrap_or(signal);
        return Some(Err(Failure::Signal(signal)));
    }
    let loads = matches!(over, Over::Skipped).then_some(loads);
    Some(Ok(Waited { interrupt, loads }))
}

/// Waits for what `loads` load to come into `images`, and gives what
/// came. A signal that `interrupt` caught meanwhile stops them, and is
/// the failure.
pub(crate) fn finish_loading(
    mut loads: Loads,
    images: &mut Images,
    interrupt: Option<&Interrupt>,
) -> Result<Images, Failure> {
    let mut came = Images::new();
    let over = wait(
        (std::io::sink(), DELAY),
        &mut loads,
        |key, image| {
            images.insert(key.clone(), image.clone());
            came.insert(key, image);
        },
        Line::Quiet,
        || interrupt.and_then(Interrupt::signal),
    );
    if let Over::Signal(signal) = over {
        loads.halt();
        loads.wait(crate::images::STOPPING);
        let signal = interrupt.and_then(Interrupt::release).unwrap_or(signal);
        return Err(Failure::Signal(signal));
    }
    Ok(came)
}

/// A document's images, and the guard that caught the signals while
/// programs drew its diagrams, if any did.
#[derive(Debug, Default)]
pub(crate) struct Loaded {
    pub(crate) images: Images,
    /// Keeps the signals that the programs were stopped by from ending
    /// kiri by surprise later: it lives until something else catches them,
    /// such as the pager's screen, or until kiri ends.
    pub(crate) interrupt: Option<Interrupt>,
}

/// The images of the documents in `parts`, loaded through the queue as
/// the pager's loads are, whatever the settings say about images, in the
/// order of the documents. When programs draw any of their diagrams,
/// Ctrl-C, SIGTERM, SIGHUP and Ctrl-\ stop them meanwhile, and are then
/// the failure. A terminal tells how far the diagrams have got.
pub(crate) fn load_parts(
    context: &Context,
    parts: &[Part],
    settings: &Settings,
    caps: &Capabilities,
) -> Result<Loaded, Failure> {
    let loading = loading(context, settings, caps);
    let jobs = crate::images::jobs(parts, &loading);
    if jobs.is_empty() {
        return Ok(Loaded::default());
    }
    // Only while a program draws one of the documents' diagrams: they
    // otherwise end kiri at once, as without diagrams.
    let interrupt = match jobs.iter().any(|job| job.program) {
        true => Interrupt::catch(),
        false => None,
    };
    let mut loads = Loads::new();
    loads.want(
        &jobs,
        (0..jobs.len()).map(|at| (at, Need::Missing)).collect(),
    );
    // Drawing diagrams can take seconds with an external program; a
    // terminal tells how far it has got.
    let diagrams = (jobs.iter().filter(|job| job.draws_diagram())).map(|job| job.key.clone());
    let out: Box<dyn Write + Send + '_> = match context.platform.stderr_is_terminal() {
        true => context.platform.stderr(),
        false => Box::new(std::io::sink()),
    };
    let mut came = HashMap::new();
    let over = wait(
        (out, DELAY),
        &mut loads,
        |key, image| {
            came.insert(key, image);
        },
        Line::Diagrams(diagrams.collect()),
        || interrupt.as_ref().and_then(Interrupt::signal),
    );
    if let Over::Signal(signal) = over {
        loads.halt();
        loads.wait(crate::images::STOPPING);
        let signal = interrupt
            .as_ref()
            .and_then(Interrupt::release)
            .unwrap_or(signal);
        return Err(Failure::Signal(signal));
    }
    if let Some(signal) = interrupt.as_ref().and_then(Interrupt::release) {
        return Err(Failure::Signal(signal));
    }
    // In the order of the documents, as they were planned and not as they
    // came.
    let mut images = Images::new();
    for job in jobs {
        if let Some(image) = came.remove(&job.key) {
            images.insert(job.key, image);
        }
    }
    Ok(Loaded { images, interrupt })
}

/// What loading images needs to know, for a window with `caps`.
pub(crate) fn loading<'a>(
    context: &'a Context,
    settings: &'a Settings,
    caps: &Capabilities,
) -> Loading<'a> {
    Loading {
        env: context.platform.environment(),
        settings,
        max_file_size: context.max_file_size,
        theme: crate::diagrams::Theme::for_page(caps.dark_background),
        formula: crate::typeset::images::formula_colors(&settings.theme, caps),
        largest: Some(crate::typeset::images::size::largest(caps)),
        headings: crate::render::heading_look(
            settings.headings,
            !settings.images,
            &settings.theme,
            caps,
        ),
    }
}

/// `document`, from `file` or not, whose images are looked for beside the
/// file, as a part whose images are loaded.
pub(crate) fn part<'a>(
    document: &'a Document,
    file: Option<&'a Path>,
    inline_images: bool,
) -> Part<'a> {
    Part {
        document,
        number: 0,
        base: file.and_then(Path::parent).unwrap_or(Path::new("")),
        by_line: file.is_some(),
        inline_images,
    }
}

/// Waits for the programs that drew `loads`, stopped, to end, with a line
/// on standard error, a terminal, while that takes more than a second.
pub(crate) fn close_programs(platform: &dyn Platform, loads: &Loads) {
    let limit = crate::images::STOPPING;
    let wait = |most| loads.wait(most);
    if !platform.stderr_is_terminal() {
        wait(limit);
        return;
    }
    let text = "kiri: closing background programs…";
    let second = std::time::Duration::from_secs(1);
    crate::term::progress::while_waiting(platform.stderr(), second, limit, text, wait);
}

/// Reads the document from its file again for the pager, and brings the
/// images it has up to date; the pager loads the ones it does not have.
pub(crate) struct FileReload<'a> {
    pub(crate) context: &'a Context<'a>,
    pub(crate) settings: &'a Settings,
    /// The regular file the document was read from, as `input`.
    pub(crate) path: PathBuf,
    pub(crate) input: Input,
    /// When the files behind the images were last changed.
    pub(crate) stamps: Stamps,
    /// How the file looked when it was last read, when it could be
    /// looked at.
    pub(crate) read: Option<Look>,
}

impl Reload for FileReload<'_> {
    fn reload(
        &mut self,
        images: &mut Images,
        loads: &mut Loads,
        caps: &Capabilities,
        reading: bool,
    ) -> Reloaded {
        let (path, input) = (self.path.as_path(), self.input);
        // Before the file is read, so that a change meanwhile counts.
        self.read = self.context.platform.look(path).ok();
        // The status line names the file already, so the reasons do not.
        let source = match read_text(self.context.platform, path) {
            Ok(source) => source,
            Err(error) => return Reloaded::Failed(error),
        };
        let mut warnings = Vec::new();
        let warn = |warning| warnings.push(warning);
        let document = match document(input, &source, path, self.settings, caps, warn) {
            Ok(Some(document)) => document,
            Ok(None) => return Reloaded::Failed("cannot be shown".to_owned()),
            Err(error) => return Reloaded::Failed(without_path(error, path)),
        };
        if crate::session::loading::images_wanted(self.settings.images, caps) {
            let parts = [part(&document, Some(path), reading)];
            let loading = loading(self.context, self.settings, caps);
            reload_in_background(&parts, &loading, images, &mut self.stamps, loads);
        }
        match warnings.is_empty() {
            true => Reloaded::Document(document),
            false => Reloaded::Warned(document, warnings),
        }
    }

    fn change(&mut self) -> Change {
        match self.context.platform.look(&self.path) {
            Ok(look) if Some(look) == self.read => Change::Unchanged,
            Ok(look) => Change::Changed(look),
            Err(why) => Change::Gone(why),
        }
    }
}

/// The text of the file at `path`, read whole; why it cannot be read
/// otherwise.
fn read_text(platform: &dyn Platform, path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    (platform.open(path))
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_owned())
}

/// `error` without the `path: ` it starts with, if it does.
pub(crate) fn without_path(error: String, path: &Path) -> String {
    let prefix = format!("{}: ", path.display());
    match error.strip_prefix(&prefix) {
        Some(reason) => reason.to_owned(),
        None => error,
    }
}

/// The image file `path`, in `format`, read from `reader` up to the size
/// limit, for a terminal with `caps`, which must draw images.
pub(crate) fn read_image_file(
    context: &Context,
    caps: &Capabilities,
    path: &Path,
    format: Format,
    reader: impl Read,
) -> Result<Source, String> {
    if !caps.draws_images() {
        return Err(format!(
            "{}: the terminal cannot show images",
            path.display()
        ));
    }
    let max_file_size = context.max_file_size;
    crate::images::read_image(reader, path, format, max_file_size, &Fonts::system()).map_err(
        |error| match error {
            error if error.names_file() => error.to_string(),
            error => format!("{}: {error}", path.display()),
        },
    )
}

#[cfg(test)]
mod tests;
