//! The highlights over the document as one list of layers: what each
//! paints, and where, so that a new highlight is a new layer and not a new
//! branch in the pager.
//!
//! A [`Layer`] is a [`Paint`] at an [`At`]. The paint says what is painted
//! in a vocabulary both layouts share: a background mixed toward the text colour,
//! colours, reverse video, a box, a frame. The address says where, in the layout's own terms:
//! lines, bytes of a line's text and spans in the text layout, glyph ranges
//! and links of a page in reading mode. Positions that both layouts share
//! are later work.
//!
//! The list goes from the bottom up, and each layout paints it in its own
//! way. A background is mixed into the background the text already has,
//! whatever its place in the list. In the text layout colours do not stack:
//! where two layers of colours cover the same bytes, the earlier one in the
//! list keeps them and the later one is dropped there, so a new highlight
//! that must show over a match goes before the matches in [`stack`]. Reverse
//! video, the selected link's, yields where colours cover the text, so that
//! a match in a link stays a match. In reading mode a box goes under the
//! glyphs, and the link's frame is drawn over the boxes.
//!
//! [`stack`] makes the list from what the pager knows, and [`paint`] paints
//! the lines with it.

use std::ops::Range;
use std::rc::Rc;

use crate::session::fade::{Fade, Rgb};
use crate::session::links;
use crate::session::search::{Marks, Search};
use crate::session::tint;
use crate::typeset::color::{Color, Underline};
use crate::typeset::images::size::Placement;
use crate::typeset::reading::engine;
use crate::typeset::reading::page::ItemRef;
use crate::typeset::style::{Embed, Line, Span, Style};

/// What a layer paints.
#[derive(Debug, Clone)]
pub enum Paint<'a> {
    /// A background mixed `amount` of the way toward the text colour,
    /// across the `columns` cells of the text area; in `rgb`'s colours.
    Background {
        amount: f32,
        rgb: &'a Rgb,
        columns: usize,
    },
    /// Colours, bold and an underline laid over the text, faded in from
    /// the text's own while a fade is under way.
    Colours {
        style: &'a Style,
        fade: Option<Fade<'a>>,
    },
    /// Reverse video, where no colours are laid over the text.
    Reverse,
    /// A box under the text, in the page's colours for a match, the
    /// current one's or the others'.
    Box { current: bool },
    /// A frame round something, in the page's colour for a link.
    Frame,
}

/// Where a layer paints, in the terms of the layout it is for.
#[derive(Debug, Clone)]
pub enum At {
    /// Whole lines of the text layout, by their number in the document.
    Lines(Range<usize>),
    /// Bytes of a line's text in the text layout: the text of its spans one
    /// after another.
    Bytes { line: usize, bytes: Range<usize> },
    /// Spans of a line in the text layout.
    Spans { line: usize, spans: Range<usize> },
    /// Glyphs of a page of reading mode: the page, by the address of its
    /// parts, the text in it and the bytes of that text.
    Glyphs {
        page: usize,
        item: ItemRef,
        range: Range<usize>,
    },
    /// A link of a page of reading mode, by its number among the page's.
    Link { page: usize, number: usize },
}

/// A paint at an address.
#[derive(Debug, Clone)]
pub struct Layer<'a> {
    pub paint: Paint<'a>,
    pub at: At,
}

impl Layer<'_> {
    /// Whether the paint is one that a layout paints at the address: the
    /// pairs the painters below handle. A layer that does not fit is a
    /// mistake in the layer, which no painter would show.
    fn fits(&self) -> bool {
        matches!(
            (&self.paint, &self.at),
            (Paint::Background { .. }, At::Lines(_))
                | (Paint::Colours { .. }, At::Bytes { .. })
                | (Paint::Reverse, At::Spans { .. })
                | (Paint::Box { .. }, At::Glyphs { .. })
                | (Paint::Frame, At::Link { .. })
        )
    }
}

/// How much to tint which lines, over how many cells, in what colours.
pub struct Tint<'a> {
    pub lines: Range<usize>,
    /// The share of the text colour mixed into the background.
    pub amount: f32,
    pub rgb: &'a Rgb,
    pub columns: usize,
}

/// What is highlighted over the document.
pub struct Highlights<'a> {
    /// The colouring after a reload.
    pub tint: Option<Tint<'a>>,
    /// The search whose matches are shown, if it is shown.
    pub search: Option<&'a Search>,
    /// How its matches are marked, and the current one's fade.
    pub marks: &'a Marks,
    pub fade: Option<Fade<'a>>,
    /// The selected link.
    pub link: Option<&'a links::Found>,
}

/// The layers of `highlights` over `lines`, which start at line `first` of
/// the document, from the bottom up: the tint, the matches and the
/// selected link.
pub fn stack<'a>(highlights: &Highlights<'a>, lines: &[Line], first: usize) -> Vec<Layer<'a>> {
    let mut layers = Vec::new();
    if let Some(tint) = &highlights.tint {
        layers.push(Layer {
            paint: Paint::Background {
                amount: tint.amount,
                rgb: tint.rgb,
                columns: tint.columns,
            },
            at: At::Lines(tint.lines.clone()),
        });
    }
    if let Some(search) = highlights.search {
        let (marks, fade) = (highlights.marks, highlights.fade);
        for line in first..first + lines.len() {
            for (bytes, current) in search.on(line) {
                let (style, fade) = match current {
                    true => (&marks.current, fade),
                    false => (&marks.found, None),
                };
                layers.push(Layer {
                    paint: Paint::Colours { style, fade },
                    at: At::Bytes { line, bytes },
                });
            }
        }
        for page in engine::pages(lines) {
            let page = std::rc::Rc::as_ptr(&page.parts) as usize;
            for (item, range, current) in search.in_page(page) {
                layers.push(Layer {
                    paint: Paint::Box { current },
                    at: At::Glyphs { page, item, range },
                });
            }
        }
    }
    if let Some(link) = highlights.link {
        for (line, spans) in &link.runs {
            layers.push(Layer {
                paint: Paint::Reverse,
                at: At::Spans {
                    line: *line,
                    spans: spans.clone(),
                },
            });
        }
        if let Some((page, number)) = link.link {
            layers.push(Layer {
                paint: Paint::Frame,
                at: At::Link { page, number },
            });
        }
    }
    layers
}

/// `lines`, from line `first` of the document on, painted with `layers`.
pub fn paint(lines: &[Line], first: usize, layers: &[Layer]) -> Vec<Line> {
    debug_assert!(
        layers.iter().all(Layer::fits),
        "a layer whose paint does not fit its address is painted by no layout: {:?}",
        layers.iter().find(|layer| !layer.fits())
    );
    let mut painted: Vec<Line> = (first..)
        .zip(lines)
        .map(|(index, line)| paint_text(line, index, layers))
        .collect();
    share_marks(lines, &mut painted);
    paint_pages(&mut painted, layers);
    painted
}

/// Line `index` in the text layout, painted with `layers`.
fn paint_text(line: &Line, index: usize, layers: &[Layer]) -> Line {
    let mut line = std::borrow::Cow::Borrowed(line);
    let mut runs = Vec::new();
    let mut selected = Vec::new();
    for layer in layers {
        match (&layer.paint, &layer.at) {
            (
                Paint::Background {
                    amount,
                    rgb,
                    columns,
                },
                At::Lines(lines),
            ) if lines.contains(&index) => {
                line = std::borrow::Cow::Owned(tint::tint(&line, rgb, *amount, *columns));
            }
            (Paint::Colours { style, fade }, At::Bytes { line, bytes }) if *line == index => {
                runs.push(Run {
                    bytes: bytes.clone(),
                    style,
                    fade: *fade,
                });
            }
            (Paint::Reverse, At::Spans { line, spans }) if *line == index => {
                selected.push(spans.clone());
            }
            _ => {}
        }
    }
    if runs.is_empty() && selected.is_empty() {
        return line.into_owned();
    }
    mark(&line, &runs, &selected)
}

/// The pages of reading mode among `lines`, with their overlays made of
/// the glyphs and links of `layers` that are on them.
fn paint_pages(lines: &mut [Line], layers: &[Layer]) {
    for page in engine::pages(lines) {
        let address = std::rc::Rc::as_ptr(&page.parts) as usize;
        let mut marks = Vec::new();
        let mut link = None;
        for layer in layers {
            match (&layer.paint, &layer.at) {
                (Paint::Box { current }, At::Glyphs { page, item, range }) if *page == address => {
                    marks.push((*item, range.clone(), *current))
                }
                (Paint::Frame, At::Link { page, number }) if *page == address => {
                    link = Some(*number);
                }
                _ => {}
            }
        }
        if let Some(overlay) = page.parts.overlay(marks, link) {
            engine::set_overlay(&mut lines[page.lines], Some(&overlay));
        }
    }
}

/// The images of `painted` that a mark made another placement of, with a
/// frame, are so on every line of `lines` that shows the same image: the
/// rows of a split, of which a match marks only the text row.
fn share_marks(lines: &[Line], painted: &mut [Line]) {
    let embeds = |line: &Line| -> Vec<Rc<Embed>> {
        (line.spans.iter())
            .filter_map(|span| span.embed.clone())
            .collect()
    };
    // What each marked placement of `lines` became.
    let mut marked: Vec<(Rc<Placement>, Rc<Placement>, Option<Color>)> = Vec::new();
    for (line, now) in lines.iter().zip(painted.iter()) {
        let now_spans = now.spans.iter().filter(|span| span.embed.is_some());
        for (before, span) in embeds(line).iter().zip(now_spans) {
            let after = &span.embed.as_ref().expect("an embed").placement;
            if !Rc::ptr_eq(&before.placement, after) {
                let known = marked
                    .iter()
                    .any(|(old, ..)| Rc::ptr_eq(old, &before.placement));
                if !known {
                    marked.push((
                        Rc::clone(&before.placement),
                        Rc::clone(after),
                        span.style.bg,
                    ));
                }
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
            if let Some((_, placement, bg)) = marked
                .iter()
                .find(|(from, ..)| Rc::ptr_eq(from, &old.placement))
            {
                span.embed = Some(Rc::new(Embed {
                    placement: Rc::clone(placement),
                    ..(**embed).clone()
                }));
                span.style.bg = *bg;
            }
        }
    }
}

/// Colours laid over the bytes of a line's text.
struct Run<'a> {
    bytes: Range<usize>,
    style: &'a Style,
    fade: Option<Fade<'a>>,
}

/// `line` with each of `runs`, from left to right, over the bytes it
/// names in the line's text, which is the text of its spans one after
/// another: its colours laid over the span's own style, from which they
/// fade in where the run has a fade. A span a run starts or ends in is
/// split there. In text at a fraction the split moves out to the ends of
/// the packed groups, so that the groups and the cells they take stay as
/// they are; a group in two runs takes the first one's style. The spans in
/// `selected` are in reverse video where no run covers them.
fn mark(line: &Line, runs: &[Run], selected: &[Range<usize>]) -> Line {
    let mut spans = Vec::new();
    // Where the span starts in the line's text.
    let mut start = 0;
    for (at, span) in line.spans.iter().enumerate() {
        let end = start + span.text.len();
        if span.embed.is_some() {
            // An image is never cut: a run that touches it marks it whole.
            let first = runs
                .iter()
                .find(|run| run.bytes.start < end && start < run.bytes.end);
            let selected = selected.iter().any(|link| link.contains(&at));
            spans.push(match first {
                Some(run) => marked_image(span, embed_marked(&span.style, run)),
                None if selected => marked_image(span, selected_image(&span.style)),
                None => span.clone(),
            });
            start = end;
            continue;
        }
        // What the span looks like where no run covers it: in reverse
        // video when it is of the selected link.
        let plain = match selected.iter().any(|link| link.contains(&at)) {
            true => Style {
                reverse: true,
                ..span.style.clone()
            },
            false => span.style.clone(),
        };
        // Where the span may be cut, from its start: found once a run
        // is in it.
        let mut edges = None;
        // The bytes of the span cut so far.
        let mut cut = start;
        let inside = runs
            .iter()
            .filter(|run| run.bytes.start < end && start < run.bytes.end);
        for run in inside {
            let bytes = &run.bytes;
            let (mut from, mut to) = (bytes.start.max(start) - start, bytes.end.min(end) - start);
            let edges = edges.get_or_insert_with(|| self::edges(span));
            if !edges.is_empty() {
                from = edges
                    .iter()
                    .rev()
                    .copied()
                    .find(|&edge| edge <= from)
                    .unwrap_or(0);
                to = (edges.iter().copied().find(|&edge| edge >= to)).unwrap_or(span.text.len());
            }
            let (from, to) = ((start + from).max(cut), start + to);
            if to <= from {
                continue;
            }
            let laid = laid_over(&span.style, run.style);
            let style = match run.fade {
                None => laid,
                Some(fade) => fade.step(&span.style, &laid),
            };
            spans.extend(pieces(span, cut - start..from - start, &plain));
            spans.extend(pieces(span, from - start..to - start, &style));
            cut = to;
        }
        spans.extend(pieces(span, cut - start..end - start, &plain));
        start = end;
    }
    spans.retain(|span| !span.text.is_empty() || span.embed.is_some());
    Line {
        spans,
        ..line.clone()
    }
}

/// The image `span` in `style`, which has a frame in its background's
/// colour as well.
fn marked_image(span: &Span, style: Style) -> Span {
    let Some(embed) = &span.embed else {
        return span.clone();
    };
    let placement = Placement {
        frame: style.bg,
        ..(*embed.placement).clone()
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

/// The style of an image in `style` that `run` marks. Only its background
/// shows, behind the image; reverse video would not, so a mark that is
/// reverse video becomes the text's colour as a background, or a light
/// grey where the text has none.
fn embed_marked(style: &Style, run: &Run) -> Style {
    let laid = laid_over(style, run.style);
    let laid = match run.fade {
        None => laid,
        Some(fade) => fade.step(style, &laid),
    };
    let bg = match laid.reverse && !style.reverse {
        true => style.fg.or(Some(Color::Palette(7))),
        false => laid.bg,
    };
    Style {
        bg,
        reverse: style.reverse,
        ..laid
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
        .map(|group| at(span, group.text))
        .chain([span.text.len()])
        .collect()
}

/// Where `text`, a slice of `span`'s text, starts in it.
fn at(span: &Span, text: &str) -> usize {
    text.as_ptr() as usize - span.text.as_ptr() as usize
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
        }];
    }
    (span.style.size.groups(&span.text).into_iter())
        .filter(|group| bytes.contains(&at(span, group.text)))
        .map(|group| Span {
            text: group.text.to_owned(),
            style: Style {
                size: style.size.as_one_group(group.align),
                ..style.clone()
            },
            embed: None,
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
    use crate::session::fade::Rgb;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::color::Color;
    use crate::typeset::size::{Fraction, HorizontalAlign, TextSize, VerticalAlign};
    use crate::typeset::theme::Theme;

    fn line(text: &str) -> Line {
        Line::new(vec![Span::new(text, Style::default())])
    }

    /// `line` with `matches`, each with whether it is the current one, in
    /// `marks`, faded by `fade`, and the spans `selected` of the selected
    /// link.
    fn mark(
        line: &Line,
        matches: &[(Range<usize>, bool)],
        marks: &Marks,
        fade: Option<Fade>,
        selected: Option<Range<usize>>,
    ) -> Line {
        let runs: Vec<Run> = (matches.iter())
            .map(|(bytes, current)| Run {
                bytes: bytes.clone(),
                style: if *current {
                    &marks.current
                } else {
                    &marks.found
                },
                fade: fade.filter(|_| *current),
            })
            .collect();
        super::mark(line, &runs, &selected.into_iter().collect::<Vec<_>>())
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
            image.unwrap().placement.frame
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
            image.embed.as_ref().unwrap().placement.frame,
            image.style.bg
        );
        assert!(marked.spans[0].style.reverse);
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
        assert_eq!(embed(&painted[0]).placement.frame, Some(Color::Palette(3)));
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
        let at = |millis, rgb| {
            let fade = Fade {
                elapsed: std::time::Duration::from_millis(millis),
                rgb,
            };
            let marked = mark(&source, &[(0..4, true)], &marks(), Some(fade), None);
            let style = &marked.spans[0].style;
            (style.fg, style.bg)
        };
        // From grey on black into grey on yellow.
        let grey = Some(Color::Rgb(200, 200, 200));
        assert_eq!(at(0, Some(&known)), (grey, Some(Color::Rgb(0, 0, 0))));
        assert_eq!(at(375, Some(&known)), (grey, Some(Color::Rgb(100, 100, 0))));
        assert_eq!(at(1000, Some(&known)), (None, Some(Color::Palette(3))));
        // Without colours known as RGB, the mark at once.
        assert_eq!(at(0, None), (None, Some(Color::Palette(3))));
        // Other matches do not fade.
        let fade = Fade {
            elapsed: std::time::Duration::ZERO,
            rgb: Some(&known),
        };
        let other = mark(&source, &[(0..4, false)], &marks(), Some(fade), None);
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

    #[test]
    fn where_colours_overlap_the_earlier_layer_keeps_the_text() {
        let first = Style {
            bg: Some(Color::Palette(1)),
            ..Style::default()
        };
        let second = Style {
            bg: Some(Color::Palette(2)),
            ..Style::default()
        };
        let layer = |style, bytes| Layer {
            paint: Paint::Colours { style, fade: None },
            at: At::Bytes { line: 0, bytes },
        };
        let layers = [layer(&first, 0..3), layer(&second, 2..4)];
        let painted = paint(&[line("kiri")], 0, &layers);
        // The later layer paints only what the earlier left.
        assert_eq!(
            painted[0].spans,
            [Span::new("kir", first), Span::new("i", second),]
        );
    }

    #[test]
    #[should_panic(expected = "paint does not fit its address")]
    fn a_layer_whose_paint_does_not_fit_its_address_is_a_mistake() {
        let layers = [Layer {
            paint: Paint::Frame,
            at: At::Lines(0..1),
        }];
        paint(&[line("kiri")], 0, &layers);
    }
}
