//! Document model to terminal output.
//!
//! This is the hub of rendering: the entry points that lay a document out
//! and write it to an `impl Write`. The options that a layout takes are in
//! `src/typeset/inputs.rs`. The two layout engines and the styled lines
//! between layout and output live in `src/typeset/`, and the byte writers in
//! `src/present/`.
//!
//! No layout rule, no escape sequence and no file access belongs in here.
//!
//! Layout rules go to the typesetting engine (`src/typeset/`), escape
//! sequences to the terminal layer (`src/present/`, kitty's in its
//! `kitty/`), and loading to the platform. The placement map in
//! docs/architecture.md has the paths.

use std::io::{self, Write};

use crate::images::{ImageKey, Images, Source};
use crate::present::encoder::Encoder;
use crate::present::output;
use crate::typeset::caps::Capabilities;
use crate::typeset::images::size;
use crate::typeset::inputs::Inputs;
use crate::typeset::model::{Document, File};
use crate::typeset::{driver, layout, style};

/// Renders `document` with `inputs`. Returns what the terminal holds of
/// its images, to check that they came.
pub fn render(document: &Document, inputs: &Inputs, out: &mut impl Write) -> io::Result<Encoder> {
    let lines = driver::layout(document, inputs);
    let page = output::Page::of(&inputs.theme);
    output::write(&lines, &inputs.images, &inputs.caps, page, out)
}

/// Renders `files` with `inputs`, one after another, each under a header
/// that names it, as [`render`] renders a document.
pub fn render_files(files: &[File], inputs: &Inputs, out: &mut impl Write) -> io::Result<Encoder> {
    let laid = driver::laid(driver::Content::Files(files), inputs);
    let page = output::Page::of(&inputs.theme);
    output::write(laid.lines(), &inputs.images, &inputs.caps, page, out)
}

/// Renders `file`, numbered `number` among the files shown together, under
/// its header, as `render_files` renders each of them, after the blank
/// line that comes between two files when `apart`, as [`render`] renders
/// a document.
pub fn render_file(
    file: &File,
    number: usize,
    apart: bool,
    inputs: &Inputs,
    out: &mut impl Write,
) -> io::Result<Encoder> {
    let mut lines = Vec::new();
    if apart {
        lines.push(style::Line::blank());
    }
    lines.extend(driver::file(file, number, inputs));
    let page = output::Page::of(&inputs.theme);
    output::write(&lines, &inputs.images, &inputs.caps, page, out)
}

/// Shows `image` on its own, as large as fits the window and no larger
/// than its natural size: a raster image one pixel per terminal pixel, an
/// SVG in CSS pixels. A row is left below it for the shell's prompt. Does
/// nothing when `caps` cannot draw images. Returns what the terminal holds
/// of the image, and the image, to check that it came.
pub fn render_image(
    image: Source,
    caps: &Capabilities,
    out: &mut impl Write,
) -> io::Result<(Encoder, Images)> {
    render_image_on(image, caps, output::Page::default(), out)
}

/// `render_image` on `page`, whose colours fill the rows beside the image,
/// as they do beside a document's lines.
pub fn render_image_on(
    image: Source,
    caps: &Capabilities,
    page: output::Page,
    out: &mut impl Write,
) -> io::Result<(Encoder, Images)> {
    let mut images = Images::new();
    if !caps.draws_images() {
        return Ok((Encoder::new(), images));
    }
    let Some(size) = size::on_its_own(&image, caps, 1) else {
        return Ok((Encoder::new(), images));
    };
    let key = ImageKey::from("");
    let lines = layout::image_rows(size::Placement::block(
        key.clone(),
        size,
        size::Shows::Image,
    ));
    images.insert(key, Ok(image));
    let encoder = output::write(&lines, &images, caps, page, out)?;
    Ok((encoder, images))
}
