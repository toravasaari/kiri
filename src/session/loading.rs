//! What stands in for the images of a reloaded document while they load
//! in the background, chosen by what their blocks showed before: a block
//! that changed shows its old image dimmed, and any other block, such as
//! one that stayed while its image file changed, shows as it does without
//! its image. The images nearest to the view load first.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::typeset::compare::{self, Comparison, Fate};
use crate::typeset::images::{ImageKey, Images, Placeholder, Source};
use crate::typeset::model::{Block, Document, Inline};
use crate::typeset::style::Line;

/// The key of the image `block` shows, when it may show one: the image of
/// a paragraph of only an image, or the diagram a code block may be drawn
/// as. Only a lone document is reloaded, so the key is of the first.
fn key_of(block: &Block) -> Option<ImageKey> {
    match block {
        Block::Paragraph(content) => {
            Inline::lone_image(content).map(|image| ImageKey::from(image.url.as_str()))
        }
        Block::CodeBlock {
            language: Some(language),
            code,
            ..
        } => Some(ImageKey::block(language, code)),
        _ => None,
    }
}

/// The images loading that `lines` show, nearest to the lines `visible`
/// first: those in view in the order they come, then the others by how
/// many lines away they are, one below the view before one above it at
/// the same distance.
pub fn nearest(lines: &[Line], visible: Range<usize>) -> Vec<ImageKey> {
    let mut found: Vec<_> = (lines.iter().enumerate())
        .filter_map(|(at, line)| {
            let band = line.band.as_ref()?;
            let shown = at.saturating_sub(band.above)..at + 1;
            let away = if shown.end <= visible.start {
                (visible.start - shown.end + 1, true)
            } else if shown.start >= visible.end {
                (shown.start - visible.end + 1, false)
            } else {
                (0, false)
            };
            Some((away, at, &band.key))
        })
        .collect();
    found.sort_by_key(|&(away, at, _)| (away, at));
    let mut seen = HashSet::new();
    (found.into_iter())
        .filter(|&(_, _, key)| seen.insert(key))
        .map(|(_, _, key)| key.clone())
        .collect()
}

/// The images reloading `old` as `new` left, which `changes` tells
/// apart, by what they are now and what they were before the reload.
pub struct Reloaded<'a> {
    pub old: &'a Document,
    pub new: &'a Document,
    pub changes: &'a Comparison,
    /// What stood in for the images that were loading before the reload.
    pub before: &'a HashMap<ImageKey, Placeholder>,
    /// The images the reload set aside: those the new document no longer
    /// shows, and those that load anew.
    pub replaced: &'a HashMap<ImageKey, Source>,
    /// The images the new document has at once.
    pub images: &'a Images,
}

impl Reloaded<'_> {
    /// What stands in for the image `key` while it loads: what stood in
    /// for it before, if it was loading then; the old image of the block
    /// it changed from, dimmed; and otherwise nothing that tells a size,
    /// also for a block that stayed while its image's file changed, so
    /// that it shows its code or alt text as it would without the image.
    pub fn placeholder(&self, key: &ImageKey) -> Placeholder {
        if let Some(before) = self.before.get(key) {
            return before.clone();
        }
        let new = compare::blocks(self.new);
        let Some((path, _)) = (new.iter()).find(|(_, block)| key_of(block).as_ref() == Some(key))
        else {
            return Placeholder::Unsized;
        };
        match self.changes.fate(path) {
            Some(Fate::Changed { was }) => {
                let old = compare::blocks(self.old);
                let old_key = (old.iter())
                    .find(|(path, _)| path == was)
                    .and_then(|(_, block)| key_of(block));
                (old_key.and_then(|old| self.old_image(&old))).unwrap_or(Placeholder::Unsized)
            }
            Some(Fate::Stayed { .. } | Fate::Added) | None => Placeholder::Unsized,
        }
    }

    /// The image `key` before the reload, as a placeholder shows it: the
    /// image dimmed, or what stood in for it if it was loading.
    fn old_image(&self, key: &ImageKey) -> Option<Placeholder> {
        if let Some(image) = self.replaced.get(key) {
            return Some(Placeholder::Dimmed(image.clone()));
        }
        if let Some(Ok(image)) = self.images.get(key) {
            return Some(Placeholder::Dimmed(image.clone()));
        }
        self.before.get(key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::compare::compare;
    use crate::typeset::parse::parse;

    fn square(size: u32) -> Source {
        Source::Raster(image::RgbaImage::new(size, size))
    }

    /// What stands in for `key` when `old` is reloaded as `new`, with
    /// `replaced` set aside and `before` standing in for what loaded.
    fn placeholder(
        old: &str,
        new: &str,
        key: impl Into<ImageKey>,
        replaced: &[(&str, Source)],
        before: &[(&str, Placeholder)],
    ) -> Placeholder {
        let (old, new) = (parse(old), parse(new));
        let changes = compare(&old, &new);
        let replaced = (replaced.iter())
            .map(|(url, image)| (ImageKey::from(*url), image.clone()))
            .collect();
        let before = (before.iter())
            .map(|(url, placeholder)| (ImageKey::from(*url), placeholder.clone()))
            .collect();
        let reloaded = Reloaded {
            old: &old,
            new: &new,
            changes: &changes,
            before: &before,
            replaced: &replaced,
            images: &Images::new(),
        };
        reloaded.placeholder(&key.into())
    }

    /// A blank line with the band of the image `url`, shown by `above`
    /// lines above it too.
    fn band(url: &str, above: usize) -> Line {
        Line {
            band: Some(crate::typeset::style::Band {
                columns: 4,
                indent: 0,
                low: false,
                key: url.into(),
                above,
            }),
            ..Line::blank()
        }
    }

    #[test]
    fn the_images_in_view_come_first_and_then_the_nearest_below_before_above() {
        let mut lines = vec![Line::blank(); 10];
        lines[0] = band("far.png", 0);
        lines[2] = band("above.png", 0);
        // Shown from above the view into it.
        lines[5] = band("in.png", 2);
        // Two lines away, as is the one above.
        lines[8] = band("below.png", 1);
        // Shown again further down.
        lines[9] = band("in.png", 0);
        let keys = nearest(&lines, 4..6);
        let urls: Vec<String> = keys.iter().map(ImageKey::to_string).collect();
        assert_eq!(urls, ["in.png", "below.png", "above.png", "far.png"]);
    }

    #[test]
    fn a_block_that_changed_shows_its_old_image_dimmed() {
        let shown = placeholder(
            "Text\n\n![a](a.png)\n",
            "Text\n\n![b](b.png)\n",
            "b.png",
            &[("a.png", square(4))],
            &[],
        );
        assert!(matches!(shown, Placeholder::Dimmed(image) if image.size() == (4, 4)));
    }

    #[test]
    fn a_diagram_that_changed_shows_its_old_drawing_dimmed() {
        let old = ImageKey::block("mermaid", "flowchart TD\n");
        let (old_doc, new_doc) = (
            parse("```mermaid\nflowchart TD\n```\n"),
            parse("```mermaid\nflowchart LR\n```\n"),
        );
        let changes = compare(&old_doc, &new_doc);
        let replaced = HashMap::from([(old, square(6))]);
        let reloaded = Reloaded {
            old: &old_doc,
            new: &new_doc,
            changes: &changes,
            before: &HashMap::new(),
            replaced: &replaced,
            images: &Images::new(),
        };
        let shown = reloaded.placeholder(&ImageKey::block("mermaid", "flowchart LR\n"));
        assert!(matches!(shown, Placeholder::Dimmed(image) if image.size() == (6, 6)));
    }

    #[test]
    fn a_block_that_stayed_while_its_file_changed_shows_as_without_its_image() {
        let source = "![a](a.png)\n";
        let shown = placeholder(source, source, "a.png", &[("a.png", square(5))], &[]);
        assert!(matches!(shown, Placeholder::Unsized), "{shown:?}");
    }

    #[test]
    fn a_new_block_or_one_without_an_old_image_has_no_size() {
        let added = placeholder("Text\n", "Text\n\n![b](b.png)\n", "b.png", &[], &[]);
        assert!(matches!(added, Placeholder::Unsized), "{added:?}");
        // Its old image did not load.
        let failed = placeholder("![a](a.png)\n", "![b](b.png)\n", "b.png", &[], &[]);
        assert!(matches!(failed, Placeholder::Unsized), "{failed:?}");
    }

    #[test]
    fn what_stood_in_before_stays_and_follows_a_block_that_changes_again() {
        let dimmed = Placeholder::Dimmed(square(3));
        let kept = placeholder(
            "![a](a.png)\n",
            "![a](a.png)\n",
            "a.png",
            &[],
            &[("a.png", dimmed.clone())],
        );
        assert!(
            matches!(&kept, Placeholder::Dimmed(image) if image.size() == (3, 3)),
            "{kept:?}"
        );
        let followed = placeholder(
            "![a](a.png)\n",
            "![b](b.png)\n",
            "b.png",
            &[],
            &[("a.png", dimmed)],
        );
        assert!(
            matches!(&followed, Placeholder::Dimmed(image) if image.size() == (3, 3)),
            "{followed:?}"
        );
    }
}
