//! The documents and themes in `examples/`.

use std::path::{Path, PathBuf};

use kiri::diagrams::{DiagramError, Drawn, Renderer, Request};
use kiri::images::{DEFAULT_MAX_FILE_SIZE, Fonts, LoadOptions};
use kiri::render::Inputs;
use kiri::typeset::caps::Capabilities;
use kiri::typeset::parse::csv::Separator;
use kiri::typeset::theme::{Paper, Theme};

use crate::common::example_theme;

/// The `examples/` directory.
fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

/// Draws Mermaid without merman, too wide for any window so the layout must
/// fit it: an empty SVG 1500 CSS pixels wide, wider than the widest window
/// the fit test tries, and 20 CSS pixels high for each line of source. A
/// diagram whose last arrow leads nowhere fails, as it does in merman, with
/// a reason long enough to wrap.
struct WideDiagrams;

impl Renderer for WideDiagrams {
    fn draws(&self, language: &str) -> bool {
        language.eq_ignore_ascii_case("mermaid")
    }

    fn render(&self, request: &Request) -> Result<Drawn, DiagramError> {
        let &Request { source, theme, .. } = request;
        if source.trim_end().ends_with("-->") {
            return Err("the last arrow of the diagram leads nowhere: \
                 expected the name of a node after `-->` at the end of the input"
                .into());
        }
        let height = 20 * source.lines().count();
        let svg =
            format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1500" height="{height}"/>"#);
        Ok(Drawn::svg(svg, theme, None))
    }
}

#[test]
fn the_visual_check_documents_fit_every_mode() {
    use kiri::typeset::wrap::width;
    let examples = examples();
    let themes = [
        Theme::default(),
        example_theme("background.toml"),
        example_theme("rules.toml"),
        example_theme("opaque.toml"),
        kiri::typeset::theme::parse("inline_code_wrap = false\n").unwrap(),
        Theme::paper(Paper::Light),
        Theme::paper(Paper::Dark),
    ];
    // The baseline, every topic document and the topic CSV files. Each says
    // which of its images kiri warns about: the baseline none, the images
    // document the five in its last section, and a build without the full
    // set of image formats also warns about the six images in the formats
    // it leaves out. The diagrams document warns about its diagram that
    // fails to draw.
    let left_out = if cfg!(feature = "full") { 0 } else { 6 };
    let mut names = vec!["README.md".to_owned()];
    for entry in std::fs::read_dir(&examples).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.starts_with('S') && (name.ends_with(".md") || name.ends_with(".csv")) {
            names.push(name);
        }
    }
    names.sort();
    assert!(names.iter().any(|name| name.ends_with(".csv")));
    // One thread per document, so the slowest documents overlap. A failed
    // assertion panics in its thread; the scope then fails the test, and the
    // message still names the document.
    std::thread::scope(|scope| {
        for name in &names {
            let (examples, themes) = (&examples, &themes);
            scope.spawn(move || {
                let warnings = match name.as_str() {
                    "S06a-images.md" => 5 + left_out,
                    "S07a-diagrams.md" => 1,
                    _ => 0,
                };
                let source = std::fs::read_to_string(examples.join(name))
                    .unwrap_or_else(|error| panic!("{name}: {error:?}"));
                let document = if name.ends_with(".csv") {
                    kiri::typeset::parse::csv::parse(&source, Separator::Comma)
                        .unwrap_or_else(|error| panic!("{name}: {error:?}"))
                } else {
                    kiri::typeset::parse::parse(&source)
                };
                // Layout needs only the sizes of the images, so it gets the real
                // ones; rendering them at every width would only take longer. Text
                // in SVGs does not change their size, so no fonts are loaded.
                // Diagrams are drawn wider than any window, and the one that
                // fails shows its reason below its code.
                let images = kiri::images::load_with(
                    &document,
                    examples,
                    DEFAULT_MAX_FILE_SIZE,
                    &Fonts::none(),
                    &LoadOptions {
                        renderer: &WideDiagrams,
                        ..LoadOptions::default()
                    },
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
                        for theme in themes {
                            without_images.caps = caps.clone();
                            without_images.theme = theme.clone();
                            let mut out = Vec::new();
                            kiri::render::render(&document, &without_images, &mut out)
                                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
                            assert!(!out.is_empty());
                            with_images.caps = caps.clone();
                            with_images.theme = theme.clone();
                            for line in kiri::typeset::layout::layout(&document, &with_images) {
                                let image = (line.image.as_ref())
                                    .map_or(0, |image| usize::from(image.placement.size.columns));
                                let cells = width(&line.lead)
                                    + width(&line.spans)
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
            });
        }
    });
}

#[test]
fn all_example_themes_load() {
    let dir = examples().join("themes");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            kiri::typeset::theme::load(&path).unwrap_or_else(|error| panic!("{error}"));
        }
    }
}

#[test]
fn the_diagrams_document_shows_each_mermaid_file_as_it_is() {
    let diagrams = std::fs::read_to_string(examples().join("S07a-diagrams.md")).unwrap();
    let mut shown = Vec::new();
    // The document also holds diagrams that are not from `mermaid/`. A block
    // from a file follows a link to it, under the same heading.
    let mut rest = diagrams.as_str();
    while let Some((before, block)) = rest.split_once("```mermaid\n") {
        let (code, after) = block.split_once("```").unwrap();
        rest = after;
        let section = before
            .rsplit_once("\n#")
            .map_or(before, |(_, section)| section);
        let Some((_, link)) = section.rsplit_once("](mermaid/") else {
            continue;
        };
        let (name, _) = link.split_once(')').unwrap();
        let file = std::fs::read_to_string(examples().join("mermaid").join(name)).unwrap();
        assert_eq!(code, file, "{name}");
        shown.push(name.to_owned());
    }
    let mut files: Vec<String> = std::fs::read_dir(examples().join("mermaid"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    shown.sort();
    assert_eq!(shown, files);
}
