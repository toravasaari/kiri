//! An image block: a paragraph of only an image, or of only a link around
//! one, laid out as the rendition its offers resolve to.

use super::{Align, Layout, embed};
use crate::typeset::images::size;
use crate::typeset::model::Inline;
use crate::typeset::rendition::{self, Readiness, RenditionKind};
use crate::typeset::style::Line;

impl Layout<'_> {
    /// A paragraph of only an image, or of only a link around an image,
    /// shown as the image itself when the terminal can and it loaded.
    /// `None` leaves the paragraph to show its alt text.
    /// The image inside a link is not clickable: OSC 8 around placeholders
    /// is not written yet.
    pub(super) fn image_block(&self, content: &[Inline], width: usize) -> Option<Vec<Line>> {
        let lone = Inline::lone_image(content)?;
        let key = embed::lone_key(self, lone);
        let chosen = rendition::resolve(&rendition::IMAGE_OFFERS, self.caps, |kind| match kind {
            RenditionKind::Picture => rendition::picture_readiness(self.images, &key),
            RenditionKind::AltText => Readiness::Ready,
        })?;
        match chosen {
            RenditionKind::Picture => {
                let wanted = size::Wanted {
                    width: lone.width,
                    height: lone.height,
                };
                self.placed(key, wanted, width, Align::Left)
            }
            // Alt text is the paragraph's own text, laid out when this is None.
            RenditionKind::AltText => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Align, with_layout};
    use crate::render::Inputs;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::images::{ImageKey, size};
    use crate::typeset::rendition::{Readiness, picture_cases, picture_readiness};

    #[test]
    fn placed_draws_a_picture_exactly_when_its_readiness_is_not_missing() {
        // Kitty with cells of 10 x 20 pixels: the 4 x 3 pixels of the cases
        // fit one cell, so the size never refuses.
        let caps = Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        };
        assert!(caps.draws_images());
        let key = ImageKey::from("p.png");
        for (name, images, _) in picture_cases(&key) {
            let inputs = Inputs {
                images,
                ..Inputs::new(caps.clone())
            };
            let placed = with_layout(&inputs, 0, |layout, _, _| {
                layout.placed(key.clone(), size::Wanted::default(), 60, Align::Left)
            });
            let ready = picture_readiness(&inputs.images, &key) != Readiness::Missing;
            assert_eq!(placed.is_some(), ready, "{name}");
        }
    }
}
