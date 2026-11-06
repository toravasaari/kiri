//! Showing the files in the pager as one stream: the documents, errors and
//! warnings of the files given are gathered, the images load first or in
//! the background, and the pager's loop runs on the terminal's screen. Where
//! the pager does not open, the files are printed after all.
//!
//! What does not belong here is the loop itself (`src/pager/`) or what a
//! key does (`src/session/`).

use std::cell::{Cell, RefCell};
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crate::config::Settings;
use crate::images::{Format, ImageKey, Images, Loads, Part, Placeholder, Stamps};
use crate::load::watch::SourceFiles;
use crate::pager::Terminal;
use crate::present::output::Page;
use crate::render::Inputs;
use crate::session::animation::Pacing;
use crate::session::loading::images_wanted;
use crate::session::plan::{self, Load};
use crate::session::settings::Paging;
use crate::session::sources::{Session, SourceFile};
use crate::term::WindowSize;
use crate::typeset::caps::Capabilities;
use crate::typeset::diagnostic::Diagnostic;
use crate::typeset::layout::Content;
use crate::typeset::model::{Block, Document};

use super::given::{Given, Shown, image_file};
use super::loads::{
    Guard, PlatformFiles, ROWS_UNLOADED, close_programs, finish_loading, load_parts, loading, part,
    read_image_file, surely_paged, wait_or_skip, waiting, without_path,
};
use super::open::image_reader;
use super::print::{Held, print_alone};
use super::route::{Context, Show, tell_drawn, told, told_line};
use super::{Failure, Platform, shown, tell};

/// The pager's screen: a [`crate::pager::Terminal`], with its clock, that
/// is left at the end.
pub trait Screen: crate::pager::Terminal {
    /// Leaves the screen and restores the terminal, runs `after`, and then
    /// ends kiri with a signal that asked it to end, if one came.
    fn close_then(self: Box<Self>, after: Box<dyn FnOnce() + '_>);
}

impl<W: Write> Screen for crate::term::Screen<W> {
    fn close_then(self: Box<Self>, after: Box<dyn FnOnce() + '_>) {
        (*self).close_then(after);
    }
}

/// The pager's loop takes a terminal of a size it knows; the screen the
/// platform opens is behind a box.
impl Terminal for Box<dyn Screen + '_> {
    fn size(&self) -> Option<WindowSize> {
        (**self).size()
    }

    fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        (**self).read(timeout)
    }

    fn now(&self) -> Instant {
        (**self).now()
    }
}

/// Ends the pager: stops what `loads` still load, leaves `screen` and
/// restores the terminal, and then waits for the programs that drew them to
/// end, with a line on standard error, a terminal, while that takes more
/// than a second. The one place where the loads stop when the pager quits;
/// the loop only stops them before it deletes the images.
pub(super) fn close(platform: &dyn Platform, screen: Box<dyn Screen + '_>, loads: &mut Loads) {
    loads.halt();
    screen.close_then(Box::new(|| close_programs(platform, loads)));
}

/// What is left to decide when the pager opens.
pub(super) struct Opening {
    /// Whether the documents' images load once the pager has opened.
    pub(super) later: bool,
    pub(super) reading_warning: Option<Diagnostic>,
    /// The file the document is read from, with the stamps of its images,
    /// when the pager can read it again.
    pub(super) file: Option<(SourceFile, Stamps)>,
    /// Who catches the signals until the pager's screen does.
    pub(super) guard: Guard,
}

/// An image file in the stream that is not read yet: only its size is known
/// to the layout, which is all that tells whether the pager opens.
struct Deferred {
    /// Its number among the files.
    number: usize,
    name: String,
    path: std::path::PathBuf,
    file: Option<std::path::PathBuf>,
    format: Format,
}

impl Deferred {
    /// Reads the file, for a terminal with `caps`, and puts the image in
    /// `inputs` in place of the size noted for it; whether the image is of
    /// that size, which the layout went by. Why it cannot be read, if it
    /// cannot, with the file's path first; the file has no image then.
    fn read(
        &self,
        context: &Context,
        caps: &Capabilities,
        inputs: &mut Inputs,
    ) -> Result<bool, String> {
        let key = ImageKey::from(self.name.as_str()).in_document(self.number);
        let reader = image_reader(context.platform, self.file.as_deref(), Vec::new(), None)
            .map_err(|error| format!("{}: {error}", self.path.display()))?;
        match read_image_file(context, caps, &self.path, self.format, reader) {
            Ok(image) => {
                let as_noted = inputs.images.is_of_extent(&key, &image);
                inputs.images.insert(key, Ok(image));
                Ok(as_noted)
            }
            Err(error) => {
                inputs.images.clear_extent(&key);
                Err(error)
            }
        }
    }
}

/// The warnings about `images` of `parts`, documents among `files`, under
/// each file's name, by the file's number among `files`.
fn told_files(
    images: &Images,
    parts: &[Part],
    files: &[crate::typeset::model::File],
    show: &Show,
) -> Vec<Vec<Diagnostic>> {
    let stderr = show.settings.render_errors.stderr();
    let mut told = vec![Vec::new(); files.len()];
    for part in parts {
        let name = &files[part.number].name;
        told[part.number] = (images.warnings_in(part.number, stderr).into_iter())
            .map(|warning| told_line(show.lone, name, warning))
            .collect();
    }
    told
}

/// Shows `given` in the pager as one stream, and prints them as `print_files`
/// does should it not open, with the images `inputs` holds. The image files
/// among them are read whole or, among several files, only measured, which
/// decides whether the pager opens: they are read when it opens, or as each
/// is printed. One file alone is shown as a document on its own is, without
/// a header, and the status line names it without its place among files.
/// A file that failed is in the pager's stream under its header, with why.
/// Why a file failed, and a warning about a file, are told before the
/// pager opens, and why the diagram of a diagram file was not drawn after
/// it closes; all of them are noted in `failed`. The guard of `opening`
/// goes once the pager's screen catches the signals. With `opening.later`,
/// the images of the documents load in the background once the pager has
/// opened; should it not open, they load before the files are printed.
#[expect(clippy::too_many_lines)]
pub(super) fn page_files(
    show: &Show,
    given: Vec<Given>,
    inputs: &mut Inputs,
    opening: Opening,
) -> Result<(), Failure> {
    let Show {
        context,
        settings,
        lone,
        failed,
    } = *show;
    let platform = context.platform;
    let Opening {
        later,
        reading_warning,
        file,
        mut guard,
    } = opening;
    let mut files = Vec::new();
    // What is told about each file before it is shown.
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    // What the pager tells of a file alone, as it was read.
    let mut warned_of = Vec::new();
    // What is told about each file after it is shown.
    let mut drawn = Vec::new();
    // The documents whose images load later, by number, and their files.
    let mut documents = Vec::new();
    // The image files that are not read until it is known that they are
    // shown.
    let mut deferred = Vec::new();
    // The key of the image file that is shown on its own, if it is.
    let mut alone = None;
    for (number, given) in given.into_iter().enumerate() {
        drawn.push(given.drawn);
        if let Shown::Document { images: true, .. } = given.shown
            && later
            && settings.images
        {
            documents.push((number, given.file.clone()));
        }
        let name = given.name;
        let mut error = None;
        let mut warned = Vec::new();
        for warning in &given.warnings {
            match lone {
                true => warned_of.push(warning.clone()),
                false => warned.push(warning.clone().in_file(&name)),
            }
        }
        let document = match given.shown {
            Shown::Failed(why) => {
                // The header names the file already.
                let reason = without_path(why.clone(), &given.path);
                error = Some(why);
                Document::new(vec![Block::Error(reason)])
            }
            Shown::Read(image) => {
                let key = ImageKey::from(name.as_str()).in_document(number);
                alone = lone.then(|| key.clone());
                inputs.images.insert(key, Ok(image));
                image_file(name.clone()).document
            }
            Shown::Sized { format, size, unit } => {
                let key = ImageKey::from(name.as_str()).in_document(number);
                inputs.images.set_extent(key, size, unit);
                deferred.push(Deferred {
                    number,
                    name: name.clone(),
                    path: given.path.clone(),
                    file: given.file.clone(),
                    format,
                });
                image_file(name.clone()).document
            }
            Shown::Document { document, .. } => {
                let stderr = settings.render_errors.stderr();
                for warning in inputs.images.warnings_in(number, stderr) {
                    warned.push(told_line(lone, &name, warning));
                }
                document
            }
            Shown::Diagram { document, .. } => document,
            // Plain output, the only one to get text as it is, has no
            // pager, nor do images read as they are shown.
            Shown::AsIs(text) => crate::typeset::parse::code(&text, None),
            Shown::Image { .. } => unreachable!("an image file in the pager is read or measured"),
        };
        errors.push(error);
        warnings.push(warned);
        files.push(crate::typeset::model::File { name, document });
    }
    let one = files.len() == 1;
    // The pager's status line names a lone file, and each of several, after
    // its source in the session.
    let mut session = Session::new(files.iter().map(|file| file.name.as_str()));
    // Only a lone file is read again, and it is the first.
    let mut source_files = None;
    if let Some((file, stamps)) = file {
        session.read_from(session.sources()[0].id(), file);
        source_files = Some(PlatformFiles {
            context,
            settings,
            stamps,
        });
    }
    let content = match one {
        true => Content::Document(&files[0].document),
        false => Content::Files(&files),
    };
    // The documents whose images load later, as parts.
    let parts_of = || -> Vec<Part> {
        (documents.iter())
            .map(|(number, file)| Part {
                number: *number,
                ..part(&files[*number].document, file.as_deref())
            })
            .collect()
    };
    let mut parts = parts_of();
    // With paging on auto, the images load first unless the files are
    // longer than the window without them.
    // Waiting, the reader can open the pager before they have all come,
    // and the rest then load in the background.
    let mut still = None;
    let load = (!parts.is_empty()).then(|| {
        plan::load(
            surely_paged(context, content, &parts, settings, inputs),
            settings.initial_load,
        )
    });
    let waited = match (load == Some(Load::Wait))
        .then(|| wait_or_skip(context, &parts, settings, inputs, &mut guard))
    {
        Some(Some(waited)) => Some(waited?),
        _ => None,
    };
    if let Some(loads) = waited {
        let told = told_files(&inputs.images, &parts, &files, show);
        for (warned, told) in warnings.iter_mut().zip(told) {
            warned.extend(told);
        }
        // The pager decides as it was told: with three rows for each
        // image that has not come.
        for key in loads.iter().flat_map(Loads::keys) {
            inputs
                .images
                .wait(key.clone(), Placeholder::Rows(ROWS_UNLOADED));
        }
        still = loads;
        parts.clear();
    } else if matches!(load, Some(Load::Now | Load::Wait)) {
        let images = load_parts(context, &parts, settings, &inputs.caps, &mut guard)?;
        let told = told_files(&images, &parts, &files, show);
        for (warned, told) in warnings.iter_mut().zip(told) {
            warned.extend(told);
        }
        inputs.images.extend(images);
        parts.clear();
    }
    let tell_before = |number: usize| {
        for warning in &warnings[number] {
            tell(platform, warning);
        }
        if let Some(error) = &errors[number] {
            told(platform, failed, error.clone());
        }
    };
    let told_before = Cell::new(false);
    let opening = || {
        (0..files.len()).for_each(tell_before);
        told_before.set(true);
        guard.caught();
    };
    // Several files, and an image file, are not read again, and there is
    // nothing to follow, which the pager says only when the command line
    // asked to follow.
    let options = crate::pager::Options {
        follow: match source_files {
            Some(_) => settings.follow,
            None => context.follow,
        },
        jump: settings.follow_jump,
        animation: Pacing::new(settings.animation_plays),
        warnings: (warned_of.into_iter()).chain(reading_warning).collect(),
        files: source_files
            .as_mut()
            .map(|files| files as &mut dyn SourceFiles),
        reading: Some(context.reading_mode()),
        // The images the documents want: those that load when the pager
        // opens, when `M` turns reading mode on, and for the document as
        // the last reload read it.
        planner: Some(Box::new(|shown, caps, _reading| {
            if !images_wanted(settings.images, caps) {
                return Vec::new();
            }
            let parts: Vec<Part> = match shown {
                Content::Document(document) => (documents.first().iter())
                    .map(|(_, file)| part(document, file.as_deref()))
                    .collect(),
                Content::Files(_) => parts_of(),
            };
            crate::images::jobs(&parts, &loading(context, settings, caps))
        })),
        background_offset: settings.background_offset,
        ..crate::pager::Options::new(settings.search_case)
    };
    // The loads the reader skipped carry on in the pager; its loop loads
    // the rest of what the documents want.
    let start = |loads: &mut Loads| {
        if let Some(still) = still.take() {
            *loads = still;
        }
    };
    // The image files are read once the pager opens, to show them. Each one
    // that cannot be read is told about, as its file's error is, before the
    // screen opens, and is returned by its number, with why, for the pager
    // to show as the error of the file.
    let failed_to_read = RefCell::new(Vec::new());
    let settle = |inputs: &mut Inputs| {
        let caps = inputs.caps.clone();
        let mut settled = Settled::default();
        for deferred in &deferred {
            match deferred.read(context, &caps, inputs) {
                // A file that changed since it was measured, or whose
                // header told another size, is laid out by the image.
                Ok(as_noted) => settled.sized &= as_noted,
                Err(error) => {
                    told(platform, failed, error.clone());
                    failed_to_read.borrow_mut().push(deferred.number);
                    let reason = without_path(error, &deferred.path);
                    settled.failures.push((deferred.number, reason));
                }
            }
        }
        settled
    };
    let opening = (opening, start, settle);
    if let Some(result) = page(
        context, content, session, inputs, settings, options, opening,
    ) {
        // What the pager would have hidden.
        (drawn.iter().flatten()).for_each(|drawn| tell_drawn(platform, drawn, failed));
        return shown(result);
    }
    // Printed after all, so the images load first, and what they tell is
    // told at each document's place; the guard lives on to the end, as it
    // does when they loaded before.
    let images_told = match (parts.is_empty(), still.take()) {
        (false, _) => {
            let images = load_parts(context, &parts, settings, &inputs.caps, &mut guard)?;
            let told = told_files(&images, &parts, &files, show);
            inputs.images.extend(images);
            told
        }
        // What the reader skipped comes first.
        (true, Some(still)) => {
            let came = finish_loading(
                &mut *waiting(platform),
                still,
                &mut inputs.images,
                &mut guard,
            )?;
            told_files(&came, &parts_of(), &files, show)
        }
        (true, None) => vec![Vec::new(); files.len()],
    };
    plan::printing(inputs.reading.as_mut().map(|reading| &mut reading.wide));
    // Printed, an animation shows its first frame, so the image files read
    // as they are printed are decoded as that alone; the output is the same
    // either way, it is the memory of the other frames that is saved.
    inputs.caps = plan::printing_still(inputs.caps.clone());
    let mut out = crate::term::wrapped(platform.stdout(), &inputs.caps);
    let mut apart = false;
    let held = Held::new(platform);
    for (number, file) in files.iter().enumerate() {
        if !told_before.get() {
            tell_before(number);
        }
        for warning in &images_told[number] {
            tell(platform, warning);
        }
        // A file that failed to read when the pager was to open has been
        // told of.
        if errors[number].is_some() || failed_to_read.borrow().contains(&number) {
            continue;
        }
        // An image file is read as it is printed, and let go after, so that
        // no more than one is held, unless the pager had read them.
        let unread = deferred.iter().find(|deferred| deferred.number == number);
        let key = ImageKey::from(file.name.as_str()).in_document(number);
        if let Some(unread) = unread {
            let caps = inputs.caps.clone();
            if inputs.images.get(&key).is_none()
                && let Err(error) = unread.read(context, &caps, inputs)
            {
                told(platform, failed, error);
                continue;
            }
        }
        // An image alone is printed as it is without paging, not on the
        // page of the theme.
        let image = alone.as_ref().and_then(|key| inputs.images.remove(key));
        let printed = match (one, image) {
            (true, Some(Ok(image))) => print_alone(image, Page::default(), inputs, &held, &mut out),
            (true, _) => crate::render::render(&file.document, inputs, &mut out)
                .and_then(|sent| held.check(sent, &inputs.images, &inputs.caps, &mut out)),
            (false, _) => crate::render::render_file(file, number, apart, inputs, &mut out)
                .and_then(|sent| held.check(sent, &inputs.images, &inputs.caps, &mut out)),
        };
        if unread.is_some() {
            inputs.images.remove(&key);
        }
        shown(printed)?;
        apart = true;
        if let Some(drawn) = &drawn[number] {
            tell_drawn(platform, drawn, failed);
        }
    }
    Ok(())
}

/// What reading the image files that were only measured came to.
struct Settled {
    /// The files that could not be read, by number, with why.
    failures: Vec<(usize, String)>,
    /// Whether every image read is of the size noted for it.
    sized: bool,
}

impl Default for Settled {
    fn default() -> Self {
        Self {
            failures: Vec::new(),
            sized: true,
        }
    }
}

/// The files of `content` with the documents of the image files that
/// could not be read, by number with why, in place of the image; `None`
/// when there are none.
fn repaired(
    content: Content,
    failures: Vec<(usize, String)>,
) -> Option<Vec<crate::typeset::model::File>> {
    let (Content::Files(files), false) = (content, failures.is_empty()) else {
        return None;
    };
    let mut files = files.to_vec();
    for (number, reason) in failures {
        files[number].document = Document::new(vec![Block::Error(reason)]);
    }
    Some(files)
}

/// Shows `content`, whose files are the sources of `session`, in the pager
/// when the paging of `settings` asks for it and the output is a terminal
/// that takes escape sequences, with the mouse taken if they say so; `None`
/// when it is to be printed instead. When it opens, `settle` reads the
/// image files that were only measured, once the screen is known to open
/// and before it does; it does not run when the files are printed.
/// `options` tell the pager how a search treats case and where `r` gets
/// the document again from, if anywhere. The pager catches Ctrl-C itself;
/// of `opening`, `caught` runs once its screen catches the signals, and
/// then `start`, which hands the pager the loads the reader skipped, if
/// any; the pager's loop starts the rest after its first layout.
fn page(
    context: &Context,
    content: crate::typeset::layout::Content,
    session: Session,
    inputs: &mut Inputs,
    settings: &Settings,
    options: crate::pager::Options,
    (caught, start, settle): (
        impl FnOnce(),
        impl FnOnce(&mut Loads),
        impl FnOnce(&mut Inputs) -> Settled,
    ),
) -> Option<io::Result<()>> {
    let paging = settings.paging;
    let platform = context.platform;
    let terminal = platform.stdout_is_terminal() && inputs.caps.styling;
    if !terminal || paging == Paging::Never {
        return None;
    }
    let stream = crate::typeset::layout::content(content, inputs);
    let rows = crate::pager::rows(&stream.lines, &inputs.caps);
    let height = platform.window().and_then(|window| window.height_in_rows());
    if !plan::pages(paging, terminal, rows, height) {
        return None;
    }
    // The image files that were only measured are read once the screen is
    // known to open, which `caught` runs after, and before it opens, so that
    // what is told of them is told on the terminal's main screen. Without a
    // terminal to read keys from, as in the background, they are not read,
    // for they are read one at a time as they are printed. A file that
    // could not be read is an error in the pager, as it is when it is known
    // before, and one that is not of the size noted for it is laid out by
    // the image it is. Either leaves the layout stale. The screen catches
    // the signals before the terminal is raw; the guard, which would
    // otherwise end kiri with the terminal raw, goes then.
    let mut settled = None;
    let tmux_passthrough = inputs.caps.tmux_passthrough;
    let screen = platform.screen(
        Box::new(|| {
            settled = Some(settle(inputs));
            caught();
        }),
        tmux_passthrough,
        settings.mouse,
    );
    let Some(mut screen) = screen else {
        // The document is printed, and the signals still end kiri; only a
        // reader who asked for the pager is told.
        if paging == Paging::Always {
            tell(
                platform,
                "no terminal to read keys from, so the document is printed",
            );
        }
        return None;
    };
    let settled = settled.unwrap_or_default();
    let intact = settled.failures.is_empty() && settled.sized;
    let repaired = repaired(content, settled.failures);
    let content = repaired.as_deref().map_or(content, Content::Files);
    // The images that still load when the pager quits, which it stops;
    // their programs may take a moment to end.
    let mut loads = platform.loads();
    start(&mut loads);
    // The layout before knew nothing of the images that load.
    let laid_out = (intact && !inputs.images.loading()).then_some(stream);
    let result = crate::pager::run(
        content,
        laid_out,
        session,
        inputs,
        crate::pager::Options {
            width: context.width,
            ..options
        },
        &mut loads,
        &mut screen,
    )
    .map(drop);
    close(platform, screen, &mut loads);
    // What the pager showed failed, as told before it opens when the
    // images load first: under each file's name among several.
    let stderr = inputs.render_errors.stderr();
    for (number, warning) in inputs.images.late_warnings(stderr) {
        let warning = match content {
            Content::Files(files) => warning.in_file(&files[number].name),
            Content::Document(_) => warning,
        };
        tell(platform, warning);
    }
    Some(result)
}
