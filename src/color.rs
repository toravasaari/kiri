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

    /// This colour seen through a filter of colour `filter`: each channel
    /// scaled by the filter's share of full, so white becomes `filter`.
    /// `None` for a palette colour.
    pub fn filter(self, filter: [u8; 3]) -> Option<Color> {
        let Self::Rgb(r, g, b) = self else {
            return None;
        };
        let channel = |from: u8, by: u8| (f32::from(from) * f32::from(by) / 255.0).round() as u8;
        let [fr, fg, fb] = filter;
        Some(Self::Rgb(channel(r, fr), channel(g, fg), channel(b, fb)))
    }

    /// The WCAG contrast ratio of two RGB colours, from 1 to 21; `None`
    /// when either is a palette colour.
    pub fn contrast(self, other: Color) -> Option<f64> {
        let (a, b) = (self.luminance()?, other.luminance()?);
        Some((a.max(b) + 0.05) / (a.min(b) + 0.05))
    }

    /// The WCAG relative luminance of an RGB colour.
    fn luminance(self) -> Option<f64> {
        let Self::Rgb(r, g, b) = self else {
            return None;
        };
        let linear = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        Some(0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b))
    }

    /// This colour readable on `background`: with at least `ratio` of
    /// contrast, its hue and saturation kept and its lightness moved away
    /// from the background no further than needed. Itself when it has the
    /// contrast already, or when either is a palette colour.
    pub fn readable_on(self, background: Color, ratio: f64) -> Color {
        let (Self::Rgb(r, g, b), Some(page)) = (self, background.luminance()) else {
            return self;
        };
        if self.contrast(background).is_none_or(|have| have >= ratio) {
            return self;
        }
        let (hue, saturation, lightness) = hsl(r, g, b);
        let at = |lightness| {
            let (r, g, b) = rgb(hue, saturation, lightness);
            Self::Rgb(r, g, b)
        };
        // Away from the background: darker on a light one, lighter on a
        // dark one, whichever end can give more contrast.
        let darker = (page + 0.05) / 0.05 >= 1.05 / (page + 0.05);
        let (mut near, mut far) = if darker {
            (lightness, 0.0)
        } else {
            (lightness, 1.0)
        };
        if at(far).contrast(background).is_none_or(|most| most < ratio) {
            return at(far);
        }
        // Lightness changes luminance one way, so halving finds the
        // nearest lightness with the contrast.
        for _ in 0..24 {
            let middle = (near + far) / 2.0;
            if at(middle)
                .contrast(background)
                .is_some_and(|have| have >= ratio)
            {
                far = middle;
            } else {
                near = middle;
            }
        }
        at(far)
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

    /// This colour as RGB, a palette colour as `palette` tells it, or
    /// else as xterm's default palette has it, where the terminal did not
    /// tell it.
    pub fn rgb_or_xterm(self, palette: &[Option<Color>; 16]) -> Color {
        match self {
            Self::Rgb(..) => self,
            Self::Palette(index) => self.rgb(palette).unwrap_or_else(|| xterm(index)),
        }
    }
}

/// xterm's default 16 colours.
const XTERM: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0xcd, 0x00, 0x00),
    (0x00, 0xcd, 0x00),
    (0xcd, 0xcd, 0x00),
    (0x00, 0x00, 0xee),
    (0xcd, 0x00, 0xcd),
    (0x00, 0xcd, 0xcd),
    (0xe5, 0xe5, 0xe5),
    (0x7f, 0x7f, 0x7f),
    (0xff, 0x00, 0x00),
    (0x00, 0xff, 0x00),
    (0xff, 0xff, 0x00),
    (0x5c, 0x5c, 0xff),
    (0xff, 0x00, 0xff),
    (0x00, 0xff, 0xff),
    (0xff, 0xff, 0xff),
];

/// The colour `index` of xterm's default 256-colour palette: its 16
/// colours, then a cube of 6 × 6 × 6 colours and 24 greys.
fn xterm(index: u8) -> Color {
    let level = |step: u8| if step == 0 { 0 } else { 55 + 40 * step };
    match index {
        0..=15 => {
            let (red, green, blue) = XTERM[usize::from(index)];
            Color::Rgb(red, green, blue)
        }
        16..=231 => {
            let cube = index - 16;
            Color::Rgb(level(cube / 36), level(cube / 6 % 6), level(cube % 6))
        }
        232..=255 => {
            let grey = 8 + 10 * (index - 232);
            Color::Rgb(grey, grey, grey)
        }
    }
}

/// Hue in turns, saturation and lightness of an RGB colour.
fn hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let [r, g, b] = [r, g, b].map(|c| f64::from(c) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, lightness);
    }
    let saturation = d / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (hue, saturation, lightness)
}

/// The RGB colour of a hue in turns, a saturation and a lightness.
fn rgb(hue: f64, saturation: f64, lightness: f64) -> (u8, u8, u8) {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let h = hue * 6.0;
    let x = chroma * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    let byte = |c: f64| ((c + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (byte(r), byte(g), byte(b))
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
    fn contrast_is_the_wcag_ratio() {
        let black = Color::Rgb(0, 0, 0);
        let white = Color::Rgb(255, 255, 255);
        assert_eq!(black.contrast(white), Some(21.0));
        assert_eq!(white.contrast(white), Some(1.0));
        // GitHub's grey text on white is just above 4.5.
        let grey = Color::Rgb(0x76, 0x76, 0x76).contrast(white).unwrap();
        assert!((4.5..4.6).contains(&grey), "{grey}");
        assert_eq!(Color::Palette(1).contrast(white), None);
    }

    #[test]
    fn a_pale_colour_darkens_on_a_light_background_keeping_its_hue() {
        let white = Color::Rgb(255, 255, 255);
        // InspiredGitHub's pale green YAML keys.
        let green = Color::Rgb(0x63, 0xa3, 0x5c);
        let readable = green.readable_on(white, 4.5);
        let ratio = readable.contrast(white).unwrap();
        assert!((4.5..4.7).contains(&ratio), "{readable:?}: {ratio}");
        assert!((hue(readable) - hue(green)).abs() <= 2, "{readable:?}");
        // Darker, still green.
        let Color::Rgb(r, g, b) = readable else {
            panic!()
        };
        assert!(g > r && g > b && g < 0xa3, "{readable:?}");
    }

    #[test]
    fn a_dim_colour_lightens_on_a_dark_background() {
        let dark = Color::Rgb(0x2b, 0x30, 0x3b);
        // base16-ocean.dark's comments.
        let comment = Color::Rgb(0x65, 0x73, 0x7e);
        let readable = comment.readable_on(dark, 4.5);
        let ratio = readable.contrast(dark).unwrap();
        assert!((4.5..4.7).contains(&ratio), "{readable:?}: {ratio}");
        assert!((hue(readable) - hue(comment)).abs() <= 2, "{readable:?}");
    }

    #[test]
    fn a_readable_colour_stays_as_it_is() {
        let white = Color::Rgb(255, 255, 255);
        let navy = Color::Rgb(0x18, 0x36, 0x91);
        assert_eq!(navy.readable_on(white, 4.5), navy);
        assert_eq!(Color::Palette(2).readable_on(white, 4.5), Color::Palette(2));
    }

    /// The hue of an RGB colour in whole degrees.
    fn hue(color: Color) -> i32 {
        let Color::Rgb(r, g, b) = color else { panic!() };
        (hsl(r, g, b).0 * 360.0).round() as i32
    }

    #[test]
    fn rgb_or_xterm_takes_xterm_s_colours_where_the_terminal_tells_none() {
        let mut palette = [None; 16];
        palette[1] = Some(Color::Rgb(1, 2, 3));
        let rgb = |index| Color::Palette(index).rgb_or_xterm(&palette);
        assert_eq!(rgb(1), Color::Rgb(1, 2, 3));
        assert_eq!(rgb(4), Color::Rgb(0, 0, 0xee));
        assert_eq!(rgb(5), Color::Rgb(0xcd, 0, 0xcd));
        assert_eq!(rgb(12), Color::Rgb(0x5c, 0x5c, 0xff));
        // The colour cube and the greys.
        assert_eq!(rgb(16), Color::Rgb(0, 0, 0));
        assert_eq!(rgb(196), Color::Rgb(0xff, 0, 0));
        assert_eq!(rgb(110), Color::Rgb(0x87, 0xaf, 0xd7));
        assert_eq!(rgb(232), Color::Rgb(8, 8, 8));
        assert_eq!(rgb(255), Color::Rgb(0xee, 0xee, 0xee));
        assert_eq!(
            Color::Rgb(9, 9, 9).rgb_or_xterm(&palette),
            Color::Rgb(9, 9, 9)
        );
    }

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
