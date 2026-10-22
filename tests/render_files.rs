//! Several files shown one after another, each under a header that names
//! it.

use image::RgbaImage;
use kiri::caps::Capabilities;
use kiri::images::{ImageKey, Images, Source};
use kiri::model::{Block, Document, File};
use kiri::render::Inputs;

mod common;

use common::{WIDTH, quadrants, readable, sent, visible, with_images};

/// A file called `name` of the Markdown `source`.
fn markdown(name: &str, source: &str) -> File {
    File {
        name: name.to_owned(),
        document: kiri::parse::parse(source),
    }
}

/// An image file called `name`.
fn image_file(name: &str) -> File {
    File {
        name: name.to_owned(),
        document: Document {
            blocks: vec![Block::ImageFile(name.to_owned())],
        },
    }
}

/// `files` rendered with `inputs`.
fn render(files: &[File], inputs: &Inputs) -> String {
    let mut out = Vec::new();
    kiri::render::render_files(files, inputs, &mut out).expect("rendering to memory succeeds");
    String::from_utf8(out).expect("output is UTF-8")
}

/// Two short documents.
fn two() -> [File; 2] {
    [
        markdown("notes.md", "# Notes\n\nFirst file.\n"),
        markdown("docs/todo.md", "- one\n- two\n"),
    ]
}

#[test]
fn ansi_files() {
    insta::assert_snapshot!(visible(&render(
        &two(),
        &Inputs::new(Capabilities::ansi(WIDTH))
    )));
}

#[test]
fn plain_files() {
    insta::assert_snapshot!(render(&two(), &Inputs::new(Capabilities::plain(WIDTH))));
}

#[test]
fn each_file_starts_with_a_header_that_names_it() {
    let inputs = Inputs::new(Capabilities::plain(WIDTH));
    let stream = kiri::render::layout::files(&two(), &inputs);
    let text = |line: &kiri::render::style::Line| -> String {
        (line.lead.iter().chain(&line.spans))
            .map(|span| span.text.as_str())
            .collect()
    };
    assert_eq!(stream.starts.len(), 2);
    for (start, name) in stream.starts.iter().zip(["notes.md", "docs/todo.md"]) {
        let header: Vec<String> = (stream.lines[*start..start + 3].iter()).map(text).collect();
        assert_eq!(header[0], "─".repeat(usize::from(WIDTH)), "{name}");
        assert_eq!(header[1].trim(), name);
        assert_eq!(header[2], header[0], "{name}");
    }
    // A blank line comes between the files.
    assert!(stream.lines[stream.starts[1] - 1].is_blank());
}

#[test]
fn a_long_name_wraps_in_the_header() {
    let name = "a/very/long/directory/name/with/a/document/in/it/README.md";
    let inputs = Inputs::new(Capabilities::plain(30));
    let stream = kiri::render::layout::files(&[markdown(name, "Text.\n")], &inputs);
    let rules = (stream.lines.iter())
        .position(|line| line.is_blank())
        .expect("a blank line below the header");
    assert!(rules > 3, "{:?}", &stream.lines[..rules]);
}

/// kitty in a window of `WIDTH` columns and `rows` rows of 10 × 40 pixels.
fn window(rows: u16) -> Capabilities {
    Capabilities {
        height: Some(rows),
        cell_pixels: Some((10, 40)),
        ..Capabilities::kitty(WIDTH)
    }
}

#[test]
fn kitty_image_files() {
    let mut images = Images::new();
    for (number, name) in ["a.png", "b.png"].into_iter().enumerate() {
        let image = Source::Raster(quadrants(50, 30));
        images.insert(ImageKey::from(name).in_document(number), Ok(image));
    }
    let files = [image_file("a.png"), image_file("b.png")];
    insta::assert_snapshot!(readable(&render(&files, &with_images(&window(24), images))));
}

#[test]
fn an_image_file_fits_the_window_with_its_header() {
    // 480 × 240 pixels of window between the header and the last row:
    // the height decides.
    let mut images = Images::new();
    let image = Source::Raster(quadrants(1000, 2000));
    images.insert(ImageKey::from("tall.png"), Ok(image));
    let out = render(&[image_file("tall.png")], &with_images(&window(11), images));
    let keys = &sent(&out)[0].keys;
    assert_eq!((keys["s"].as_str(), keys["v"].as_str()), ("120", "240"));
    assert_eq!((keys["c"].as_str(), keys["r"].as_str()), ("12", "6"));
    assert!(out.contains("tall.png"), "{out:?}");
}

#[test]
fn an_image_file_that_cannot_be_drawn_shows_its_name() {
    let out = render(
        &[image_file("photo.jpg")],
        &Inputs::new(Capabilities::plain(WIDTH)),
    );
    assert!(out.contains("[image: photo.jpg]"), "{out:?}");
}

#[test]
fn files_with_the_same_image_url_show_their_own_images() {
    let mut images = Images::new();
    for (number, width) in [(0, 30), (1, 50)] {
        let image = Source::Raster(RgbaImage::new(width, 40));
        images.insert(ImageKey::from("fig.png").in_document(number), Ok(image));
    }
    let files = [
        markdown("a/notes.md", "![a](fig.png)\n"),
        markdown("b/notes.md", "![b](fig.png)\n"),
    ];
    let out = render(&files, &with_images(&window(24), images));
    let widths: Vec<String> = sent(&out)
        .iter()
        .map(|sent| sent.keys["s"].clone())
        .collect();
    // Twice as large in cells of 40 pixels, as in any document.
    assert_eq!(widths, ["60", "100"]);
}
