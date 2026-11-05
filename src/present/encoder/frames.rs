//! The frames of an animation that an encoder sends: of an animated image
//! placed as a block or in a line, and of the part of a page of reading mode
//! that holds an animated picture.

use std::rc::Rc;
use std::sync::Arc;

use image::RgbaImage;

use super::{Overlays, Pages, fit_frame, frame_of};
use crate::typeset::caps::Capabilities;
use crate::typeset::images::build::framed;
use std::collections::HashMap;

use crate::typeset::images::animation::Animation;
use crate::typeset::images::marks::{self, State};
use crate::typeset::images::size::Placement;
use crate::typeset::images::{ImageKey, Images, Source};
use crate::typeset::reading::animated::Pic;
use crate::typeset::reading::engine::Parts;
use crate::typeset::reading::page::Overlay;

/// The animation that a placement shows and how each of its frames is drawn.
pub(super) struct Frames<'a> {
    pub animation: Arc<Animation>,
    placement: &'a Placement,
    cell: (u16, u16),
    /// The colour round the selected animation.
    outline: Option<[u8; 3]>,
    /// For a part of a page: how to draw it.
    page: Option<Part<'a>>,
}

struct Part<'a> {
    marks: &'a HashMap<ImageKey, State>,
    parts: &'a Rc<Parts>,
    slice: u32,
    overlay: Option<&'a Overlay>,
    pictures: Vec<Pic>,
    /// The number of the picture that plays among those in the part.
    driver: usize,
}

impl<'a> Frames<'a> {
    /// The frames `placement` shows, when it shows an animation: an image
    /// from `images`, or a part of a page with the animated picture `driver`
    /// in it, whose frames play while the others stay on their first.
    pub fn of(
        placement: &'a Placement,
        images: &'a Images,
        (pages, overlays): (&'a Pages, &'a Overlays),
        cell: (u16, u16),
        caps: &Capabilities,
        (driver, marks): (Option<&ImageKey>, &'a HashMap<ImageKey, State>),
    ) -> Option<Self> {
        let outline = frame_of(placement, caps).map(|(rgb, _)| rgb);
        let page = match placement.key {
            ImageKey::Page {
                page,
                slice,
                overlay,
                ..
            } => {
                let parts = pages.get(&page)?;
                let pictures = parts.animated(images);
                let here = parts.in_part(slice, &pictures);
                let driver = here.iter().position(|(p, _)| Some(&p.key) == driver)?;
                let animation = Arc::clone(&here[driver].0.animation);
                let overlay = overlays.get(&(page, overlay)).map(Rc::as_ref);
                let part = Part {
                    marks,
                    parts,
                    slice,
                    overlay,
                    pictures,
                    driver,
                };
                return Some(Self {
                    animation,
                    placement,
                    cell,
                    outline,
                    page: Some(part),
                });
            }
            _ => None,
        };
        let Some(Ok(Source::Animated(animation))) = images.get(&placement.key) else {
            return None;
        };
        Some(Self {
            animation: Arc::clone(animation),
            placement,
            cell,
            outline,
            page,
        })
    }

    /// Frame `index`, drawn as the first frame of the placement is.
    pub fn pixels(&self, index: usize) -> RgbaImage {
        let Some(part) = &self.page else {
            let mut pixels = fit_frame(&self.animation, index, self.placement, self.cell);
            if let Some(frame) = frame_of_rgb(self) {
                framed(&mut pixels, self.cell.1, frame);
            }
            return pixels;
        };
        let line = ((u32::from(self.cell.1) + 10) / 20).max(1);
        let outline = self.outline.map(|rgb| (rgb, line));
        // The picture that plays shows its frame and has no mark; the others
        // in the part are as they stand.
        let playing = part
            .pictures_in_part()
            .get(part.driver)
            .map(|pic| pic.key.clone());
        let drawn = |pic: &Pic| match Some(&pic.key) == playing.as_ref() {
            true => (index, None),
            false => static_look(part.marks, pic),
        };
        let outline = outline.map(|(rgb, line)| (part.driver, rgb, line));
        (part.parts).draw_frame(part.slice, part.overlay, &part.pictures, &drawn, outline)
    }
}

/// The frame round a whole image placement, as a placement has it.
fn frame_of_rgb(frames: &Frames) -> Option<([u8; 3], std::ops::Range<u16>)> {
    let frame = frames.placement.frame.as_ref()?;
    Some((frames.outline?, frame.rows.clone()))
}

impl Part<'_> {
    /// The animated pictures in the part, from the top.
    fn pictures_in_part(&self) -> Vec<Pic> {
        (self.parts.in_part(self.slice, &self.pictures).into_iter())
            .map(|(pic, _)| pic)
            .collect()
    }
}

/// The frame of `pic` that stands in a part and the mark over it, by the
/// state it is in; a picture that is in no state shows its first frame.
pub(super) fn static_look(
    states: &HashMap<ImageKey, State>,
    pic: &Pic,
) -> (usize, Option<marks::Mark>) {
    let big = marks::is_big(pic.width, pic.height);
    match states.get(&pic.key) {
        Some(state) => marks::drawn(*state, pic.animation.frames.len(), big),
        None => (0, None),
    }
}
