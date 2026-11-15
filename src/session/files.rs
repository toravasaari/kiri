//! The files the pager shows one after another as one stream: where each
//! starts in the laid-out lines, which one a line is in, and where `:n`
//! and `:p` go.

use crate::session::sources::Source;
use crate::typeset::locate::Map;

/// The files of a stream, found from the places of the document its lines
/// show. Their names and their number are the session's sources'. A lone
/// document is no file, so it has none.
#[derive(Debug, Clone, Copy)]
pub struct Files<'a> {
    files: &'a [&'a Source],
    map: &'a Map,
    /// How many lines there are.
    lines: usize,
}

impl<'a> Files<'a> {
    /// The sources `files`, laid out as `lines` lines of which `map` is the
    /// map.
    pub fn new(files: &'a [&'a Source], map: &'a Map, lines: usize) -> Self {
        Self { files, map, lines }
    }

    /// The number of the file of the first place that the line `line`
    /// shows, if it shows one. A line of reading mode's page whose rows no
    /// text touches shows none though its block is known, and a row that
    /// a text touches shows one though it is outside every block; `at`
    /// and `start` look upward and forward past them, so they give what
    /// the blocks of the lines would.
    fn of_line(&self, line: usize) -> Option<usize> {
        Some(self.map.first_on(line)?.file)
    }

    /// The file that line `line` is in: the one of the nearest line at or
    /// above it that shows a place. The blank line between two files is
    /// the first one's.
    pub fn at(&self, line: usize) -> Option<usize> {
        if self.files.is_empty() {
            return None;
        }
        let last = line.min(self.lines.checked_sub(1)?);
        (0..=last).rev().find_map(|above| self.of_line(above))
    }

    /// The name of the file numbered `file`, counted from 0.
    pub fn name(&self, file: usize) -> &'a str {
        self.files[file].name()
    }

    /// The line the file numbered `file` starts on, the first of its
    /// header.
    pub fn start(&self, file: usize) -> usize {
        (0..self.lines)
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
    use crate::typeset::locate::Map;
    use crate::typeset::style::Line;

    fn named(names: &[&str]) -> Session {
        Session::new(names.iter().copied())
    }

    /// Lines as the files start on `starts`, in a stream of `count` lines:
    /// each comes from the block of the file it is in, which is all it
    /// shows.
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
                    inner: Some(ids[file]),
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
        let map = Map::new(&lines, &blocks);
        let sources = shown.sources_of(shown.base().id());
        let files = Files::new(&sources, &map, lines.len());
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
            (lines[line].block, lines[line].inner) = (None, None);
        }
        let map = Map::new(&lines, &blocks);
        let sources = shown.sources_of(shown.base().id());
        let files = Files::new(&sources, &map, lines.len());
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
        let map = Map::new(&lines, &blocks);
        let files = Files::new(&[], &map, lines.len());
        assert_eq!(files.at(0), None);
        assert!(!files.several());
    }

    #[test]
    fn lines_with_blocks_are_in_no_file_when_there_are_no_files() {
        let (lines, blocks) = stream(&[0], 5);
        let map = Map::new(&lines, &blocks);
        let files = Files::new(&[], &map, lines.len());
        assert_eq!(files.at(2), None);
    }

    /// Every fixture and example as the first and the third of three files,
    /// with the next one between them, laid out as text and as reading mode.
    fn three_files(mut check: impl FnMut(&str, &[Line], &Blocks)) {
        use crate::render::Inputs;
        use crate::typeset::caps::Capabilities;
        use crate::typeset::driver::{Content, Stream, content};
        use crate::typeset::model::File;
        use crate::typeset::parse::parse;
        use crate::typeset::reading::Fonts;
        use crate::typeset::reading::engine::Reading;
        use crate::typeset::reading::page::Wide;
        let paths = crate::testing::documents();
        let read = |path: &std::path::PathBuf| parse(&std::fs::read_to_string(path).unwrap());
        let documents: Vec<_> = paths.iter().map(read).collect();
        let reading = Inputs {
            reading: Some(Reading {
                pictures: Default::default(),
                fonts: std::rc::Rc::new(Fonts::fixture()),
                wide: Wide::Scroll,
            }),
            ..Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(60)
            })
        };
        for inputs in [Inputs::new(Capabilities::kitty(60)), reading] {
            for (index, document) in documents.iter().enumerate() {
                let next = &documents[(index + 1) % documents.len()];
                let files = [document, next, document].map(|document| File {
                    name: "a.md".to_owned(),
                    document: document.clone(),
                });
                let Stream { lines, blocks } = content(Content::Files(&files), &inputs);
                check(&paths[index].display().to_string(), &lines, &blocks);
            }
        }
    }

    #[test]
    fn the_file_of_a_line_is_that_of_the_nearest_line_above_it_with_a_block() {
        // How the files were found before the map: from the blocks of the
        // lines. A row of a page may show a place and have no block, or
        // the other way round, but the nearest above gives the same file.
        let shown = named(&["a.md", "b.md", "c.md"]);
        let mut checked = 0;
        three_files(|name, lines, blocks| {
            let map = Map::new(lines, blocks);
            let sources = shown.sources_of(shown.base().id());
            let files = Files::new(&sources, &map, lines.len());
            let of_block = |line: usize| Some(blocks.get(lines[line].block?).file());
            let mut above = None;
            for line in 0..lines.len() {
                above = of_block(line).or(above);
                assert_eq!(files.at(line), above, "{name}: line {line}");
            }
            for file in 0..3 {
                let start = (0..lines.len()).find(|&line| of_block(line) == Some(file));
                assert_eq!(Some(files.start(file)), start, "{name}: file {file}");
            }
            checked += lines.len();
        });
        assert!(checked > 10_000, "{checked} lines checked");
    }

    #[test]
    fn colon_n_goes_to_the_next_file_until_the_last() {
        let shown = named(&["a.md", "b.md"]);
        let (lines, blocks) = stream(&[0, 10], 14);
        let map = Map::new(&lines, &blocks);
        let sources = shown.sources_of(shown.base().id());
        let files = Files::new(&sources, &map, lines.len());
        assert!(files.several());
        assert_eq!(files.next(0), Some(1));
        assert_eq!(files.next(1), None);
    }

    #[test]
    fn colon_p_goes_to_the_start_of_the_file_and_then_the_one_before() {
        let shown = named(&["a.md", "b.md"]);
        let (lines, blocks) = stream(&[0, 10], 14);
        let map = Map::new(&lines, &blocks);
        let sources = shown.sources_of(shown.base().id());
        let files = Files::new(&sources, &map, lines.len());
        assert_eq!(files.previous(1, 15), Some(1));
        assert_eq!(files.previous(1, 10), Some(0));
        // Short of the start of the file, as on the last page.
        assert_eq!(files.previous(1, 4), Some(0));
        assert_eq!(files.previous(0, 4), Some(0));
        assert_eq!(files.previous(0, 0), None);
    }
}
