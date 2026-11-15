//! When the files behind a document's images were last changed, so that a
//! reload loads again an image whose file has.

use std::collections::HashMap;
use std::time::SystemTime;

use super::{ImageError, ImageKey, Images, Part, Source};
use crate::typeset::images::plan::shown_urls;
use crate::typeset::images::url::local_file;

/// The modification time of the file behind each image of the documents
/// shown, when the image was last loaded: `None` for a file that could
/// not be looked at. Images in the middle of text are stamped whatever
/// the mode is, so that turning reading mode on does not make a reload
/// take them for new. An image that does not come from a local file, such
/// as a diagram or a `data:` URL, has no stamp and stays as it is.
#[derive(Debug, Default)]
pub struct Stamps {
    by_key: HashMap<ImageKey, Option<SystemTime>>,
}

impl Stamps {
    /// The stamps of the images that `parts` show, as the files are now.
    pub fn of(parts: &[Part<'_>]) -> Self {
        Self {
            by_key: modified_times(parts),
        }
    }

    /// Takes out of `images` the ones whose files have changed since their
    /// stamps, so that loading them again is a matter of loading what is
    /// missing, and stamps what `parts` show as the files are now; the stamps of
    /// other documents stay. Gives
    /// the keys of the files that changed, with what `images` held of
    /// them.
    pub fn refresh(
        &mut self,
        parts: &[Part<'_>],
        images: &mut Images,
    ) -> Vec<(ImageKey, Option<Result<Source, ImageError>>)> {
        let now = modified_times(parts);
        let mut changed = Vec::new();
        for (key, stamp) in &now {
            if self.by_key.get(key) != Some(stamp) {
                changed.push((key.clone(), images.remove(key)));
            }
        }
        // The stamps of the documents that are not these are of other views.
        self.by_key
            .retain(|key, _| !parts.iter().any(|part| part.number == key.document()));
        self.by_key.extend(now);
        changed
    }

    /// Forgets the stamps of the document numbered `document`, as its view
    /// is closed.
    pub fn forget(&mut self, document: usize) {
        self.by_key.retain(|key, _| key.document() != document);
    }
}

/// The modification time of the file behind each image of `parts` that
/// comes from a local file, those in the middle of text too.
fn modified_times(parts: &[Part<'_>]) -> HashMap<ImageKey, Option<SystemTime>> {
    let mut times = HashMap::new();
    for part in parts {
        for url in shown_urls(part.document) {
            let Some(path) = local_file(url, part.base) else {
                continue;
            };
            let modified = std::fs::metadata(path).and_then(|meta| meta.modified());
            times.insert(ImageKey::from(url).in_document(part.number), modified.ok());
        }
    }
    times
}
