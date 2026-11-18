//! A layout made in steps against the layout made whole, which is what
//! `Laid::extend` must give: the integration tests and the unit tests of the
//! engines run the same check.

use std::ops::Range;

use kiri::render::Inputs;
use kiri::typeset::driver::{self, Content};
use kiri::typeset::laid::Laid;
use kiri::typeset::locate::Hidden;
use kiri::typeset::model::{Block, Document};
use kiri::typeset::position::Pos;
use kiri::typeset::style::Line;
use kiri::typeset::wrap::width;

/// How far each call of `extend` goes.
#[derive(Debug, Clone, Copy)]
pub enum Step {
    /// To one line more than there is: the next piece, or stretch, or page.
    Piece,
    /// A window of lines more.
    Window(usize),
    /// One line past the next multiple of `n` lines: a stop falls on the end
    /// of a piece, stretch or page, wherever that is, so this does not stop
    /// at the seams of a code block's stretches exactly.
    Seam(usize),
}

pub const STEPS: [Step; 3] = [Step::Piece, Step::Window(24), Step::Seam(512)];

/// The inputs of one check: for the layout made whole and for one made in
/// each of the [`STEPS`]. A layout in reading mode keeps what it drew in
/// its inputs, so each gets inputs of its own there; where no layout does,
/// the same inputs do for all, as `alike` gives them.
pub type Inputses<'a> = [&'a Inputs; 1 + STEPS.len()];

/// `inputs` four times.
pub fn alike(inputs: &Inputs) -> Inputses<'_> {
    [inputs; 1 + STEPS.len()]
}

impl Step {
    fn until(self, laid: usize) -> usize {
        match self {
            Step::Piece => laid + 1,
            Step::Window(rows) => laid + rows,
            Step::Seam(stretch) => (laid / stretch + 1) * stretch + 1,
        }
    }
}

/// The ranges that each block of `content` is, whole, and the ranges of the
/// places the walk of a code block's text cuts.
pub fn ranges(content: Content) -> Vec<Range<Pos>> {
    let documents: Vec<(usize, &Document)> = match content {
        Content::Document(document) => vec![(0, document)],
        Content::Files(files) => files
            .iter()
            .enumerate()
            .map(|(number, file)| (number, &file.document))
            .collect(),
    };
    let mut found = Vec::new();
    for (number, document) in documents {
        for (path, block) in document.blocks() {
            found.push(Pos::block(number, &path)..Pos::end(number, &path));
            if let Block::CodeBlock { code, .. } = block {
                // A range across what two stretches show.
                let mid = code.len() / 2;
                found.push(Pos::text(number, &path, 0, mid)..Pos::text(number, &path, 0, mid + 3));
            }
            if let Block::Table(table) = block {
                // A cell in the middle of a long table, and a range of cells
                // across what two stretches of one show.
                let mid = table.first_text(table.row_count() / 2);
                found.push(Pos::text(number, &path, mid, 0)..Pos::text(number, &path, mid, 2));
                let seam = table.first_text(127);
                found
                    .push(Pos::text(number, &path, seam, 0)..Pos::text(number, &path, seam + 9, 1));
            }
        }
    }
    found
}

/// Whether two lines are the same. A link of a row of reading mode is told
/// by the address of its page's parts, which no two layouts share, so lines
/// that differ are compared again with those addresses left out.
fn alike_lines(a: &Line, b: &Line) -> bool {
    fn without_addresses(line: &Line) -> String {
        let text = format!("{line:?}");
        let mut kept = String::new();
        let mut rest = text.as_str();
        while let Some(at) = rest.find("link: (") {
            let from = at + "link: (".len();
            let end = rest[from..].find(", ").map_or(rest.len(), |end| from + end);
            kept += &rest[..from];
            kept += "_";
            rest = &rest[end..];
        }
        kept + rest
    }
    a == b || without_addresses(a) == without_addresses(b)
}

/// Asserts that `laid` has as its first lines those of `whole`, and says
/// which differs first when it has not.
fn assert_prefix(laid: &Laid, whole: &Laid, message: &str) {
    let now = laid.len();
    assert!(
        now <= whole.len(),
        "{message}: {now} lines of {}",
        whole.len()
    );
    let (lines, want) = (laid.lines(), &whole.lines()[..now]);
    if let Some(at) = (0..now).find(|&at| !alike_lines(&lines[at], &want[at])) {
        panic!(
            "{message}: line {at} of {now} differs:\n{:?}\nagainst\n{:?}",
            lines[at], want[at]
        );
    }
}

/// A layout of `content` made in each of the [`STEPS`] is, once whole, the
/// layout made in one call: its lines, its table of blocks, its height, and
/// what `locate` gives for each block and `hit` for the first and last cell
/// of each line. After each step its lines are the first lines of the whole
/// layout, and `locate` gives the whole layout's answer or says the range is
/// not laid out.
pub fn assert_in_steps(what: &str, content: Content, inputs: Inputses) {
    assert_in_steps_of(what, content, inputs, &STEPS);
}

/// [`assert_in_steps`] for some of the steps: the layout made in `steps[n]`
/// has `inputs[n + 1]`.
pub fn assert_in_steps_of(what: &str, content: Content, inputs: Inputses, steps: &[Step]) {
    let whole_inputs = inputs[0];
    let whole = driver::laid(content, whole_inputs);
    assert!(!whole.is_empty(), "{what}: nothing laid out");
    let ranges = ranges(content);
    let caps = whole_inputs.caps.clone();
    let total = whole.len();
    for (&step, inputs) in steps.iter().zip(&inputs[1..]) {
        let mut laid = Laid::begin(content, 0, inputs);
        assert!(
            laid.is_empty() && !laid.is_whole(),
            "{what}: {step:?}: begun"
        );
        // Every table is whole before a line is laid.
        assert_eq!(laid.blocks(), whole.blocks(), "{what}: {step:?}: table");
        let mut calls = 0;
        loop {
            let before = laid.len();
            let until = step.until(before);
            let finished = laid.extend(content, inputs, until);
            calls += 1;
            assert!(
                calls <= total + 8,
                "{what}: {step:?}: no end after {calls} calls"
            );
            assert_eq!(finished, laid.is_whole(), "{what}: {step:?}");
            assert!(laid.len() >= before, "{what}: {step:?}: lines went");
            assert!(
                finished || laid.len() >= until,
                "{what}: {step:?}: {} lines for {until}",
                laid.len()
            );
            let message = format!("{what}: {step:?}: the lines laid after {calls} calls");
            assert_prefix(&laid, &whole, &message);
            for range in &ranges {
                let located = laid.locate(range);
                assert!(
                    located == Err(Hidden::NotLaidOut) || located == whole.locate(range),
                    "{what}: {step:?}: locate {range:?} after {calls} calls: {located:?} against {:?}",
                    whole.locate(range)
                );
            }
            if finished {
                break;
            }
        }
        assert_eq!(laid.len(), total, "{what}: {step:?}");
        assert_eq!(laid.blocks(), whole.blocks(), "{what}: {step:?}: table");
        assert_eq!(laid.height(&caps), whole.height(&caps), "{what}: {step:?}");
        assert_prefix(&laid, &whole, &format!("{what}: {step:?}: the lines"));
        for range in &ranges {
            assert_eq!(
                laid.locate(range),
                whole.locate(range),
                "{what}: {step:?}: locate {range:?}"
            );
        }
        for line in 0..total {
            let last = width(&whole.lines()[line].lead) + width(&whole.lines()[line].spans);
            for column in [0, last.saturating_sub(1)] {
                assert_eq!(
                    laid.hit(line, column),
                    whole.hit(line, column),
                    "{what}: {step:?}: hit {line}:{column}"
                );
            }
        }
        assert_eq!(laid.bands().len(), whole.bands().len(), "{what}: {step:?}");
    }
}
