//! Text laid out in a proportional font with parley: runs of styled text
//! in, lines with their baselines and the boxes of any part of the text
//! out.

use std::ops::Range;
use std::rc::Rc;

use image::RgbaImage;

use parley::fontique::GenericFamily;
use parley::{
    Affinity, Alignment, AlignmentOptions, Cursor, FontFamily, FontFamilyName, FontStyle,
    FontWeight, InlineBox, InlineBoxKind, Layout, LineHeight, OverflowWrap, Selection,
    StyleProperty, TextWrapMode, VerticalAlign, WordBreak,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::typeset::reading::fonts::Fonts;
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
    pub pixels: Rc<RgbaImage>,
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
    pub(crate) layout: Layout<Brush>,
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

/// `runs` laid out in `fonts`, set as `look` says, in lines at most `width`
/// pixels wide. A line breaks where the text has a newline.
#[expect(clippy::too_many_lines)]
pub fn lay_out(fonts: &Fonts, runs: &[Run], look: &Look, width: f32) -> Paragraph {
    let mut text = String::new();
    let mut ranges = Vec::new();
    for run in runs {
        let start = text.len();
        text.push_str(&run.text);
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
    let layout = fonts.with(|contexts| {
        let mut builder = contexts
            .layout
            .ranged_builder(&mut contexts.fonts, &text, 1.0, true);
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
        for (number, (picture, at)) in (runs.iter().zip(&ranges))
            .filter_map(|(run, range)| Some((run.picture.as_ref()?, range.start)))
            .enumerate()
        {
            builder.push_inline_box(InlineBox {
                id: number as u64,
                kind: InlineBoxKind::InFlow,
                index: at,
                width: picture.pixels.width() as f32,
                height: picture.pixels.height() as f32,
                baseline: Some(picture.pixels.height() as f32 - picture.depth),
                vertical_align: VerticalAlign::BASELINE,
            });
        }
        for (run, range) in runs.iter().zip(&ranges) {
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
                    for (at, _) in run.text.match_indices(' ') {
                        let at = range.start + at;
                        builder.push(TextWrapMode::Wrap, at..at + 1);
                    }
                }
            }
        }
        // Emoji come from the emoji font first, as a browser takes them,
        // even where the text's font has a plain glyph of its own.
        for (at, grapheme) in text.grapheme_indices(true) {
            if is_emoji(grapheme) {
                builder.push(
                    FontFamily::Single(FontFamilyName::Generic(GenericFamily::Emoji)),
                    at..at + grapheme.len(),
                );
            }
        }
        let mut layout = builder.build(&text);
        layout.break_all_lines(Some(width));
        let alignment = match look.align {
            Align::Left => Alignment::Start,
            Align::Centre => Alignment::Center,
            Align::Right => Alignment::End,
        };
        layout.align(alignment, AlignmentOptions::default());
        layout
    });
    Paragraph {
        layout,
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
        self.layout.width()
    }

    pub fn height(&self) -> f32 {
        self.layout.height()
    }

    /// The baseline of the first line, from the top; 0 without text.
    pub fn first_baseline(&self) -> f32 {
        self.layout.first_baseline().unwrap_or(0.0)
    }

    pub fn lines(&self) -> Vec<LineBox> {
        (self.layout.lines())
            .map(|line| {
                let metrics = line.metrics();
                LineBox {
                    top: metrics.block_min_coord,
                    height: metrics.line_height,
                    baseline: metrics.baseline,
                }
            })
            .collect()
    }

    /// The parts of lines that the text in `range` covers.
    pub fn boxes(&self, range: Range<usize>) -> Vec<RangeBox> {
        let selection = Selection::new(
            Cursor::from_byte_index(&self.layout, range.start, Affinity::Downstream),
            Cursor::from_byte_index(&self.layout, range.end, Affinity::Upstream),
        );
        (selection.geometry(&self.layout).into_iter())
            .map(|(rect, line)| RangeBox {
                line,
                left: rect.x0 as f32,
                right: rect.x1 as f32,
            })
            .collect()
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

    /// The byte of the text that the point (`x`, `y`) of the paragraph is
    /// at, the nearest one when it is past the end of a line.
    pub fn index_at(&self, x: f32, y: f32) -> Option<usize> {
        if self.text.is_empty() {
            return None;
        }
        Some(Cursor::from_point(&self.layout, x, y).index())
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
        (self.layout.lines()).enumerate().find_map(|(line, laid)| {
            laid.items().find_map(|item| match item {
                parley::PositionedLayoutItem::InlineBox(placed) if placed.id == number as u64 => {
                    Some(RangeBox {
                        line,
                        left: placed.x,
                        right: placed.x + placed.width,
                    })
                }
                _ => None,
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
        let widths = self.layout.calculate_content_widths();
        (widths.min, widths.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOK: Look = Look {
        color: [0xdd; 3],
        ..Look::PLAIN
    };

    fn code(text: &str) -> Run {
        Run {
            code: true,
            ..Run::new(text)
        }
    }

    fn paragraph(runs: &[Run], width: f32) -> Paragraph {
        lay_out(&Fonts::fixture(), runs, &LOOK, width)
    }

    #[test]
    fn lines_are_one_and_a_half_times_the_font_size_apart() {
        let text = "Reading mode lays text out in a proportional font. ".repeat(4);
        let laid = paragraph(&[Run::new(&text)], 300.0);
        let lines = laid.lines();
        assert!(lines.len() > 2, "{lines:?}");
        for (above, below) in lines.iter().zip(&lines[1..]) {
            assert_eq!(below.top - above.top, 24.0);
            assert_eq!(below.baseline - above.baseline, 24.0);
        }
        assert_eq!(laid.height(), 24.0 * lines.len() as f32);
        assert_eq!(laid.first_baseline(), lines[0].baseline);
    }

    #[test]
    fn inline_code_keeps_lines_their_height() {
        let runs = [
            Run::new("Run "),
            code("make check"),
            Run::new(" before you commit, and "),
            code("make fmt"),
            Run::new(" too."),
        ];
        let laid = paragraph(&runs, 120.0);
        let lines = laid.lines();
        assert!(lines.len() > 2, "{lines:?}");
        assert!(lines.iter().all(|line| line.height == 24.0), "{lines:?}");
    }

    #[test]
    fn inline_code_breaks_only_at_its_spaces() {
        let runs = [
            Run::new("See "),
            code("src/kitty/graphics.rs and --reading-mode"),
            Run::new(" for more."),
        ];
        let laid = paragraph(&runs, 150.0);
        let range = laid.code()[0].clone();
        let starts: Vec<usize> = (laid.layout.lines())
            .map(|line| line.text_range().start)
            .filter(|start| range.contains(start) && *start > range.start)
            .collect();
        assert!(!starts.is_empty(), "the code should wrap at 150 px");
        for start in starts {
            assert_eq!(&laid.text()[start - 1..start], " ", "a break at {start}");
        }
        let (min, _) = laid.content_widths();
        let alone = paragraph(&[code("src/kitty/graphics.rs")], 1000.0);
        assert_eq!(min, alone.width());
    }

    #[test]
    fn a_range_has_a_box_on_each_line_it_covers() {
        let words = "one two three four five six seven eight nine ten";
        let laid = paragraph(&[Run::new(words)], 120.0);
        assert!(laid.lines().len() > 1);
        let boxes = laid.boxes(0..words.len());
        assert_eq!(boxes.len(), laid.lines().len());
        let two = words.find("two").unwrap();
        let [one] = laid.boxes(two..two + 3)[..] else {
            panic!("one box");
        };
        assert_eq!(one.line, 0);
        assert!(one.left > 0.0 && one.right > one.left, "{one:?}");
    }

    #[test]
    fn content_widths_are_the_longest_word_and_the_whole_line() {
        let laid = paragraph(&[Run::new("short and loooooooooooong")], 1000.0);
        let (min, max) = laid.content_widths();
        let long = paragraph(&[Run::new("loooooooooooong")], 1000.0);
        assert_eq!(min, long.width());
        assert_eq!(max, laid.width());
    }

    #[test]
    fn a_newline_breaks_the_line() {
        let laid = paragraph(&[Run::new("one\ntwo")], 1000.0);
        assert_eq!(laid.lines().len(), 2);
    }

    /// A picture of `width` × `height` pixels, all red.
    fn picture(width: u32, height: u32, id: u64) -> Run {
        Run::picture(
            InlinePicture {
                pixels: Rc::new(RgbaImage::from_pixel(
                    width,
                    height,
                    image::Rgba([255, 0, 0, 255]),
                )),
                id,
                depth: 0.0,
            },
            None,
        )
    }

    /// The boxes of `laid`: id, left, top, width and height.
    fn boxes(laid: &Paragraph) -> Vec<(u64, f32, f32, f32, f32)> {
        (laid.layout.lines())
            .flat_map(|line| line.items())
            .filter_map(|item| match item {
                parley::PositionedLayoutItem::InlineBox(placed) => {
                    Some((placed.id, placed.x, placed.y, placed.width, placed.height))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_picture_in_a_line_sits_on_the_baseline_and_makes_the_line_taller() {
        let runs = [Run::new("Before "), picture(20, 40, 1), Run::new(" after")];
        let laid = paragraph(&runs, 600.0);
        let [(id, left, top, width, height)] = boxes(&laid)[..] else {
            panic!("one box: {:?}", boxes(&laid));
        };
        assert_eq!((id, width, height), (0, 20.0, 40.0));
        assert!(left > 40.0, "after the text before it: {left}");
        let [line] = laid.lines()[..] else {
            panic!("one line");
        };
        // Its bottom edge on the baseline, and the line tall enough.
        assert!(
            (top + height - line.baseline).abs() < 0.01,
            "{top} {line:?}"
        );
        assert!(line.height >= 40.0, "{line:?}");
        assert_eq!(laid.text(), "Before  after");
    }

    #[test]
    fn a_picture_with_a_depth_reaches_below_the_baseline_and_the_line_grows_there() {
        let formula = Run::picture(
            InlinePicture {
                depth: 14.0,
                ..match picture(20, 40, 1).picture {
                    Some(picture) => picture,
                    None => unreachable!("a picture"),
                }
            },
            None,
        );
        let runs = [Run::new("Before "), formula, Run::new(" after")];
        let laid = paragraph(&runs, 600.0);
        let [(_, _, top, _, height)] = boxes(&laid)[..] else {
            panic!("one box: {:?}", boxes(&laid));
        };
        let [line] = laid.lines()[..] else {
            panic!("one line");
        };
        // Its baseline, 26 pixels from its top, is the line's.
        assert!((top + 26.0 - line.baseline).abs() < 0.01, "{top} {line:?}");
        assert!(
            (top + height - line.baseline - 14.0).abs() < 0.01,
            "{top} {line:?}"
        );
        // The line is as tall as the formula needs on both sides, not
        // taller than the formula and the text's usual room below it.
        assert!(line.height >= 40.0 && line.height < 40.0 + 14.0, "{line:?}");
    }

    #[test]
    fn a_picture_moves_to_the_next_line_whole() {
        let words = "word ".repeat(8);
        let runs = [Run::new(&words), picture(60, 20, 1), Run::new(" end")];
        let laid = paragraph(&runs, 200.0);
        let [(_, left, _, width, _)] = boxes(&laid)[..] else {
            panic!("one box");
        };
        assert!(left + width <= 200.0 + 0.01, "{left} + {width}");
        assert!(laid.lines().len() > 1);
        // Two of them in a row, as badges are, side by side while they fit.
        let badges = [picture(60, 20, 1), Run::new(" "), picture(60, 20, 2)];
        let laid = paragraph(&badges, 400.0);
        let found = boxes(&laid);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].2, found[1].2, "{found:?}");
        assert!(found[1].1 >= found[0].1 + 60.0);
    }

    #[test]
    fn bold_text_is_wider() {
        let regular = paragraph(&[Run::new("Markdown")], 1000.0);
        let bold = Run {
            bold: true,
            ..Run::new("Markdown")
        };
        assert!(paragraph(&[bold], 1000.0).width() > regular.width());
    }

    #[test]
    fn a_point_is_at_a_byte_of_the_text_or_on_a_picture() {
        let picture = InlinePicture {
            pixels: Rc::new(RgbaImage::new(10, 10)),
            id: 1,
            depth: 0.0,
        };
        let runs = [
            Run::new("one "),
            Run::picture(picture, None),
            Run::new(" two"),
        ];
        let laid = paragraph(&runs, 300.0);
        assert_eq!(laid.index_at(0.5, 5.0), Some(0));
        // Past the end of the line, the end of the text.
        assert_eq!(laid.index_at(299.0, 5.0), Some(laid.text().len()));
        let place = laid.picture_box(0).expect("the picture is in a line");
        let middle = (place.left + place.right) / 2.0;
        assert_eq!(laid.picture_at(middle, 5.0), Some(0));
        assert_eq!(laid.picture_at(0.5, 5.0), None);
        assert_eq!(paragraph(&[Run::new("")], 300.0).index_at(1.0, 1.0), None);
    }
}
