//! The store of loaded images that layout reads: every image of a document by
//! its key, loaded or not, with what stands in for the ones still loading.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::typeset::diagnostic::{Code, Diagnostic};
use crate::typeset::images::decode::{Source, opaque_rows};
use crate::typeset::images::error::ImageError;
use crate::typeset::images::key::ImageKey;
use crate::typeset::images::warnings;

/// Every image of a document by its key, loaded or not.
#[derive(Debug, Default)]
pub struct Images {
    pub(crate) by_key: HashMap<ImageKey, Result<Source, ImageError>>,
    /// The keys in the order they were first inserted, the document's order.
    order: Vec<ImageKey>,
    /// The images of the document before a reload in the background that
    /// the new one no longer shows, or that are loading anew, until they
    /// are taken: what was there while the new ones load.
    pub(crate) replaced: HashMap<ImageKey, Source>,
    /// The images loading in the background, each with what stands in
    /// for it meanwhile.
    loading: HashMap<ImageKey, Placeholder>,
    /// The images whose outcome came from loading in the background, which
    /// nothing has told about yet.
    pub(crate) late: HashSet<ImageKey>,
    /// The keys of the images the documents show, in their order, as a
    /// load in the background last found them.
    pub(crate) shown: Vec<ImageKey>,
    /// What each loaded image is, by a hash of it.
    prints: std::cell::RefCell<HashMap<ImageKey, u64>>,
    /// The opaque rows of each loaded image, as fractions of its height,
    /// found when asked for.
    opaques: std::cell::RefCell<HashMap<ImageKey, Range<f64>>>,
    /// What the layout says of an image that it left on its first frame,
    /// once for each image and size shown at, as warnings.
    notes: std::cell::RefCell<Vec<Note>>,
}

/// A note of the layout: the image and the size it is shown at, and what it
/// says.
type Note = ((ImageKey, (u32, u32)), String);

/// The opaque rows of `image` as fractions of its height.
fn fractions(image: &image::RgbaImage) -> Range<f64> {
    let height = f64::from(image.height().max(1));
    match opaque_rows(image) {
        Some(rows) => f64::from(rows.start) / height..f64::from(rows.end) / height,
        None => 0.0..1.0,
    }
}

/// What a block shows in place of its image while the image loads.
#[derive(Debug, Clone)]
pub enum Placeholder {
    /// The image the block showed before, dimmed.
    Dimmed(Source),
    /// Nothing that tells a size: the block shows as it does without its
    /// image.
    Unsized,
    /// A box of this many rows, only to measure how tall the documents are
    /// before their images come; never drawn.
    Rows(u16),
}

impl Images {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the outcome of loading `key`, replacing an earlier one,
    /// and what stood in for it while it loaded.
    pub fn insert(&mut self, key: impl Into<ImageKey>, image: Result<Source, ImageError>) {
        let key = key.into();
        self.loading.remove(&key);
        self.late.remove(&key);
        if !self.by_key.contains_key(&key) {
            self.order.push(key.clone());
        }
        // A hash of the new image is made when it is asked for.
        self.notes.get_mut().retain(|(at, _)| at.0 != key);
        self.prints.get_mut().remove(&key);
        self.opaques.get_mut().remove(&key);
        self.by_key.insert(key, image);
    }

    /// Notes that the layout left the animation `key`, shown `size` pixels
    /// large, on its first frame, for the reason `text`; once for each image
    /// and size, whatever number of layouts say it.
    pub fn note_still(&self, key: &ImageKey, size: (u32, u32), text: String) {
        let mut notes = self.notes.borrow_mut();
        if !notes.iter().any(|(at, _)| at.0 == *key && at.1 == size) {
            notes.push(((key.clone(), size), text));
        }
    }

    /// The notes of the layouts, as warnings, with the number of the document
    /// of each image.
    pub fn notes(&self) -> Vec<(usize, Diagnostic)> {
        (self.notes.borrow().iter())
            .map(|((key, _), text)| {
                let note = Diagnostic::new(Code::Image, text.clone()).about(key.to_string());
                (key.document(), note)
            })
            .collect()
    }

    /// A hash of the image loaded under `key`, which another image under
    /// the same key does not have; `None` when none loaded.
    pub fn print(&self, key: &ImageKey) -> Option<u64> {
        if let Some(print) = self.prints.borrow().get(key) {
            return Some(*print);
        }
        let print = self.by_key.get(key)?.as_ref().ok()?.fingerprint();
        self.prints.borrow_mut().insert(key.clone(), print);
        Some(print)
    }

    /// Takes the outcome for `key` out, as for an image shown on its own;
    /// `None` when it was never loaded.
    pub fn remove(&mut self, key: &ImageKey) -> Option<Result<Source, ImageError>> {
        self.order.retain(|loaded| loaded != key);
        self.late.remove(key);
        self.prints.get_mut().remove(key);
        self.opaques.get_mut().remove(key);
        self.by_key.remove(key)
    }

    /// The rows of the image under `key` that are not transparent (alpha
    /// above 8), as fractions of its height: `0.0..1.0` for an image that
    /// is wholly transparent. `None` when the key holds no image. An SVG
    /// is drawn at its natural size to find them, a raster image is
    /// scanned as it is; the answer is kept.
    pub fn opaque(&self, key: &ImageKey) -> Option<Range<f64>> {
        if let Some(rows) = self.opaques.borrow().get(key) {
            return Some(rows.clone());
        }
        let rows = match self.by_key.get(key)?.as_ref().ok()? {
            Source::Raster(image) | Source::Dense { image, .. } | Source::Text { image, .. } => {
                fractions(image)
            }
            Source::Animated(animation) => fractions(animation.first()),
            Source::Svg(svg) => {
                let (width, height) = svg.size;
                fractions(&svg.render(width.max(1), height.max(1)))
            }
        };
        self.opaques.borrow_mut().insert(key.clone(), rows.clone());
        Some(rows)
    }

    /// Takes in every outcome of `other`, after those it holds.
    pub fn extend(&mut self, mut other: Images) {
        for key in std::mem::take(&mut other.order) {
            if let Some(image) = other.by_key.remove(&key) {
                self.insert(key, image);
            }
        }
    }

    /// Records the outcome of loading `key` in the background, as
    /// [`Images::insert`] does; [`Images::late_warnings`] tells it if it
    /// failed.
    pub fn came(&mut self, key: ImageKey, image: Result<Source, ImageError>) {
        self.insert(key.clone(), image);
        self.late.insert(key);
    }

    /// The outcome for `key`, or `None` when it was never loaded.
    pub fn get(&self, key: &ImageKey) -> Option<&Result<Source, ImageError>> {
        self.by_key.get(key)
    }

    /// How many distinct keys were loaded.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Every key and its outcome, in the order they were first inserted.
    pub fn iter(&self) -> impl Iterator<Item = (&ImageKey, &Result<Source, ImageError>)> {
        (self.order.iter()).map(|key| (key, &self.by_key[key]))
    }

    /// Takes `key` as loading, with `placeholder` in its place until it
    /// comes.
    pub fn wait(&mut self, key: ImageKey, placeholder: Placeholder) {
        self.loading.insert(key, placeholder);
    }

    /// What stands in for `key` while it loads; `None` when it is not
    /// loading.
    pub fn placeholder(&self, key: &ImageKey) -> Option<&Placeholder> {
        self.loading.get(key)
    }

    /// Whether any image is loading.
    pub fn loading(&self) -> bool {
        !self.loading.is_empty()
    }

    /// Takes the placeholders of the images loading, which are then no
    /// longer loading as far as `self` tells.
    pub fn take_placeholders(&mut self) -> HashMap<ImageKey, Placeholder> {
        std::mem::take(&mut self.loading)
    }

    /// Sets the image `key` aside, as `reload_in_background` does with
    /// one that is no longer shown or that loads anew.
    #[cfg(test)]
    pub(crate) fn set_aside(&mut self, key: &ImageKey) {
        if let Some(Ok(image)) = self.remove(key) {
            self.replaced.insert(key.clone(), image);
        }
    }

    /// Takes the images `reload_in_background` replaced.
    pub fn take_replaced(&mut self) -> HashMap<ImageKey, Source> {
        std::mem::take(&mut self.replaced)
    }

    /// One message for each image that did not load, in document order.
    pub fn warnings(&self) -> Vec<Diagnostic> {
        self.warnings_with(true)
    }

    /// `warnings`, with one for each diagram that could not be drawn only
    /// when `render_errors`.
    pub fn warnings_with(&self, render_errors: bool) -> Vec<Diagnostic> {
        warnings::warnings(self.iter(), render_errors)
    }

    /// `warnings_with` for the images that came from loading in the
    /// background and are still shown, in the documents' order, each with
    /// the number of its document.
    pub fn late_warnings(&self, render_errors: bool) -> Vec<(usize, Diagnostic)> {
        let mut told = HashSet::new();
        let late: Vec<(usize, Diagnostic)> = (self.shown.iter())
            .filter(|key| self.late.contains(*key) && told.insert(*key))
            .filter_map(|key| Some((key, self.by_key.get(key)?)))
            .flat_map(|(key, image)| {
                let warnings = warnings::warnings(std::iter::once((key, image)), render_errors);
                warnings
                    .into_iter()
                    .map(|warning| (key.document(), warning))
            })
            .collect();
        // What the layouts left still, told when the pager quits as well.
        late.into_iter().chain(self.notes()).collect()
    }

    /// `warnings_with` for the images of the document numbered
    /// `document` alone.
    pub fn warnings_in(&self, document: usize, render_errors: bool) -> Vec<Diagnostic> {
        let images = self.iter().filter(|(key, _)| key.document() == document);
        warnings::warnings(images, render_errors)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use image::RgbaImage;

    use super::*;
    use crate::typeset::diagnostic::Code;

    /// The text of each of `diagnostics`, as standard error tells them.
    fn texts(diagnostics: Vec<Diagnostic>) -> Vec<String> {
        diagnostics.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_note_is_kept_once_for_each_image_and_size_and_goes_with_a_new_image() {
        let mut images = Images::new();
        let key = ImageKey::from("a.gif");
        for _ in 0..3 {
            images.note_still(&key, (20, 200), "only the first frame shows".into());
        }
        images.note_still(&key, (40, 400), "only the first frame shows".into());
        assert_eq!(images.notes().len(), 2);
        let told: Vec<String> = images.notes().iter().map(|(_, n)| n.to_string()).collect();
        assert_eq!(told[0], "a.gif: only the first frame shows");
        // The quitting pager tells them with the late warnings.
        assert_eq!(images.late_warnings(true).len(), 2);
        images.insert("a.gif", Ok(Source::Raster(RgbaImage::new(1, 1))));
        assert!(images.notes().is_empty());
    }

    #[test]
    fn an_animation_over_the_budget_is_warned_of_with_its_size() {
        use crate::typeset::images::animation::{Animation, Frame, Plays};
        let frame = Frame {
            image: RgbaImage::new(2, 2),
            gap: std::time::Duration::ZERO,
        };
        let animation = |over_budget| {
            Ok(Source::Animated(std::sync::Arc::new(Animation {
                frames: vec![frame.clone()],
                plays: Plays::Forever,
                over_budget,
            })))
        };
        let mut images = Images::new();
        images.insert("big.gif", animation(Some(100 * 1024 * 1024)));
        images.insert("fine.gif", animation(None));
        assert_eq!(
            texts(images.warnings()),
            ["big.gif: its frames take 100 MiB, over the limit of 64 MiB: only the first shows"]
        );
    }

    #[test]
    fn late_warnings_tell_what_came_failed_once_in_the_documents_order() {
        let missing = |url: &str| Err(ImageError::Missing(PathBuf::from(url)));
        let mut images = Images::new();
        let (a, b, c) = (
            ImageKey::from("a.png"),
            ImageKey::from("b.png"),
            ImageKey::from("c.png"),
        );
        images.shown = vec![a.clone(), b.clone(), c.clone(), b.clone()];
        images.came(b.clone(), missing("b.png"));
        images.came(a.clone(), missing("a.png"));
        // Told before the pager opened, as when it loaded first.
        images.insert(c.clone(), missing("c.png"));
        let told: Vec<_> = images
            .late_warnings(true)
            .into_iter()
            .map(|(_, w)| w.to_string())
            .collect();
        assert_eq!(told, [missing_text("a.png"), missing_text("b.png")]);
        // Loaded at last.
        images.came(a, Ok(Source::Raster(RgbaImage::new(1, 1))));
        assert_eq!(images.late_warnings(true).len(), 1);
    }

    /// The warning about the missing file `url`.
    fn missing_text(url: &str) -> String {
        ImageError::Missing(PathBuf::from(url)).to_string()
    }

    #[test]
    fn the_opaque_rows_of_an_image_are_found_once() {
        let mut images = Images::new();
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="20">
            <rect x="0" y="5" width="10" height="10" fill="red"/></svg>"#;
        let source = crate::typeset::images::decode::decode(
            svg,
            crate::typeset::images::decode::DEFAULT_MAX_FILE_SIZE,
            &crate::images::svg::Fonts::none(),
        )
        .expect("an SVG");
        images.insert("a.svg", Ok(source));
        let mut pixels = RgbaImage::new(4, 8);
        pixels.put_pixel(0, 2, image::Rgba([0, 0, 0, 255]));
        images.insert("b.png", Ok(Source::Raster(pixels)));
        images.insert("c.png", Ok(Source::Raster(RgbaImage::new(4, 8))));
        let (a, b, c) = (
            ImageKey::from("a.svg"),
            ImageKey::from("b.png"),
            ImageKey::from("c.png"),
        );
        assert_eq!(images.opaque(&a), Some(0.25..0.75));
        assert_eq!(images.opaque(&b), Some(0.25..0.375));
        assert_eq!(images.opaque(&c), Some(0.0..1.0));
        assert_eq!(images.opaque(&ImageKey::from("none.png")), None);
        // A second call reads the answer kept, not the image.
        images.by_key.remove(&a);
        assert_eq!(images.opaque(&a), Some(0.25..0.75));
        // A new image under the key has its own.
        images.insert("b.png", Ok(Source::Raster(RgbaImage::new(4, 8))));
        assert_eq!(images.opaque(&b), Some(0.0..1.0));
    }

    #[test]
    fn an_image_is_hashed_when_asked_and_once_and_not_when_it_is_inserted() {
        let mut images = Images::new();
        let pixels = |red| image::RgbaImage::from_pixel(8, 8, image::Rgba([red, 0, 0, 255]));
        images.insert("a.png", Ok(Source::Raster(pixels(1))));
        // Inserting costs no hash: it is made when the cache asks.
        assert!(images.prints.borrow().is_empty());
        let key = ImageKey::from("a.png");
        let first = images.print(&key).expect("a loaded image has one");
        assert_eq!(images.prints.borrow().len(), 1);
        assert_eq!(images.print(&key), Some(first));
        // The same pixels loaded again are the same, other pixels not, and
        // a failure has none.
        images.insert("a.png", Ok(Source::Raster(pixels(1))));
        assert_eq!(images.print(&key), Some(first));
        images.insert("a.png", Ok(Source::Raster(pixels(2))));
        assert_ne!(images.print(&key), Some(first));
        images.insert("a.png", Err(ImageError::Missing("a.png".into())));
        assert_eq!(images.print(&key), None);
    }

    #[test]
    fn keeps_the_document_order() {
        let mut images = Images::new();
        for url in ["b.png", "a.png", "c.png"] {
            images.insert(url, Err(ImageError::Remote));
        }
        images.insert("a.png", Err(ImageError::InvalidData));
        let urls: Vec<String> = images.iter().map(|(key, _)| key.to_string()).collect();
        assert_eq!(urls, ["b.png", "a.png", "c.png"]);
        assert_eq!(
            images.get(&"a.png".into()),
            Some(&Err(ImageError::InvalidData))
        );
    }

    #[test]
    fn keeps_a_block_apart_from_a_url_with_the_same_text() {
        let block = ImageKey::rendered("mermaid", "a.png");
        let mut images = Images::new();
        images.insert("a.png", Err(ImageError::Remote));
        images.insert(block.clone(), Err(ImageError::UnknownFormat));
        assert_eq!(images.len(), 2);
        assert_eq!(images.get(&block), Some(&Err(ImageError::UnknownFormat)));
        assert_eq!(
            texts(images.warnings()),
            [
                "a.png: remote images are not loaded",
                "mermaid block: not an image format kiri knows",
            ]
        );
    }

    #[test]
    fn a_warning_has_the_kind_and_the_subject_it_is_about() {
        let mut images = Images::new();
        images.insert("gone.png", Err(ImageError::Missing("/doc/gone.png".into())));
        images.insert(
            ImageKey::rendered("mermaid", "x"),
            Err(ImageError::Diagram {
                name: "mermaid diagram on line 3".into(),
                reason: "parse error".into(),
            }),
        );
        images.insert("https://example.com/a.png", Err(ImageError::Remote));
        images.insert("", Err(ImageError::NoPath));
        let told = images.warnings();
        let seen: Vec<_> = told
            .iter()
            .map(|d| (d.code, d.subject.as_deref(), d.message.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                (Code::Image, Some("/doc/gone.png"), "file not found"),
                (
                    Code::Diagram,
                    Some("mermaid diagram on line 3"),
                    "parse error"
                ),
                (
                    Code::Image,
                    Some("https://example.com/a.png"),
                    "remote images are not loaded"
                ),
                (Code::Image, None, "an image URL is empty"),
            ]
        );
        assert!(told.iter().all(|d| d.file.is_none()));
    }

    #[test]
    fn warns_about_each_image_that_did_not_load_in_document_order() {
        let mut images = Images::new();
        images.insert("https://example.com/a.png", Err(ImageError::Remote));
        images.insert("ok.png", Ok(Source::Raster(RgbaImage::new(1, 1))));
        images.insert("gone.png", Err(ImageError::Missing("/doc/gone.png".into())));
        images.insert("data:image/png;base64,AAAA", Err(ImageError::UnknownFormat));
        images.insert("file://?x", Err(ImageError::NoPath));
        images.insert("", Err(ImageError::NoPath));
        assert_eq!(
            texts(images.warnings()),
            [
                "https://example.com/a.png: remote images are not loaded",
                "/doc/gone.png: file not found",
                "data: URL: not an image format kiri knows",
                "file://?x: the URL names no file",
                "an image URL is empty",
            ]
        );
    }
}
