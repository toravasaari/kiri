//! A style that fades into another over a moment: the strong colours of a
//! search match just jumped to, going over into its ordinary ones.

use std::time::Duration;

use crate::color::Color;
use crate::render::style::Style;
use crate::term::Capabilities;
use crate::theme::Theme;

/// How long a fade takes.
pub const FADE: Duration = Duration::from_millis(1500);

/// How long the strong style stays, all at once, where it cannot fade.
pub const FLASH: Duration = Duration::from_secs(1);

/// The colours of the page and of the terminal's palette as RGB, where
/// they are known: what a fade mixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgb {
    foreground: Option<Color>,
    background: Option<Color>,
    palette: [Option<Color>; 16],
}

impl Rgb {
    /// The colours of `theme`'s page on a terminal with `caps`; `None`
    /// without colours.
    pub fn new(theme: &Theme, caps: &Capabilities) -> Option<Self> {
        if !caps.color {
            return None;
        }
        let mut rgb = Self {
            foreground: None,
            background: None,
            palette: caps.palette,
        };
        rgb.foreground = theme.foreground.or(caps.foreground).and_then(|c| rgb.of(c));
        rgb.background = theme.background.or(caps.background).and_then(|c| rgb.of(c));
        Some(rgb)
    }

    /// `color` as RGB; `None` when it is a palette colour not known.
    fn of(&self, color: Color) -> Option<Color> {
        match color {
            Color::Rgb(..) => Some(color),
            Color::Palette(index) => *self.palette.get(usize::from(index))?,
        }
    }

    /// The colours text in `style` shows in, its text's and its
    /// background's, as RGB; `None` when either is not known.
    pub fn colors(&self, style: &Style) -> Option<(Color, Color)> {
        let foreground = match style.fg {
            Some(color) => self.of(color)?,
            None => self.foreground?,
        };
        let background = match style.bg {
            Some(color) => self.of(color)?,
            None => self.background?,
        };
        Some(match style.reverse {
            true => (background, foreground),
            false => (foreground, background),
        })
    }
}

/// A fade under way: how long ago it began, and the colours it mixes.
#[derive(Debug, Clone, Copy)]
pub struct Fade<'a> {
    pub elapsed: Duration,
    pub rgb: Option<&'a Rgb>,
}

impl Fade<'_> {
    /// `from` faded as far into `to` as the fade has come.
    pub fn step(&self, from: &Style, to: &Style) -> Style {
        step(from, to, self.elapsed, self.rgb)
    }
}

/// `from` faded into `to` for `elapsed` of [`FADE`], their colours mixed
/// with `rgb`. Where either's colours are not known as RGB, `from` stays
/// for [`FLASH`] and then gives way to `to` at once.
pub fn step(from: &Style, to: &Style, elapsed: Duration, rgb: Option<&Rgb>) -> Style {
    if elapsed >= FADE {
        return to.clone();
    }
    let colors = rgb.and_then(|rgb| Some((rgb.colors(from)?, rgb.colors(to)?)));
    match colors {
        Some(((fg_from, bg_from), (fg_to, bg_to))) => {
            let amount = elapsed.as_secs_f32() / FADE.as_secs_f32();
            Style {
                fg: fg_from.mix(fg_to, amount),
                bg: bg_from.mix(bg_to, amount),
                reverse: false,
                ..from.clone()
            }
        }
        None if elapsed < FLASH => from.clone(),
        None => to.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Underline;

    fn rgb() -> Rgb {
        let mut caps = Capabilities::kitty(80);
        caps.foreground = Some(Color::Rgb(200, 200, 200));
        caps.background = Some(Color::Rgb(0, 0, 0));
        caps.palette[3] = Some(Color::Rgb(200, 200, 0));
        Rgb::new(&Theme::default(), &caps).unwrap()
    }

    fn colored(fg: Option<Color>, bg: Option<Color>) -> Style {
        Style {
            fg,
            bg,
            ..Style::default()
        }
    }

    #[test]
    fn text_without_colours_of_its_own_is_in_the_page_s() {
        let grey = Color::Rgb(200, 200, 200);
        let black = Color::Rgb(0, 0, 0);
        assert_eq!(rgb().colors(&Style::default()), Some((grey, black)));
        let reverse = Style {
            reverse: true,
            ..Style::default()
        };
        assert_eq!(rgb().colors(&reverse), Some((black, grey)));
        let yellow = Color::Rgb(200, 200, 0);
        let marked = colored(Some(Color::Palette(3)), Some(Color::Rgb(1, 2, 3)));
        assert_eq!(rgb().colors(&marked), Some((yellow, Color::Rgb(1, 2, 3))));
    }

    #[test]
    fn colours_not_known_as_rgb_are_none() {
        // Palette colours the terminal did not tell, and beyond the 16.
        assert_eq!(rgb().colors(&colored(Some(Color::Palette(4)), None)), None);
        assert_eq!(rgb().colors(&colored(None, Some(Color::Palette(16)))), None);
        let caps = Capabilities::kitty(80);
        let unknown = Rgb::new(&Theme::default(), &caps).unwrap();
        assert_eq!(unknown.colors(&Style::default()), None);
        // The theme's page wins over the terminal's.
        let theme = Theme::paper(crate::theme::Paper::Light);
        let paper = Rgb::new(&theme, &caps).unwrap();
        assert_eq!(
            paper.colors(&Style::default()),
            Some((Color::Rgb(0x1f, 0x1f, 0x1f), Color::Rgb(0xff, 0xff, 0xff)))
        );
    }

    #[test]
    fn without_colours_nothing_is_mixed() {
        let mut caps = Capabilities::kitty(80);
        caps.foreground = Some(Color::Rgb(200, 200, 200));
        caps.background = Some(Color::Rgb(0, 0, 0));
        caps.color = false;
        assert_eq!(Rgb::new(&Theme::default(), &caps), None);
    }

    #[test]
    fn a_fade_mixes_the_colours_from_one_style_into_the_other() {
        let from = Style {
            bold: true,
            ..colored(Some(Color::Palette(0)), Some(Color::Palette(3)))
        };
        let to = Style {
            reverse: true,
            ..Style::default()
        };
        let mut rgb = rgb();
        rgb.palette[0] = Some(Color::Rgb(0, 0, 0));
        let at = |millis| step(&from, &to, Duration::from_millis(millis), Some(&rgb));
        let start = at(0);
        assert_eq!(
            (start.fg, start.bg),
            (Some(Color::Rgb(0, 0, 0)), Some(Color::Rgb(200, 200, 0)))
        );
        assert!(start.bold && !start.reverse);
        let midway = at(750);
        assert_eq!(
            (midway.fg, midway.bg),
            (Some(Color::Rgb(0, 0, 0)), Some(Color::Rgb(200, 200, 100)))
        );
        assert_eq!(at(1500), to);
        assert_eq!(at(2000), to);
    }

    #[test]
    fn colours_not_known_flash_for_a_second_without_steps() {
        let from = Style {
            underline: Underline::Single,
            ..Style::default()
        };
        let to = Style {
            reverse: true,
            ..Style::default()
        };
        for rgb in [None, Some(&rgb())] {
            let at = |millis| step(&from, &to, Duration::from_millis(millis), rgb);
            // Palette colour 4 is not known.
            let unknown = colored(Some(Color::Palette(4)), None);
            let from_unknown = |millis| step(&unknown, &to, Duration::from_millis(millis), rgb);
            if rgb.is_none() {
                assert_eq!(at(0), from);
                assert_eq!(at(999), from);
                assert_eq!(at(1000), to);
            }
            assert_eq!(from_unknown(500), unknown);
            assert_eq!(from_unknown(1000), to);
        }
    }
}
