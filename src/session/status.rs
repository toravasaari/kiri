//! The status line at the bottom of the pager: a badge that tells the
//! pager is on, the document's name, how far down it the view is, and the
//! keys for help and quitting.

use std::time::Duration;

use unicode_width::UnicodeWidthStr;

use crate::session::commands::{Action, hint};
use crate::session::fade::{FADE_OUT, FLASH, Fade};
use crate::session::settings::Follow;
use crate::session::view::Position;
use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::images::marks::State;
use crate::typeset::style::{self, Span, Style};
use crate::typeset::theme::{StatusColor, Theme, VISIBLE};
use crate::typeset::wrap;

/// The styles of the status line, and the one the match number fades
/// into after a search went around an end of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Styles {
    pub bar: Style,
    pub badge: Style,
    pub inverse: Style,
    /// What tells the link that is selected, in the colour of the frame
    /// round it: the theme's link colour.
    pub link: Style,
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
        // In reverse video, a colour of the text would colour the bar.
        let coloured = caps.color && !bar.reverse;
        let link = match (coloured, theme.link.or(Some(Color::Palette(4)))) {
            (true, Some(color)) => Style {
                fg: Some(color),
                ..bar.clone()
            },
            _ => bar.clone(),
        };
        Self {
            bar,
            badge,
            inverse,
            link,
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
/// `None` when either is not known as RGB. The built-in theme lightens a
/// grey the palette makes too close to the page, as it does its lines.
fn mixed(theme: &Theme, caps: &Capabilities) -> Option<Color> {
    let page = theme.background.or(caps.background)?.rgb(&caps.palette)?;
    let grey = Color::Palette(8).rgb(&caps.palette)?;
    let grey = if theme.built_in {
        grey.readable_on(page, VISIBLE)
    } else {
        grey
    };
    page.mix(grey, 0.5)
}

/// What the status line tells besides the document's name and position.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags {
    /// How the pager follows the document.
    pub follow: Follow,
    /// Whether the document is shown in reading mode.
    pub reading: bool,
    /// The warnings about the document shown, when there are any.
    pub warnings: Option<Warned>,
    /// The animation that plays or played last, once one has started.
    pub pill: Option<Pill>,
    /// The animation Tab selected, when it is not the one in the pill.
    pub selected: Option<Selected>,
}

/// The animation that is selected: its name, and the style that tells it,
/// in the colour of the frame round the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub name: String,
    pub style: Style,
}

impl From<Follow> for Tags {
    fn from(follow: Follow) -> Self {
        Self {
            follow,
            ..Self::default()
        }
    }
}

/// The animation that plays or played last, as the status line tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pill {
    /// Its alt text, or its file's name without the extension.
    pub name: String,
    pub state: State,
    /// How far into one round of it, and how long a round takes.
    pub elapsed: Duration,
    pub total: Duration,
}

/// The mark that tells the document has warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warned {
    /// How many there are.
    pub count: usize,
    /// A style that shows the number apart for a moment, after it
    /// changed.
    pub flash: Option<Style>,
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
    /// Where the selected link goes.
    Link(&'a str),
}

/// The status line for the document `name` at `position`: [`line`], with the
/// pill of the animation that plays or played last between the name and the
/// position, and the name of the animation Tab selected after the name, as
/// `tags` hold them. Pieces drop out as the window narrows, in this order:
/// the start of the name, the position and the other words after it, the
/// selected picture, then the pill's times, then half its bar.
pub fn spans(
    name: &str,
    place: Option<(usize, usize)>,
    position: Position,
    tags: impl Into<Tags>,
    found: Option<Found>,
    width: usize,
    styles: &Styles,
) -> Vec<Span> {
    let mut tags = tags.into();
    let (pill, selected) = (tags.pill.take(), tags.selected.take());
    if pill.is_none() && selected.is_none() {
        return line(name, place, position, tags, found, width, styles);
    }
    // The largest form of the pill that the badge and it fit in.
    let form = pill.as_ref().and_then(|_| {
        [Form::Full, Form::NoTime, Form::HalfBar]
            .into_iter()
            .find(|form| BADGE.width() + pill_width_of_opt(pill.as_ref(), *form) <= width)
    });
    let pill_width = form.map_or(0, |form| pill_width_of_opt(pill.as_ref(), form));
    // The selected picture, a gap before it, kept while it fits with the
    // pill. What goes before it, the name and the position and the words
    // after it, goes when it does not: it is kept longer than they are.
    let selected = selected.map(|selected| {
        let shown = format!(" \u{25c2} {}", head(&printable(&selected.name), SELECTED));
        (shown, selected.style)
    });
    // The pill gives up its times only once the selected picture is gone.
    let whole = pill
        .as_ref()
        .map_or(0, |pill| pill_width_of(pill, Form::Full));
    let fits = |(shown, _): &(String, Style)| BADGE.width() + whole + shown.width() <= width;
    let left_out = selected.as_ref().is_some_and(|selected| !fits(selected));
    let selected = selected.filter(fits);
    let selected_width = selected.as_ref().map_or(0, |(shown, _)| shown.width());
    let room = width - pill_width - selected_width;
    let room = if left_out {
        room.min(BADGE.width())
    } else {
        room
    };
    let mut spans = line(name, place, position, tags, found, room, styles);
    let pill = pill
        .zip(form)
        .map(|(pill, form)| pill_spans(&pill, form, styles));
    // Over the name, which is the second span, and the pill after the gap
    // that fills the line; a window with nothing but the badge has no gap.
    let selected = selected.map(|(shown, style)| Span::new(&shown, style));
    match spans.len() {
        0..=1 => {
            spans.extend(selected);
            let used = wrap::width(&spans) + pill_width;
            spans.push(Span::new(
                &" ".repeat(width.saturating_sub(used)),
                styles.bar.clone(),
            ));
            spans.extend(pill.into_iter().flatten());
        }
        _ => {
            let right = spans.split_off(3);
            let filler = spans.pop().expect("a gap that fills the line");
            spans.extend(selected);
            spans.push(filler);
            spans.extend(pill.into_iter().flatten());
            spans.extend(right);
        }
    }
    spans
}

/// How much of the pill shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Full,
    /// Without the times.
    NoTime,
    /// Without the times, and with half the bar.
    HalfBar,
}

/// The cells of the pill's picture name, and of its bar, and the most of
/// the name of a selected picture.
const NAME: usize = 14;
const BAR: usize = 8;
const SELECTED: usize = 16;

/// The cells the pill takes in `form`, with the gap after it. A pill keeps
/// one width whether the animation plays or not.
fn pill_width_of(pill: &Pill, form: Form) -> usize {
    pill_width_of_opt(Some(pill), form)
}

fn pill_width_of_opt(pill: Option<&Pill>, form: Form) -> usize {
    // A space, the mark, a space, the name, a space, the bar, the times
    // after a space, a space, and a gap.
    let times = match form {
        Form::Full => 1 + times(&Pill::of_total(pill)).width(),
        _ => 0,
    };
    1 + 1 + 1 + NAME + 1 + bar_cells(form) + times + 1 + 1
}

fn bar_cells(form: Form) -> usize {
    match form {
        Form::HalfBar => BAR / 2,
        _ => BAR,
    }
}

impl Pill {
    /// A pill with the total of `pill`, for measuring the times.
    fn of_total(pill: Option<&Pill>) -> Pill {
        Pill {
            name: String::new(),
            state: State::Stopped,
            elapsed: Duration::ZERO,
            total: pill.map_or(Duration::ZERO, |pill| pill.total),
        }
    }
}

/// `elapsed / total` as minutes and seconds, `0:03 / 0:08`, the minutes
/// with as many digits as the total's.
fn times(pill: &Pill) -> String {
    let total = pill.total.as_secs();
    let digits = (total / 60).to_string().len();
    let one = |secs: u64| format!("{:0digits$}:{:02}", secs / 60, secs % 60);
    let elapsed = pill.elapsed.as_secs().min(total);
    format!("{} / {}", one(elapsed), one(total))
}

/// The pill in `form`, in the badge's style, and the gap after it.
fn pill_spans(pill: &Pill, form: Form, styles: &Styles) -> Vec<Span> {
    let mark = match pill.state {
        State::Playing => '\u{25b6}',
        State::Stopped | State::Ended | State::Waiting(_) => '\u{25a0}',
    };
    let name = head(&printable(&pill.name), NAME);
    let name = format!("{name}{}", " ".repeat(NAME.saturating_sub(name.width())));
    let cells = bar_cells(form);
    let progress = match pill.total.is_zero() {
        true => 0.0,
        false => (pill.elapsed.as_secs_f64() / pill.total.as_secs_f64()).clamp(0.0, 1.0),
    };
    let filled = (progress * cells as f64).round() as usize;
    let bar = format!(
        "{}{}",
        "\u{2501}".repeat(filled),
        "\u{2500}".repeat(cells - filled)
    );
    let times = match form {
        Form::Full => format!(" {}", times(pill)),
        _ => String::new(),
    };
    let shown = format!(" {mark} {name} {bar}{times} ");
    vec![
        Span::new(&shown, styles.badge.clone()),
        Span::new(" ", styles.bar.clone()),
    ]
}

/// The status line for the document `name` at `position`, with what was
/// `found` by a search, `width` cells wide, and a word after the position
/// when the pager follows the document, as `follow` says. `place` is the
/// file's number, from 1, and how many files there are, which the line
/// tells after the name. `tags` holds that word and the mark for the
/// document's warnings, which comes before the position. A narrow window
/// leaves out the keys first, then the start of the name and then of its
/// place, then the words for following and for reading mode, then the
/// warning mark, then the search.
#[expect(clippy::too_many_lines)]
fn line(
    name: &str,
    place: Option<(usize, usize)>,
    position: Position,
    tags: impl Into<Tags>,
    found: Option<Found>,
    width: usize,
    styles: &Styles,
) -> Vec<Span> {
    let Tags {
        follow,
        reading,
        warnings,
        ..
    } = tags.into();
    let warnings = warnings.as_ref();
    // Measured as shown: a file name may hold any character but `/`.
    let mut name = printable(name);
    if let Some((number, count)) = place {
        name = format!("{name} {number}/{count}");
    }
    let name = name.as_str();
    let percent = format!("{}%", position.percent);
    let keys = [(hint(Action::Help), " help"), (hint(Action::Quit), " quit")];
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
        Some(Found::Link(url)) => {
            told = middle(&printable(url), (width - fixed).saturating_sub(3));
            Some(Found::Link(&told))
        }
        found => found,
    };
    // The word that tells the pager follows the document, and a gap of
    // three before it.
    // Reading mode's word comes after it, and goes with it.
    let words: Vec<(&str, &str)> = follow_word(follow)
        .into_iter()
        .chain(reading.then_some((hint(Action::Reading), " reading")))
        .collect();
    let mark: usize = (words.iter())
        .map(|(key, what)| key.width() + what.width() + 3)
        .sum();
    // The search and a gap of three before the position.
    let search = found.map(|found| search(found, styles)).unwrap_or_default();
    let search_width = if search.is_empty() {
        0
    } else {
        wrap::width(&search) + 3
    };
    let with_search = fixed + search_width <= width;
    let search_width = if with_search { search_width } else { 0 };
    // The warning mark and a gap of three after it: it is kept longer
    // than the word.
    let warned = warnings.map(|warned| format!("\u{26a0} {}", warned.count));
    let warned_width = warned.as_ref().map_or(0, |warned| warned.width() + 3);
    let with_warned = fixed + search_width + warned_width <= width;
    let warned_width = if with_warned { warned_width } else { 0 };
    let with_mark = fixed + search_width + warned_width + mark <= width;
    let fixed = if with_mark { fixed + mark } else { fixed };
    let fixed = fixed + warned_width;
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
    if let Some(warned) = warned.filter(|_| with_warned) {
        let flash = warnings.and_then(|warned| warned.flash.clone());
        right.push(Span::new("\u{26a0} ", styles.bar.clone()));
        let number = warned.trim_start_matches("\u{26a0} ");
        right.push(Span::new(
            number,
            flash.unwrap_or_else(|| styles.bar.clone()),
        ));
        right.push(Span::new("   ", styles.bar.clone()));
    }
    right.push(Span::new(&percent, styles.bar.clone()));
    for (key, what) in words.into_iter().filter(|_| with_mark) {
        right.push(Span::new("   ", styles.bar.clone()));
        right.push(Span::new(key, bold.clone()));
        right.push(Span::new(what, styles.bar.clone()));
    }
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

/// `text` in at most `max` cells, with the middle left out for `…` when it
/// does not fit: the start and the end are what tells a link apart.
fn middle(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_owned();
    }
    if max < 3 {
        return head(text, max);
    }
    // The `…` takes a cell; the start gets the odd one of the rest.
    let kept = max - 1;
    let start = wrap::cut(vec![Span::new(text, Style::default())], kept - kept / 2);
    let end = wrap::tail(text, kept / 2 + 1);
    let end = if end.is_empty() {
        "…".to_owned()
    } else {
        end
    };
    format!("{}{end}", style::text(&start))
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
        Found::Link(url) => vec![Span::new(url, styles.link.clone())],
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

/// What tells that the pager follows the document as `follow` says: the
/// key that changes it, and the word after it; nothing when it does not.
fn follow_word(follow: Follow) -> Option<(&'static str, &'static str)> {
    match follow {
        Follow::Off => None,
        Follow::On => Some((hint(Action::Follow), " follow")),
        Follow::Auto => Some((hint(Action::Follow), " auto")),
    }
}

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
mod tests;
