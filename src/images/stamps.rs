//! When the files behind a document's images were last changed, so that a
//! reload loads again an image whose file has.

use std::collections::HashMap;
use std::time::SystemTime;

use super::url::local_file;
use super::{ImageKey, Images, Part, lone_urls};

/// The modification time of the file behind each image of the documents
/// shown, when the image was last loaded: `None` for a file that could not
/// be looked at. An image that does not come from a local file, such as a
/// diagram or a `data:` URL, has no stamp and stays as it is.
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
    /// missing, and stamps what `parts` show as the files are now.
    pub fn refresh(&mut self, parts: &[Part<'_>], images: &mut Images) {
        let now = modified_times(parts);
        for (key, stamp) in &now {
            if self.by_key.get(key) != Some(stamp) {
                images.remove(key);
            }
        }
        self.by_key = now;
    }
}

/// The modification time of the file behind each image of `parts` that
/// comes from a local file.
fn modified_times(parts: &[Part<'_>]) -> HashMap<ImageKey, Option<SystemTime>> {
    let mut times = HashMap::new();
    for part in parts {
        for url in lone_urls(part.document) {
            let Some(path) = local_file(url, part.base) else {
                continue;
            };
            let modified = std::fs::metadata(path).and_then(|meta| meta.modified());
            times.insert(ImageKey::from(url).in_document(part.number), modified.ok());
        }
    }
    times
}
