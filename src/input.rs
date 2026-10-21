//! What a file given on the command line is: a Markdown document, a CSV
//! or TSV table, an image to show on its own, or any text to show as it
//! is.

use std::ffi::OsStr;
use std::path::Path;

use image::ImageFormat;

use crate::images::Format;
use crate::parse::csv::Separator;

/// How many bytes from the start of a file `detect` looks at.
pub const HEAD: usize = 64;

/// What kiri shows a file as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Markdown,
    /// CSV or TSV, shown as a table.
    Table(Separator),
    Image(Format),
    /// Any text, shown as it is, without reading it as Markdown.
    Text,
}

/// Extensions of Markdown files, which are Markdown whatever they hold.
const MARKDOWN: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn", "mdwn"];

/// Extensions of files of diagram source, and the language each holds.
const DIAGRAMS: &[(&str, &str)] = &[("mmd", "mermaid"), ("mermaid", "mermaid")];

/// What the file at `path`, which starts with `head`, is. A Markdown
/// extension makes it Markdown, a CSV or TSV extension a table and an SVG
/// extension an SVG. Otherwise the content tells a raster image, unless it
/// is text: a Markdown file may well start with `BM`, which also starts a
/// BMP image. Then an image extension tells formats whose content does
/// not, such as TGA. Anything else is Markdown.
pub fn detect(path: &Path, head: &[u8]) -> Input {
    let extension = path.extension().and_then(OsStr::to_str);
    let is =
        |names: &[&str]| extension.is_some_and(|e| names.iter().any(|n| e.eq_ignore_ascii_case(n)));
    if is(MARKDOWN) {
        Input::Markdown
    } else if let Some(table) = extension.and_then(table) {
        table
    } else if is(&["svg", "svgz"]) {
        Input::Image(Format::Svg)
    } else if !looks_like_text(head) && image::guess_format(head).is_ok() {
        Input::Image(Format::Detect)
    } else if let Some(format) = extension.and_then(ImageFormat::from_extension) {
        Input::Image(Format::Raster(format))
    } else {
        Input::Markdown
    }
}

/// The input `name` asks for, as `--format` takes it: `markdown`, `csv`,
/// `tsv`, `text` or `txt` for any text as it is, `image` for an image in
/// whatever format its content tells, or an image format by its usual
/// extension, such as `svg` or `png`. Any case goes.
pub fn format(name: &str) -> Option<Input> {
    let is = |names: &[&str]| names.iter().any(|n| name.eq_ignore_ascii_case(n));
    if is(&["markdown", "md"]) {
        Some(Input::Markdown)
    } else if is(&["text", "txt"]) {
        Some(Input::Text)
    } else if let Some(table) = table(name) {
        Some(table)
    } else if is(&["image"]) {
        Some(Input::Image(Format::Detect))
    } else if is(&["svg", "svgz"]) {
        Some(Input::Image(Format::Svg))
    } else {
        ImageFormat::from_extension(name).map(|format| Input::Image(Format::Raster(format)))
    }
}

/// The language of the file at `path` shown as text, for highlighting:
/// the language its extension names, as code blocks name it. A diagram
/// file's extension names the diagram's language, `mmd` Mermaid; any other
/// is taken as it is, as highlighting knows `rs` or `py`. `None` without an
/// extension.
pub fn language(path: &Path) -> Option<String> {
    let extension = path.extension().and_then(OsStr::to_str)?;
    let diagram = (DIAGRAMS.iter()).find(|(name, _)| extension.eq_ignore_ascii_case(name));
    Some(match diagram {
        Some((_, language)) => (*language).to_owned(),
        None => extension.to_ascii_lowercase(),
    })
}

/// The table `name`, an extension or a `--format` value, names: `csv` is
/// comma-separated, `tsv` and `tab` tab-separated.
fn table(name: &str) -> Option<Input> {
    if name.eq_ignore_ascii_case("csv") {
        Some(Input::Table(Separator::Comma))
    } else if name.eq_ignore_ascii_case("tsv") || name.eq_ignore_ascii_case("tab") {
        Some(Input::Table(Separator::Tab))
    } else {
        None
    }
}

/// Whether `head`, the start of a file, is text: UTF-8, perhaps cut inside
/// a character at the end, without control characters other than
/// whitespace.
fn looks_like_text(head: &[u8]) -> bool {
    let text = match std::str::from_utf8(head) {
        Ok(text) => text,
        // Cut inside a character at the end.
        Err(error) if error.error_len().is_none() => {
            std::str::from_utf8(&head[..error.valid_up_to()]).expect("valid up to here")
        }
        Err(_) => return false,
    };
    !text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\x0c'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    fn detected(name: &str, head: &[u8]) -> Input {
        detect(Path::new(name), head)
    }

    #[test]
    fn format_names_markdown_or_an_image_format() {
        assert_eq!(format("markdown"), Some(Input::Markdown));
        assert_eq!(format("Markdown"), Some(Input::Markdown));
        assert_eq!(format("md"), Some(Input::Markdown));
        assert_eq!(format("image"), Some(Input::Image(Format::Detect)));
        assert_eq!(format("svg"), Some(Input::Image(Format::Svg)));
        assert_eq!(format("SVGZ"), Some(Input::Image(Format::Svg)));
        assert_eq!(
            format("jpeg"),
            Some(Input::Image(Format::Raster(ImageFormat::Jpeg)))
        );
        assert_eq!(
            format("tga"),
            Some(Input::Image(Format::Raster(ImageFormat::Tga)))
        );
        assert_eq!(format("csv"), Some(Input::Table(Separator::Comma)));
        assert_eq!(format("TSV"), Some(Input::Table(Separator::Tab)));
        assert_eq!(format("tab"), Some(Input::Table(Separator::Tab)));
        assert_eq!(format("text"), Some(Input::Text));
        assert_eq!(format("TXT"), Some(Input::Text));
        assert_eq!(format("docx"), None);
        assert_eq!(format(""), None);
    }

    #[test]
    fn the_extension_names_the_language_of_text() {
        assert_eq!(language(Path::new("a.rs")).as_deref(), Some("rs"));
        assert_eq!(language(Path::new("a.MD")).as_deref(), Some("md"));
        assert_eq!(language(Path::new("a.mmd")).as_deref(), Some("mermaid"));
        assert_eq!(language(Path::new("a.Mermaid")).as_deref(), Some("mermaid"));
        assert_eq!(language(Path::new("Makefile")), None);
        assert_eq!(language(Path::new("<stdin>")), None);
    }

    #[test]
    fn a_markdown_extension_is_always_markdown() {
        for name in [
            "a.md",
            "a.MD",
            "a.markdown",
            "a.mdown",
            "a.mkd",
            "a.mkdn",
            "a.mdwn",
        ] {
            assert_eq!(detected(name, PNG), Input::Markdown, "{name}");
        }
    }

    #[test]
    fn a_table_is_told_by_its_extension() {
        for (name, separator) in [
            ("a.csv", Separator::Comma),
            ("a.CSV", Separator::Comma),
            ("a.tsv", Separator::Tab),
            ("a.Tab", Separator::Tab),
        ] {
            assert_eq!(detected(name, b"BM,x\n"), Input::Table(separator), "{name}");
            // Whatever the content looks like.
            assert_eq!(detected(name, PNG), Input::Table(separator), "{name}");
        }
        assert_eq!(detected("a.txt", b"a,b\n1,2\n"), Input::Markdown);
    }

    #[test]
    fn an_svg_is_told_by_its_extension() {
        assert_eq!(detected("a.svg", b"<svg"), Input::Image(Format::Svg));
        assert_eq!(detected("a.SVGZ", b"\x1f\x8b"), Input::Image(Format::Svg));
        // Markdown may start with an SVG drawing too.
        assert_eq!(detected("a.txt", b"<svg>"), Input::Markdown);
    }

    #[test]
    fn a_binary_image_is_told_by_its_content() {
        for name in ["a", "a.txt", "a.jpg"] {
            assert_eq!(detected(name, PNG), Input::Image(Format::Detect), "{name}");
        }
        let gif = b"GIF89a\x40\0\x40\0\xf7\0\0";
        assert_eq!(detected("a", gif), Input::Image(Format::Detect));
    }

    #[test]
    fn text_that_starts_like_an_image_is_markdown() {
        for head in ["BMW and Audi\n", "P1 is first\n", "P6\tsix\r\n"] {
            assert_eq!(detected("a", head.as_bytes()), Input::Markdown, "{head:?}");
        }
    }

    #[test]
    fn an_image_extension_tells_what_the_content_does_not() {
        // TGA has no magic bytes, and PNM may be text.
        assert_eq!(
            detected("a.tga", b"\0\0\x02\0"),
            Input::Image(Format::Raster(ImageFormat::Tga))
        );
        assert_eq!(
            detected("a.PPM", b"P3\n2 2\n255\n"),
            Input::Image(Format::Raster(ImageFormat::Pnm))
        );
        assert_eq!(
            detected("not-an-image.png", b"This is text"),
            Input::Image(Format::Raster(ImageFormat::Png))
        );
    }

    #[test]
    fn anything_else_is_markdown() {
        assert_eq!(detected("a.txt", b"# Title\n"), Input::Markdown);
        assert_eq!(detected("a", b""), Input::Markdown);
        assert_eq!(detected("a.bin", b"\0\x01\x02"), Input::Markdown);
    }

    #[test]
    fn text_may_be_cut_inside_a_character() {
        assert_eq!(
            detected("a", "BMW ä".as_bytes()[..5].as_ref()),
            Input::Markdown
        );
        assert!(looks_like_text("BMW ä".as_bytes()[..5].as_ref()));
        assert!(!looks_like_text(b"BM\xff\xfe"));
        assert!(!looks_like_text(b"BM\0\0"));
    }
}
