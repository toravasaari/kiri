//! Highlights over the lines of a layout: what the state engine wants shown
//! on a range of the document, and how both layouts paint it.
//!
//! A [`Highlight`] is a range of the document, what it means (a [`Paint`])
//! and a `z`. The meaning is the same in both layouts, and each layout picks
//! the means: the text layout (`layout/layers.rs`) lays colours and reverse
//! video over the bytes of its lines, and reading mode
//! (`reading/layers.rs`) puts boxes and a frame over a page. A range is
//! turned into lines, bytes and glyphs by the layout's [`Map`], so the same
//! range is painted wherever either layout shows it.
//!
//! A tint is a background mixed into the lines of its range, whole, and the
//! same for both layouts, so it is painted here. Where two highlights paint
//! the same text with the same ink, the higher `z` wins; a background is
//! mixed whatever its `z`.

use std::ops::Range;

use crate::typeset::color::Rgb;
use crate::typeset::layout::layers::{self as text, Marks};
use crate::typeset::locate::Map;
use crate::typeset::position::Pos;
use crate::typeset::reading::layers as pages;
use crate::typeset::style::{Line, Span, Style};
use crate::typeset::wrap;

/// A range of the document and what is painted on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Highlight {
    pub range: Range<Pos>,
    pub paint: Paint,
    /// Higher wins where two highlights paint the same text.
    pub z: u8,
}

/// What a highlight means; each layout paints it in its own way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Paint {
    /// A background mixed `amount` of the way toward the text colour, across
    /// the text area, on the lines of the range from its first to its last.
    Tint { amount: f32 },
    /// A search match. `current` is the one jumped to, and `fade` how far
    /// its fade in is, from 0 to 1; `None` when it is done.
    Match { current: bool, fade: Option<f32> },
    /// The selected link or picture.
    Selected,
}

/// What a layout needs to paint: how a match looks in text, the colours that
/// are mixed as RGB and the width of the text area.
pub struct Look<'a> {
    pub marks: &'a Marks,
    pub rgb: Option<&'a Rgb>,
    pub columns: usize,
}

/// The lines `window` of `lines`, the lines of a layout that `map` is of,
/// painted with `highlights`. A highlight that is not on the window is not
/// looked at.
pub fn paint(
    lines: &[Line],
    window: Range<usize>,
    map: &Map,
    highlights: &[Highlight],
    look: &Look,
) -> Vec<Line> {
    let window = window.start.min(lines.len())..window.end.min(lines.len());
    let mut painted = lines[window.clone()].to_vec();
    for highlight in highlights {
        let (Paint::Tint { amount }, Some(rgb)) = (highlight.paint, look.rgb) else {
            continue;
        };
        let Ok((_, rows)) = map.drawn_rows(lines, &highlight.range) else {
            continue;
        };
        for row in rows.start.max(window.start)..rows.end.min(window.end) {
            let line = &mut painted[row - window.start];
            *line = tint(line, rgb, amount, look.columns);
        }
    }
    // Matches and the selection are on the window when their range overlaps
    // the places the window shows.
    let shown = map.span_of(window.clone());
    let near: Vec<&Highlight> = (highlights.iter())
        .filter(|highlight| !matches!(highlight.paint, Paint::Tint { .. }))
        .filter(|highlight| {
            shown.as_ref().is_some_and(|shown| {
                highlight.range.end > shown.start && highlight.range.start <= shown.end
            })
        })
        .collect();
    if near.is_empty() {
        return painted;
    }
    text::paint(&mut painted, window.start, map, &near, look);
    text::share_marks(&lines[window], &mut painted);
    pages::paint(&mut painted, map, &near);
    painted
}

/// `line` with its background mixed `amount` of the way toward the text
/// colour, across the `width` cells of the text area. The background a span
/// has of its own, such as a code block's, is the one mixed; a span in
/// reverse video is left as it is. `line` as it is where the colours are
/// not known as RGB.
pub fn tint(line: &Line, rgb: &Rgb, amount: f32, width: usize) -> Line {
    let Some(page) = rgb.tinted(None, amount) else {
        return line.clone();
    };
    let spans = |spans: &[Span]| -> Vec<Span> {
        spans
            .iter()
            .map(|span| Span {
                style: tinted(&span.style, rgb, amount),
                ..span.clone()
            })
            .collect()
    };
    let mut line = Line {
        lead: spans(&line.lead),
        spans: spans(&line.spans),
        trail: spans(&line.trail),
        ..line.clone()
    };
    // The cells the line fills: what is repeated on every row, the text,
    // as scaled, an image row and the trail.
    let image = line.image.as_ref().map_or(0, |row| row.columns.len());
    let used =
        wrap::width(&line.lead) + wrap::width(&line.spans) + image + wrap::width(&line.trail);
    if used < width {
        let rest = Style {
            bg: Some(page),
            ..Style::default()
        };
        line.trail.push(Span::new(&" ".repeat(width - used), rest));
    }
    line
}

/// `style` with its background, or the page's where it has none, mixed
/// toward the text colour.
fn tinted(style: &Style, rgb: &Rgb, amount: f32) -> Style {
    if style.reverse {
        return style.clone();
    }
    match rgb.tinted(style.bg, amount) {
        Some(bg) => Style {
            bg: Some(bg),
            ..style.clone()
        },
        None => style.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::render::Inputs;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::color::Color;
    use crate::typeset::layout::{Content, Stream, content};
    use crate::typeset::parse::parse;
    use crate::typeset::reading::Fonts;
    use crate::typeset::reading::engine::{self, Reading};
    use crate::typeset::reading::page::Wide;
    use crate::typeset::targets::targets;
    use crate::typeset::text::DocumentText;
    use crate::typeset::theme::Theme;

    /// The cell layout at `columns`, or reading mode.
    fn inputs(reading: bool, columns: u16) -> Inputs {
        let mut caps = Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(columns)
        };
        caps.foreground = Some(Color::Rgb(200, 200, 200));
        caps.background = Some(Color::Rgb(0, 0, 0));
        let reading = reading.then(|| Reading {
            pictures: Default::default(),
            fonts: Rc::new(Fonts::fixture()),
            wide: Wide::Wrap,
        });
        Inputs {
            reading,
            ..Inputs::new(caps)
        }
    }

    fn marks() -> Marks {
        let style = |color| Style {
            bg: Some(Color::Palette(color)),
            ..Style::default()
        };
        Marks {
            found: style(1),
            current: style(3),
        }
    }

    fn rgb() -> Rgb {
        let mut caps = Capabilities::kitty(80);
        caps.foreground = Some(Color::Rgb(200, 200, 200));
        caps.background = Some(Color::Rgb(0, 0, 0));
        Rgb::new(&Theme::default(), &caps).unwrap()
    }

    /// `source` laid out, with its map and text.
    fn laid(source: &str, reading: bool, columns: u16) -> (Stream, Map, DocumentText) {
        let document = parse(source);
        let stream = content(Content::Document(&document), &inputs(reading, columns));
        let map = Map::new(&stream.lines, &stream.blocks);
        (stream, map, DocumentText::new([&document]))
    }

    fn matched(range: Range<Pos>, current: bool, z: u8) -> Highlight {
        Highlight {
            range,
            paint: Paint::Match {
                current,
                fade: None,
            },
            z,
        }
    }

    fn painted(stream: &Stream, map: &Map, window: Range<usize>, set: &[Highlight]) -> Vec<Line> {
        let (marks, rgb) = (marks(), rgb());
        let look = Look {
            marks: &marks,
            rgb: Some(&rgb),
            columns: 20,
        };
        paint(&stream.lines, window, map, set, &look)
    }

    /// The text of each span of `line` with its background.
    fn backgrounds(line: &Line) -> Vec<(&str, Option<Color>)> {
        let spans = line.spans.iter();
        spans
            .map(|span| (span.text.as_str(), span.style.bg))
            .collect()
    }

    #[test]
    fn a_range_is_painted_where_either_layout_shows_it() {
        let source = "one kiri two\n";
        let (stream, map, text) = laid(source, false, 40);
        let range = text.find("kiri", false).remove(0);
        let set = [matched(range.clone(), false, 2)];
        let lines = painted(&stream, &map, 0..stream.lines.len(), &set);
        assert_eq!(
            backgrounds(&lines[0]),
            [
                ("one ", None),
                ("kiri", Some(Color::Palette(1))),
                (" two", None)
            ]
        );
        // Reading mode: an overlay with the same bytes over the page.
        let (stream, map, text) = laid(source, true, 40);
        let range = text.find("kiri", false).remove(0);
        let set = [matched(range, true, 3)];
        let lines = painted(&stream, &map, 0..stream.lines.len(), &set);
        let overlays: Vec<_> = (lines.iter())
            .filter_map(|line| line.image.as_ref()?.overlay.clone())
            .collect();
        assert!(!overlays.is_empty());
        assert_eq!(overlays[0].marks.len(), 1);
        assert!(overlays[0].marks[0].2, "the current match");
    }

    #[test]
    fn a_match_that_a_line_break_splits_is_painted_on_both_lines() {
        let (stream, map, text) = laid("aaaaa bbbbb ccccc\n", false, 12);
        let range = text.find("bbbbb ccccc", false).remove(0);
        let lines = painted(
            &stream,
            &map,
            0..stream.lines.len(),
            &[matched(range, false, 2)],
        );
        let found = Some(Color::Palette(1));
        let marked: Vec<(usize, &str)> = (lines.iter().enumerate())
            .flat_map(|(at, line)| {
                let spans = line.spans.iter().filter(|span| span.style.bg == found);
                spans.map(move |span| (at, span.text.as_str()))
            })
            .collect();
        // The space the break takes is no part of either line's mark.
        assert_eq!(marked.len(), 2, "{marked:?}");
        assert_eq!(marked[0].1.trim_end(), marked[0].1);
        assert_ne!(marked[0].0, marked[1].0);
        let words: Vec<&str> = marked.iter().map(|(_, text)| *text).collect();
        assert_eq!(words, ["bbbbb", "ccccc"]);
    }

    #[test]
    fn where_a_match_and_the_selection_cover_the_same_text_the_higher_z_wins() {
        let source = "see [kiri link](https://example.com) here\n";
        let (stream, map, text) = laid(source, false, 40);
        let link = targets([&parse(source)]).links.remove(0).range;
        let found = text.find("kiri", false).remove(0);
        let selected = Highlight {
            range: link,
            paint: Paint::Selected,
            z: 1,
        };
        for set in [
            [selected.clone(), matched(found.clone(), false, 2)],
            [matched(found.clone(), false, 2), selected.clone()],
        ] {
            let lines = painted(&stream, &map, 0..stream.lines.len(), &set);
            let spans: Vec<(&str, bool, Option<Color>)> = (lines[0].spans.iter())
                .map(|span| (span.text.as_str(), span.style.reverse, span.style.bg))
                .collect();
            // The match keeps its bytes and the rest of the link is reversed.
            assert_eq!(
                spans,
                [
                    ("see ", false, None),
                    ("kiri", false, Some(Color::Palette(1))),
                    (" link", true, None),
                    (" here", false, None),
                ]
            );
        }
    }

    #[test]
    fn a_highlight_off_the_window_changes_nothing_and_only_the_window_comes_back() {
        let body: String = (0..20).map(|n| format!("word{n} kiri\n\n")).collect();
        let (stream, map, text) = laid(&body, false, 40);
        let first = text.find("kiri", false).remove(0);
        let set = [matched(first, false, 2)];
        let window = 10..14;
        let lines = painted(&stream, &map, window.clone(), &set);
        assert_eq!(lines, stream.lines[window]);
        let all = painted(&stream, &map, 0..stream.lines.len(), &set);
        assert_ne!(all[0], stream.lines[0]);
    }

    #[test]
    fn a_tint_is_on_the_lines_of_its_range_blank_ones_between_them_too() {
        let (stream, map, _) = laid("Alpha\n\nBeta\n\nGamma\n", false, 20);
        let text = DocumentText::new([&parse("Alpha\n\nBeta\n\nGamma\n")]);
        let (alpha, beta) = (
            text.find("Alpha", false).remove(0),
            text.find("Beta", false).remove(0),
        );
        let tint = Highlight {
            range: alpha.start..beta.end,
            paint: Paint::Tint { amount: 0.15 },
            z: 0,
        };
        let lines = painted(&stream, &map, 0..stream.lines.len(), &[tint]);
        let grey = Some(Color::Rgb(30, 30, 30));
        let tinted = |line: &Line| line.trail.last().map(|span| span.style.bg) == Some(grey);
        // Alpha, the blank line, Beta; not the blank line after it, nor Gamma.
        assert_eq!(
            lines.iter().map(tinted).collect::<Vec<_>>(),
            [true, true, true, false, false]
        );
    }

    #[test]
    fn the_selected_link_of_a_page_gets_a_frame_and_a_match_its_box() {
        let source = "a [kiri](https://example.com) and kiri\n";
        let (stream, map, text) = laid(source, true, 40);
        let link = targets([&parse(source)]).links.remove(0).range;
        let current = text.find("kiri", false).remove(1);
        let set = [
            Highlight {
                range: link,
                paint: Paint::Selected,
                z: 1,
            },
            matched(current, true, 3),
        ];
        let lines = painted(&stream, &map, 0..stream.lines.len(), &set);
        let page = engine::pages(&lines).remove(0);
        let overlay = (lines[page.lines].iter())
            .find_map(|line| line.image.as_ref()?.overlay.clone())
            .expect("an overlay");
        assert_eq!(overlay.link, Some(0));
        assert_eq!(overlay.marks.len(), 1);
    }
}
