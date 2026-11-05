//! What a file given on the command line is: a Markdown document, a CSV
//! or TSV table, an image or a diagram to show on its own, or any other
//! text to show as it is.

use std::ffi::OsStr;
use std::path::Path;

use image::ImageFormat;

use crate::typeset::images::Format;
use crate::typeset::layout::highlight;
use crate::typeset::parse::csv::Separator;

/// How many bytes from the start of a file `detect` looks at.
pub const HEAD: usize = 64;

/// What kiri shows a file as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Markdown,
    /// CSV or TSV, shown as a table.
    Table(Separator),
    Image(Format),
    /// The source of a diagram in this language, such as `mermaid`, drawn
    /// on its own.
    Diagram(&'static str),
    /// Text that is not Markdown, shown as it is: highlighted when kiri
    /// knows its language, as `language` tells.
    Text,
}

/// Extensions of Markdown files, which are Markdown whatever they hold.
const MARKDOWN: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn", "mdwn"];

/// What the file at `path`, which starts with `head`, is. Only a Markdown
/// extension makes it Markdown, a CSV or TSV extension a table, a diagram
/// extension such as `mmd` a diagram and an SVG extension an SVG.
/// Otherwise the content tells a raster image, unless it is text: a
/// Markdown file may well start with `BM`, which also starts a BMP image.
/// Then an image extension tells formats whose content does not, such as
/// TGA. Any other file that looks like text is text, shown as it is. What
/// is left, a file that is not text, is Markdown, for the caller to find
/// that it cannot be read.
///
/// The path is the file's name, which standard input does not have: the
/// caller treats it, and a directory's `README`, as Markdown when this says
/// text.
pub fn detect(path: &Path, head: &[u8]) -> Input {
    let extension = path.extension().and_then(OsStr::to_str);
    let is =
        |names: &[&str]| extension.is_some_and(|e| names.iter().any(|n| e.eq_ignore_ascii_case(n)));
    if is(MARKDOWN) {
        Input::Markdown
    } else if let Some(table) = extension.and_then(table) {
        table
    } else if let Some(language) = extension.and_then(diagram_file) {
        Input::Diagram(language)
    } else if is(&["svg", "svgz"]) {
        Input::Image(Format::Svg)
    } else if !looks_like_text(head) && image::guess_format(head).is_ok() {
        Input::Image(Format::Detect)
    } else if let Some(format) = extension.and_then(ImageFormat::from_extension) {
        Input::Image(Format::Raster(format))
    } else if looks_like_text(head) {
        Input::Text
    } else {
        Input::Markdown
    }
}

/// The input `name` asks for, as `--format` takes it: `markdown`, `csv`,
/// `tsv`, a diagram language such as `mermaid` or its extension, `mmd`,
/// `text` or `txt` for any text as it is, `image` for an image in
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
    } else if let Some(language) = diagram_file(name).or_else(|| diagram_language(name)) {
        Some(Input::Diagram(language))
    } else if is(&["image"]) {
        Some(Input::Image(Format::Detect))
    } else if is(&["svg", "svgz"]) {
        Some(Input::Image(Format::Svg))
    } else {
        ImageFormat::from_extension(name).map(|format| Input::Image(Format::Raster(format)))
    }
}

/// The language of the file at `path`, whose content is `source`, shown as
/// text, for highlighting. A diagram file's extension names the diagram's
/// language, `mmd` Mermaid. Otherwise it is what the highlighter knows the
/// file's name or extension as, such as `Makefile`, `.gitignore` or `rs`,
/// and failing that what the content tells (`sniff`). `None` shows the
/// file without highlighting.
pub fn language(path: &Path, source: &str) -> Option<String> {
    let extension = path.extension().and_then(OsStr::to_str);
    if let Some(language) = extension.and_then(diagram_file) {
        return Some(language.to_owned());
    }
    let name = path.file_name().and_then(OsStr::to_str);
    name.and_then(highlight::file_language)
        .or_else(|| sniff(source).map(str::to_owned))
}

/// The language that the content `source` of a file tells, when it is
/// one that Markdown could not be mistaken for; `None` otherwise. For
/// standard input, which has no name, and for a file whose name tells
/// nothing, such as `LICENSE` or a script `deploy`. Not guessed are YAML
/// and TOML: `---` starts front matter and a thematic break, and `key:
/// value` or `[section]` lines are ordinary Markdown text. A new format
/// is a new case here.
pub fn sniff(source: &str) -> Option<&'static str> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let trimmed = source.trim_start();
    if is_json(trimmed) || is_json_lines(trimmed) {
        return Some("json");
    }
    if is_diff(source) {
        return Some("diff");
    }
    let first = trimmed.lines().next()?;
    let lower = first.to_ascii_lowercase();
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        // The highlighter's first line patterns may take it for Svelte.
        Some("html")
    } else if source.starts_with("#!") || first.starts_with("<?") {
        highlight::first_line_language(first)
    } else {
        None
    }
}

/// Whether `text`, which starts with a non-blank character, is one JSON
/// object or array.
fn is_json(text: &str) -> bool {
    text.starts_with(['{', '[']) && serde_json::from_str::<serde::de::IgnoredAny>(text).is_ok()
}

/// Whether `text` is JSON Lines: at least two lines, each an object or an
/// array.
fn is_json_lines(text: &str) -> bool {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let mut count = 0;
    let all = lines.all(|line| {
        count += 1;
        line.trim_start().starts_with(['{', '['])
            && serde_json::from_str::<serde::de::IgnoredAny>(line).is_ok()
    });
    all && count >= 2
}

/// Whether `source` is a unified diff, as `git diff` or `diff -u` writes
/// it: it starts with `diff --git `, `diff -`, or a `--- ` line followed
/// by a `+++ ` line.
fn is_diff(source: &str) -> bool {
    if source.starts_with("diff --git ") || source.starts_with("diff -") {
        return true;
    }
    let mut lines = source.lines();
    matches!(
        (lines.next(), lines.next()),
        (Some(old), Some(new)) if old.starts_with("--- ") && new.starts_with("+++ ")
    )
}

/// The diagram language whose files have the extension `name`, which is
/// also how `--format` names them.
fn diagram_file(name: &str) -> Option<&'static str> {
    (crate::typeset::diagrams::language::LANGUAGES.iter())
        .find(|language| (language.file_extensions.iter()).any(|e| name.eq_ignore_ascii_case(e)))
        .map(|language| language.name)
}

/// The diagram language `name` names, as a code block does, for
/// `--format`.
fn diagram_language(name: &str) -> Option<&'static str> {
    crate::typeset::diagrams::language::language(name).map(|language| language.name)
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
        assert_eq!(format("mermaid"), Some(Input::Diagram("mermaid")));
        assert_eq!(format("MMD"), Some(Input::Diagram("mermaid")));
        assert_eq!(format("text"), Some(Input::Text));
        assert_eq!(format("TXT"), Some(Input::Text));
        assert_eq!(format("docx"), None);
        assert_eq!(format(""), None);
    }

    fn language_of(name: &str, source: &str) -> Option<String> {
        language(Path::new(name), source)
    }

    #[test]
    fn the_extension_names_the_language_of_text() {
        assert_eq!(language_of("a.rs", "").as_deref(), Some("rs"));
        assert_eq!(language_of("a.MD", "").as_deref(), Some("md"));
        assert_eq!(language_of("a.mmd", "").as_deref(), Some("mermaid"));
        assert_eq!(language_of("a.Mermaid", "").as_deref(), Some("mermaid"));
        assert_eq!(language_of("a.yaml", "").as_deref(), Some("yaml"));
        assert_eq!(language_of("a.YML", "").as_deref(), Some("yml"));
        assert_eq!(language_of("a.html", "").as_deref(), Some("html"));
    }

    #[test]
    fn the_name_names_the_language_of_text_without_an_extension() {
        // Whole file names that two-face's grammars list.
        for name in [
            "Makefile",
            "GNUmakefile",
            "Dockerfile",
            ".gitignore",
            ".gitattributes",
            ".bashrc",
            ".zshrc",
            ".editorconfig",
            ".env",
            "Gemfile",
            "Rakefile",
            "Vagrantfile",
            "Jenkinsfile",
            "PKGBUILD",
            "Cargo.lock",
            "CMakeLists.txt",
            ".env.local",
            "dir/Makefile",
        ] {
            assert!(language_of(name, "").is_some(), "{name}");
        }
        assert_eq!(language_of("Makefile", "").as_deref(), Some("makefile"));
        assert_eq!(language_of(".gitignore", "").as_deref(), Some(".gitignore"));
    }

    #[test]
    fn text_without_a_known_language_has_none() {
        for name in ["a.txt", "a.xyz", "LICENSE", "README", "NOTICE", "deploy"] {
            assert_eq!(language_of(name, "just words\n"), None, "{name}");
        }
        assert_eq!(language_of("<stdin>", "# Title\n"), None);
    }

    #[test]
    fn the_content_tells_the_language_a_name_does_not() {
        assert_eq!(
            language_of("deploy", "{\"a\": 1}\n").as_deref(),
            Some("json")
        );
        assert_eq!(language_of("<stdin>", "[1, 2]\n").as_deref(), Some("json"));
        // The name wins.
        assert_eq!(
            language_of("a.yaml", "{\"a\": 1}\n").as_deref(),
            Some("yaml")
        );
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
        assert_eq!(detected("a.txt", b"a,b\n1,2\n"), Input::Text);
    }

    #[test]
    fn a_diagram_is_told_by_its_extension() {
        for name in ["a.mmd", "a.MMD", "a.mermaid"] {
            assert_eq!(detected(name, b"flowchart TD\n"), Input::Diagram("mermaid"));
            // Whatever the content looks like.
            assert_eq!(detected(name, PNG), Input::Diagram("mermaid"), "{name}");
        }
        assert_eq!(detected("a.txt", b"flowchart TD\n"), Input::Text);
    }

    #[test]
    fn an_svg_is_told_by_its_extension() {
        assert_eq!(detected("a.svg", b"<svg"), Input::Image(Format::Svg));
        assert_eq!(detected("a.SVGZ", b"\x1f\x8b"), Input::Image(Format::Svg));
        // A text file may start with an SVG drawing too.
        assert_eq!(detected("a.txt", b"<svg>"), Input::Text);
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
    fn text_that_starts_like_an_image_is_text() {
        for head in ["BMW and Audi\n", "P1 is first\n", "P6\tsix\r\n"] {
            assert_eq!(detected("a", head.as_bytes()), Input::Text, "{head:?}");
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
    fn only_a_markdown_extension_makes_text_markdown() {
        // However much it looks like Markdown.
        for name in ["a.txt", "a.yaml", "a.yml", "a.json", "a.toml", "a.rs"] {
            assert_eq!(detected(name, b"# Title\n"), Input::Text, "{name}");
        }
        for name in ["a", "LICENSE", "README", "Makefile", ".gitignore", "a.xyz"] {
            assert_eq!(detected(name, b"# Title\n"), Input::Text, "{name}");
        }
        assert_eq!(detected("a", b""), Input::Text);
        assert_eq!(detected("a.md", b"key: value\n"), Input::Markdown);
    }

    #[test]
    fn a_file_that_is_not_text_is_not_text() {
        // For the caller to find that it cannot be read.
        for name in ["a.bin", "a.ts", "a.rs", "Makefile", "a"] {
            assert_eq!(detected(name, b"\0\x01\x02"), Input::Markdown, "{name}");
            assert_eq!(detected(name, b"\xff\xfe\xfd"), Input::Markdown, "{name}");
        }
    }

    #[test]
    fn text_may_be_cut_inside_a_character() {
        assert_eq!(detected("a", "BMW ä".as_bytes()[..5].as_ref()), Input::Text);
        assert!(looks_like_text("BMW ä".as_bytes()[..5].as_ref()));
        assert!(!looks_like_text(b"BM\xff\xfe"));
        assert!(!looks_like_text(b"BM\0\0"));
    }

    #[test]
    fn sniff_knows_json_json_lines_and_diffs() {
        assert_eq!(sniff("{\"a\": [1, 2]}\n"), Some("json"));
        assert_eq!(sniff("\n  [1, {\"a\": null}]\n"), Some("json"));
        assert_eq!(sniff("{\n  \"a\": 1\n}\n"), Some("json"));
        assert_eq!(sniff("{\"a\": 1}\n{\"a\": 2}\n\n[3]\n"), Some("json"));
        assert_eq!(sniff("diff --git a/x b/x\nindex 1..2\n"), Some("diff"));
        assert_eq!(sniff("diff -u a b\n"), Some("diff"));
        assert_eq!(sniff("--- a/x\n+++ b/x\n@@ -1 +1 @@\n"), Some("diff"));
    }

    #[test]
    fn sniff_knows_scripts_and_markup_by_their_first_line() {
        assert!(sniff("#!/bin/sh\necho hi\n").is_some());
        assert!(sniff("#!/usr/bin/env python3\nprint(1)\n").is_some());
        assert!(sniff("<?xml version=\"1.0\"?>\n<a/>\n").is_some());
        assert!(sniff("<?php echo 1;\n").is_some());
        assert_eq!(sniff("<!DOCTYPE html>\n<p>hi</p>\n"), Some("html"));
        assert_eq!(sniff("<!doctype HTML>\n"), Some("html"));
        assert_eq!(sniff("<html lang=\"en\">\n"), Some("html"));
    }

    #[test]
    fn sniff_leaves_markdown_alone() {
        for source in [
            "",
            "# Title\n\nText.\n",
            "---\ntitle: Notes\n---\n# Title\n",
            "---\n",
            "+++\ntitle = \"x\"\n+++\n",
            "<div align=\"center\">\n  <img src=\"a.png\">\n</div>\n",
            "<p>hi</p>\n",
            "[link]: https://example.com\n",
            "[x] done\n[ ] not yet\n",
            "[a](b) is a link\n",
            "{curly} braces\n",
            "{\"a\": 1} and then text\n",
            "key: value\nother: 2\n",
            "[section]\nkey = 1\n",
            "- item\n",
            "#hashtag, not a shebang\n",
            "From here on\n",
            "[1, 2]\ntext\n",
        ] {
            assert_eq!(sniff(source), None, "{source:?}");
        }
    }
}
