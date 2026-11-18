//! The driver of the layout: it walks what is to be laid out, one
//! document or several files, and gives back the lines, with the table of
//! the blocks they come from.
//!
//! A layout is made in steps. [`begin`] numbers the blocks and lays nothing
//! out, [`advance`] lays out the next pieces from where the layout stopped
//! and stops again, and what the next step needs of the pen and the walk is
//! kept in a [`Checkpoint`] that owns it. The steps' stops are between two
//! pieces, between two stretches of a code block and between two pages of
//! reading mode; a layout laid out to its end in one step is the one the
//! steps give. Some checkpoints are kept, so that a layout can go back to
//! one and be laid out from there again, as when an image arrives.
//!
//! The layout of the blocks themselves is the cell layout engine's
//! (`src/typeset/layout/`) or reading mode's (`src/typeset/reading/`).

use crate::render::Inputs;
use crate::typeset::block::Blocks;
use crate::typeset::laid::Laid;
use crate::typeset::layout::{Cells, CellsSaved, Derived, texts_hidden, with_cells};
use crate::typeset::model::{Block, Document, File, Inline};
use crate::typeset::position::Pos;
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::pen::Pixels;
use crate::typeset::stretch::{self, Stretch};
use crate::typeset::style::{Line, Loading};
use crate::typeset::walk::{Place, Walk};
use crate::typeset::wrap::width;

/// `document` laid out, without the table of its blocks, as [`file`] has
/// it.
pub fn layout(document: &Document, inputs: &Inputs) -> Vec<Line> {
    document_stream(document, 0, inputs, stretch::LINES).lines
}

/// `document` laid out as a lone document, with the blocks its lines come
/// from.
fn document_stream(document: &Document, first: usize, inputs: &Inputs, size: usize) -> Stream {
    stream(Content::Document(document), (first, 0), inputs, size)
}

/// `content` laid out, the keys of the images of its first document carrying
/// `first` and those of the others the numbers after it, and its files
/// numbered from `at`; a code block of more than `size` lines is cut in
/// stretches of `size` lines. The files are laid out one after another, a
/// blank line apart; the blank line has no block, as the blank lines between
/// blocks have none.
fn stream(content: Content, (first, at): (usize, usize), inputs: &Inputs, size: usize) -> Stream {
    let mut laid = begin(content, (first, at), inputs, size);
    laid.extend(content, inputs, usize::MAX);
    laid.into()
}

/// A layout of `content` with nothing laid out, its table of blocks whole,
/// the keys of its images numbered as `stream` has it.
pub(crate) fn begin(
    content: Content,
    (first, at): (usize, usize),
    inputs: &Inputs,
    size: usize,
) -> Laid {
    let progress = Progress {
        derived: Derived::new(inputs),
        first,
        at,
        size,
        measure: false,
        run: Run {
            now: Checkpoint::of_file(0, 0),
            saved: Vec::new(),
            frontier: None,
            done: false,
            parts: Vec::new(),
            truth: true,
            colours: Colours::default(),
        },
    };
    Laid::stepped(table(content, at, inputs), progress)
}

impl Laid {
    /// A layout of `content` that nothing is laid out of yet: its table of
    /// blocks is whole, and [`Laid::extend`] lays the lines out.
    pub fn begin(content: Content, first: usize, inputs: &Inputs) -> Self {
        begin(content, (first, 0), inputs, stretch::LINES)
    }

    /// Lays out more of `content`, which must be what the layout was begun
    /// with, with `inputs` of what it was begun with: the next pieces, from
    /// where it stopped, until there are at least `until` lines, or it is
    /// whole. The stops are between two pieces, two stretches of a code block
    /// or two pages of reading mode. Whether it is whole.
    pub fn extend(&mut self, content: Content, inputs: &Inputs, until: usize) -> bool {
        let Some((lines, blocks, progress)) = self.stepping() else {
            return true;
        };
        advance(progress, content, inputs, blocks, lines, until);
        self.went_on();
        self.is_whole()
    }

    /// A layout of `content` that nothing is laid out of yet, like
    /// [`Laid::begin`], that only measures the stretches between the first
    /// and the last of a large code block: they get their lines, each
    /// empty, and [`Laid::show`] lays out the ones that are wanted.
    pub fn begin_measured(content: Content, first: usize, inputs: &Inputs) -> Self {
        let mut laid = begin(content, (first, 0), inputs, stretch::LINES);
        if let Some((_, _, progress)) = laid.stepping() {
            progress.measure = true;
        }
        laid
    }

    /// Lays out the stretches that are only measured and hold a line of
    /// `near`, with `content` and `inputs` of what the layout was begun
    /// with, and lets go of the lines of those far from it. No line moves.
    /// Whether a line changed.
    pub fn show(
        &mut self,
        content: Content,
        inputs: &Inputs,
        near: std::ops::Range<usize>,
    ) -> bool {
        let Some((lines, blocks, progress)) = self.stepping() else {
            return false;
        };
        let changed = show(progress, content, inputs, blocks, lines, near);
        for lines in &changed {
            self.changed(lines.clone());
        }
        !changed.is_empty()
    }

    /// The lines that are only measured, by stretch, in order.
    pub fn measured(&self) -> Vec<std::ops::Range<usize>> {
        self.progress().map(measured).unwrap_or_default()
    }

    /// Does a piece of what is left to do in the background for a layout
    /// that measures, for a view among the lines `near`: it colours the
    /// stretches near the view, lays out those a little past it, and takes
    /// the highlighting of a large block on from its top while it has time
    /// left for that. No line moves. Whether a line changed.
    pub fn work(
        &mut self,
        content: Content,
        inputs: &Inputs,
        near: std::ops::Range<usize>,
    ) -> bool {
        let Some((lines, blocks, progress)) = self.stepping() else {
            return false;
        };
        let changed = work(progress, content, inputs, blocks, lines, near);
        if let Some(lines) = &changed {
            self.changed(lines.clone());
        }
        changed.is_some()
    }

    /// Whether [`Laid::work`] has something left to do for a view among
    /// the lines `near`.
    pub fn pending(&self, near: std::ops::Range<usize>) -> bool {
        self.progress()
            .is_some_and(|progress| pending(progress, near))
    }
}

/// [`laid_from`], with the stretches between the first and the last of a
/// large code block only measured; see [`Laid::begin_measured`].
pub fn laid_measured(content: Content, first: usize, inputs: &Inputs) -> Laid {
    let mut laid = Laid::begin_measured(content, first, inputs);
    laid.extend(content, inputs, usize::MAX);
    laid
}

/// What a piece is: a block, or a part of one that the engines treat as a
/// block of its own.
#[derive(Clone, Copy)]
pub(crate) enum Shape<'a> {
    /// A block as a whole.
    Block(&'a Block),
    /// The summary of the `<details>` block; its blocks are pieces of
    /// their own.
    Summary {
        summary: &'a [Inline],
        blocks: &'a [Block],
        /// The number of the summary among the texts of the block.
        number: Option<usize>,
    },
    /// The header of a file, when files are shown together.
    Header(&'a str),
    /// A part of a large code block, which goes on from the part before it:
    /// the block's one entry in the table of blocks, and no blank line
    /// between the parts.
    Stretch(Stretch<'a>),
    /// A part of a long table, its rows, which go on from the part before it
    /// as a code block's stretches do.
    Rows(stretch::Rows<'a>),
}

impl Shape<'_> {
    /// Whether the piece is the first and whether the last of the parts of a
    /// large block, when it is one of them.
    fn ends(&self) -> Option<(bool, bool)> {
        match self {
            Shape::Stretch(stretch) => Some((stretch.first, stretch.last)),
            Shape::Rows(rows) => Some((rows.first, rows.last)),
            _ => None,
        }
    }
}

/// A piece of the document: a block, or the summary of a `<details>`, or a
/// file's header, which the lines of the layout mark as their own
/// (`Line::block`), with its place in the document.
pub(crate) struct Piece<'a> {
    pub shape: Shape<'a>,
    /// The place of the piece's block in the document, as the table of
    /// blocks has it; empty for a header.
    pub path: Vec<usize>,
}

/// What an engine's pen keeps from one piece to the next, owned, so that a
/// layout that stops can go on with a pen made again.
#[derive(Debug, Clone, Default)]
pub(crate) struct Saved {
    /// What the cell layout's pen keeps.
    pub cells: CellsSaved,
    /// The run of blocks that reading mode holds: the place of its first
    /// block and how many there are. They are consecutive pieces of the
    /// walk, so the walk gives them again.
    pub held: Option<(Place, usize)>,
    /// The bottom margin of the page of the run that was laid out last,
    /// which the next goes on with.
    pub margin: Option<f32>,
    /// Whether a page of the run held was laid out, and the next page joins
    /// it.
    pub joined: bool,
}

/// An engine's pen: what lays the pieces out in cells or in pixels, and
/// decides where its flow can be cut.
pub(crate) trait Pen<'d> {
    /// Lays `piece` out, and stacks what the pen decides to stack now.
    fn piece(&mut self, piece: Piece<'d>, stack: &mut Stack);
    /// Whether `piece` is held, to be laid out with the pieces after it,
    /// and so lays out nothing yet.
    fn holds(&self, _piece: &Piece<'d>) -> bool {
        false
    }
    /// Whether the pen holds pieces it has not laid out.
    fn pending(&self) -> bool {
        false
    }
    /// Cuts the flow: the pieces the pen holds are laid out and stacked, a
    /// part at a time while the stack has fewer than `until` lines. Whether
    /// the pen holds nothing now.
    fn cut(&mut self, stack: &mut Stack, until: usize) -> bool;
    /// Whether the pen lays the code block `block` out in stretches, when it
    /// is large: not when it draws it whole, as it does a diagram.
    fn cuts(&self, _block: &Block) -> bool {
        false
    }
    /// Whether the pen can measure a stretch of a code block: stack lines
    /// that stand in the place of its lines, as many, without laying it out.
    fn measures(&self) -> bool {
        false
    }
    /// Stacks the lines of `piece`, a stretch that is neither the first nor
    /// the last of its block, each only measured. A pen that does not
    /// measure lays the piece out.
    fn measure(&mut self, piece: Piece<'d>, stack: &mut Stack) {
        self.piece(piece, stack);
    }
    /// Has the stretches from here on of the code block that is open
    /// between two stretches highlighted as `basis` says, when they are
    /// not highlighted from where the code before them puts it.
    fn colour(&mut self, _basis: Basis) {}
    /// Takes the highlighting of the code block that is open over `piece`,
    /// a stretch of it that is `rows` lines of the layout, without laying
    /// it out. How many lines of code it has, and how many of them a guess
    /// would colour as they are.
    fn carry(&mut self, _piece: Piece<'d>, _rows: usize) -> (usize, usize) {
        (0, 0)
    }
    /// What the pen keeps from one piece to the next.
    fn save(&self) -> Saved;
    /// Puts the pen where `saved` was, in the file `walk` is of.
    fn restore(&mut self, saved: &Saved, walk: &Walk<'d>);
}

/// The lines of the pieces of a file, a blank line between the pieces.
#[derive(Default)]
pub(crate) struct Stack {
    /// The lines of the pieces since the stack was last emptied.
    lines: Vec<Line>,
    /// Whether something came before, as a piece's lines are set off by a
    /// blank line from it.
    before: bool,
    /// The lines of the layout before these.
    base: usize,
}

impl Stack {
    /// A stack on `base` lines of the layout, with something before it if
    /// `before`.
    fn after(base: usize, before: bool) -> Self {
        Self {
            lines: Vec::new(),
            before,
            base,
        }
    }

    /// How many lines the layout has with this stack's.
    pub(crate) fn len(&self) -> usize {
        self.base + self.lines.len()
    }

    /// Adds the lines of a piece, after a blank line if something came
    /// before. A piece without lines, such as an empty code block, gets no
    /// separator either.
    pub(crate) fn push(&mut self, lines: Vec<Line>) {
        self.add(lines, true);
    }

    /// Adds the lines of a piece that goes on from the one before it, as
    /// the next page of a run that was cut does: no blank line comes
    /// between them.
    pub(crate) fn join(&mut self, lines: Vec<Line>) {
        self.add(lines, false);
    }

    fn add(&mut self, lines: Vec<Line>, blank: bool) {
        if lines.is_empty() {
            return;
        }
        if blank && self.before {
            self.lines.push(Line::blank());
        }
        self.lines.extend(lines);
        self.before = true;
    }
}

/// The engine that a layout is made in.
#[derive(Clone, Copy)]
enum Engine<'a> {
    Cells,
    Pixels(&'a Reading),
}

impl<'a> Engine<'a> {
    /// Reading mode where the terminal draws images, which its text is;
    /// the cell layout otherwise.
    fn of(inputs: &'a Inputs) -> Self {
        match &inputs.reading {
            Some(reading) if inputs.caps.draws_images() => Engine::Pixels(reading),
            _ => Engine::Cells,
        }
    }
}

/// Where a layout in steps is, owned: what [`advance`] needs to go on from
/// it, and a restart goes back to.
#[derive(Debug, Clone)]
pub(crate) struct Checkpoint {
    /// The file it is in, counted from the first shown.
    file: usize,
    /// Whether the file has begun, which is when the blank line between it
    /// and the file before is laid.
    started: bool,
    /// The piece the walk gives next.
    place: Place,
    /// The lines laid out before it.
    lines: usize,
    /// Whether the file's stack has something in it.
    before: bool,
    /// How many of the lines have their bands settled: all but, perhaps, the
    /// last, which waits for the line that may come below it.
    settled: usize,
    /// The band of the last line, as it was before it was settled, when it
    /// is not yet.
    tail: Option<Box<Loading>>,
    pen: Saved,
}

impl Checkpoint {
    /// The start of the file numbered `file`, `lines` lines in.
    fn of_file(file: usize, lines: usize) -> Self {
        Self {
            file,
            started: false,
            place: Place::start(false),
            lines,
            before: false,
            settled: lines,
            tail: None,
            pen: Saved::default(),
        }
    }
}

/// The checkpoints of a layout in steps: where it is, and where it can go
/// back to.
#[derive(Debug)]
struct Run {
    now: Checkpoint,
    /// Earlier checkpoints, by the lines before them.
    saved: Vec<Checkpoint>,
    /// What is not laid out yet starts at this place of the document, when
    /// something is not.
    frontier: Option<Pos>,
    done: bool,
    /// The stretches after the first of the code blocks laid out in
    /// stretches, in the order of the lines, when the layout measures.
    parts: Vec<Part>,
    /// Whether the highlighting of the pen is where the whole code before
    /// it puts it: not after a stretch that was only measured.
    truth: bool,
    colours: Colours,
}

/// A stretch of a code block after its first, in a layout that measures:
/// its lines are laid out, or only measured, as many.
#[derive(Debug)]
struct Part {
    /// Its lines in the layout.
    lines: std::ops::Range<usize>,
    /// The layout at its start, with the pen as it was then.
    from: Checkpoint,
    /// Whether it is the last of its block, which is always laid out.
    last: bool,
    /// Whether its lines are laid out.
    laid: bool,
    /// How its lines are highlighted, when they are laid out.
    basis: Basis,
    /// The pen at its start with the highlighting where the code before it
    /// puts it, when that is known. A block that is not highlighted always
    /// knows.
    truth: Option<Saved>,
}

/// How the lines of a stretch of a code block are highlighted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Basis {
    /// Not at all: in the theme's text colour.
    Plain,
    /// From a fresh start at the first line of the stretch.
    Guess,
    /// From where the code before the stretch puts the highlighting.
    True,
}

/// How far the highlighting of the stretches that are only measured is
/// known, and how well a guess does in this layout.
#[derive(Debug, Default)]
struct Colours {
    /// The time spent taking the highlighting on from the top.
    spent: std::time::Duration,
    /// The lines of code a guess was held against the highlighting from
    /// the top, and how many of them it coloured the same.
    lines: usize,
    equal: usize,
    /// No stretch before this one has its highlighting to take on.
    next: usize,
}

impl Colours {
    /// Whether a stretch whose highlighting is not known is highlighted
    /// from a guess: when enough lines were compared and nearly all of them
    /// were the same.
    fn guesses(&self) -> bool {
        self.lines >= GUESS_LINES && self.equal * 100 >= self.lines * GUESS_PERCENT
    }
}

/// The lines a guess is held against the highlighting from the top before
/// guesses are used, and the share of them, in percent, that must be the
/// same.
const GUESS_LINES: usize = 1000;
const GUESS_PERCENT: usize = 99;

/// The time a layout spends taking the highlighting of a large block on
/// from its top, in the background.
const COLOUR_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(test)]
thread_local! {
    /// The budget the tests set in the place of `COLOUR_BUDGET`.
    static BUDGET: std::cell::Cell<Option<std::time::Duration>> =
        const { std::cell::Cell::new(None) };
}

/// Sets the time the layouts made on this thread spend taking highlighting
/// on from the top; `None` for the time they have outside the tests.
#[cfg(test)]
pub(crate) fn set_colour_budget(budget: Option<std::time::Duration>) {
    BUDGET.with(|kept| kept.set(budget));
}

/// The time a layout spends taking highlighting on from the top.
fn colour_budget() -> std::time::Duration {
    #[cfg(test)]
    if let Some(budget) = BUDGET.with(std::cell::Cell::get) {
        return budget;
    }
    COLOUR_BUDGET
}

/// What a layout in steps keeps between its steps besides its lines.
#[derive(Debug)]
pub(crate) struct Progress {
    derived: Derived,
    /// The number the keys of the first document's images carry.
    first: usize,
    /// The number of the first file.
    at: usize,
    /// The source lines of a stretch of a code block.
    size: usize,
    /// Whether the stretches between the first and the last of a code
    /// block are only measured, until [`Laid::show`] lays them out.
    measure: bool,
    run: Run,
}

impl Progress {
    /// Whether the layout is whole.
    pub(crate) fn is_done(&self) -> bool {
        self.run.done
    }

    /// The first place of the document that is not laid out, `None` when
    /// all is.
    pub(crate) fn frontier(&self) -> Option<&Pos> {
        self.run.frontier.as_ref()
    }

    /// Takes the layout back to the last checkpoint at or before `line`:
    /// `lines`, the lines laid out so far, lose those after it. The first
    /// checkpoint is the start of the first file.
    pub(crate) fn restart(&mut self, lines: &mut Vec<Line>, content: Content, line: usize) {
        let shown = units(content, self.first, self.at);
        let run = &mut self.run;
        let at = run.saved.iter().rposition(|saved| saved.lines <= line);
        let mut now = match at {
            Some(at) => {
                run.saved.truncate(at + 1);
                run.saved[at].clone()
            }
            None => Checkpoint::of_file(0, 0),
        };
        lines.truncate(now.lines);
        // The stretches from there on are measured again; the highlighting
        // is known there if it was at the stretch that started there.
        let kept = run
            .parts
            .partition_point(|part| part.from.lines < now.lines);
        // The pen of a checkpoint inside a measured block has the
        // highlighting of the last stretch that was laid out, not its own:
        // it goes on from what is known of the stretch that starts there.
        let known = (run.parts.get(kept))
            .filter(|part| part.from.lines == now.lines)
            .map(|part| part.truth.clone());
        match known {
            Some(Some(truth)) => (now.pen, run.truth) = (truth, true),
            Some(None) => run.truth = false,
            None => run.truth = true,
        }
        run.parts.truncate(kept);
        run.colours.next = run.colours.next.min(kept);
        if let Some(tail) = &now.tail
            && let Some(last) = lines.last_mut()
        {
            last.loading = Some(tail.clone());
        }
        run.frontier = frontier(&shown, &now);
        run.now = now;
        run.done = false;
    }
}

/// How many lines the layout lays out between two checkpoints it keeps, at
/// least, and the start of every stretch of a code block is one.
const KEPT_EVERY: usize = 512;

/// Lays out `content`, which is what `progress` was begun with, from where
/// the layout stopped, until `lines` has `until` lines or it is whole, with
/// `inputs` and the blocks in `blocks`.
pub(crate) fn advance(
    progress: &mut Progress,
    content: Content,
    inputs: &Inputs,
    blocks: &mut Blocks,
    lines: &mut Vec<Line>,
    until: usize,
) {
    let shown = units(content, progress.first, progress.at);
    while !progress.run.done && lines.len() < until {
        let Some(unit) = shown.get(progress.run.now.file) else {
            break;
        };
        if !progress.run.now.started {
            // The start of a file is a checkpoint to go back to.
            let start = progress.run.now.clone();
            keep(&mut progress.run, start);
            let now = &mut progress.run.now;
            if now.file > 0 {
                lines.push(Line::blank());
                now.lines += 1;
                now.settled = now.lines;
            }
            now.started = true;
            now.place = Place::start(unit.header.is_some());
        }
        let walk = Walk::new(unit.header, &unit.document.blocks, progress.size);
        let numbers = (unit.number, unit.keys);
        let run = &mut progress.run;
        let to = (until, progress.measure);
        let finished =
            with_cells(
                inputs,
                &progress.derived,
                numbers,
                blocks,
                |cells| match Engine::of(inputs) {
                    Engine::Cells => step(cells, &walk, run, lines, to),
                    Engine::Pixels(reading) => {
                        step(Pixels::new(cells, reading), &walk, run, lines, to)
                    }
                },
            );
        if finished {
            let next = run.now.file + 1;
            run.now = Checkpoint::of_file(next, lines.len());
        }
    }
    let run = &mut progress.run;
    if run.now.file >= shown.len() && !run.done {
        run.done = true;
        run.frontier = None;
        let from = progress.first + progress.at;
        end(inputs, from..from + shown.len());
    } else if !run.done {
        run.frontier = frontier(&shown, &run.now);
    }
}

/// What lays the stretches of a layout that measures out and lets them go:
/// the layout's progress, what it is of, and its lines.
struct Parts<'a, 'c> {
    progress: &'a mut Progress,
    content: Content<'c>,
    inputs: &'a Inputs,
    blocks: &'a mut Blocks,
    lines: &'a mut [Line],
}

impl Parts<'_, '_> {
    /// Runs `with` with the pen where `from` puts it, at the piece that the
    /// stretch numbered `index` is.
    fn with_pen<T>(
        &mut self,
        index: usize,
        from: &Saved,
        with: impl FnOnce(&mut Cells, Piece) -> T,
    ) -> Option<T> {
        let progress = &*self.progress;
        let part = &progress.run.parts[index];
        let shown = units(self.content, progress.first, progress.at);
        let unit = shown.get(part.from.file)?;
        let walk = Walk::new(unit.header, &unit.document.blocks, progress.size);
        let numbers = (unit.number, unit.keys);
        with_cells(
            self.inputs,
            &progress.derived,
            numbers,
            self.blocks,
            |mut pen| {
                Pen::restore(&mut pen, from, &walk);
                let (piece, _) = walk.next(&part.from.place, |block| pen.cuts(block))?;
                Some(with(&mut pen, piece))
            },
        )
    }

    /// Lays the stretch numbered `index` out, highlighted as `basis` says,
    /// in the place of its lines, as many. A stretch whose highlighted
    /// lines are not as many as were measured is laid out without colours,
    /// which is how it was measured.
    fn lay(&mut self, index: usize, basis: Basis) {
        self.lay_as(index, basis, basis);
    }

    /// [`Parts::lay`], noting the stretch as highlighted as `noted` says:
    /// what was asked for, also when it could not be done, so that it is
    /// not asked for again.
    fn lay_as(&mut self, index: usize, basis: Basis, noted: Basis) {
        let part = &self.progress.run.parts[index];
        let from = match (basis, &part.truth) {
            (Basis::True, Some(truth)) => truth.clone(),
            _ => part.from.pen.clone(),
        };
        let (range, start) = (part.lines.clone(), part.from.lines);
        let laid = self.with_pen(index, &from, |pen, piece| {
            Pen::colour(pen, basis);
            let mut stack = Stack::after(start, true);
            Pen::piece(pen, piece, &mut stack);
            (stack.lines, Pen::save(pen))
        });
        let Some((laid, after)) = laid else {
            let part = &mut self.progress.run.parts[index];
            (part.laid, part.basis) = (true, noted);
            return;
        };
        if laid.len() != range.len() && basis != Basis::Plain {
            return self.lay_as(index, Basis::Plain, noted);
        }
        debug_assert_eq!(laid.len(), range.len(), "a stretch has the lines measured");
        let block = self.lines[range.start].block;
        let mut laid = laid.into_iter();
        for line in &mut self.lines[range] {
            // The bands of the last stretch are settled where they are.
            let loading = line.loading.take();
            *line = laid.next().unwrap_or_else(|| Line {
                block,
                inner: block,
                ..Line::blank()
            });
            line.loading = loading;
        }
        let parts = &mut self.progress.run.parts;
        (parts[index].laid, parts[index].basis) = (true, noted);
        let (end, last) = (parts[index].lines.end, parts[index].last);
        if basis == Basis::True
            && !last
            && let Some(next) = parts.get_mut(index + 1)
            && next.from.lines == end
            && next.truth.is_none()
        {
            next.truth = Some(after);
        }
    }

    /// Has the lines of the stretch numbered `index` only measured again.
    fn let_go(&mut self, index: usize) {
        let part = &self.progress.run.parts[index];
        let (range, start, from) = (part.lines.clone(), part.from.lines, part.from.pen.clone());
        let measured = self.with_pen(index, &from, |pen, piece| {
            let mut stack = Stack::after(start, true);
            Pen::measure(pen, piece, &mut stack);
            stack.lines
        });
        let Some(measured) = measured else {
            return;
        };
        debug_assert_eq!(
            measured.len(),
            range.len(),
            "a stretch is measured the same"
        );
        for (line, measured) in self.lines[range].iter_mut().zip(measured) {
            *line = measured;
        }
        self.progress.run.parts[index].laid = false;
    }

    /// How the stretch numbered `index` is best highlighted now: from what
    /// is before it when that is known, from a guess when guesses do well
    /// in this layout, and otherwise not at all.
    fn best(&self, index: usize) -> Basis {
        let run = &self.progress.run;
        match (&run.parts[index].truth, run.colours.guesses()) {
            (Some(_), _) => Basis::True,
            (None, true) => Basis::Guess,
            (None, false) => Basis::Plain,
        }
    }

    /// The stretch whose highlighting is known and can be taken on to the
    /// stretch after it, whose is not, if the layout has time left for it.
    fn to_carry(&self) -> Option<usize> {
        let run = &self.progress.run;
        if run.colours.spent >= colour_budget() {
            return None;
        }
        (run.colours.next..run.parts.len().saturating_sub(1)).find(|&index| {
            let (part, next) = (&run.parts[index], &run.parts[index + 1]);
            part.truth.is_some()
                && !part.last
                && next.truth.is_none()
                && next.from.lines == part.lines.end
        })
    }

    /// Takes the highlighting on over the stretch numbered `index`, to the
    /// stretch after it, and holds a guess against it on the way.
    fn carry(&mut self, index: usize) {
        let part = &self.progress.run.parts[index];
        let (Some(truth), rows) = (part.truth.clone(), part.lines.len()) else {
            return;
        };
        let started = std::time::Instant::now();
        let carried = self.with_pen(index, &truth, |pen, piece| {
            let compared = Pen::carry(pen, piece, rows);
            (compared, Pen::save(pen))
        });
        let run = &mut self.progress.run;
        run.colours.next = index + 1;
        let Some(((lines, equal), after)) = carried else {
            return;
        };
        run.colours.spent += started.elapsed();
        run.colours.lines += lines;
        run.colours.equal += equal;
        run.parts[index + 1].truth = Some(after);
    }

    /// The stretch that holds a line of `near` and is laid out with less
    /// colour than it can have now, or is only measured.
    fn to_colour(&self, near: &std::ops::Range<usize>) -> Option<(usize, Basis)> {
        let parts = &self.progress.run.parts;
        let from = parts.partition_point(|part| part.lines.end <= near.start);
        (from..parts.len())
            .take_while(|&index| parts[index].lines.start < near.end)
            .find_map(|index| {
                let best = self.best(index);
                (!parts[index].laid || parts[index].basis < best).then_some((index, best))
            })
    }
}

/// How many lines from the lines that are wanted a stretch that is laid out
/// stays so, before its lines are only measured again.
const KEPT_NEAR: usize = 2048;

/// How many lines past the lines that are wanted the stretches are laid
/// out in the background, so that a reader who reads on meets them laid
/// out and coloured.
const AHEAD: usize = 256;

/// Lays out the stretches of `lines` that are only measured and hold a line
/// of `near`, without colours when they are highlighted, which takes no
/// time: [`work`] colours them. The stretches far from `near` are only
/// measured again. No line moves: a stretch has as many lines either way.
/// The lines that changed, by stretch.
pub(crate) fn show(
    progress: &mut Progress,
    content: Content,
    inputs: &Inputs,
    blocks: &mut Blocks,
    lines: &mut [Line],
    near: std::ops::Range<usize>,
) -> Vec<std::ops::Range<usize>> {
    let mut parts = Parts {
        progress,
        content,
        inputs,
        blocks,
        lines,
    };
    let far = near.start.saturating_sub(KEPT_NEAR)..near.end.saturating_add(KEPT_NEAR);
    let mut changed = Vec::new();
    for index in 0..parts.progress.run.parts.len() {
        let part = &parts.progress.run.parts[index];
        let range = part.lines.clone();
        let holds =
            |lines: &std::ops::Range<usize>| range.start < lines.end && lines.start < range.end;
        if holds(&near) && !part.laid {
            // A block that is not highlighted is laid out as it is.
            let basis = match part.from.pen.cells.highlighted() {
                true => Basis::Plain,
                false => Basis::True,
            };
            parts.lay(index, basis);
            changed.push(range);
        } else if part.laid && !part.last && !holds(&far) {
            parts.let_go(index);
            changed.push(range);
        }
    }
    changed
}

/// Does a piece of what is left to do for the stretches of a layout that
/// measures, for a reader whose view is among the lines `near`: colours a
/// stretch near the view, or lays out and colours one a little past it, or
/// takes the highlighting on from the top over one stretch more, while
/// the layout has time left for that. The lines that changed, if any did.
pub(crate) fn work(
    progress: &mut Progress,
    content: Content,
    inputs: &Inputs,
    blocks: &mut Blocks,
    lines: &mut [Line],
    near: std::ops::Range<usize>,
) -> Option<std::ops::Range<usize>> {
    let mut parts = Parts {
        progress,
        content,
        inputs,
        blocks,
        lines,
    };
    let ahead = near.start.saturating_sub(AHEAD)..near.end.saturating_add(AHEAD);
    let found = parts.to_colour(&near).or_else(|| parts.to_colour(&ahead));
    if let Some((index, basis)) = found {
        parts.lay(index, basis);
        return Some(parts.progress.run.parts[index].lines.clone());
    }
    if let Some(index) = parts.to_carry() {
        parts.carry(index);
    }
    None
}

/// Whether [`work`] has something left to do for a view among `near`.
pub(crate) fn pending(progress: &Progress, near: std::ops::Range<usize>) -> bool {
    let run = &progress.run;
    let ahead = near.start.saturating_sub(AHEAD)..near.end.saturating_add(AHEAD);
    let from = run
        .parts
        .partition_point(|part| part.lines.end <= ahead.start);
    let near = run.parts[from..]
        .iter()
        .take_while(|part| part.lines.start < ahead.end);
    let best = |part: &Part| match (&part.truth, run.colours.guesses()) {
        (Some(_), _) => Basis::True,
        (None, true) => Basis::Guess,
        (None, false) => Basis::Plain,
    };
    let mut to_colour = near.filter(|part| !part.laid || part.basis < best(part));
    let carries = run.colours.spent < colour_budget()
        && (run.parts[run.colours.next.min(run.parts.len())..].windows(2)).any(|pair| {
            pair[0].truth.is_some()
                && !pair[0].last
                && pair[1].truth.is_none()
                && pair[1].from.lines == pair[0].lines.end
        });
    to_colour.next().is_some() || carries
}

/// The lines of `progress`'s layout that are only measured, by stretch.
pub(crate) fn measured(progress: &Progress) -> Vec<std::ops::Range<usize>> {
    let parts = progress.run.parts.iter().filter(|part| !part.laid);
    parts.map(|part| part.lines.clone()).collect()
}

/// Where what is not laid out starts in the document, for the checkpoint
/// `now`, which is not the last: the first block reading mode holds, or the
/// piece the walk gives next, or the start of the next file.
fn frontier(shown: &[Unit], now: &Checkpoint) -> Option<Pos> {
    let unit = shown.get(now.file)?;
    let walk = Walk::new(unit.header, &unit.document.blocks, usize::MAX);
    if let Some((first, _)) = &now.pen.held {
        return walk.pos(first, unit.number);
    }
    let at = match now.started {
        true => now.place.clone(),
        false => Place::start(unit.header.is_some()),
    };
    // A file whose last piece is laid is not done until the walk is asked
    // for another, so what is not laid is the start of the next file.
    walk.pos(&at, unit.number).or_else(|| {
        (shown[now.file + 1..].iter()).find_map(|next| match next.header {
            Some(_) => Some(Pos::block(next.number, &[])),
            None => (!next.document.blocks.is_empty()).then(|| Pos::block(next.number, &[0])),
        })
    })
}

/// Keeps the checkpoint `at`, unless the last kept one is the same place.
fn keep(run: &mut Run, at: Checkpoint) {
    let same = run.saved.last().is_some_and(|last| {
        (last.file, last.started, last.lines) == (at.file, at.started, at.lines)
            && last.place == at.place
    });
    if !same {
        run.saved.push(at);
    }
}

/// Keeps the checkpoint of `pen` at `at` in the file `file`, when `stretch`
/// says a stretch starts here or a page is about to be laid out, or `lines`
/// has grown by `KEPT_EVERY` since the last one kept.
fn kept<'d>(
    run: &mut Run,
    pen: &impl Pen<'d>,
    (file, at): (usize, &Place),
    (lines, stack, settled): (&[Line], &Stack, usize),
    stretch: bool,
) {
    let far = (run.saved.last()).is_none_or(|last| lines.len() >= last.lines + KEPT_EVERY);
    if far || stretch {
        let tail = lines.get(settled).and_then(|line| line.loading.clone());
        let checkpoint = Checkpoint {
            file,
            started: true,
            place: at.clone(),
            lines: lines.len(),
            before: stack.before,
            settled,
            tail,
            pen: pen.save(),
        };
        keep(run, checkpoint);
    }
}

/// Lays out the file `walk` is of with `pen`, from the checkpoint `run` is
/// at, until `lines` has `until` lines. Whether the file is whole. `run` is
/// at where it stopped.
fn step<'d>(
    mut pen: impl Pen<'d>,
    walk: &Walk<'d>,
    run: &mut Run,
    lines: &mut Vec<Line>,
    (until, measure): (usize, bool),
) -> bool {
    let from = run.now.clone();
    pen.restore(&from.pen, walk);
    let mut stack = Stack::after(lines.len(), from.before);
    let (mut at, mut settled) = (from.place, from.settled);
    let mut finished = false;
    loop {
        lines.append(&mut stack.lines);
        stack.base = lines.len();
        settle(lines, &mut settled, false);
        // A step does not stop with the band of its last line waiting for
        // the line below it: the lines it leaves are those of the whole
        // layout. It lays out one more piece instead.
        let waiting = settled < lines.len();
        if lines.len() >= until && !waiting {
            break;
        }
        let want = if waiting {
            until.max(lines.len() + 1)
        } else {
            until
        };
        // A run that reading mode holds is laid out a page at a time, with a
        // checkpoint before each page.
        let one_more = want.min(lines.len() + 1);
        let Some((piece, next)) = walk.next(&at, |block| pen.cuts(block)) else {
            kept(
                run,
                &pen,
                (from.file, &at),
                (lines, &stack, settled),
                pen.pending(),
            );
            if pen.cut(&mut stack, one_more) {
                finished = true;
                break;
            }
            continue;
        };
        if !pen.holds(&piece) {
            // What the cut lays out is stacked before the piece is.
            kept(
                run,
                &pen,
                (from.file, &at),
                (lines, &stack, settled),
                pen.pending(),
            );
            if !pen.cut(&mut stack, one_more) || !stack.lines.is_empty() {
                continue;
            }
        }
        kept(
            run,
            &pen,
            (from.file, &at),
            (lines, &stack, settled),
            piece.shape.ends().is_some(),
        );
        match piece.shape.ends() {
            Some((first, _)) if measure && pen.measures() && !first => {
                let start = Checkpoint {
                    file: from.file,
                    started: true,
                    place: at.clone(),
                    lines: lines.len(),
                    before: stack.before,
                    settled,
                    tail: None,
                    pen: pen.save(),
                };
                part(&mut pen, run, piece, start, &mut stack);
            }
            Some((first, _)) => {
                run.truth = first;
                pen.piece(piece, &mut stack);
            }
            None => pen.piece(piece, &mut stack),
        }
        at = next;
    }
    lines.append(&mut stack.lines);
    settle(lines, &mut settled, finished);
    let tail = lines.get(settled).and_then(|line| line.loading.clone());
    run.now = Checkpoint {
        file: from.file,
        started: true,
        place: at,
        lines: lines.len(),
        before: stack.before,
        settled,
        tail,
        pen: pen.save(),
    };
    finished
}

/// Stacks `piece`, a stretch of a code block after its first, in a layout
/// that measures, and notes it in `run`: it is only measured, unless it is
/// the last of its block, which is laid out, without colours when what is
/// before it was only measured. `start` is the layout at its start.
fn part<'d>(
    pen: &mut impl Pen<'d>,
    run: &mut Run,
    piece: Piece<'d>,
    start: Checkpoint,
    stack: &mut Stack,
) {
    let Some((_, last)) = piece.shape.ends() else {
        return pen.piece(piece, stack);
    };
    // A block that is not highlighted has nothing to know.
    let known = run.truth || !start.pen.cells.highlighted();
    let truth = known.then(|| start.pen.clone());
    let basis = match known {
        true => Basis::True,
        false => Basis::Plain,
    };
    if last {
        pen.colour(basis);
    }
    match last {
        true => pen.piece(piece, stack),
        false => pen.measure(piece, stack),
    }
    run.truth = false;
    run.parts.push(Part {
        lines: start.lines..stack.len(),
        from: start,
        last,
        laid: last,
        basis,
        truth,
    });
}

/// Puts each band in `lines` from `settled` on where it is drawn: on the
/// blank line after its block when there is one, a little below its top, so
/// that the band takes no row and nothing moves when the image comes;
/// otherwise over the bottom of the block's last line. It starts where the
/// block's text or image does. The last line's band waits for the line that
/// may come below it, unless `end`: then none does. `settled` is how many
/// lines are done.
fn settle(lines: &mut [Line], settled: &mut usize, end: bool) {
    let mut index = *settled;
    while index < lines.len() {
        if lines[index].loading.is_none() {
            index += 1;
            continue;
        }
        if index + 1 == lines.len() && !end {
            break;
        }
        let Some(mut loading) = lines[index].loading.take() else {
            continue;
        };
        let line = &lines[index];
        let mut indent = width(&line.lead);
        if line.image.is_some() {
            indent += width(&line.spans);
        }
        let blank = (lines.get(index + 1)).is_some_and(|below| {
            below.is_blank() && below.lead.is_empty() && below.block.is_none()
        });
        let at = if blank { index + 1 } else { index };
        loading.above += usize::from(blank);
        if let Some(band) = &mut loading.band {
            (band.indent, band.low) = (indent + band.indent, !blank);
        }
        lines[at].loading = Some(loading);
        index = at + 1;
    }
    *settled = index;
}

/// What is laid out: one document, or several files one after another.
#[derive(Debug, Clone, Copy)]
pub enum Content<'a> {
    Document(&'a Document),
    Files(&'a [File]),
}

/// `content` laid out with `inputs`.
pub fn content(content: Content, inputs: &Inputs) -> Stream {
    content_from(content, 0, inputs)
}

/// `content` laid out with `inputs`, the keys of the images of its first
/// document carrying the number `first` and those of the others the
/// numbers after it. The blocks of its files are numbered from 0 all the
/// same.
pub fn content_from(content: Content, first: usize, inputs: &Inputs) -> Stream {
    content_cut(content, first, inputs, stretch::LINES)
}

/// [`content_from`], with the code blocks of more than `size` lines cut in
/// stretches of `size` lines.
pub(crate) fn content_cut(content: Content, first: usize, inputs: &Inputs, size: usize) -> Stream {
    stream(content, (first, 0), inputs, size)
}

/// [`content`], as a [`Laid`].
pub fn laid(content: Content, inputs: &Inputs) -> Laid {
    laid_from(content, 0, inputs)
}

/// [`content_from`], as a [`Laid`].
pub fn laid_from(content: Content, first: usize, inputs: &Inputs) -> Laid {
    let mut laid = begin(content, (first, 0), inputs, stretch::LINES);
    laid.extend(content, inputs, usize::MAX);
    laid
}

/// What the columns of each long table of `content` ask for over all its
/// rows, as [`Inputs::tables`] carries it to the layouts after it, which take
/// the widths of their columns from it: one pass over the rows, a CSV's read
/// from its source as texts without making a cell, a Markdown table's cells
/// laid out. `r` and `:w` measure so in the pager.
pub fn fitted(content: Content, inputs: &Inputs) -> stretch::Tables {
    let derived = Derived::new(inputs);
    let mut tables = stretch::Tables::default();
    for unit in units(content, 0, 0) {
        let mut blocks = Blocks::default();
        let numbers = (unit.number, unit.keys);
        with_cells(inputs, &derived, numbers, &mut blocks, |cells| {
            for (path, block) in unit.document.blocks() {
                if let Block::Table(table) = block
                    && let Some(wants) = cells.wants(table)
                {
                    tables.keep(unit.number, path, wants);
                }
            }
        });
    }
    tables
}

/// One file of a layout, or the lone document: what the driver lays out and
/// how its blocks and images are numbered.
struct Unit<'a> {
    /// The name that a header shows, when files are shown together.
    header: Option<&'a str>,
    document: &'a Document,
    /// The number of its blocks' file.
    number: usize,
    /// The number the keys of its images carry.
    keys: usize,
}

/// The files of `content`, the first numbered `at` and the keys of their
/// images `first` and on from it; a lone document is file 0 and has `first`.
fn units(content: Content<'_>, first: usize, at: usize) -> Vec<Unit<'_>> {
    match content {
        Content::Document(document) => vec![Unit {
            header: None,
            document,
            number: 0,
            keys: first,
        }],
        Content::Files(files) => (files.iter().enumerate())
            .map(|(nth, file)| Unit {
                header: Some(&file.name),
                document: &file.document,
                number: at + nth,
                keys: first + at + nth,
            })
            .collect(),
    }
}

/// The table of the blocks of `content` as the layout numbers them: the
/// header of each file when files are shown together, then every block of
/// the document, each before the blocks inside it. The files are numbered
/// from `at`, and `inputs` say which front matter is not shown.
fn table(content: Content, at: usize, inputs: &Inputs) -> Blocks {
    let mut blocks = Blocks::default();
    for unit in units(content, 0, at) {
        if unit.header.is_some() {
            blocks.add(unit.number, Vec::new());
        }
        for (path, _) in unit.document.blocks() {
            blocks.add(unit.number, path);
        }
        let mut path = Vec::new();
        hide_front_matter(
            &mut blocks,
            unit.number,
            &unit.document.blocks,
            &mut path,
            inputs,
        );
    }
    blocks
}

/// Notes in `blocks` the pieces among `document`'s blocks at `path` whose
/// texts are on no line.
fn hide_front_matter(
    blocks: &mut Blocks,
    number: usize,
    document: &[Block],
    path: &mut Vec<usize>,
    inputs: &Inputs,
) {
    for (index, block) in document.iter().enumerate() {
        path.push(index);
        if texts_hidden(inputs.front_matter, block)
            && let Some(id) = blocks.find(number, path)
        {
            blocks.hide(id);
        }
        if let Block::Details { blocks: inside, .. } = block {
            hide_front_matter(blocks, number, inside, path, inputs);
        }
        path.pop();
    }
}

/// Ends a layout: the pictures of reading mode that it did not show are
/// let go.
fn end(inputs: &Inputs, documents: std::ops::Range<usize>) {
    inputs.images.end_notes(documents);
    if let Some(reading) = &inputs.reading {
        reading.pictures.borrow_mut().finish();
    }
}

/// Lines laid out, with the blocks they come from.
#[derive(Debug, Default)]
pub struct Stream {
    pub lines: Vec<Line>,
    /// The blocks that the lines' [`Line::block`] numbers.
    pub blocks: Blocks,
}

/// `files` laid out one after another, a blank line apart, each as
/// [`file`] lays it out. The blank line between two files has no block,
/// as the blank lines between blocks have none.
pub fn files(files: &[File], inputs: &Inputs) -> Stream {
    files_from(files, 0, inputs, stretch::LINES)
}

/// [`files`], the keys of the images of the first file carrying `first`.
fn files_from(files: &[File], first: usize, inputs: &Inputs, size: usize) -> Stream {
    stream(Content::Files(files), (first, 0), inputs, size)
}

/// `file`, numbered `number` among the files shown together, laid out
/// under a header that names it, as the cell layout's pen draws it, and a
/// blank line. An image file below it is fitted to the window with its
/// header, however many rows a long name takes. The table of its blocks is
/// not kept, so the block numbers of its lines point into nothing;
/// [`files`] keeps the table.
pub fn file(file: &File, number: usize, inputs: &Inputs) -> Vec<Line> {
    let shown = Content::Files(std::slice::from_ref(file));
    stream(shown, (0, number), inputs, stretch::LINES).lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::model::Block;
    use crate::typeset::parse::parse;
    use crate::typeset::style::text;
    use std::rc::Rc;

    fn inputs() -> Inputs {
        Inputs::new(Capabilities::kitty(60))
    }

    fn lone(source: &str) -> Stream {
        content(Content::Document(&parse(source)), &inputs())
    }

    fn file_of(name: &str, source: &str) -> File {
        File {
            name: name.to_owned(),
            document: parse(source),
        }
    }

    /// Each line's text with the path of its block: `None` for a line of
    /// no block, and an empty path for the header of a file.
    fn marked(stream: &Stream) -> Vec<(String, Option<Vec<usize>>)> {
        let table = &stream.blocks;
        stream
            .lines
            .iter()
            .map(|line| {
                let path = line.block.map(|id| table.path(id).to_vec());
                (text(&line.spans), path)
            })
            .collect()
    }

    /// The paths of the blocks of the lines that have text, one for each
    /// run of lines of a block, skipping the blank ones.
    fn paths(stream: &Stream) -> Vec<Vec<usize>> {
        let mut paths: Vec<Vec<usize>> = Vec::new();
        for (text, path) in marked(stream) {
            let Some(path) = path else { continue };
            if !text.trim().is_empty() && paths.last() != Some(&path) {
                paths.push(path);
            }
        }
        paths
    }

    #[test]
    fn a_block_marks_all_its_lines_and_the_blank_between_blocks_none() {
        let stream = lone("One\n\n- a\n\n  b\n\nTwo\n");
        let lines = marked(&stream);
        let blocks: Vec<_> = lines
            .iter()
            .map(|(_, path)| path.as_ref().map(|path| path[0]))
            .collect();
        // The blank line between the two paragraphs of the loose list item
        // is inside the list, so it keeps the list's block.
        let a = lines
            .iter()
            .position(|(text, _)| text.contains('a'))
            .unwrap();
        assert!(stream.lines[a + 1].is_blank(), "{lines:?}");
        assert_eq!(blocks[a + 1], Some(1), "{lines:?}");
        // Blank separators between blocks have no block.
        assert_eq!(blocks.iter().filter(|block| block.is_none()).count(), 2);
        assert_eq!(paths(&stream), [[0], [1], [2]]);
        // Only blank lines go without a block.
        for (text, path) in &lines {
            if path.is_none() {
                assert!(text.is_empty(), "{lines:?}");
            }
        }
    }

    #[test]
    fn two_lines_of_a_block_are_the_same_block_and_two_blocks_are_not() {
        let stream = lone(
            "One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen\n\nNext\n",
        );
        let ids: Vec<_> = stream.lines.iter().filter_map(|line| line.block).collect();
        assert!(ids.len() >= 3, "{ids:?}");
        assert_eq!(ids[0], ids[1]);
        assert_ne!(ids[0], ids[ids.len() - 1]);
    }

    #[test]
    fn a_details_summary_is_the_details_block_and_its_content_is_deeper() {
        let stream = lone(
            "Before\n\n<details>\n<summary>Outer</summary>\n\nIn outer\n\n\
             <details>\n<summary>Inner</summary>\n\nDeep\n\n</details>\n\n\
             Back in outer\n\n</details>\n\nAfter\n",
        );
        assert_eq!(
            paths(&stream),
            [
                vec![0],
                vec![1],
                vec![1, 0],
                vec![1, 1],
                vec![1, 1, 0],
                vec![1, 2],
                vec![2]
            ],
            "{:?}",
            marked(&stream)
        );
        let lines = marked(&stream);
        let path = |find: &str| {
            let (_, path) = lines.iter().find(|(text, _)| text.contains(find)).unwrap();
            path.clone()
        };
        assert_eq!(path("Outer"), Some(vec![1]));
        assert_eq!(path("Inner"), Some(vec![1, 1]));
        assert_eq!(path("Deep"), Some(vec![1, 1, 0]));
    }

    #[test]
    fn a_details_in_reading_mode_keeps_a_place_for_its_summary_and_each_block_in_it() {
        let source = "Before\n\n<details>\n<summary>Outer</summary>\n\nIn outer\n\n\
                      <details>\n<summary>Inner</summary>\n\nDeep\n\n</details>\n\n\
                      Back in outer\n\n</details>\n\nAfter\n";
        let inputs = Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
                snap: false,
            }),
            caps: Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            },
            ..Inputs::new(Capabilities::kitty(80))
        };
        let stream = content(Content::Document(&parse(source)), &inputs);
        let mut seen = Vec::new();
        for id in stream.lines.iter().filter_map(|line| line.block) {
            let path = stream.blocks.path(id).to_vec();
            if seen.last() != Some(&path) {
                seen.push(path);
            }
        }
        assert_eq!(
            seen,
            [
                vec![0],
                vec![1],
                vec![1, 0],
                vec![1, 1],
                vec![1, 1, 0],
                vec![1, 2],
                vec![2]
            ]
        );
    }

    #[test]
    fn blocks_are_found_in_the_source_map_by_their_path() {
        let document = parse(
            "Before\n\n<details>\n<summary>Outer</summary>\n\nIn outer\n\n</details>\n\n- item\n",
        );
        let stream = content(Content::Document(&document), &inputs());
        let mut seen = Vec::new();
        for line in stream.lines.iter().filter_map(|line| line.block) {
            if seen.last() != Some(&line) {
                seen.push(line);
            }
        }
        let lines: Vec<_> = seen
            .iter()
            .map(|&id| document.places.line(stream.blocks.get(id).path()))
            .collect();
        assert_eq!(lines, [Some(1), Some(3), Some(6), Some(10)]);
    }

    #[test]
    fn the_lines_of_blocks_are_those_of_their_own_file() {
        let shown = [
            file_of("a.md", "One\n\nTwo\n"),
            file_of("b.md", "\n\nThree\n"),
        ];
        let stream = files(&shown, &inputs());
        let line_of = |find: &str| {
            let line = stream
                .lines
                .iter()
                .find(|line| text(&line.spans).contains(find))
                .unwrap();
            let info = stream.blocks.get(line.block.unwrap());
            shown[info.file()].document.places.line(info.path())
        };
        assert_eq!(line_of("Two"), Some(3));
        assert_eq!(line_of("Three"), Some(3));
        // The header of a file is not in the source.
        let header = stream.lines[0].block.unwrap();
        let info = stream.blocks.get(header);
        assert_eq!(shown[info.file()].document.places.line(info.path()), None);
    }

    #[test]
    fn files_mark_their_headers_with_the_file() {
        let shown = [file_of("a.md", "One\n\nTwo\n"), file_of("b.md", "Three\n")];
        let stream = files(&shown, &inputs());
        let table = &stream.blocks;
        let file_of_line = |line: &Line| line.block.map(|id| table.get(id).file());
        let start = stream
            .lines
            .iter()
            .position(|line| file_of_line(line) == Some(1))
            .unwrap();
        // The header, up to the first block, has the file's entry.
        let header = &stream.lines[..stream
            .lines
            .iter()
            .position(|line| text(&line.spans).contains("One"))
            .unwrap()
            - 1];
        assert!(header.len() >= 3);
        for line in header {
            assert_eq!(file_of_line(line), Some(0));
            assert_eq!(table.path(line.block.unwrap()), &[] as &[usize]);
        }
        // The blank line before the second file has no block.
        assert!(stream.lines[start - 1].is_blank());
        assert_eq!(stream.lines[start - 1].block, None);
        // The second file's header and block.
        assert_eq!(file_of_line(&stream.lines[start]), Some(1));
        let three = stream
            .lines
            .iter()
            .find(|line| text(&line.spans).contains("Three"))
            .unwrap();
        assert_eq!(file_of_line(three), Some(1));
        assert_eq!(table.path(three.block.unwrap()), [0]);
        // The blank line between a header and the first block has none.
        let first = stream
            .lines
            .iter()
            .position(|line| text(&line.spans).contains("One"))
            .unwrap();
        assert_eq!(stream.lines[first - 1].block, None);
    }

    /// The paths the layout records for a document with blocks nested in
    /// every kind of container are paths of blocks of the document, as
    /// `compare` counts them.
    #[test]
    fn the_paths_of_the_blocks_table_are_paths_of_the_documents_blocks() {
        let document = parse(crate::typeset::compare::NESTED);
        let known: Vec<Vec<usize>> = crate::typeset::compare::blocks(&document)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        let reading = Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
                snap: false,
            }),
            caps: Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            },
            ..Inputs::new(Capabilities::kitty(80))
        };
        for inputs in [inputs(), reading] {
            let stream = content(Content::Document(&document), &inputs);
            let mut recorded = 0;
            for line in &stream.lines {
                if let Some(id) = line.block {
                    let path = stream.blocks.path(id);
                    recorded += 1;
                    assert!(known.iter().any(|known| known == path), "{path:?}");
                }
            }
            assert!(recorded > 0);
        }
    }

    /// The lines of the fixture and its table agree: the blocks are in
    /// order, each is used, and all belong to the one document.
    fn assert_agree(fixture: &str) {
        let source = std::fs::read_to_string(format!("tests/fixtures/{fixture}")).unwrap();
        let stream = lone(&source);
        let mut seen = Vec::new();
        for line in &stream.lines {
            if let Some(id) = line.block {
                assert_eq!(stream.blocks.get(id).file(), 0);
                if seen.last() != Some(&id) {
                    seen.push(id);
                }
            }
        }
        let paths: Vec<_> = seen.iter().map(|&id| stream.blocks.path(id)).collect();
        // Each block is one run of lines, so none is seen twice, and the
        // blocks follow each other as the document does.
        for (number, path) in paths.iter().enumerate() {
            assert!(!paths[..number].contains(path), "{fixture}: {path:?}");
            assert!(!path.is_empty());
        }
        assert!(
            paths.windows(2).all(|pair| pair[0] < pair[1]),
            "{fixture}: {paths:?}"
        );
        // Every block of the table is a block of the document, in the
        // document's order, and the innermost block of some line unless it
        // holds blocks that have all its lines, or is not shown.
        let known: Vec<Vec<usize>> = (parse(&source).blocks().into_iter())
            .map(|(path, _)| path)
            .collect();
        let table: Vec<&[usize]> = (stream.blocks.ids())
            .map(|id| stream.blocks.path(id))
            .collect();
        assert!(
            table.windows(2).all(|pair| pair[0] < pair[1]),
            "{fixture}: {table:?}"
        );
        let inner: Vec<_> = stream.lines.iter().filter_map(|line| line.inner).collect();
        for id in stream.blocks.ids() {
            let path = stream.blocks.path(id);
            assert!(
                known.iter().any(|known| known == path),
                "{fixture}: {path:?}"
            );
            // A block whose lines are all of the blocks inside it, such as a
            // list, is the innermost block of none.
            let holds_blocks = table
                .iter()
                .any(|at| at.len() > path.len() && at.starts_with(path));
            // An empty heading and an empty code block have no lines.
            let empty = (parse(&source).blocks().into_iter()).any(|(at, block)| {
                at == path
                    && match block {
                        Block::Heading { content, .. } => content.is_empty(),
                        Block::CodeBlock { code, .. } => code.is_empty(),
                        _ => false,
                    }
            });
            assert!(
                inner.contains(&id) || holds_blocks || empty || stream.blocks.is_hidden(id),
                "{fixture}: {path:?} is the innermost block of no line"
            );
        }
    }

    #[test]
    fn the_table_and_the_lines_agree_for_blocks_in_quotes_and_lists() {
        assert_agree("nested.md");
    }

    #[test]
    fn the_table_and_the_lines_agree_for_html_tags() {
        assert_agree("html-tags.md");
    }

    /// Reading mode with fixture fonts and cells of 10 × 20 pixels.
    fn reading() -> Inputs {
        Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
                snap: false,
            }),
            caps: Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            },
            ..Inputs::new(Capabilities::kitty(80))
        }
    }

    fn pages_of(stream: &Stream) -> usize {
        crate::typeset::reading::engine::pages(&stream.lines).len()
    }

    #[test]
    fn pieces_are_stacked_a_blank_line_apart_and_a_piece_without_lines_gets_none() {
        let stream = lone("One\n\n#\n\nTwo\n");
        let texts: Vec<String> = stream.lines.iter().map(|l| text(&l.spans)).collect();
        assert_eq!(texts, ["One", "", "Two"], "{texts:?}");
    }

    #[test]
    fn in_reading_mode_a_heading_a_paragraph_and_a_details_with_its_blocks_are_one_page() {
        let source =
            "# Title\n\nText.\n\n<details>\n<summary>S</summary>\n\nInside.\n\n</details>\n";
        let stream = content(Content::Document(&parse(source)), &reading());
        assert_eq!(pages_of(&stream), 1);
    }

    #[test]
    fn an_error_between_two_reading_documents_is_cells_and_each_document_is_a_page() {
        let shown = [
            file_of("a.md", "One\n"),
            File {
                name: "b.md".to_owned(),
                document: crate::typeset::model::Document::new(vec![Block::Error("bad".into())]),
            },
            file_of("c.md", "Three\n"),
        ];
        let stream = files(&shown, &reading());
        let error = stream
            .lines
            .iter()
            .find(|line| text(&line.spans).contains("bad"))
            .unwrap();
        assert!(error.image.is_none(), "{error:?}");
        // The document before the error and the one after are pages of
        // their own, and the error is not on either.
        assert_eq!(pages_of(&stream), 2);
    }

    #[test]
    fn the_header_of_a_file_is_cells_in_reading_mode_too() {
        let shown = [file_of("a.md", "One\n")];
        let stream = files(&shown, &reading());
        let header = stream
            .lines
            .iter()
            .find(|l| text(&l.spans).contains("a.md"));
        assert!(
            header.is_some_and(|line| line.image.is_none()),
            "{header:?}"
        );
    }

    #[test]
    fn a_band_settles_on_the_last_line_of_its_file_and_not_on_the_blank_line_between_files() {
        use crate::typeset::images::Placeholder;
        let mut inputs = Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        });
        let key = inputs.drawn("a.png");
        let pixels = crate::typeset::images::Source::Raster(::image::RgbaImage::new(40, 30));
        inputs.images.wait(key, Placeholder::Dimmed(pixels));
        let shown = [file_of("a.md", "![a](a.png)\n"), file_of("b.md", "Two\n")];
        let stream = files(&shown, &inputs);
        let at = stream
            .lines
            .iter()
            .position(|line| line.loading.is_some())
            .unwrap();
        assert!(stream.lines[at].image.is_some(), "{:?}", stream.lines[at]);
        assert!(stream.lines[at + 1].is_blank() && stream.lines[at + 1].loading.is_none());
    }

    /// The table is the header, if there is one, and then every block of
    /// the document in its order, whichever engine lays it out.
    #[test]
    fn the_table_is_the_header_and_the_blocks_of_the_document_in_both_engines() {
        let mut seen = 0;
        for entry in crate::testing::documents() {
            let source = std::fs::read_to_string(&entry).unwrap();
            let document = parse(&source);
            let known: Vec<Vec<usize>> = document.blocks().into_iter().map(|(p, _)| p).collect();
            for inputs in [inputs(), reading()] {
                let stream = content(Content::Document(&document), &inputs);
                let table: Vec<Vec<usize>> = stream
                    .blocks
                    .ids()
                    .map(|id| stream.blocks.path(id).to_vec())
                    .collect();
                assert_eq!(table, known, "{entry:?}");
                let shown = [File {
                    name: "a.md".to_owned(),
                    document: parse(&source),
                }];
                let stream = files(&shown, &inputs);
                let table: Vec<Vec<usize>> = stream
                    .blocks
                    .ids()
                    .map(|id| stream.blocks.path(id).to_vec())
                    .collect();
                let mut with_header = vec![vec![]];
                with_header.extend(known.clone());
                assert_eq!(table, with_header, "{entry:?} as a file");
            }
            seen += 1;
        }
        assert!(seen > 10);
    }

    #[test]
    fn hidden_front_matter_is_in_the_table_as_hidden_and_has_no_lines_in_both_engines() {
        use crate::render::FrontMatterStyle;
        for mut inputs in [inputs(), reading()] {
            inputs.front_matter = FrontMatterStyle::Hide;
            let stream = content(
                Content::Document(&parse("---\ntitle: A\n---\n\nText.\n")),
                &inputs,
            );
            let first = stream.blocks.ids().next().unwrap();
            assert_eq!(stream.blocks.path(first), [0]);
            assert!(stream.blocks.is_hidden(first));
            assert!(stream.lines.iter().all(|line| line.block != Some(first)));
        }
    }

    #[test]
    fn blank_front_matter_is_not_hidden_in_the_table_or_the_code_style_in_both_engines() {
        use crate::render::FrontMatterStyle;
        for style in [FrontMatterStyle::Table, FrontMatterStyle::Code] {
            for mut inputs in [inputs(), reading()] {
                inputs.front_matter = style;
                let stream = content(Content::Document(&parse("---\n---\n\nText.\n")), &inputs);
                let first = stream.blocks.ids().next().unwrap();
                assert_eq!(stream.blocks.path(first), [0]);
                assert!(
                    !stream.blocks.is_hidden(first),
                    "blank front matter is hidden as {style:?}"
                );
            }
        }
    }

    /// The keys of the images that load, in the lines of `stream`.
    fn loading_keys(stream: &Stream) -> Vec<crate::typeset::images::key::ImageKey> {
        let loading = stream.lines.iter().filter_map(|line| line.loading.as_ref());
        loading.map(|loading| loading.key.clone()).collect()
    }

    /// Inputs in which the image `a.png` of each of the documents
    /// numbered `numbers` loads.
    fn loading_in(numbers: &[usize]) -> Inputs {
        use crate::typeset::images::key::ImageKey;
        use crate::typeset::images::store::Placeholder;
        let mut inputs = inputs();
        for &number in numbers {
            let pixels = image::RgbaImage::from_pixel(40, 30, image::Rgba([9, 9, 9, 255]));
            let key = ImageKey::from("a.png").in_document(number);
            let placeholder =
                Placeholder::Dimmed(crate::typeset::images::decode::Source::Raster(pixels));
            inputs.images.wait(key, placeholder);
        }
        inputs
    }

    #[test]
    fn content_from_numbers_the_image_keys_from_first_and_the_blocks_from_0() {
        use crate::typeset::images::key::ImageKey;
        let files = [
            file_of("a.md", "![a](a.png)\n"),
            file_of("b.md", "![a](a.png)\n"),
        ];
        let inputs = loading_in(&[7, 8]);
        let stream = content_from(Content::Files(&files), 7, &inputs);
        let keys = loading_keys(&stream);
        let expected = [7, 8].map(|number| ImageKey::from("a.png").in_document(number));
        assert_eq!(keys, expected);
        let mut at_files: Vec<usize> = (stream.lines.iter())
            .filter_map(|line| line.block)
            .map(|id| stream.blocks.get(id).file())
            .collect();
        at_files.dedup();
        assert_eq!(at_files, [0, 1]);
        // A lone document's keys carry `first` too.
        let document = parse("![a](a.png)\n");
        let inputs = loading_in(&[3]);
        let stream = content_from(Content::Document(&document), 3, &inputs);
        let expected = [ImageKey::from("a.png").in_document(3)];
        assert_eq!(loading_keys(&stream), expected);
        assert!(
            stream
                .lines
                .iter()
                .filter_map(|line| line.block)
                .all(|id| stream.blocks.get(id).file() == 0)
        );
    }

    /// Rust code of `count` lines with what a layout in stretches must get
    /// right: a comment that runs over the seams at 512 and 1024 lines,
    /// tabs, a line that wraps and one of no text.
    fn rust(count: usize) -> String {
        let mut code = String::new();
        for line in 0..count {
            let text = match line % 1024 {
                508 | 1020 => "/* a comment that goes on".to_owned(),
                509..=514 | 1021..=1026 => "   still the comment".to_owned(),
                515 | 1027 => "   the end */ fn after() {}".to_owned(),
                _ if line % 97 == 0 => {
                    format!("\tlet tabbed{line} = \"a\\tb\"; // {}", "word ".repeat(20))
                }
                _ if line % 89 == 0 => String::new(),
                _ => format!("fn f{line}() {{ let x = {line}; }}"),
            };
            code.push_str(&text);
            code.push('\n');
        }
        code
    }

    fn code_block(language: Option<&str>, code: &str, whole_file: bool) -> Document {
        crate::typeset::model::Document::new(vec![Block::CodeBlock {
            language: language.map(str::to_owned),
            code: code.into(),
            whole_file,
        }])
    }

    /// A document of a paragraph, the code block and a paragraph.
    fn between(block: Document) -> Document {
        let mut blocks = parse("Before.\n").blocks;
        blocks.extend(block.blocks);
        blocks.extend(parse("After.\n").blocks);
        crate::typeset::model::Document::new(blocks)
    }

    /// The layout of `document` with its code blocks cut in stretches of
    /// `size` lines.
    fn cut(document: &Document, inputs: &Inputs, size: usize) -> Stream {
        content_cut(Content::Document(document), 0, inputs, size)
    }

    /// What a pen is given: the kind of each piece, and for a stretch
    /// whether it is the first and the last.
    #[derive(Default)]
    struct Recorder {
        seen: Vec<String>,
    }

    impl<'d> Pen<'d> for Recorder {
        fn piece(&mut self, piece: Piece<'d>, _: &mut Stack) {
            self.seen.push(match piece.shape {
                Shape::Block(_) => format!("block {:?}", piece.path),
                Shape::Stretch(stretch) => format!(
                    "stretch {:?} at {} {}{}",
                    piece.path,
                    stretch.offset,
                    if stretch.first { "first" } else { "" },
                    if stretch.last { "last" } else { "" },
                ),
                Shape::Summary { .. } => format!("summary {:?}", piece.path),
                Shape::Header(_) => "header".to_owned(),
                Shape::Rows(rows) => format!("rows {:?} {:?}", piece.path, rows.rows()),
            });
        }
        fn cut(&mut self, _: &mut Stack, _: usize) -> bool {
            true
        }
        fn cuts(&self, _: &Block) -> bool {
            true
        }
        fn save(&self) -> Saved {
            Saved::default()
        }
        fn restore(&mut self, _: &Saved, _: &Walk<'d>) {}
    }

    fn pieces_of(document: &Document, size: usize) -> Vec<String> {
        let mut pen = Recorder::default();
        let mut stack = Stack::default();
        let walk = Walk::new(None, &document.blocks, size);
        let mut at = Place::start(false);
        while let Some((piece, next)) = walk.next(&at, |block| pen.cuts(block)) {
            pen.piece(piece, &mut stack);
            at = next;
        }
        pen.seen
    }

    #[test]
    fn a_large_code_block_is_handed_to_the_pen_as_stretches_and_a_small_one_whole() {
        let code = "a\n".repeat(2000);
        let document = between(code_block(Some("rust"), &code, false));
        assert_eq!(
            pieces_of(&document, 512),
            [
                "block [0]".to_owned(),
                "stretch [1] at 0 first".to_owned(),
                "stretch [1] at 1024 ".to_owned(),
                "stretch [1] at 2048 ".to_owned(),
                "stretch [1] at 3072 last".to_owned(),
                "block [2]".to_owned(),
            ]
        );
        // Exactly 512 lines is not cut; a block of one stretch is a block.
        let code = "a\n".repeat(512);
        let document = code_block(Some("rust"), &code, false);
        assert_eq!(pieces_of(&document, 512), ["block [0]"]);
        // Inside a `<details>` the block is cut as well.
        let code = "a\n".repeat(30);
        let source =
            format!("<details>\n<summary>S</summary>\n\n```rust\n{code}```\n\n</details>\n");
        let seen = pieces_of(&parse(&source), 10);
        assert_eq!(seen[0], "summary [0]");
        assert_eq!(seen.len(), 4, "{seen:?}");
        assert!(seen[1].starts_with("stretch [0, 0] at 0 first"), "{seen:?}");
        assert!(seen[3].ends_with("last"), "{seen:?}");
    }

    /// What each span of a line's `lead`, `spans` and `trail` shows of the
    /// document. `Origins` compares equal whatever it holds, so `Line`'s own
    /// equality does not see it.
    fn origins(line: &Line) -> Vec<Vec<crate::typeset::style::Origin>> {
        let spans = line.lead.iter().chain(&line.spans).chain(&line.trail);
        spans
            .map(|span| span.origins.into_iter().cloned().collect())
            .collect()
    }

    /// A block in stretches and the same block whole: their lines (spans and
    /// styles, block ids), what each span shows of the document, and the
    /// table of blocks are equal, and the table has the block once.
    fn assert_stretches_equal_whole(document: &Document, inputs: &Inputs, size: usize) {
        let whole = cut(document, inputs, usize::MAX);
        let stretched = cut(document, inputs, size);
        assert_eq!(stretched.lines.len(), whole.lines.len());
        for (at, (a, b)) in stretched.lines.iter().zip(&whole.lines).enumerate() {
            assert_eq!(a, b, "line {at}");
            assert_eq!(origins(a), origins(b), "origins of line {at}");
        }
        assert_eq!(stretched.blocks, whole.blocks);
        let entries = stretched.blocks.ids().count();
        assert_eq!(entries, document.blocks.len());
        // A blank line has no block. The only ones are before and after the
        // code block, which has none inside.
        let blank: Vec<usize> = (0..stretched.lines.len())
            .filter(|&at| stretched.lines[at].block.is_none())
            .collect();
        assert_eq!(blank.len(), document.blocks.len() - 1, "{blank:?}");
    }

    fn no_colours(width: u16) -> Inputs {
        let caps = Capabilities {
            color: false,
            ..Capabilities::kitty(width)
        };
        Inputs::new(caps)
    }

    #[test]
    fn a_highlighted_block_in_stretches_is_the_block_whole() {
        let code = rust(2000);
        for size in [512, 100, 7] {
            let fenced = between(code_block(Some("rust"), &code, false));
            assert_stretches_equal_whole(&fenced, &inputs(), size);
        }
        // With its label and box.
        let fenced = between(code_block(Some("rust"), &code, false));
        let stream = cut(&fenced, &inputs(), 512);
        let label = text(&stream.lines[2].spans);
        assert!(label.trim_end().ends_with("rust"), "{label:?}");
        let last = text(&stream.lines[stream.lines.len() - 3].spans);
        assert!(last.trim().is_empty(), "{last:?}");
    }

    #[test]
    fn a_whole_file_and_an_uncoloured_block_in_stretches_are_the_block_whole() {
        let code = rust(1300);
        for language in [Some("rust"), None] {
            let file = code_block(language, &code, true);
            assert_stretches_equal_whole(&file, &inputs(), 512);
        }
        let plain = between(code_block(Some("rust"), &code, false));
        assert_stretches_equal_whole(&plain, &no_colours(60), 512);
        let unknown = between(code_block(None, &code, false));
        assert_stretches_equal_whole(&unknown, &inputs(), 400);
    }

    #[test]
    fn a_failing_line_leaves_the_block_in_its_geometry_and_stretches_equal_whole() {
        let mut lines: Vec<String> = rust(1500).lines().map(str::to_owned).collect();
        // A NUL makes the highlighter fail in a test build.
        lines[700].push('\0');
        let code = lines.join("\n") + "\n";
        for size in [512, 64] {
            let document = between(code_block(Some("rust"), &code, false));
            assert_stretches_equal_whole(&document, &inputs(), size);
        }
        // The lines from the error on have no colours, and the box and its
        // label stay: every row is as wide as the block.
        let document = between(code_block(Some("rust"), &code, false));
        let stream = cut(&document, &inputs(), 512);
        // The spans that have a colour: the code, in one piece or in many.
        let colours = |line: &Line| line.spans.iter().filter(|s| s.style.fg.is_some()).count();
        let row = |text: &str| {
            let at = stream
                .lines
                .iter()
                .position(|line| crate::typeset::style::text(&line.spans).contains(text));
            &stream.lines[at.unwrap()]
        };
        assert!(colours(row("fn f699()")) > 1);
        assert_eq!(colours(row("fn f701()")), 1);
        let label = text(&stream.lines[2].spans);
        assert!(label.trim_end().ends_with("rust"), "{label:?}");
        let width = crate::typeset::wrap::width(&stream.lines[2].spans);
        assert!(
            stream.lines[2..stream.lines.len() - 2]
                .iter()
                .all(|line| { crate::typeset::wrap::width(&line.spans) == width })
        );
    }

    #[test]
    fn a_diagram_is_never_cut() {
        use crate::typeset::images::{ImageKey, Placeholder};
        let code = "graph TD\n".to_owned() + &"  A --> B\n".repeat(900);
        let document = code_block(Some("mermaid"), &code, false);
        let mut inputs = inputs();
        let key = inputs.drawn(ImageKey::rendered("mermaid", &code));
        inputs.images.wait(key, Placeholder::Rows(3));
        let stretched = cut(&document, &inputs, 100);
        assert_eq!(stretched.lines.len(), 3);
        assert_eq!(stretched.blocks.ids().count(), 1);
        assert_eq!(stretched.lines, cut(&document, &inputs, usize::MAX).lines);
    }

    #[test]
    fn the_reason_a_diagram_failed_goes_below_the_last_stretch_only() {
        use crate::render::RenderErrors;
        use crate::typeset::images::{ImageError, ImageKey};
        let code = "graph TD\n".to_owned() + &"  A --> B\n".repeat(900);
        let document = code_block(Some("mermaid"), &code, false);
        let mut inputs = Inputs {
            render_errors: RenderErrors::Both,
            ..inputs()
        };
        let key = inputs.drawn(ImageKey::rendered("mermaid", &code));
        let error = ImageError::Diagram {
            name: "diagram".into(),
            reason: "no program".into(),
        };
        inputs.images.insert(key, Err(error));
        assert_stretches_equal_whole(&document, &inputs, 100);
        let stretched = cut(&document, &inputs, 100);
        let texts: Vec<String> = stretched.lines.iter().map(|l| text(&l.spans)).collect();
        let told = |text: &String| text.contains("could not be drawn");
        assert_eq!(texts.iter().filter(|text| told(text)).count(), 1);
        assert!(told(texts.last().unwrap()), "{:?}", texts.last());
    }

    #[test]
    fn a_loading_band_goes_on_the_last_stretch_and_covers_the_lines_above_it_in_the_block() {
        use crate::typeset::images::{ImageKey, Placeholder, Source};
        let code = "graph TD\n".to_owned() + &"  A --> B\n".repeat(300);
        let document = code_block(Some("mermaid"), &code, false);
        let mut inputs = inputs();
        let key = inputs.drawn(ImageKey::rendered("mermaid", &code));
        // An image without size is not placed, so the code is shown.
        let empty = Source::Raster(image::RgbaImage::new(0, 0));
        inputs.images.wait(key, Placeholder::Dimmed(empty));
        let whole = cut(&document, &inputs, usize::MAX);
        let loading: Vec<usize> = (0..whole.lines.len())
            .filter(|&at| whole.lines[at].loading.is_some())
            .collect();
        assert_eq!(
            loading,
            [whole.lines.len() - 1],
            "the block is shown as code"
        );
        assert_stretches_equal_whole(&document, &inputs, 100);
    }

    #[test]
    fn a_range_across_two_stretches_is_found_as_in_the_block_whole() {
        use crate::typeset::locate::Map;
        use crate::typeset::position::Pos;
        let code = rust(1100);
        let document = code_block(Some("rust"), &code, false);
        let (whole, stretched) = (
            cut(&document, &inputs(), usize::MAX),
            cut(&document, &inputs(), 512),
        );
        let (a, b) = (
            Map::new(&whole.lines, &whole.blocks),
            Map::new(&stretched.lines, &stretched.blocks),
        );
        // From the last line of the first stretch to the first line of the
        // second: lines 511 and 512 of the code.
        let at =
            |line: usize| -> usize { code.split_inclusive('\n').take(line).map(str::len).sum() };
        let range = Pos::text(0, &[0], 0, at(511) + 3)..Pos::text(0, &[0], 0, at(513) - 2);
        let found = |map: &Map, lines: &[Line]| map.locate(lines, &range).unwrap();
        let (in_whole, in_stretches) = (found(&a, &whole.lines), found(&b, &stretched.lines));
        assert_eq!(in_whole, in_stretches);
        let rows: usize = in_stretches.iter().map(|part| part.rows.len()).sum();
        assert!(rows >= 2, "{in_stretches:?}");
        // Each row of the seam says where it is.
        for row in 0..whole.lines.len() {
            for column in [0, 2, 9] {
                assert_eq!(
                    a.hit(&whole.lines, row, column),
                    b.hit(&stretched.lines, row, column),
                    "row {row} column {column}"
                );
            }
        }
        assert_eq!(a.line_of(&range.start), b.line_of(&range.start));
        assert_eq!(a.line_of(&range.end), b.line_of(&range.end));
        assert!(b.line_of(&range.start).is_some());
        assert_eq!(a.line_bytes(&range), b.line_bytes(&range));
    }

    #[test]
    fn content_from_0_is_content() {
        let files = [
            file_of("a.md", "One\n\n![a](a.png)\n"),
            file_of("b.md", "![a](a.png)\n"),
        ];
        let inputs = loading_in(&[0, 1]);
        let (from, plain) = (
            content_from(Content::Files(&files), 0, &inputs),
            content(Content::Files(&files), &inputs),
        );
        assert_eq!(marked(&from), marked(&plain));
        assert_eq!(loading_keys(&from), loading_keys(&plain));
        assert_eq!(loading_keys(&from).len(), 2);
    }

    /// The table that `table` makes before anything is laid out is the one
    /// the layout makes, whichever engine lays it out and however front
    /// matter shows.
    #[test]
    fn the_table_of_blocks_is_known_before_anything_is_laid_out() {
        use crate::render::FrontMatterStyle;
        let mut seen = 0;
        let mut documents = vec!["---\ntitle: A\nlist: [1, 2]\n---\n\nText.\n".to_owned()];
        for entry in crate::testing::documents() {
            documents.push(std::fs::read_to_string(&entry).unwrap());
        }
        for source in &documents {
            let document = parse(source);
            let shown = [file_of("a.md", source), file_of("b.md", "Two\n\n- x\n")];
            for mut inputs in [inputs(), reading()] {
                for style in [
                    FrontMatterStyle::Table,
                    FrontMatterStyle::Code,
                    FrontMatterStyle::Hide,
                ] {
                    inputs.front_matter = style;
                    let lone = content(Content::Document(&document), &inputs);
                    assert_eq!(
                        table(Content::Document(&document), 0, &inputs),
                        lone.blocks,
                        "{style:?}"
                    );
                    let several = content(Content::Files(&shown), &inputs);
                    assert_eq!(
                        table(Content::Files(&shown), 0, &inputs),
                        several.blocks,
                        "{style:?} as files"
                    );
                    seen += 1;
                }
            }
        }
        assert!(seen > 60);
    }

    /// `laid` extended to its end, with `inputs`.
    fn whole(content: Content, inputs: &Inputs, size: usize) -> Laid {
        let mut laid = begin(content, (0, 0), inputs, size);
        assert!(laid.extend(content, inputs, usize::MAX));
        assert!(laid.is_whole());
        laid
    }

    /// `a` and `b` have the same lines, with what each shows of the
    /// document, and the same table of blocks.
    fn assert_same(a: &Laid, b: &Laid, what: &str) {
        assert_eq!(a.len(), b.len(), "{what}: lines");
        for at in 0..a.len() {
            assert_eq!(a.lines()[at], b.lines()[at], "{what}: line {at}");
            assert_eq!(
                origins(&a.lines()[at]),
                origins(&b.lines()[at]),
                "{what}: origins of line {at}"
            );
        }
        assert_eq!(a.blocks(), b.blocks(), "{what}: table");
    }

    #[test]
    fn a_stretch_laid_out_again_from_its_checkpoint_equals_itself() {
        let code = rust(520);
        for size in [200, 64] {
            let document = between(code_block(Some("rust"), &code, false));
            let content = Content::Document(&document);
            let inputs = inputs();
            let eager = whole(content, &inputs, size);
            let mut laid = whole(content, &inputs, size);
            let total = laid.len();
            let mut rewound_far = false;
            for line in (0..=total + 3)
                .step_by(97)
                .chain([1, size, size + 1, total - 1])
            {
                laid.restart(content, line);
                let from = laid.len();
                assert!(from <= line.min(total), "restart({line}) is at {from}");
                assert!(!laid.is_whole(), "restart({line})");
                // What is kept is the layout's own lines.
                for at in 0..from {
                    assert_eq!(laid.lines()[at], eager.lines()[at], "restart({line}): {at}");
                }
                // What the layout has not laid out again is not told as laid
                // out, and the rest is told as the whole layout tells it.
                let mut not_laid = 0;
                for range in crate::steps::ranges(content) {
                    match laid.locate(&range) {
                        Err(crate::typeset::locate::Hidden::NotLaidOut) => not_laid += 1,
                        found => assert_eq!(found, eager.locate(&range), "restart({line})"),
                    }
                }
                assert!(not_laid > 0, "restart({line}) at {from}: all is laid out");
                rewound_far |= from > 2 * size;
                assert!(laid.extend(content, &inputs, usize::MAX));
                assert_same(&laid, &eager, &format!("size {size}: restart({line})"));
            }
            assert!(rewound_far, "no restart began after the second stretch");
        }
    }

    #[test]
    fn a_restart_goes_back_to_the_start_of_the_stretch_not_the_top() {
        let code = rust(2000);
        let document = code_block(Some("rust"), &code, false);
        let content = Content::Document(&document);
        let inputs = inputs();
        let mut laid = whole(content, &inputs, 512);
        let total = laid.len();
        laid.restart(content, total - 1);
        // The last stretch has the 2000 - 3 * 512 = 464 last lines and the
        // box's bottom row; the checkpoint is where it starts.
        assert!(
            laid.len() > total - 520 && laid.len() < total - 400,
            "{}",
            laid.len()
        );
        laid.restart(content, 0);
        assert_eq!(laid.len(), 0);
    }

    #[test]
    fn an_image_that_arrives_after_a_stretch_does_not_lay_the_stretches_out_again() {
        let code = rust(1300);
        let mut blocks = parse("![a](a.png)\n").blocks;
        blocks.extend(code_block(Some("rust"), &code, false).blocks);
        blocks.extend(parse("Between.\n\n![a](a.png)\n\nAfter.\n").blocks);
        let document = Document::new(blocks);
        let content = Content::Document(&document);
        let (before, after) = (inputs(), loading_in(&[0]));
        let want = whole(content, &after, 512);
        let mut laid = whole(content, &before, 512);
        assert_ne!(laid.lines(), want.lines(), "the image changes the lines");
        // The second image is where the loading is told, on the last lines.
        let at = (0..want.len())
            .rev()
            .find(|&at| want.lines()[at].loading.is_some());
        let at = at.expect("a loading band");
        laid.restart(content, at);
        assert!(laid.len() > 1000, "restarted at {}", laid.len());
        assert!(laid.extend(content, &after, usize::MAX));
        // The lines before the restart are the first layout's: the first image
        // is still laid out as it was, which is one line fewer than it takes
        // now. The lines from the restart on are the new layout's.
        let fewer = want.len() - laid.len();
        assert_eq!(fewer, 1);
        for line in 1000..laid.len() {
            assert_eq!(
                laid.lines()[line],
                want.lines()[line + fewer],
                "line {line}"
            );
        }
        assert_eq!(laid.blocks(), want.blocks());
        // From the top, the first image is changed too.
        laid.restart(content, 0);
        assert!(laid.extend(content, &after, usize::MAX));
        assert_same(&laid, &want, "restarted from the top");
    }

    #[test]
    fn a_band_waiting_for_the_line_below_it_is_kept_by_a_checkpoint() {
        // The image's last row has a band that settles on the blank row
        // that the next piece brings; a checkpoint between them keeps it.
        let code = rust(90);
        let mut blocks = parse("![a](a.png)\n").blocks;
        blocks.extend(code_block(Some("rust"), &code, false).blocks);
        blocks.extend(parse("![a](a.png)\n").blocks);
        let document = Document::new(blocks);
        let content = Content::Document(&document);
        let inputs = loading_in(&[0]);
        let eager = whole(content, &inputs, 30);
        assert!(
            eager
                .lines()
                .iter()
                .filter(|line| line.loading.is_some())
                .count()
                >= 2
        );
        let mut laid = whole(content, &inputs, 30);
        for line in 0..=eager.len() {
            laid.restart(content, line);
            assert!(laid.extend(content, &inputs, usize::MAX));
            assert_same(&laid, &eager, &format!("restart({line})"));
        }
    }

    #[test]
    fn a_layout_in_reading_mode_goes_on_from_a_checkpoint_between_pages() {
        let long: String = (0..4000)
            .map(|n| format!("Paragraph {n} of a long document, with words to wrap.\n\n"))
            .collect();
        let document = parse(&long);
        let content = Content::Document(&document);
        let inputs = reading();
        let eager = whole(content, &inputs, 512);
        // The checkpoints are the starts of the pages, and a restart goes back
        // to the last at or before the line.
        let starts: Vec<usize> = crate::typeset::reading::engine::pages(eager.lines())
            .iter()
            .map(|page| page.lines.start)
            .collect();
        assert!(starts.len() > 2, "{starts:?}");
        let mut laid = whole(content, &inputs, 512);
        let total = laid.len();
        let mut not_the_top = 0;
        for line in [
            0,
            1,
            starts[1] - 1,
            starts[1],
            starts[1] + 5,
            total / 2,
            total - 1,
        ] {
            let want = starts.iter().copied().filter(|&at| at <= line).max();
            laid.restart(content, line);
            assert_eq!(
                laid.len(),
                want.unwrap_or(0),
                "restart({line}) of {starts:?}"
            );
            not_the_top += usize::from(!laid.is_empty());
            assert!(laid.extend(content, &inputs, usize::MAX));
            assert_same(&laid, &eager, &format!("restart({line})"));
        }
        assert!(not_the_top >= 3);
    }

    /// Files with images that are loading, and a code block in stretches
    /// between them: the bands settle on the lines they settle on, wherever
    /// the steps stop.
    #[test]
    fn bands_settle_as_they_do_whole_wherever_the_steps_stop() {
        use crate::steps::{alike, assert_in_steps};
        let code = rust(700);
        let first =
            format!("![a](a.png)\n\n```rust\n{code}```\n\nTwo ![a](a.png)\n\n![a](a.png)\n");
        let shown = [file_of("a.md", &first), file_of("b.md", "![a](a.png)\n")];
        let inputs = loading_in(&[0, 1]);
        assert_in_steps("files with bands", Content::Files(&shown), alike(&inputs));
        let document = parse(&first);
        assert_in_steps(
            "a document with bands",
            Content::Document(&document),
            alike(&loading_in(&[0])),
        );
    }

    /// A header leaves rows to the shell and the status line, which an image
    /// file below it is fitted to the rest of: the pen that the next step
    /// makes knows how many.
    #[test]
    fn the_rows_a_header_leaves_to_others_are_kept_between_the_steps() {
        use crate::steps::{alike, assert_in_steps};
        use crate::typeset::images::{ImageKey, Source};
        let mut inputs = Inputs::new(Capabilities {
            height: Some(30),
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        });
        let key = ImageKey::from("a.png").in_document(0);
        inputs
            .images
            .insert(key, Ok(Source::Raster(image::RgbaImage::new(100, 3000))));
        let image = File {
            name: "a.png".to_owned(),
            document: Document::new(vec![Block::ImageFile("a.png".to_owned())]),
        };
        let shown = [image, file_of("b.md", "Two\n")];
        assert_in_steps("an image file", Content::Files(&shown), alike(&inputs));
        let whole = stream(Content::Files(&shown), (0, 0), &inputs, 512);
        let rows = whole
            .lines
            .iter()
            .filter(|line| line.image.is_some())
            .count();
        // The image is as tall as the window leaves room for: 30 rows less the
        // header's three and its blank line and the row below.
        assert_eq!(rows, 30 - 1 - 4, "{rows} rows");
    }

    /// Front matter that is hidden, or shown as its source, is known not to
    /// be on a line before any line is laid out, and the layout in steps is
    /// the layout whole in each style, in both engines.
    #[test]
    fn hidden_front_matter_is_known_at_the_start_and_the_steps_equal_the_whole() {
        use crate::render::FrontMatterStyle;
        use crate::steps::{Inputses, assert_in_steps};
        let document = parse("---\ntitle: A\nlist: [1, 2]\n---\n\nText.\n\nMore text.\n");
        for style in [
            FrontMatterStyle::Table,
            FrontMatterStyle::Code,
            FrontMatterStyle::Hide,
        ] {
            let each: Vec<Inputs> = (0..8)
                .map(|at| Inputs {
                    front_matter: style,
                    ..if at < 4 { inputs() } else { reading() }
                })
                .collect();
            for engine in each.chunks(4) {
                let refs: Inputses = std::array::from_fn(|at| &engine[at]);
                let name = format!("{style:?}");
                assert_in_steps(&name, Content::Document(&document), refs);
                let laid = Laid::begin(Content::Document(&document), 0, &engine[0]);
                let first = laid.blocks().ids().next().unwrap();
                assert_eq!(
                    laid.blocks().is_hidden(first),
                    style != FrontMatterStyle::Table
                );
            }
        }
    }

    /// What the layout says of the images it did not show again is said
    /// when the layout is whole, once, and not at each step.
    #[test]
    fn a_layout_in_steps_lets_go_of_what_it_did_not_show_when_it_is_whole() {
        use crate::typeset::images::ImageKey;
        let document = parse("One\n\nTwo\n\nThree\n");
        let content = Content::Document(&document);
        let inputs = inputs();
        // A note of an earlier layout, and one that it says again.
        let key = ImageKey::from("a.png").in_document(0);
        inputs
            .images
            .note_still(&key, (2, 2), "an earlier note".to_owned());
        inputs.images.end_notes(0..1);
        assert_eq!(inputs.images.notes().len(), 1);
        let mut laid = Laid::begin(content, 0, &inputs);
        let mut steps = 0;
        while !laid.extend(content, &inputs, laid.len() + 1) {
            steps += 1;
            assert_eq!(inputs.images.notes().len(), 1, "after step {steps}");
        }
        assert!(steps >= 3, "{steps}");
        // The note was not said again: it goes with the end.
        assert_eq!(inputs.images.notes().len(), 0);
        // A layout that is whole does not end again.
        assert!(laid.extend(content, &inputs, usize::MAX));
    }

    /// The band of an image that loads covers the lines of the code above
    /// it, those of every stretch: the pen that lays the last stretch out
    /// knows how many rows the stretches before it made.
    #[test]
    fn the_rows_of_the_stretches_before_a_loading_band_are_kept_between_the_steps() {
        use crate::steps::{alike, assert_in_steps};
        use crate::typeset::images::{ImageKey, Placeholder, Source};
        let code = "graph TD\n".to_owned() + &"  A --> B\n".repeat(1200);
        let document = code_block(Some("mermaid"), &code, false);
        let mut inputs = inputs();
        let key = inputs.drawn(ImageKey::rendered("mermaid", &code));
        // An image without size is not placed, so the code is shown.
        let empty = Source::Raster(image::RgbaImage::new(0, 0));
        inputs.images.wait(key, Placeholder::Dimmed(empty));
        let content = Content::Document(&document);
        let laid = whole(content, &inputs, 512);
        let above = laid.lines().iter().filter_map(|line| line.loading.as_ref());
        // The band is on the last line and covers all the lines above it.
        assert_eq!(
            above.map(|loading| loading.above).max(),
            Some(laid.len() - 1)
        );
        assert_in_steps("a loading band", content, alike(&inputs));
    }

    /// A page is laid out of the blocks it takes and not of the whole run
    /// that is held: a layout of many pages hands the layout of pages a few
    /// times as many blocks as there are, not as many as pages times blocks.
    #[test]
    fn a_run_of_many_pages_is_handed_to_the_layout_a_few_times_over() {
        use crate::typeset::reading::pen::handed;
        let long: String = (0..20_000)
            .map(|n| format!("Paragraph {n} of a long document, with words to wrap.\n\n"))
            .collect();
        let document = parse(&long);
        let before = handed();
        let laid = whole(Content::Document(&document), &reading(), 512);
        let given = handed() - before;
        let pages = crate::typeset::reading::engine::pages(laid.lines()).len();
        assert!(pages > 8, "{pages} pages");
        assert!(
            given <= 4 * 20_000,
            "{given} blocks handed for 20000 in {pages} pages"
        );
    }

    /// A page may start at a block inside a `<details>`, or at its summary,
    /// and a layout in steps and a restart go on from there as the whole
    /// layout does. The top paragraphs shift where the pages start.
    #[test]
    fn a_page_that_starts_inside_a_details_is_restarted_as_it_is_laid_out_whole() {
        let mut nested_starts = 0;
        for shift in 0..5 {
            let mut source: String = (0..shift).map(|n| format!("Top {n}.\n\n")).collect();
            for n in 0..900 {
                source += &format!(
                    "Paragraph {n} of a long document, with words to wrap.\n\n\
                     <details>\n<summary>Summary {n}</summary>\n\n\
                     First in {n}, with some words more.\n\nSecond in {n}.\n\n</details>\n\n"
                );
            }
            let document = parse(&source);
            let content = Content::Document(&document);
            let inputs = reading();
            let eager = whole(content, &inputs, 512);
            let starts: Vec<usize> = crate::typeset::reading::engine::pages(eager.lines())
                .iter()
                .map(|page| page.lines.start)
                .collect();
            assert!(starts.len() >= 3, "shift {shift}: {starts:?}");
            // One step per page is the layout whole.
            let mut laid = Laid::begin(content, 0, &inputs);
            while !laid.extend(content, &inputs, laid.len() + 1) {}
            assert_same(&laid, &eager, &format!("shift {shift}: in steps"));
            // The kept checkpoints are the page starts, and some of those
            // are inside a `<details>`.
            for &start in &starts[1..] {
                let lines = &eager.lines()[start..];
                let block = lines.iter().find_map(|line| line.inner).expect("a block");
                nested_starts += usize::from(eager.blocks().path(block).len() > 1);
            }
            for &start in &starts {
                for line in [start, start + 7] {
                    laid.restart(content, line);
                    let want = starts.iter().copied().filter(|&at| at <= line).max();
                    assert_eq!(
                        laid.len(),
                        want.unwrap_or(0),
                        "shift {shift}: restart({line})"
                    );
                    assert!(laid.extend(content, &inputs, usize::MAX));
                    assert_same(&laid, &eager, &format!("shift {shift}: restart({line})"));
                }
            }
        }
        assert!(nested_starts > 0, "no page started inside a <details>");
    }
}
