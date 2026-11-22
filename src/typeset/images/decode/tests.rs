use super::*;
use crate::load::tests::{fixture, fixtures, rgba};

fn raster(name: &str) -> RgbaImage {
    match fixture(name) {
        Ok(Source::Raster(image)) => image,
        Ok(other) => panic!("{name} is not a raster image: {other:?}"),
        Err(error) => panic!("{name} does not load: {error:?}"),
    }
}

/// Mean absolute difference per channel, for comparing lossy images.
fn difference(a: &RgbaImage, b: &RgbaImage) -> f64 {
    assert_eq!(a.dimensions(), b.dimensions());
    let total: u64 = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    total as f64 / a.as_raw().len() as f64
}

#[test]
fn the_size_read_from_a_header_is_the_size_of_the_decoded_image() {
    let dir = fixtures().join("images");
    let fonts = Fonts::none();
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap().map(Result::unwrap) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let read = |size| match size {
            true => {
                let file = std::fs::File::open(&path).unwrap();
                read_size_as(file, &path, Format::Detect, DEFAULT_MAX_FILE_SIZE, &fonts).map(Some)
            }
            false => {
                let file = std::fs::File::open(&path).unwrap();
                let image = read_image_as(file, &path, Format::Detect, 99 * MB, &fonts, true);
                image.map(|image| Some((image.size(), image.unit())))
            }
        };
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        match (read(false), read(true)) {
            (Ok(decoded), Ok(measured)) => {
                assert_eq!(measured, decoded, "{name}");
                checked += 1;
            }
            // What cannot be read fails to be measured in the same way.
            (Err(decoded), Err(measured)) => {
                assert_eq!(measured.to_string(), decoded.to_string(), "{name}");
            }
            // A file with a good header and a broken body is measured
            // by its header: the size is not that of the image, which
            // there is none of. `truncated.png` is the one such file.
            (Err(_), Ok(_)) => assert_eq!(name, "truncated.png"),
            (Ok(_), Err(measured)) => panic!("{name} is read but not measured: {measured}"),
        }
    }
    assert!(checked > 40, "{checked} files checked");
}

#[test]
fn a_file_with_a_good_header_and_a_broken_body_has_the_size_its_header_gives() {
    let fonts = Fonts::none();
    let path = fixtures().join("images/pattern.png");
    let good = std::fs::read(&path).unwrap();
    let mut bad = good.clone();
    // Pixel data that fails its checksum.
    let at = bad.len() * 2 / 3;
    bad[at] ^= 0xff;
    let measure =
        |bytes: &[u8]| read_size_as(bytes, &path, Format::Detect, DEFAULT_MAX_FILE_SIZE, &fonts);
    let read = |bytes: &[u8]| {
        read_image_as(
            bytes,
            &path,
            Format::Detect,
            DEFAULT_MAX_FILE_SIZE,
            &fonts,
            true,
        )
    };
    assert!(read(&good).is_ok());
    assert!(read(&bad).is_err());
    assert_eq!(measure(&bad).unwrap(), measure(&good).unwrap());
}

#[test]
fn a_file_that_cannot_be_read_has_no_size_either() {
    let fonts = Fonts::none();
    let no_size =
        |bytes: &[u8]| read_size_as(bytes, Path::new("x"), Format::Detect, MB, &fonts).unwrap_err();
    assert!(matches!(
        no_size(b"not an image"),
        ImageError::UnknownFormat
    ));
    assert!(matches!(
        read_size_as(&[0u8; 20][..], Path::new("x"), Format::Detect, 10, &fonts),
        Err(ImageError::FileTooLarge { .. })
    ));
}

#[test]
fn opaque_rows_are_those_with_alpha_above_8() {
    let with_alpha = |rows: &[(u32, u8)]| {
        let mut image = RgbaImage::new(4, 6);
        for &(y, alpha) in rows {
            image.put_pixel(2, y, image::Rgba([1, 2, 3, alpha]));
        }
        image
    };
    assert_eq!(opaque_rows(&with_alpha(&[(1, 255), (2, 40)])), Some(1..3));
    assert_eq!(
        opaque_rows(&RgbaImage::from_pixel(4, 6, image::Rgba([0, 0, 0, 255]))),
        Some(0..6)
    );
    assert_eq!(opaque_rows(&RgbaImage::new(4, 6)), None);
    assert_eq!(opaque_rows(&with_alpha(&[(3, 8)])), None);
    assert_eq!(opaque_rows(&with_alpha(&[(3, 9)])), Some(3..4));
    assert_eq!(opaque_rows(&with_alpha(&[(0, 255), (5, 255)])), Some(0..6));
}

#[test]
fn decodes_png_with_its_colours_in_place() {
    let image = raster("pattern.png");
    assert_eq!(image.dimensions(), (320, 240));
    assert_eq!(rgba(&image, 20, 20), [255, 0, 0, 255]);
    assert_eq!(rgba(&image, 300, 20), [0, 255, 0, 255]);
    assert_eq!(rgba(&image, 20, 220), [0, 0, 255, 255]);
    assert_eq!(rgba(&image, 300, 220), [255, 255, 0, 255]);
}

#[test]
fn keeps_transparency() {
    let image = raster("transparent.png");
    assert_eq!(rgba(&image, 0, 0)[3], 0);
    assert_eq!(rgba(&image, 100, 100), [255, 0, 0, 255]);
    let half = rgba(&image, 170, 170);
    assert_eq!(half[2], 255);
    assert!((120..=136).contains(&half[3]), "{half:?}");
}

#[test]
fn decodes_png_bit_depths_palettes_and_interlacing() {
    for name in [
        "basn0g16", "basn3p08", "basn6a16", "basi0g08", "basi6a16", "tbbn3p08",
    ] {
        let image = raster(&format!("pngsuite/{name}.png"));
        assert_eq!(image.dimensions(), (32, 32), "{name}");
    }
}

#[test]
fn decodes_the_default_image_of_an_animated_png() {
    assert_eq!(raster("beachball-apng.png").dimensions(), (100, 100));
}

#[test]
fn decodes_jpeg() {
    assert_eq!(raster("photo-landscape.jpg").dimensions(), (1920, 1277));
    assert_eq!(raster("photo-portrait.jpg").dimensions(), (1280, 1918));
}

#[test]
fn decodes_progressive_and_cmyk_jpeg_like_the_baseline() {
    let baseline = raster("photo-landscape.jpg");
    for name in ["photo-progressive.jpg", "photo-cmyk.jpg"] {
        let other = raster(name);
        let difference = difference(&baseline, &other);
        assert!(difference < 4.0, "{name}: {difference}");
    }
}

#[test]
fn turns_jpeg_upright_by_its_exif_orientation() {
    let upright = raster("exif-1.jpg");
    assert_eq!(upright.dimensions(), (600, 399));
    for orientation in 2..=8 {
        let name = format!("exif-{orientation}.jpg");
        let image = raster(&name);
        assert_eq!(image.dimensions(), (600, 399), "{name}");
        let difference = difference(&upright, &image);
        assert!(difference < 4.0, "{name}: {difference}");
    }
}

#[test]
fn turns_png_and_webp_upright_by_their_exif_orientation() {
    let upright = raster("pattern.png");
    for name in ["pattern-exif-6.png", "pattern-exif-6.webp"] {
        // Stored on its side, 240 × 320, with orientation 6.
        assert_eq!(raster(name), upright, "{name}");
    }
}

fn read_animated(name: &str, animated: bool) -> Source {
    let bytes = std::fs::read(fixtures().join("images").join(name)).unwrap();
    decode_named_as(
        &bytes,
        None,
        DEFAULT_MAX_FILE_SIZE,
        &Fonts::none(),
        animated,
    )
    .unwrap()
}

#[test]
fn an_animation_keeps_its_frames_when_asked_to() {
    for name in ["animated.gif", "animated.webp", "beachball-apng.png"] {
        let Source::Animated(animation) = read_animated(name, true) else {
            panic!("{name} keeps its frames");
        };
        assert!(animation.frames.len() > 1, "{name}");
        // Shown as the first frame it was before.
        assert_eq!(
            Source::Animated(animation.clone()).size(),
            animation.first().dimensions()
        );
        assert_eq!(&raster(name), animation.first(), "{name}");
    }
}

#[test]
fn an_animation_over_the_budget_keeps_its_first_frame_and_says_so() {
    let bytes = std::fs::read(fixtures().join("images/animated.gif")).unwrap();
    let named = |budget| {
        decode_named_with(
            &bytes,
            None,
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
            Some(budget),
        )
    };
    let Ok(Source::Animated(whole)) = named(1 << 30) else {
        panic!("within the budget");
    };
    let (width, height) = whole.first().dimensions();
    let frames = 3 * u64::from(width) * u64::from(height) * 4;
    let Ok(Source::Animated(whole)) = named(frames) else {
        panic!("within the budget");
    };
    assert_eq!((whole.frames.len(), whole.over_budget), (3, false));
    let Ok(Source::Animated(first)) = named(frames - 1) else {
        panic!("over the budget");
    };
    assert_eq!(first.frames.len(), 1);
    assert!(first.over_budget);
    assert_eq!(first.first(), whole.first());
}

#[test]
fn an_animation_is_its_first_frame_unless_asked_for_its_frames() {
    for name in ["animated.gif", "animated.webp", "beachball-apng.png"] {
        assert!(
            matches!(read_animated(name, false), Source::Raster(_)),
            "{name}"
        );
    }
    // A still stays one.
    assert!(matches!(
        read_animated("pattern.png", true),
        Source::Raster(_)
    ));
}

#[test]
fn an_animated_image_is_drawn_as_its_first_frame() {
    let source = read_animated("animated.gif", true);
    let still = read_animated("animated.gif", false);
    assert_eq!(source.render(8, 8), still.render(8, 8));
    assert_ne!(source.fingerprint(), still.fingerprint());
}

#[test]
fn decodes_the_first_frame_of_an_animated_gif() {
    let image = raster("animated.gif");
    assert_eq!(image.dimensions(), (64, 64));
    assert_eq!(rgba(&image, 32, 32), [255, 0, 0, 255]);
    assert_eq!(raster("jiggy.gif").dimensions(), (512, 512));
}

#[test]
fn decodes_webp() {
    assert_eq!(raster("lossless.webp").dimensions(), (200, 200));
    assert_eq!(raster("lossy.webp").dimensions(), (300, 300));
    let animated = raster("animated.webp");
    assert_eq!(animated.dimensions(), (64, 64));
    assert_eq!(rgba(&animated, 32, 32), [255, 0, 0, 255]);
}

/// The fixtures in formats only the full build decodes, and the PNG
/// each holds without loss.
const FULL_BUILD_FORMATS: [(&str, &str, &str); 5] = [
    ("pattern.bmp", "BMP", "pattern.png"),
    ("pattern.tif", "TIFF", "pattern.png"),
    ("pattern.qoi", "QOI", "pattern.png"),
    ("pattern.ppm", "PNM", "pattern.png"),
    ("transparent.ico", "ICO", "transparent.png"),
];

#[cfg(feature = "full")]
#[test]
fn decodes_the_full_builds_formats_without_loss() {
    for (name, _, png) in FULL_BUILD_FORMATS {
        assert_eq!(raster(name), raster(png), "{name}");
    }
}

#[cfg(not(feature = "full"))]
#[test]
fn names_the_formats_left_out_of_a_minimal_build() {
    for (name, format, _) in FULL_BUILD_FORMATS {
        let error = ImageError::UnsupportedFormat(format.into());
        assert_eq!(fixture(name), Err(error), "{name}");
    }
}

#[test]
fn scales_transparency_without_a_halo() {
    // Dark blue on the left; on the right, fully transparent pixels
    // whose colour is white, as many image editors save them.
    let image = RgbaImage::from_fn(16, 16, |x, _| {
        if x < 8 {
            image::Rgba([0, 0, 128, 255])
        } else {
            image::Rgba([255, 255, 255, 0])
        }
    });
    let small = Source::Raster(image).render(4, 4);
    for (x, y, pixel) in small.enumerate_pixels() {
        // Allowing for rounding and the filter's slight ringing.
        let [red, green, blue, alpha] = pixel.0;
        if alpha > 0 {
            let close = red <= 2 && green <= 2 && blue.abs_diff(128) <= 2;
            assert!(close, "({x}, {y}) is {pixel:?}");
        }
    }
    // The edge is still there: the transparent side stays transparent.
    assert_eq!(rgba(&small, 0, 0)[3], 255);
    assert_eq!(rgba(&small, 3, 0)[3], 0);
}

#[test]
fn scales_raster_images_like_the_image_crate() {
    let cases = [
        ("pattern.png", 32, 24),
        ("pattern.png", 213, 160),
        ("photo-landscape.jpg", 300, 200),
        ("transparent.png", 64, 64),
        ("transparent.png", 150, 150),
        ("compass-rose.png", 225, 200),
        ("icon.png", 23, 23),
    ];
    for (name, width, height) in cases {
        let image = raster(name);
        let scaled = Source::Raster(image.clone()).render(width, height);
        let expected = scale_with_the_image_crate(&image, width, height);
        assert_eq!(scaled.dimensions(), (width, height), "{name}");
        // Colour weighed by alpha: a faint pixel may change hue without
        // anyone seeing it.
        let shown = |pixel: [u8; 4]| {
            let alpha = u32::from(pixel[3]);
            pixel
                .map(|channel| u32::from(channel) * alpha / 255)
                .map(|value| value as u8)
        };
        let mut differences = Vec::new();
        for (x, y, pixel) in scaled.enumerate_pixels() {
            let (a, b) = (shown(pixel.0), shown(rgba(&expected, x, y)));
            let alpha = pixel[3].abs_diff(rgba(&expected, x, y)[3]);
            differences.extend((0..3).map(|channel| a[channel].abs_diff(b[channel])));
            differences.push(alpha);
        }
        // Ringing where hard edges cross comes out a little different,
        // because fast_image_resize rounds and clamps between its two
        // passes; elsewhere the two agree to rounding.
        let largest = differences.iter().max().unwrap();
        let rounding = differences.iter().filter(|&&d| d <= 2).count();
        assert!(
            *largest <= 16,
            "{name} at {width} × {height}: off by {largest}"
        );
        assert!(
            rounding * 100 >= differences.len() * 99,
            "{name} at {width} × {height}: {} of {} values off by more than 2",
            differences.len() - rounding,
            differences.len(),
        );
    }
}

/// How kiri scaled images with the image crate's resize: in
/// premultiplied alpha with 16 bits per channel, unless the image is
/// opaque.
fn scale_with_the_image_crate(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    use image::imageops::{self, FilterType};
    use image::{ImageBuffer, Rgba};
    const FILTER: FilterType = FilterType::CatmullRom;
    if image.pixels().all(|pixel| pixel[3] == u8::MAX) {
        return imageops::resize(image, width, height, FILTER);
    }
    let premultiplied: ImageBuffer<Rgba<u16>, Vec<u16>> =
        ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
            let [red, green, blue, alpha] = image.get_pixel(x, y).0;
            let times_alpha = |channel: u8| {
                let product = u32::from(channel) * u32::from(alpha);
                ((u64::from(product) * 65535 + 65025 / 2) / 65025) as u16
            };
            Rgba([
                times_alpha(red),
                times_alpha(green),
                times_alpha(blue),
                u16::from(alpha) * 257,
            ])
        });
    let scaled = imageops::resize(&premultiplied, width, height, FILTER);
    RgbaImage::from_fn(width, height, |x, y| {
        let [red, green, blue, alpha] = scaled.get_pixel(x, y).0;
        if alpha == 0 {
            return Rgba([0, 0, 0, 0]);
        }
        let divided = |channel: u16| {
            let value = (u32::from(channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha);
            value.min(255) as u8
        };
        let alpha = ((u32::from(alpha) + 128) / 257) as u8;
        Rgba([divided(red), divided(green), divided(blue), alpha])
    })
}

#[test]
fn reports_a_truncated_png() {
    assert_eq!(fixture("truncated.png"), Err(ImageError::Truncated));
}

#[test]
fn shows_what_there_is_of_a_truncated_jpeg() {
    // Like browsers, the JPEG decoder fills in what is missing.
    assert_eq!(raster("truncated.jpg").dimensions(), (1920, 1277));
}

#[test]
fn refuses_images_with_too_many_pixels() {
    assert_eq!(
        fixture("huge.png"),
        Err(ImageError::TooLarge {
            width: 20000,
            height: 20000
        })
    );
}

#[test]
fn reports_data_that_is_no_image() {
    assert_eq!(fixture("not-an-image.png"), Err(ImageError::UnknownFormat));
}

#[test]
fn reports_a_format_this_build_cannot_decode() {
    let farbfeld = b"farbfeld\0\0\0\x01\0\0\0\x01\0\0\0\0\0\0\0\0";
    let error = decode(farbfeld, DEFAULT_MAX_FILE_SIZE, &Fonts::none()).unwrap_err();
    assert_eq!(error, ImageError::UnsupportedFormat("farbfeld".into()));
    assert_eq!(
        error.to_string(),
        "farbfeld images are not supported in this build"
    );
}

#[test]
fn names_formats_as_people_write_them() {
    let names = [
        ImageFormat::Bmp,
        ImageFormat::Ico,
        ImageFormat::Tiff,
        ImageFormat::Qoi,
        ImageFormat::Pnm,
        ImageFormat::Tga,
        ImageFormat::OpenExr,
        ImageFormat::Farbfeld,
    ]
    .map(format_name);
    assert_eq!(
        names,
        [
            "BMP", "ICO", "TIFF", "QOI", "PNM", "TGA", "OpenEXR", "farbfeld"
        ]
    );
}

#[test]
fn reads_an_image_file_in_the_format_it_is_given() {
    let read = |name: &str, format| {
        let path = fixtures().join("images").join(name);
        let file = std::fs::File::open(&path).unwrap();
        read_image(file, &path, format, DEFAULT_MAX_FILE_SIZE, &Fonts::none())
    };
    let png = Format::Raster(ImageFormat::Png);
    assert!(matches!(read("icon.png", png), Ok(Source::Raster(_))));
    assert!(matches!(
        read("pattern.svg", Format::Svg),
        Ok(Source::Svg(_))
    ));
    // A JPEG is not a PNG, whatever its bytes tell.
    let result = read("photo-landscape.jpg", png).map(|_| ());
    assert!(matches!(result, Err(ImageError::Corrupt(_))), "{result:?}");
}

#[test]
fn reads_the_size_of_a_png_from_its_header() {
    let mut png = Vec::new();
    RgbaImage::new(7, 3)
        .write_to(&mut io::Cursor::new(&mut png), ImageFormat::Png)
        .unwrap();
    assert_eq!(png_size(&png), Some((7, 3)));
    assert_eq!(png_size(&png[..24]), Some((7, 3)));
    assert_eq!(png_size(&png[..23]), None);
    assert_eq!(png_size(b"GIF89a"), None);
    let mut not_ihdr = png.clone();
    not_ihdr[12..16].copy_from_slice(b"IDAT");
    assert_eq!(png_size(&not_ihdr), None);
}
