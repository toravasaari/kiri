use image::{Rgba, RgbaImage};

use super::{Options, Prepared, Rgb, tiles};

/// A small sixel decoder, so that tests check what a terminal would draw
/// and not only the bytes. Colours come back as the percentages the
/// sequence defines them in, scaled to 0..=255 as xterm does.
pub(crate) struct Decoded {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) pixels: Vec<Option<Rgb>>,
    /// The parameters of the DCS: P1, P2, P3.
    params: Vec<u32>,
    /// The raster attributes: Pan, Pad, Ph, Pv.
    raster: Vec<usize>,
}

pub(crate) fn decode(bytes: &[u8]) -> Decoded {
    assert!(bytes.starts_with(b"\x1bP"), "starts with DCS");
    assert!(bytes.ends_with(b"\x1b\\"), "ends with ST");
    let body = &bytes[2..bytes.len() - 2];
    let q = body.iter().position(|b| *b == b'q').expect("final byte q");
    let params: Vec<u32> = std::str::from_utf8(&body[..q])
        .unwrap()
        .split(';')
        .map(|p| p.parse().unwrap())
        .collect();
    let data = &body[q + 1..];
    let mut registers: std::collections::HashMap<usize, Rgb> = Default::default();
    let mut raster = Vec::new();
    let (mut width, mut height) = (0, 0);
    let mut canvas: Vec<Vec<Option<Rgb>>> = Vec::new();
    let (mut x, mut band, mut cur) = (0usize, 0usize, 0usize);
    let number = |d: &[u8], i: &mut usize| -> Option<usize> {
        let s = *i;
        while *i < d.len() && d[*i].is_ascii_digit() {
            *i += 1;
        }
        std::str::from_utf8(&d[s..*i]).unwrap().parse().ok()
    };
    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        i += 1;
        match c {
            b'"' => {
                loop {
                    raster.push(number(data, &mut i).unwrap());
                    if data.get(i) == Some(&b';') {
                        i += 1;
                    } else {
                        break;
                    }
                }
                width = raster[2];
                height = raster[3];
            }
            b'#' => {
                cur = number(data, &mut i).unwrap();
                if data.get(i) == Some(&b';') {
                    i += 1;
                    assert_eq!(number(data, &mut i), Some(2), "RGB colour space");
                    let mut rgb = [0u8; 3];
                    for v in &mut rgb {
                        assert_eq!(data[i], b';');
                        i += 1;
                        let p = number(data, &mut i).unwrap();
                        assert!(p <= 100);
                        *v = ((p * 255 + 50) / 100) as u8;
                    }
                    registers.insert(cur, rgb);
                }
            }
            b'$' => x = 0,
            b'-' => {
                band += 1;
                x = 0;
            }
            b'!' | b'?'..=b'~' => {
                let (n, ch) = if c == b'!' {
                    let n = number(data, &mut i).unwrap();
                    i += 1;
                    (n, data[i - 1])
                } else {
                    (1, c)
                };
                let bits = ch - 63;
                for _ in 0..n {
                    for k in 0..6 {
                        if bits & (1 << k) != 0 {
                            let (px, py) = (x, band * 6 + k);
                            assert!(px < width && py < height, "inside the raster");
                            while canvas.len() <= py {
                                canvas.push(vec![None; width]);
                            }
                            canvas[py][px] =
                                Some(*registers.get(&cur).expect("register defined before use"));
                        }
                    }
                    x += 1;
                }
            }
            other => panic!("unexpected byte {other:?}"),
        }
    }
    let mut pixels = vec![None; width * height];
    for (y, row) in canvas.iter().enumerate() {
        pixels[y * width..(y + 1) * width].copy_from_slice(row);
    }
    Decoded {
        width,
        height,
        pixels,
        params,
        raster,
    }
}

/// What the encoder promises for a colour: its percentage, scaled back.
fn quantised(c: Rgb) -> Rgb {
    c.map(|v| ((((u32::from(v) * 100 + 127) / 255) * 255 + 50) / 100) as u8)
}

fn picture(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| Rgba(f(x, y)))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .replace('\x1b', "ESC ")
}

#[test]
fn bytes_of_a_two_colour_slice() {
    // Red on the top three rows, blue below; a transparent column at the
    // right.
    let img = picture(4, 6, |x, y| {
        if x == 3 {
            [0, 0, 0, 0]
        } else if y < 3 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        }
    });
    let out = Prepared::new(&img, Options::new()).slice(0, 0, 4, 6);
    insta::assert_snapshot!(text(&out), @r#"ESC P0;1;0q"1;1;4;6#0;2;100;0;0#1;2;0;0;100#0FFF$#1wwwESC \"#);
}

#[test]
fn a_slice_round_trips_through_the_decoder() {
    let img = picture(11, 23, |x, y| {
        if (x + y) % 7 == 0 {
            [0, 0, 0, 0]
        } else {
            [(x * 19) as u8, (y * 11) as u8, ((x * y) % 256) as u8, 255]
        }
    });
    let out = Prepared::new(&img, Options::new()).slice(0, 0, 11, 23);
    let d = decode(&out);
    assert_eq!((d.width, d.height), (11, 23));
    for y in 0..23 {
        for x in 0..11 {
            let p = img.get_pixel(x, y).0;
            let want = (p[3] >= 128).then(|| quantised([p[0], p[1], p[2]]));
            assert_eq!(d.pixels[(y * 11 + x) as usize], want, "pixel {x},{y}");
        }
    }
}

#[test]
fn a_slice_is_one_cell_high_with_exact_raster_and_no_trailing_band_mark() {
    let img = picture(30, 46, |x, y| [(x * 8) as u8, (y * 5) as u8, 90, 255]);
    let prepared = Prepared::new(&img, Options::new());
    let out = prepared.slice(0, 23, 30, 23);
    let d = decode(&out);
    assert_eq!(
        d.params,
        [0, 1, 0],
        "P2 = 1: undrawn pixels stay transparent"
    );
    assert_eq!(d.raster, [1, 1, 30, 23]);
    assert!(out.ends_with(b"\x1b\\") && !out.ends_with(b"-\x1b\\"));
    assert_eq!(
        d.pixels.iter().flatten().count(),
        30 * 23,
        "every pixel drawn"
    );
}

#[test]
fn every_slice_defines_the_registers_it_uses_from_zero() {
    let img = picture(8, 12, |_, y| {
        if y < 6 {
            [9, 9, 9, 255]
        } else {
            [200, 1, 1, 255]
        }
    });
    let prepared = Prepared::new(&img, Options::new());
    // The decoder panics on a register used before it is defined.
    for (y, want) in [(0, [9, 9, 9]), (6, [200, 1, 1])] {
        let out = prepared.slice(0, y, 8, 6);
        assert!(text(&out).contains("#0;2;"), "{}", text(&out));
        assert!(!text(&out).contains("#1"), "one colour, one register");
        assert_eq!(decode(&out).pixels[0], Some(quantised(want)));
    }
}

#[test]
fn a_stack_of_slices_draws_what_the_whole_picture_draws() {
    let img = picture(20, 36, |x, y| {
        [
            (x * 12) as u8,
            (y / 12 * 50) as u8,
            ((x ^ (y / 6)) * 9) as u8,
            255,
        ]
    });
    let prepared = Prepared::new(&img, Options::new());
    let whole = decode(&prepared.slice(0, 0, 20, 36));
    let mut stacked = Vec::new();
    for row in 0..3 {
        let d = decode(&prepared.slice(0, row * 12, 20, 12));
        stacked.extend(d.pixels);
    }
    assert_eq!(whole.pixels, stacked);
}

#[test]
fn a_region_is_cut_to_the_picture() {
    let img = picture(5, 5, |_, _| [1, 2, 3, 255]);
    let prepared = Prepared::new(&img, Options::new());
    let d = decode(&prepared.slice(3, 2, 99, 99));
    assert_eq!((d.width, d.height), (2, 3));
    assert!(prepared.slice(5, 0, 1, 1).is_empty());
    assert!(prepared.slice(0, 0, 0, 1).is_empty());
}

#[test]
fn runs_are_run_length_coded_from_four() {
    let img = picture(10, 6, |_, _| [255, 255, 255, 255]);
    let out = Prepared::new(&img, Options::new()).slice(0, 0, 10, 6);
    assert!(text(&out).contains("#0!10~"), "{}", text(&out));
    let img = picture(3, 6, |_, _| [255, 255, 255, 255]);
    let out = Prepared::new(&img, Options::new()).slice(0, 0, 3, 6);
    assert!(text(&out).contains("#0~~~"), "{}", text(&out));
}

#[test]
fn a_colour_is_written_from_its_first_to_its_last_column() {
    // Red only in columns 2 and 3 of 8, white elsewhere.
    let img = picture(8, 6, |x, _| {
        if (2..4).contains(&x) {
            [255, 0, 0, 255]
        } else {
            [255, 255, 255, 255]
        }
    });
    let out = text(&Prepared::new(&img, Options::new()).slice(0, 0, 8, 6));
    assert!(out.contains("$#1??~~"), "{out}");
}

#[test]
fn transparent_pixels_are_undrawn_without_a_background() {
    let img = picture(4, 6, |x, _| {
        if x < 2 {
            [10, 20, 30, 0]
        } else {
            [10, 20, 30, 255]
        }
    });
    let d = decode(&Prepared::new(&img, Options::new()).slice(0, 0, 4, 6));
    assert_eq!(d.pixels[0], None);
    assert!(d.pixels[3].is_some());
}

#[test]
fn a_background_is_flattened_into_the_picture() {
    let img = picture(4, 6, |x, _| match x {
        0 => [200, 0, 0, 0],
        1 => [255, 255, 255, 255],
        _ => [255, 255, 255, 128],
    });
    let options = Options {
        background: Some([0, 0, 0]),
        ..Options::new()
    };
    let d = decode(&Prepared::new(&img, options).slice(0, 0, 4, 6));
    assert_eq!(
        d.pixels[0],
        Some([0, 0, 0]),
        "fully transparent: the background"
    );
    assert_eq!(d.pixels[1], Some(quantised([255, 255, 255])));
    assert_eq!(
        d.pixels[2],
        Some(quantised([128, 128, 128])),
        "half over black"
    );
    assert_eq!(d.pixels.iter().flatten().count(), 24, "all drawn");
}

#[test]
fn more_colours_than_registers_are_reduced_to_a_palette() {
    let img = picture(64, 24, |x, y| {
        [(x * 4) as u8, (y * 10) as u8, ((x + y) * 3) as u8, 255]
    });
    let options = Options {
        registers: 16,
        ..Options::new()
    };
    let prepared = Prepared::new(&img, options);
    let out = prepared.slice(0, 0, 64, 12);
    let defined = text(&out).matches(";2;").count();
    assert!((1..=16).contains(&defined), "{defined} registers");
    let d = decode(&out);
    assert_eq!(d.pixels.iter().flatten().count(), 64 * 12);
    // Close to the original on average.
    let mut total = 0u32;
    for y in 0..12 {
        for x in 0..64 {
            let got = d.pixels[y * 64 + x].unwrap();
            let want = img.get_pixel(x as u32, y as u32).0;
            total += (0..3)
                .map(|k| u32::from(got[k].abs_diff(want[k])))
                .sum::<u32>();
        }
    }
    assert!(
        total / (64 * 12 * 3) < 40,
        "mean error {}",
        total / (64 * 12 * 3)
    );
}

#[test]
fn a_slice_whose_colours_fit_is_exact_inside_a_picture_that_does_not() {
    // The top rows are two flat colours; the bottom holds a gradient.
    let img = picture(40, 24, |x, y| {
        if y < 12 {
            [30, 60, 90, 255]
        } else {
            [(x * 6) as u8, (y * 10) as u8, (x * y) as u8, 255]
        }
    });
    let options = Options {
        registers: 8,
        ..Options::new()
    };
    let d = decode(&Prepared::new(&img, options).slice(0, 0, 40, 12));
    assert!(d.pixels.iter().all(|p| *p == Some(quantised([30, 60, 90]))));
}

#[test]
fn single_pixel_wide_or_high_slices_are_reduced_without_panicking() {
    let img = picture(50, 50, |x, y| {
        [(x * 5) as u8, (y * 5) as u8, ((x + y) * 2) as u8, 255]
    });
    let options = Options {
        registers: 4,
        ..Options::new()
    };
    let prepared = Prepared::new(&img, options);
    assert_eq!(decode(&prepared.slice(7, 0, 1, 40)).pixels.len(), 40);
    assert_eq!(decode(&prepared.slice(0, 7, 40, 1)).pixels.len(), 40);
}

#[test]
fn tiles_are_cut_at_cell_boundaries() {
    assert_eq!(tiles(100, 10, 1000), [(0, 100)]);
    assert_eq!(tiles(2500, 12, 1000), [(0, 996), (996, 996), (1992, 508)]);
    assert_eq!(
        tiles(30, 10, 5),
        [(0, 10), (10, 10), (20, 10)],
        "at least a cell"
    );
    assert!(tiles(0, 10, 1000).is_empty());
}

#[test]
fn tiles_side_by_side_draw_what_the_whole_row_draws() {
    let img = picture(60, 12, |x, y| [(x * 4) as u8, (y / 6 * 90) as u8, 7, 255]);
    let prepared = Prepared::new(&img, Options::new());
    let whole = decode(&prepared.slice(0, 0, 60, 12));
    let mut joined = vec![None; 60 * 12];
    for (x0, w) in tiles(60, 10, 25) {
        let d = decode(&prepared.slice(x0, 0, w, 12));
        for y in 0..12 {
            for x in 0..w as usize {
                joined[y * 60 + x0 as usize + x] = d.pixels[y * w as usize + x];
            }
        }
    }
    assert_eq!(whole.pixels, joined);
}
