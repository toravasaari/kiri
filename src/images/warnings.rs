//! The warnings about images and diagrams that did not load.

use super::url::scheme;
use super::{ImageError, ImageKey, Source};
use crate::typeset::diagnostic::{Code, Diagnostic};

/// The warnings about `images`, as `Images::warnings_with` makes them.
pub(super) fn warnings<'a>(
    images: impl Iterator<Item = (&'a ImageKey, &'a Result<Source, ImageError>)>,
    render_errors: bool,
) -> Vec<Diagnostic> {
    images
        .filter_map(|(key, image)| warning(key, image.as_ref().err()?, render_errors))
        .collect()
}

/// What `Images::warnings_with` says of the image `key` that failed with
/// `error`; `None` when nothing is said.
fn warning(key: &ImageKey, error: &ImageError, render_errors: bool) -> Option<Diagnostic> {
    let code = match error {
        ImageError::Diagram { .. } => Code::Diagram,
        _ => Code::Image,
    };
    let text = error.to_string();
    let (subject, message) = match (key, error) {
        // A heading without its image shows as text, which is enough.
        (ImageKey::Heading { .. }, _) => return None,
        (_, ImageError::Diagram { .. }) if !render_errors => return None,
        // These name the file, resolved against the document, or the
        // diagram, in their text.
        (_, error) if error.names_file() => {
            let subject = match error {
                ImageError::Missing(path)
                | ImageError::NotAFile(path)
                | ImageError::Unreadable { path, .. }
                | ImageError::FileTooLarge { path, .. } => path.display().to_string(),
                ImageError::Diagram { name, .. } => name.clone(),
                _ => unreachable!("names_file lists these"),
            };
            let message = text.strip_prefix(&format!("{subject}: ")).unwrap_or(&text);
            (Some(subject.clone()), message.to_owned())
        }
        (ImageKey::Url { url, .. }, ImageError::NoPath) if url.is_empty() => {
            (None, "an image URL is empty".to_owned())
        }
        // A data: URL is the image itself, too long for a message.
        (ImageKey::Url { url, .. }, _)
            if scheme(url).is_some_and(|s| s.eq_ignore_ascii_case("data")) =>
        {
            (Some("data: URL".to_owned()), text)
        }
        (key, _) => (Some(key.to_string()), text),
    };
    let warning = Diagnostic::new(code, message);
    Some(match subject {
        Some(subject) => warning.about(subject),
        None => warning,
    })
}
