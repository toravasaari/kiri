//! The status line at the bottom of the pager: a badge that tells the
//! pager is on, the document's name, how far down it the view is, and the
//! keys for help and quitting.

use unicode_width::UnicodeWidthStr;

use super::fade::{FADE_OUT, FLASH, Fade};
use super::view::Position;
use crate::caps::Capabilities;
use crate::color::Color;
use crate::render::style::{self, Span, Style};
use crate::render::wrap;
use crate::theme::{StatusColor, Theme};

/// The styles of the status line, and the one the match number fades
/// into after a search went around an end of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Styles {
    pub bar: Style,
    pub badge: Style,
    pub inverse: Style,
}

impl Styles {
    /// The status line for `theme` on a terminal with `caps`. Without
    /// colours from the theme, the bar is a little apart from the page with
    /// the badge in reverse video; when the page's colour is not known
    /// either, the bar is in reverse video and the badge in bold.
    pub fn new(theme: &Theme, caps: &Capabilities) -> Self {
        let (bar, badge) = bar_and_badge(theme, caps);
        // Bold, and in reverse video against the bar.
        let inverse = Style {
            bold: true,
            reverse: !bar.reverse,
            ..bar.clone()
        };
        Self {
            bar,
            badge,
            inverse,
        }
    }

    /// The match number's style `fade` into a search going around an
    /// end: the reverse of the bar's colours fading back into them. Where
    /// they cannot fade, the reverse for [`FLASH`] and then the bar's.
    pub fn number(&self, fade: Fade) -> Style {
        match fade.mix(&self.inverse, &self.bar) {
            Some(style) => style,
            None if fade.elapsed < FLASH => self.inverse.clone(),
            None => self.bar.clone(),
        }
    }

    /// The style of a message at `fade` through its fade out: the bar's
    /// colours, with the text going into the bar's background. `None`
    /// where they are not known as RGB, so the message stays as it is.
    pub fn fading(&self, fade: Fade) -> Option<Style> {
        let (_, background) = fade.rgb?.colors(&self.bar)?;
        let gone = Style {
            fg: Some(background),
            bg: Some(background),
            reverse: false,
            ..self.bar.clone()
        };
        fade.mix_over(FADE_OUT, &self.bar, &gone)
    }
}

/// The styles of the bar and the badge for `theme` on a terminal with
/// `caps`, as [`Styles::new`] tells.
fn bar_and_badge(theme: &Theme, caps: &Capabilities) -> (Style, Style) {
    let status = &theme.status_line;
    let bar = match status.background {
        _ if !caps.color => None,
        StatusColor::Color(color) => Some(color),
        StatusColor::Auto => mixed(theme, caps).map(Some),
    };
    let Some(background) = bar else {
        return (
            Style {
                reverse: true,
                ..Style::default()
            },
            Style {
                bold: true,
                ..Style::default()
            },
        );
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
    (
        Style {
            fg: color(status.foreground, theme.foreground),
            bg: background,
            ..Style::default()
        },
        Style {
            bold: true,
            ..badge
        },
    )
}

/// The page mixed halfway with bright black, a grey in most palettes;
/// `None` when either is not known as RGB.
fn mixed(theme: &Theme, caps: &Capabilities) -> Option<Color> {
    let page = theme.background.or(caps.background)?.rgb(&caps.palette)?;
    page.mix(Color::Palette(8).rgb(&caps.palette)?, 0.5)
}

/// What the status line tells about the last search, or in its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found<'a> {
    /// The reader is at match `current`, counted from 1, of the `total`
    /// matches of `term`; `flash` is a style that shows `current` apart
    /// for a moment, after the search went around an end of the document.
    At {
        term: &'a str,
        current: usize,
        total: usize,
        flash: Option<Style>,
    },
    /// `term` is nowhere in the document.
    Nowhere(&'a str),
    /// Not about a search: a message about the last key, such as that
    /// there is no next file, told where the search would be; `fading`
    /// is its style while it fades out.
    Told {
        message: &'a str,
        fading: Option<Style>,
    },
}

/// The status line for the document `name` at `position`, with what was
/// `found` by a search, `width` cells wide. `place` is the file's number,
/// from 1, and how many files there are, which the line tells after the
/// name. A narrow window leaves out the keys first, then the start of the
/// name and then of its place, then the search.
pub fn spans(
    name: &str,
    place: Option<(usize, usize)>,
    position: Position,
    found: Option<Found>,
    width: usize,
    styles: &Styles,
) -> Vec<Span> {
    // Measured as shown: a file name may hold any character but `/`.
    let mut name = printable(name);
    if let Some((number, count)) = place {
        name = format!("{name} {number}/{count}");
    }
    let name = name.as_str();
    let percent = format!("{}%", position.percent);
    let keys = [("?", " help"), ("q", " quit")];
    let keys_width: usize = keys
        .iter()
        .map(|(key, what)| 3 + key.width() + what.width())
        .sum();
    // The badge, the position and a space after it, and a space between
    // the badge and the name.
    let fixed = BADGE.width() + percent.width() + 2;
    if fixed > width {
        return badge_only(width, styles);
    }
    // What a message tells the reader must not be left out for want of
    // room, so it loses its end instead.
    let told;
    let found = match found {
        Some(Found::Told { message, fading }) => {
            told = head(message, (width - fixed).saturating_sub(3));
            Some(Found::Told {
                message: &told,
                fading,
            })
        }
        found => found,
    };
    // The search and a gap of three before the position.
    let search = found.map(|found| search(found, styles)).unwrap_or_default();
    let search_width = if search.is_empty() {
        0
    } else {
        wrap::width(&search) + 3
    };
    let with_search = fixed + search_width <= width;
    let search_width = if with_search { search_width } else { 0 };
    // The name needs a space between it and what comes after it.
    let with_keys = fixed + search_width + name.width() + 1 + keys_width <= width;
    let keys_width = if with_keys { keys_width } else { 0 };
    let room = (width - fixed - search_width - keys_width).saturating_sub(1);

    let bold = Style {
        bold: true,
        ..styles.bar.clone()
    };
    let mut spans = vec![Span::new(BADGE, styles.badge.clone())];
    let shown = wrap::tail(name, room);
    spans.push(Span::new(&format!(" {shown}"), styles.bar.clone()));
    let mut right = Vec::new();
    if with_search && !search.is_empty() {
        right.extend(search);
        right.push(Span::new("   ", styles.bar.clone()));
    }
    right.push(Span::new(&percent, styles.bar.clone()));
    if with_keys {
        for (key, what) in keys {
            right.push(Span::new("   ", styles.bar.clone()));
            right.push(Span::new(key, bold.clone()));
            right.push(Span::new(what, styles.bar.clone()));
        }
    }
    right.push(Span::new(" ", styles.bar.clone()));
    let used = wrap::width(&spans) + wrap::width(&right);
    spans.push(Span::new(
        &" ".repeat(width.saturating_sub(used)),
        styles.bar.clone(),
    ));
    spans.extend(right);
    spans
}

/// The start of `text` in at most `max` cells, before `…` when its end is
/// left out.
fn head(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_owned();
    }
    let kept = wrap::cut(
        vec![Span::new(text, Style::default())],
        max.saturating_sub(1),
    );
    format!("{}…", style::text(&kept))
}

/// The spans that tell what was `found`: the term and the place among
/// its matches, or that it is nowhere.
fn search(found: Found, styles: &Styles) -> Vec<Span> {
    let span = |text: &str| Span::new(text, styles.bar.clone());
    match found {
        Found::At {
            term,
            current,
            total,
            flash,
        } => vec![
            span(&format!("/{} ", printable(term))),
            match flash {
                Some(style) => Span::new(&current.to_string(), style),
                None => span(&current.to_string()),
            },
            span(&format!("/{total}")),
        ],
        Found::Nowhere(term) => vec![span(&format!("not found: {}", printable(term)))],
        Found::Told { message, fading } => vec![match fading {
            Some(style) => Span::new(message, style),
            None => span(message),
        }],
    }
}

/// The status line while a prompt is open with `term` typed so far after
/// `mark`, a slash for a search or a colon for a command: the badge, then
/// the prompt with a cursor after the term. A term too long for the
/// window loses its start.
pub fn prompt(mark: char, term: &str, width: usize, styles: &Styles) -> Vec<Span> {
    // The badge, and a space, the mark and the cursor.
    let fixed = BADGE.width() + 3;
    if fixed > width {
        return badge_only(width, styles);
    }
    let shown = wrap::tail(&printable(term), width - fixed);
    let mut spans = vec![
        Span::new(BADGE, styles.badge.clone()),
        Span::new(&format!(" {mark}{shown}{CURSOR}"), styles.bar.clone()),
    ];
    let used = wrap::width(&spans);
    spans.push(Span::new(&" ".repeat(width - used), styles.bar.clone()));
    spans
}

/// The badge that tells the pager is on.
const BADGE: &str = " kiri ";

/// The cursor after the term in the prompt.
const CURSOR: char = '\u{258f}';

/// The badge alone, cut to `width` cells, for a window too narrow for
/// more.
fn badge_only(width: usize, styles: &Styles) -> Vec<Span> {
    // The badge is ASCII.
    let badge = &BADGE[..width.min(BADGE.len())];
    [Span::new(badge, styles.badge.clone())]
        .into_iter()
        .filter(|span| !span.text.is_empty())
        .collect()
}

/// `text` with each control character replaced, so that it takes as many
/// cells as it is measured to.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            c if c.is_control() => char::REPLACEMENT_CHARACTER,
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::style::text;
    use crate::render::wrap::width;
    use crate::theme::{Paper, StatusLineTheme};

    fn at(percent: u8) -> Position {
        Position { percent }
    }

    fn styles() -> Styles {
        Styles {
            bar: Style::fg(Some(Color::Palette(1))),
            badge: Style::fg(Some(Color::Palette(2))),
            inverse: Style::fg(Some(Color::Palette(3))),
        }
    }

    #[test]
    fn shows_the_badge_the_name_the_position_and_the_keys() {
        let spans = spans("notes.md", None, at(42), None, 60, &styles());
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
    fn the_badge_stays_at_the_end() {
        let spans = spans("notes.md", None, at(100), None, 60, &styles());
        assert!(text(&spans).starts_with(" kiri  notes.md "), "{spans:?}");
    }

    #[test]
    fn the_keys_go_first_when_the_window_is_narrow() {
        let spans = spans("notes.md", None, at(42), None, 30, &styles());
        assert_eq!(text(&spans), " kiri  notes.md           42% ");
    }

    #[test]
    fn a_long_name_loses_its_start() {
        let spans = spans(
            "docs/a/long/way/to/roadmap.md",
            None,
            at(7),
            None,
            26,
            &styles(),
        );
        assert_eq!(text(&spans), " kiri  …/to/roadmap.md 7% ");
        assert_eq!(width(&spans), 26);
    }

    #[test]
    fn several_files_tell_the_file_s_place_after_its_name() {
        let spans = spans("b.md", Some((2, 3)), at(42), None, 60, &styles());
        assert_eq!(
            text(&spans),
            " kiri  b.md 2/3                       42%   ? help   q quit "
        );
        assert_eq!(width(&spans), 60);
    }

    #[test]
    fn a_narrow_window_shortens_the_name_before_its_place() {
        let shown = |width| {
            let spans = spans(
                "docs/notes.md",
                Some((2, 3)),
                at(42),
                None,
                width,
                &styles(),
            );
            assert_eq!(wrap::width(&spans), width, "{spans:?}");
            text(&spans)
        };
        assert_eq!(shown(30), " kiri  docs/notes.md 2/3  42% ");
        assert_eq!(shown(22), " kiri  …es.md 2/3 42% ");
        assert_eq!(shown(17), " kiri  … 2/3 42% ");
        assert_eq!(shown(15), " kiri  …/3 42% ");
        assert_eq!(shown(13), " kiri    42% ");
    }

    #[test]
    fn the_line_fills_the_window_and_no_more() {
        for width in 0..80 {
            let spans = spans("docs/notes.md", None, at(42), None, width, &styles());
            let expected = if width < 11 { width.min(6) } else { width };
            assert_eq!(wrap::width(&spans), expected, "{width}: {spans:?}");
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
    fn control_characters_in_the_name_take_a_cell_each() {
        let spans = spans("a\nb\x07.md", None, at(42), None, 40, &styles());
        assert_eq!(width(&spans), 40, "{spans:?}");
        assert!(
            text(&spans).contains(" a\u{fffd}b\u{fffd}.md "),
            "{spans:?}"
        );
    }

    #[test]
    fn wide_characters_in_the_name_take_two_cells() {
        let spans = spans("文書/説明.md", None, at(42), None, 18, &styles());
        assert_eq!(width(&spans), 18, "{spans:?}");
        assert_eq!(text(&spans), " kiri  …明.md 42% ");
    }

    const AT: Option<Found> = Some(Found::At {
        term: "kiri",
        current: 3,
        total: 12,
        flash: None,
    });

    #[test]
    fn shows_the_search_between_the_name_and_the_position() {
        let spans = spans("notes.md", None, at(42), AT, 60, &styles());
        assert_eq!(
            text(&spans),
            " kiri  notes.md          /kiri 3/12   42%   ? help   q quit "
        );
        assert_eq!(width(&spans), 60);
        let nowhere = Some(Found::Nowhere("xyz"));
        let spans = super::spans("notes.md", None, at(42), nowhere, 60, &styles());
        assert_eq!(
            text(&spans),
            " kiri  notes.md      not found: xyz   42%   ? help   q quit "
        );
        assert_eq!(width(&spans), 60);
    }

    #[test]
    fn a_message_too_long_for_the_window_loses_its_end() {
        let message = "No such file or directory (os error 2)";
        let told = Some(Found::Told {
            message,
            fading: None,
        });
        let spans = spans("notes.md", None, at(42), told, 40, &styles());
        assert_eq!(text(&spans), " kiri  No such file or directory…   42% ");
        assert_eq!(width(&spans), 40);
    }

    #[test]
    fn a_flash_shows_the_current_match_number_apart() {
        let flash = Some(Found::At {
            term: "kiri",
            current: 1,
            total: 12,
            flash: Some(styles().inverse),
        });
        let spans = spans("notes.md", None, at(42), flash, 60, &styles());
        assert_eq!(
            text(&spans),
            text(&super::spans("notes.md", None, at(42), AT, 60, &styles()))
                .replace("3/12", "1/12")
        );
        let one = spans.iter().find(|span| span.text == "1").unwrap();
        assert_eq!(one.style, styles().inverse);
        let twelve = spans.iter().find(|span| span.text == "/12").unwrap();
        assert_eq!(twelve.style, styles().bar);
    }

    #[test]
    fn the_match_number_fades_into_the_bar_s_reverse() {
        let mut caps = Capabilities::kitty(80);
        caps.background = Some(Color::Rgb(0, 0, 0));
        caps.palette[8] = Some(Color::Rgb(100, 100, 100));
        let styles = Styles::new(&Theme::default(), &caps);
        assert_eq!(
            styles.inverse,
            Style {
                bold: true,
                reverse: true,
                ..styles.bar.clone()
            }
        );
        // Without colours, bold against the bar's reverse video.
        caps.color = false;
        let styles = Styles::new(&Theme::default(), &caps);
        assert!(styles.inverse.bold && !styles.inverse.reverse);
        assert_eq!((styles.inverse.fg, styles.inverse.bg), (None, None));
    }

    #[test]
    fn a_message_fades_out_into_the_bar_s_background() {
        let mut caps = Capabilities::kitty(80);
        caps.foreground = Some(Color::Rgb(200, 200, 200));
        caps.background = Some(Color::Rgb(0, 0, 0));
        caps.palette[8] = Some(Color::Rgb(100, 100, 100));
        let theme = Theme::default();
        let styles = Styles::new(&theme, &caps);
        let rgb = super::super::fade::Rgb::new(&theme, &caps);
        let at = |millis| {
            styles.fading(Fade {
                elapsed: std::time::Duration::from_millis(millis),
                rgb: rgb.as_ref(),
            })
        };
        let colors = |style: Option<Style>| style.map(|style| (style.fg, style.bg));
        let grey = |level| Some(Color::Rgb(level, level, level));
        assert_eq!(colors(at(0)), Some((grey(200), grey(50))));
        assert_eq!(colors(at(1500)), Some((grey(125), grey(50))));
        assert_eq!(colors(at(3000)), Some((grey(50), grey(50))));
        // Without colours known as RGB, it does not fade.
        let rgb = None;
        let fade = Fade {
            elapsed: std::time::Duration::from_millis(1500),
            rgb,
        };
        assert_eq!(styles.fading(fade), None);
    }

    #[test]
    fn a_narrow_window_leaves_out_the_keys_then_the_name_then_the_search() {
        let shown = |width| text(&spans("notes.md", None, at(42), AT, width, &styles()));
        assert_eq!(shown(40), " kiri  notes.md        /kiri 3/12   42% ");
        assert_eq!(shown(27), " kiri  …d /kiri 3/12   42% ");
        assert_eq!(shown(24), " kiri  /kiri 3/12   42% ");
        assert_eq!(shown(23), " kiri  notes.md    42% ");
        for width in 0..80 {
            let spans = spans("docs/notes.md", None, at(42), AT, width, &styles());
            let expected = if width < 11 { width.min(6) } else { width };
            assert_eq!(wrap::width(&spans), expected, "{width}: {spans:?}");
        }
    }

    #[test]
    fn the_prompt_replaces_the_name_and_the_keys() {
        let spans = prompt('/', "kiri", 30, &styles());
        assert_eq!(text(&spans), " kiri  /kiri\u{258f}                 ");
        assert_eq!(width(&spans), 30);
        assert_eq!(spans[0].style, styles().badge);
    }

    #[test]
    fn a_colon_waits_for_a_command_on_the_prompt() {
        let spans = prompt(':', "", 20, &styles());
        assert_eq!(text(&spans), " kiri  :\u{258f}           ");
        assert_eq!(width(&spans), 20);
    }

    #[test]
    fn a_long_term_in_the_prompt_loses_its_start() {
        let spans = prompt('/', "a long search term", 20, &styles());
        assert_eq!(text(&spans), " kiri  /…earch term\u{258f}");
        assert_eq!(width(&spans), 20);
        for width in 0..30 {
            let spans = prompt('/', "kiri", width, &styles());
            let expected = if width < 9 { width.min(6) } else { width };
            assert_eq!(wrap::width(&spans), expected, "{width}: {spans:?}");
        }
    }

    #[test]
    fn a_window_too_narrow_for_more_shows_the_badge() {
        assert_eq!(
            text(&spans("notes.md", None, at(42), None, 4, &styles())),
            " kir"
        );
        assert_eq!(
            text(&spans("notes.md", None, at(42), None, 0, &styles())),
            ""
        );
    }
}
