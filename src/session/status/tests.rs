use crate::session::status::*;
use std::time::Duration;

use crate::typeset::images::marks::State;
use crate::typeset::style::text;
use crate::typeset::theme::{Paper, StatusColor, StatusLineTheme};
use crate::typeset::wrap::width;

fn at(percent: u8) -> Position {
    Position { percent }
}

fn styles() -> Styles {
    Styles {
        bar: Style::fg(Some(Color::Palette(1))),
        badge: Style::fg(Some(Color::Palette(2))),
        inverse: Style::fg(Some(Color::Palette(3))),
        link: Style::fg(Some(Color::Palette(4))),
    }
}

#[test]
fn shows_the_badge_the_name_the_position_and_the_keys() {
    let spans = spans("notes.md", None, at(42), Follow::Off, None, 60, &styles());
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
    let spans = spans("notes.md", None, at(100), Follow::Off, None, 60, &styles());
    assert!(text(&spans).starts_with(" kiri  notes.md "), "{spans:?}");
}

#[test]
fn the_keys_go_first_when_the_window_is_narrow() {
    let spans = spans("notes.md", None, at(42), Follow::Off, None, 30, &styles());
    assert_eq!(text(&spans), " kiri  notes.md           42% ");
}

#[test]
fn following_is_told_after_the_position_with_its_key() {
    let spans = spans("notes.md", None, at(42), Follow::On, None, 60, &styles());
    assert_eq!(
        text(&spans),
        " kiri  notes.md            42%   F follow   ? help   q quit "
    );
    assert_eq!(width(&spans), 60);
    // The key in bold, as the keys for help and quitting are.
    let style = |text: &str| {
        let span = spans.iter().find(|span| span.text == text);
        span.map(|span| span.style.clone())
    };
    assert_eq!(style("F"), style("?"));
    assert!(style("F").is_some_and(|style| style.bold), "{spans:?}");
}

/// `Tags` that follow as `follow` says, with a mark for `count`
/// warnings.
fn warned(follow: Follow, count: usize) -> Tags {
    Tags {
        follow,
        warnings: Some(Warned { count, flash: None }),
        ..Tags::default()
    }
}

#[test]
fn the_warning_mark_is_told_before_the_position() {
    let spans = spans(
        "notes.md",
        None,
        at(42),
        warned(Follow::Auto, 1),
        None,
        60,
        &styles(),
    );
    assert_eq!(
        text(&spans),
        " kiri  notes.md        \u{26a0} 1   42%   F auto   ? help   q quit "
    );
    assert_eq!(width(&spans), 60);
    assert!(
        spans[1..]
            .iter()
            .all(|span| span.style.fg == styles().bar.fg)
    );
}

#[test]
fn the_warning_mark_comes_after_a_search() {
    let spans = spans(
        "notes.md",
        None,
        at(42),
        warned(Follow::Off, 3),
        AT,
        60,
        &styles(),
    );
    assert_eq!(
        text(&spans),
        " kiri  notes.md    /kiri 3/12   \u{26a0} 3   42%   ? help   q quit "
    );
}

#[test]
fn the_number_of_the_warning_mark_shows_in_the_flash_style() {
    let flash = Style::fg(Some(Color::Palette(5)));
    let tags = Tags {
        warnings: Some(Warned {
            count: 12,
            flash: Some(flash.clone()),
        }),
        ..Tags::default()
    };
    let spans = spans("notes.md", None, at(42), tags, None, 60, &styles());
    let flashed: Vec<_> = spans.iter().filter(|span| span.style == flash).collect();
    assert_eq!(flashed.len(), 1, "{spans:?}");
    assert_eq!(flashed[0].text, "12");
}

#[test]
fn a_narrow_window_leaves_out_the_warning_mark_after_the_following_word() {
    let shown = |width, follow| {
        let spans = spans(
            "notes.md",
            None,
            at(42),
            warned(follow, 1),
            None,
            width,
            &styles(),
        );
        assert_eq!(wrap::width(&spans), width, "{spans:?}");
        text(&spans)
    };
    assert_eq!(
        shown(30, Follow::On),
        " kiri    \u{26a0} 1   42%   F follow "
    );
    assert_eq!(shown(21, Follow::On), " kiri  \u{2026}md \u{26a0} 1   42% ");
    assert_eq!(shown(14, Follow::On), " kiri  \u{2026}d 42% ");
    for width in 0..80 {
        let spans = spans(
            "docs/notes.md",
            None,
            at(42),
            warned(Follow::On, 1),
            AT,
            width,
            &styles(),
        );
        let expected = if width < 11 { width.min(6) } else { width };
        assert_eq!(wrap::width(&spans), expected, "{width}: {spans:?}");
    }
}

#[test]
fn auto_following_is_told_as_auto() {
    let shown = |width| {
        spans(
            "notes.md",
            None,
            at(42),
            Follow::Auto,
            None,
            width,
            &styles(),
        )
    };
    assert_eq!(
        text(&shown(60)),
        " kiri  notes.md              42%   F auto   ? help   q quit "
    );
    assert_eq!(width(&shown(60)), 60);
    assert_eq!(text(&shown(20)), " kiri  42%   F auto ");
}

#[test]
fn a_narrow_window_leaves_out_following_after_the_name_but_before_the_search() {
    let shown = |width, found| {
        text(&spans(
            "notes.md",
            None,
            at(42),
            Follow::On,
            found,
            width,
            &styles(),
        ))
    };
    assert_eq!(shown(30, None), " kiri  …tes.md 42%   F follow ");
    assert_eq!(shown(22, None), " kiri  42%   F follow ");
    assert_eq!(shown(21, None), " kiri  notes.md  42% ");
    assert_eq!(shown(35, AT), " kiri  /kiri 3/12   42%   F follow ");
    assert_eq!(shown(34, AT), " kiri  notes.md  /kiri 3/12   42% ");
    for width in 0..80 {
        let spans = spans(
            "docs/notes.md",
            None,
            at(42),
            Follow::On,
            AT,
            width,
            &styles(),
        );
        let expected = if width < 11 { width.min(6) } else { width };
        assert_eq!(wrap::width(&spans), expected, "{width}: {spans:?}");
    }
}

#[test]
fn a_long_name_loses_its_start() {
    let spans = spans(
        "docs/a/long/way/to/roadmap.md",
        None,
        at(7),
        Follow::Off,
        None,
        26,
        &styles(),
    );
    assert_eq!(text(&spans), " kiri  …/to/roadmap.md 7% ");
    assert_eq!(width(&spans), 26);
}

#[test]
fn several_files_tell_the_file_s_place_after_its_name() {
    let spans = spans(
        "b.md",
        Some((2, 3)),
        at(42),
        Follow::Off,
        None,
        60,
        &styles(),
    );
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
            Follow::Off,
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
        let spans = spans(
            "docs/notes.md",
            None,
            at(42),
            Follow::Off,
            None,
            width,
            &styles(),
        );
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
fn the_bar_shows_where_the_palette_s_grey_is_the_page() {
    // Ghostty's Bright Lights: bright black is the background.
    let page = Color::Rgb(0x19, 0x19, 0x19);
    let mut caps = Capabilities::kitty(80);
    caps.background = Some(page);
    caps.palette[8] = Some(page);
    let bar = Styles::new(&Theme::default(), &caps).bar.bg.expect("a bar");
    assert_ne!(bar, page);
    // A theme of the user's own keeps the palette's grey.
    let theme = Theme {
        built_in: false,
        ..Theme::default()
    };
    assert_eq!(Styles::new(&theme, &caps).bar.bg, Some(page));
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
    let spans = spans(
        "a\nb\x07.md",
        None,
        at(42),
        Follow::Off,
        None,
        40,
        &styles(),
    );
    assert_eq!(width(&spans), 40, "{spans:?}");
    assert!(
        text(&spans).contains(" a\u{fffd}b\u{fffd}.md "),
        "{spans:?}"
    );
}

#[test]
fn wide_characters_in_the_name_take_two_cells() {
    let spans = spans(
        "文書/説明.md",
        None,
        at(42),
        Follow::Off,
        None,
        18,
        &styles(),
    );
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
    let spans = spans("notes.md", None, at(42), Follow::Off, AT, 60, &styles());
    assert_eq!(
        text(&spans),
        " kiri  notes.md          /kiri 3/12   42%   ? help   q quit "
    );
    assert_eq!(width(&spans), 60);
    let nowhere = Some(Found::Nowhere("xyz"));
    let spans = super::spans(
        "notes.md",
        None,
        at(42),
        Follow::Off,
        nowhere,
        60,
        &styles(),
    );
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
    let spans = spans("notes.md", None, at(42), Follow::Off, told, 40, &styles());
    assert_eq!(text(&spans), " kiri  No such file or directory…   42% ");
    assert_eq!(width(&spans), 40);
}

#[test]
fn a_selected_link_is_told_after_the_name_and_loses_its_middle_when_it_is_long() {
    let url = "https://example.org/a/very/long/path/to/the/page-with-a-long-name.html";
    let tags = Tags {
        selected: Some(link(url)),
        ..Tags::default()
    };
    let spans = spans("notes.md", None, at(42), tags, None, 100, &styles());
    let line = text(&spans);
    // Its start and its end stay: the host and the page.
    assert!(line.contains("https://example.org/"), "{line:?}");
    assert!(line.contains("name.html"), "{line:?}");
    assert!(line.contains('…'), "{line:?}");
    assert!(line.find("notes.md").unwrap() < line.find("https").unwrap());
    // One that fits is whole.
    let tags = Tags {
        selected: Some(link("https://example.org/")),
        ..Tags::default()
    };
    let line = text(&spans_for(tags, 100));
    assert!(line.contains("notes.md https://example.org/"), "{line:?}");
    assert_eq!(width(&spans_for(Tags::default(), 100)), 100);
}

#[test]
fn a_flash_shows_the_current_match_number_apart() {
    let flash = Some(Found::At {
        term: "kiri",
        current: 1,
        total: 12,
        flash: Some(styles().inverse),
    });
    let spans = spans("notes.md", None, at(42), Follow::Off, flash, 60, &styles());
    assert_eq!(
        text(&spans),
        text(&super::spans(
            "notes.md",
            None,
            at(42),
            Follow::Off,
            AT,
            60,
            &styles()
        ))
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
    let shown = |width| {
        text(&spans(
            "notes.md",
            None,
            at(42),
            Follow::Off,
            AT,
            width,
            &styles(),
        ))
    };
    assert_eq!(shown(40), " kiri  notes.md        /kiri 3/12   42% ");
    assert_eq!(shown(27), " kiri  …d /kiri 3/12   42% ");
    assert_eq!(shown(24), " kiri  /kiri 3/12   42% ");
    assert_eq!(shown(23), " kiri  notes.md    42% ");
    for width in 0..80 {
        let spans = spans(
            "docs/notes.md",
            None,
            at(42),
            Follow::Off,
            AT,
            width,
            &styles(),
        );
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
        text(&spans(
            "notes.md",
            None,
            at(42),
            Follow::Off,
            None,
            4,
            &styles()
        )),
        " kir"
    );
    assert_eq!(
        text(&spans(
            "notes.md",
            None,
            at(42),
            Follow::Off,
            None,
            0,
            &styles()
        )),
        ""
    );
}

/// `Tags` for a document that follows as `follow` says, in reading mode
/// when `reading` says so.
fn reading(follow: Follow, reading: bool) -> Tags {
    Tags {
        follow,
        reading,
        ..Tags::default()
    }
}

#[test]
fn reading_mode_is_told_after_the_position_with_its_key() {
    let shown = |tags| text(&spans("notes.md", None, at(42), tags, None, 60, &styles()));
    assert_eq!(
        shown(reading(Follow::Off, true)),
        " kiri  notes.md           42%   M reading   ? help   q quit "
    );
    // Nothing when it is off.
    assert_eq!(
        shown(reading(Follow::Off, false)),
        " kiri  notes.md                       42%   ? help   q quit "
    );
    // After the word for following, which keeps its place; the keys for
    // help and quitting have no room left.
    assert_eq!(
        shown(reading(Follow::On, true)),
        " kiri  notes.md                  42%   F follow   M reading "
    );
    // The key in bold, as the others are.
    let spans = spans(
        "notes.md",
        None,
        at(42),
        reading(Follow::Off, true),
        None,
        60,
        &styles(),
    );
    let bold = |text: &str| {
        spans
            .iter()
            .find(|span| span.text == text)
            .map(|s| s.style.bold)
    };
    assert_eq!(bold("M"), Some(true), "{spans:?}");
}

#[test]
fn a_narrow_window_leaves_out_the_reading_word_with_the_following_word() {
    let shown = |width, tags| {
        let spans = spans("notes.md", None, at(42), tags, None, width, &styles());
        assert_eq!(wrap::width(&spans), width, "{spans:?}");
        text(&spans)
    };
    // Room for the word for following and not for both: both go, as the
    // warning mark and the search go before the keys.
    assert_eq!(
        shown(33, reading(Follow::On, true)),
        " kiri  notes.md              42% "
    );
    assert_eq!(
        shown(34, reading(Follow::On, true)),
        " kiri  42%   F follow   M reading "
    );
    assert_eq!(
        shown(36, reading(Follow::Off, true)),
        " kiri  notes.md     42%   M reading "
    );
    for width in 0..80 {
        let spans = spans(
            "docs/notes.md",
            None,
            at(42),
            reading(Follow::On, true),
            AT,
            width,
            &styles(),
        );
        let expected = if width < 11 { width.min(6) } else { width };
        assert_eq!(wrap::width(&spans), expected, "{width}: {spans:?}");
    }
}

/// A pill for the picture `horse`.
fn pill(state: State, elapsed: u64, total: u64) -> Pill {
    Pill {
        name: "horse".to_owned(),
        state,
        elapsed: Duration::from_secs(elapsed),
        total: Duration::from_secs(total),
    }
}

fn with_pill(pill: Pill) -> Tags {
    Tags {
        pill: Some(pill),
        ..Tags::default()
    }
}

#[test]
fn the_pill_sits_after_the_position_and_tells_the_picture_a_bar_and_the_times() {
    let tags = with_pill(pill(State::Playing, 3, 8));
    let spans = spans("notes.md", None, at(42), tags, None, 100, &styles());
    let line = text(&spans);
    assert!(line.starts_with(" kiri  notes.md "), "{line:?}");
    let at_pill = line.find("▶ horse").expect(&line);
    assert!(at_pill > line.find("notes.md").unwrap(), "{line:?}");
    assert!(at_pill > line.find("42%").unwrap(), "{line:?}");
    // 3 of 8 seconds: 3 of the 8 cells of the bar.
    assert!(line.contains("━━━─────"), "{line:?}");
    assert!(line.contains("3.0 / 8.0 s"), "{line:?}");
    assert_eq!(width(&spans), 100);
}

#[test]
fn the_mark_says_playing_stopped_or_ended_and_the_pill_keeps_its_width() {
    let line = |state, elapsed| {
        let spans = spans(
            "notes.md",
            None,
            at(42),
            with_pill(pill(state, elapsed, 8)),
            None,
            100,
            &styles(),
        );
        text(&spans)
    };
    let (playing, stopped, ended) = (
        line(State::Playing, 3),
        line(State::Stopped, 0),
        line(State::Ended, 8),
    );
    assert!(playing.contains("▶ horse"), "{playing:?}");
    assert!(stopped.contains("■ horse") && stopped.contains("0.0 / 8.0 s"));
    assert!(ended.contains("■ horse") && ended.contains("8.0 / 8.0 s"));
    assert!(ended.contains("━━━━━━━━"), "{ended:?}");
    // The position stays where it is.
    let at = |line: &str| line.find("42%");
    assert_eq!(at(&playing), at(&stopped));
    assert_eq!(at(&stopped), at(&ended));
}

#[test]
fn the_pill_has_the_badge_s_colours_and_a_long_name_loses_its_end() {
    let mut long = pill(State::Playing, 1, 8);
    long.name = "a picture with a long name".to_owned();
    let spans = spans(
        "notes.md",
        None,
        at(42),
        with_pill(long),
        None,
        100,
        &styles(),
    );
    let line = text(&spans);
    assert!(line.contains("▶ a picture wit… "), "{line:?}");
    let in_pill = spans
        .iter()
        .find(|span| span.text.contains("a picture wit"))
        .expect("a span of the name");
    assert_eq!(in_pill.style.fg, styles().badge.fg);
    assert_eq!(in_pill.style.bg, styles().badge.bg);
}

#[test]
fn the_selected_picture_is_told_after_the_name_with_no_symbol_before_it() {
    let tags = Tags {
        selected: Some(chosen("other")),
        ..with_pill(pill(State::Playing, 3, 8))
    };
    let spans = spans("notes.md", None, at(42), tags, None, 100, &styles());
    let line = text(&spans);
    let (name, selected) = (
        line.find("notes.md").unwrap(),
        line.find("other").expect(&line),
    );
    assert!(
        name < selected && selected < line.find("▶ horse").unwrap(),
        "{line:?}"
    );
    assert!(!line.contains('◂'), "{line:?}");
    // With no pill, it is told too.
    let tags = Tags {
        selected: Some(chosen("other")),
        ..Tags::default()
    };
    let line = text(&spans_for(tags, 100));
    assert!(line.contains("other"), "{line:?}");
}

fn spans_for(tags: Tags, width: usize) -> Vec<Span> {
    spans("notes.md", None, at(42), tags, None, width, &styles())
}

/// What of a line made with a pill shows, for the order of dropping:
/// the file's name, the times, the whole bar, the position, the selected
/// picture or link, the pill. The key hints are not on such a line.
fn shown(line: &str, selected: &str) -> [bool; 6] {
    let bar = line.chars().filter(|c| matches!(c, '━' | '─')).count();
    [
        line.contains(".md"),
        line.contains("/ 8.0 s"),
        bar == 8,
        line.contains("42%"),
        line.contains(selected),
        line.contains("horse"),
    ]
}

const ORDER: [&str; 6] = [
    "the file's name",
    "the times",
    "the whole bar",
    "the position",
    "the selected one",
    "the pill",
];

fn search_for_needle() -> Option<Found<'static>> {
    Some(Found::At {
        term: "needle",
        current: 3,
        total: 12,
        flash: None,
    })
}

#[test]
fn a_narrow_window_drops_the_name_then_the_times_then_half_the_bar_then_the_rest() {
    let animation = || Tags {
        selected: Some(chosen("other")),
        ..with_pill(pill(State::Playing, 3, 8))
    };
    let link = || Tags {
        selected: Some(link("https://example.org/page")),
        ..with_pill(pill(State::Playing, 3, 8))
    };
    let cases: [(&str, Tags, &str, bool); 4] = [
        ("an animation", animation(), "other", false),
        ("an animation and a search", animation(), "other", true),
        ("a link", link(), "https://", false),
        ("a link and a search", link(), "https://", true),
    ];
    for (what, tags, selected, searching) in cases {
        for window in (30..=140).rev() {
            let found = searching.then(search_for_needle).flatten();
            let spans = spans(
                "notes.md",
                None,
                at(42),
                tags.clone(),
                found,
                window,
                &styles(),
            );
            assert_eq!(width(&spans), window, "{what}: {:?}", text(&spans));
            let line = text(&spans);
            let present = shown(&line, selected);
            assert!(
                !line.contains("quit") && !line.contains("help"),
                "{what} at {window}: {line:?}"
            );
            // The pill gives way to the search, last of all: without it the
            // line is as it is without one. While it is there, what goes first
            // is gone when what goes later is: a piece that is there needs all
            // those that go after it.
            let last = if present[ORDER.len() - 1] {
                ORDER.len()
            } else {
                0
            };
            for early in 0..last {
                for late in early + 1..last {
                    assert!(
                        !present[early] || present[late],
                        "{what} at {window}: {} is there without {}: {line:?}",
                        ORDER[early],
                        ORDER[late]
                    );
                }
            }
            // The search is never dropped for them.
            if searching && window >= 40 {
                assert!(
                    line.contains("/needle 3/12"),
                    "{what} at {window}: {line:?}"
                );
            }
        }
    }
}

#[test]
fn at_95_cells_the_times_show_with_a_selected_animation_and_no_search() {
    let tags = Tags {
        selected: Some(chosen("other")),
        ..with_pill(pill(State::Playing, 3, 8))
    };
    let line = text(&spans_for(tags, 95));
    assert!(line.contains("/ 8.0 s"), "{line:?}");
    assert!(line.contains(".md"), "{line:?}");
    assert!(!line.contains("quit") && !line.contains("help"), "{line:?}");
}

#[test]
fn at_95_cells_with_a_search_too_the_name_goes_before_the_times() {
    let tags = |selected: Selected| Tags {
        selected: Some(selected),
        ..with_pill(pill(State::Playing, 3, 8))
    };
    let line = |tags| {
        let found = search_for_needle();
        text(&spans("notes.md", None, at(42), tags, found, 95, &styles()))
    };
    // An animation: the times stay, and the search.
    let animation = line(tags(chosen("other")));
    assert!(
        animation.contains("/ 8.0 s") && animation.contains("/needle 3/12"),
        "{animation:?}"
    );
    // A link, which is long: the name is gone and the times may be too, but
    // never the search, and the link stays as long as they do.
    let linked = line(tags(link("https://example.org/page")));
    assert!(linked.contains("/needle 3/12"), "{linked:?}");
    assert!(linked.contains("https://"), "{linked:?}");
    assert!(!linked.contains("quit") && !animation.contains("quit"));
}

#[test]
fn the_key_hints_are_not_shown_while_the_pill_is_on_the_line() {
    let hints = |line: &str| line.contains("help") && line.contains("quit");
    // No pill: as before, in a wide window.
    let plain = text(&spans_for(Tags::default(), 100));
    assert!(hints(&plain), "{plain:?}");
    let selected = Tags {
        selected: Some(chosen("other")),
        ..Tags::default()
    };
    assert!(hints(&text(&spans_for(selected, 120))));
    // A pill, in any window: no hints; the mode letter, the warnings and the
    // position stay.
    let tags = Tags {
        reading: true,
        warnings: Some(Warned {
            count: 2,
            flash: None,
        }),
        ..with_pill(pill(State::Playing, 3, 8))
    };
    for window in [60, 80, 95, 100, 140, 200] {
        let line = text(&spans_for(tags.clone(), window));
        assert!(
            !line.contains("help") && !line.contains("quit"),
            "{window}: {line:?}"
        );
    }
    let line = text(&spans_for(tags, 140));
    assert!(
        line.contains("\u{26a0} 2") && line.contains("42%"),
        "{line:?}"
    );
    assert!(line.contains("reading"), "{line:?}");
    // The room goes to the name: a long one is cut at its start only.
    let mut name = "a-long-name-".repeat(3);
    name.push_str(".md");
    let line = text(&spans(
        &name,
        None,
        at(42),
        with_pill(pill(State::Ended, 8, 8)),
        None,
        80,
        &styles(),
    ));
    assert!(line.contains("name-.md"), "{line:?}");
}

#[test]
fn the_search_stays_when_the_pill_and_a_selected_link_take_room() {
    let found = || {
        Some(Found::At {
            term: "needle",
            current: 3,
            total: 12,
            flash: None,
        })
    };
    let tags = Tags {
        selected: Some(link("https://example.org/a/path")),
        ..with_pill(pill(State::Playing, 3, 8))
    };
    for window in 20..=140 {
        let alone = spans(
            "notes.md",
            None,
            at(42),
            Tags::default(),
            found(),
            window,
            &styles(),
        );
        if !text(&alone).contains("/needle 3/12") {
            continue;
        }
        let with = spans(
            "notes.md",
            None,
            at(42),
            tags.clone(),
            found(),
            window,
            &styles(),
        );
        assert!(
            text(&with).contains("/needle 3/12"),
            "{window}: {:?}",
            text(&with)
        );
        assert_eq!(width(&with), window);
    }
    // And the pill is what gives way to it: smaller, or gone.
    let alone = spans(
        "notes.md",
        None,
        at(42),
        Tags::default(),
        found(),
        40,
        &styles(),
    );
    assert!(text(&alone).contains("/needle 3/12"));
    let with = text(&spans(
        "notes.md",
        None,
        at(42),
        tags,
        found(),
        40,
        &styles(),
    ));
    assert!(!with.contains("/ 8.0 s"), "{with:?}");
}

#[test]
fn the_pill_is_last_on_the_line_at_the_edge_and_before_the_tags_when_not() {
    let tags = with_pill(pill(State::Playing, 3, 8));
    let lay = |edge| {
        let spans = laid(
            "notes.md",
            None,
            at(42),
            tags.clone(),
            None,
            100,
            &styles(),
            edge,
        );
        assert_eq!(width(&spans), 100);
        text(&spans)
    };
    let (edged, apart) = (lay(true), lay(false));
    assert!(
        edged.find("42%").unwrap() < edged.find("▶ horse").unwrap(),
        "{edged:?}"
    );
    assert!(edged.trim_end().ends_with("3.0 / 8.0 s"), "{edged:?}");
    assert!(
        apart.find("▶ horse").unwrap() < apart.find("42%").unwrap(),
        "{apart:?}"
    );
    // The same pieces either way.
    let sorted = |line: &str| {
        let mut words: Vec<_> = line.split_whitespace().map(str::to_owned).collect();
        words.sort();
        words
    };
    assert_eq!(sorted(&edged), sorted(&apart));
}

#[test]
fn the_times_are_tenths_of_a_second_under_a_minute_and_minutes_from_a_minute_on() {
    let ms = |elapsed: u64, total: u64| Pill {
        name: "x".to_owned(),
        state: State::Playing,
        elapsed: Duration::from_millis(elapsed),
        total: Duration::from_millis(total),
    };
    assert_eq!(times(&ms(3200, 8000)), "3.2 / 8.0 s");
    assert_eq!(times(&ms(1500, 1500)), "1.5 / 1.5 s");
    assert_eq!(times(&ms(0, 59_900)), " 0.0 / 59.9 s");
    assert_eq!(times(&ms(5, 8000)), "0.0 / 8.0 s");
    assert_eq!(times(&ms(63_000, 130_000)), "1:03 / 2:10");
    assert_eq!(times(&ms(5000, 60_000)), "0:05 / 1:00");
    assert_eq!(times(&ms(70_000, 3_700_000)), "01:10 / 61:40");
    // The form follows the total, and the width does not change as it plays.
    for total in [8000, 12_500, 59_900, 60_000, 130_000, 700_000] {
        let widths: std::collections::HashSet<_> = (0..=total)
            .step_by(700)
            .map(|elapsed| times(&ms(elapsed, total)).width())
            .collect();
        assert_eq!(widths.len(), 1, "{total}: {widths:?}");
    }
}

/// A selected animation called `name`, in a colour of its own.
fn chosen(name: &str) -> Selected {
    Selected {
        name: name.to_owned(),
        style: Style::fg(Some(Color::Palette(5))),
        url: false,
    }
}

/// A selected link to `url`, in the colour of links.
fn link(url: &str) -> Selected {
    Selected {
        name: url.to_owned(),
        style: styles().link,
        url: true,
    }
}

#[test]
fn the_selected_picture_is_told_in_the_colour_of_its_frame_and_a_link_in_the_links() {
    let tags = Tags {
        selected: Some(chosen("other")),
        ..Tags::default()
    };
    let spans = spans("notes.md", None, at(42), tags, None, 100, &styles());
    let told = spans
        .iter()
        .find(|span| span.text.contains("other"))
        .unwrap();
    assert_eq!(told.style.fg, Some(Color::Palette(5)));
    // A link that is selected: its address in the theme's link colour.
    let tags = Tags {
        selected: Some(link("https://example.com/x")),
        ..Tags::default()
    };
    let spans = super::spans("notes.md", None, at(42), tags, None, 100, &styles());
    let told = spans
        .iter()
        .find(|span| span.text.contains("example.com"))
        .unwrap();
    assert_eq!(told.style.fg, styles().link.fg);
    assert_ne!(styles().link.fg, styles().bar.fg);
}

#[test]
fn the_link_style_is_the_themes_link_colour_or_blue() {
    let caps = Capabilities::kitty(80);
    let with = |link: Option<Color>| {
        let mut theme = Theme {
            link,
            ..Theme::default()
        };
        theme.status_line.background = StatusColor::Color(Some(Color::Rgb(40, 40, 40)));
        Styles::new(&theme, &caps)
    };
    assert_eq!(with(Theme::default().link).link.fg, Theme::default().link);
    assert_eq!(
        with(Some(Color::Rgb(1, 2, 3))).link.fg,
        Some(Color::Rgb(1, 2, 3))
    );
    // Without colours, or where the bar is reverse video, the bar's own.
    for caps in [Capabilities::plain(80), Capabilities::kitty(80)] {
        let plain = Styles::new(&Theme::default(), &caps);
        assert_eq!(plain.link, plain.bar);
    }
}
