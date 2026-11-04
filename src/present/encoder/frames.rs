//! The frames of an animation that an encoder sends: of an animated image
//! placed as a block or in a line, and of the part of a page of reading mode
//! that holds an animated picture.

use std::rc::Rc;
use std::sync::Arc;

use image::RgbaImage;

use super::{Overlays, Pages, fit_frame, frame_of, framed};
use crate::typeset::caps::Capabilities;
use crate::typeset::images::animation::{Animation, FRAME_BUDGET};
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
    parts: &'a Rc<Parts>,
    slice: u32,
    overlay: Option<&'a Overlay>,
    pictures: Vec<Pic>,
}

impl<'a> Frames<'a> {
    /// The frames `placement` shows, when it shows an animation: an image
    /// from `images`, or a part of a page with an animated picture.
    pub fn of(
        placement: &'a Placement,
        images: &'a Images,
        (pages, overlays): (&'a Pages, &'a Overlays),
        cell: (u16, u16),
        caps: &Capabilities,
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
                let (pic, _) = parts.driver(slice, &pictures, FRAME_BUDGET)?;
                let animation = Arc::clone(&pic.animation);
                let overlay = overlays.get(&(page, overlay)).map(Rc::as_ref);
                let part = Part {
                    parts,
                    slice,
                    overlay,
                    pictures,
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
                framed(&mut pixels, self.cell, frame);
            }
            return pixels;
        };
        let line = ((u32::from(self.cell.1) + 10) / 20).max(1);
        let outline = self.outline.map(|rgb| (rgb, line));
        (part.parts).draw_frame(
            part.slice,
            part.overlay,
            &part.pictures,
            (0, index),
            outline,
        )
    }
}

/// The frame round a whole image placement, as a placement has it.
fn frame_of_rgb(frames: &Frames) -> Option<([u8; 3], std::ops::Range<u16>)> {
    let frame = frames.placement.frame.as_ref()?;
    Some((frames.outline?, frame.rows.clone()))
}
