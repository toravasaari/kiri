//! The status line at the bottom of the pager: a badge that tells the
//! pager is on, the document's name, how far down it the view is, and the
//! keys for help and quitting.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::view::Position;
use crate::color::Color;
use crate::render::style::{Span, Style};
use crate::term::Capabilities;
use crate::theme::{StatusColor, Theme};

/// The styles of the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Styles {
    pub bar: Style,
    pub badge: Style,
}

impl Styles {
    /// The status line for `theme` on a terminal with `caps`. Without
    /// colours from the theme, the bar is a little apart from the page with
    /// the badge in reverse video; when the page's colour is not known
    /// either, the bar is in reverse video and the badge in bold.
    pub fn new(theme: &Theme, caps: &Capabilities) -> Self {
        let status = &theme.status_line;
        let bar = match status.background {
            _ if !caps.color => None,
            StatusColor::Color(color) => Some(color),
            StatusColor::Auto => mixed(theme, caps).map(Some),
        };
        let Some(background) = bar else {
            return Self {
                bar: Style {
                    reverse: true,
                    ..Style::default()
                },
                badge: Style {
                    bold: true,
                    ..Style::default()
                },
            };
        };
        let color = |color, auto| match color {
            StatusColor::Color(color) => color,
            StatusColor::Auto => auto,
        };
        let badge = match (status.badge_foreground, status.badge_background) {
            (StatusColor::Auto, StatusColor::Auto) => Style {
                reverse: true,
                fg: theme.foreground,
                bg: theme.background,
                ..Style::default()
            },
            (foreground, background) => Style {
                fg: color(foreground, None),
                bg: color(background, None),
                ..Style::default()
            },
        };
        Self {
            bar: Style {
                fg: color(status.foreground, theme.foreground),
                bg: background,
                ..Style::default()
            },
            badge: Style {
                bold: true,
                ..badge
            },
        }
    }
}

/// The page mixed halfway with bright black, a grey in most palettes;
/// `None` when either is not known as RGB.
fn mixed(theme: &Theme, caps: &Capabilities) -> Option<Color> {
    let rgb = |color: Color| match color {
        Color::Rgb(..) => Some(color),
        Color::Palette(index) => *caps.palette.get(usize::from(index))?,
    };
    let page = rgb(theme.background.or(caps.background)?)?;
    page.mix(rgb(Color::Palette(8))?, 0.5)
}

/// The status line for the document `name` at `position`, `width` cells
/// wide. A narrow window leaves out the keys first, then the start of the
/// name.
pub fn spans(name: &str, position: Position, width: usize, styles: &Styles) -> Vec<Span> {
    let badge = if position.end { " END " } else { " kiri " };
    let percent = format!("{}%", position.percent);
    let keys = [("?", " help"), ("q", " quit")];
    let keys_width: usize = keys
        .iter()
        .map(|(key, what)| 3 + key.width() + what.width())
        .sum();
    // The badge, the position and a space after it, and a space between
    // the badge and the name.
    let fixed = badge.width() + percent.width() + 2;
    if fixed > width {
        // The badge is ASCII.
        let badge = &badge[..width.min(badge.len())];
        return [Span::new(badge, styles.badge.clone())]
            .into_iter()
            .filter(|span| !span.text.is_empty())
            .collect();
    }
    // The name needs a space between it and the position.
    let with_keys = fixed + name.width() + 1 + keys_width <= width;
    let keys_width = if with_keys { keys_width } else { 0 };
    let room = (width - fixed - keys_width).saturating_sub(1);

    let bold = Style {
        bold: true,
        ..styles.bar.clone()
    };
    let mut spans = vec![Span::new(badge, styles.badge.clone())];
    let shown = tail(name, room);
    spans.push(Span::new(&format!(" {shown}"), styles.bar.clone()));
    let mut right = vec![Span::new(&percent, styles.bar.clone())];
    if with_keys {
        for (key, what) in keys {
            right.push(Span::new("   ", styles.bar.clone()));
            right.push(Span::new(key, bold.clone()));
            right.push(Span::new(what, styles.bar.clone()));
        }
    }
    right.push(Span::new(" ", styles.bar.clone()));
    let used = width_of(&spans) + width_of(&right);
    spans.push(Span::new(
        &" ".repeat(width.saturating_sub(used)),
        styles.bar.clone(),
    ));
    spans.extend(right);
    spans
}

/// The end of `name` in at most `room` cells, after `…` when its start is
/// left out; empty when not even one character fits after `…`.
fn tail(name: &str, room: usize) -> String {
    if name.width() <= room {
        return name.to_owned();
    }
    let mut kept = Vec::new();
    let mut used = 1;
    for c in name.chars().rev() {
        let cells = c.width().unwrap_or(0);
        if used + cells > room {
            break;
        }
        used += cells;
        kept.push(c);
    }
    if kept.is_empty() {
        return String::new();
    }
    std::iter::once('…').chain(kept.into_iter().rev()).collect()
}

fn width_of(spans: &[Span]) -> usize {
    crate::render::wrap::width(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::wrap::width;
    use crate::theme::{Paper, StatusLineTheme};

    fn at(percent: u8, end: bool) -> Position {
        Position { percent, end }
    }

    fn styles() -> Styles {
        Styles {
            bar: Style::fg(Some(Color::Palette(1))),
            badge: Style::fg(Some(Color::Palette(2))),
        }
    }

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn shows_the_badge_the_name_the_position_and_the_keys() {
        let spans = spans("notes.md", at(42, false), 60, &styles());
        assert_eq!(
            text(&spans),
            " kiri  notes.md                       42%   ? help   q quit "
        );
        assert_eq!(width(&spans), 60);
        assert_eq!(spans[0].style, styles().badge);
        assert!(
            spans[1..]
                .iter()
                .all(|span| span.style.fg == styles().bar.fg)
        );
    }

    #[test]
    fn the_badge_says_end_at_the_end() {
        let spans = spans("notes.md", at(100, true), 60, &styles());
        assert!(text(&spans).starts_with(" END  notes.md "), "{spans:?}");
    }

    #[test]
    fn the_keys_go_first_when_the_window_is_narrow() {
        let spans = spans("notes.md", at(42, false), 30, &styles());
        assert_eq!(text(&spans), " kiri  notes.md           42% ");
    }

    #[test]
    fn a_long_name_loses_its_start() {
        let spans = spans("docs/a/long/way/to/roadmap.md", at(7, false), 26, &styles());
        assert_eq!(text(&spans), " kiri  …/to/roadmap.md 7% ");
        assert_eq!(width(&spans), 26);
    }

    #[test]
    fn the_line_fills_the_window_and_no_more() {
        for width in 0..80 {
            let spans = spans("docs/notes.md", at(42, false), width, &styles());
            let expected = if width < 11 { width.min(6) } else { width };
            assert_eq!(super::width_of(&spans), expected, "{width}: {spans:?}");
        }
    }

    #[test]
    fn the_bar_is_mixed_from_the_page_and_grey_when_both_are_known() {
        let mut caps = Capabilities::kitty(80);
        caps.background = Some(Color::Rgb(0, 0, 0));
        caps.palette[8] = Some(Color::Rgb(100, 100, 100));
        let styles = Styles::new(&Theme::default(), &caps);
        assert_eq!(styles.bar.bg, Some(Color::Rgb(50, 50, 50)));
        assert!(!styles.bar.reverse);
        assert!(styles.badge.reverse && styles.badge.bold);
    }

    #[test]
    fn colours_from_the_theme_win() {
        let mut theme = Theme::default();
        let red = Some(Color::Palette(1));
        let blue = Some(Color::Palette(4));
        theme.status_line = StatusLineTheme {
            background: StatusColor::Color(red),
            foreground: StatusColor::Color(blue),
            badge_background: StatusColor::Color(blue),
            badge_foreground: StatusColor::Color(None),
        };
        let styles = Styles::new(&theme, &Capabilities::kitty(80));
        assert_eq!((styles.bar.fg, styles.bar.bg), (blue, red));
        assert!(!styles.bar.reverse);
        assert_eq!((styles.badge.fg, styles.badge.bg), (None, blue));
        assert!(styles.badge.bold && !styles.badge.reverse);
    }

    #[test]
    fn the_paper_themes_have_grey_status_lines() {
        let grey = |level| Some(Color::Rgb(level, level, level));
        let caps = Capabilities::kitty(80);
        let light = Styles::new(&Theme::paper(Paper::Light), &caps);
        assert_eq!((light.bar.fg, light.bar.bg), (grey(0x1f), grey(0xe0)));
        assert_eq!((light.badge.fg, light.badge.bg), (grey(0xff), grey(0x1f)));
        let dark = Styles::new(&Theme::paper(Paper::Dark), &caps);
        assert_eq!((dark.bar.fg, dark.bar.bg), (grey(0xe0), grey(0x3a)));
        assert_eq!((dark.badge.fg, dark.badge.bg), (grey(0x1e), grey(0xd4)));
    }

    #[test]
    fn without_known_colours_the_bar_is_in_reverse_video() {
        let mut caps = Capabilities::kitty(80);
        caps.background = Some(Color::Rgb(0, 0, 0));
        caps.palette[8] = Some(Color::Rgb(100, 100, 100));
        caps.color = false;
        for caps in [Capabilities::kitty(80), caps] {
            let styles = Styles::new(&Theme::default(), &caps);
            assert!(styles.bar.reverse, "{caps:?}");
            assert_eq!(styles.bar.bg, None);
            assert!(styles.badge.bold && !styles.badge.reverse);
        }
    }

    #[test]
    fn a_window_too_narrow_for_more_shows_the_badge() {
        assert_eq!(
            text(&spans("notes.md", at(42, false), 4, &styles())),
            " kir"
        );
        assert_eq!(text(&spans("notes.md", at(42, false), 0, &styles())), "");
    }
}
