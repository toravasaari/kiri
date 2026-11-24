//! Text laid out in a proportional font with parley: runs of styled text
//! in, lines with their baselines and the boxes of any part of the text
//! out.

use std::ops::Range;
use std::rc::Rc;

use parley::fontique::GenericFamily;
use parley::{
    Affinity, Alignment, AlignmentOptions, BaseDirection, Cluster, Cursor, FontFamily,
    FontFamilyName, FontStyle, FontWeight, InlineBox, InlineBoxKind, Layout, LineHeight,
    OverflowWrap, Selection, StyleProperty, TextWrapMode, VerticalAlign, WordBreak,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

use crate::typeset::reading::fonts::Fonts;
use crate::typeset::reading::pixels::Pixels;
use crate::typeset::style::{Origin, Origins};

/// The colour of a piece of text, as RGB.
pub type Brush = [u8; 3];

/// How much smaller inline code is than the text around it, as on
/// GitHub.
pub const CODE_SIZE: f32 = 0.85;

/// How a block's text is set, before its runs' own styles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// The size of the font in pixels.
    pub size: f32,
    /// The height of a line, as a multiple of `size`.
    pub line_height: f32,
    /// The weight of the font, 400 for regular.
    pub weight: f32,
    pub color: Brush,
    /// In the monospace font, as a code block is.
    pub mono: bool,
    pub wrap: Wrap,
    pub align: Align,
}

impl Look {
    /// Regular text at 16 pixels in lines of 1.5, in black, wrapped at
    /// words and set from the left, for the other fields to change.
    pub const PLAIN: Look = Look {
        size: 16.0,
        line_height: 1.5,
        weight: 400.0,
        color: [0; 3],
        mono: false,
        wrap: Wrap::Words,
        align: Align::Left,
    };
}

/// Where lines of text may break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    /// Between words, and inside inline code only at its spaces; a word
    /// longer than the line reaches past it.
    Words,
    /// Anywhere, as code lines wrap at the edge of their block.
    Anywhere,
    /// Between words, and inside a word or inline code where it must, so
    /// that nothing reaches past the line, as in a table squeezed into
    /// less room than its words take.
    Forced,
}

/// How lines of text are set across their width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

/// A picture that stands in a line of text as a box, such as an image or a
/// formula: its pixels, and an id of them, which is what two of them are
/// the same by.
#[derive(Debug, Clone)]
pub struct InlinePicture {
    pub pixels: Rc<Pixels>,
    pub id: u64,
    /// How many pixels of it are below the baseline it sits on in its
    /// line: none for an image, which sits on its bottom edge; some for a
    /// formula, whose baseline is a letter's.
    pub depth: f32,
}

impl PartialEq for InlinePicture {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

/// A subscript or a superscript: its size as a share of the text's, and how
/// far it is moved down from the baseline, in font sizes of the text; a
/// superscript is moved up, so by less than none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Script {
    pub size: f32,
    pub drop: f32,
}

/// What parley wants in the text where a picture's box is.
const OBJECT: char = '\u{FFFC}';

/// A piece of text in one style, or a picture in its place.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// Inline code, or a key: in the monospace font, smaller, and broken
    /// only at its spaces.
    pub code: bool,
    /// Struck through, in its colour.
    pub strike: bool,
    /// Underlined, as `<u>` has it.
    pub underline: bool,
    /// Its own colour, such as a link's.
    pub color: Option<Brush>,
    /// A box of colour behind it, as marked text has.
    pub background: Option<Brush>,
    /// Smaller and moved from the baseline, as a subscript or a
    /// superscript is.
    pub script: Option<Script>,
    /// Where it links to, when it is in a link.
    pub link: Option<Rc<str>>,
    /// A picture that stands here as a box on the line, sitting on its
    /// baseline, which has no text.
    pub picture: Option<InlinePicture>,
    /// What of the document's text the bytes of the run show, and for a
    /// picture, what it stands in the place of; none for text that the layout
    /// makes up. It does not make two runs differ.
    pub from: Origins,
}

impl Run {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            ..Self::default()
        }
    }

    /// A picture in a line.
    pub fn picture(picture: InlinePicture, link: Option<Rc<str>>) -> Self {
        Self {
            picture: Some(picture),
            link,
            ..Self::default()
        }
    }
}

/// A line of laid-out text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineBox {
    /// The top of the line, from the top of the text.
    pub top: f32,
    pub height: f32,
    /// The baseline, from the top of the text.
    pub baseline: f32,
}

/// The part of a line that a range of the text covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeBox {
    /// The line, counted from 0.
    pub line: usize,
    /// Its left and right edges, from the left of the text.
    pub left: f32,
    pub right: f32,
}

/// Text laid out at a width.
pub struct Paragraph {
    /// The text laid out in one or more pieces, one below another; see
    /// `Piece`.
    pub(crate) pieces: Vec<Piece>,
    pub(crate) look: Look,
    text: String,
    /// The ranges of the text in inline code.
    code: Vec<Range<usize>>,
    /// The ranges of the text on a background of its own, with its colour.
    backgrounds: Vec<(Range<usize>, Brush)>,
    /// The ranges of the text moved from the baseline, with how far down
    /// in pixels; up when it is below nothing.
    drops: Vec<(Range<usize>, f32)>,
    /// The ranges of the text in links, with where each links to.
    links: Vec<(Range<usize>, Rc<str>)>,
    /// The pictures in links: where each is in the text, its number among
    /// the pictures, and where its link goes.
    linked_pictures: Vec<(usize, usize, Rc<str>)>,
    /// The pictures in the lines, by the number of their boxes.
    pub(crate) pictures: Vec<InlinePicture>,
    /// What of the document each part of the text shows, as the runs gave
    /// it, in the bytes of the text; a picture's is where it stands, an empty
    /// range.
    from: Vec<Origin>,
    /// The same of each of the pictures, by number.
    picture_from: Vec<Option<Origin>>,
}

/// The most style pushes that go into one piece of a text. parley takes at
/// most 65 535 styles in one text and a push may make two of them, so a
/// piece stays well below half of that.
const PIECE_STYLES: usize = 30_000;

/// How many styles a piece of text collects before it is cut at the next
/// line break, so that a piece is far from `PIECE_STYLES` as a rule.
const CUT_STYLES: usize = 15_000;

/// One part of a paragraph's text laid out as a layout of its own: a text of
/// more styles than one parley layout holds is cut at line breaks, and the
/// line break at a cut is left out of the pieces, which sit one below another
/// as the lines of one layout do.
pub(crate) struct Piece {
    pub(crate) layout: Layout<Brush>,
    /// Where its text starts in the paragraph's text.
    pub(crate) start: usize,
    /// Where it starts below the top of the paragraph.
    pub(crate) top: f32,
    /// The number of the paragraph's first line in it.
    pub(crate) first_line: usize,
}

/// The emoji of `text`, as the ranges of the text they are, found in the
/// whole of it, so that one in two runs is one; none in a text that is all
/// ASCII.
fn emoji_of(text: &str) -> Vec<Range<usize>> {
    if has_no_emoji(text) {
        return Vec::new();
    }
    (text.grapheme_indices(true))
        .filter(|(_, grapheme)| is_emoji(grapheme))
        .map(|(at, grapheme)| at..at + grapheme.len())
        .collect()
}

/// Whether `text` is plain: ASCII, and the object characters that stand
/// for pictures, so that no emoji lookup is needed.
fn has_no_emoji(text: &str) -> bool {
    text.chars().all(|c| c.is_ascii() || c == OBJECT)
}

/// The emoji of `emoji`, in order, that start in `range`.
fn emoji_in<'a>(emoji: &'a [Range<usize>], range: &Range<usize>) -> &'a [Range<usize>] {
    let from = emoji.partition_point(|found| found.start < range.start);
    let to = emoji.partition_point(|found| found.start < range.end);
    &emoji[from..to.max(from)]
}

/// The cost in style pushes of `run`, the pushes of its styles and of its
/// `emoji`, as `piece_of` makes them: the one cost the cuts go by, and the
/// budget of a piece for the styles.
fn styles_of(run: &Run, look: &Look, emoji: usize) -> usize {
    let wraps = match run.code && look.wrap == Wrap::Words {
        true => 1 + run.text.matches(' ').count(),
        false => 0,
    };
    usize::from(run.bold)
        + usize::from(run.italic)
        + usize::from(run.color.is_some())
        + usize::from(run.strike)
        + usize::from(run.underline)
        + usize::from(run.script.is_some())
        + if run.code { 2 } else { 0 }
        + wraps
        + emoji
}

/// A piece of a text: where it is, and what of the runs it has.
struct Cut {
    /// Its bytes of the text.
    bytes: Range<usize>,
    /// The runs that have bytes in it, or a picture: the run that has the
    /// line break at its end is in the piece before it and the one after.
    runs: Range<usize>,
    /// How many pictures are in the runs before `runs`.
    pictures: usize,
}

/// The pieces that `runs` of `text` are laid out in: one, unless there are
/// many styles, and then cut at a line break each time `CUT_STYLES` have been
/// collected. The line break is in no piece.
fn cuts(whole: &Whole, text: &str, look: &Look) -> Vec<Cut> {
    let Whole {
        runs,
        ranges,
        emoji,
    } = *whole;
    let mut pieces = Vec::new();
    let (mut start, mut first, mut cost) = (0, 0, 0);
    let (mut pictures, mut before) = (0, 0);
    for (index, (run, range)) in runs.iter().zip(ranges).enumerate() {
        pictures += usize::from(run.picture.is_some());
        cost += styles_of(run, look, emoji_in(emoji, range).len());
        if cost < CUT_STYLES {
            continue;
        }
        // The last line break of the run, with text after it.
        let Some(at) = text[range.clone()].rfind('\n').map(|at| range.start + at) else {
            continue;
        };
        if at + 1 >= text.len() {
            continue;
        }
        pieces.push(Cut {
            bytes: start..at,
            runs: first..index + 1,
            pictures: before,
        });
        (start, first, cost, before) = (at + 1, index, 0, pictures);
    }
    pieces.push(Cut {
        bytes: start..text.len(),
        runs: first..runs.len(),
        pictures: before,
    });
    pieces
}

/// The direction of `text` as one layout sets it: that of its first strong
/// character, which parley takes by its own rules. A text of several pieces
/// gives it to each, so that a piece that starts with other letters is not
/// set the other way round. The probe is the text up to its first letter, so
/// a strong character before the letter counts, and a combining mark is not
/// the letter; a text without a letter is
/// set from the left.
fn direction_of(fonts: &Fonts, text: &str) -> BaseDirection {
    let Some((at, letter)) = text
        .char_indices()
        .find(|(_, c)| c.is_alphabetic() && c.width() != Some(0))
    else {
        return BaseDirection::Ltr;
    };
    let probe = &text[..at + letter.len_utf8()];
    let rtl = fonts.with(|contexts| {
        let builder = contexts
            .layout
            .ranged_builder(&mut contexts.fonts, probe, 1.0, true);
        builder.build(probe).is_rtl()
    });
    if rtl {
        BaseDirection::Rtl
    } else {
        BaseDirection::Ltr
    }
}

/// `runs` laid out in `fonts`, set as `look` says, in lines at most `width`
/// pixels wide. A line breaks where the text has a newline.
pub fn lay_out(fonts: &Fonts, runs: &[Run], look: &Look, width: f32) -> Paragraph {
    let mut text = String::new();
    let mut ranges = Vec::new();
    for run in runs {
        let start = text.len();
        text.push_str(&run.text);
        // parley wants the character that an in-flow box stands for.
        if run.picture.is_some() {
            text.push(OBJECT);
        }
        ranges.push(start..text.len());
    }
    let links: Vec<(Range<usize>, Rc<str>)> = (runs.iter().zip(&ranges))
        .filter_map(|(run, range)| Some((range.clone(), run.link.clone()?)))
        .collect();
    let linked_pictures: Vec<(usize, usize, Rc<str>)> = (runs.iter().zip(&ranges))
        .filter(|(run, _)| run.picture.is_some())
        .enumerate()
        .filter_map(|(number, (run, range))| Some((range.start, number, run.link.clone()?)))
        .collect();
    let code: Vec<Range<usize>> = (runs.iter().zip(&ranges))
        .filter(|(run, _)| run.code)
        .map(|(_, range)| range.clone())
        .collect();
    let backgrounds: Vec<(Range<usize>, Brush)> = (runs.iter().zip(&ranges))
        .filter_map(|(run, range)| Some((range.clone(), run.background?)))
        .collect();
    let drops: Vec<(Range<usize>, f32)> = (runs.iter().zip(&ranges))
        .filter_map(|(run, range)| Some((range.clone(), run.script?.drop * look.size)))
        .collect();
    let pictures: Vec<InlinePicture> = runs.iter().filter_map(|run| run.picture.clone()).collect();
    let mut from = Vec::new();
    let mut picture_from = Vec::new();
    for (run, range) in runs.iter().zip(&ranges) {
        let at = u32::try_from(range.start).unwrap_or(u32::MAX);
        let mut shifted = run.from.iter().map(|origin| Origin {
            shown: origin.shown.start + at..origin.shown.end + at,
            ..origin.clone()
        });
        match run.picture {
            Some(_) => picture_from.push(shifted.next_back()),
            None => from.extend(shifted),
        }
    }
    let mut pieces: Vec<Piece> = Vec::new();
    let (mut top, mut first_line) = (0.0, 0);
    let emoji = emoji_of(&text);
    let whole = Whole {
        runs,
        ranges: &ranges,
        emoji: &emoji,
    };
    let cuts = cuts(&whole, &text, look);
    let setting = Setting {
        width,
        direction: match cuts.len() {
            1 => BaseDirection::Auto,
            _ => direction_of(fonts, &text),
        },
    };
    for cut in cuts {
        let start = cut.bytes.start;
        let layout = piece_of(fonts, &text, &cut, &whole, look, setting);
        let (height, lines) = (layout.height(), layout.len());
        pieces.push(Piece {
            layout,
            start,
            top,
            first_line,
        });
        top += height;
        first_line += lines;
    }
    Paragraph {
        pieces,
        look: *look,
        text,
        code,
        backgrounds,
        drops,
        links,
        linked_pictures,
        pictures,
        from,
        picture_from,
    }
}

/// All the runs of a text, where each is in it, and its emoji.
#[derive(Clone, Copy)]
struct Whole<'a> {
    runs: &'a [Run],
    ranges: &'a [Range<usize>],
    emoji: &'a [Range<usize>],
}

/// How the pieces of a text are laid out.
#[derive(Clone, Copy)]
struct Setting {
    /// The width of the lines.
    width: f32,
    /// The direction of the text, that of the whole of it.
    direction: BaseDirection,
}

/// The piece `cut` of `text`, with the styles of the runs it has in it, laid
/// out in lines at most `setting.width` pixels wide. `whole` is all the runs
/// of the text. A piece that has more styles than `PIECE_STYLES`
/// leaves the later ones out, so that its text has fewer colours and weights
/// rather than the layout failing.
#[expect(clippy::too_many_lines)]
fn piece_of(
    fonts: &Fonts,
    text: &str,
    cut: &Cut,
    whole: &Whole,
    look: &Look,
    setting: Setting,
) -> Layout<Brush> {
    let Setting { width, direction } = setting;
    let bytes = &cut.bytes;
    let runs = &whole.runs[cut.runs.clone()];
    let ranges = &whole.ranges[cut.runs.clone()];
    let shown = &text[bytes.clone()];
    // What of a range of the text is in the piece, in the piece's bytes.
    let inside = |range: &Range<usize>| {
        let (from, to) = (range.start.max(bytes.start), range.end.min(bytes.end));
        (from < to).then(|| from - bytes.start..to - bytes.start)
    };
    let mut left = PIECE_STYLES;
    fonts.with(|contexts| {
        let mut builder = contexts
            .layout
            .ranged_builder(&mut contexts.fonts, shown, 1.0, true);
        builder.set_base_direction(direction);
        let family = if look.mono { "monospace" } else { "sans-serif" };
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(family.into())));
        match look.wrap {
            Wrap::Words => {}
            Wrap::Anywhere => builder.push_default(StyleProperty::WordBreak(WordBreak::BreakAll)),
            Wrap::Forced => {
                builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
            }
        }
        builder.push_default(StyleProperty::FontSize(look.size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(look.weight)));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            look.line_height,
        )));
        builder.push_default(StyleProperty::Brush(look.color));
        // The pictures stand in the lines as boxes on the baseline.
        let mut number = cut.pictures;
        for (run, range) in runs.iter().zip(ranges) {
            let Some(picture) = run.picture.as_ref() else {
                continue;
            };
            number += 1;
            if range.start < bytes.start || range.start > bytes.end {
                continue;
            }
            builder.push_inline_box(InlineBox {
                id: (number - 1) as u64,
                kind: InlineBoxKind::InFlow,
                index: range.start - bytes.start,
                width: picture.pixels.width() as f32,
                height: picture.pixels.height() as f32,
                baseline: Some(picture.pixels.height() as f32 - picture.depth),
                vertical_align: VerticalAlign::BASELINE,
            });
        }
        for (run, range) in runs.iter().zip(ranges) {
            let Some(range) = inside(range) else {
                continue;
            };
            // Past the budget a run has the default style.
            let cost = styles_of(run, look, 0);
            if cost > left {
                continue;
            }
            left -= cost;
            if run.bold {
                builder.push(FontWeight::new(look.weight.max(600.0)), range.clone());
            }
            if run.italic {
                builder.push(FontStyle::Italic, range.clone());
            }
            if let Some(color) = run.color {
                builder.push(StyleProperty::Brush(color), range.clone());
            }
            if run.strike {
                builder.push(StyleProperty::Strikethrough(true), range.clone());
            }
            if run.underline {
                builder.push(StyleProperty::Underline(true), range.clone());
            }
            if let Some(script) = run.script {
                builder.push(
                    StyleProperty::FontSize(look.size * script.size),
                    range.clone(),
                );
            }
            if run.code {
                builder.push(FontFamily::Source("monospace".into()), range.clone());
                builder.push(
                    StyleProperty::FontSize(look.size * CODE_SIZE),
                    range.clone(),
                );
                // Inline code breaks only at its spaces, unless the text
                // must break where it can.
                if look.wrap == Wrap::Words {
                    builder.push(TextWrapMode::NoWrap, range.clone());
                    for (at, _) in shown[range.clone()].match_indices(' ') {
                        let at = range.start + at;
                        builder.push(TextWrapMode::Wrap, at..at + 1);
                    }
                }
            }
        }
        // Emoji come from the emoji font first, as a browser takes them, even
        // where the text's font has a plain glyph of its own, so they are
        // pushed last. Each is pushed once, over the whole of it, in the piece
        // that has it, and costs a push of the budget of its own.
        for range in ranges {
            for found in emoji_in(whole.emoji, range) {
                if let Some(found) = inside(found).filter(|_| left > 0) {
                    left -= 1;
                    builder.push(
                        FontFamily::Single(FontFamilyName::Generic(GenericFamily::Emoji)),
                        found,
                    );
                }
            }
        }
        let mut layout = builder.build(shown);
        // parley 0.13 applies the alignment as the lines are broken.
        let alignment = match look.align {
            Align::Left => Alignment::Start,
            Align::Centre => Alignment::Center,
            Align::Right => Alignment::End,
        };
        layout.set_alignment(alignment, AlignmentOptions::default());
        layout.break_all_lines(Some(width));
        layout
    })
}

/// Whether `grapheme` is an emoji shown as a picture by default, or asked
/// to be with U+FE0F; not one such as © that is text unless asked.
fn is_emoji(grapheme: &str) -> bool {
    let picture = grapheme.contains('\u{fe0f}')
        || grapheme
            .chars()
            .next()
            .is_some_and(|c| u32::from(c) >= 0x1f000);
    picture && emojis::get(grapheme).is_some()
}

impl Paragraph {
    /// The text, all runs together.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The width of the widest line.
    pub fn width(&self) -> f32 {
        (self.pieces.iter()).fold(0.0, |widest, piece| piece.layout.width().max(widest))
    }

    /// How far right the lines reach, from the paragraph's left edge: where
    /// each starts (right-to-left text starts at the right edge of the
    /// box) and how far it advances.
    pub fn right(&self) -> f32 {
        let lines = self.pieces.iter().flat_map(|piece| piece.layout.lines());
        lines.fold(0.0, |far, line| {
            let metrics = line.metrics();
            far.max(metrics.offset + metrics.advance)
        })
    }

    pub fn height(&self) -> f32 {
        self.pieces.iter().map(|piece| piece.layout.height()).sum()
    }

    /// The baseline of the first line, from the top; 0 without text.
    pub fn first_baseline(&self) -> f32 {
        (self.pieces.first()).map_or(0.0, |piece| piece.layout.first_baseline().unwrap_or(0.0))
    }

    pub fn lines(&self) -> Vec<LineBox> {
        (self.pieces.iter())
            .flat_map(|piece| {
                piece.layout.lines().map(|line| {
                    let metrics = line.metrics();
                    LineBox {
                        top: metrics.block_min_coord + piece.top,
                        height: metrics.line_height,
                        baseline: metrics.baseline + piece.top,
                    }
                })
            })
            .collect()
    }

    /// Line `number` alone, found by a search for its piece, so that a
    /// question about one line of a text of 100 000 lines does not walk
    /// them all as `lines` does.
    pub fn line(&self, number: usize) -> Option<LineBox> {
        let at = self
            .pieces
            .partition_point(|piece| piece.first_line <= number);
        let piece = self.pieces.get(at.checked_sub(1)?)?;
        let metrics = *piece.layout.get(number - piece.first_line)?.metrics();
        Some(LineBox {
            top: metrics.block_min_coord + piece.top,
            height: metrics.line_height,
            baseline: metrics.baseline + piece.top,
        })
    }

    /// The lines that show some of the bytes `bytes` of the text, each with
    /// its number and the bytes of the text it shows, in order. Found by
    /// searches for the first of them, and the work after is that of the
    /// lines it returns, however many lines the text has.
    pub fn lines_in(&self, bytes: &Range<usize>) -> Vec<(usize, Range<usize>)> {
        let mut found = Vec::new();
        // The last piece that starts at or before the bytes; a line of the
        // one before it cannot reach them, as the cut between is no line's.
        let first = (self.pieces)
            .partition_point(|piece| piece.start <= bytes.start)
            .saturating_sub(1);
        for piece in self.pieces.iter().skip(first) {
            let layout = &piece.layout;
            let shown = |line: usize| layout.get(line).map(|line| line.text_range());
            // The first line that ends past the start of the bytes.
            let (mut low, mut high) = (0, layout.len());
            while low < high {
                let middle = low + (high - low) / 2;
                match shown(middle) {
                    Some(range) if piece.start + range.end <= bytes.start => low = middle + 1,
                    _ => high = middle,
                }
            }
            for line in low..layout.len() {
                let Some(range) = shown(line) else { break };
                let range = piece.start + range.start..piece.start + range.end;
                if range.start >= bytes.end {
                    return found;
                }
                found.push((piece.first_line + line, range));
            }
        }
        found
    }

    /// The parts of lines that the text in `range` covers.
    pub fn boxes(&self, range: Range<usize>) -> Vec<RangeBox> {
        let mut boxes = Vec::new();
        for (number, piece) in self.pieces.iter().enumerate() {
            let layout = &piece.layout;
            // What of the range the piece has, in its bytes; the range
            // as it is when the text is the one piece.
            let mut within = range.clone();
            if self.pieces.len() > 1 {
                let end = self
                    .pieces
                    .get(number + 1)
                    .map_or(self.text.len(), |next| next.start - 1);
                let (from, to) = (range.start.max(piece.start), range.end.min(end));
                if from > to || (from == to && range.start != range.end) {
                    continue;
                }
                within = from - piece.start..to - piece.start;
            }
            let selection = Selection::new(
                Cursor::from_byte_index(layout, within.start, Affinity::Downstream),
                Cursor::from_byte_index(layout, within.end, Affinity::Upstream),
            );
            boxes.extend(
                selection
                    .geometry(layout)
                    .into_iter()
                    .map(|(rect, line)| RangeBox {
                        line: line + piece.first_line,
                        left: rect.x0 as f32,
                        right: rect.x1 as f32,
                    }),
            );
        }
        boxes
    }

    /// What of the document's text each part of the text shows, in the bytes
    /// of the text, in order.
    pub fn from(&self) -> &[Origin] {
        &self.from
    }

    /// What picture `number` stands in the place of.
    pub fn picture_from(&self, number: usize) -> Option<&Origin> {
        self.picture_from.get(number)?.as_ref()
    }

    /// The byte of the text where the character under the point (`x`, `y`)
    /// of the paragraph starts; none when the point is past the end of a
    /// line or on a picture.
    pub fn index_at(&self, x: f32, y: f32) -> Option<usize> {
        // Between two lines, or below the last one, nothing is under the
        // point, whatever room the paragraph has there.
        if !(self.lines().iter()).any(|line| (line.top..line.top + line.height).contains(&y)) {
            return None;
        }
        let piece = (self.pieces.iter())
            .rfind(|piece| piece.top <= y)
            .or(self.pieces.first())?;
        let (cluster, _) = Cluster::from_point_exact(&piece.layout, x, y - piece.top)?;
        Some(piece.start + cluster.text_range().start)
    }

    /// The number of the picture that the point (`x`, `y`) of the paragraph
    /// is on, if it is on one.
    pub fn picture_at(&self, x: f32, y: f32) -> Option<usize> {
        let lines = self.lines();
        (0..self.pictures.len()).find(|&number| {
            let Some(part) = self.picture_box(number) else {
                return false;
            };
            let Some(line) = lines.get(part.line) else {
                return false;
            };
            (part.left..=part.right).contains(&x)
                && (line.top..=line.top + line.height).contains(&y)
        })
    }

    /// The ranges of the text in links, with where each links to; a link
    /// of several runs is several ranges.
    pub fn links(&self) -> &[(Range<usize>, Rc<str>)] {
        &self.links
    }

    /// The pictures in links: the place of each in the text, its number
    /// and where it links to.
    pub fn linked_pictures(&self) -> &[(usize, usize, Rc<str>)] {
        &self.linked_pictures
    }

    /// The part of its line that picture `number` is in.
    pub fn picture_box(&self, number: usize) -> Option<RangeBox> {
        self.pieces.iter().find_map(|piece| {
            (piece.layout.lines()).enumerate().find_map(|(line, laid)| {
                laid.items().find_map(|item| match item {
                    parley::PositionedLayoutItem::InlineBox(placed)
                        if placed.id == number as u64 =>
                    {
                        Some(RangeBox {
                            line: line + piece.first_line,
                            left: placed.x,
                            right: placed.x + placed.width,
                        })
                    }
                    _ => None,
                })
            })
        })
    }

    /// The ranges of the text on a background of its own, such as marked
    /// text, with its colour.
    pub fn backgrounds(&self) -> &[(Range<usize>, Brush)] {
        &self.backgrounds
    }

    /// How far down in pixels the text at `range` is moved from the
    /// baseline: for a subscript, and up for a superscript.
    pub(crate) fn drop_at(&self, range: &Range<usize>) -> f32 {
        (self.drops.iter())
            .find(|(drop, _)| drop.start <= range.start && range.end <= drop.end)
            .map_or(0.0, |(_, drop)| *drop)
    }

    /// The ranges of the text in inline code.
    pub fn code(&self) -> &[Range<usize>] {
        &self.code
    }

    /// How narrow the text can be, breaking wherever it may, and how wide
    /// it is unbroken: a table column's least and natural width.
    pub fn content_widths(&self) -> (f32, f32) {
        (self.pieces.iter()).fold((0.0, 0.0), |(least, natural), piece| {
            let widths = piece.layout.calculate_content_widths();
            (widths.min.max(least), widths.max.max(natural))
        })
    }
}

#[cfg(test)]
mod tests;
