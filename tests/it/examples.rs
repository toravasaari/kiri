//! The documents and themes in `examples/`.

use std::path::{Path, PathBuf};

use kiri::diagrams::{DiagramError, Drawn, Renderer, Request};
use kiri::images::{DEFAULT_MAX_FILE_SIZE, LoadOptions};
use kiri::render::Inputs;
use kiri::typeset::caps::Capabilities;
use kiri::typeset::fonts::Fonts;
use kiri::typeset::parse::csv::Separator;
use kiri::typeset::stretch::Tables;
use kiri::typeset::theme::{Paper, Theme};

use crate::common::example_theme;
use crate::common::steps::{Step, alike, assert_in_steps, assert_in_steps_of};

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
    // Two tables longer than a stretch, which the pager lays out in
    // stretches: their layout in steps is checked with the pager's inputs.
    names.extend([LONG_MARKDOWN.to_owned(), LONG_CSV.to_owned()]);
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
                let source = match name.as_str() {
                    LONG_MARKDOWN => long_table(false),
                    LONG_CSV => long_table(true),
                    _ => std::fs::read_to_string(examples.join(name))
                        .unwrap_or_else(|error| panic!("{name}: {error:?}")),
                };
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
                let images = kiri::load::load_with(
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
                let long = [LONG_MARKDOWN, LONG_CSV].contains(&name.as_str());
                let mut with_images = Inputs {
                    images,
                    tables: long.then(Tables::default),
                    ..Inputs::new(Capabilities::plain(16))
                };
                let mut terminal = 0;
                for terminal_width in [16, 24, 40, 80, 120] {
                    for caps in [
                        Capabilities::plain(terminal_width),
                        Capabilities::ansi(terminal_width),
                        Capabilities::kitty(terminal_width),
                    ] {
                        terminal += 1;
                        for (number, theme) in themes.iter().enumerate() {
                            without_images.caps = caps.clone();
                            without_images.theme = theme.clone();
                            let mut out = Vec::new();
                            kiri::render::render(&document, &without_images, &mut out)
                                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
                            assert!(!out.is_empty());
                            with_images.caps = caps.clone();
                            with_images.theme = theme.clone();
                            // A layout made in steps is the layout made whole. Each
                            // terminal and width gets one theme, the next for the next
                            // terminal, so that every theme is one of several: the
                            // check lays a document out four times.
                            if number == terminal % themes.len() {
                                let content = kiri::typeset::driver::Content::Document(&document);
                                assert_in_steps(name, content, alike(&with_images));
                            }
                            for line in kiri::typeset::driver::layout(&document, &with_images) {
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

/// The names of the generated long tables in the loop.
const LONG_MARKDOWN: &str = "a long table.md";
const LONG_CSV: &str = "a long table.csv";

/// A table of 300 rows, as a CSV or in Markdown: short rows, cells that
/// wrap, a wide row in the middle and empty cells, so that its stretches
/// have rows of every height.
fn long_table(csv: bool) -> String {
    let mut rows = vec![["Number", "Name", "Notes"].map(str::to_owned)];
    for row in 0..300 {
        rows.push(match row {
            150 => [
                row.to_string(),
                "the widest row of the table with words enough to wrap".to_owned(),
                "Київ і Софія".to_owned(),
            ],
            _ if row % 7 == 3 => [row.to_string(), String::new(), String::new()],
            _ => [row.to_string(), format!("name {row}"), "note".to_owned()],
        });
    }
    let line = |row: &[String; 3]| match csv {
        true => row.join(",") + "\n",
        false => format!("| {} |\n", row.join(" | ")),
    };
    let mut source: String = rows.iter().take(1).map(line).collect();
    if !csv {
        source += "|---|---|---|\n";
    }
    source + &rows[1..].iter().map(line).collect::<String>()
}

#[test]
fn all_example_themes_load() {
    let dir = examples().join("themes");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            kiri::load::theme::load(&path).unwrap_or_else(|error| panic!("{error}"));
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

/// A Rust file of `count` lines, with a comment that runs over the seams
/// between stretches, tabs, a line that wraps and some of no text.
fn rust(count: usize) -> String {
    let mut code = String::new();
    for line in 0..count {
        let text = match line % 512 {
            508 => "/* a comment that goes on".to_owned(),
            509..=511 | 0..=2 => "   still the comment".to_owned(),
            3 => "   the end */ fn after() {}".to_owned(),
            _ if line % 97 == 0 => {
                format!("\tlet tabbed{line} = \"a\\tb\"; // {}", "word ".repeat(20))
            }
            _ if line % 89 == 0 => String::new(),
            _ => format!("fn f{line}() {{ let x = {line}; }}"),
        };
        code.push_str(&text);
        code.push('\n');
    }
    code
}

#[test]
fn a_code_block_longer_than_a_stretch_is_laid_out_in_steps_as_it_is_whole() {
    use kiri::typeset::driver::Content;
    let code = rust(1100);
    // As a block of a Markdown document, between paragraphs, and as a file
    // with and without a language.
    let markdown = kiri::typeset::parse::parse(&format!(
        "Before the code.\n\n```rust\n{code}```\n\nAfter it, with a [link](https://example.com).\n"
    ));
    let file = kiri::typeset::parse::file(&code, Some("rust"));
    let text = kiri::typeset::parse::file(&code, None);
    // A terminal of each kind, narrow and wide, with a theme or two.
    for (caps, theme) in [
        (Capabilities::plain(40), Theme::default()),
        (Capabilities::ansi(24), Theme::paper(Paper::Light)),
        (Capabilities::kitty(60), Theme::default()),
        (Capabilities::kitty(120), Theme::paper(Paper::Dark)),
    ] {
        let inputs = Inputs {
            theme,
            ..Inputs::new(caps.clone())
        };
        for (what, document) in [("markdown", &markdown), ("file", &file), ("text", &text)] {
            let name = format!("{what}, {} wide", caps.width);
            // A block of three stretches stops at each of them whatever the step;
            // the steps that differ are by the piece and by the seam.
            let steps = [Step::Piece, Step::Seam(512)];
            let content = Content::Document(document);
            assert_in_steps_of(&name, content, alike(&inputs), &steps);
        }
    }
}
