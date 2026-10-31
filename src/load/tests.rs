//! The tests of loading, and the helpers other tests load images with.

use std::path::{Path, PathBuf};

use image::RgbaImage;

use crate::diagrams::{DiagramError, Drawn, Renderer, Request, Theme};
use crate::images::{
    CodeLook, DEFAULT_MAX_FILE_SIZE, Fonts, HeadingLook, ImageError, ImageKey, Images, LevelLook,
    LoadOptions, Looks, Part, Source, plain_words, plan,
};
use crate::load::url::load_url;
use crate::load::{load, load_parts, load_with};
use crate::typeset::diagnostic::Diagnostic;

/// The text of each of `diagnostics`, as standard error tells them.
fn texts(diagnostics: Vec<Diagnostic>) -> Vec<String> {
    diagnostics.iter().map(ToString::to_string).collect()
}

pub(crate) fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The images of the Markdown `source` with the default size limit, no
/// fonts and the fixtures directory as the base.
pub(crate) fn load_source(source: &str, options: &LoadOptions<'_>) -> Images {
    load_with(
        &crate::typeset::parse::parse(source),
        &fixtures(),
        DEFAULT_MAX_FILE_SIZE,
        &Fonts::none(),
        options,
    )
}

/// The image at `url` with the default size limit and no fonts.
pub(crate) fn open(url: &str, base: &Path) -> Result<Source, ImageError> {
    load_url(url, base, DEFAULT_MAX_FILE_SIZE, &Fonts::none())
}

pub(crate) fn fixture(name: &str) -> Result<Source, ImageError> {
    open(name, &fixtures().join("images"))
}

pub(crate) fn rgba(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    image.get_pixel(x, y).0
}

/// A look for headings in cells of 10 × 20 pixels.
fn heading_look() -> HeadingLook {
    let level = LevelLook {
        rows: 2,
        font_px: 30.0,
        bold: true,
        italic: false,
        color: [0x20, 0x60, 0xd0],
        code: CodeLook {
            color: [0xe0, 0x20, 0xe0],
            background: Some([0x30, 0x30, 0x30]),
            padding: true,
        },
    };
    HeadingLook {
        cell: (10, 20),
        levels: [level; 6],
    }
}

/// The images of the Markdown `source`, with headings drawn as
/// `heading_look` says in `fonts`.
fn with_headings(source: &str, fonts: &Fonts) -> Images {
    let options = LoadOptions {
        headings: Some(heading_look()),
        ..LoadOptions::default()
    };
    let document = crate::typeset::parse::parse(source);
    load_with(
        &document,
        &fixtures(),
        DEFAULT_MAX_FILE_SIZE,
        fonts,
        &options,
    )
}

#[test]
fn heading_images_load_when_asked() {
    let images = with_headings("# Kiri\n\n## Kiri *is* `here`\n", &Fonts::fixture());
    let one = images.get(&ImageKey::heading(1, "Kiri", (10, 20)));
    assert!(matches!(one, Some(Ok(Source::Text { .. }))), "{images:?}");
    // Emphasis in italics, inline code as code, and apart from the
    // same text at another level.
    let mut words = plain_words("Kiri is here");
    words[1].italic = true;
    words[2].code = Some(0);
    let two = images.get(&ImageKey::heading_of(2, words, (10, 20)));
    assert!(matches!(two, Some(Ok(Source::Text { .. }))), "{images:?}");
    assert_eq!(images.len(), 2);
}

#[test]
fn the_same_words_with_and_without_code_are_two_images() {
    let images = with_headings("# a `b`\n\n# a b\n", &Fonts::fixture());
    assert_eq!(images.len(), 2, "{images:?}");
    // So with and without emphasis.
    let images = with_headings("# a *b*\n\n# a b\n", &Fonts::fixture());
    assert_eq!(images.len(), 2, "{images:?}");
}

#[test]
fn heading_images_inside_details_load() {
    let source = "<details>\n<summary>S</summary>\n\n# Inside\n\n</details>\n";
    let images = with_headings(source, &Fonts::fixture());
    let inside = images.get(&ImageKey::heading(1, "Inside", (10, 20)));
    assert!(matches!(inside, Some(Ok(_))), "{images:?}");
}

#[test]
fn no_heading_images_by_default() {
    let images = load_source("# Kiri\n", &LoadOptions::default());
    assert!(images.is_empty(), "{images:?}");
}

#[test]
fn headings_in_a_quote_are_not_loaded() {
    let source = "> # Quoted\n\n- # Listed\n\n> [!NOTE]\n> # Alerted\n\n# Top\n";
    let images = with_headings(source, &Fonts::fixture());
    let keys: Vec<String> = images.iter().map(|(key, _)| key.to_string()).collect();
    assert_eq!(keys, ["heading \"Top\""]);
}

#[test]
fn a_failed_heading_image_warns_of_nothing() {
    let images = with_headings("# Kiri\n", &Fonts::none());
    let image = images.get(&ImageKey::heading(1, "Kiri", (10, 20)));
    assert!(matches!(image, Some(Err(_))), "{images:?}");
    assert_eq!(texts(images.warnings()), Vec::<String>::new());
    assert_eq!(texts(images.warnings_in(0, true)), Vec::<String>::new());
}

#[test]
fn the_same_heading_in_two_documents_has_two_images() {
    let options = LoadOptions {
        headings: Some(heading_look()),
        ..LoadOptions::default()
    };
    let document = crate::typeset::parse::parse("# Kiri\n");
    let part = |number| Part {
        document: &document,
        number,
        base: Path::new("."),
        by_line: true,
        inline_images: false,
    };
    let images = load_parts(
        &[part(0), part(1)],
        DEFAULT_MAX_FILE_SIZE,
        &Fonts::fixture(),
        &options,
    );
    assert!(
        images
            .get(&ImageKey::heading(1, "Kiri", (10, 20)))
            .is_some()
    );
    let second = ImageKey::heading(1, "Kiri", (10, 20)).in_document(1);
    assert!(images.get(&second).is_some(), "{images:?}");
}

#[test]
fn loads_the_images_inside_details() {
    let document = crate::typeset::parse::parse(
        "<details>\n<summary>S</summary>\n\n![x](pattern.png)\n\n</details>\n",
    );
    let images = load(
        &document,
        &fixtures().join("images"),
        DEFAULT_MAX_FILE_SIZE,
        &LoadOptions::default(),
    );
    assert!(
        matches!(images.get(&"pattern.png".into()), Some(Ok(_))),
        "{document:?}"
    );
}

#[test]
fn loads_each_image_shown_as_a_block_once() {
    let source = "\
![a](images/pattern.png)

![again](images/pattern.png)

- ![b](images/missing.png)

> [![c](images/transparent.png)](https://example.com)
";
    let images = load_source(source, &LoadOptions::default());
    assert_eq!(images.len(), 3);
    assert!(images.get(&"images/pattern.png".into()).unwrap().is_ok());
    assert!(
        images
            .get(&"images/transparent.png".into())
            .unwrap()
            .is_ok()
    );
    assert!(matches!(
        images.get(&"images/missing.png".into()),
        Some(Err(ImageError::Missing(_)))
    ));
    assert!(images.get(&"https://example.com".into()).is_none());
}

#[test]
fn does_not_load_images_shown_as_alt_text() {
    let source = "\
# Title ![h](images/icon.png)

Text ![inline](images/pattern.png) and words.

[![with words](images/transparent.png) and words](https://example.com)

![one](images/icon.png) ![two](images/icon.png)

| x |
|---|
| ![d](images/missing.png) |
";
    let images = load_source(source, &LoadOptions::default());
    assert!(images.is_empty(), "{images:?}");
}

/// Draws Mermaid as an empty SVG one line high for each line of
/// source, and fails on a source that says `broken`, and on one that
/// says `flaky` the first time it is asked to draw it. Remembers what
/// it was asked to draw.
#[derive(Default)]
pub(crate) struct Stub {
    pub(super) calls: std::cell::RefCell<Vec<(String, Theme)>>,
}

impl Renderer for Stub {
    fn draws(&self, language: &str) -> bool {
        language.eq_ignore_ascii_case("mermaid")
    }

    fn render(&self, request: &Request) -> Result<Drawn, DiagramError> {
        let &Request { source, theme, .. } = request;
        self.calls.borrow_mut().push((source.to_owned(), theme));
        if source.contains("broken") {
            return Err("parse error".into());
        }
        let asked = self
            .calls
            .borrow()
            .iter()
            .filter(|(drawn, _)| drawn == source)
            .count();
        if source.contains("flaky") && asked == 1 {
            return Err("timed out".into());
        }
        let height = 20 * source.lines().count();
        let svg =
            format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="{height}"/>"#);
        Ok(Drawn::svg(svg, theme, None))
    }
}

pub(crate) fn with_stub(stub: &Stub, theme: Theme, by_line: bool) -> LoadOptions<'_> {
    LoadOptions {
        renderer: stub,
        theme,
        by_line,
        ..LoadOptions::default()
    }
}

#[test]
fn a_reload_draws_only_the_diagrams_it_does_not_have() {
    let stub = Stub::default();
    let options = with_stub(&stub, Theme::Light, true);
    let part = |document| Part {
        document,
        number: 0,
        base: Path::new(""),
        by_line: true,
        inline_images: false,
    };
    let before = crate::typeset::parse::parse("```mermaid\na\n```\n\n```mermaid\nb\n```\n");
    let after = crate::typeset::parse::parse("```mermaid\na\n```\n\n```mermaid\nc\n```\n");
    let mut images = load_parts(
        &[part(&before)],
        DEFAULT_MAX_FILE_SIZE,
        &Fonts::none(),
        &options,
    );
    let calls = |stub: &Stub| -> Vec<String> {
        stub.calls
            .borrow()
            .iter()
            .map(|(source, _)| source.clone())
            .collect()
    };
    assert_eq!(calls(&stub), ["a\n", "b\n"]);
    let parts = [part(&after)];
    let (wanted, have) = plan(&parts, &stub, &Looks::default(), Some(&mut images));
    let keys: Vec<_> = (wanted.iter())
        .map(|(_, image)| image.key(&Looks::default()))
        .collect();
    assert_eq!(
        keys,
        [
            ImageKey::block("mermaid", "a\n"),
            ImageKey::block("mermaid", "c\n")
        ]
    );
    // `a` is taken as it was, `c` is to be drawn, and `b` is left.
    assert!(matches!(have[..], [Some(Ok(_)), None]), "{have:?}");
    let left: Vec<_> = images.iter().map(|(key, _)| key.clone()).collect();
    assert_eq!(left, [ImageKey::block("mermaid", "b\n")]);
}

#[test]
fn a_reload_draws_again_a_diagram_that_failed() {
    let stub = Stub::default();
    let options = with_stub(&stub, Theme::Light, true);
    let document = crate::typeset::parse::parse("```mermaid\nflaky\n```\n");
    let parts = [Part {
        document: &document,
        number: 0,
        base: Path::new(""),
        by_line: true,
        inline_images: false,
    }];
    let key = ImageKey::block("mermaid", "flaky\n");
    let mut images = load_parts(&parts, DEFAULT_MAX_FILE_SIZE, &Fonts::none(), &options);
    assert!(matches!(images.get(&key), Some(Err(_))), "{images:?}");
    let (wanted, have) = plan(&parts, &stub, &Looks::default(), Some(&mut images));
    assert_eq!(wanted.len(), 1);
    assert!(matches!(have[..], [None]), "{have:?}");
}

#[test]
fn draws_each_diagram_once_wherever_a_lone_image_would_show() {
    let source = "\
```mermaid
a
```

- ```mermaid
  b
  ```

> ```Mermaid
> c
> ```

> [!NOTE]
> ```mermaid
> d
> ```

<details>

```mermaid
e
```

</details>

```mermaid
a
```

```rust
fn main() {}
```

| `x` |
|---|
";
    let stub = Stub::default();
    let options = with_stub(&stub, Theme::Dark, true);
    let images = load_source(source, &options);
    let look = options.looks().diagram;
    let calls = stub.calls.into_inner();
    let sources: Vec<&str> = calls.iter().map(|(source, _)| source.as_str()).collect();
    assert_eq!(sources, ["a\n", "b\n", "c\n", "d\n", "e\n"]);
    assert!(calls.iter().all(|(_, theme)| *theme == Theme::Dark));
    assert_eq!(images.len(), 5);
    for key in [
        ImageKey::block("mermaid", "a\n").with_look(look),
        ImageKey::block("Mermaid", "c\n").with_look(look),
    ] {
        match images.get(&key) {
            Some(Ok(Source::Svg(svg))) => assert_eq!(svg.size, (200, 20)),
            other => panic!("{key:?}: {other:?}"),
        }
    }
}

#[test]
fn a_diagram_is_drawn_once_whatever_the_case_of_its_language() {
    let source = "```Mermaid\na\n```\n\n```mermaid\na\n```\n\n```MERMAID\na\n```\n";
    let stub = Stub::default();
    let images = load_source(source, &with_stub(&stub, Theme::Light, true));
    assert_eq!(stub.calls.into_inner().len(), 1);
    assert_eq!(images.len(), 1);
    for language in ["Mermaid", "mermaid", "MERMAID"] {
        let key = ImageKey::block(language, "a\n");
        assert!(matches!(images.get(&key), Some(Ok(_))), "{language}");
    }
}

#[test]
fn tells_how_many_diagrams_are_drawn_of_how_many() {
    let source = "```mermaid\na\n```\n\n```mermaid\nb\n```\n\n```mermaid\na\n```\n";
    let told = std::sync::Mutex::new(Vec::new());
    let tell = |done, total| told.lock().unwrap().push((done, total));
    let stub = Stub::default();
    let options = LoadOptions {
        progress: Some(&tell),
        ..with_stub(&stub, Theme::Light, true)
    };
    let load = |source: &str| load_source(source, &options);
    load(source);
    // The same diagram twice is drawn once.
    assert_eq!(*told.lock().unwrap(), [(0, 2), (1, 2), (2, 2)]);
    told.lock().unwrap().clear();
    load("No diagrams.\n");
    assert_eq!(*told.lock().unwrap(), []);
}

#[test]
fn draws_a_formula_block_in_the_text_colour() {
    let options = LoadOptions {
        renderer: &crate::diagrams::BuiltIn,
        formula: crate::diagrams::FormulaColors {
            text: Some([0x12, 0x34, 0x56]),
            background: None,
        },
        ..LoadOptions::default()
    };
    let images = load_source("Square:\n\n$$\\rule{1em}{1em}$$\n", &options);
    let key = ImageKey::block("math", "\\rule{1em}{1em}\n").with_look(options.looks().diagram);
    let Some(Ok(image)) = images.get(&key) else {
        panic!("{images:?}");
    };
    let image = image.render(8, 8);
    assert_eq!(image.get_pixel(4, 4).0, [0x12, 0x34, 0x56, 255]);
}

#[test]
fn draws_no_diagrams_without_a_renderer() {
    let images = load_source("```mermaid\na\n```\n", &LoadOptions::default());
    assert!(images.is_empty(), "{images:?}");
}

#[test]
fn documents_loaded_together_keep_their_own_images() {
    // Both documents show `fig.png`, each from its own directory, and
    // the same diagram, which fails in both.
    let scratch = crate::testing::Scratch::new("images-two-documents");
    for (dir, width) in [("a", 3), ("b", 5)] {
        std::fs::create_dir(scratch.join(dir)).unwrap();
        let image = RgbaImage::new(width, 1);
        image.save(scratch.join(dir).join("fig.png")).unwrap();
    }
    let first = crate::typeset::parse::parse("![a](fig.png)\n\n```mermaid\nbroken\n```\n");
    let second =
        crate::typeset::parse::parse("Text.\n\n![b](fig.png)\n\n```mermaid\nbroken\n```\n");
    let (a, b) = (scratch.join("a"), scratch.join("b"));
    let parts = [
        Part {
            document: &first,
            number: 0,
            base: &a,
            by_line: true,
            inline_images: false,
        },
        Part {
            document: &second,
            number: 1,
            base: &b,
            by_line: true,
            inline_images: false,
        },
    ];
    let stub = Stub::default();
    let images = load_parts(
        &parts,
        DEFAULT_MAX_FILE_SIZE,
        &Fonts::none(),
        &with_stub(&stub, Theme::Light, true),
    );
    let width = |document| match images.get(&ImageKey::from("fig.png").in_document(document)) {
        Some(Ok(source)) => source.size().0,
        other => panic!("{document}: {other:?}"),
    };
    assert_eq!((width(0), width(1)), (3, 5));
    // Each document is told about its own diagram, by its own line.
    assert_eq!(
        texts(images.warnings_in(0, true)),
        ["mermaid diagram on line 3: parse error"]
    );
    assert_eq!(
        texts(images.warnings_in(1, true)),
        ["mermaid diagram on line 5: parse error"]
    );
}
