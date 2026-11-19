//! The contents of the shown documents: their headings as a flat list, and
//! the heading the reader is in.
//!
//! The list is made from the named places of [`Targets`], not from the
//! documents again: a heading there has its id and its place, and this adds
//! its level and its text, read from the block at that place. It does not
//! know where it is drawn, nor how a heading is shown.

use crate::typeset::anchors::anchor_text;
use crate::typeset::model::{Block, Document};
use crate::typeset::targets::{NamedKind, Targets};

/// One line of the contents: a heading, or a file when several are shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub file: usize,
    /// The block of the heading; empty for a file.
    pub path: Vec<usize>,
    /// 1 to 6 for a heading, 0 for a file.
    pub level: u8,
    /// What the heading says, on one line; the name of a file.
    pub text: String,
    /// The id of the heading, as a link to `#id` names it; empty for a
    /// file.
    pub id: String,
}

/// The entries of the shown documents, in the order of the documents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contents {
    entries: Vec<Entry>,
}

/// The contents of `files`, each a document and the name its header shows
/// when several files are shown, with `targets` of the same documents.
pub fn contents<'a>(
    files: impl IntoIterator<Item = (Option<&'a str>, &'a Document)>,
    targets: &Targets,
) -> Contents {
    let mut entries = Vec::new();
    // `named` is in the order of the documents, so a file's headings are
    // one run of it.
    let mut named = targets.named.iter().peekable();
    for (file, (name, document)) in files.into_iter().enumerate() {
        if let Some(name) = name {
            entries.push(Entry {
                file,
                path: Vec::new(),
                level: 0,
                text: one_line(name),
                id: String::new(),
            });
        }
        let headings: Vec<_> =
            std::iter::from_fn(|| named.next_if(|named| named.range.start.file == file))
                .filter(|named| named.kind == NamedKind::Heading)
                .collect();
        if headings.is_empty() {
            continue;
        }
        // The blocks are listed once for the file, and only when it has a
        // heading, so a file without one costs nothing here.
        let blocks = document.blocks();
        entries.extend(headings.into_iter().filter_map(|named| {
            let path = &named.range.start.path;
            let at = blocks.binary_search_by(|(other, _)| other.cmp(path)).ok()?;
            let Block::Heading { level, content } = blocks[at].1 else {
                return None;
            };
            Some(Entry {
                file,
                path: path.clone(),
                level: *level,
                text: one_line(&anchor_text(content)),
                id: named.id.clone(),
            })
        }));
    }
    Contents { entries }
}

/// `text` on one line: the runs of white space are one space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Contents {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::model::File;
    use crate::typeset::parse::csv::parse as csv;
    use crate::typeset::parse::{file as text_file, parse};
    use crate::typeset::source_rows::Separator;
    use crate::typeset::targets::targets;

    fn of(source: &str) -> Vec<(u8, String, String)> {
        let document = parse(source);
        let found = targets([&document]);
        let contents = contents([(None, &document)], &found);
        contents
            .entries()
            .iter()
            .map(|e| (e.level, e.text.clone(), e.id.clone()))
            .collect()
    }

    fn row(level: u8, text: &str, id: &str) -> (u8, String, String) {
        (level, text.to_owned(), id.to_owned())
    }

    #[test]
    fn levels_one_to_six() {
        let source = "# a\n## b\n### c\n#### d\n##### e\n###### f\n";
        let levels: Vec<u8> = of(source).iter().map(|e| e.0).collect();
        assert_eq!(levels, [1, 2, 3, 4, 5, 6]);
        assert_eq!(of(source)[2], row(3, "c", "c"));
    }

    #[test]
    fn a_document_without_headings_has_no_entries() {
        assert_eq!(of("just text\n\n- a list\n"), []);
        assert_eq!(of(""), []);
    }

    #[test]
    fn an_image_in_a_heading_is_its_alt_text_and_a_formula_its_source() {
        let found = of("# Logo ![the logo](a.png) here\n\n## Euler $e^{i\\pi}$ says\n");
        assert_eq!(found[0].1, "Logo the logo here");
        assert_eq!(found[1].1, "Euler e^{i\\pi} says");
    }

    #[test]
    fn markup_and_breaks_in_a_heading_are_one_line_of_plain_text() {
        let found = of("# *Em* **strong** `code` [link](u)\n\n<h2>Two<br>lines</h2>\n");
        assert_eq!(found[0].1, "Em strong code link");
        assert_eq!(found[1], row(2, "Two lines", "two-lines"));
    }

    #[test]
    fn an_html_heading_has_its_level() {
        assert_eq!(of("<h2>Html two</h2>\n"), [row(2, "Html two", "html-two")]);
    }

    #[test]
    fn a_heading_inside_details_a_quote_and_a_list_is_listed_with_its_own_level() {
        let source = "<details>\n<summary>S</summary>\n\n### In details\n\n</details>\n\n\
            > ## In quote\n\n- item\n\n  #### In list\n";
        let found = of(source);
        assert_eq!(
            found,
            [
                row(3, "In details", "in-details"),
                row(2, "In quote", "in-quote"),
                row(4, "In list", "in-list"),
            ]
        );
    }

    #[test]
    fn duplicate_headings_have_the_ids_of_the_links() {
        let found = of("# Same\n\n# Same\n");
        assert_eq!(found[1].2, "same-1");
    }

    #[test]
    fn csv_and_plain_text_have_no_entries() {
        for document in [
            csv("a,b\n1,2\n", Separator::Comma).unwrap(),
            text_file("# not a heading\nline\n", None),
        ] {
            let found = targets([&document]);
            assert_eq!(contents([(None, &document)], &found).entries(), []);
        }
    }

    fn files() -> Vec<File> {
        let file = |name: &str, source: &str| File {
            name: name.to_owned(),
            document: parse(source),
        };
        vec![
            file("a.md", "# A one\n\n## A two\n"),
            file("b.md", "text only\n"),
            file("c.md", "# C one\n"),
        ]
    }

    fn of_files(files: &[File]) -> Contents {
        let found = targets(files.iter().map(|f| &f.document));
        contents(
            files.iter().map(|f| (Some(f.name.as_str()), &f.document)),
            &found,
        )
    }

    #[test]
    fn each_file_is_an_entry_above_its_headings() {
        let files = files();
        let contents = of_files(&files);
        let seen: Vec<_> = contents
            .entries()
            .iter()
            .map(|e| (e.file, e.level, e.text.as_str(), e.path.clone()))
            .collect();
        assert_eq!(
            seen,
            [
                (0, 0, "a.md", vec![]),
                (0, 1, "A one", vec![0]),
                (0, 2, "A two", vec![1]),
                (1, 0, "b.md", vec![]),
                (2, 0, "c.md", vec![]),
                (2, 1, "C one", vec![0]),
            ]
        );
    }
}
