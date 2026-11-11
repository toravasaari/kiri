//! The files the pager shows one after another as one stream: where each
//! starts in the laid-out lines, which one a line is in, and where `:n`
//! and `:p` go.

use crate::session::sources::Source;
use crate::typeset::block::Blocks;
use crate::typeset::style::Line;

/// The files of a stream, found from the blocks their lines come from.
/// Their names and their number are the session's sources'. A lone document
/// is no file, so it has none.
#[derive(Debug, Clone, Copy)]
pub struct Files<'a> {
    files: &'a [Source],
    lines: &'a [Line],
    blocks: &'a Blocks,
}

impl<'a> Files<'a> {
    /// The sources `files`, laid out as `lines`, which come from `blocks`.
    pub fn new(files: &'a [Source], lines: &'a [Line], blocks: &'a Blocks) -> Self {
        Self {
            files,
            lines,
            blocks,
        }
    }

    /// The number of the file that the line `line` comes from, if it comes
    /// from a block.
    fn of_line(&self, line: usize) -> Option<usize> {
        let id = self.lines.get(line)?.block?;
        Some(self.blocks.get(id).file())
    }

    /// The file that line `line` is in: the one of the nearest line at or
    /// above it that comes from a block. The blank line between two files
    /// is the first one's.
    pub fn at(&self, line: usize) -> Option<usize> {
        if self.files.is_empty() {
            return None;
        }
        let last = line.min(self.lines.len().checked_sub(1)?);
        (0..=last).rev().find_map(|above| self.of_line(above))
    }

    /// The name of the file numbered `file`, counted from 0.
    pub fn name(&self, file: usize) -> &'a str {
        self.files[file].name()
    }

    /// The line the file numbered `file` starts on, the first of its
    /// header.
    pub fn start(&self, file: usize) -> usize {
        (0..self.lines.len())
            .find(|&line| self.of_line(line) == Some(file))
            .expect("every file has a header")
    }

    /// How many files there are.
    pub fn count(&self) -> usize {
        self.files.len()
    }

    /// Whether there is more than one file.
    pub fn several(&self) -> bool {
        self.count() > 1
    }

    /// The file `:n` goes to from the file `file`: the one after it, if
    /// any.
    pub fn next(&self, file: usize) -> Option<usize> {
        Some(file + 1).filter(|&next| next < self.count())
    }

    /// The file `:p` goes to from the file `file`, with the top of the view
    /// on the line `top`: `file` itself when its start is above the view,
    /// or else the one before it, if any.
    pub fn previous(&self, file: usize, top: usize) -> Option<usize> {
        match top > self.start(file) {
            true => Some(file),
            false => file.checked_sub(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::sources::Session;
    use crate::typeset::block::Blocks;
    use crate::typeset::style::Line;

    fn named(names: &[&str]) -> Session {
        Session::new(names.iter().copied())
    }

    /// Lines as the files start on `starts`, in a stream of `count` lines:
    /// each comes from the block of the file it is in.
    fn stream(starts: &[usize], count: usize) -> (Vec<Line>, Blocks) {
        let mut blocks = Blocks::default();
        let ids: Vec<_> = (0..starts.len())
            .map(|file| blocks.add(file, Vec::new()))
            .collect();
        let lines = (0..count)
            .map(|line| {
                let file = starts.iter().rposition(|&start| start <= line).unwrap();
                Line {
                    block: Some(ids[file]),
                    ..Line::blank()
                }
            })
            .collect();
        (lines, blocks)
    }

    #[test]
    fn a_line_is_in_the_last_file_that_starts_above_it() {
        let shown = named(&["a.md", "b.md", "c.md"]);
        let (lines, blocks) = stream(&[0, 10, 12], 14);
        let files = Files::new(shown.sources(), &lines, &blocks);
        let at = |line| files.at(line).map(|file| files.name(file));
        assert_eq!(at(0), Some("a.md"));
        assert_eq!(at(9), Some("a.md"));
        assert_eq!(at(10), Some("b.md"));
        assert_eq!(at(11), Some("b.md"));
        assert_eq!(at(12), Some("c.md"));
        assert_eq!(at(100), Some("c.md"));
    }

    #[test]
    fn a_line_without_a_block_is_in_the_file_above_it() {
        let shown = named(&["a.md", "b.md"]);
        let (mut lines, blocks) = stream(&[0, 6], 12);
        // A blank line in the middle of each file, the one between the
        // files, and one under the second header.
        for line in [3, 5, 7, 9] {
            lines[line].block = None;
        }
        let files = Files::new(shown.sources(), &lines, &blocks);
        assert_eq!(files.at(3), Some(0));
        assert_eq!(files.at(5), Some(0));
        assert_eq!(files.at(7), Some(1));
        assert_eq!(files.at(9), Some(1));
        assert_eq!(files.at(11), Some(1));
        assert_eq!(files.start(1), 6);
    }

    #[test]
    fn a_lone_document_is_in_no_file() {
        let (lines, blocks) = stream(&[], 0);
        let files = Files::new(&[], &lines, &blocks);
        assert_eq!(files.at(0), None);
        assert!(!files.several());
    }

    #[test]
    fn lines_with_blocks_are_in_no_file_when_there_are_no_files() {
        let (lines, blocks) = stream(&[0], 5);
        let files = Files::new(&[], &lines, &blocks);
        assert_eq!(files.at(2), None);
    }

    #[test]
    fn colon_n_goes_to_the_next_file_until_the_last() {
        let shown = named(&["a.md", "b.md"]);
        let (lines, blocks) = stream(&[0, 10], 14);
        let files = Files::new(shown.sources(), &lines, &blocks);
        assert!(files.several());
        assert_eq!(files.next(0), Some(1));
        assert_eq!(files.next(1), None);
    }

    #[test]
    fn colon_p_goes_to_the_start_of_the_file_and_then_the_one_before() {
        let shown = named(&["a.md", "b.md"]);
        let (lines, blocks) = stream(&[0, 10], 14);
        let files = Files::new(shown.sources(), &lines, &blocks);
        assert_eq!(files.previous(1, 15), Some(1));
        assert_eq!(files.previous(1, 10), Some(0));
        // Short of the start of the file, as on the last page.
        assert_eq!(files.previous(1, 4), Some(0));
        assert_eq!(files.previous(0, 4), Some(0));
        assert_eq!(files.previous(0, 0), None);
    }
}
