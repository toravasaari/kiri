//! Colours and underline styles: the values the theme and the renderers
//! share.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// An entry of the terminal's 256-colour palette; 0-15 follow its theme.
    Palette(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    /// `amount` of `other` mixed into this colour, both RGB; `None` when
    /// either is a palette colour, whose value only the terminal knows.
    pub fn mix(self, other: Color, amount: f32) -> Option<Color> {
        let (Self::Rgb(r1, g1, b1), Self::Rgb(r2, g2, b2)) = (self, other) else {
            return None;
        };
        let channel = |from: u8, to: u8| {
            (f32::from(from) + (f32::from(to) - f32::from(from)) * amount).round() as u8
        };
        Some(Self::Rgb(channel(r1, r2), channel(g1, g2), channel(b1, b2)))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}
