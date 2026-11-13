//! Opening a file: the file at a path, the README of a directory, or
//! standard input, read far enough to tell what it is, and the choice of
//! parser for a text file's input format.
//!
//! What does not belong here is what the file is shown as, which `given.rs`
//! decides, nor the guess of its format from its bytes
//! (`src/typeset/input.rs`).

use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::images::Format;
use crate::session::plan;
use crate::session::state::Look;
use crate::typeset::caps::Capabilities;
use crate::typeset::diagnostic::Diagnostic;
use crate::typeset::input::Input;
use crate::typeset::model::Document;
use crate::typeset::position::Pos;

use super::Platform;

/// What errors and warnings call standard input. Without an extension, so
/// that the content tells what standard input is, as for such a file.
pub(super) const STDIN: &str = "<stdin>";

/// A file opened and read far enough to tell what it is.
pub(super) struct Opened {
    /// What errors and warnings call it: its path, the README's for a
    /// directory, or `<stdin>`.
    pub(super) path: PathBuf,
    /// The file, or `None` for standard input.
    pub(super) file: Option<PathBuf>,
    /// How the file looked before it was read, when it could be looked at.
    pub(super) look: Option<Look>,
    pub(super) input: Input,
    pub(super) content: Content,
}

impl Opened {
    /// Whether it is a document read from a regular file, which can be read
    /// again: not an image or a diagram, nor a pipe given by its path.
    pub(super) fn reloadable(&self, platform: &dyn Platform) -> bool {
        !plan::shown_alone(self.input)
            && (self.file.as_deref()).is_some_and(|file| platform.is_file(file))
    }
}

/// What a file opened holds.
pub(super) enum Content {
    /// An image in `format`, which starts with `head` and goes on in
    /// `rest`; `None` when the file is closed, to be opened again.
    Image {
        format: Format,
        head: Vec<u8>,
        rest: Option<Box<dyn Read>>,
    },
    /// Text, read whole.
    Text(String),
}

/// Opens the file at `path`, the README of a directory there, or standard
/// input without one, and tells what it is: an image is read only as far
/// as that takes, and any other file whole, as text.
pub(super) fn open(
    platform: &dyn Platform,
    format: Option<Input>,
    path: Option<&Path>,
) -> Result<Opened, String> {
    // A directory's README is the name of what is shown, but a README
    // without an extension is Markdown, as it usually is.
    let from_dir = path.is_some_and(|dir| platform.is_dir(dir));
    let file = match path {
        Some(dir) if from_dir => Some(readme(platform, dir)?),
        path => path.map(Path::to_owned),
    };
    let path = file.clone().unwrap_or_else(|| PathBuf::from(STDIN));
    // Before the file is read, so that a change meanwhile counts.
    let look = file.as_deref().and_then(|file| platform.look(file).ok());
    let failed = |error: io::Error| format!("{}: {error}", path.display());
    let mut reader: Box<dyn Read> = match &file {
        Some(file) => platform.open(file).map_err(failed)?,
        None => platform.stdin(),
    };
    let mut head = Vec::new();
    (&mut reader)
        .take(crate::typeset::input::HEAD as u64)
        .read_to_end(&mut head)
        .map_err(failed)?;
    // Standard input, or a pipe given by its path, such as `<(git diff)`:
    // a stream, whose name tells nothing of what it holds.
    let stream = file.as_deref().is_none_or(|file| !platform.is_file(file));
    let nameless = stream || from_dir;
    let input = format.unwrap_or_else(|| match crate::typeset::input::detect(&path, &head) {
        Input::Text if nameless => Input::Markdown,
        input => input,
    });
    if let Input::Image(format) = input {
        let content = Content::Image {
            format,
            head,
            rest: Some(reader),
        };
        return Ok(Opened {
            path,
            file,
            look,
            input,
            content,
        });
    }
    let mut bytes = head;
    reader.read_to_end(&mut bytes).map_err(failed)?;
    // Text named by `--format` or by its extension is not an image either.
    let named = format.is_some() || !matches!(input, Input::Markdown);
    let source = String::from_utf8(bytes).map_err(|_| {
        if named {
            format!("{}: not UTF-8 text", path.display())
        } else {
            format!(
                "{}: neither Markdown nor an image kiri knows",
                path.display()
            )
        }
    })?;
    // A stream has no name to tell what it is, so its content does: the
    // formats Markdown cannot be mistaken for. Any other text is Markdown,
    // as ever.
    let sniffed = format.is_none()
        && stream
        && input == Input::Markdown
        && crate::typeset::input::sniff(&source).is_some();
    let input = if sniffed { Input::Text } else { input };
    Ok(Opened {
        path,
        file,
        look,
        input,
        content: Content::Text(source),
    })
}

/// The image that starts with `head` and goes on in `rest`, or, once
/// `rest` is closed, in `file` opened again from its start.
pub(super) fn image_reader(
    platform: &dyn Platform,
    file: Option<&Path>,
    head: Vec<u8>,
    rest: Option<Box<dyn Read>>,
) -> io::Result<Box<dyn Read>> {
    Ok(match (rest, file) {
        (Some(rest), _) => Box::new(Cursor::new(head).chain(rest)),
        (None, Some(file)) => platform.open(file)?,
        (None, None) => unreachable!("standard input is never closed"),
    })
}

/// The document that `source`, from the file at `path`, is as `input`,
/// anything but a diagram or an image, for `settings` and `caps`; `None`
/// for text that plain output gets as it is, as from cat. A warning
/// about its front matter, and one about each kind of Markdown element
/// that is left out, goes to `warn`.
pub(super) fn document(
    input: Input,
    source: &str,
    path: &Path,
    settings: &Settings,
    caps: &Capabilities,
    mut warn: impl FnMut(Diagnostic),
) -> Result<Option<Document>, String> {
    Ok(Some(match input {
        Input::Table(separator) => crate::typeset::parse::csv::parse(source, separator)
            .map_err(|error| format!("{}: {error}", path.display()))?,
        Input::Text if !caps.styling => return Ok(None),
        Input::Text => crate::typeset::parse::file(
            source,
            crate::typeset::input::language(path, source).as_deref(),
        ),
        _ => {
            let style = settings.front_matter.style();
            let (document, left_out) =
                crate::typeset::parse::parse_with_warnings(source, style.is_some());
            let error = style.and_then(|style| crate::render::front_matter_error(&document, style));
            if let Some(error) = error {
                // The front matter is the first block of the document, which
                // is file 0 until the pager tells it among several.
                warn(Diagnostic::from(error).at(Pos::block(0, &[0])..Pos::end(0, &[0])));
            }
            left_out.into_iter().for_each(warn);
            document
        }
    }))
}

/// The README the directory `dir` shows: `README.md`, or else `README`.
pub(super) fn readme(platform: &dyn Platform, dir: &Path) -> Result<PathBuf, String> {
    ["README.md", "README"]
        .iter()
        .map(|name| dir.join(name))
        .find(|path| platform.is_file(path))
        .ok_or_else(|| format!("{}: no README.md or README", dir.display()))
}
