//! The store of loaded images that layout reads: every image of a document by
//! its key, loaded or not, with what stands in for the ones still loading.
//!
//! The store is `Images`, and `Placeholder` is what stands in for an image that
//! is loading. It also keeps the notes that the layout leaves, such as an
//! animation kept on its first frame (`note_still`, `notes`): each layout
//! replaces them, the pager shows them with its warnings, `late_warnings` tells
//! them when the pager quits, and the print frontend tells them after a file.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::typeset::diagnostic::{Code, Diagnostic};
use crate::typeset::images::decode::{Source, opaque_rows};
use crate::typeset::images::error::ImageError;
use crate::typeset::images::key::ImageKey;
use crate::typeset::images::size::Unit;
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
    /// The natural size of image files that are not decoded, which the
    /// layout sizes them by, until the image itself is inserted.
    extents: HashMap<ImageKey, ((u32, u32), Unit)>,
    /// The keys of the images the documents show, in their order, as a
    /// load in the background last found them.
    pub(crate) shown: Vec<ImageKey>,
    /// What each loaded image is, by a hash of it.
    prints: std::cell::RefCell<HashMap<ImageKey, u64>>,
    /// The opaque rows of each loaded image, as fractions of its height,
    /// found when asked for.
    opaques: std::cell::RefCell<HashMap<ImageKey, Range<f64>>>,
    /// What the last layout said of an image that it left on its first
    /// frame, once for each image and size shown at, as warnings; with
    /// whether the layout under way has said it again.
    notes: std::cell::RefCell<Vec<(Note, bool)>>,
    /// The alt text each image was shown with, first come first kept, as
    /// the layouts say it.
    alts: std::cell::RefCell<HashMap<ImageKey, String>>,
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

    /// Notes that the image `key` is shown with the alt text `alt`, which
    /// names it where it is told of, such as in the status line. The first
    /// text kept for an image stays; an empty one is not kept.
    pub fn note_alt(&self, key: &ImageKey, alt: &str) {
        let alt = alt.trim();
        if alt.is_empty() {
            return;
        }
        (self.alts.borrow_mut())
            .entry(key.clone())
            .or_insert_with(|| alt.to_owned());
    }

    /// The alt text the image `key` was shown with, if it had one.
    pub fn alt(&self, key: &ImageKey) -> Option<String> {
        self.alts.borrow().get(key).cloned()
    }

    /// Records the outcome of loading `key`, replacing an earlier one,
    /// and what stood in for it while it loaded.
    pub fn insert(&mut self, key: impl Into<ImageKey>, image: Result<Source, ImageError>) {
        let key = key.into();
        self.extents.remove(&key);
        self.loading.remove(&key);
        self.late.remove(&key);
        if !self.by_key.contains_key(&key) {
            self.order.push(key.clone());
        }
        // A hash of the new image is made when it is asked for.
        self.notes.get_mut().retain(|((at, _), _)| at.0 != key);
        self.prints.get_mut().remove(&key);
        self.opaques.get_mut().remove(&key);
        self.by_key.insert(key, image);
    }

    /// Notes that the image file `key`, not decoded, has the natural size
    /// `natural` in `unit`, so that the layout can size it without its
    /// pixels. [`Self::insert`] of the image takes the note back.
    pub fn set_extent(&mut self, key: impl Into<ImageKey>, natural: (u32, u32), unit: Unit) {
        self.extents.insert(key.into(), (natural, unit));
    }

    /// Takes the note of `key`'s natural size back, as an image file that
    /// could not be decoded has none to show.
    pub fn clear_extent(&mut self, key: &ImageKey) {
        self.extents.remove(key);
    }

    /// Whether `image` is of the natural size and unit that
    /// [`Self::set_extent`] noted for `key`, as a layout by the note holds
    /// for it; true when nothing is noted.
    pub fn is_of_extent(&self, key: &ImageKey, image: &Source) -> bool {
        (self.extent(key)).is_none_or(|noted| noted == (image.size(), image.unit()))
    }

    /// The natural size and its unit that [`Self::set_extent`] noted for
    /// `key`.
    pub fn extent(&self, key: &ImageKey) -> Option<((u32, u32), Unit)> {
        self.extents.get(key).copied()
    }

    /// Notes that the layout under way left the animation `key`, shown
    /// `size` pixels large, on its first frame, for the reason `text`; once
    /// for each image and size, whatever number of pages say it.
    pub fn note_still(&self, key: &ImageKey, size: (u32, u32), text: String) {
        let mut notes = self.notes.borrow_mut();
        match notes
            .iter_mut()
            .find(|(((k, s), _), _)| k == key && *s == size)
        {
            // The first page of a layout to say it gives the text: a note of an
            // earlier layout has the reason of that layout.
            Some((note, said)) => {
                if !*said {
                    note.1 = text;
                }
                *said = true;
            }
            None => notes.push((((key.clone(), size), text), true)),
        }
    }

    /// Ends a layout of the documents numbered `documents`: the notes of
    /// those are the ones it said, and those of the layouts before that it
    /// did not say again are taken back. The notes of other documents,
    /// which belong to the layouts of other views, stay.
    pub fn end_notes(&self, documents: Range<usize>) {
        let mut notes = self.notes.borrow_mut();
        notes.retain(|(((key, _), _), said)| *said || !documents.contains(&key.document()));
        notes.iter_mut().for_each(|(_, said)| *said = false);
    }

    /// The notes of the last layout, as warnings, with the number of the
    /// document of each image.
    pub fn notes(&self) -> Vec<(usize, Diagnostic)> {
        self.notes_in(0..usize::MAX)
    }

    /// The notes of the documents numbered `documents`, as [`Images::notes`].
    pub fn notes_in(&self, documents: Range<usize>) -> Vec<(usize, Diagnostic)> {
        (self.notes.borrow().iter())
            .filter(|(((key, _), _), _)| documents.contains(&key.document()))
            .map(|(((key, _), text), _)| {
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

    /// Takes out what is held of the documents whose numbers are not
    /// `numbers`: their outcomes in their order, the late marks, the
    /// extents, the alt texts and the shown keys. [`Images::restore`]
    /// puts them back.
    pub fn take_besides(&mut self, numbers: &[usize]) -> Images {
        let other = |key: &ImageKey| !numbers.contains(&key.document());
        let mut taken = Images::new();
        for key in self
            .order
            .iter()
            .filter(|key| other(key))
            .cloned()
            .collect::<Vec<_>>()
        {
            let late = self.late.contains(&key);
            if let Some(image) = self.remove(&key) {
                taken.insert(key.clone(), image);
            }
            if late {
                taken.late.insert(key);
            }
        }
        let extents = std::mem::take(&mut self.extents);
        (taken.extents, self.extents) = extents.into_iter().partition(|(key, _)| other(key));
        let alts = std::mem::take(self.alts.get_mut());
        let (theirs, mine) = alts.into_iter().partition(|(key, _)| other(key));
        (*taken.alts.get_mut(), *self.alts.get_mut()) = (theirs, mine);
        let shown = std::mem::take(&mut self.shown);
        (taken.shown, self.shown) = shown.into_iter().partition(other);
        taken
    }

    /// Forgets everything held of the document numbered `document`, as its
    /// view is closed: its outcomes, what stands in for its loads, its late
    /// marks, extents, alt texts and shown keys, and what was set aside of it.
    pub fn forget(&mut self, document: usize) {
        let there = |key: &ImageKey| key.document() == document;
        let keys: Vec<ImageKey> = self
            .order
            .iter()
            .filter(|key| there(key))
            .cloned()
            .collect();
        for key in keys {
            self.remove(&key);
        }
        self.replaced.retain(|key, _| !there(key));
        self.loading.retain(|key, _| !there(key));
        self.late.retain(|key| !there(key));
        self.extents.retain(|key, _| !there(key));
        self.shown.retain(|key| !there(key));
        self.alts.get_mut().retain(|key, _| !there(key));
        self.notes.get_mut().retain(|(note, _)| !there(&note.0.0));
    }

    /// Puts back what [`Images::take_besides`] took, after what is held.
    pub fn restore(&mut self, mut taken: Images) {
        self.alts.get_mut().extend(taken.alts.get_mut().drain());
        self.shown.append(&mut taken.shown);
        let extents = std::mem::take(&mut taken.extents);
        let late = std::mem::take(&mut taken.late);
        self.extend(taken);
        // Putting an image back drops its extent, so the extents come
        // after it: they are kept whether the key has an outcome or not.
        self.extents.extend(extents);
        self.late.extend(late);
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
    /// the number of its document, and then the notes of the last layout,
    /// which the pager tells when it quits.
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
    fn forgetting_a_document_drops_all_that_is_held_of_it_and_only_that() {
        let mut images = Images::new();
        let key = |document: usize| ImageKey::from("a.png").in_document(document);
        let loading = ImageKey::from("b.png").in_document(3);
        for document in [0, 3] {
            images.insert(key(document), Ok(Source::Raster(RgbaImage::new(1, 1))));
            images.came(key(document), Ok(Source::Raster(RgbaImage::new(1, 1))));
            images.note_alt(&key(document), "alt");
            images.shown.push(key(document));
        }
        images.set_extent(ImageKey::from("c.png").in_document(3), (3, 4), Unit::Css);
        images.wait(loading.clone(), Placeholder::Unsized);
        images
            .replaced
            .insert(key(3), Source::Raster(RgbaImage::new(1, 1)));
        images.forget(3);
        assert!(images.get(&key(3)).is_none());
        assert!(!images.late.contains(&key(3)));
        assert!(images.alt(&key(3)).is_none());
        assert!(!images.shown.contains(&key(3)));
        assert!(
            images
                .extent(&ImageKey::from("c.png").in_document(3))
                .is_none()
        );
        assert!(images.placeholder(&loading).is_none());
        assert!(!images.replaced.contains_key(&key(3)));
        // The base's own stay.
        assert!(images.get(&key(0)).is_some());
        assert!(images.late.contains(&key(0)));
        assert_eq!(images.alt(&key(0)).as_deref(), Some("alt"));
        assert_eq!(images.shown, [key(0)]);
        assert_eq!(images.iter().count(), 1);
    }

    #[test]
    fn an_image_is_of_the_extent_noted_for_it_when_its_size_and_unit_are_the_same() {
        let mut images = Images::new();
        let key = ImageKey::from("a.png");
        let image = Source::Raster(RgbaImage::new(4, 2));
        // Nothing noted: nothing to differ from.
        assert!(images.is_of_extent(&key, &image));
        images.set_extent(key.clone(), (4, 2), Unit::Pixel);
        assert!(images.is_of_extent(&key, &image));
        images.set_extent(key.clone(), (2, 4), Unit::Pixel);
        assert!(!images.is_of_extent(&key, &image));
        images.set_extent(key.clone(), (4, 2), Unit::Css);
        assert!(!images.is_of_extent(&key, &image));
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
    fn an_animation_over_the_budget_is_warned_of() {
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
        images.insert("big.gif", animation(true));
        images.insert("fine.gif", animation(false));
        assert_eq!(
            texts(images.warnings()),
            ["big.gif: shows only its first frame: its frames take more than the limit of 64 MiB"]
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
            &crate::typeset::fonts::Fonts::none(),
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
