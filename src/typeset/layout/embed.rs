//! Images and formulas inside running text: made into spans that are the
//! cells of an image, and placed in the lines the wrapper made of them.
//!
//! An embed is fitted to its line by the rules of `images::size`; here the
//! line is shaped around the embeds it holds. The text sits in a *text
//! cell* of the line, a cell `ch` pixels high whose top is `T` pixels below
//! the line's top. A picture in a row has its opaque bottom on the text
//! cell's bottom, a tall one and a formula have it, or their baseline, on
//! the text's baseline. A line too low to hold what is above that is made
//! higher, and then split into one line for each of its rows, each showing
//! its row of every embed.

use std::ops::Range;
use std::rc::Rc;

use crate::typeset::diagrams::renderer;
use crate::typeset::diagrams::request::INLINE_FORMULA;
use crate::typeset::images::ImageKey;
use crate::typeset::images::size::{self, Fit, Placement, Shows, Size, Wanted};
use crate::typeset::layout::Layout;
use crate::typeset::model::{Image, Inline};
use crate::typeset::size::{TextSize, VerticalAlign};
use crate::typeset::style::{Embed, Line, Span, Style};
use crate::typeset::wrap::{self, width, wrap};

/// The span that draws `image` in the cells of a line of running text, or
/// `None` when it is to show as its alt text: when `layout` does not draw
/// embeds, when the terminal draws no images, and when the image is not
/// loaded, is loading, or failed.
pub(super) fn image(layout: &Layout, image: &Image, style: &Style) -> Option<Span> {
    let room = layout.embed_width?;
    if !layout.caps.draws_images() {
        return None;
    }
    let key = ImageKey::from(image.url.as_str()).in_document(layout.document);
    if layout.images.placeholder(&key).is_some() {
        return None;
    }
    let Some(Ok(source)) = layout.images.get(&key) else {
        return None;
    };
    let cell = size::cell_size(layout.caps);
    let scale = style.size.scale();
    let wanted = Wanted {
        width: image.width,
        height: image.height,
    };
    let room_pixels = room as f64 * f64::from(cell.0);
    let (wide, high) = size::wanted_size(source.size(), wanted, cell, room_pixels)?;
    let natural = (wide.round().max(1.0) as u32, high.round().max(1.0) as u32);
    let fit = size::fit_image(
        natural,
        layout.images.opaque(&key)?,
        (cell.0, cell.1 * u16::from(scale)),
        u16::try_from(room).unwrap_or(u16::MAX),
        layout.caps.max_image_cells(),
        scale == 1,
    );
    let alt = Inline::plain_text(&image.alt);
    Some(span(key, fit, &alt, style, false))
}

/// The span that draws the formula `source` in the cells of a line of
/// running text, in text style at 11/12 of its size in CSS pixels, or
/// `None` when it is to show as `written`: when `layout` does not draw
/// embeds, when the terminal draws no images, and when the formula's image
/// is not loaded, is loading, or failed.
pub(super) fn formula(
    layout: &Layout,
    source: &str,
    written: &[Inline],
    style: &Style,
) -> Option<Span> {
    let room = layout.embed_width?;
    if !layout.caps.draws_images() {
        return None;
    }
    // As reading mode asks for it.
    let code = format!("{source}\n");
    let key = ImageKey::diagram(INLINE_FORMULA, &code, layout.looks.diagram, layout.document);
    if layout.images.placeholder(&key).is_some() {
        return None;
    }
    let Some(Ok(image)) = layout.images.get(&key) else {
        return None;
    };
    let cell = size::cell_size(layout.caps);
    let scale = style.size.scale();
    let room_cell = (cell.0, cell.1 * u16::from(scale));
    // The text's size in the formula's own pixels: 11/12 of a CSS pixel.
    let css = f64::from(room_cell.1) / 20.0 * 11.0 / 12.0;
    let (wide, high) = image.size();
    let natural = (
        (f64::from(wide) * css).round().max(1.0) as u32,
        (f64::from(high) * css).round().max(1.0) as u32,
    );
    let depth = renderer::baseline_depth(INLINE_FORMULA, &code).unwrap_or(0.0);
    let fit = size::fit_formula(
        natural,
        layout.images.opaque(&key)?,
        depth,
        room_cell,
        u16::try_from(room).unwrap_or(u16::MAX),
        scale == 1,
    );
    Some(span(key, fit, &Inline::plain_text(written), style, true))
}

/// The span of an image fitted as `fit` says, which `text` stands for in a
/// search; shaped into its line by [`lines`].
fn span(key: ImageKey, fit: Fit, text: &str, style: &Style, formula: bool) -> Span {
    let size = Size {
        width: fit.width,
        height: fit.height,
        columns: fit.columns,
        rows: fit.rows,
    };
    let placement = Placement::block(key, size, Shows::Image);
    let mut embed = Embed::new(Rc::new(placement), 0, fit.rows);
    embed.fit = fit;
    embed.formula = formula;
    Span::embed(text, style.clone(), embed)
}

/// `spans` wrapped to `width` cells, as lines; the lines that hold embeds
/// are shaped to them. `scale` is the rows of the text the spans are in,
/// more than 1 in a heading.
pub(super) fn lines(layout: &Layout, spans: &[Span], width: usize) -> Vec<Line> {
    let wrapped = wrap(spans, width);
    if !spans.iter().any(|span| span.embed.is_some()) {
        return wrapped.into_iter().map(Line::new).collect();
    }
    let scale = (spans.iter())
        .map(|span| u16::from(span.style.size.scale()))
        .max()
        .unwrap_or(1);
    let cell = size::cell_size(layout.caps);
    shape(wrapped, spans, width, cell, layout.caps.text_sizing, scale)
}

/// The lines of `lines`, the spans of each line the wrapper made of
/// `spans`, with each embed in place: a line that holds embeds is one line
/// or, when its embeds need more rows than one above and below the text,
/// one line for each row. A heading's `scale` rows count as one row of
/// `scale` times the cell's height. The text of a line of two rows is
/// centred in them with text sizing, in packed groups that can take more
/// cells than the text did; a line that grows wider than `width` is
/// wrapped again, to a smaller width, until it fits.
pub(super) fn shape(
    lines: Vec<Vec<Span>>,
    spans: &[Span],
    width: usize,
    cell: (u16, u16),
    text_sizing: bool,
    scale: u16,
) -> Vec<Line> {
    let room = Room {
        cell,
        text_sizing,
        scale: i32::from(scale),
    };
    let mut shaped = shape_lines(lines, &room);
    let mut narrower = width;
    while (shaped.iter()).any(|line| wrap::width(&line.spans) > width) && narrower > 1 {
        narrower -= 1;
        shaped = shape_lines(wrap(spans, narrower), &room);
    }
    shaped
}

/// What a line is shaped for.
struct Room {
    cell: (u16, u16),
    text_sizing: bool,
    /// The rows of text the line's one row stands for.
    scale: i32,
}

fn shape_lines(lines: Vec<Vec<Span>>, room: &Room) -> Vec<Line> {
    lines
        .into_iter()
        .flat_map(|spans| shape_line(spans, room))
        .collect()
}

/// Where an embed lies in the line, in pixels from the line's top: of
/// its image's baseline and of the rows of ink on either side.
struct Reach {
    /// The ink above the baseline, and below it (negative when the ink
    /// ends above it).
    above: i32,
    below: i32,
    /// The baseline's distance below the top of the text cell.
    anchor: i32,
}

fn reach(embed: &Embed, ch: i32) -> Reach {
    let fit = &embed.fit;
    let baseline = fit.baseline as i32;
    // A picture in a row stands on the bottom of the text cell, a tall one
    // on the baseline of the text.
    let anchor = match embed.formula || fit.rows > 1 {
        true => size::baseline_in(ch as u32) as i32,
        false => ch,
    };
    Reach {
        above: baseline - fit.opaque.start as i32,
        below: fit.opaque.end as i32 - baseline,
        anchor,
    }
}

/// The size of text in a line of two rows, which is centred in them: twice
/// the scale at half the size, so that it is as large as it was.
fn centred_size(size: TextSize) -> TextSize {
    let two = TextSize::new(2).unwrap_or(TextSize::NORMAL);
    two.times(size.value() / 2.0, VerticalAlign::Centre)
}

/// Blank cells of the width of `span`, with only its background.
fn blank_of(span: &Span) -> Span {
    let style = Style {
        bg: span.style.bg,
        ..Style::default()
    };
    Span::new(&" ".repeat(width(std::slice::from_ref(span))), style)
}

/// The rows of the canvas of `(height, row height)` pixels that the opaque
/// rows of `fit`, its top `y` pixels down, are on, in cell rows of `scale`
/// rows of text each.
fn ink_rows(y: i32, fit: &Fit, (canvas, ch): (i32, i32), scale: i32) -> Range<u16> {
    let top = (y + fit.opaque.start as i32).clamp(0, canvas);
    let bottom = (y + fit.opaque.end as i32).clamp(top, canvas);
    let first = top / ch * scale;
    let last = (ceil_div(bottom, ch) * scale).max(first + 1);
    first as u16..last as u16
}

/// `a` over `b`, rounded up, for `a` not negative.
fn ceil_div(a: i32, b: i32) -> i32 {
    (a + b - 1) / b
}

fn shape_line(spans: Vec<Span>, room: &Room) -> Vec<Line> {
    let embeds: Vec<&Embed> = (spans.iter())
        .filter_map(|span| span.embed.as_deref())
        .collect();
    if embeds.is_empty() {
        return vec![Line::new(spans)];
    }
    let (cw, ch) = (i32::from(room.cell.0), i32::from(room.cell.1) * room.scale);
    let reaches: Vec<Reach> = embeds.iter().map(|embed| reach(embed, ch)).collect();
    let tall = (embeds.iter()).any(|embed| !embed.formula && embed.fit.rows > 1);
    let two = (embeds.iter()).any(|embed| embed.formula && embed.fit.rows > 1);
    // Two rows with the text centred in them, or one line for each row.
    let centred = two && !tall && room.text_sizing;
    let split = (tall || two) && !centred;
    // The rows above the text cell, and the rows of the line.
    let (above, rows) = if centred {
        (0, 2)
    } else if split {
        let above = (reaches.iter())
            .map(|reach| ceil_div((reach.above - reach.anchor).max(0), ch))
            .max()
            .unwrap_or(0);
        let bottom = (reaches.iter())
            .map(|reach| above * ch + reach.anchor + reach.below)
            .max()
            .unwrap_or(0);
        (above, (above + 1).max(ceil_div(bottom, ch)))
    } else {
        (0, 1)
    };
    let top = if centred { ch / 2 } else { above * ch };
    let canvas = rows * ch;
    let placements: Vec<(Rc<Placement>, Range<u16>)> = (embeds.iter().zip(&reaches))
        .map(|(embed, reach)| {
            // Moved only as far as the rows need to hold it.
            let low = reach.above;
            let high = (canvas - reach.below).max(low);
            let baseline = (top + reach.anchor).clamp(low, high);
            let fit = &embed.fit;
            let y = baseline - fit.baseline as i32;
            let x = (i32::from(fit.columns) * cw - fit.width as i32) / 2;
            let ink = ink_rows(y, fit, (canvas, ch), room.scale);
            let placement = Placement {
                size: Size {
                    rows: (rows * room.scale) as u16,
                    ..embed.placement.size
                },
                at: (x, y),
                ..(*embed.placement).clone()
            };
            (Rc::new(placement), ink)
        })
        .collect();
    // The line's own rows: one, or one line for each row of a split.
    let line_of = |row: i32| -> Vec<Span> {
        let mut placements = placements.iter();
        let mut line = Vec::new();
        for span in &spans {
            match &span.embed {
                Some(embed) => {
                    let (placement, ink_rows) =
                        placements.next().expect("a placement for each embed");
                    let (placement, ink_rows) = (Rc::clone(placement), ink_rows.clone());
                    let embed = match split {
                        true => Embed {
                            placement,
                            ink_rows,
                            row: row as u16,
                            rows: 1,
                            ..(**embed).clone()
                        },
                        false => Embed {
                            placement,
                            ink_rows,
                            row: 0,
                            rows: (rows * room.scale) as u16,
                            ..(**embed).clone()
                        },
                    };
                    // The text of an image is found on the text row only.
                    let text = if split && row != above {
                        ""
                    } else {
                        &span.text
                    };
                    line.push(Span::embed(text, span.style.clone(), embed));
                }
                None if split && row != above => line.push(blank_of(span)),
                None if centred => line.push(Span {
                    style: Style {
                        size: centred_size(span.style.size),
                        ..span.style.clone()
                    },
                    ..span.clone()
                }),
                None => line.push(span.clone()),
            }
        }
        line
    };
    match split {
        true => one_line_a_row(rows, above, line_of),
        false => vec![Line::new(line_of(0))],
    }
}

/// A split: one line for each of `rows` rows, `line_of` the spans of a row;
/// the first line says that a list's marker goes `above` lines down.
fn one_line_a_row(rows: i32, above: i32, line_of: impl Fn(i32) -> Vec<Span>) -> Vec<Line> {
    (0..rows)
        .map(|row| Line {
            marker_row: if row == 0 { above as u16 } else { 0 },
            ..Line::new(line_of(row))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: (u16, u16) = (10, 20);

    /// A formula of 30 × 40 pixels, which makes its line two rows high.
    fn formula() -> Span {
        let fit = size::fit_formula((30, 40), 0.0..1.0, 0.0, CELL, 100, true);
        assert_eq!(fit.rows, 2);
        span(ImageKey::from("f.png"), fit, "f", &Style::default(), true)
    }

    fn text(lines: &[Line]) -> String {
        let spans = lines.iter().flat_map(|line| &line.spans);
        spans
            .filter(|span| span.embed.is_none())
            .map(|span| span.text.as_str())
            .collect()
    }

    #[test]
    fn text_that_packs_wider_than_it_was_wraps_the_line_again() {
        // Characters in styles of their own each take a packed group, which
        // at half of scale 2 is two cells for one.
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let mut spans = vec![formula()];
        for (at, letter) in "abcdefghij".chars().enumerate() {
            let style = if at % 2 == 0 {
                Style::default()
            } else {
                bold.clone()
            };
            spans.push(Span::new(&letter.to_string(), style));
        }
        // 3 columns and 10 characters fit 13 as they are.
        let wrapped = wrap(&spans, 13);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(width(&wrapped[0]), 13);
        let lines = shape(wrapped, &spans, 13, CELL, true, 1);
        assert!(lines.len() > 1, "{lines:?}");
        for line in &lines {
            assert!(wrap::width(&line.spans) <= 13, "{line:?}");
        }
        // The line with the formula has its two rows; the others are text.
        assert_eq!(lines[0].rows(), 2);
        assert_eq!(lines[1].rows(), 1);
        assert_eq!(text(&lines), "abcdefghij");
        // Where the text does not pack wider, the line is as it was.
        let plain = vec![formula(), Span::new("abcdefghij", Style::default())];
        let lines = shape(wrap(&plain, 20), &plain, 20, CELL, true, 1);
        assert_eq!(lines.len(), 1);
        assert_eq!(text(&lines), "abcdefghij");
    }

    #[test]
    fn an_embed_knows_the_rows_its_image_is_on_not_those_its_canvas_covers() {
        // A tall image and an icon on one line: five rows, the text on the
        // last, the icon on the cell's bottom.
        let tall = size::fit_image((40, 90), 0.0..1.0, CELL, 100, 30, true);
        let icon = size::fit_image((16, 16), 0.0..1.0, CELL, 100, 30, true);
        let spans = vec![
            span(ImageKey::from("t.png"), tall, "t", &Style::default(), false),
            span(ImageKey::from("i.png"), icon, "i", &Style::default(), false),
        ];
        let lines = shape(vec![spans.clone()], &spans, 40, CELL, true, 1);
        assert_eq!(lines.len(), 5);
        let ink = |line: &Line, at: usize| line.spans[at].embed.as_ref().unwrap().ink_rows.clone();
        assert_eq!(ink(&lines[4], 0), 0..5);
        assert_eq!(ink(&lines[4], 1), 4..5);
        // A one-row line: the one row.
        let one = shape(vec![vec![spans[1].clone()]], &spans[1..], 40, CELL, true, 1);
        assert_eq!(ink(&one[0], 0), 0..1);
    }
}
