//! The files the pager shows one after another as one stream: where each
//! starts in the laid-out lines, and which one a line is in.

use crate::model::File;

/// The files of a stream, each with the line it starts on, the first of
/// its header. A lone document is no file, so it has none.
#[derive(Debug, Clone, Copy)]
pub struct Files<'a> {
    files: &'a [File],
    starts: &'a [usize],
}

impl<'a> Files<'a> {
    /// `files`, which start on the lines `starts`, in order.
    pub fn new(files: &'a [File], starts: &'a [usize]) -> Self {
        Self { files, starts }
    }

    /// The file that line `line` is in: the last that starts on it or
    /// above it. The blank line between two files is the first one's.
    pub fn at(&self, line: usize) -> Option<usize> {
        self.starts.iter().rposition(|&start| start <= line)
    }

    /// The name of the file numbered `file`, counted from 0.
    pub fn name(&self, file: usize) -> &'a str {
        &self.files[file].name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Document;

    fn named(names: &[&str]) -> Vec<File> {
        names
            .iter()
            .map(|name| File {
                name: (*name).to_owned(),
                document: Document { blocks: Vec::new() },
            })
            .collect()
    }

    #[test]
    fn a_line_is_in_the_last_file_that_starts_above_it() {
        let shown = named(&["a.md", "b.md", "c.md"]);
        let files = Files::new(&shown, &[0, 10, 12]);
        let at = |line| files.at(line).map(|file| files.name(file));
        assert_eq!(at(0), Some("a.md"));
        assert_eq!(at(9), Some("a.md"));
        assert_eq!(at(10), Some("b.md"));
        assert_eq!(at(11), Some("b.md"));
        assert_eq!(at(12), Some("c.md"));
        assert_eq!(at(100), Some("c.md"));
    }

    #[test]
    fn a_lone_document_is_in_no_file() {
        assert_eq!(Files::new(&[], &[]).at(0), None);
    }
}
