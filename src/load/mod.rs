//! Loading: the platform's work of reading files and running the programs
//! that draw, for the engines to draw from the data it gives them.
//!
//! `load`, `load_with` and `load_parts` turn the images a document wants,
//! which `src/typeset/images/` plans, into pixels: they read the files
//! (`url.rs`) and have the renderers draw the diagrams. A failure is a
//! value in the result, never an error for the whole document.
//!
//! What does not belong here is deciding which images a document wants,
//! decoding a format or drawing from given data, which are the typesetting
//! engine's, and showing anything, which is the terminal layer's.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::diagrams::Request;
use crate::images::{Fonts, Images, LoadOptions, Part, Wanted, draw_heading, plan};
use crate::typeset::images::drawn::finish_diagram;
use crate::typeset::model::Document;
use url::load_url;

mod renderers;
pub(crate) mod url;

pub use renderers::Renderers;

#[cfg(test)]
pub(crate) mod tests;

/// Loads every image that `document` shows as an image block once,
/// resolving relative paths against `base`, the directory of the Markdown
/// file, and draws each diagram that `options` can draw once. Files larger
/// than `max_file_size` bytes are refused, and text in SVGs is drawn with
/// the system's fonts.
pub fn load(
    document: &Document,
    base: &Path,
    max_file_size: u64,
    options: &LoadOptions<'_>,
) -> Images {
    load_with(document, base, max_file_size, &Fonts::system(), options)
}

/// `load`, drawing text in SVGs with `fonts`.
pub fn load_with(
    document: &Document,
    base: &Path,
    max_file_size: u64,
    fonts: &Fonts,
    options: &LoadOptions<'_>,
) -> Images {
    let part = Part {
        document,
        number: 0,
        base,
        by_line: options.by_line,
        inline_images: false,
    };
    load_parts(&[part], max_file_size, fonts, options)
}

/// `load_with` for each of `parts`, into one map, with the diagrams of
/// all of them drawn together. `options.by_line` gives way to each part's
/// own.
pub fn load_parts(
    parts: &[Part<'_>],
    max_file_size: u64,
    fonts: &Fonts,
    options: &LoadOptions<'_>,
) -> Images {
    let cell = options.headings.map(|look| look.cell);
    let (wanted, mut have) = plan(parts, options.renderer, cell, None);
    let key = |&(index, ref image): &(usize, Wanted)| image.key().in_document(parts[index].number);
    // The diagrams first, all together, so a renderer can draw several
    // at once.
    let requests: Vec<Request> = (wanted.iter().zip(&have))
        .filter_map(|((_, image), have)| match image {
            Wanted::Diagram {
                language, source, ..
            } if have.is_none() => {
                let request = Request::new(language, source, options.theme);
                Some(request.in_colors(options.formula))
            }
            Wanted::Diagram { .. } | Wanted::Url(_) | Wanted::Heading { .. } => None,
        })
        .collect();
    let total = requests.len();
    let done = AtomicUsize::new(0);
    let progress = options.progress;
    let tell = |done| {
        if let Some(progress) = progress {
            progress(done, total);
        }
    };
    if total > 0 {
        tell(0);
    }
    let one_more = || tell(done.fetch_add(1, Ordering::Relaxed) + 1);
    let mut drawn = (options.renderer)
        .render_all(&requests, &one_more)
        .into_iter();
    let mut images = Images::new();
    for (image, have) in wanted.into_iter().zip(have.iter_mut()) {
        let key = key(&image);
        let (index, image) = image;
        let part = &parts[index];
        let loaded = match image {
            _ if have.is_some() => have.take().expect("an image kept"),
            Wanted::Url(url) => load_url(url, part.base, max_file_size, fonts),
            Wanted::Heading {
                level, ref words, ..
            } => match &options.headings {
                Some(look) => draw_heading(words, look.level(level), look.cell, fonts),
                None => unreachable!("headings are wanted only with a look"),
            },
            Wanted::Diagram {
                language,
                source,
                line,
            } => {
                let drawn = drawn.next().expect("a drawing for each diagram");
                let options = LoadOptions {
                    by_line: part.by_line,
                    ..*options
                };
                finish_diagram(
                    language,
                    &source,
                    line,
                    drawn,
                    max_file_size,
                    fonts,
                    &options,
                )
            }
        };
        images.insert(key, loaded);
    }
    images
}
