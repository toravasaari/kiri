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

    /// This colour as RGB, a palette colour as `palette` tells it: the
    /// terminal's 16 colours, where it told them. `None` when it is not
    /// known.
    pub fn rgb(self, palette: &[Option<Color>; 16]) -> Option<Color> {
        match self {
            Self::Rgb(..) => Some(self),
            Self::Palette(index) => *palette.get(usize::from(index))?,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_looks_palette_colours_up() {
        let mut palette = [None; 16];
        palette[3] = Some(Color::Rgb(200, 200, 0));
        assert_eq!(Color::Rgb(1, 2, 3).rgb(&palette), Some(Color::Rgb(1, 2, 3)));
        assert_eq!(
            Color::Palette(3).rgb(&palette),
            Some(Color::Rgb(200, 200, 0))
        );
        assert_eq!(Color::Palette(4).rgb(&palette), None);
        assert_eq!(Color::Palette(16).rgb(&palette), None);
    }
}
