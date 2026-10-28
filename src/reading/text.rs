//! Text laid out in a proportional font with parley: runs of styled text
//! in, lines with their baselines and the boxes of any part of the text
//! out.

use std::ops::Range;

use parley::{
    Affinity, Alignment, AlignmentOptions, Cursor, FontFamily, FontStyle, FontWeight, Layout,
    LineHeight, Selection, StyleProperty, TextWrapMode,
};

use super::fonts::Fonts;

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
}

/// A piece of text in one style.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// Inline code, or a key: in the monospace font, smaller, and broken
    /// only at its spaces.
    pub code: bool,
    /// Its own colour, such as a link's.
    pub color: Option<Brush>,
}

impl Run {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
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
    text: String,
    /// The ranges of the text in inline code.
    code: Vec<Range<usize>>,
}

/// `runs` laid out in `fonts`, set as `look` says, in lines at most `width`
/// pixels wide. A line breaks where the text has a newline.
pub fn lay_out(fonts: &Fonts, runs: &[Run], look: &Look, width: f32) -> Paragraph {
    let mut text = String::new();
    let mut ranges = Vec::new();
    for run in runs {
        let start = text.len();
        text.push_str(&run.text);
        ranges.push(start..text.len());
    }
    let code: Vec<Range<usize>> = (runs.iter().zip(&ranges))
        .filter(|(run, _)| run.code)
        .map(|(_, range)| range.clone())
        .collect();
    let layout = fonts.with(|contexts| {
        let mut builder = contexts
            .layout
            .ranged_builder(&mut contexts.fonts, &text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(
            "sans-serif".into(),
        )));
        builder.push_default(StyleProperty::FontSize(look.size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(look.weight)));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            look.line_height,
        )));
        builder.push_default(StyleProperty::Brush(look.color));
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
            if run.code {
                builder.push(FontFamily::Source("monospace".into()), range.clone());
                builder.push(
                    StyleProperty::FontSize(look.size * CODE_SIZE),
                    range.clone(),
                );
                // Inline code breaks only at its spaces.
                builder.push(TextWrapMode::NoWrap, range.clone());
                for (at, _) in run.text.match_indices(' ') {
                    let at = range.start + at;
                    builder.push(TextWrapMode::Wrap, at..at + 1);
                }
            }
        }
        let mut layout = builder.build(&text);
        layout.break_all_lines(Some(width));
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    });
    Paragraph { layout, text, code }
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
        size: 16.0,
        line_height: 1.5,
        weight: 400.0,
        color: [0xdd; 3],
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

    #[test]
    fn bold_text_is_wider() {
        let regular = paragraph(&[Run::new("Markdown")], 1000.0);
        let bold = Run {
            bold: true,
            ..Run::new("Markdown")
        };
        assert!(paragraph(&[bold], 1000.0).width() > regular.width());
    }
}
