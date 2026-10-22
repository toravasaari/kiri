//! The documents and themes in `examples/`.

use std::path::{Path, PathBuf};

use kiri::caps::Capabilities;
use kiri::images::{DEFAULT_MAX_FILE_SIZE, Fonts, LoadOptions};
use kiri::render::Inputs;
use kiri::theme::{Paper, Theme};

mod common;

use common::example_theme;

/// The `examples/` directory.
fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

#[test]
fn the_visual_check_documents_fit_every_mode() {
    use kiri::render::wrap::width;
    let examples = examples();
    let themes = [
        Theme::default(),
        example_theme("background.toml"),
        example_theme("rules.toml"),
        example_theme("opaque.toml"),
        kiri::theme::parse("inline_code_wrap = false\n").unwrap(),
        Theme::paper(Paper::Light),
        Theme::paper(Paper::Dark),
    ];
    // Each document says which of its images kiri warns about. A build
    // without the full set of image formats also warns about the six
    // images in the formats it leaves out.
    let left_out = if cfg!(feature = "full") { 0 } else { 6 };
    for (name, warnings) in [("visual-check.md", 1), ("S06a-images.md", 5 + left_out)] {
        let source = std::fs::read_to_string(examples.join(name)).unwrap();
        let document = kiri::parse::parse(&source);
        // Layout needs only the sizes of the images, so it gets the real
        // ones; rendering them at every width would only take longer. Text
        // in SVGs does not change their size, so no fonts are loaded.
        let images = kiri::images::load_with(
            &document,
            &examples,
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
            &LoadOptions::default(),
        );
        assert_eq!(images.warnings().len(), warnings, "{name}: {images:?}");
        let mut without_images = Inputs::new(Capabilities::plain(16));
        let mut with_images = Inputs {
            images,
            ..Inputs::new(Capabilities::plain(16))
        };
        for terminal_width in [16, 24, 40, 80, 120] {
            for caps in [
                Capabilities::plain(terminal_width),
                Capabilities::ansi(terminal_width),
                Capabilities::kitty(terminal_width),
            ] {
                for theme in &themes {
                    without_images.caps = caps.clone();
                    without_images.theme = theme.clone();
                    let mut out = Vec::new();
                    kiri::render::render(&document, &without_images, &mut out).unwrap();
                    assert!(!out.is_empty());
                    with_images.caps = caps.clone();
                    with_images.theme = theme.clone();
                    for line in kiri::render::layout::layout(&document, &with_images) {
                        let image = (line.image.as_ref())
                            .map_or(0, |image| usize::from(image.placement.size.columns));
                        let cells = width(&line.lead)
                            + width(&line.spans) * usize::from(line.scale)
                            + image
                            + width(&line.trail);
                        assert!(
                            cells <= usize::from(terminal_width),
                            "{name}, {caps:?}: {cells} cells in {line:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn all_example_themes_load() {
    let dir = examples().join("themes");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            kiri::theme::load(&path).unwrap_or_else(|error| panic!("{error}"));
        }
    }
}

#[test]
fn the_mermaid_showcase_shows_each_example_file_as_it_is() {
    let showcase = std::fs::read_to_string(examples().join("mermaid-showcase.md")).unwrap();
    let mut shown = Vec::new();
    // Each block follows the link to its source file.
    let mut rest = showcase.as_str();
    while let Some((before, block)) = rest.split_once("```mermaid\n") {
        let (_, link) = before
            .rsplit_once("](mermaid/")
            .expect("a link to its file");
        let (name, _) = link.split_once(')').unwrap();
        let (code, after) = block.split_once("```").unwrap();
        let file = std::fs::read_to_string(examples().join("mermaid").join(name)).unwrap();
        assert_eq!(code, file, "{name}");
        shown.push(name.to_owned());
        rest = after;
    }
    let mut files: Vec<String> = std::fs::read_dir(examples().join("mermaid"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    shown.sort();
    assert_eq!(shown, files);
}
