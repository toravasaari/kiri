//! The files the pager shows one after another as one stream: where each
//! starts in the laid-out lines, which one a line is in, and where `:n`
//! and `:p` go.

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

    /// The line the file numbered `file` starts on.
    pub fn start(&self, file: usize) -> usize {
        self.starts[file]
    }

    /// How many files there are.
    pub fn count(&self) -> usize {
        self.starts.len()
    }

    /// Whether there is more than one file.
    pub fn several(&self) -> bool {
        self.count() > 1
    }

    /// The file `:n` goes to from the file `file`: the one after it, if
    /// any.
    pub fn next(&self, file: usize) -> Option<usize> {
        Some(file + 1).filter(|&next| next < self.starts.len())
    }

    /// The file `:p` goes to from the file `file`, with the top of the view
    /// on the line `top`: `file` itself when its start is above the view,
    /// or else the one before it, if any.
    pub fn previous(&self, file: usize, top: usize) -> Option<usize> {
        match top > self.starts[file] {
            true => Some(file),
            false => file.checked_sub(1),
        }
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
        let files = Files::new(&[], &[]);
        assert_eq!(files.at(0), None);
        assert!(!files.several());
    }

    #[test]
    fn colon_n_goes_to_the_next_file_until_the_last() {
        let shown = named(&["a.md", "b.md"]);
        let files = Files::new(&shown, &[0, 10]);
        assert!(files.several());
        assert_eq!(files.next(0), Some(1));
        assert_eq!(files.next(1), None);
    }

    #[test]
    fn colon_p_goes_to_the_start_of_the_file_and_then_the_one_before() {
        let shown = named(&["a.md", "b.md"]);
        let files = Files::new(&shown, &[0, 10]);
        assert_eq!(files.previous(1, 15), Some(1));
        assert_eq!(files.previous(1, 10), Some(0));
        // Short of the start of the file, as on the last page.
        assert_eq!(files.previous(1, 4), Some(0));
        assert_eq!(files.previous(0, 4), Some(0));
        assert_eq!(files.previous(0, 0), None);
    }
}
