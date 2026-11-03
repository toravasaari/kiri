//! A file among several, ready to show: what it is shown as (`Shown`), what
//! is told after it (`Drawn`), and the loading of the images of the
//! documents among them.
//!
//! What does not belong here is the opening of a file (`open.rs`), nor how
//! the files are shown together, in the pager or printed (`pager.rs` and
//! `print.rs`).

use std::io::Read;
use std::path::PathBuf;

use crate::config::Settings;
use crate::images::{Format, ImageError, ImageKey, Images, Part, Source};
use crate::session::plan;
use crate::session::state::Look;
use crate::typeset::caps::Capabilities;
use crate::typeset::diagnostic::{Code, Diagnostic};
use crate::typeset::input::Input;
use crate::typeset::model::{Block, Document};

use super::loads::{Guard, load_parts, part, read_image_file};
use super::open::{Content, Opened, document, image_reader};
use super::route::{Context, Run};
use super::{Failure, tell};

/// The file `opened`, ready to show; `None` for text that is not shown, as
/// the configuration file could not be read.
pub(super) fn given_from(run: &Run, opened: Opened) -> Option<Given> {
    let (settings, caps, lone) = (&run.settings, &run.caps, run.lone);
    let reload =
        (lone && opened.reloadable(run.context.platform)).then_some((opened.input, opened.look));
    // A pipe given by its path is read to its end already, so the images
    // of a document alone from one are relative to the current directory.
    let alone = plan::shown_alone(opened.input);
    let platform = run.context.platform;
    let file = (opened.file).filter(|file| alone || !lone || platform.is_file(file));
    let name = opened.path.display().to_string();
    let mut warnings = Vec::new();
    let warn = |warning: Diagnostic| {
        // On standard error at once for a file alone, and in the pager too.
        if lone {
            tell(platform, &warning);
        }
        warnings.push(warning);
    };
    let shown = match opened.content {
        Content::Image { format, head, rest } => Shown::Image { format, head, rest },
        // The configuration file's error tells why.
        Content::Text(_) if !run.documents => return None,
        Content::Text(source) => match opened.input {
            Input::Diagram(_) if !caps.styling => Shown::AsIs(source),
            Input::Diagram(language) => Shown::Diagram {
                document: crate::typeset::parse::code(&source, Some(language)),
                language,
                source,
            },
            input => match document(input, &source, &opened.path, settings, caps, warn) {
                Ok(Some(document)) => Shown::Document {
                    document,
                    images: input != Input::Text,
                },
                Ok(None) => Shown::AsIs(source),
                Err(error) => Shown::Failed(error),
            },
        },
    };
    Some(Given {
        name,
        path: opened.path,
        file,
        shown,
        warnings,
        drawn: None,
        reload,
    })
}

/// `given`, an image file read whole, to be shown in a stream of files
/// that its reader cannot wait for, such as the pager's, or failed with why
/// it cannot be read; any other file as it is.
pub(super) fn read_whole(context: &Context, caps: &Capabilities, mut given: Given) -> Given {
    given.shown = match std::mem::replace(&mut given.shown, Shown::AsIs(String::new())) {
        Shown::Image { format, head, rest } => {
            let image = image_reader(context.platform, given.file.as_deref(), head, rest)
                .map_err(|error| format!("{}: {error}", given.path.display()))
                .and_then(|reader| read_image_file(context, caps, &given.path, format, reader));
            match image {
                Ok(image) => Shown::Read(image),
                Err(error) => Shown::Failed(error),
            }
        }
        shown => shown,
    };
    given
}

/// A file among several, ready to show.
pub(super) struct Given {
    /// What its header calls it.
    pub(super) name: String,
    /// What errors and warnings call it.
    pub(super) path: PathBuf,
    /// The file, or `None` for standard input.
    pub(super) file: Option<PathBuf>,
    pub(super) shown: Shown,
    /// What is told about it before it is shown, besides its images. A
    /// file alone has told them already, and the pager tells them too.
    pub(super) warnings: Vec<Diagnostic>,
    /// What is told after it is shown: why its diagram was not drawn, for
    /// a diagram file.
    pub(super) drawn: Option<Drawn>,
    /// For a document alone read from a regular file: how it was read, so
    /// that the pager can read it again.
    pub(super) reload: Option<(Input, Option<Look>)>,
}

/// What a file among several is shown as.
pub(super) enum Shown {
    /// An image in `format`, read when it is shown, as `image_reader`
    /// reads it.
    Image {
        format: Format,
        head: Vec<u8>,
        rest: Option<Box<dyn Read>>,
    },
    /// An image file read whole.
    Read(Source),
    /// A document, with its images unless it is text.
    Document { document: Document, images: bool },
    /// A diagram file in `language`: `source`, and the document of its
    /// code, shown where it is not drawn.
    Diagram {
        language: &'static str,
        source: String,
        document: Document,
    },
    /// Text written as it is.
    AsIs(String),
    /// Nothing, as the file cannot be shown: why, told as an error.
    Failed(String),
}

impl Given {
    /// The file called `name` that is `shown`, with nothing told about it.
    pub(super) fn new(name: String, path: PathBuf, shown: Shown) -> Self {
        Given {
            name,
            path,
            file: None,
            shown,
            warnings: Vec::new(),
            drawn: None,
            reload: None,
        }
    }

    /// The file at `path`, which cannot be shown for `error`.
    pub(super) fn failed(path: PathBuf, error: String) -> Self {
        Self::new(path.display().to_string(), path, Shown::Failed(error))
    }

    /// Whether the file cannot be shown.
    pub(super) fn is_failed(&self) -> bool {
        matches!(self.shown, Shown::Failed(_))
    }
}

/// What is told after a diagram file is shown.
pub(super) enum Drawn {
    /// It was drawn: the file is its image, and there is nothing to tell.
    Done,
    /// It could not be drawn: why, told as an error.
    Failed(Diagnostic),
    /// It could not be drawn, and the document tells why.
    Told,
    /// Nothing could draw it.
    Hint(Diagnostic),
}

/// The images of the documents among `given`, loaded and drawn as
/// `load_parts` loads them, keyed by the number of each among them. The
/// diagram of a diagram file is drawn also with images off, as it is on
/// its own, and text has none.
pub(super) fn load_given(
    context: &Context,
    given: &[Given],
    settings: &Settings,
    caps: &Capabilities,
    documents: bool,
    guard: &mut Guard,
) -> Result<Images, Failure> {
    if !caps.draws_images() {
        return Ok(Images::new());
    }
    let parts: Vec<Part> = (given.iter().enumerate())
        .filter_map(|(number, given)| {
            let document = match &given.shown {
                Shown::Document { document, images } if *images && settings.images && documents => {
                    document
                }
                Shown::Diagram { document, .. } => document,
                _ => return None,
            };
            let file = given.file.as_deref();
            Some(Part {
                number,
                ..part(document, file)
            })
        })
        .collect();
    load_parts(context, &parts, settings, caps, guard)
}

/// The diagram file `given`, numbered `number` among the files shown,
/// made an image file where its diagram in `images` was drawn: the image
/// is then keyed by the file's name. Otherwise what is told after it is
/// shown, as for a diagram file on its own.
pub(super) fn drawn(
    number: usize,
    given: &mut Given,
    images: &mut crate::images::Images,
    settings: &Settings,
    caps: &Capabilities,
) -> Option<Drawn> {
    let Shown::Diagram {
        language, source, ..
    } = &given.shown
    else {
        return None;
    };
    let language = *language;
    let look = crate::typeset::images::diagram_look_for(
        &settings.theme,
        caps,
        settings.diagram_background_panel,
    );
    let key = ImageKey::rendered(language, source)
        .with_look(look)
        .in_document(number);
    let path = given.path.display();
    match images.remove(&key) {
        Some(Ok(image)) => {
            let name = given.name.clone();
            images.insert(ImageKey::from(name.as_str()).in_document(number), Ok(image));
            given.shown = Shown::Document {
                document: Document::new(vec![Block::ImageFile(name)]),
                images: true,
            };
            Some(Drawn::Done)
        }
        Some(Err(error)) => {
            let (code, reason) = match &error {
                ImageError::Diagram { reason, .. } => (Code::Diagram, reason.to_string()),
                error => (Code::Image, error.to_string()),
            };
            // Back in, so that the document tells it below the source.
            images.insert(key, Err(error));
            Some(match settings.render_errors.stderr() {
                true => Drawn::Failed(Diagnostic::new(code, reason).in_file(path.to_string())),
                false => Drawn::Told,
            })
        }
        // Not drawn, though it could have been shown: no renderer for it.
        None if settings.diagrams && caps.draws_images() => {
            let hint = format!(
                "no program to draw {} diagrams; see --doctor",
                crate::diagrams::display_name(language)
            );
            Some(Drawn::Hint(
                Diagnostic::new(Code::NoDiagramProgram, hint).in_file(path.to_string()),
            ))
        }
        None => None,
    }
}

/// The image file `name` among several, as a file to render.
pub(super) fn image_file(name: String) -> crate::typeset::model::File {
    let document = Document::new(vec![Block::ImageFile(name.clone())]);
    crate::typeset::model::File { name, document }
}
