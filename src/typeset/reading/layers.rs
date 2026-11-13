//! How reading mode paints the highlights of the state engine
//! (`typeset/layers.rs`): an overlay on the page, made of the boxes of the
//! matches and the frame of the selected link.
//!
//! A range is turned into what shows it on a page by the map of the layout:
//! the bytes of the texts, which are boxes under the glyphs, and, for the
//! selected link, the link of the page that covers the first of them, which
//! gets a frame drawn over the boxes. The page is drawn from its parts as it
//! was, and its overlay is what differs.

use crate::typeset::layers::{Highlight, Paint};
use crate::typeset::locate::{Map, Spot};
use crate::typeset::reading::engine;
use crate::typeset::style::Line;

/// Puts an overlay over each page among `lines` that a match or the
/// selection is on.
pub(crate) fn paint(lines: &mut [Line], map: &Map, highlights: &[&Highlight]) {
    for page in engine::pages(lines) {
        let mut marks = Vec::new();
        let mut link = None;
        for highlight in highlights {
            let spots = map.page_spots(&page.parts, &highlight.range);
            match highlight.paint {
                Paint::Match { current, .. } => {
                    for spot in spots {
                        if let Spot::Text { item, bytes } = spot {
                            marks.push((item, bytes, current));
                        }
                    }
                }
                Paint::Selected if link.is_none() => {
                    link = spots.iter().find_map(|spot| match spot {
                        Spot::Text { item, bytes } => page
                            .parts
                            .centre_of(*item, bytes.clone())
                            .and_then(|centre| page.parts.link_at(&centre)),
                        Spot::Picture(centre) => page.parts.link_at(centre),
                    });
                }
                Paint::Selected | Paint::Tint { .. } => {}
            }
        }
        if let Some(overlay) = page.parts.overlay(marks, link) {
            engine::set_overlay(&mut lines[page.lines], Some(&overlay));
        }
    }
}
