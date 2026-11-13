//! How the text layout paints the highlights of the state engine
//! (`typeset/layers.rs`): the match styles a theme and a terminal give, and
//! the painter that lays colours and reverse video over the bytes of lines.
//!
//! A range is turned into the bytes of the lines that show it by the map of
//! the layout. A match is colours laid over the text, faded in from the
//! text's own while it is the one just jumped to; the selection is reverse
//! video. Colours do not stack: where two highlights cover the same bytes,
//! the one with the higher `z` keeps them, so a match in the selected link
//! stays a match. A span an image is in is never cut: a highlight that
//! touches it marks it whole, with a background and a frame, which the
//! placeholders show and reverse video would not.

use std::collections::BTreeMap;
use std::ops::Range;
use std::rc::Rc;

use crate::typeset::caps::Capabilities;
use crate::typeset::color::{Color, Rgb, Underline};
use crate::typeset::images::size::{Frame, Placement};
use crate::typeset::layers::{Highlight, Look, Paint};
use crate::typeset::locate::Map;
use crate::typeset::style::{Embed, Line, Origins, Span, Style};
use crate::typeset::theme::{SearchColor, Theme};

/// How matches are shown: the style laid over a match's text, and over
/// the current match's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marks {
    pub found: Style,
    pub current: Style,
}

impl Marks {
    /// The marks for `theme` on a terminal with `caps`.
    pub fn new(theme: &Theme, caps: &Capabilities) -> Self {
        let search = &theme.search;
        let style = |foreground, background| match (foreground, background) {
            _ if !caps.color => Style {
                reverse: true,
                ..Style::default()
            },
            (SearchColor::Auto, SearchColor::Auto) => Style {
                reverse: true,
                ..Style::default()
            },
            (foreground, background) => Style {
                fg: color(foreground),
                bg: color(background),
                ..Style::default()
            },
        };
        let current = Style {
            bold: true,
            ..style(search.current_foreground, search.current_background)
        };
        Self {
            found: style(search.match_foreground, search.match_background),
            current,
        }
    }
}

/// The colour `color` gives a match: none for the text's own.
fn color(color: SearchColor) -> Option<crate::typeset::color::Color> {
    match color {
        SearchColor::Color(color) => color,
        SearchColor::Auto => None,
    }
}

/// Paints `lines`, which start at line `first` of the layout `map` is of,
/// with the matches and the selection among `highlights`: the bytes they are
/// of each line.
pub(crate) fn paint(
    lines: &mut [Line],
    first: usize,
    map: &Map,
    highlights: &[&Highlight],
    look: &Look,
) {
    // Highest `z` first; the order of the list among the same `z`.
    let mut ordered: Vec<&&Highlight> = highlights.iter().collect();
    ordered.sort_by_key(|highlight| std::cmp::Reverse(highlight.z));
    let mut runs: BTreeMap<usize, Vec<Run>> = BTreeMap::new();
    for highlight in ordered {
        let ink = match highlight.paint {
            Paint::Match { current, fade } => Ink::Colours {
                style: match current {
                    true => &look.marks.current,
                    false => &look.marks.found,
                },
                fade,
            },
            Paint::Selected => Ink::Reverse,
            Paint::Tint { .. } => continue,
        };
        for (line, bytes) in map.line_bytes(&highlight.range) {
            if (first..first + lines.len()).contains(&line) {
                runs.entry(line).or_default().push(Run { bytes, ink });
            }
        }
    }
    for (line, runs) in runs {
        let line = &mut lines[line - first];
        *line = mark(line, &runs, look.rgb);
    }
}

/// The images of `painted` that a mark gave a placement of their own, with
/// a frame, have it on every line of `lines` that shows the same image: the
/// rows of a split, of which a match marks only the text row. The mark's
/// background goes only on the lines whose rows the image is on.
pub(crate) fn share_marks(lines: &[Line], painted: &mut [Line]) {
    let embeds = |line: &Line| -> Vec<Rc<Embed>> {
        (line.spans.iter())
            .filter_map(|span| span.embed.clone())
            .collect()
    };
    // What each marked placement of `lines` became, and the background it
    // has: the faded one where the line it was marked on shows it, and
    // otherwise the frame's colour.
    let mut marked: Vec<(Rc<Placement>, Rc<Placement>, Option<Color>)> = Vec::new();
    for (line, now) in lines.iter().zip(painted.iter()) {
        let now_spans = now.spans.iter().filter(|span| span.embed.is_some());
        for (before, span) in embeds(line).iter().zip(now_spans) {
            let after = &span.embed.as_ref().expect("an embed").placement;
            let known = (marked.iter()).any(|(old, ..)| Rc::ptr_eq(old, &before.placement));
            if !Rc::ptr_eq(&before.placement, after) && !known {
                let colour = after.frame.as_ref().map(|frame| frame.color);
                let now_bg = match on_rows(span.embed.as_ref().expect("an embed")) {
                    true => span.style.bg,
                    false => colour,
                };
                marked.push((Rc::clone(&before.placement), Rc::clone(after), now_bg));
            }
        }
    }
    if marked.is_empty() {
        return;
    }
    for (line, now) in lines.iter().zip(painted.iter_mut()) {
        let before = embeds(line);
        let spans = now.spans.iter_mut().filter(|span| span.embed.is_some());
        for (old, span) in before.iter().zip(spans) {
            let embed = span.embed.as_ref().expect("an embed");
            if !Rc::ptr_eq(&old.placement, &embed.placement) {
                continue;
            }
            let found = (marked.iter()).find(|(from, ..)| Rc::ptr_eq(from, &old.placement));
            if let Some((_, placement, bg)) = found {
                let shared = Embed {
                    placement: Rc::clone(placement),
                    ..(**embed).clone()
                };
                if on_rows(&shared) {
                    span.style.bg = *bg;
                }
                span.embed = Some(Rc::new(shared));
            }
        }
    }
}

/// What lies over some bytes of a line.
#[derive(Debug, Clone, Copy)]
enum Ink<'a> {
    /// Colours, bold and an underline laid over the text, faded in from
    /// the text's own by `fade` of the way while a fade is under way.
    Colours { style: &'a Style, fade: Option<f32> },
    /// Reverse video.
    Reverse,
}

/// Something over the bytes of a line's text.
#[derive(Debug, Clone)]
struct Run<'a> {
    bytes: Range<usize>,
    ink: Ink<'a>,
}

/// `style` with `ink` laid over it, in `rgb`'s colours for a fade.
fn laid(style: &Style, ink: Ink, rgb: Option<&Rgb>) -> Style {
    match ink {
        Ink::Reverse => Style {
            reverse: true,
            ..style.clone()
        },
        Ink::Colours { style: over, fade } => {
            let laid = laid_over(style, over);
            match (fade, rgb) {
                (Some(share), Some(rgb)) => rgb.mix(style, &laid, share).unwrap_or(laid),
                _ => laid,
            }
        }
    }
}

/// `line` with each of `runs` over the bytes it names in the line's text,
/// which is the text of its spans one after another: the colours laid over
/// the span's own style, from which they fade in where the run has a fade.
/// The runs are in the order of their weight: where two cover the same
/// bytes, the earlier keeps them. A span a run starts or ends in is split
/// there. In text at a fraction the split moves out to the ends of the
/// packed groups, so that the groups and the cells they take stay as they
/// are.
fn mark(line: &Line, runs: &[Run], rgb: Option<&Rgb>) -> Line {
    let mut spans = Vec::new();
    // Where the span starts in the line's text.
    let mut start = 0;
    for span in &line.spans {
        let end = start + span.text.len();
        let inside: Vec<&Run> = (runs.iter())
            .filter(|run| run.bytes.start < end && start < run.bytes.end)
            .collect();
        if span.embed.is_some() {
            // An image is never cut: a run that touches it marks it whole.
            spans.push(match inside.first() {
                Some(Run {
                    ink: Ink::Colours { style, fade },
                    ..
                }) => {
                    // The frame is in the colour the fade ends on, so that
                    // the image is not drawn anew at every step of it.
                    let frame = embed_marked(&span.style, style, None, rgb).bg;
                    marked_image(span, embed_marked(&span.style, style, *fade, rgb), frame)
                }
                Some(_) => {
                    let style = selected_image(&span.style);
                    let frame = style.bg;
                    marked_image(span, style, frame)
                }
                None => span.clone(),
            });
            start = end;
            continue;
        }
        if inside.is_empty() {
            spans.push(span.clone());
            start = end;
            continue;
        }
        // The runs in the span's own bytes, moved out to the ends of the
        // groups where the text is at a fraction.
        let edges = edges(span);
        let cut: Vec<Range<usize>> = (inside.iter())
            .map(|run| {
                let (mut from, mut to) = (
                    run.bytes.start.max(start) - start,
                    run.bytes.end.min(end) - start,
                );
                if !edges.is_empty() {
                    from = (edges.iter().rev().copied())
                        .find(|&edge| edge <= from)
                        .unwrap_or(0);
                    to = (edges.iter().copied())
                        .find(|&edge| edge >= to)
                        .unwrap_or(span.text.len());
                }
                from..to
            })
            .collect();
        let mut points: Vec<usize> = cut
            .iter()
            .flat_map(|bytes| [bytes.start, bytes.end])
            .chain([0, span.text.len()])
            .collect();
        points.sort_unstable();
        points.dedup();
        // Each stretch between two points is under the first run that
        // covers it; stretches under the same run, or none, go together.
        let mut pieces_of: Vec<(Range<usize>, Option<usize>)> = Vec::new();
        for pair in points.windows(2) {
            let covering = cut
                .iter()
                .position(|bytes| bytes.start <= pair[0] && pair[1] <= bytes.end);
            match pieces_of.last_mut() {
                Some((bytes, kept)) if *kept == covering => bytes.end = pair[1],
                _ => pieces_of.push((pair[0]..pair[1], covering)),
            }
        }
        for (bytes, covering) in pieces_of {
            let style = match covering {
                Some(at) => laid(&span.style, inside[at].ink, rgb),
                None => span.style.clone(),
            };
            spans.extend(pieces(span, bytes, &style));
        }
        start = end;
    }
    spans.retain(|span| !span.text.is_empty() || span.embed.is_some());
    Line {
        spans,
        ..line.clone()
    }
}

/// Whether the image of `embed` is on a row that its line shows.
fn on_rows(embed: &Embed) -> bool {
    embed.ink_rows.start < embed.row + embed.rows && embed.row < embed.ink_rows.end
}

/// The image `span` marked: its placement has a frame in `frame`, the
/// colour the mark's fade ends on, round the rows the image is on, and the
/// span has `style` if the line shows one of them.
fn marked_image(span: &Span, style: Style, frame: Option<Color>) -> Span {
    let Some(embed) = &span.embed else {
        return span.clone();
    };
    let placement = Placement {
        frame: frame.map(|color| Frame {
            color,
            rows: embed.ink_rows.clone(),
            picture: None,
        }),
        ..(*embed.placement).clone()
    };
    // The background shows only on the rows the image itself is on.
    let style = match on_rows(embed) {
        true => style,
        false => span.style.clone(),
    };
    Span {
        style,
        embed: Some(Rc::new(Embed {
            placement: Rc::new(placement),
            ..(**embed).clone()
        })),
        ..span.clone()
    }
}

/// The style of an image in `style` that is in the selected link: the
/// background that reverse video would be on text.
fn selected_image(style: &Style) -> Style {
    Style {
        bg: style.fg.or(Some(Color::Palette(7))),
        ..style.clone()
    }
}

/// The style of an image in `style` that `mark` marks. Only its background
/// shows, behind the image; reverse video would not, so a mark that is
/// reverse video becomes the text's colour as a background, or a light
/// grey where the text has none.
fn embed_marked(style: &Style, mark: &Style, fade: Option<f32>, rgb: Option<&Rgb>) -> Style {
    let marked = laid(style, Ink::Colours { style: mark, fade }, rgb);
    let bg = match marked.reverse && !style.reverse {
        true => style.fg.or(Some(Color::Palette(7))),
        false => marked.bg,
    };
    Style {
        bg,
        reverse: style.reverse,
        ..marked
    }
}

/// Where `span` can be cut without changing how it is drawn, as bytes of
/// its text: anywhere at a whole size, which has no edges, and only between
/// packed groups at a fraction.
fn edges(span: &Span) -> Vec<usize> {
    if span.style.size.fraction().is_none() {
        return Vec::new();
    }
    (span.style.size.groups(&span.text).iter())
        .map(|group| group.at)
        .chain([span.text.len()])
        .collect()
}

/// The bytes `bytes` of `span`'s text in `style`. A part of text at a
/// fraction is one span for each of its groups, packed as it was in the
/// whole text.
fn pieces(span: &Span, bytes: Range<usize>, style: &Style) -> Vec<Span> {
    if bytes == (0..span.text.len()) || span.style.size.fraction().is_none() {
        let text = span.text[bytes].to_owned();
        return vec![Span {
            text,
            style: style.clone(),
            embed: None,
            origins: Origins::default(),
        }];
    }
    (span.style.size.groups(&span.text).into_iter())
        .filter(|group| bytes.contains(&group.at))
        .map(|group| Span {
            text: group.text.to_owned(),
            style: Style {
                size: style.size.as_one_group(group.align),
                ..style.clone()
            },
            embed: None,
            origins: Origins::default(),
        })
        .collect()
}

/// `style` with `mark` laid over it: its colours where it has them, its
/// underline where it has one, and its bold and reverse video.
fn laid_over(style: &Style, mark: &Style) -> Style {
    Style {
        bold: style.bold || mark.bold,
        reverse: style.reverse || mark.reverse,
        underline: match mark.underline {
            Underline::None => style.underline,
            underline => underline,
        },
        fg: mark.fg.or(style.fg),
        bg: mark.bg.or(style.bg),
        ..style.clone()
    }
}

#[cfg(test)]
mod tests {
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
        let start = |spans: usize| -> usize {
            line.spans[..spans].iter().map(|span| span.text.len()).sum()
        };
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
}
