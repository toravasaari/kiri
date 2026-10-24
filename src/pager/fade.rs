//! A style that fades into another over a moment: a search match just
//! jumped to, going over from the text's own colours into the mark of the
//! current match, or a message on the status line fading out.

use std::time::Duration;

use crate::caps::Capabilities;
use crate::color::Color;
use crate::render::style::Style;
use crate::theme::Theme;

/// How long a fade takes.
pub const FADE: Duration = Duration::from_millis(750);

/// How long the match number stays in its end style, all at once, where
/// it cannot fade.
pub const FLASH: Duration = Duration::from_millis(500);

/// How long the status line tells that a reload worked, as it is, before
/// the message begins to fade out.
pub const STAY: Duration = Duration::from_secs(4);

/// How long that message takes to fade out into the status line's
/// background. Where it cannot fade, it goes at the end of this all at
/// once.
pub const FADE_OUT: Duration = Duration::from_secs(3);

/// How long the warning line stays, as it is, before it begins to fade
/// out over [`FADE`] into the page's background.
pub const WARNING_STAY: Duration = Duration::from_secs(5);

/// How long the lines of the change auto-follow went to stay tinted, as
/// they are, before the tint begins to fade.
pub const HIGHLIGHT_STAY: Duration = Duration::from_secs(1);

/// How long the tint takes to fade out into the page's background.
pub const HIGHLIGHT_FADE: Duration = Duration::from_secs(2);

/// How much of the text colour is mixed into the background of those
/// lines at the start.
pub const HIGHLIGHT_SHARE: f32 = 0.15;

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
        let rgb = |color: Option<Color>| color?.rgb(&caps.palette);
        Some(Self {
            foreground: rgb(theme.foreground.or(caps.foreground)),
            background: rgb(theme.background.or(caps.background)),
            palette: caps.palette,
        })
    }

    /// `color` as RGB; `None` when it is a palette colour not known.
    fn of(&self, color: Color) -> Option<Color> {
        color.rgb(&self.palette)
    }

    /// `bg`, or the page's background where there is none, as RGB with
    /// `amount` of the text colour mixed into it; `None` when any of them
    /// is not known as RGB.
    pub fn tinted(&self, bg: Option<Color>, amount: f32) -> Option<Color> {
        let base = match bg {
            Some(color) => self.of(color)?,
            None => self.background?,
        };
        base.mix(self.foreground?, amount)
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
    /// `from` faded as far into `to` as the fade has come, their colours
    /// mixed as RGB; `to` at once where either's colours are not known as
    /// RGB.
    pub fn step(&self, from: &Style, to: &Style) -> Style {
        self.mix(from, to).unwrap_or_else(|| to.clone())
    }

    /// `from` faded as far into `to` as the fade has come, their colours
    /// mixed as RGB, in `to`'s bold, underline and the rest all the way;
    /// `None` where either's colours are not known as RGB.
    pub fn mix(&self, from: &Style, to: &Style) -> Option<Style> {
        self.mix_over(FADE, from, to)
    }

    /// As [`Fade::mix`], for a fade that takes `length`.
    pub fn mix_over(&self, length: Duration, from: &Style, to: &Style) -> Option<Style> {
        if self.elapsed >= length {
            return Some(to.clone());
        }
        let rgb = self.rgb?;
        let ((fg_from, bg_from), (fg_to, bg_to)) = (rgb.colors(from)?, rgb.colors(to)?);
        let amount = self.elapsed.as_secs_f32() / length.as_secs_f32();
        Some(Style {
            fg: fg_from.mix(fg_to, amount),
            bg: bg_from.mix(bg_to, amount),
            reverse: false,
            ..to.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb() -> Rgb {
        let mut caps = Capabilities::kitty(80);
        caps.foreground = Some(Color::Rgb(200, 200, 200));
        caps.background = Some(Color::Rgb(0, 0, 0));
        caps.palette[3] = Some(Color::Rgb(200, 200, 0));
        Rgb::new(&Theme::default(), &caps).unwrap()
    }

    /// A fade `millis` milliseconds in, mixing with `rgb`.
    fn fade(millis: u64, rgb: Option<&Rgb>) -> Fade<'_> {
        Fade {
            elapsed: Duration::from_millis(millis),
            rgb,
        }
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
    fn a_fade_goes_from_the_text_s_own_colours_into_their_reverse() {
        // Yellow text on the page, and the current match's reverse video
        // laid over it: yellow on black into black on yellow.
        let from = colored(Some(Color::Palette(3)), None);
        let to = Style {
            bold: true,
            reverse: true,
            ..from.clone()
        };
        let rgb = rgb();
        let at = |millis| fade(millis, Some(&rgb)).step(&from, &to);
        let yellow = Some(Color::Rgb(200, 200, 0));
        let black = Some(Color::Rgb(0, 0, 0));
        let start = at(0);
        assert_eq!((start.fg, start.bg), (yellow, black));
        // In the mark's bold all the way, without its reverse video.
        assert!(start.bold && !start.reverse);
        let midway = at(300);
        assert_eq!(
            (midway.fg, midway.bg),
            (Some(Color::Rgb(120, 120, 0)), Some(Color::Rgb(80, 80, 0)))
        );
        assert!(midway.bold && !midway.reverse);
        let end = at(749);
        assert_eq!((end.fg, end.bg), (black, yellow));
        assert_eq!(at(750), to);
        assert_eq!(at(1000), to);
    }

    #[test]
    fn a_fade_goes_into_a_theme_s_own_colours() {
        let from = Style::default();
        let to = Style {
            bold: true,
            ..colored(Some(Color::Palette(0)), Some(Color::Palette(3)))
        };
        let mut rgb = rgb();
        rgb.palette[0] = Some(Color::Rgb(0, 0, 0));
        let at = |millis| fade(millis, Some(&rgb)).step(&from, &to);
        let start = at(0);
        assert_eq!(
            (start.fg, start.bg),
            (Some(Color::Rgb(200, 200, 200)), Some(Color::Rgb(0, 0, 0)))
        );
        assert!(start.bold);
        let midway = at(375);
        assert_eq!(
            (midway.fg, midway.bg),
            (
                Some(Color::Rgb(100, 100, 100)),
                Some(Color::Rgb(100, 100, 0))
            )
        );
        assert_eq!(at(750), to);
    }

    #[test]
    fn a_fade_can_take_longer() {
        let from = Style::default();
        let to = colored(Some(Color::Rgb(0, 0, 0)), None);
        let rgb = rgb();
        let at = |millis| fade(millis, Some(&rgb)).mix_over(FADE_OUT, &from, &to);
        let grey = |level| Some(Color::Rgb(level, level, level));
        assert_eq!(at(0).unwrap().fg, grey(200));
        assert_eq!(at(1500).unwrap().fg, grey(100));
        assert_eq!(at(3000), Some(to));
    }

    #[test]
    fn colours_not_known_give_the_end_style_at_once() {
        let to = Style {
            bold: true,
            reverse: true,
            ..Style::default()
        };
        // Palette colour 4 is not known.
        let unknown = colored(Some(Color::Palette(4)), None);
        for (from, rgb) in [
            (Style::default(), None),
            (unknown.clone(), None),
            (unknown, Some(&rgb())),
        ] {
            assert_eq!(fade(0, rgb).step(&from, &to), to);
            assert_eq!(fade(400, rgb).step(&from, &to), to);
        }
    }
}
