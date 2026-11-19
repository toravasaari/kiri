//! The pictures of a page in reading mode, found in the images that were
//! loaded for the document.

use std::cell::RefCell;
use std::rc::Rc;

use image::RgbaImage;

use crate::render::loading;
use crate::typeset::diagrams::{names, renderer, request::INLINE_FORMULA};
use crate::typeset::images::size::{self as sizes, Wanted};
use crate::typeset::images::{ImageError, ImageKey, Images, Placeholder, Source};
use crate::typeset::model;
use crate::typeset::reading::engine::{MAX_ROWS, PictureCache, hashed};
use crate::typeset::reading::page::{Picture, Pictures};

/// The pictures of a document, found in the images that were loaded for it.
pub(super) struct Gallery<'a> {
    pub(super) images: &'a Images,
    /// The number of the document in the session, which the keys of its
    /// images carry.
    pub(super) document: usize,
    /// The fingerprint of how diagrams are drawn, which their keys carry.
    pub(super) diagrams: u64,
    pub(super) cell: (u16, u16),
    /// Whether a diagram that could not be drawn tells why below its code.
    pub(super) inline_errors: bool,
    /// The keys of the pictures that are loading, which their tags number.
    pub(super) keys: RefCell<Vec<ImageKey>>,
    /// The pictures of the last layout, which this one takes from.
    pub(super) cache: &'a RefCell<PictureCache>,
}

impl Gallery<'_> {
    /// The picture the images hold under `key`, at the size `wanted` asks
    /// for in `room`; `language` when it is a diagram.
    fn picture(
        &self,
        key: ImageKey,
        wanted: Wanted,
        room: (f32, f32),
        language: Option<&str>,
    ) -> Picture {
        let room = (
            f64::from(room.0),
            f64::from(room.1).min(f64::from(MAX_ROWS) * f64::from(self.cell.1)),
        );
        let tag = || {
            let mut keys = self.keys.borrow_mut();
            keys.push(key.clone());
            keys.len() - 1
        };
        let drawn = |source: &Source, dimmed: bool| {
            let size = sizes::pixel_size(source.size(), wanted, self.cell, room)?;
            let mut pixels = source.render(size.0, size.1);
            if dimmed {
                pixels = loading::dimmed(pixels);
            }
            Some(pixels)
        };
        match (self.images.get(&key), self.images.placeholder(&key)) {
            (Some(Ok(source)), _) => {
                let Some(size) = sizes::pixel_size(source.size(), wanted, self.cell, room) else {
                    return Picture::None;
                };
                let print = self.images.print(&key).unwrap_or(0);
                let (pixels, id) = (self.cache.borrow_mut())
                    .get((key.clone(), print, size), || {
                        super::animated::render(source, &key, size)
                    });
                Picture::Ready {
                    pixels,
                    id,
                    loading: None,
                    depth: 0.0,
                }
            }
            (Some(Err(ImageError::Diagram { reason, .. })), _) if language.is_some() => {
                let noun = language.map(names::noun).unwrap_or_default();
                let message = format!("✗ {noun} could not be drawn: {reason}");
                Picture::Failed(self.inline_errors.then_some(message))
            }
            (Some(Err(_)), _) => Picture::None,
            (_, Some(Placeholder::Dimmed(old))) => match drawn(old, true) {
                Some(pixels) => ready(pixels, Some(tag())),
                None => Picture::None,
            },
            (_, Some(&Placeholder::Rows(rows))) => Picture::Loading {
                width: u32::from(self.cell.0),
                height: u32::from(rows) * u32::from(self.cell.1),
                tag: tag(),
            },
            _ => Picture::None,
        }
    }
}

/// `pixels` as a picture, with an id from what they are.
fn ready(pixels: RgbaImage, loading: Option<usize>) -> Picture {
    let (pixels, id) = hashed(pixels);
    Picture::Ready {
        pixels: Rc::new(pixels),
        id,
        loading,
        depth: 0.0,
    }
}

impl Pictures for Gallery<'_> {
    fn formula(&self, source: &str, room: (f32, f32)) -> Picture {
        let key = ImageKey::diagram(INLINE_FORMULA, source, self.diagrams, self.document);
        let picture = self.picture(key, Wanted::default(), room, Some(INLINE_FORMULA));
        match picture {
            Picture::Ready {
                pixels,
                id,
                loading,
                ..
            } => {
                // The renderer tells how much of the image is below the
                // formula's baseline.
                let below = renderer::baseline_depth(INLINE_FORMULA, source);
                let depth = below.map_or(0.0, |below| below as f32 * pixels.height() as f32);
                Picture::Ready {
                    pixels,
                    id,
                    loading,
                    depth,
                }
            }
            other => other,
        }
    }

    fn image(&self, shown: &model::Image, room: (f32, f32)) -> Picture {
        let key = ImageKey::from(shown.url.as_str()).in_document(self.document);
        self.images
            .note_alt(&key, &model::Inline::plain_text(&shown.alt));
        let wanted = Wanted {
            width: shown.width,
            height: shown.height,
        };
        self.picture(key, wanted, room, None)
    }

    fn diagram(&self, language: &str, code: &str, room: (f32, f32)) -> Picture {
        // Spares building a key for every code block of a document
        // without images.
        if self.images.is_empty() && !self.images.loading() {
            return Picture::None;
        }
        let key = ImageKey::diagram(language, code, self.diagrams, self.document);
        self.picture(key, Wanted::default(), room, Some(language))
    }
}
