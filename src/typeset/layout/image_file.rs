//! An image file shown on its own, as one of several files is.

use super::{Layout, image_rows};
use crate::typeset::images::ImageKey;
use crate::typeset::images::size::{self, Placement, Shows};
use crate::typeset::model::{Image, Inline};
use crate::typeset::style::{Line, Style};

impl Layout<'_> {
    /// The image file `name` shown on its own, as large as fits the window
    /// with the rows it leaves to others; its alt text, the name, where it
    /// cannot be drawn, at most `width` cells wide. An image that is not
    /// decoded yet is sized by the natural size noted for it, which gives
    /// the rows its pixels would.
    pub(super) fn image_file(&self, name: &str, width: usize) -> Vec<Line> {
        let key = ImageKey::from(name).in_document(self.document);
        let size = match self.images.get(&key) {
            Some(Ok(source)) => size::on_its_own(source, self.caps, self.reserved),
            _ => (self.images.extent(&key)).and_then(|(natural, unit)| {
                size::on_its_own_at(natural, unit, self.caps, self.reserved)
            }),
        };
        if self.caps.draws_images()
            && let Some(size) = size
        {
            return image_rows(Placement::block(key, size, Shows::Image));
        }
        let alt = Inline::Image(Image {
            url: name.to_owned(),
            alt: vec![Inline::Text(name.to_owned())],
            width: None,
            height: None,
        });
        let mut spans = Vec::new();
        self.inline(&alt, &Style::default(), &mut spans);
        super::wrap(&spans, width)
            .into_iter()
            .map(Line::new)
            .collect()
    }
}
