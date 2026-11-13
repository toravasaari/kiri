//! Pictures in the flow: images, diagrams and formulas, placed at the left
//! or centred, and what a picture that is not ready shows instead.

use std::rc::Rc;

use crate::typeset::model::{Image, Inline};
use crate::typeset::reading::page::{Flow, LinkPlace, Loading, Picture, Piece, Shape};
use crate::typeset::text::inline_len;

impl Flow<'_> {
    /// Lays the picture `picture` out at `x` in `width`, at the left or
    /// `centred`: whether it is one. `false` for what is no picture, or a
    /// diagram that failed.
    pub(super) fn place(
        &mut self,
        picture: Picture,
        link: Option<&str>,
        x: f32,
        width: f32,
        centred: bool,
    ) -> bool {
        let mut reserved = 0.0;
        let (pixels, loading, height) = match picture {
            Picture::Ready {
                pixels,
                id,
                loading,
                ..
            } => {
                self.ids.push(id);
                let height = pixels.height() as f32;
                (Some((pixels, id)), loading, height)
            }
            Picture::Loading { width, height, tag } => {
                reserved = width as f32;
                (None, Some(tag), height as f32)
            }
            Picture::None | Picture::Failed(_) => return false,
        };
        let picture_width = match &pixels {
            Some((pixels, _)) => pixels.width() as f32,
            None => reserved,
        };
        let left = match centred {
            true => x + ((width - picture_width) / 2.0).max(0.0),
            false => x,
        };
        let top = self.y;
        if let (Some((pixels, _)), Some(url)) = (&pixels, link) {
            self.linked.push(LinkPlace {
                url: Rc::from(url),
                pieces: vec![Piece {
                    x: left,
                    y: top,
                    width: pixels.width() as f32,
                    height,
                }],
                band: None,
            });
        }
        if let Some((pixels, _)) = pixels {
            self.push(
                top..top + height,
                Shape::Picture {
                    x: left,
                    y: top,
                    pixels,
                },
            );
        }
        if let Some(tag) = loading {
            self.loading.push(Loading {
                rows: top..top + height,
                x: left,
                width: picture_width,
                tag,
            });
        }
        self.y += height;
        true
    }

    /// Lays an image block out: the picture, or its alt text in the quote
    /// colour where it is not drawn.
    pub(super) fn image(&mut self, image: &Image, link: Option<&str>, x: f32, width: f32) {
        let picture = self.pictures.image(image, (width, f32::MAX));
        let first = self.items.len();
        if self.place(picture, link, x, width, false) {
            // The picture stands in the place of the alt text.
            self.picture_is(first, 0..image.alt.iter().map(inline_len).sum());
            return;
        }
        // Where the picture is missing, its alt text is the link.
        let paragraph = match link {
            Some(url) => {
                let linked = [Inline::Link {
                    url: url.to_owned(),
                    content: image.alt.clone(),
                }];
                self.paragraph(&linked, self.look.quote, width)
            }
            None => self.paragraph(&image.alt, self.look.quote, width),
        };
        self.y += self.text(paragraph, x);
    }
}
