//! Text sizes: the values the theme and the renderers share, as kitty's
//! text sizing protocol takes them.

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

/// A fraction below one: `numerator` from 1 and below `denominator`, which
/// is at most 15.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fraction {
    numerator: u8,
    denominator: u8,
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
}

impl Fraction {
    /// `numerator / denominator`; `None` unless the numerator is from 1 and
    /// below the denominator, which is at most 15.
    pub const fn new(numerator: u8, denominator: u8) -> Option<Self> {
        if numerator < 1 || numerator >= denominator || denominator > 15 {
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
        let fraction = Fraction::new(14, 15).unwrap();
        assert_eq!((fraction.numerator(), fraction.denominator()), (14, 15));
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

    #[test]
    fn the_normal_size_has_no_fraction() {
        assert_eq!(TextSize::NORMAL.scale(), 1);
        assert_eq!(TextSize::NORMAL.fraction(), None);
        assert_eq!(TextSize::NORMAL.align(), VerticalAlign::Top);
    }
}
