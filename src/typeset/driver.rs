//! The driver of the layout: it walks what is to be laid out, one document or
//! several files, and gives back the lines and the table of their blocks.
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
//!
//! The entry points are `content`, `laid`, `files`, `file` and `layout`. The
//! driver walks the pieces of a file (its blocks, the summaries of `<details>`
//! and a file's header) and gives each to the `Pen` of the engine: `Cells` in
//! `layout/mod.rs`, `Pixels` in `reading/pen.rs`. It stacks the lines with a
//! blank line between pieces; a code block of more than 128 lines in the cell
//! layout, and in the pager's a table of more than 128 rows, is handed over in
//! stretches, which join without one. It settles the loading bands per file,
//! and it ends a layout, when the pictures that reading mode did not show are
//! let go. `Laid::extend(until)` goes on until there are `until` lines, and
//! `driver::laid` lays out to the end in one step.
//!
//! What the next step needs is a `Checkpoint`: the place of the walk, the pen's
//! state, whether the file's stack has something before, and the band of the
//! last line, which waits for the line that may come below it. The pen's state
//! is, for the cell layout, its reserved rows, its text cursor and the open
//! code block with its highlighter's state, and for reading mode, its held run
//! (kept as the place of its first block and how many follow), the margin and
//! whether a page of the run is laid. Checkpoints are kept at the start of
//! every stretch of a code block, before every page of a run of reading mode,
//! and about every 512 lines. `Laid::restart(line)` goes back to the last one
//! at or before a line, so that a layout that reads an image that arrives is
//! laid out again from its stretch, not from the top.
//!
//! The table of blocks is filled before anything is laid out, by a walk of each
//! document's blocks, and the pens look their blocks up in it, so a block's
//! number does not depend on how far the layout has gone.
//!
//! The methods of `Laid` that lay out (`begin`, `extend`, `begin_measured`,
//! `show`, `work`, `grow`, `measured` and `pending`) are written in this file,
//! although they belong to `Laid` in `laid.rs`: they read and change the
//! private fields of `Progress` and `Run`, which stay private to this file.
//! Moving them takes either widening those fields or moving the stepping
//! machinery (`Progress`, `Run`, `Checkpoint` and `advance`) out of this file.

use crate::typeset::block::Blocks;
use crate::typeset::inputs::Inputs;
use crate::typeset::laid::Laid;
use crate::typeset::layout::table::Wants;
use crate::typeset::layout::{Cells, CellsSaved, Derived, texts_hidden, with_cells};
use crate::typeset::model::{Block, Document, File, Inline, Table};
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

    /// Takes the whole layout of a document of one block into one of
    /// `content`, a document that grew at the end of that block and is the
    /// same before it, byte for byte, which the caller knows: a whole file
    /// shown as text, or the table of a CSV. `inputs` are those the layout
    /// was made with. The stretches before the last of the block stay as
    /// they are, with their checkpoints and the highlighting they carry,
    /// and the layout goes on from the start of the last, which the growth
    /// may have changed, to the end. Whether it could: a layout of reading
    /// mode, one not in steps or not whole, or one whose table of blocks is
    /// not the new document's, is left as it is.
    pub fn grow(&mut self, content: Content, inputs: &Inputs) -> bool {
        let Some((_, blocks, progress)) = self.stepping() else {
            return false;
        };
        let cells = matches!(Engine::of(inputs), Engine::Cells);
        if !cells || !progress.run.done || table(content, progress.at, inputs) != *blocks {
            return false;
        }
        // The checkpoint at the start of the last stretch of the block.
        let last = (progress.run.saved.iter())
            .rfind(|saved| saved.file == 0 && saved.started && saved.place.is_at(&[0]));
        let Some(line) = last.map(|saved| saved.lines) else {
            return false;
        };
        self.go_back(content, line);
        self.extend(content, inputs, usize::MAX);
        true
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
    /// A centred wrapper that the pen lays out as its blocks, which are
    /// pieces of their own, one step deeper. The wrapper itself has no
    /// lines, only its place in the table of blocks.
    Wrapper,
    /// A block of a centred wrapper that the wrapper centres: a paragraph,
    /// a heading or a code block, as `Block::is_centred_in_wrapper` says.
    Centred(&'a Block),
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
/// centred wrapper that the pen lays out as its blocks, or a file's header,
/// which the lines of the layout mark as their own (`Line::block`), with
/// its place in the document.
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
    /// Whether the pen lays `block` out in stretches when it is large: a code
    /// block, but not when it draws it whole, as it does a diagram, and a
    /// table, in the pager's cell layout.
    fn cuts(&self, _block: &Block) -> bool {
        false
    }
    /// Whether the pen can measure a stretch of a code block or of a table:
    /// stack lines that stand in the place of its lines, as many, without
    /// laying it out.
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

#[cfg(test)]
thread_local! {
    /// How many stretches of large blocks the layouts on this thread laid
    /// out or measured.
    static STRETCHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many stretches of large blocks the layouts on this thread laid out
/// or measured, each time one was.
#[cfg(test)]
pub(crate) fn stretches_done() -> usize {
    STRETCHES.with(std::cell::Cell::get)
}

/// Counts a stretch laid out or measured, in the tests.
fn stretch_done() {
    #[cfg(test)]
    STRETCHES.with(|done| done.set(done.get() + 1));
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
        stretch_done();
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
        stretch_done();
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
        if piece.shape.ends().is_some() {
            stretch_done();
        }
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
/// the widths of their columns from it: a CSV's from what its parse found of
/// its columns, with the rows it left measured again from the source as
/// texts, without making a cell; a Markdown table's cells laid out. A
/// Markdown table that its first layout fits to all its rows is left out.
/// `r` and `:w` measure so in the pager.
pub fn fitted(content: Content, inputs: &Inputs) -> stretch::Tables {
    let derived = Derived::new(inputs);
    let mut tables = stretch::Tables::default();
    for unit in units(content, 0, 0) {
        let mut blocks = Blocks::default();
        let numbers = (unit.number, unit.keys);
        with_cells(inputs, &derived, numbers, &mut blocks, |cells| {
            let mut keep = |path, wants| tables.keep(unit.number, path, wants);
            let blocks = &unit.document.blocks;
            fit_blocks(&cells, blocks, &mut Vec::new(), (&|_, _| true, &mut keep));
        });
    }
    tables
}

/// Takes into `tables` what the columns of each long CSV of `content` ask
/// for, which its parse found, where `tables` holds nothing for it, or what
/// it holds is of another number of columns: the widths a CSV opens with
/// in the pager, those of all its rows, as `:w` gives them. What `tables`
/// holds of a table stays, so that a reload of follow mode keeps it.
pub fn scanned(content: Content, inputs: &Inputs, tables: &mut stretch::Tables) {
    let derived = Derived::new(inputs);
    for unit in units(content, 0, 0) {
        let mut fitted = Vec::new();
        let mut blocks = Blocks::default();
        let numbers = (unit.number, unit.keys);
        let missing = |path: &[usize], table: &Table| {
            table.source().is_some()
                && (tables.measured(unit.number, path))
                    .is_none_or(|kept| kept.columns() != table.alignments.len())
        };
        with_cells(inputs, &derived, numbers, &mut blocks, |cells| {
            let mut keep = |path, wants| fitted.push((path, wants));
            let blocks = &unit.document.blocks;
            fit_blocks(&cells, blocks, &mut Vec::new(), (&missing, &mut keep));
        });
        for (path, wants) in fitted {
            tables.keep(unit.number, path, wants);
        }
    }
}

/// Which tables [`fit_blocks`] measures, by their place.
type Wanted<'a> = &'a dyn Fn(&[usize], &Table) -> bool;

/// What [`fit_blocks`] hands what it measured of a table to, with its place.
type Keep<'a> = &'a mut dyn FnMut(Vec<usize>, Wants);

/// Hands `keep` the place and what its columns ask for of each long table
/// among `blocks`, at `path`, that `wanted` says, and that the walk can
/// cut: one of the file's own blocks, or of a `<details>` or a centred
/// wrapper, and not one inside a quote or a list, which is laid out whole.
fn fit_blocks(
    cells: &Cells,
    blocks: &[Block],
    path: &mut Vec<usize>,
    (wanted, keep): (Wanted, Keep),
) {
    for (index, block) in blocks.iter().enumerate() {
        path.push(index);
        match block {
            Block::Table(table) if wanted(path, table) => {
                if let Some(wants) = cells.wants(table) {
                    keep(path.clone(), wants);
                }
            }
            Block::Details { blocks, .. } | Block::Centered(blocks) => {
                fit_blocks(cells, blocks, path, (wanted, keep));
            }
            _ => {}
        }
        path.pop();
    }
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
        if let Block::Details { blocks: inside, .. } | Block::Centered(inside) = block {
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
mod tests;
