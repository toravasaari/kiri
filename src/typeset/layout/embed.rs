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

use std::rc::Rc;

use crate::typeset::images::ImageKey;
use crate::typeset::images::size::{self, Fit, Placement, Shows, Size, Wanted};
use crate::typeset::layout::Layout;
use crate::typeset::model::Image;
use crate::typeset::style::{Embed, Line, Span, Style};
use crate::typeset::wrap::{width, wrap};

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
        cell,
        u16::try_from(room).unwrap_or(u16::MAX),
        layout.caps.max_image_cells,
        true,
    );
    let alt = crate::typeset::model::Inline::plain_text(&image.alt);
    Some(span(key, fit, &alt, style, false))
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
/// are shaped to them.
pub(super) fn lines(layout: &Layout, spans: &[Span], width: usize) -> Vec<Line> {
    let wrapped = wrap(spans, width);
    if !spans.iter().any(|span| span.embed.is_some()) {
        return wrapped.into_iter().map(Line::new).collect();
    }
    shape(wrapped, size::cell_size(layout.caps))
}

/// The lines of `lines`, the spans of each line the wrapper made, with
/// each embed in place: a line that holds embeds is one line or, when its
/// embeds need more rows than one above and below the text, one line for
/// each row.
pub(super) fn shape(lines: Vec<Vec<Span>>, cell: (u16, u16)) -> Vec<Line> {
    lines
        .into_iter()
        .flat_map(|spans| shape_line(spans, cell))
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

/// `a` over `b`, rounded up, for `a` not negative.
fn ceil_div(a: i32, b: i32) -> i32 {
    (a + b - 1) / b
}

fn shape_line(spans: Vec<Span>, cell: (u16, u16)) -> Vec<Line> {
    let embeds: Vec<&Embed> = (spans.iter())
        .filter_map(|span| span.embed.as_deref())
        .collect();
    if embeds.is_empty() {
        return vec![Line::new(spans)];
    }
    let (cw, ch) = (i32::from(cell.0), i32::from(cell.1));
    let reaches: Vec<Reach> = embeds.iter().map(|embed| reach(embed, ch)).collect();
    let tall = (embeds.iter()).any(|embed| !embed.formula && embed.fit.rows > 1);
    let two = (embeds.iter()).any(|embed| embed.formula && embed.fit.rows > 1);
    let split = tall || two;
    // The rows above the text cell, and the rows of the line.
    let (above, rows) = if split {
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
    let top = above * ch;
    let canvas = rows * ch;
    let placements: Vec<Rc<Placement>> = (embeds.iter().zip(&reaches))
        .map(|(embed, reach)| {
            // Moved only as far as the rows need to hold it.
            let low = reach.above;
            let high = (canvas - reach.below).max(low);
            let baseline = (top + reach.anchor).clamp(low, high);
            let fit = &embed.fit;
            let y = baseline - fit.baseline as i32;
            let x = (i32::from(fit.columns) * cw - fit.width as i32) / 2;
            Rc::new(Placement {
                size: Size {
                    rows: rows as u16,
                    ..embed.placement.size
                },
                at: (x, y),
                ..(*embed.placement).clone()
            })
        })
        .collect();
    // The line's own rows: one, or one line for each row of a split.
    let line_of = |row: i32| -> Vec<Span> {
        let mut placements = placements.iter();
        let mut line = Vec::new();
        for span in &spans {
            match &span.embed {
                Some(embed) => {
                    let placement =
                        Rc::clone(placements.next().expect("a placement for each embed"));
                    let embed = match split {
                        true => Embed {
                            placement,
                            row: row as u16,
                            rows: 1,
                            ..(**embed).clone()
                        },
                        false => Embed {
                            placement,
                            row: 0,
                            rows: rows as u16,
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
                None if split && row != above => {
                    let blank = width(std::slice::from_ref(span));
                    let style = Style {
                        bg: span.style.bg,
                        ..Style::default()
                    };
                    line.push(Span::new(&" ".repeat(blank), style));
                }
                None => line.push(span.clone()),
            }
        }
        line
    };
    match split {
        true => (0..rows).map(|row| Line::new(line_of(row))).collect(),
        false => vec![Line::new(line_of(0))],
    }
}
