//! A style that fades into another over a moment: a search match just
//! jumped to, going over from the text's own colours into the mark of the
//! current match, or a message on the status line fading out.

use std::time::Duration;

use crate::typeset::color::Rgb;
use crate::typeset::style::Style;

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
        match self.share(length) {
            None => Some(to.clone()),
            Some(share) => self.rgb?.mix(from, to, share),
        }
    }

    /// How far a fade that takes `length` has come, from 0 to 1; `None` when
    /// it is over.
    pub fn share(&self, length: Duration) -> Option<f32> {
        (self.elapsed < length).then(|| self.elapsed.as_secs_f32() / length.as_secs_f32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::color::Color;
    use crate::typeset::theme::Theme;

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
        let mut caps = Capabilities::kitty(80);
        caps.foreground = Some(Color::Rgb(200, 200, 200));
        caps.background = Some(Color::Rgb(0, 0, 0));
        caps.palette[0] = Some(Color::Rgb(0, 0, 0));
        caps.palette[3] = Some(Color::Rgb(200, 200, 0));
        let rgb = Rgb::new(&Theme::default(), &caps).unwrap();
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
