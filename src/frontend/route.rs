//! The route: how the files that kiri is given are shown. One file, the
//! README of a directory, standard input or several files are opened, read
//! far enough to tell what they are, and shown in the pager as one stream,
//! or printed, each under a header that names it. A single file is the
//! special case of several, with no header, with reloading, following and
//! the warnings that name no file.
//!
//! What does not belong here is a decision that needs no I/O, such as
//! whether a run pages, which is in `src/session/plan.rs`, nor the opening
//! of one file (`open.rs`), the printing (`print.rs`) or the pager's loop
//! (`pager.rs`).

use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::config::Settings;
use crate::images::Stamps;
use crate::render::Inputs;
use crate::session::plan::{self, Route};
use crate::session::settings::{Follow, Paging};
use crate::session::sources::SourceFile;
use crate::typeset::caps::Capabilities;
use crate::typeset::diagnostic::Diagnostic;
use crate::typeset::input::Input;
use crate::typeset::model::Document;
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page::Wide;

use super::given::{Drawn, Given, Shown, drawn, given_from, load_given, read_whole};
use super::loads::{Guard, part};
use super::open::{Content, STDIN, open};
use super::pager::{Opening, page_files};
use super::print::print_files;
use super::{Failure, Platform, tell};

/// What the command line asks the frontend for.
pub struct Asked<'a> {
    /// The files given; `-` is standard input.
    pub files: &'a [PathBuf],
    /// The input format `--format` names.
    pub format: Option<Input>,
    /// How the command line asks to follow, `Off` when it does not.
    pub follow: Follow,
}

/// The settings, as the binary resolved them from the command line, the
/// environment and the configuration file.
pub struct Configured {
    /// Warnings about them.
    pub warnings: Vec<Diagnostic>,
    /// The settings; or why the configuration file could not be read,
    /// with the settings without it.
    pub settings: Result<Settings, (String, Settings)>,
    /// Whether reading mode was asked for by name, with `--reading`,
    /// `KIRI_READING` or the configuration file, and not left to the
    /// default. Only then is it told that it cannot be shown.
    pub reading_asked: bool,
}

/// Shows the files that `route` names: one file, the README of a directory,
/// standard input, or several files, one after another, each under a header
/// that names it. A file that cannot be shown is told about at its place
/// among the others, which are still shown; kiri then fails. One file has
/// no header, and the pager reads it again when it changes.
pub fn show_files(
    platform: &dyn Platform,
    asked: &Asked,
    route: Route,
    configured: Configured,
) -> Result<(), Failure> {
    let lone = route.is_alone();
    let paths: Vec<Option<&Path>> = match route {
        Route::Stdin => vec![None],
        _ => (asked.files.iter())
            .map(|path| Some(path.as_path()).filter(|path| *path != Path::new("-")))
            .collect(),
    };
    let mut opened = Vec::new();
    for path in paths {
        opened.push(match open(platform, asked.format, path) {
            // Nothing else is shown, so nothing is told before it.
            Err(error) if lone => return Err(error.into()),
            Ok(mut file) => {
                // Many image files may be given, more than kiri can keep
                // open, so a regular file is closed until it is shown.
                if let Content::Image { rest, .. } = &mut file.content
                    && !lone
                    && (file.file.as_deref()).is_some_and(|file| platform.is_file(file))
                {
                    *rest = None;
                }
                Ok(file)
            }
            Err(error) => Err((
                path.map_or_else(|| PathBuf::from(STDIN), Path::to_owned),
                error,
            )),
        });
    }
    let shown = || opened.iter().flatten();
    let files = plan::Files {
        route,
        images_only: shown().all(|file| matches!(file.content, Content::Image { .. })),
        alone: shown().all(|file| plan::shown_alone(file.input)),
        reloadable: shown().any(|file| file.reloadable(platform)),
        text: shown().any(|file| file.input == Input::Text),
    };
    let Configured {
        warnings,
        settings,
        reading_asked,
    } = configured;
    // A broken configuration file does not stop images; the documents are
    // not shown without it.
    let (documents, mut settings, broken) = match settings {
        Ok(settings) => (true, settings, None),
        Err((error, without)) => (false, without, Some(error)),
    };
    settings.paging = plan::paging(settings.paging, &files, settings.follow);
    settings.diagrams = plan::diagrams(settings.diagrams, &files);
    let context = Context::new(platform, asked.follow, reading_asked, &settings);
    let caps = platform.capabilities(context.mode, context.width, settings.passthrough);
    let caps = plan::on_page(caps, settings.paper);
    let would_page = plan::pages_alone(settings.paging, platform.stdout_is_terminal(), &caps);
    if plan::tells_warnings(&files, would_page) {
        for warning in &warnings {
            tell(platform, warning);
        }
    }
    let failed = Cell::new(false);
    if let Some(error) = broken.filter(|_| !files.images_only) {
        told(platform, &failed, error);
    }
    let run = Run {
        context: &context,
        settings,
        caps,
        documents,
        lone,
        reading: true,
        failed: &failed,
    };
    let given = (opened.into_iter())
        .filter_map(|opened| match opened {
            Ok(opened) => given_from(&run, opened),
            Err((path, error)) => Some(Given::failed(path, error)),
        })
        .collect();
    show_given(run, given)
}

/// What shows the files, as `show_files` decides it.
pub(super) struct Run<'a> {
    pub(super) context: &'a Context<'a>,
    /// With the paging for these files.
    pub(super) settings: Settings,
    /// For the terminal, with the paper that was asked for.
    pub(super) caps: Capabilities,
    /// Whether documents are shown: the configuration file could be read.
    pub(super) documents: bool,
    /// Whether it is one file, shown on its own.
    pub(super) lone: bool,
    /// Whether to tell that reading mode is not shown.
    pub(super) reading: bool,
    /// A file that cannot be shown is noted here, and kiri then fails.
    pub(super) failed: &'a Cell<bool>,
}

/// How the files are shown, once their paging is set.
#[derive(Clone, Copy)]
pub(super) struct Show<'a> {
    pub(super) context: &'a Context<'a>,
    pub(super) settings: &'a Settings,
    pub(super) lone: bool,
    pub(super) failed: &'a Cell<bool>,
}

/// Shows `given` as `run` says: in the pager as one stream when the paging
/// asks for it, and otherwise printed.
pub(super) fn show_given(run: Run, mut given: Vec<Given>) -> Result<(), Failure> {
    let Run {
        context,
        settings,
        caps,
        lone,
        reading,
        failed,
        ..
    } = run;
    let terminal = context.platform.stdout_is_terminal() && caps.styling;
    // In the pager, the files are one stream, read whole before it opens.
    let mut paged = terminal && settings.paging != Paging::Never;
    if paged {
        given = (given.into_iter())
            .map(|given| read_whole(context, &caps, given))
            .collect();
    }
    // Every file failed; an empty pager would only wait.
    if given.iter().all(Given::is_failed) {
        for given in &given {
            if let Shown::Failed(error) = &given.shown {
                told(context.platform, failed, error.clone());
            }
        }
        if failed.get() {
            return Err(Failure::Told);
        }
    }
    // Before the images are read, so that a file changed meanwhile counts
    // as changed.
    let file = (given.first().filter(|_| lone)).and_then(|given| source_file(given, context));
    // The documents' images load once the pager has opened.
    let later = paged && plan::may_load_later(settings.paging, settings.images, terminal, &caps);
    // Who catches the signals until the pager's screen does: the one guard
    // of the run, which lives on to the end when the files are printed.
    let mut guard = Guard::default();
    let mut images = load_given(context, &given, &settings, &caps, !later, &mut guard)?;
    for (number, file) in given.iter_mut().enumerate() {
        file.drawn = self::drawn(number, file, &mut images, &settings, &caps);
    }
    let drawn = given
        .first()
        .is_some_and(|given| matches!(given.drawn, Some(Drawn::Done)));
    paged = plan::paged_when_drawn(paged, lone, drawn, settings.paging);
    // Only the shape of what is shown decides this, so it stays a mapping.
    // A diagram drawn is a document by now, and not bare; as it needs a
    // terminal that draws images, nothing is told for it either way.
    let bare = given.first().is_some_and(|given| {
        matches!(
            given.shown,
            Shown::AsIs(_) | Shown::Image { .. } | Shown::Diagram { .. }
        )
    });
    let as_image = plan::pages_alone(settings.paging, terminal, &caps);
    let asked = plan::tells_reading(context.reading_asked && reading, lone, bare, as_image);
    let reading_warning = plan::reading_warning(asked, &caps);
    if let Some(warning) = &reading_warning {
        tell(context.platform, warning);
    }
    let mut inputs = Inputs {
        images,
        theme: settings.theme.clone(),
        front_matter: settings.front_matter.style().unwrap_or_default(),
        headings: settings.headings.with_images(settings.images),
        table_rows: settings.table_rows,
        render_errors: settings.render_errors,
        diagram_panel: settings.diagram_background_panel,
        reading: plan::reading(context.reading, &caps).then(|| context.reading_mode()),
        ..Inputs::new(caps)
    };
    let show = Show {
        context,
        settings: &settings,
        lone,
        failed,
    };
    if paged {
        let opening = Opening {
            later,
            reading_warning,
            file,
            guard,
        };
        page_files(&show, given, &mut inputs, opening)?;
    } else {
        shown(print_files(&show, given, &mut inputs))?;
    }
    match failed.get() {
        true => Err(Failure::Told),
        false => Ok(()),
    }
}

/// The file `given` is read from, when it is a document alone read from a
/// regular file, with how the file looked when it was read, which the
/// session has to read it again; with its images' stamps, taken now.
pub(super) fn source_file(given: &Given, context: &Context) -> Option<(SourceFile, Stamps)> {
    let (input, look) = given.reload?;
    let (Shown::Document { document, .. }, Some(path)) = (&given.shown, given.file.clone()) else {
        return None;
    };
    let stamps = Stamps::of(&[part(document, Some(&path), context.reading)]);
    Some((SourceFile { path, input, look }, stamps))
}

/// `warning` about the file `name`, as told: named among several files.
pub(super) fn told_line(lone: bool, name: &str, warning: Diagnostic) -> Diagnostic {
    match lone {
        true => warning,
        false => warning.in_file(name),
    }
}

/// What writing the output came to: a closed pipe (kiri *.md | head) takes
/// no more, which is no failure.
pub fn shown(result: io::Result<()>) -> Result<(), Failure> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(Failure::Error(error.to_string())),
        Ok(()) => Ok(()),
    }
}

/// Tells `error`, with which kiri is to fail, as `failed` then holds.
pub(super) fn told(platform: &dyn Platform, failed: &Cell<bool>, error: String) {
    tell(platform, error);
    failed.set(true);
}

/// Tells `drawn`, after its diagram file is shown, and notes in `failed`
/// whether kiri is to fail for it.
pub(super) fn tell_drawn(platform: &dyn Platform, drawn: &Drawn, failed: &Cell<bool>) {
    match drawn {
        Drawn::Failed(error) => told(platform, failed, error.to_string()),
        Drawn::Told => failed.set(true),
        Drawn::Hint(hint) => tell(platform, hint),
        Drawn::Done => {}
    }
}

/// What a run of kiri takes from its command line and its environment
/// below `show_files`, resolved.
pub(super) struct Context<'a> {
    /// The files, streams, environment and terminal it works with.
    pub(super) platform: &'a dyn Platform,
    pub(super) mode: crate::term::Mode,
    /// The width in cells that `--width` asks for.
    pub(super) width: Option<u16>,
    /// The largest image file to read, in bytes.
    pub(super) max_file_size: u64,
    /// How the command line asks to follow the file, so that the pager
    /// says when there is nothing to follow.
    pub(super) follow: Follow,
    /// Whether reading mode is on.
    pub(super) reading: bool,
    /// Whether it was asked for by name, so that it is told when it cannot
    /// be shown.
    pub(super) reading_asked: bool,
    /// What becomes of code blocks and tables wider than reading mode's
    /// area in the pager, as `--wide` says.
    pub(super) wide: Wide,
}

impl<'a> Context<'a> {
    pub(super) fn new(
        platform: &'a dyn Platform,
        follow: Follow,
        reading_asked: bool,
        settings: &Settings,
    ) -> Self {
        Context {
            platform,
            mode: settings.mode,
            width: settings.width,
            max_file_size: settings.max_image_size.0 * crate::images::MB,
            follow,
            reading: settings.reading,
            reading_asked,
            wide: settings.wide,
        }
    }

    /// Reading mode in the system's fonts, which load when text is first
    /// laid out in them, as `M` in the pager turns it on.
    pub(super) fn reading_mode(&self) -> Reading {
        Reading {
            pictures: Default::default(),
            fonts: Rc::new(self.platform.reading_fonts()),
            wide: self.wide,
        }
    }
}

/// Shows `document`, called `name`, as a report that kiri writes itself
/// is shown, not a file: on its own, with nothing to read again.
pub fn show_document(
    platform: &dyn Platform,
    follow: Follow,
    name: &str,
    document: Document,
    settings: Settings,
    caps: Capabilities,
) -> Result<(), Failure> {
    let shown = Shown::Document {
        document,
        images: true,
    };
    let given = Given::new(name.to_owned(), PathBuf::from(name), shown);
    let context = Context::new(platform, follow, false, &settings);
    let failed = Cell::new(false);
    let run = Run {
        context: &context,
        caps: plan::on_page(caps, settings.paper),
        settings,
        documents: true,
        lone: true,
        reading: false,
        failed: &failed,
    };
    show_given(run, vec![given])
}
