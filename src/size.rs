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

    /// The normal cells `text` takes at this size: `scale` for each cell
    /// of its width, or with a fraction, for each cell of its groups.
    pub fn width(self, text: &str) -> usize {
        let cells = match self.fraction {
            None => text.width(),
            Some(fraction) => (fraction.groups(text).iter())
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

    /// `text` cut into the groups it is packed in: `denominator` cells of
    /// text in `numerator` cells, so half-size `abc` is `ab` and `c`, one
    /// cell each. A group ends between graphemes, early where a wide
    /// character would cross its end, and a shorter group takes the cells
    /// its text needs, rounded up.
    ///
    /// kitty draws the text a little narrower than its cells, so each
    /// group leaves a gap, and the gaps go between words: a group that
    /// ends inside a word sits at the right of its cells, so that it meets
    /// the next group without a gap. A group ends after its last space
    /// instead where the word would also cross the next group's end, and
    /// a group inside a word at both ends with no space is centred.
    pub fn groups(self, text: &str) -> Vec<Group<'_>> {
        let (numerator, denominator) = (usize::from(self.numerator), usize::from(self.denominator));
        let graphemes: Vec<_> = (text.grapheme_indices(true))
            .map(|(at, grapheme)| {
                (
                    at,
                    grapheme.width(),
                    grapheme.starts_with(char::is_whitespace),
                )
            })
            .collect();
        let offset = |index: usize| graphemes.get(index).map_or(text.len(), |&(at, ..)| at);
        // Whether the graphemes on both sides of `index` are not spaces.
        let inside_word = |index: usize| {
            index > 0 && index < graphemes.len() && !graphemes[index - 1].2 && !graphemes[index].2
        };
        let mut groups = Vec::new();
        let mut start = 0;
        while start < graphemes.len() {
            let (mut end, mut width) = (start, 0);
            while let Some(&(_, grapheme_width, _)) = graphemes.get(end) {
                if width + grapheme_width > denominator && width > 0 {
                    break;
                }
                width += grapheme_width;
                end += 1;
            }
            // The cells the rest of the word after `end` takes.
            let rest: usize = (graphemes[end..].iter())
                .take_while(|&&(_, _, space)| !space)
                .map(|&(_, width, _)| width)
                .sum();
            if inside_word(end)
                && (inside_word(start) || rest > denominator)
                && let Some(space) = (start..end).rev().find(|&index| graphemes[index].2)
            {
                end = space + 1;
                width = graphemes[start..end]
                    .iter()
                    .map(|&(_, width, _)| width)
                    .sum();
            }
            let align = match (inside_word(start), inside_word(end)) {
                (_, false) => HorizontalAlign::Left,
                (false, true) => HorizontalAlign::Right,
                (true, true) => HorizontalAlign::Centre,
            };
            groups.push(Group {
                text: &text[offset(start)..offset(end)],
                cells: u8::try_from((width * numerator).div_ceil(denominator)).unwrap_or(u8::MAX),
                align,
            });
            start = end;
        }
        groups
    }
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
    fn text_at_a_fraction_is_packed_in_groups_of_the_denominator_s_cells() {
        assert_eq!(groups(half(), "abcde"), [("ab", 1), ("cd", 1), ("e", 1)]);
        // A shorter last group takes the cells its text needs, rounded up.
        let three_quarters = Fraction::new(3, 4).unwrap();
        assert_eq!(
            groups(three_quarters, "abcdefghi"),
            [("abcd", 3), ("efgh", 3), ("i", 1)]
        );
        assert_eq!(groups(three_quarters, "abcdef"), [("abcd", 3), ("ef", 2)]);
        assert_eq!(groups(half(), ""), []);
    }

    #[test]
    fn a_group_never_splits_a_grapheme() {
        // A wide character that would cross a group's end starts the next.
        assert_eq!(groups(half(), "a日b"), [("a", 1), ("日", 1), ("b", 1)]);
        // An e with a combining acute accent stays whole.
        assert_eq!(groups(half(), "ae\u{301}b"), [("ae\u{301}", 1), ("b", 1)]);
        // A flag is two regional indicators, two cells together.
        assert_eq!(groups(half(), "🇫🇮"), [("🇫🇮", 1)]);
    }

    fn aligned(fraction: Fraction, text: &str) -> Vec<(&str, u8, HorizontalAlign)> {
        (fraction.groups(text).into_iter())
            .map(|group| (group.text, group.cells, group.align))
            .collect()
    }

    // kitty draws text at a fraction a little narrower than its cells, and
    // the rest of the cells shows as a gap. Gaps go between words.
    #[test]
    fn a_group_that_ends_inside_a_word_sits_at_the_right_of_its_cells() {
        use HorizontalAlign::{Left, Right};
        let three_quarters = Fraction::new(3, 4).unwrap();
        assert_eq!(
            aligned(three_quarters, "ab cdef"),
            [("ab c", 3, Right), ("def", 3, Left)]
        );
        // A group that ends at a space, or at the end of the text, keeps
        // its gap at its end.
        assert_eq!(
            aligned(three_quarters, "abc defg"),
            [("abc ", 3, Left), ("defg", 3, Left)]
        );
    }

    #[test]
    fn a_group_inside_a_word_at_both_ends_ends_after_its_last_space() {
        use HorizontalAlign::{Left, Right};
        let three_quarters = Fraction::new(3, 4).unwrap();
        assert_eq!(
            aligned(three_quarters, "abcde fghij"),
            [
                ("abcd", 3, Right),
                ("e ", 2, Left),
                ("fghi", 3, Right),
                ("j", 1, Left)
            ]
        );
    }

    #[test]
    fn a_word_longer_than_a_group_starts_one() {
        use HorizontalAlign::{Left, Right};
        let three_quarters = Fraction::new(3, 4).unwrap();
        // Two groups that meet inside a word leave no gap between them.
        assert_eq!(
            aligned(three_quarters, "ab cdefgh"),
            [("ab ", 3, Left), ("cdef", 3, Right), ("gh", 2, Left)]
        );
    }

    #[test]
    fn a_group_inside_a_word_with_no_space_is_centred() {
        use HorizontalAlign::{Centre, Left, Right};
        let three_quarters = Fraction::new(3, 4).unwrap();
        assert_eq!(
            aligned(three_quarters, "abcdefghij"),
            [("abcd", 3, Right), ("efgh", 3, Centre), ("ij", 2, Left)]
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
                // The wrapper adds text a grapheme at a time.
                let size = TextSize::NORMAL.with_fraction(fraction, VerticalAlign::Top);
                let mut widths = (text.grapheme_indices(true).map(|(at, _)| at))
                    .chain([text.len()])
                    .map(|at| size.width(&text[..at]));
                let mut last = 0;
                assert!(widths.all(|width| std::mem::replace(&mut last, width) <= width));
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
        assert_eq!(at(1, 1, 2).width("a日b"), 3);
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
}
