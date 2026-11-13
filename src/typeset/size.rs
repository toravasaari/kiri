//! Text sizes: the values the theme and the renderers share, as kitty's
//! text sizing protocol takes them.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The size text is drawn at. The text occupies `scale` rows and `scale`
/// cells for each normal cell of its width. A fraction draws the font
/// smaller inside those same cells, at `scale * numerator / denominator`
/// times the normal size, and `align` places it among their rows; the
/// fraction never changes the cells or rows the text occupies. So one and
/// a half times the normal size is scale 2 with the fraction 3/4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSize {
    scale: u8,
    fraction: Option<Fraction>,
    align: VerticalAlign,
    /// Where text at a fraction sits among its cells when it is packed as
    /// one group of its own, as a piece cut out between groups is, so that
    /// it keeps the packing it had in the whole text.
    group: Option<HorizontalAlign>,
}

/// A fraction below one: `numerator` from 1 to 7 and below `denominator`,
/// which is at most 15. Text at a fraction is packed: `denominator` cells of
/// it take `numerator` cells, and the protocol gives one escape sequence at
/// most seven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fraction {
    numerator: u8,
    denominator: u8,
}

/// Text at a fraction that one escape sequence holds, packed into fewer
/// cells than it would take at the normal size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group<'a> {
    pub text: &'a str,
    /// Where the group starts in the text it was cut from, in bytes.
    pub at: usize,
    /// The cells of the text's scale the group takes, the protocol's `w`.
    pub cells: u8,
    /// Where the text sits among its cells when it is drawn narrower than
    /// they are.
    pub align: HorizontalAlign,
}

/// Where text drawn at a fraction of its scale sits among the cells of its
/// group.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HorizontalAlign {
    #[default]
    Left,
    Right,
    Centre,
}

/// Where text drawn at a fraction of its scale sits among the rows the
/// scale gives it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VerticalAlign {
    #[default]
    Top,
    Bottom,
    Centre,
}

impl TextSize {
    /// The normal size, scale 1.
    pub const NORMAL: Self = Self {
        scale: 1,
        fraction: None,
        align: VerticalAlign::Top,
        group: None,
    };

    /// `scale` times the normal size; `None` unless it is 1 to 7, the
    /// scales the text sizing protocol accepts.
    pub const fn new(scale: u8) -> Option<Self> {
        if scale < 1 || scale > 7 {
            return None;
        }
        Some(Self {
            scale,
            ..Self::NORMAL
        })
    }

    /// The size nearest to `value` times the normal size that kiri can
    /// draw, with a fraction placed by `align`; `None` unless `value` is
    /// from 0.5 to 7. The scale is the whole number at or above `value`,
    /// so the text takes no more rows than it needs, and the fraction the
    /// one nearest to the rest; a tie goes to the smaller denominator. So
    /// 1.71 is 2 · 6/7, and 3 is 3 with no fraction.
    pub fn nearest(value: f64, align: VerticalAlign) -> Option<Self> {
        if !(0.5..=7.0).contains(&value) {
            return None;
        }
        // In range, so the cast cannot truncate.
        let size = Self::new(value.ceil() as u8)?;
        Some(size.nearest_fraction(value / f64::from(size.scale), align))
    }

    /// The size nearest to `factor` times this one in the same scale, so
    /// in the same rows, with its fraction placed by `align`.
    pub fn times(self, factor: f64, align: VerticalAlign) -> Self {
        let whole = Self {
            scale: self.scale,
            ..Self::NORMAL
        };
        whole.nearest_fraction(self.value() * factor / f64::from(self.scale), align)
    }

    /// This scale with the fraction nearest to `rest`, or none when the
    /// whole scale is nearer; a tie goes to the smaller denominator.
    fn nearest_fraction(self, rest: f64, align: VerticalAlign) -> Self {
        // The whole scale first, so that it wins a tie.
        let mut best = (1.0 - rest).abs();
        let mut nearest = self;
        for denominator in 2..=15 {
            for numerator in 1..denominator.min(8) {
                let Some(fraction) = Fraction::new(numerator, denominator) else {
                    continue;
                };
                let distance = (f64::from(numerator) / f64::from(denominator) - rest).abs();
                // A distance that only rounding errors make smaller is a
                // tie.
                if distance < best - 1e-12 {
                    best = distance;
                    nearest = self.with_fraction(fraction, align);
                }
            }
        }
        nearest
    }

    /// This size with the font drawn at `fraction` of it, placed by
    /// `align`.
    pub const fn with_fraction(self, fraction: Fraction, align: VerticalAlign) -> Self {
        Self {
            fraction: Some(fraction),
            align,
            ..self
        }
    }

    /// This size for text that is packed as one group, placed by `align`
    /// among its cells, whatever the text around it was.
    pub const fn as_one_group(self, align: HorizontalAlign) -> Self {
        Self {
            group: Some(align),
            ..self
        }
    }

    /// The rows the text occupies, and the cells for each normal cell of
    /// its width.
    pub const fn scale(self) -> u8 {
        self.scale
    }

    /// How many times the normal size the font is drawn at.
    pub fn value(self) -> f64 {
        let fraction = self.fraction.map_or(1.0, |fraction| {
            f64::from(fraction.numerator) / f64::from(fraction.denominator)
        });
        f64::from(self.scale) * fraction
    }

    pub const fn fraction(self) -> Option<Fraction> {
        self.fraction
    }

    pub const fn align(self) -> VerticalAlign {
        self.align
    }

    /// `text` cut into the groups it is packed in at this size; none at a
    /// whole size, which packs nothing.
    pub fn groups(self, text: &str) -> Vec<Group<'_>> {
        match (self.fraction, self.group) {
            (None, _) => Vec::new(),
            (Some(_), Some(_)) if text.is_empty() => Vec::new(),
            (Some(fraction), Some(align)) => vec![Group {
                text,
                at: 0,
                cells: fraction.cells(text.width()),
                align,
            }],
            (Some(fraction), None) => fraction.groups(text),
        }
    }

    /// The normal cells `text` takes at this size: `scale` for each cell
    /// of its width, or with a fraction, for each cell of its groups.
    pub fn width(self, text: &str) -> usize {
        let cells = match self.fraction {
            None => text.width(),
            Some(_) => (self.groups(text).iter())
                .map(|group| usize::from(group.cells))
                .sum(),
        };
        usize::from(self.scale) * cells
    }
}

impl Default for TextSize {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl Fraction {
    /// `numerator / denominator`; `None` unless the numerator is from 1 to 7
    /// and below the denominator, which is at most 15.
    pub const fn new(numerator: u8, denominator: u8) -> Option<Self> {
        if numerator < 1 || numerator > 7 || numerator >= denominator || denominator > 15 {
            return None;
        }
        Some(Self {
            numerator,
            denominator,
        })
    }

    pub const fn numerator(self) -> u8 {
        self.numerator
    }

    pub const fn denominator(self) -> u8 {
        self.denominator
    }

    /// The cells a group of text `width` cells wide takes, rounded up.
    fn cells(self, width: usize) -> u8 {
        let cells = (width * usize::from(self.numerator)).div_ceil(usize::from(self.denominator));
        u8::try_from(cells).unwrap_or(u8::MAX)
    }

    /// `text` cut into the groups it is packed in. A group of text `w`
    /// cells wide takes `w * numerator / denominator` cells, rounded up and
    /// at most seven, the most one escape sequence holds, and ends between
    /// graphemes.
    ///
    /// kitty draws text at a fraction a little narrower than its cells,
    /// and a group that rounds up has cells to spare, so each group leaves
    /// a gap on the side it is not aligned to. The groups and their
    /// alignment are chosen for the smallest and most even gaps between
    /// words, with none before the text, which starts at the margin, and
    /// none inside a word: where two groups meet in a word, the first sits
    /// at the right of its cells and the second at the left. Only a word
    /// too long for two groups has a gap in it.
    #[expect(clippy::too_many_lines)]
    pub fn groups(self, text: &str) -> Vec<Group<'_>> {
        let graphemes: Vec<Grapheme> = (text.grapheme_indices(true))
            .map(|(at, grapheme)| Grapheme {
                at,
                width: grapheme.width(),
                space: grapheme.starts_with(char::is_whitespace),
            })
            .collect();
        if graphemes.iter().all(|grapheme| grapheme.width == 0) {
            // Nothing to pack: a group of no cells, which shows nothing.
            return (!text.is_empty())
                .then_some(Group {
                    text,
                    at: 0,
                    cells: 0,
                    align: HorizontalAlign::Left,
                })
                .into_iter()
                .collect();
        }
        // The text cells one group holds.
        let most = 7 * usize::from(self.denominator) / usize::from(self.numerator);
        let inside_word = |index: usize| {
            index > 0
                && index < graphemes.len()
                && !graphemes[index - 1].space
                && !graphemes[index].space
        };
        let gaps = |width: usize, align: HorizontalAlign| {
            let drawn = width as f64 * f64::from(self.numerator) / f64::from(self.denominator);
            let spare = f64::from(self.cells(width)) - drawn * (1.0 - NARROWER);
            match align {
                HorizontalAlign::Left => (0.0, spare),
                HorizontalAlign::Right => (spare, 0.0),
                HorizontalAlign::Centre => (spare / 2.0, spare / 2.0),
            }
        };
        let cost = |index: usize, gap: f64| match index {
            0 if !graphemes[0].space => START * gap * gap,
            _ if index == graphemes.len() && !graphemes[index - 1].space => END * gap * gap,
            _ if inside_word(index) && gap > 0.0 => INSIDE_WORD + gap * gap,
            _ => gap * gap,
        };
        // The best packings of the text up to each grapheme, one for each
        // way its last group can start and be aligned.
        let mut ends: Vec<Vec<Packing>> = (0..=graphemes.len()).map(|_| Vec::new()).collect();
        for end in 1..=graphemes.len() {
            let (done, rest) = ends.split_at_mut(end);
            let mut width = 0;
            for start in (0..end).rev() {
                width += graphemes[start].width;
                if width > most {
                    break;
                }
                // Zero-width graphemes go with the text around them.
                if width == 0 {
                    continue;
                }
                for align in [
                    HorizontalAlign::Left,
                    HorizontalAlign::Right,
                    HorizontalAlign::Centre,
                ] {
                    let (before, after) = gaps(width, align);
                    // The gaps on both sides of a group of spaces show as
                    // one, so they are weighed together.
                    let spaces = graphemes[start..end].iter().all(|grapheme| grapheme.space);
                    // Of packings with even gaps, the one of fewer groups.
                    let packing = |cost_so_far: f64, gap: f64, last| match spaces {
                        true => (cost_so_far + GROUP, gap + after, last),
                        false => (cost_so_far + GROUP + cost(start, gap), after, last),
                    };
                    let best = match start {
                        0 => Some(packing(0.0, before, None)),
                        _ => (done[start].iter().enumerate())
                            .map(|(index, last)| {
                                packing(last.cost, last.after + before, Some(index))
                            })
                            .min_by(|a, b| a.0.total_cmp(&b.0)),
                    };
                    if let Some((cost, after, last)) = best {
                        rest[0].push(Packing {
                            start,
                            width,
                            align,
                            after,
                            cost,
                            last,
                        });
                    }
                }
            }
        }
        let end = graphemes.len();
        let mut best = (ends[end].iter().enumerate())
            .min_by(|(_, a), (_, b)| {
                (a.cost + cost(end, a.after)).total_cmp(&(b.cost + cost(end, b.after)))
            })
            .map(|(index, _)| index);
        let offset = |index: usize| {
            graphemes
                .get(index)
                .map_or(text.len(), |grapheme| grapheme.at)
        };
        let mut groups = Vec::new();
        let mut stop = end;
        while let Some(index) = best {
            let packing = &ends[stop][index];
            groups.push(Group {
                text: &text[offset(packing.start)..offset(stop)],
                at: offset(packing.start),
                cells: self.cells(packing.width),
                align: packing.align,
            });
            (stop, best) = (packing.start, packing.last);
        }
        groups.reverse();
        groups
    }
}

/// How much narrower than its cells kitty draws text at a fraction, as a
/// share of the text's width: about 4 % with an 11 pt font. It only weighs
/// one packing against another.
const NARROWER: f64 = 0.04;

/// What a gap before text that starts with a word costs, against the
/// square of the gap for one between words: a heading starts at the
/// margin, and text after other text is most often after a space.
const START: f64 = 100.0;

/// What a gap after text that ends in a word costs: it is most often at
/// the end of the line, where it does not show. After a space, it is a gap
/// between words like any other.
const END: f64 = 0.25;

/// What a gap inside a word costs: more than any gaps between words.
const INSIDE_WORD: f64 = 1000.0;

/// What a group costs, so that of two packings with gaps as even, the one
/// of fewer groups wins.
const GROUP: f64 = 1e-6;

struct Grapheme {
    /// Where it starts in the text, in bytes.
    at: usize,
    width: usize,
    space: bool,
}

/// The best packing of text up to a grapheme whose last group starts at
/// `start` and is aligned by `align`.
struct Packing {
    start: usize,
    /// The text cells of the last group.
    width: usize,
    align: HorizontalAlign,
    /// The gap the last group leaves at its end.
    after: f64,
    /// What the gaps before the end cost.
    cost: f64,
    /// The packing of the text before the last group, in the packings that
    /// end at `start`.
    last: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_are_one_to_seven() {
        assert_eq!(TextSize::new(0), None);
        assert_eq!(TextSize::new(1), Some(TextSize::NORMAL));
        assert_eq!(TextSize::new(7).map(TextSize::scale), Some(7));
        assert_eq!(TextSize::new(8), None);
    }

    #[test]
    fn fractions_are_below_one_with_a_denominator_up_to_fifteen() {
        assert_eq!(Fraction::new(0, 2), None);
        assert_eq!(Fraction::new(2, 2), None);
        assert_eq!(Fraction::new(3, 2), None);
        assert_eq!(Fraction::new(1, 16), None);
        let fraction = Fraction::new(7, 15).unwrap();
        assert_eq!((fraction.numerator(), fraction.denominator()), (7, 15));
    }

    #[test]
    fn a_numerator_is_at_most_seven() {
        // Packed text takes the numerator's cells, and an escape sequence
        // takes at most seven.
        assert_eq!(Fraction::new(8, 9), None);
        assert_eq!(Fraction::new(14, 15), None);
        assert!(Fraction::new(7, 8).is_some());
    }

    #[test]
    fn a_fraction_keeps_the_scale() {
        let half = Fraction::new(1, 2).unwrap();
        let size = TextSize::new(2)
            .unwrap()
            .with_fraction(half, VerticalAlign::Centre);
        assert_eq!(size.scale(), 2);
        assert_eq!(size.fraction(), Some(half));
        assert_eq!(size.align(), VerticalAlign::Centre);
    }

    fn half() -> Fraction {
        Fraction::new(1, 2).unwrap()
    }

    fn at(scale: u8, numerator: u8, denominator: u8) -> TextSize {
        let fraction = Fraction::new(numerator, denominator).unwrap();
        TextSize::new(scale)
            .unwrap()
            .with_fraction(fraction, VerticalAlign::Top)
    }

    fn groups(fraction: Fraction, text: &str) -> Vec<(&str, u8)> {
        (fraction.groups(text).into_iter())
            .map(|group| (group.text, group.cells))
            .collect()
    }

    #[test]
    fn a_group_takes_the_cells_of_its_text_at_the_fraction_rounded_up() {
        assert_eq!(groups(half(), "abcde"), [("abcde", 3)]);
        let three_quarters = Fraction::new(3, 4).unwrap();
        assert_eq!(groups(three_quarters, "abcdefghi"), [("abcdefghi", 7)]);
        assert_eq!(groups(half(), ""), []);
        // Text of no width is a group of no cells.
        assert_eq!(groups(half(), "\u{200b}"), [("\u{200b}", 0)]);
    }

    #[test]
    fn a_group_never_splits_a_grapheme() {
        // Twenty cells of text at a half take two groups at least.
        let accents = "e\u{301}".repeat(20);
        let groups = half().groups(&accents);
        assert!(groups.len() > 1);
        assert!(groups.iter().all(|group| group.text.starts_with('e')));
        // A flag is two regional indicators, two cells together.
        let flags = "🇫🇮".repeat(10);
        let groups = half().groups(&flags);
        assert!(groups.len() > 1);
        assert!(
            groups
                .iter()
                .all(|group| group.text.len() % "🇫🇮".len() == 0)
        );
    }

    fn aligned(fraction: Fraction, text: &str) -> Vec<(&str, u8, HorizontalAlign)> {
        (fraction.groups(text).into_iter())
            .map(|group| (group.text, group.cells, group.align))
            .collect()
    }

    #[test]
    fn a_group_takes_up_to_seven_cells() {
        // `a heading` takes 6 3/4 cells at 3/4, so one group of seven, and
        // its gap is at the end of the text.
        let three_quarters = Fraction::new(3, 4).unwrap();
        assert_eq!(
            aligned(three_quarters, "a heading"),
            [("a heading", 7, HorizontalAlign::Left)]
        );
    }

    #[test]
    fn text_starts_at_the_left_of_its_first_group() {
        // A heading starts at the margin, as the text below it does, so the
        // first group ends at a space rather than inside `three`.
        let seven_eighths = Fraction::new(7, 8).unwrap();
        assert_eq!(
            aligned(seven_eighths, "Level three"),
            [
                ("Level", 5, HorizontalAlign::Left),
                (" three", 6, HorizontalAlign::Left)
            ]
        );
    }

    #[test]
    fn text_between_spaces_leaves_even_gaps_on_both_sides() {
        // Inline code in a heading has a no-break space at each end, and its
        // background shows the gaps on both sides.
        let seven_eighths = Fraction::new(7, 8).unwrap();
        assert_eq!(
            aligned(seven_eighths, "\u{a0}code\u{a0}"),
            [("\u{a0}code\u{a0}", 6, HorizontalAlign::Centre)]
        );
    }

    // kitty draws text at a fraction a little narrower than its cells, and
    // the rest of the cells shows as a gap. Gaps go between words.
    #[test]
    fn two_groups_meet_inside_a_word_without_a_gap() {
        use HorizontalAlign::{Left, Right};
        let three_quarters = Fraction::new(3, 4).unwrap();
        assert_eq!(
            aligned(three_quarters, "abcdefghijkl"),
            [("abcd", 3, Right), ("efghijkl", 6, Left)]
        );
        let seven_eighths = Fraction::new(7, 8).unwrap();
        assert_eq!(
            aligned(seven_eighths, "abcdefghij"),
            [("abcdefgh", 7, Right), ("ij", 2, Left)]
        );
    }

    #[test]
    fn no_gap_shows_inside_a_word_that_two_groups_hold() {
        use HorizontalAlign::{Left, Right};
        for (numerator, denominator) in [(1, 2), (2, 3), (3, 4), (5, 6), (5, 8), (7, 8)] {
            let fraction = Fraction::new(numerator, denominator).unwrap();
            for text in texts(&["a", " "], 10) {
                let groups = fraction.groups(&text);
                let mut at = 0;
                for pair in groups.windows(2) {
                    at += pair[0].text.len();
                    let inside = !text[..at].ends_with(' ') && !text[at..].starts_with(' ');
                    let meet = (pair[0].align, pair[1].align) == (Right, Left);
                    assert!(!inside || meet, "{fraction:?} {text:?} {groups:?}");
                }
            }
        }
    }

    #[test]
    fn only_a_word_too_long_for_two_groups_has_a_gap_inside() {
        use HorizontalAlign::{Left, Right};
        // At a half a group holds fourteen letters, and the first starts at
        // the margin.
        assert_eq!(
            aligned(half(), "abcdefghijklmnopqrstuvwxyzabcdefgh"),
            [
                ("abcdefghijklmn", 7, Left),
                ("opqrst", 3, Right),
                ("uvwxyzabcdefgh", 7, Left)
            ]
        );
    }

    #[test]
    fn the_spare_cells_spread_over_the_spaces() {
        use HorizontalAlign::{Centre, Left, Right};
        let three_quarters = Fraction::new(3, 4).unwrap();
        assert_eq!(
            aligned(three_quarters, "Several words in a heading at level four"),
            [
                ("Several ", 6, Left),
                ("word", 3, Right),
                ("s", 1, Left),
                (" in ", 3, Left),
                ("a", 1, Centre),
                (" heading", 6, Centre),
                (" at ", 3, Right),
                ("level", 4, Centre),
                (" four", 4, Left)
            ]
        );
    }

    /// Every text of up to `length` graphemes from `alphabet`.
    fn texts(alphabet: &[&str], length: usize) -> Vec<String> {
        let mut texts = vec![String::new()];
        let mut last = vec![String::new()];
        for _ in 0..length {
            last = (last.iter())
                .flat_map(|text| {
                    alphabet
                        .iter()
                        .map(move |grapheme| format!("{text}{grapheme}"))
                })
                .collect();
            texts.extend(last.iter().cloned());
        }
        texts
    }

    #[test]
    fn groups_hold_the_text_in_order_in_at_most_seven_cells() {
        let fractions = [(1, 2), (2, 3), (3, 4), (5, 6), (5, 8), (7, 8), (7, 12)];
        let texts = [
            texts(&["a", " "], 9),
            texts(&["a", " ", "日", "\u{200b}"], 5),
        ]
        .concat();
        for (numerator, denominator) in fractions {
            let fraction = Fraction::new(numerator, denominator).unwrap();
            for text in &texts {
                let groups = fraction.groups(text);
                let joined: String = groups.iter().map(|group| group.text).collect();
                assert_eq!(&joined, text, "{fraction:?}");
                for group in &groups {
                    let cells = (group.text.width() * usize::from(numerator))
                        .div_ceil(usize::from(denominator));
                    assert!(!group.text.is_empty(), "{fraction:?} {text:?}");
                    assert_eq!(usize::from(group.cells), cells, "{fraction:?} {text:?}");
                    assert!(group.cells <= 7, "{fraction:?} {text:?}");
                }
            }
        }
    }

    #[test]
    fn text_is_as_wide_as_its_groups_at_its_scale() {
        assert_eq!(TextSize::NORMAL.width("abc"), 3);
        assert_eq!(TextSize::new(3).unwrap().width("日本"), 12);
        assert_eq!(
            TextSize::NORMAL
                .with_fraction(half(), VerticalAlign::Top)
                .width("abc"),
            2
        );
        assert_eq!(at(2, 7, 8).width("abcdefgh"), 14);
        assert_eq!(at(2, 7, 8).width("abcdefghi"), 16);
        assert_eq!(at(1, 1, 2).width("a日b"), 2);
        assert_eq!(at(1, 1, 2).width(""), 0);
    }

    /// The size nearest to `value` as scale, numerator and denominator.
    fn nearest(value: f64) -> Option<(u8, u8, u8)> {
        let size = TextSize::nearest(value, VerticalAlign::Bottom)?;
        Some(match size.fraction() {
            Some(fraction) => {
                assert_eq!(size.align(), VerticalAlign::Bottom);
                (size.scale(), fraction.numerator(), fraction.denominator())
            }
            None => (size.scale(), 1, 1),
        })
    }

    #[test]
    fn a_decimal_size_rounds_to_the_nearest_drawable_fraction() {
        // The scale is the whole number at or above the value, so 1.71
        // takes two rows, never three.
        assert_eq!(nearest(1.71), Some((2, 6, 7)));
        assert_eq!(nearest(2.5), Some((3, 5, 6)));
        assert_eq!(nearest(1.75), Some((2, 7, 8)));
        assert_eq!(nearest(1.25), Some((2, 5, 8)));
        assert_eq!(nearest(0.875), Some((1, 7, 8)));
        assert_eq!(nearest(0.5), Some((1, 1, 2)));
        // 0.65 would be 9/14, but a numerator is at most 7.
        assert_eq!(nearest(1.3), Some((2, 7, 11)));
        // A tie goes to the smaller denominator: 2/4 is 1/2.
        assert_eq!(nearest(1.0 + 1.0 / 3.0), Some((2, 2, 3)));
    }

    #[test]
    fn a_whole_size_has_no_fraction() {
        assert_eq!(nearest(3.0), Some((3, 1, 1)));
        assert_eq!(nearest(7.0), Some((7, 1, 1)));
        assert_eq!(nearest(1.0), Some((1, 1, 1)));
        assert_eq!(
            TextSize::nearest(3.0, VerticalAlign::Bottom),
            TextSize::new(3)
        );
        // Nearer to the whole scale than to any fraction of it.
        assert_eq!(nearest(1.98), Some((2, 1, 1)));
    }

    #[test]
    fn a_decimal_size_is_from_a_half_to_seven() {
        for value in [0.49, 7.01, 0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                TextSize::nearest(value, VerticalAlign::Top),
                None,
                "{value}"
            );
        }
    }

    /// `size` as scale, numerator and denominator.
    fn parts(size: TextSize) -> (u8, u8, u8) {
        size.fraction().map_or((size.scale(), 1, 1), |fraction| {
            (size.scale(), fraction.numerator(), fraction.denominator())
        })
    }

    #[test]
    fn a_size_times_a_factor_keeps_its_scale() {
        let two_thirds = |size: TextSize| parts(size.times(2.0 / 3.0, VerticalAlign::Top));
        assert_eq!(two_thirds(TextSize::NORMAL), (1, 2, 3));
        assert_eq!(two_thirds(TextSize::new(2).unwrap()), (2, 2, 3));
        // 5/6 · 2/3 is 5/9; 7/8 · 2/3 is 7/12; 5/8 · 2/3 is 5/12.
        assert_eq!(two_thirds(at(3, 5, 6)), (3, 5, 9));
        assert_eq!(two_thirds(at(2, 7, 8)), (2, 7, 12));
        assert_eq!(two_thirds(at(2, 3, 4)), (2, 1, 2));
        assert_eq!(two_thirds(at(2, 5, 8)), (2, 5, 12));
        // 7/11 · 2/3 is 14/33, which cannot be drawn: 3/7 is nearest.
        assert_eq!(two_thirds(at(2, 7, 11)), (2, 3, 7));
        let size = TextSize::NORMAL.times(2.0 / 3.0, VerticalAlign::Bottom);
        assert_eq!(size.align(), VerticalAlign::Bottom);
    }

    #[test]
    fn the_normal_size_has_no_fraction() {
        assert_eq!(TextSize::NORMAL.scale(), 1);
        assert_eq!(TextSize::NORMAL.fraction(), None);
        assert_eq!(TextSize::NORMAL.align(), VerticalAlign::Top);
    }

    #[test]
    fn groups_say_where_they_start_and_cover_the_text() {
        let size = at(2, 1, 2);
        for text in [
            "hello, world",
            "a b c d e f g h i j",
            "日本語のテキスト",
            "e\u{301}x",
        ] {
            let groups = size.groups(text);
            let mut next = 0;
            for group in &groups {
                assert_eq!(group.at, next, "{text:?}: {groups:?}");
                assert_eq!(&text[group.at..group.at + group.text.len()], group.text);
                next += group.text.len();
            }
            assert_eq!(next, text.len(), "{text:?}: the groups cover the text");
        }
    }
}
