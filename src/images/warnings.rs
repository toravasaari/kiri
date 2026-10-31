//! The warnings about images that did not load.

use super::url::scheme;
use super::{ImageError, ImageKey, Source};
use crate::typeset::diagnostic::{Code, Diagnostic};

/// The warnings about `images`, as `Images::warnings_with` makes them.
pub(super) fn warnings<'a>(
    images: impl Iterator<Item = (&'a ImageKey, &'a Result<Source, ImageError>)>,
    render_errors: bool,
) -> Vec<Diagnostic> {
    images
        .filter_map(|(key, image)| {
            let text = warning_text(key, image.as_ref().err()?, render_errors)?;
            let code = match image {
                Err(ImageError::Diagram { .. }) => Code::Diagram,
                _ => Code::Image,
            };
            Some(Diagnostic::new(code, text))
        })
        .collect()
}

/// What `Images::warnings_with` says of the image `key` that failed with
/// `error`; `None` when nothing is said.
fn warning_text(key: &ImageKey, error: &ImageError, render_errors: bool) -> Option<String> {
    match (key, error) {
        // A heading without its image shows as text, which is enough.
        (ImageKey::Heading { .. }, _) => None,
        (_, ImageError::Diagram { .. }) if !render_errors => None,
        // These name the file, resolved against the document.
        (_, error) if error.names_file() => Some(error.to_string()),
        (ImageKey::Url { url, .. }, ImageError::NoPath) if url.is_empty() => {
            Some("an image URL is empty".to_owned())
        }
        // A data: URL is the image itself, too long for a message.
        (ImageKey::Url { url, .. }, error)
            if scheme(url).is_some_and(|s| s.eq_ignore_ascii_case("data")) =>
        {
            Some(format!("data: URL: {error}"))
        }
        (key, error) => Some(format!("{key}: {error}")),
    }
}
