use super::*;
use crate::typeset::color::Color;
use crate::typeset::size::{Fraction, HorizontalAlign, TextSize, VerticalAlign};
use crate::typeset::theme::{Paper, SearchTheme};

#[test]
fn matches_are_in_reverse_video_and_the_current_one_bold_by_default() {
    let marks = Marks::new(&Theme::default(), &Capabilities::kitty(80));
    let current = Style {
        reverse: true,
        bold: true,
        ..Style::default()
    };
    assert_eq!(
        marks,
        Marks {
            found: Style {
                reverse: true,
                ..Style::default()
            },
            current,
        }
    );
    // Without colours, the paper themes' greys give way too.
    let mut caps = Capabilities::kitty(80);
    caps.color = false;
    assert_eq!(Marks::new(&Theme::paper(Paper::Light), &caps), marks);
}

#[test]
fn colours_from_the_theme_mark_matches_without_reverse_video() {
    let grey = |level| Some(Color::Rgb(level, level, level));
    let caps = Capabilities::kitty(80);
    let light = Marks::new(&Theme::paper(Paper::Light), &caps);
    assert_eq!((light.found.fg, light.found.bg), (grey(0x00), grey(0xcc)));
    assert!(!light.found.reverse && !light.found.bold);
    assert_eq!(
        (light.current.fg, light.current.bg),
        (grey(0xff), grey(0x1f))
    );
    assert!(!light.current.reverse && light.current.bold);
    // One colour given: the other is the text's own.
    let theme = Theme {
        search: SearchTheme {
            current_background: SearchColor::Color(Some(Color::Palette(3))),
            ..SearchTheme::default()
        },
        ..Theme::default()
    };
    let marks = Marks::new(&theme, &caps);
    assert!(marks.found.reverse);
    assert_eq!(
        (marks.current.fg, marks.current.bg),
        (None, Some(Color::Palette(3)))
    );
    assert!(!marks.current.reverse && marks.current.bold);
}

fn line(text: &str) -> Line {
    Line::new(vec![Span::new(text, Style::default())])
}

/// `line` with `matches`, each with whether it is the current one, in
/// `marks`, the current one faded by `fade` of the way in `rgb`'s
/// colours, and the spans `selected` of the selected link.
fn mark(
    line: &Line,
    matches: &[(Range<usize>, bool)],
    marks: &Marks,
    fade: Option<(f32, Option<&Rgb>)>,
    selected: Option<Range<usize>>,
) -> Line {
    let rgb = fade.and_then(|(_, rgb)| rgb);
    let mut runs: Vec<Run> = (matches.iter())
        .map(|(bytes, current)| Run {
            bytes: bytes.clone(),
            ink: Ink::Colours {
                style: if *current {
                    &marks.current
                } else {
                    &marks.found
                },
                fade: fade.map(|(share, _)| share).filter(|_| *current),
            },
        })
        .collect();
    // The spans of the link are the bytes of their text.
    let start =
        |spans: usize| -> usize { line.spans[..spans].iter().map(|span| span.text.len()).sum() };
    if let Some(spans) = selected {
        let (from, to) = (start(spans.start), start(spans.end));
        runs.push(Run {
            bytes: from..to,
            ink: Ink::Reverse,
        });
    }
    super::mark(line, &runs, rgb)
}

fn marks() -> Marks {
    Marks {
        found: Style {
            reverse: true,
            ..Style::default()
        },
        current: Style {
            bold: true,
            bg: Some(Color::Palette(3)),
            ..Style::default()
        },
    }
}

#[test]
fn a_match_on_a_striped_table_row_stays_marked() {
    // The stripe is a background; reverse video swaps it with the
    // text colour, and a match colour takes its place.
    let striped = Style {
        bg: Some(Color::Rgb(16, 16, 16)),
        ..Style::default()
    };
    let found = laid_over(&striped, &marks().found);
    assert!(found.reverse);
    assert_eq!(found.bg, striped.bg);
    let current = laid_over(&striped, &marks().current);
    assert_eq!(current.bg, Some(Color::Palette(3)));
}

#[test]
fn marking_splits_spans_where_matches_start_and_end() {
    let bold = Style {
        bold: true,
        ..Style::default()
    };
    let source = Line::new(vec![
        Span::new("a ki", Style::default()),
        Span::new("ri b", bold.clone()),
    ]);
    let marked = mark(&source, &[(2..6, false)], &marks(), None, None);
    let reverse = |style: &Style| Style {
        reverse: true,
        ..style.clone()
    };
    assert_eq!(
        marked.spans,
        [
            Span::new("a ", Style::default()),
            Span::new("ki", reverse(&Style::default())),
            Span::new("ri", reverse(&bold)),
            Span::new(" b", bold),
        ]
    );
}

#[test]
fn the_selected_link_is_reversed_except_where_a_match_marks_it() {
    let link = Style {
        link: Some("https://example.com".into()),
        ..Style::default()
    };
    let reversed = Style {
        reverse: true,
        ..link.clone()
    };
    let source = Line::new(vec![
        Span::new("a ", Style::default()),
        Span::new("kiri link", link.clone()),
        Span::new(" b", Style::default()),
    ]);
    // Matches in yellow, not in reverse video.
    let marks = Marks {
        found: Style {
            bg: Some(Color::Palette(3)),
            ..Style::default()
        },
        current: Style::default(),
    };
    let marked = mark(&source, &[(2..6, false)], &marks, None, Some(1..2));
    let found = laid_over(&link, &marks.found);
    assert!(!found.reverse);
    assert_eq!(
        marked.spans,
        [
            Span::new("a ", Style::default()),
            Span::new("kiri", found),
            Span::new(" link", reversed.clone()),
            Span::new(" b", Style::default()),
        ]
    );
    // Without a match the whole link is reversed, and only it.
    let marked = mark(&source, &[], &marks, None, Some(1..2));
    assert_eq!(
        marked.spans,
        [
            Span::new("a ", Style::default()),
            Span::new("kiri link", reversed),
            Span::new(" b", Style::default()),
        ]
    );
}

#[test]
fn the_current_match_is_marked_apart_and_keeps_the_text_s_style() {
    let link = Style {
        italic: true,
        fg: Some(Color::Palette(4)),
        link: Some("https://example.com".into()),
        ..Style::default()
    };
    let source = Line::new(vec![Span::new("kiri, kiri", link.clone())]);
    let marked = mark(
        &source,
        &[(0..4, false), (6..10, true)],
        &marks(),
        None,
        None,
    );
    assert_eq!(
        marked.spans,
        [
            Span::new(
                "kiri",
                Style {
                    reverse: true,
                    ..link.clone()
                }
            ),
            Span::new(", ", link.clone()),
            Span::new(
                "kiri",
                Style {
                    bold: true,
                    bg: Some(Color::Palette(3)),
                    ..link
                }
            ),
        ]
    );
}

#[test]
fn a_mark_in_text_at_a_fraction_keeps_its_groups() {
    // Packing looks at the words around a group, so a piece cut out of
    // the text could be packed apart from the rest.
    let two_thirds =
        TextSize::NORMAL.with_fraction(Fraction::new(2, 3).unwrap(), VerticalAlign::Top);
    let style = Style {
        size: two_thirds,
        ..Style::default()
    };
    let text = "dceceddaae ba";
    let source = Line::new(vec![Span::new(text, style.clone())]);
    let groups = |line: &Line| -> Vec<(String, u8, HorizontalAlign)> {
        (line.spans.iter())
            .flat_map(|span| span.style.size.groups(&span.text))
            .map(|group| (group.text.to_owned(), group.cells, group.align))
            .collect()
    };
    for start in 0..text.len() {
        for end in start + 1..=text.len() {
            let marked = mark(&source, &[(start..end, false)], &marks(), None, None);
            assert_eq!(groups(&marked), groups(&source), "{start}..{end}");
        }
    }
}

#[test]
fn a_mark_in_text_at_a_fraction_covers_whole_groups() {
    // Half-size, this word takes three groups: fourteen letters, six
    // and fourteen. Cutting a group would pack its pieces apart, each
    // rounded up, and move the cells after them.
    let half = TextSize::NORMAL.with_fraction(Fraction::new(1, 2).unwrap(), VerticalAlign::Top);
    let style = Style {
        size: half,
        ..Style::default()
    };
    let text = "abcdefghijklmnopqrstuvwxyzabcdefgh";
    let source = Line::new(vec![Span::new(text, style.clone())]);
    let marked = mark(&source, &[(16..17, false)], &marks(), None, None);
    let found: Vec<(&str, bool)> = (marked.spans.iter())
        .map(|span| (span.text.as_str(), span.style.reverse))
        .collect();
    assert_eq!(
        found,
        [
            ("abcdefghijklmn", false),
            ("opqrst", true),
            ("uvwxyzabcdefgh", false)
        ]
    );
    assert_eq!(
        crate::typeset::wrap::width(&marked.spans),
        crate::typeset::wrap::width(&source.spans)
    );
    // Two matches in one group mark it once; one across groups marks
    // both.
    let marked = mark(
        &source,
        &[(0..1, true), (1..15, false)],
        &marks(),
        None,
        None,
    );
    let texts: Vec<&str> = marked.spans.iter().map(|span| span.text.as_str()).collect();
    assert_eq!(texts, ["abcdefghijklmn", "opqrst", "uvwxyzabcdefgh"]);
    assert!(marked.spans[0].style.bold && marked.spans[1].style.reverse);
}

#[test]
fn a_match_that_touches_an_embed_marks_the_whole_image_with_a_background() {
    let source = Line::new(vec![
        Span::new("a ", Style::default()),
        Span::image("alt", 2, Style::default()),
        Span::new(" b", Style::default()),
    ]);
    // "a al": the text before it and a part of the embed's.
    let marked = mark(&source, &[(0..4, false)], &marks(), None, None);
    assert_eq!(marked.spans.len(), 3);
    let embed = &marked.spans[1];
    assert!(embed.embed.is_some());
    assert_eq!(embed.text, "alt");
    // A background, which the placeholders show; no reverse video, which
    // they would not.
    assert!(
        embed.style.bg.is_some() && !embed.style.reverse,
        "{:?}",
        embed.style
    );
    assert!(marked.spans[0].style.reverse);
    assert!(!marked.spans[2].style.reverse);
    // A mark with a background of its own gives it.
    let current = mark(&source, &[(3..4, true)], &marks(), None, None);
    assert_eq!(current.spans[1].style.bg, Some(Color::Palette(3)));
    // Nothing marks an embed that no match touches.
    let apart = mark(&source, &[(0..1, false)], &marks(), None, None);
    assert_eq!(apart.spans[2], source.spans[1]);
}

#[test]
fn a_marked_embed_has_a_frame_in_the_colour_of_its_background() {
    let source = Line::new(vec![
        Span::new("a ", Style::default()),
        Span::image("alt", 2, Style::default()),
    ]);
    let frame = |marked: &Line| {
        let image = marked.spans.iter().find_map(|span| span.embed.as_ref());
        image
            .unwrap()
            .placement
            .frame
            .as_ref()
            .map(|frame| frame.color)
    };
    assert_eq!(frame(&source), None);
    let current = mark(&source, &[(2..4, true)], &marks(), None, None);
    assert_eq!(frame(&current), Some(Color::Palette(3)));
    assert_eq!(current.spans[1].style.bg, Some(Color::Palette(3)));
    // A mark in reverse video frames in the colour that shows instead.
    let found = mark(&source, &[(2..4, false)], &marks(), None, None);
    assert_eq!(frame(&found), found.spans[1].style.bg);
    assert!(frame(&found).is_some());
    // Not marked: no frame.
    let apart = mark(&source, &[(0..1, false)], &marks(), None, None);
    assert_eq!(frame(&apart), None);
}

#[test]
fn an_image_in_the_selected_link_shows_the_selection_with_a_background_and_a_frame() {
    let link = Style {
        link: Some("u".into()),
        ..Style::default()
    };
    let source = Line::new(vec![
        Span::new("see ", link.clone()),
        Span::image("alt", 2, link.clone()),
        Span::new(" here", link),
    ]);
    let marked = mark(&source, &[], &marks(), None, Some(0..3));
    let image = &marked.spans[1];
    assert!(
        image.style.bg.is_some() && !image.style.reverse,
        "{:?}",
        image.style
    );
    assert_eq!(
        image
            .embed
            .as_ref()
            .unwrap()
            .placement
            .frame
            .clone()
            .map(|frame| frame.color),
        image.style.bg
    );
    assert!(marked.spans[0].style.reverse);
}

/// An image span of a line of a split: it shows row `row` of its
/// canvas, where the image itself is on the rows `ink`.
fn split_image(text: &str, row: u16, ink: Range<u16>) -> Span {
    let mut span = Span::image(text, 2, Style::default());
    let embed = span.embed.take().unwrap();
    span.embed = Some(Rc::new(Embed {
        row,
        rows: 1,
        ink_rows: ink,
        ..(*embed).clone()
    }));
    span
}

#[test]
fn a_mark_colours_and_frames_only_the_rows_the_image_is_on() {
    let on = |row| Line::new(vec![split_image("alt", row, 2..4)]);
    let embed = |line: &Line| line.spans[0].embed.clone().unwrap();
    // The text row is off the image's rows: no background there, but the
    // frame, which goes round its rows, is on its placement.
    let off = mark(&on(0), &[(0..3, true)], &marks(), None, None);
    assert_eq!(off.spans[0].style.bg, None);
    let frame = embed(&off).placement.frame.clone().expect("a frame");
    assert_eq!((frame.color, frame.rows), (Color::Palette(3), 2..4));
    let marked = mark(&on(3), &[(0..3, true)], &marks(), None, None);
    assert_eq!(marked.spans[0].style.bg, Some(Color::Palette(3)));
}

#[test]
fn a_mark_spreads_over_a_split_to_the_rows_the_image_is_on() {
    let first = split_image("", 0, 1..3);
    let shared = first.embed.as_ref().unwrap().placement.clone();
    let row = |row, text| {
        let mut span = split_image(text, row, 1..3);
        let embed = span.embed.take().unwrap();
        span.embed = Some(Rc::new(Embed {
            placement: shared.clone(),
            ..(*embed).clone()
        }));
        Line::new(vec![span])
    };
    let lines = [row(0, ""), row(1, ""), row(2, "alt")];
    let mut painted = vec![
        lines[0].clone(),
        lines[1].clone(),
        mark(&lines[2], &[(0..3, true)], &marks(), None, None),
    ];
    share_marks(&lines, &mut painted);
    let frame = |line: &Line| {
        line.spans[0]
            .embed
            .as_ref()
            .unwrap()
            .placement
            .frame
            .clone()
    };
    // The row above the image has nothing of it to mark.
    assert_eq!(painted[0].spans[0].style.bg, None);
    assert_eq!(painted[1].spans[0].style.bg, Some(Color::Palette(3)));
    assert_eq!(painted[2].spans[0].style.bg, Some(Color::Palette(3)));
    assert_eq!(frame(&painted[0]), frame(&painted[2]));
    assert_eq!(frame(&painted[0]).unwrap().rows, 1..3);
}

#[test]
fn a_mark_on_an_image_marks_it_on_every_row_of_a_split() {
    // The text row has its text, the rows above it only the image, all
    // showing the same placement.
    let text_row = Line::new(vec![
        Span::new("a ", Style::default()),
        Span::image("alt", 2, Style::default()),
    ]);
    let mut above = Line::new(vec![
        Span::new("  ", Style::default()),
        Span::image("", 2, Style::default()),
    ]);
    let shared = text_row.spans[1].embed.as_ref().unwrap().placement.clone();
    above.spans[1] = Span {
        embed: Some(std::rc::Rc::new(crate::typeset::style::Embed {
            placement: shared,
            ..(**above.spans[1].embed.as_ref().unwrap()).clone()
        })),
        ..above.spans[1].clone()
    };
    let lines = [above, text_row];
    let mut painted = vec![
        lines[0].clone(),
        mark(&lines[1], &[(2..4, true)], &marks(), None, None),
    ];
    share_marks(&lines, &mut painted);
    let embed = |line: &Line| line.spans[1].embed.clone().unwrap();
    let frame = |line: &Line| embed(line).placement.frame.clone().map(|frame| frame.color);
    assert_eq!(frame(&painted[0]), Some(Color::Palette(3)));
    assert_eq!(painted[0].spans[1].style.bg, Some(Color::Palette(3)));
    // The same placement on both rows, so one image for the terminal.
    assert!(std::rc::Rc::ptr_eq(
        &embed(&painted[0]).placement,
        &embed(&painted[1]).placement
    ));
}

#[test]
fn an_embed_without_text_stays_in_a_marked_line() {
    let source = Line::new(vec![
        Span::new("ab", Style::default()),
        Span::image("", 2, Style::default()),
    ]);
    let marked = mark(&source, &[(0..1, false)], &marks(), None, None);
    assert_eq!(marked.spans.len(), 3);
    assert!(marked.spans[2].embed.is_some());
}

#[test]
fn marking_keeps_the_rest_of_the_line() {
    let source = Line {
        lead: vec![Span::new("> ", Style::default())],
        trail: vec![Span::new("  ", Style::default())],
        ..Line::new(vec![Span::new(
            "Ä kiri",
            Style {
                size: TextSize::new(2).unwrap(),
                ..Style::default()
            },
        )])
    };
    let marked = mark(&source, &[(3..7, true)], &marks(), None, None);
    assert_eq!(
        Line {
            spans: Vec::new(),
            ..marked.clone()
        },
        Line {
            spans: Vec::new(),
            ..source
        }
    );
    let texts: Vec<&str> = marked.spans.iter().map(|span| span.text.as_str()).collect();
    assert_eq!(texts, ["Ä ", "kiri"]);
    assert_eq!(mark(&line("kiri"), &[], &marks(), None, None), line("kiri"));
}

#[test]
fn the_current_match_fades_from_the_text_s_own_style() {
    let source = Line::new(vec![Span::new("kiri", Style::default())]);
    let mut caps = Capabilities::kitty(80);
    caps.foreground = Some(Color::Rgb(200, 200, 200));
    caps.background = Some(Color::Rgb(0, 0, 0));
    caps.palette[3] = Some(Color::Rgb(200, 200, 0));
    let known = Rgb::new(&Theme::default(), &caps).unwrap();
    let at = |share, rgb| {
        let marked = mark(&source, &[(0..4, true)], &marks(), Some((share, rgb)), None);
        let style = &marked.spans[0].style;
        (style.fg, style.bg)
    };
    // From grey on black into grey on yellow.
    let grey = Some(Color::Rgb(200, 200, 200));
    assert_eq!(at(0.0, Some(&known)), (grey, Some(Color::Rgb(0, 0, 0))));
    assert_eq!(at(0.5, Some(&known)), (grey, Some(Color::Rgb(100, 100, 0))));
    // Without colours known as RGB, the mark at once.
    assert_eq!(at(0.0, None), (None, Some(Color::Palette(3))));
    // Other matches do not fade.
    let other = mark(
        &source,
        &[(0..4, false)],
        &marks(),
        Some((0.0, Some(&known))),
        None,
    );
    assert!(other.spans[0].style.reverse);
}

/// Row `row` of a heading `text` shown as an image of two rows.
fn heading_row(text: &str, row: u16) -> Line {
    let placement = crate::typeset::images::size::Placement::block(
        crate::typeset::images::ImageKey::heading(1, text, (10, 20)),
        crate::typeset::images::size::Size {
            width: 100,
            height: 40,
            columns: 10,
            rows: 2,
        },
        crate::typeset::images::size::Shows::Image,
    );
    let mut line = Line::image(std::rc::Rc::new(placement), row);
    if let Some(image) = &mut line.image {
        image.text = Some(text.to_owned());
    }
    line
}

#[test]
fn search_highlight_skips_an_image_match() {
    let marks = Marks::new(&Theme::default(), &Capabilities::kitty(80));
    let row = heading_row("Kiri heading", 0);
    assert_eq!(mark(&row, &[(0..4, true)], &marks, None, None), row);
    // Beside text before the image, such as a quote's bar.
    let mut row = heading_row("Kiri", 0);
    row.spans = vec![Span::new("> ", Style::default())];
    assert_eq!(mark(&row, &[(2..6, false)], &marks, None, None), row);
}
