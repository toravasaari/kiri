//! Finding a term in the texts of a page, for the tests that draw marks over
//! it. The search itself looks in the document's text and asks the layout
//! where it is shown (`typeset::locate`); a page no longer finds anything.

use std::ops::Range;

use super::{ItemRef, Page, Piece, Shape};

/// A place of a search term on the page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hit {
    /// The text it is in, and its bytes there.
    pub(crate) item: ItemRef,
    pub(crate) range: Range<usize>,
    /// One piece for each line it covers, in the pixels of the page, or of
    /// the band `item` is in.
    pub(crate) pieces: Vec<Piece>,
    /// The top of its first piece from the top of the page.
    pub(crate) top: f32,
}

impl Page {
    /// Where `places` finds a search term in the texts on the page, from
    /// the top down: `places` gives the byte ranges in a text.
    pub(crate) fn find(&self, places: &dyn Fn(&str) -> Vec<Range<usize>>) -> Vec<Hit> {
        let mut found = Vec::new();
        let on_page = (self.items.iter().enumerate())
            .map(|(index, item)| (ItemRef { band: None, index }, 0.0, item));
        let in_bands = (self.bands.iter().enumerate()).flat_map(|(number, band)| {
            (band.items.iter().enumerate()).map(move |(index, item)| {
                let at = ItemRef {
                    band: Some(number),
                    index,
                };
                (at, band.top, item)
            })
        });
        for (at, top, item) in on_page.chain(in_bands) {
            let Shape::Text {
                paragraph, x, y, ..
            } = &item.shape
            else {
                continue;
            };
            let lines = paragraph.lines();
            for range in places(paragraph.text()) {
                let pieces: Vec<Piece> = (paragraph.boxes(range.clone()).into_iter())
                    .filter_map(|part| {
                        let line = lines.get(part.line)?;
                        Some(Piece {
                            x: x + part.left,
                            y: y + line.top,
                            width: part.right - part.left,
                            height: line.height,
                        })
                    })
                    .collect();
                if let Some(first) = pieces.first() {
                    let top = top + first.y;
                    found.push(Hit {
                        item: at,
                        range,
                        pieces,
                        top,
                    });
                }
            }
        }
        let key = |hit: &Hit| (hit.top, hit.pieces[0].x);
        found.sort_by(|a, b| {
            key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        found
    }
}
