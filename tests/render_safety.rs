//! What the document cannot do to the terminal, and the visual check
//! documents in every mode.

use std::path::Path;

use kiri::caps::Capabilities;
use kiri::images::{DEFAULT_MAX_FILE_SIZE, Fonts, LoadOptions};
use kiri::render::Inputs;
use kiri::theme::{Paper, Theme};

mod common;

use common::{WIDTH, example_theme, render, strip_own_sequences};

#[test]
fn escape_sequences_in_the_document_are_neutralised() {
    let out = render(
        "text \x1b]0;evil\x07 here\n",
        &Inputs::new(Capabilities::kitty(WIDTH)),
    );
    assert!(!out.contains("\x1b]0;"), "{out:?}");
}

#[test]
fn control_characters_in_the_document_do_not_reach_the_terminal() {
    // ESC and the C1 string terminator U+009C in a heading, a link target,
    // image alt text, a table cell, a code fence's info string and a code
    // block.
    let source = "# Head \x1b]0;evil\x07 \u{9c}\n\n\
        [link](<https://example.com/\x1b]0;evil\x07\u{9c}>) and \
        ![alt \x1bc \u{9c}](image.png)\n\n\
        | Cell |\n|---|\n| \x1b]0;evil\x07 \u{9c} |\n\n\
        ```rust\x1b]0;evil\x07\u{9c}\ncode \x1b[2J \u{9c}\n```\n";
    let document = kiri::parse::parse(source);
    // The control characters survive parsing, so the renderer sees them.
    assert_eq!(format!("{document:?}").matches("\\u{1b}").count(), 6);
    assert_eq!(format!("{document:?}").matches("\\u{9c}").count(), 6);
    for caps in [
        Capabilities::plain(WIDTH),
        Capabilities::ansi(WIDTH),
        Capabilities::kitty(WIDTH),
    ] {
        let inputs = Inputs::new(caps.clone());
        let mut out = Vec::new();
        kiri::render::render(&document, &inputs, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        if caps.hyperlinks {
            // The link target is still written, without the controls.
            assert!(
                out.contains("\x1b]8;;https://example.com/]0;evil\x1b\\"),
                "{out:?}"
            );
        }
        let text = strip_own_sequences(&out);
        assert!(
            !text.chars().any(|c| c.is_control() && c != '\n'),
            "{caps:?}: {out:?}"
        );
    }
}

#[test]
fn the_visual_check_documents_fit_every_mode() {
    use kiri::render::wrap::width;
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
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
    for (name, warnings) in [
        ("visual-check.md", 1),
        ("visual-check-images.md", 5 + left_out),
    ] {
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
