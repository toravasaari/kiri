//! Where a large block is cut: its lines in stretches, each a range of
//! whole lines that the layout lays out on its own.
//!
//! A text is not cut by a count of bytes but by where its lines start, which
//! an index keeps. The index is made by a scanner that can carry on where it
//! stopped, so that a text that grows only adds to it, and that stops once it
//! knows what is asked of it, so that a block that is not cut costs one look
//! at its first lines.

use crate::typeset::model::Block;

/// The number of source lines in a stretch. A block of this many lines or
/// fewer is not cut.
pub const LINES: usize = 128;

/// Where the lines of a text start.
#[derive(Debug, Default, Clone)]
pub(crate) struct LineIndex {
    /// The start of each line found so far.
    starts: Vec<usize>,
    /// The start of the line the scan is in.
    line: usize,
    /// How far the text has been searched for a line break.
    scanned: usize,
}

impl LineIndex {
    /// An index of the lines of a text from the byte `from` on, which is the
    /// start of a line. It numbers the lines from there.
    fn starting_at(from: usize) -> Self {
        Self {
            starts: Vec::new(),
            line: from,
            scanned: from,
        }
    }

    /// Goes on in `text`, which is what it was given before and what may have
    /// come after it, until it knows the start of `lines` lines or has
    /// reached the end. A line is there when something is in it, as
    /// `split_inclusive('\n')` has it: nothing follows a last line break.
    pub(crate) fn scan(&mut self, text: &str, lines: usize) {
        let bytes = text.as_bytes();
        while self.line < bytes.len() && self.starts.len() < lines {
            if self.starts.last() != Some(&self.line) {
                self.starts.push(self.line);
            }
            let from = self.scanned.max(self.line);
            match bytes[from..].iter().position(|&byte| byte == b'\n') {
                Some(at) => (self.line, self.scanned) = (from + at + 1, from + at + 1),
                None => {
                    self.scanned = bytes.len();
                    break;
                }
            }
        }
    }

    /// The start of line number `line`, if the scan has found it.
    pub(crate) fn start(&self, line: usize) -> Option<usize> {
        self.starts.get(line).copied()
    }

    /// The number of lines found.
    pub(crate) fn len(&self) -> usize {
        self.starts.len()
    }
}

/// A part of a code block: whole lines of it, which go on from the stretch
/// before. Together the stretches are the block, as one stretch of all of its
/// lines is.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stretch<'d> {
    /// The block it is of, whole.
    pub block: &'d Block,
    /// Its lines, with their line breaks.
    pub code: &'d str,
    /// The bytes of the block before it.
    pub offset: usize,
    pub first: bool,
    pub last: bool,
}

/// Where a stretch starts in its block: its line number, and the bytes of
/// the block before it. A block in stretches is laid out from a stretch on,
/// which a layout that is saved and goes on does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cut {
    pub line: usize,
    pub byte: usize,
}

/// The stretches of a block's code, one after another.
pub(crate) struct Stretches<'d> {
    block: &'d Block,
    code: &'d str,
    /// The lines from where the stretches start, counted from there.
    index: LineIndex,
    size: usize,
    /// Where the next stretch starts, when there is one.
    next: Option<Cut>,
    /// The line the first stretch started with.
    from: usize,
}

impl<'d> Stretches<'d> {
    /// The stretches of `code`, which is that of `block`, `size` lines each;
    /// `None` when it has no more than `size` lines and is not cut.
    pub(crate) fn of(block: &'d Block, code: &'d str, size: usize) -> Option<Self> {
        let mut index = LineIndex::default();
        index.scan(code, size.saturating_add(1));
        (index.len() > size).then_some(Self {
            block,
            code,
            index,
            size,
            next: Some(Cut { line: 0, byte: 0 }),
            from: 0,
        })
    }

    /// The stretches of a block that is cut, from the one that starts at
    /// `at` on, which an earlier walk of its stretches gave.
    pub(crate) fn resume(block: &'d Block, code: &'d str, size: usize, at: Cut) -> Self {
        Self {
            block,
            code,
            index: LineIndex::starting_at(at.byte),
            size,
            next: Some(at),
            from: at.line,
        }
    }

    /// Where the next stretch starts; `None` once the last was given.
    pub(crate) fn at(&self) -> Option<Cut> {
        self.next
    }
}

impl<'d> Iterator for Stretches<'d> {
    type Item = Stretch<'d>;

    fn next(&mut self) -> Option<Stretch<'d>> {
        let at = self.next?;
        // The start of the line after the stretch tells whether one follows.
        let (from, until) = (at.line - self.from, at.line - self.from + self.size);
        self.index.scan(self.code, until.saturating_add(1));
        let start = self.index.start(from)?;
        let end = match self.index.start(until) {
            Some(end) => {
                let line = at.line.saturating_add(self.size);
                self.next = Some(Cut { line, byte: end });
                end
            }
            None => {
                self.next = None;
                self.code.len()
            }
        };
        Some(Stretch {
            block: self.block,
            code: &self.code[start..end],
            offset: start,
            first: at.line == 0,
            last: self.next.is_none(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(code: &str) -> Block {
        Block::CodeBlock {
            language: None,
            code: code.into(),
            whole_file: false,
        }
    }

    fn lines(count: usize) -> String {
        (0..count).map(|line| format!("line {line}\n")).collect()
    }

    /// The text of each stretch of `code` cut in `size` lines.
    fn cut(code: &str, size: usize) -> Vec<&str> {
        let block = block(code);
        // The stretches borrow the block, so it lives as long as the test.
        let block: &Block = Box::leak(Box::new(block));
        match Stretches::of(block, code, size) {
            Some(stretches) => stretches.map(|stretch| stretch.code).collect(),
            None => vec![code],
        }
    }

    #[test]
    fn a_text_of_no_more_lines_than_a_stretch_is_not_cut() {
        let code = lines(4);
        assert!(Stretches::of(&block(&code), &code, 4).is_none());
        assert!(Stretches::of(&block(&code), &code, 5).is_none());
        assert!(Stretches::of(&block(""), "", 4).is_none());
        let unended = "a\nb\nc\nd";
        assert!(Stretches::of(&block(unended), unended, 4).is_none());
    }

    #[test]
    fn the_stretches_of_a_text_put_together_are_the_text() {
        for (code, size) in [
            (lines(10), 4),
            (lines(8), 4),
            (lines(9), 4),
            ("a\nb\nc\nd\ne".to_owned(), 2),
            ("a\r\nb\r\n\r\n\r\nc\n".to_owned(), 2),
            ("a\n\n\n\n\n".to_owned(), 1),
            ("é\nü\n\n日本\n".to_owned(), 2),
        ] {
            let parts = cut(&code, size);
            assert!(parts.len() > 1, "{code:?}");
            assert_eq!(parts.concat(), code);
            for part in &parts[..parts.len() - 1] {
                assert_eq!(part.split_inclusive('\n').count(), size, "{part:?}");
                assert!(part.ends_with('\n'));
            }
            let last = parts.last().unwrap();
            assert!(!last.is_empty() && last.split_inclusive('\n').count() <= size);
        }
    }

    #[test]
    fn a_stretch_knows_where_it_is_in_the_block() {
        let code = lines(9);
        let block = block(&code);
        let stretches: Vec<_> = Stretches::of(&block, &code, 4).unwrap().collect();
        let places: Vec<_> = stretches
            .iter()
            .map(|stretch| (stretch.offset, stretch.first, stretch.last))
            .collect();
        let second = code.split_inclusive('\n').take(4).map(str::len).sum();
        let third = code.split_inclusive('\n').take(8).map(str::len).sum();
        assert_eq!(
            places,
            [
                (0, true, false),
                (second, false, false),
                (third, false, true)
            ]
        );
        for stretch in &stretches {
            assert_eq!(&code[stretch.offset..][..stretch.code.len()], stretch.code);
        }
    }

    #[test]
    fn the_scan_goes_on_where_it_stopped_and_a_text_that_grows_only_adds() {
        let mut text = String::from("one\ntwo\nthr");
        let mut index = LineIndex::default();
        index.scan(&text, 2);
        assert_eq!((index.len(), index.start(1)), (2, Some(4)));
        index.scan(&text, usize::MAX);
        // The last line is not ended yet.
        assert_eq!((index.len(), index.start(2)), (3, Some(8)));
        text.push_str("ee\nfour\n");
        index.scan(&text, usize::MAX);
        let starts: Vec<_> = (0..5).map(|line| index.start(line)).collect();
        assert_eq!(starts, [Some(0), Some(4), Some(8), Some(14), None]);
        // What comes after the last line break starts a line once it is
        // something.
        text.push('f');
        index.scan(&text, usize::MAX);
        assert_eq!(index.start(4), Some(19));
        let mut again = LineIndex::default();
        again.scan(&text, usize::MAX);
        assert_eq!(again.starts, index.starts);
    }

    #[test]
    fn stretches_go_on_from_the_place_the_one_before_gave() {
        for (code, size) in [
            (lines(10), 4),
            (lines(9), 3),
            ("é\nü\n\n日本\nx".to_owned(), 1),
        ] {
            let block = block(&code);
            let all: Vec<_> = Stretches::of(&block, &code, size).unwrap().collect();
            let mut stretches = Stretches::of(&block, &code, size).unwrap();
            let mut seen = 0;
            while let Some(at) = stretches.at() {
                let rest: Vec<_> = Stretches::resume(&block, &code, size, at).collect();
                let want: Vec<_> = all[seen..]
                    .iter()
                    .map(|s| (s.code, s.offset, s.first, s.last))
                    .collect();
                let got: Vec<_> = rest
                    .iter()
                    .map(|s| (s.code, s.offset, s.first, s.last))
                    .collect();
                assert_eq!(got, want, "from stretch {seen} of {code:?}");
                stretches.next().unwrap();
                seen += 1;
            }
            assert_eq!(seen, all.len());
        }
    }
}
