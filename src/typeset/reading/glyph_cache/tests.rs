use std::cell::Cell;

use resvg::tiny_skia::{Color, FillRule, Mask, Paint, PathBuilder, Pixmap, Rect, Transform};

use super::*;

const INK: [u8; 3] = [0x20, 0xc0, 0x40];

fn face() -> Face {
    Face::new(1, 0, 16.0, 0.0, [])
}

/// A triangle 8 pixels wide and 10 high, its corner at the offset it is
/// made at.
fn glyph((dx, dy): (f32, f32)) -> Glyph {
    let mut path = PathBuilder::new();
    path.move_to(dx, dy);
    path.line_to(dx + 8.0, dy);
    path.line_to(dx, dy + 10.0);
    path.close();
    Glyph::new(false, path.finish())
}

/// What `glyph` fills when it is filled as a path, with its origin at
/// (`x`, `y`).
fn filled((x, y): (f32, f32), size: (u32, u32)) -> Pixmap {
    let mut pixmap = Pixmap::new(size.0, size.1).unwrap();
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(INK[0], INK[1], INK[2], 255));
    paint.anti_alias = true;
    let mut path = PathBuilder::new();
    path.move_to(x, y);
    path.line_to(x + 8.0, y);
    path.line_to(x, y + 10.0);
    path.close();
    pixmap.fill_path(
        &path.finish().unwrap(),
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    pixmap
}

fn drawn_at(cache: &mut GlyphCache, target: &mut Pixmap, (x, y): (f32, f32), id: u32) {
    let colour = |_: &mut Pixmap| panic!("not a colour glyph");
    cache.draw(
        target,
        (face(), id),
        Origin::of(x, y),
        (INK, None),
        glyph,
        colour,
    );
}

#[test]
fn a_place_is_rounded_to_a_sixteenth_of_a_pixel() {
    let steps = |x, y| {
        let Origin { whole, steps } = Origin::of(x, y);
        (whole, steps)
    };
    assert_eq!(steps(3.0, 4.0), ((3, 4), (0, 0)));
    assert_eq!(steps(3.5, 4.25), ((3, 4), (8, 4)));
    assert_eq!(steps(3.03, 4.04), ((3, 4), (0, 1)));
    // A fraction that rounds up to the next pixel is that pixel's corner.
    assert_eq!(steps(3.99, 4.97), ((4, 5), (0, 0)));
    // Left of and above the canvas.
    assert_eq!(steps(-0.5, -1.25), ((-1, -2), (8, 12)));
    assert_eq!(steps(-0.01, -2.0), ((0, -2), (0, 0)));
}

#[test]
fn a_glyph_drawn_twice_is_made_once() {
    let mut cache = GlyphCache::default();
    let made = Cell::new(0);
    let make = |offset| {
        made.set(made.get() + 1);
        glyph(offset)
    };
    let colour = |_: &mut Pixmap| panic!("not a colour glyph");
    let mut target = Pixmap::new(30, 30).unwrap();
    for at in [(4.0, 5.0), (14.0, 5.0), (4.01, 5.02)] {
        let origin = Origin::of(at.0, at.1);
        cache.draw(&mut target, (face(), 7), origin, (INK, None), make, colour);
    }
    // The first two are the same glyph a few pixels apart; so is the third,
    // a hundredth of a pixel from the first.
    assert_eq!(made.get(), 1);
    assert_eq!(cache.counts(), (2, 1));
    // Another glyph, another size, another slant, another font and another
    // place each make a glyph of their own.
    let others = [
        ((face(), 8), (4.0, 5.0)),
        ((Face::new(1, 0, 17.0, 0.0, []), 7), (4.0, 5.0)),
        ((Face::new(1, 0, 16.0, 0.2, []), 7), (4.0, 5.0)),
        ((Face::new(1, 0, 16.0, 0.0, [100]), 7), (4.0, 5.0)),
        ((Face::new(2, 0, 16.0, 0.0, []), 7), (4.0, 5.0)),
        ((Face::new(1, 1, 16.0, 0.0, []), 7), (4.0, 5.0)),
        ((face(), 7), (4.5, 5.0)),
        ((face(), 7), (4.0, 5.5)),
    ];
    for (which, at) in others {
        let origin = Origin::of(at.0, at.1);
        cache.draw(&mut target, which, origin, (INK, None), make, colour);
    }
    assert_eq!(made.get(), 1 + others.len());
}

#[test]
fn a_glyph_held_is_laid_down_as_the_one_made() {
    let mut cache = GlyphCache::default();
    let (mut first, mut second) = (Pixmap::new(20, 20).unwrap(), Pixmap::new(20, 20).unwrap());
    drawn_at(&mut cache, &mut first, (3.3, 4.7), 1);
    drawn_at(&mut cache, &mut second, (3.3, 4.7), 1);
    assert_eq!(cache.counts(), (1, 1));
    assert_eq!(first.data(), second.data());
    assert!(first.data().iter().any(|&byte| byte != 0));
}

#[test]
fn the_cache_stays_within_its_budget() {
    // A mask of this glyph is 9 by 11 pixels; a budget for five of them.
    let one = glyph((0.0, 0.0)).bytes();
    assert!(one > 99);
    let budget = 5 * one;
    let mut cache = GlyphCache::with_budget(budget);
    let mut target = Pixmap::new(20, 20).unwrap();
    let mut most = 0;
    for id in 0..40 {
        drawn_at(&mut cache, &mut target, (3.0, 4.0), id);
        most = most.max(cache.bytes());
        assert!(cache.bytes() <= budget, "{} bytes", cache.bytes());
    }
    // It filled up, and began again, more than once.
    assert!(most > 3 * one, "{most} bytes held at most");
    assert_eq!(cache.counts(), (0, 40));
    // What is drawn after a clearing is what was drawn before it.
    let mut after = Pixmap::new(20, 20).unwrap();
    drawn_at(&mut cache, &mut after, (3.0, 4.0), 39);
    let mut again = Pixmap::new(20, 20).unwrap();
    drawn_at(&mut GlyphCache::default(), &mut again, (3.0, 4.0), 39);
    assert_eq!(after.data(), again.data());
}

#[test]
fn a_glyph_over_the_budget_is_drawn_and_not_held() {
    let mut cache = GlyphCache::with_budget(10);
    let mut target = Pixmap::new(20, 20).unwrap();
    drawn_at(&mut cache, &mut target, (3.0, 4.0), 1);
    drawn_at(&mut cache, &mut target, (3.0, 4.0), 1);
    assert_eq!((cache.bytes(), cache.counts()), (0, (0, 2)));
    assert!(target.data().iter().any(|&byte| byte != 0));
}

#[test]
fn a_glyph_is_within_a_little_of_the_same_path_filled() {
    for at in [
        (3.0, 4.0),
        (3.5, 4.5),
        (3.3, 4.7),
        (3.97, 4.03),
        (5.06, 6.5),
    ] {
        let mut target = Pixmap::new(24, 24).unwrap();
        drawn_at(&mut GlyphCache::default(), &mut target, at, 1);
        let exact = filled(at, (24, 24));
        let worst = (target.data().iter().zip(exact.data()))
            .map(|(&a, &b)| a.abs_diff(b))
            .max()
            .unwrap();
        assert!(worst <= 64, "{worst} of 255 at {at:?}");
    }
}

#[test]
fn a_glyph_laid_on_a_pixel_takes_what_the_pixel_keeps() {
    // A pixel of the page's colour, and one half transparent.
    let mut target = Pixmap::new(12, 12).unwrap();
    target.fill(Color::from_rgba8(200, 100, 50, 255));
    let mut other = Pixmap::new(12, 12).unwrap();
    other.fill(Color::from_rgba8(200, 100, 50, 128));
    for target in [&mut target, &mut other] {
        // A half covered row: the glyph is a rectangle through the middle.
        let mut path = PathBuilder::new();
        path.push_rect(Rect::from_xywh(2.0, 2.0, 6.0, 4.5).unwrap());
        let glyph = Glyph::new(false, path.finish());
        let mut cache = GlyphCache::default();
        let colour = |_: &mut Pixmap| panic!("not a colour glyph");
        let origin = Origin::of(0.0, 0.0);
        cache.draw(target, (face(), 1), origin, (INK, None), |_| glyph, colour);
    }
    let pixel = |pixmap: &Pixmap, x, y| pixmap.pixel(x, y).unwrap();
    // Inside: the ink, opaque. Outside: what was there.
    for pixmap in [&target, &other] {
        assert_eq!(pixel(pixmap, 4, 3).red(), INK[0]);
        assert_eq!(pixel(pixmap, 4, 3).alpha(), 255);
    }
    assert_eq!(pixel(&target, 10, 10).red(), 200);
    assert_eq!(pixel(&other, 10, 10).alpha(), 128);
    // The half covered row over the page colour is the two mixed: tiny-skia
    // fills it so too.
    let mut by_fill = Pixmap::new(12, 12).unwrap();
    by_fill.fill(Color::from_rgba8(200, 100, 50, 255));
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(INK[0], INK[1], INK[2], 255));
    by_fill.fill_rect(
        Rect::from_xywh(2.0, 2.0, 6.0, 4.5).unwrap(),
        &paint,
        Transform::identity(),
        None,
    );
    let worst = (target.data().iter().zip(by_fill.data()))
        .map(|(&a, &b)| a.abs_diff(b))
        .max()
        .unwrap();
    assert!(worst <= 1, "{worst} of 255");
}

#[test]
fn a_clip_keeps_a_glyph_to_its_own() {
    let mut clip = Mask::new(20, 20).unwrap();
    clip.fill_path(
        &PathBuilder::from_rect(Rect::from_xywh(0.0, 0.0, 6.0, 20.0).unwrap()),
        FillRule::Winding,
        false,
        Transform::identity(),
    );
    let mut cache = GlyphCache::default();
    let mut target = Pixmap::new(20, 20).unwrap();
    let colour = |_: &mut Pixmap| panic!("not a colour glyph");
    let origin = Origin::of(2.0, 3.0);
    cache.draw(
        &mut target,
        (face(), 1),
        origin,
        (INK, Some(&clip)),
        glyph,
        colour,
    );
    let ink: Vec<u32> = (0..20)
        .filter(|&x| (0..20).any(|y| target.pixel(x, y).unwrap().alpha() > 0))
        .collect();
    assert_eq!((ink.first(), ink.last()), (Some(&2), Some(&5)));
}

#[test]
fn a_glyph_at_the_edges_of_the_canvas_is_cut_there() {
    let mut cache = GlyphCache::default();
    for at in [
        (-5.0, 3.0),
        (16.0, 3.0),
        (3.0, -6.0),
        (3.0, 15.0),
        (-40.0, -40.0),
    ] {
        let mut target = Pixmap::new(20, 20).unwrap();
        drawn_at(&mut cache, &mut target, at, 1);
    }
    let mut target = Pixmap::new(20, 20).unwrap();
    drawn_at(&mut cache, &mut target, (-5.0, 3.0), 1);
    // The right part of the triangle's top shows; its left is off the canvas.
    assert_eq!(target.pixel(0, 3).unwrap().alpha(), 255);
}

/// The same glyph drawn through `colour`, which says whether it drew it.
fn colour_glyph(drew: bool) -> (Pixmap, Cell<usize>) {
    let asked = Cell::new(0);
    let mut target = Pixmap::new(20, 20).unwrap();
    let make = |offset: (f32, f32)| {
        let mut path = PathBuilder::new();
        path.push_rect(Rect::from_xywh(offset.0 + 2.0, offset.1 + 2.0, 6.0, 6.0).unwrap());
        Glyph::new(true, path.finish())
    };
    let colour = |_: &mut Pixmap| {
        asked.set(asked.get() + 1);
        drew
    };
    let origin = Origin::of(0.0, 0.0);
    GlyphCache::default().draw(&mut target, (face(), 1), origin, (INK, None), make, colour);
    (target, asked)
}

#[test]
fn a_colour_glyph_is_left_to_the_colour_painter_when_it_draws_it() {
    let (target, asked) = colour_glyph(true);
    assert_eq!(asked.get(), 1);
    assert!(target.data().iter().all(|&byte| byte == 0));
}

#[test]
fn a_colour_glyph_that_fails_falls_back_to_its_outline() {
    let (target, asked) = colour_glyph(false);
    assert_eq!(asked.get(), 1);
    assert_eq!(target.pixel(4, 4).unwrap().alpha(), 255);
    assert_eq!(target.pixel(4, 4).unwrap().red(), INK[0]);
    assert_eq!(target.pixel(12, 12).unwrap().alpha(), 0);
}

#[test]
fn a_monochrome_glyph_never_asks_the_colour_painter() {
    // `drawn_at` panics if it is asked.
    let mut target = Pixmap::new(20, 20).unwrap();
    drawn_at(&mut GlyphCache::default(), &mut target, (3.0, 4.0), 1);
}
