//! Several files shown one after another, each under a header that names
//! it.

use image::RgbaImage;
use kiri::images::{ImageKey, Images, Source};
use kiri::render::Inputs;
use kiri::typeset::caps::Capabilities;
use kiri::typeset::color::Color;
use kiri::typeset::model::{Block, Document, File};
use kiri::typeset::style::Line;

mod common;

use common::{WIDTH, quadrants, readable, sent, visible, with_images};

/// A file called `name` of the Markdown `source`.
fn markdown(name: &str, source: &str) -> File {
    File {
        name: name.to_owned(),
        document: kiri::typeset::parse::parse(source),
    }
}

/// An image file called `name`.
fn image_file(name: &str) -> File {
    File {
        name: name.to_owned(),
        document: Document::new(vec![Block::ImageFile(name.to_owned())]),
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
fn kitty_files_on_a_known_page() {
    insta::assert_snapshot!(visible(&render(&two(), &Inputs::new(known_colours()))));
}

#[test]
fn plain_files() {
    insta::assert_snapshot!(render(&two(), &Inputs::new(Capabilities::plain(WIDTH))));
}

#[test]
fn each_file_starts_with_a_header_that_names_it() {
    // Plain text names each file between arrows, as head does.
    let inputs = Inputs::new(Capabilities::plain(WIDTH));
    let stream = kiri::typeset::layout::files(&two(), &inputs);
    let starts = starts(&stream);
    assert_eq!(starts.len(), 2);
    for (start, name) in starts.iter().zip(["notes.md", "docs/todo.md"]) {
        let header = line_text(&stream.lines[*start]);
        assert_eq!(header, format!("==> {name} <=="));
        assert!(stream.lines[start + 1].is_blank(), "{name}");
    }
    // A blank line comes between the files.
    assert!(stream.lines[starts[1] - 1].is_blank());
}

/// The line each file of `stream` starts on, found from the blocks of its
/// lines.
fn starts(stream: &kiri::typeset::layout::Stream) -> Vec<usize> {
    let file_of = |line: &Line| line.block.map(|id| stream.blocks.get(id).file());
    let count = stream
        .lines
        .iter()
        .filter_map(file_of)
        .max()
        .map_or(0, |last| last + 1);
    (0..count)
        .map(|file| {
            let found = stream
                .lines
                .iter()
                .position(|line| file_of(line) == Some(file));
            found.expect("every file has lines")
        })
        .collect()
}

/// The text of `line`, all its spans.
fn line_text(line: &Line) -> String {
    (line.lead.iter().chain(&line.spans).chain(&line.trail))
        .map(|span| span.text.as_str())
        .collect()
}

/// kitty on a black page with light grey text, as kitty starts.
fn known_colours() -> Capabilities {
    Capabilities {
        background: Some(Color::Rgb(0, 0, 0)),
        foreground: Some(Color::Rgb(0xdd, 0xdd, 0xdd)),
        dark_background: Some(true),
        ..Capabilities::kitty(WIDTH)
    }
}

#[test]
fn the_header_is_a_band_with_a_bar_that_markdown_cannot_draw() {
    let inputs = Inputs::new(known_colours());
    let stream = kiri::typeset::layout::files(&two(), &inputs);
    let start = starts(&stream)[1];
    let header = &stream.lines[start..start + 3];
    // An empty row, the name and another empty row, each the text's
    // width, all on the band's colour, with the bar down the left edge.
    let band = header[0].spans[0].style.bg;
    assert!(band.is_some(), "{header:?}");
    for line in header {
        let text = line_text(line);
        assert!(text.starts_with('▌'), "{text:?}");
        assert_eq!(text.chars().count(), usize::from(WIDTH), "{text:?}");
        assert_eq!(line.spans[0].style.fg, Some(Color::Palette(4)), "{line:?}");
        assert!(
            line.spans.iter().all(|span| span.style.bg == band),
            "{line:?}"
        );
    }
    assert_eq!(line_text(&header[0]).trim_end(), "▌");
    assert_eq!(line_text(&header[1]).trim_end(), "▌ docs/todo.md");
    assert_eq!(line_text(&header[2]).trim_end(), "▌");
    // The directory muted, the file's own name in bold.
    let span = |text: &str| {
        (header[1].spans.iter())
            .find(|span| span.text == text)
            .unwrap_or_else(|| panic!("{text:?} in {:?}", header[1]))
    };
    assert!(span("docs/").style.dim && !span("docs/").style.bold);
    assert!(span("todo.md").style.bold && !span("todo.md").style.dim);
    // A blank line below the band and above it, after the first file.
    assert!(stream.lines[start + 3].is_blank());
    assert!(stream.lines[start - 1].is_blank());
}

#[test]
fn the_band_is_lighter_on_a_light_page_and_gone_without_colours() {
    let band = |caps: Capabilities| {
        let stream = kiri::typeset::layout::files(&two(), &Inputs::new(caps));
        let header = &stream.lines[..3];
        (
            header.iter().map(line_text).collect::<Vec<_>>(),
            header[1].spans[0].style.bg,
        )
    };
    // A little of the black text in the white page: a light grey.
    let light = Capabilities {
        background: Some(Color::Rgb(0xff, 0xff, 0xff)),
        foreground: Some(Color::Rgb(0, 0, 0)),
        dark_background: Some(false),
        ..known_colours()
    };
    let (_, bg) = band(light);
    assert!(
        matches!(bg, Some(Color::Rgb(r, _, _)) if (0xd0..0xf8).contains(&r)),
        "{bg:?}"
    );
    // Without colours, the bar alone marks the header, on every row.
    let (rows, bg) = band(Capabilities {
        color: false,
        ..known_colours()
    });
    assert_eq!(bg, None);
    assert_eq!(rows, ["▌", "▌ notes.md", "▌"]);
}

#[test]
fn a_file_that_failed_shows_why_in_the_error_colour() {
    let failed = File {
        name: "missing.md".to_owned(),
        document: Document::new(vec![Block::Error("No such file or directory".to_owned())]),
    };
    let stream = kiri::typeset::layout::files(&[failed], &Inputs::new(known_colours()));
    let line = &stream.lines[4];
    assert_eq!(line_text(line), "  ✗ No such file or directory");
    let message = line
        .spans
        .iter()
        .find(|span| span.text.contains('✗'))
        .unwrap();
    assert_eq!(message.style.fg, Some(Color::Palette(1)));
}

#[test]
fn a_long_name_wraps_in_the_header() {
    let name = "a/very/long/directory/name/with/a/document/in/it/README.md";
    let inputs = Inputs::new(Capabilities {
        width: 30,
        ..known_colours()
    });
    let stream = kiri::typeset::layout::files(&[markdown(name, "Text.\n")], &inputs);
    let header = (stream.lines.iter())
        .position(|line| line.is_blank())
        .expect("a blank line below the header");
    let header: Vec<String> = stream.lines[..header].iter().map(line_text).collect();
    // The band grows with the name, each of its rows the text's width.
    assert!(header.len() > 3, "{header:#?}");
    for row in &header {
        assert!(row.starts_with('▌'), "{header:#?}");
        assert_eq!(row.chars().count(), 30, "{header:#?}");
    }
    let shown: String = header.iter().map(|row| row[3..].trim()).collect();
    assert_eq!(shown, name);
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
fn an_image_file_fits_the_window_with_a_header_of_a_long_name() {
    // The name takes two rows of the band, so 200 pixels are left.
    let name = "a/very/long/directory/name/with/an/image/in/it/tall.png";
    let mut images = Images::new();
    let image = Source::Raster(quadrants(1000, 2000));
    images.insert(ImageKey::from(name), Ok(image));
    let out = render(&[image_file(name)], &with_images(&window(11), images));
    let keys = &sent(&out)[0].keys;
    assert_eq!((keys["s"].as_str(), keys["v"].as_str()), ("100", "200"));
    assert_eq!((keys["c"].as_str(), keys["r"].as_str()), ("10", "5"));
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
