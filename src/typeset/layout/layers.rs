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
mod tests;
