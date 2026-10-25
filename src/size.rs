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
    pub fn groups(self, text: &str) -> Vec<Group<'_>> {
        let (numerator, denominator) = (usize::from(self.numerator), usize::from(self.denominator));
        let mut groups = Vec::new();
        let group = |text, width: usize| Group {
            text,
            cells: u8::try_from((width * numerator).div_ceil(denominator)).unwrap_or(u8::MAX),
        };
        let (mut start, mut width) = (0, 0);
        for (at, grapheme) in text.grapheme_indices(true) {
            let grapheme_width = grapheme.width();
            if width + grapheme_width > denominator && width > 0 {
                groups.push(group(&text[start..at], width));
                (start, width) = (at, 0);
            }
            width += grapheme_width;
        }
        if start < text.len() {
            groups.push(group(&text[start..], width));
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

    #[test]
    fn the_normal_size_has_no_fraction() {
        assert_eq!(TextSize::NORMAL.scale(), 1);
        assert_eq!(TextSize::NORMAL.fraction(), None);
        assert_eq!(TextSize::NORMAL.align(), VerticalAlign::Top);
    }
}
