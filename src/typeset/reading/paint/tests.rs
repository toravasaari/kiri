use std::rc::Rc;

use super::*;
use crate::typeset::reading::Fonts;
use crate::typeset::reading::text::{Look, Run, Script, lay_out};

const INK: Brush = [0x20, 0xc0, 0x40];
const CHIP: Brush = [0x40, 0x40, 0xa0];

const LOOK: Look = Look {
    color: INK,
    ..Look::PLAIN
};

fn drawn(runs: &[Run], chip: Option<Brush>) -> RgbaImage {
    let paragraph = lay_out(&Fonts::fixture(), runs, &LOOK, 400.0);
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(420, 30, &mut glyphs);
    canvas.text(&paragraph, 10.0, 3.0, chip);
    canvas.into_image()
}

fn count(image: &RgbaImage, color: Brush) -> usize {
    (image.pixels())
        .filter(|pixel| pixel.0 == [color[0], color[1], color[2], 255])
        .count()
}

#[test]
fn text_is_drawn_in_its_colour_on_transparent_pixels() {
    let image = drawn(&[Run::new("Reading mode")], None);
    assert!(
        count(&image, INK) > 100,
        "{} pixels of ink",
        count(&image, INK)
    );
    assert_eq!(image.get_pixel(0, 0).0, [0; 4]);
    // Nothing left of where the text starts.
    assert!((0..30).all(|y| image.get_pixel(9, y).0[3] == 0));
}

#[test]
fn inline_code_is_drawn_on_a_chip() {
    let runs = [
        Run::new("Run "),
        Run {
            code: true,
            ..Run::new("make check")
        },
    ];
    let without = drawn(&runs, None);
    let with = drawn(&runs, Some(CHIP));
    assert_eq!(count(&without, CHIP), 0);
    assert!(
        count(&with, CHIP) > 500,
        "{} pixels of chip",
        count(&with, CHIP)
    );
    // The text over the chip, not under it.
    assert!(
        count(&with, INK) > 50,
        "{} pixels of ink",
        count(&with, INK)
    );
    // Text without code gets no chip.
    assert_eq!(count(&drawn(&[Run::new("Run")], Some(CHIP)), CHIP), 0);
}

/// The first and last row of `image` that have ink.
fn ink_rows(image: &RgbaImage) -> (u32, u32) {
    let rows: Vec<u32> = (0..image.height())
        .filter(|&y| (0..image.width()).any(|x| image.get_pixel(x, y).0 == [0x20, 0xc0, 0x40, 255]))
        .collect();
    (rows[0], rows[rows.len() - 1])
}

#[test]
fn text_in_pieces_is_drawn_one_line_below_the_other_across_a_cut() {
    // Lines of two colours each, so many that the text is cut in pieces.
    let word = |text: &str, grey: u8| Run {
        color: Some([grey; 3]),
        ..Run::new(text)
    };
    let mut runs = Vec::new();
    for line in 0..20_000 {
        if line > 0 {
            runs.push(Run::new("\n"));
        }
        runs.extend([word("let", 0x10), word(" x", 0x80)]);
    }
    let paragraph = lay_out(&Fonts::fixture(), &runs, &LOOK, 400.0);
    assert!(paragraph.pieces.len() > 1);
    let cut = paragraph.lines()[paragraph.pieces[1].first_line];
    // The strip: the two lines before the cut and the first one after.
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(420, 72, &mut glyphs);
    canvas.text(&paragraph, 10.0, 48.0 - cut.top, None);
    let image = canvas.into_image();
    let rows = |band: u32| -> Vec<u32> {
        (24 * band..24 * (band + 1))
            .filter(|&y| (0..420).any(|x| image.get_pixel(x, y).0[3] > 0))
            .map(|y| y - 24 * band)
            .collect()
    };
    assert!(!rows(1).is_empty(), "no ink in the line before the cut");
    assert_eq!(rows(2), rows(1), "the line after the cut");
    assert_eq!(rows(0), rows(1));
}

#[test]
fn a_superscript_after_a_cut_is_raised_as_in_a_text_of_one_piece() {
    // Many lines of two colours, then a line with a superscript, which is
    // the first line of the second piece or is far after the cut: its
    // glyphs are drawn higher than the plain `xx` after it.
    let word = |text: &str, grey: u8| Run {
        color: Some([grey; 3]),
        ..Run::new(text)
    };
    let mut runs = Vec::new();
    for line in 0..20_000 {
        if line > 0 {
            runs.push(Run::new("\n"));
        }
        runs.extend([word("let", 0x10), word(" x", 0x80)]);
    }
    runs.push(Run::new("\n"));
    runs.push(Run {
        script: Some(Script {
            size: 0.8,
            drop: -0.33,
        }),
        color: Some(INK),
        ..Run::new("xx")
    });
    runs.push(Run {
        color: Some(INK),
        ..Run::new(" xx")
    });
    let paragraph = lay_out(&Fonts::fixture(), &runs, &LOOK, 400.0);
    assert!(paragraph.pieces.len() > 1);
    let last = *paragraph.lines().last().unwrap();
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(420, 40, &mut glyphs);
    canvas.text(&paragraph, 10.0, 8.0 - last.top, None);
    let image = canvas.into_image();
    let ink = |from: u32, to: u32| -> Vec<u32> {
        (0..40)
            .filter(|&y| {
                (from..to).any(|x| image.get_pixel(x, y).0 == [INK[0], INK[1], INK[2], 255])
            })
            .collect()
    };
    // The superscript is the first two letters, the plain ones follow.
    let raised = ink(10, 24);
    let plain = ink(30, 50);
    assert!(!raised.is_empty() && !plain.is_empty());
    assert!(
        raised[0] < plain[0],
        "raised from row {}, plain from row {}",
        raised[0],
        plain[0]
    );
}

#[test]
fn a_struck_superscript_after_a_cut_has_its_line_raised_as_in_a_text_of_one_piece() {
    // Two `x` with a gap between them, struck: the longest row of ink in
    // each half is the strike-through line. The first is a superscript,
    // the second is plain.
    let word = |text: &str, grey: u8| Run {
        color: Some([grey; 3]),
        ..Run::new(text)
    };
    let mut runs = Vec::new();
    for line in 0..20_000 {
        if line > 0 {
            runs.push(Run::new("\n"));
        }
        runs.extend([word("let", 0x10), word(" x", 0x80)]);
    }
    runs.push(Run::new("\n"));
    runs.push(Run {
        script: Some(Script {
            size: 0.8,
            drop: -0.33,
        }),
        strike: true,
        color: Some(INK),
        ..Run::new("x        x")
    });
    runs.push(Run {
        strike: true,
        color: Some(INK),
        ..Run::new("x        x")
    });
    let paragraph = lay_out(&Fonts::fixture(), &runs, &LOOK, 400.0);
    assert!(paragraph.pieces.len() > 1);
    let last = *paragraph.lines().last().unwrap();
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(420, 40, &mut glyphs);
    canvas.text(&paragraph, 10.0, 8.0 - last.top, None);
    let image = canvas.into_image();
    let inked = |x: u32, y: u32| image.get_pixel(x, y).0[3] > 0;
    let columns: Vec<u32> = (0..420).filter(|&x| (0..40).any(|y| inked(x, y))).collect();
    let (first, last) = (columns[0], columns[columns.len() - 1]);
    let span = last - first;
    assert!(span > 100, "{span} columns of ink");
    // The row with the most ink in the columns from `from` to `to`.
    let line = |from: u32, to: u32| {
        let ink = |y: u32| (from..to).filter(|&x| inked(x, y)).count();
        (0..40).max_by_key(|&y| (ink(y), 40 - y)).unwrap()
    };
    let (raised, plain) = (
        line(first, first + span * 2 / 5),
        line(last - span * 2 / 5, last),
    );
    // The superscript is raised by 0.33 of its size, 5 pixels: its line
    // is at least 3 rows above the plain one.
    assert!(
        raised + 3 <= plain,
        "raised line on row {raised}, plain on row {plain}"
    );
}

#[test]
fn marked_text_is_drawn_on_its_background() {
    let marked = Run {
        background: Some(CHIP),
        ..Run::new("found")
    };
    let image = drawn(&[Run::new("a "), marked], None);
    assert!(
        count(&image, CHIP) > 300,
        "{} pixels of background",
        count(&image, CHIP)
    );
    // The text over the box, not under it.
    assert!(count(&image, INK) > 50);
    assert_eq!(count(&drawn(&[Run::new("a found")], None), CHIP), 0);
}

#[test]
fn a_subscript_is_lowered_and_a_superscript_raised_and_both_are_smaller() {
    let script = |drop| Run {
        script: Some(Script { size: 0.8, drop }),
        ..Run::new("xx")
    };
    let (top, bottom) = ink_rows(&drawn(&[Run::new("xx")], None));
    let (sub_top, sub_bottom) = ink_rows(&drawn(&[script(0.2)], None));
    let (sup_top, sup_bottom) = ink_rows(&drawn(&[script(-0.33)], None));
    assert!(
        sub_bottom > bottom,
        "sub ends at {sub_bottom}, text at {bottom}"
    );
    assert!(
        sup_bottom < bottom,
        "sup ends at {sup_bottom}, text at {bottom}"
    );
    assert!(sup_top < top, "sup starts at {sup_top}, text at {top}");
    assert!(
        sub_bottom - sub_top < bottom - top,
        "sub is {} high, text {}",
        sub_bottom - sub_top,
        bottom - top
    );
}

/// The last row of `image` with ink in the columns `from..to`.
fn ink_bottom_in(image: &RgbaImage, from: u32, to: u32) -> u32 {
    (0..image.height())
        .rev()
        .find(|&y| (from..to).any(|x| image.get_pixel(x, y).0 == [0x20, 0xc0, 0x40, 255]))
        .expect("ink in the columns")
}

#[test]
fn a_superscript_beside_a_subscript_keeps_its_own_shift() {
    let script = |text: &str, drop| Run {
        script: Some(Script { size: 0.8, drop }),
        ..Run::new(text)
    };
    let sup = drawn(&[Run::new("x"), script("ab", -0.33)], None);
    let edge = (0..sup.width())
        .rev()
        .find(|&x| (0..sup.height()).any(|y| sup.get_pixel(x, y).0[3] > 0))
        .unwrap()
        + 2;
    let both = drawn(
        &[
            Run::new("x"),
            script("ab", -0.33),
            script("cd", 0.2),
            Run::new("x"),
        ],
        None,
    );
    // The leading `x` ends where the superscript starts.
    let lone = drawn(&[Run::new("x")], None);
    let start = (0..lone.width())
        .rev()
        .find(|&x| (0..lone.height()).any(|y| lone.get_pixel(x, y).0[3] > 0))
        .unwrap()
        + 2;
    let (plain, high, low) = (
        ink_bottom_in(&both, 0, start),
        ink_bottom_in(&both, start, edge),
        ink_bottom_in(&both, edge, both.width()),
    );
    assert!(high < plain, "sup ends at {high}, text at {plain}");
    assert!(low > plain, "sub ends at {low}, text at {plain}");
}

#[test]
fn the_strike_of_a_subscript_beside_a_superscript_is_at_the_subscripts_height() {
    let struck = |text: &str, drop| Run {
        strike: true,
        script: Some(Script { size: 0.8, drop }),
        ..Run::new(text)
    };
    // After the `a`, the `x` and its strike are drawn as they are alone.
    let rows = |image: &RgbaImage, from: u32| -> Vec<u32> {
        (0..image.height())
            .filter(|&y| (from..image.width()).any(|x| image.get_pixel(x, y).0[3] > 0))
            .collect()
    };
    let alone = drawn(&[struck("x", 0.2)], None);
    let first = drawn(&[struck("a", -0.33)], None);
    let edge = (0..first.width())
        .rev()
        .find(|&x| (0..first.height()).any(|y| first.get_pixel(x, y).0[3] > 0))
        .unwrap()
        + 2;
    let both = drawn(&[struck("a", -0.33), struck("x", 0.2)], None);
    assert_eq!(
        rows(&both, edge),
        rows(&alone, 0),
        "the subscript and its strike beside a superscript"
    );
}

#[test]
fn the_strike_stops_at_the_end_of_its_own_text_in_a_run_of_other_styles() {
    let struck = Run {
        strike: true,
        ..Run::new("bbbb")
    };
    let runs = [Run::new("aa "), struck, Run::new(" cccc dddd")];
    // Wide enough for "aa bbbb " and no more: the line ends in a space.
    let paragraph = lay_out(&Fonts::fixture(), &runs, &LOOK, 70.0);
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(420, 60, &mut glyphs);
    canvas.text(&paragraph, 10.0, 3.0, None);
    let image = canvas.into_image();
    let right = (0..image.width())
        .rev()
        .find(|&x| (0..28).any(|y| image.get_pixel(x, y).0[3] > 0))
        .unwrap();
    // "aa " is 24.7 pixels and "bbbb" 40.6, from the text's left edge, 10.
    assert!(right <= 76, "ink on the first line reaches column {right}");
}

#[test]
fn a_script_keeps_its_shift_beside_another_of_a_different_colour() {
    // One glyph run for the shaping, two for the colours.
    let script = |text: &str, drop, color| Run {
        script: Some(Script { size: 0.8, drop }),
        color: Some(color),
        ..Run::new(text)
    };
    let (high, low): (Brush, Brush) = ([0xc0, 0xc0, 0x20], [0xc0, 0x20, 0x40]);
    // The plain `x` in the ink colour is the line the scripts are measured by.
    let image = drawn(
        &[
            Run::new("x"),
            script("ab", -0.33, high),
            script("cd", 0.2, low),
        ],
        None,
    );
    let bottom_of = |color: Brush| {
        (0..image.height())
            .rev()
            .find(|&y| (0..image.width()).any(|x| image.get_pixel(x, y).0[..3] == color))
            .unwrap()
    };
    let (plain, sup, sub) = (bottom_of(INK), bottom_of(high), bottom_of(low));
    assert!(sup < plain, "sup ends at {sup}, text at {plain}");
    assert!(sub > plain, "sub ends at {sub}, text at {plain}");
}

#[test]
fn a_mark_does_not_change_the_struck_text_outside_it() {
    const RED: Brush = [0xc0, 0x20, 0x20];
    const YELLOW: Brush = [0xff, 0xff, 0];
    let struck = |text: &str| Run {
        strike: true,
        ..Run::new(text)
    };
    // Both words are struck, in one glyph run; the mark is on the second.
    let runs = [struck("ss mm")];
    let paragraph = lay_out(&Fonts::fixture(), &runs, &LOOK, 400.0);
    let mark_left = paragraph.boxes(3..5)[0].left;
    let draw = |marks: &[TextMark]| {
        let mut glyphs = GlyphCache::default();
        let mut canvas = Canvas::new(420, 30, &mut glyphs);
        canvas.text_marked(&paragraph, 10.0, 3.0, None, marks);
        canvas.into_image()
    };
    let plain = draw(&[]);
    let mark = TextMark {
        range: 3..5,
        foreground: Some(RED),
        background: Some(YELLOW),
    };
    let marked = draw(&[mark]);
    let left_of_mark = (10.0 + mark_left).floor() as u32;
    let differing = (0..left_of_mark)
        .flat_map(|x| (0..30).map(move |y| (x, y)))
        .filter(|&(x, y)| plain.get_pixel(x, y) != marked.get_pixel(x, y))
        .count();
    assert_eq!(differing, 0, "a mark changes the struck text outside it");
    // Inside the mark the strike has the mark's colour.
    assert!(
        count(&marked, RED) > 20,
        "{} pixels of red",
        count(&marked, RED)
    );
}

#[test]
fn italic_text_is_slanted() {
    let upright = drawn(&[Run::new("llll")], None);
    let italic = drawn(
        &[Run {
            italic: true,
            ..Run::new("llll")
        }],
        None,
    );
    assert!(upright != italic, "italic text drawn upright");
}

/// Text as it was drawn before glyphs were kept: the outlines of a
/// glyph run's glyphs in one path, at the exact places of the glyphs,
/// filled once. Without colour glyphs, scripts or marks.
fn filled_once(paragraph: &Paragraph, (x, y): (f32, f32), size: (u32, u32)) -> Pixmap {
    let mut pixmap = Pixmap::new(size.0, size.1).unwrap();
    for piece in &paragraph.pieces {
        for item in piece.layout.lines().flat_map(|line| line.items()) {
            let PositionedLayoutItem::GlyphRun(glyphs) = item else {
                continue;
            };
            let run = glyphs.run();
            let font = run.font();
            let face = FontRef::from_index(font.data.data(), font.index).unwrap();
            let coords: Vec<NormalizedCoord> = (run.normalized_coords().iter())
                .map(|coord| F2Dot14::from_bits(coord.to_bits()))
                .collect();
            let skew = run.synthesis().skew().map_or(0.0, |d| d.to_radians().tan());
            let outlines = face.outline_glyphs();
            let mut path = PathBuilder::new();
            for glyph in glyphs.positioned_glyphs() {
                let outline = outlines.get(GlyphId::new(glyph.id)).unwrap();
                let mut pen = Pen {
                    path: &mut path,
                    x: x + glyph.x,
                    y: y + piece.top + glyph.y,
                    skew,
                };
                let size = Size::new(run.font_size());
                let settings = DrawSettings::unhinted(size, LocationRef::new(&coords));
                outline.draw(settings, &mut pen).unwrap();
            }
            let path = path.finish().unwrap();
            let ink = &paint(glyphs.style().brush, 255);
            let rule = FillRule::Winding;
            pixmap.fill_path(&path, ink, rule, Transform::identity(), None);
        }
    }
    pixmap
}

/// How the pixels of `drawn` differ from those of `exact`: the largest
/// difference of a channel, and the share of the pixels with ink in
/// either that differ at all.
fn differences(drawn: &Pixmap, exact: &Pixmap) -> (u8, f64) {
    let (mut largest, mut inked, mut differing) = (0, 0, 0);
    for (a, b) in drawn.data().chunks(4).zip(exact.data().chunks(4)) {
        if a[3] == 0 && b[3] == 0 {
            continue;
        }
        inked += 1;
        let most = (a.iter().zip(b))
            .map(|(&a, &b)| a.abs_diff(b))
            .max()
            .unwrap();
        differing += usize::from(most > 0);
        largest = largest.max(most);
    }
    (largest, differing as f64 / inked as f64)
}

#[test]
fn text_is_drawn_within_a_little_of_the_glyphs_filled_in_one_path() {
    let plain = |text: &str| Run::new(text);
    let italic = |text: &str| Run {
        italic: true,
        ..Run::new(text)
    };
    let bold = |text: &str| Run {
        bold: true,
        ..Run::new(text)
    };
    let texts = [
        // Pairs that sit tight together, the glyphs' outlines overlapping
        // at their feet and tops where the kerning pulls them in.
        vec![plain("AVATAR To Wa Ty AV,")],
        vec![italic("AVATAR To Wa Ty fly,"), plain(" ff fi fl")],
        vec![plain("Reading mode "), bold("kerning "), italic("pairs")],
    ];
    for runs in texts {
        let paragraph = lay_out(&Fonts::fixture(), &runs, &LOOK, 400.0);
        // At places that fall at every part of a pixel.
        for (x, y) in [(10.0, 3.0), (10.37, 3.5), (10.81, 3.09), (11.5, 3.91)] {
            let mut glyphs = GlyphCache::default();
            let mut canvas = Canvas::new(420, 30, &mut glyphs);
            canvas.text(&paragraph, x, y, None);
            let exact = filled_once(&paragraph, (x, y), (420, 30));
            let (largest, share) = differences(&canvas.pixmap, &exact);
            // A glyph's place is rounded to a sixteenth of a pixel, which
            // moves an edge by a sub-scanline of tiny-skia's rasteriser, a
            // quarter of the coverage; measured at 64 of 255 on edges and
            // 15 to 23 % of the pixels with ink.
            assert!(largest <= 64, "{largest} of 255 differ at most");
            assert!(share < 0.3, "{share} of the pixels with ink differ");
        }
    }
}

#[test]
fn emoji_are_drawn_in_their_colours() {
    let image = drawn(&[Run::new("Kiri 😀")], None);
    let coloured = (image.pixels())
        .filter(|pixel| pixel.0[3] == 255 && pixel.0[..3] != INK)
        .count();
    assert!(coloured > 100, "{coloured} pixels in colour");
    let yellow = (image.pixels())
        .filter(|pixel| pixel.0[0] > 200 && pixel.0[1] > 150 && pixel.0[2] < 100)
        .count();
    assert!(yellow > 50, "{yellow} yellow pixels");
}

fn red(width: u32, height: u32, alpha: u8) -> RgbaImage {
    RgbaImage::from_pixel(width, height, image::Rgba([200, 0, 0, alpha]))
}

#[test]
fn a_picture_is_drawn_where_it_is_and_cut_at_the_canvas() {
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(10, 10, &mut glyphs);
    canvas.picture(&Pixels::new(red(4, 4, 255)), 2.0, 3.0);
    let image = canvas.into_image();
    assert_eq!(image.get_pixel(2, 3).0, [200, 0, 0, 255]);
    assert_eq!(image.get_pixel(5, 6).0, [200, 0, 0, 255]);
    assert_eq!(image.get_pixel(6, 6).0[3], 0);
    assert_eq!(image.get_pixel(1, 3).0[3], 0);
    assert_eq!(image.get_pixel(2, 2).0[3], 0);
    // Reaching past the top, the left and the bottom edges.
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(10, 10, &mut glyphs);
    canvas.picture(&Pixels::new(red(4, 4, 255)), -2.0, -3.0);
    canvas.picture(&Pixels::new(red(4, 4, 255)), 8.0, 8.0);
    let image = canvas.into_image();
    assert_eq!(image.get_pixel(0, 0).0, [200, 0, 0, 255]);
    assert_eq!(image.get_pixel(1, 0).0, [200, 0, 0, 255]);
    assert_eq!(image.get_pixel(2, 0).0[3], 0);
    assert_eq!(image.get_pixel(0, 1).0[3], 0);
    assert_eq!(image.get_pixel(9, 9).0, [200, 0, 0, 255]);
    // Wholly off the canvas: nothing, and no trouble.
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(10, 10, &mut glyphs);
    canvas.picture(&Pixels::new(red(4, 4, 255)), 20.0, 0.0);
    canvas.picture(&Pixels::new(red(4, 4, 255)), 0.0, -20.0);
    canvas.picture(&Pixels::new(red(4, 4, 255)), -20.0, 0.0);
    assert!(canvas.into_image().pixels().all(|pixel| pixel.0[3] == 0));
}

#[test]
fn a_canvas_converts_to_the_same_pixels_as_the_old_loop() {
    // The loop that `into_image` used to run, kept as the reference.
    fn old_loop(pixmap: &Pixmap) -> Vec<u8> {
        (pixmap.pixels().iter())
            .flat_map(|pixel| {
                let color = pixel.demultiply();
                [color.red(), color.green(), color.blue(), color.alpha()]
            })
            .collect()
    }
    // The first row runs through every alpha from 0 to 255, the second
    // back down from 255 to 0, and the third is transparent and opaque.
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(256, 3, &mut glyphs);
    for (index, pixel) in canvas.pixmap.pixels_mut().iter_mut().enumerate() {
        let (x, y) = (index % 256, index / 256);
        let alpha = match y {
            0 => x,
            1 => 255 - x,
            _ if x % 3 == 0 => 0,
            _ => 255,
        } as u8;
        let colour = [
            (x * 37 % 256) as u8,
            (x * 91 + y * 13) as u8,
            ((x ^ y) * 53) as u8,
        ];
        let [red, green, blue, alpha] = premultiplied([colour[0], colour[1], colour[2], alpha]);
        *pixel = PremultipliedColorU8::from_rgba(red, green, blue, alpha)
            .expect("premultiplied colours are at most their alpha");
    }
    let expected = old_loop(&canvas.pixmap);
    let image = canvas.into_image();
    assert_eq!((image.width(), image.height()), (256, 3));
    assert_eq!(image.as_raw(), &expected);
    // Runs of clear and of opaque pixels, with translucent ones between
    // them, so that the runs cross the pairs of pixels that the
    // conversion skips over, and the odd widths leave a pixel over.
    for width in [1, 2, 3, 5, 37] {
        let mut glyphs = GlyphCache::default();
        let mut canvas = Canvas::new(width, 29, &mut glyphs);
        for (index, pixel) in canvas.pixmap.pixels_mut().iter_mut().enumerate() {
            let index = index as u32;
            let alpha = match index % 29 {
                0..=10 => 0,
                11..=18 => 255,
                _ => (index * 37 % 254 + 1) as u8,
            };
            let colour = [
                (index * 53 % 256) as u8,
                (index * 91 % 256) as u8,
                (index % 7) as u8,
            ];
            let [red, green, blue, alpha] = premultiplied([colour[0], colour[1], colour[2], alpha]);
            *pixel = PremultipliedColorU8::from_rgba(red, green, blue, alpha)
                .expect("premultiplied colours are at most their alpha");
        }
        let expected = old_loop(&canvas.pixmap);
        let image = canvas.into_image();
        assert_eq!(image.as_raw(), &expected, "width {width}");
    }
}

#[test]
fn a_mark_is_a_box_with_the_text_drawn_again_in_it() {
    let paragraph = lay_out(&Fonts::fixture(), &[Run::new("Ab cd")], &LOOK, 400.0);
    let lines = paragraph.lines();
    let boxes = paragraph.boxes(3..5);
    let [part] = boxes[..] else {
        panic!("one box: {boxes:?}");
    };
    let mark = TextMark {
        range: 3..5,
        foreground: Some([0, 200, 0]),
        background: Some([200, 0, 200]),
    };
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(120, 40, &mut glyphs);
    canvas.text_marked(&paragraph, 4.0, 5.0, None, &[mark]);
    let image = canvas.into_image();
    let (left, right) = (4.0 + part.left, 4.0 + part.right);
    let (top, bottom) = (5.0 + lines[0].top, 5.0 + lines[0].top + lines[0].height);
    let count = |from: f32, to: f32, colour: [u8; 3]| {
        (from.round() as u32..to.round() as u32)
            .flat_map(|x| (top.round() as u32..bottom.round() as u32).map(move |y| (x, y)))
            .filter(|&(x, y)| {
                let pixel = image.get_pixel(x, y).0;
                pixel[3] > 100 && (0..3).all(|i| pixel[i].abs_diff(colour[i]) <= 3)
            })
            .count()
    };
    // The box is in the background colour, with the text in the
    // foreground colour over it, and none of the text's own colour.
    assert!(count(left, right, [200, 0, 200]) > 100);
    let greenish = (left.round() as u32..right.round() as u32)
        .flat_map(|x| (top.round() as u32..bottom.round() as u32).map(move |y| (x, y)))
        .filter(|&(x, y)| {
            let [red, green, ..] = image.get_pixel(x, y).0;
            green > red.saturating_add(40)
        })
        .count();
    assert!(greenish > 10, "{greenish}");
    assert_eq!(count(left, right, INK), 0);
    // Beside it the text is as it was, with no box.
    assert!(
        count(4.0, left - 2.0, INK) > 5,
        "{}",
        count(4.0, left - 2.0, INK)
    );
    assert_eq!(count(4.0, left - 2.0, [200, 0, 200]), 0);
    assert_eq!(count(right + 2.0, 100.0, [200, 0, 200]), 0);
}

#[test]
fn marks_of_one_colour_and_of_another_in_a_paragraph_are_each_drawn_in_theirs() {
    let paragraph = lay_out(&Fonts::fixture(), &[Run::new("ab cd ef gh")], &LOOK, 400.0);
    let green = Some([0, 200, 0]);
    let marks = [
        TextMark {
            range: 0..2,
            foreground: green,
            background: Some([200, 0, 200]),
        },
        TextMark {
            range: 3..5,
            foreground: Some([250, 250, 0]),
            background: Some([0, 0, 90]),
        },
        TextMark {
            range: 6..8,
            foreground: green,
            background: Some([200, 0, 200]),
        },
    ];
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(160, 40, &mut glyphs);
    canvas.text_marked(&paragraph, 4.0, 5.0, None, &marks);
    let image = canvas.into_image();
    let lines = paragraph.lines();
    let (top, bottom) = (5.0 + lines[0].top, 5.0 + lines[0].top + lines[0].height);
    let within = |range: std::ops::Range<usize>, test: &dyn Fn([u8; 4]) -> bool| {
        let part = paragraph.boxes(range)[0];
        let (left, right) = (
            (4.0 + part.left).round() as u32,
            (4.0 + part.right).round() as u32,
        );
        (left..right)
            .flat_map(|x| (top.round() as u32..bottom.round() as u32).map(move |y| (x, y)))
            .filter(|&(x, y)| test(image.get_pixel(x, y).0))
            .count()
    };
    let greenish = |p: [u8; 4]| p[1] > p[0].saturating_add(40) && p[1] > p[2].saturating_add(40);
    let yellowish = |p: [u8; 4]| p[0] > 150 && p[1] > 150 && p[2] < 100;
    // The first and third in green, the second in yellow, none in the
    // text's own colour.
    assert!(within(0..2, &greenish) > 5 && within(6..8, &greenish) > 5);
    assert!(within(3..5, &yellowish) > 5 && within(3..5, &greenish) == 0);
    assert!(within(0..2, &yellowish) == 0 && within(6..8, &yellowish) == 0);
}

#[test]
fn a_translucent_picture_keeps_its_colour_and_its_alpha() {
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(4, 4, &mut glyphs);
    canvas.picture(&Pixels::new(red(2, 2, 128)), 0.0, 0.0);
    let pixel = canvas.into_image().get_pixel(0, 0).0;
    assert_eq!(pixel[3], 128);
    assert!(pixel[0].abs_diff(200) <= 2, "{pixel:?}");
}

#[test]
fn a_picture_in_a_line_is_drawn_where_the_line_puts_it() {
    let red = Run::picture(
        crate::typeset::reading::text::InlinePicture {
            pixels: Rc::new(Pixels::new(red(10, 10, 255))),
            id: 1,
            depth: 0.0,
        },
        None,
    );
    let paragraph = lay_out(&Fonts::fixture(), &[Run::new("Ab "), red], &LOOK, 400.0);
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(120, 40, &mut glyphs);
    canvas.text(&paragraph, 0.0, 5.0, None);
    let image = canvas.into_image();
    let reds = image
        .pixels()
        .filter(|pixel| pixel.0 == [200, 0, 0, 255])
        .count();
    assert_eq!(reds, 100);
    // After the text, on its baseline: the red square's bottom row is
    // where the glyphs' feet are.
    let (x, y) = (0..120)
        .flat_map(|x| (0..40).map(move |y| (x, y)))
        .find(|&(x, y)| image.get_pixel(x, y).0 == [200, 0, 0, 255])
        .expect("a red pixel");
    assert!(x > 20, "{x}");
    let baseline = paragraph.first_baseline() + 5.0;
    assert_eq!(y + 10, baseline.round() as u32);
}

/// The rows of `image` that have ink, first and last.
fn inked_rows(image: &RgbaImage) -> Option<(u32, u32)> {
    let rows: Vec<u32> = (0..image.height())
        .filter(|&y| (0..image.width()).any(|x| image.get_pixel(x, y).0[3] > 0))
        .collect();
    Some((*rows.first()?, *rows.last()?))
}

#[test]
fn text_reaching_only_a_pixel_or_two_into_the_canvas_still_draws_there() {
    // Lines of half the font size, so that glyphs reach well past
    // their line boxes: a skip that went by the boxes alone, or by too
    // little room around them, would leave these out.
    let look = Look {
        line_height: 0.5,
        ..LOOK
    };
    let paragraph = lay_out(&Fonts::fixture(), &[Run::new("Hxgpq")], &look, 400.0);
    let draw = |height: u32, y: f32| {
        let mut glyphs = GlyphCache::default();
        let mut canvas = Canvas::new(120, height, &mut glyphs);
        canvas.text(&paragraph, 0.0, y, None);
        canvas.into_image()
    };
    // The whole text on a canvas it is well inside of, and where its
    // ink is from the text's own top.
    let whole = draw(300, 100.0);
    let (top, bottom) = inked_rows(&whole).expect("ink");
    let (top, bottom) = (top as f32 - 100.0, bottom as f32 - 100.0);
    // The same text on a canvas of 30 rows, moved so that the ink's
    // last row is the canvas's third, and its first row the
    // second last.
    for (y, row) in [(2.0 - bottom, 2), (28.0 - top, 28)] {
        let small = draw(30, y);
        let (first, last) = inked_rows(&small).expect("ink at the edge");
        assert!(
            first <= row && row <= last,
            "{first}..{last} for ink at row {row}"
        );
        // The same pixels as those rows of the whole text, but for the
        // canvas's first and last rows, where a glyph cut by the edge
        // may antialias a level or two differently.
        let from = (100.0 - y) as u32 + 1;
        let inner = |image: &RgbaImage, from: u32| {
            image::imageops::crop_imm(image, 0, from, 120, 28).to_image()
        };
        let (small, cut) = (inner(&small, 1), inner(&whole, from));
        let far = (small.pixels().zip(cut.pixels()))
            .filter(|(a, b)| a.0.iter().zip(&b.0).any(|(a, b)| a.abs_diff(*b) > 8))
            .count();
        assert_eq!(far, 0, "{far} pixels differ at y = {y}");
    }
    // Far from the canvas, nothing draws.
    assert_eq!(inked_rows(&draw(30, -2000.0)), None);
    assert_eq!(inked_rows(&draw(30, 2000.0)), None);
}

#[test]
fn rectangles_fill_their_pixels() {
    let mut glyphs = GlyphCache::default();
    let mut canvas = Canvas::new(10, 10, &mut glyphs);
    canvas.rect(2.0, 3.0, 4.0, 1.0, INK);
    let image = canvas.into_image();
    assert_eq!(image.get_pixel(2, 3).0, [INK[0], INK[1], INK[2], 255]);
    assert_eq!(image.get_pixel(2, 4).0, [0; 4]);
    assert_eq!(count(&image, INK), 4);
}
