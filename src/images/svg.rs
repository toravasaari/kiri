//! SVG images: parsing, drawing at the size they are shown, and the fonts
//! for their text.

use std::cell::OnceCell;
use std::collections::HashSet;
use std::fmt;
use std::io;
use std::sync::Arc;

use image::RgbaImage;
use resvg::tiny_skia;
use resvg::usvg::{self, fontdb};

use crate::images::ImageError;
use crate::typeset::images::url::read_at_most;

/// A parsed SVG document.
#[derive(Clone)]
pub struct Svg {
    tree: Arc<usvg::Tree>,
    /// The file it was parsed from, which tells two SVGs apart.
    bytes: Arc<[u8]>,
    /// The part of the drawing that is shown, in its CSS pixels: from its
    /// top left corner to its size, unless it was grown.
    view: tiny_skia::Rect,
    /// The colour of a panel under the whole image, if it lies on one.
    panel: Option<[u8; 3]>,
    /// The natural size in whole CSS pixels.
    pub(crate) size: (u32, u32),
}

impl Svg {
    fn new(tree: usvg::Tree, bytes: &[u8]) -> Self {
        let size = tree.size();
        let view = tiny_skia::Rect::from_xywh(0.0, 0.0, size.width(), size.height())
            .expect("an SVG's size is positive");
        Self {
            tree: Arc::new(tree),
            bytes: bytes.into(),
            view,
            panel: None,
            size: (0, 0),
        }
        .showing(view)
    }

    /// This SVG showing `view` of its drawing.
    fn showing(self, view: tiny_skia::Rect) -> Self {
        let natural = |length: f32| (length.round() as u32).max(1);
        Self {
            view,
            size: (natural(view.width()), natural(view.height())),
            ..self
        }
    }

    /// This SVG grown to show all of its text, also where the text reaches
    /// past its edges. A renderer that measures text with metrics of its
    /// own may place it there, when the fonts it is drawn with are wider.
    pub(crate) fn grown_to_text(self) -> Self {
        let mut view = self.view;
        each_text_bounds(self.tree.root(), &mut |bounds| {
            view = tiny_skia::Rect::from_ltrb(
                view.left().min(bounds.left()),
                view.top().min(bounds.top()),
                view.right().max(bounds.right()),
                view.bottom().max(bounds.bottom()),
            )
            .unwrap_or(view);
        });
        self.showing(view)
    }

    /// The left and right edges of all of its text on the canvas, in CSS
    /// pixels; `None` when it draws no text, as when no font has it.
    pub(crate) fn text_edges(&self) -> Option<(f32, f32)> {
        let mut edges: Option<(f32, f32)> = None;
        each_text_bounds(self.tree.root(), &mut |bounds| {
            let (left, right) = edges.unwrap_or((bounds.left(), bounds.right()));
            edges = Some((left.min(bounds.left()), right.max(bounds.right())));
        });
        edges
    }

    /// This SVG on a panel of `colour`, `padding` CSS pixels wider than
    /// what it shows on each side.
    pub(crate) fn on_panel(self, colour: [u8; 3], padding: f32) -> Self {
        let view = self.view;
        let view = tiny_skia::Rect::from_ltrb(
            view.left() - padding,
            view.top() - padding,
            view.right() + padding,
            view.bottom() + padding,
        )
        .unwrap_or(view);
        Self {
            panel: Some(colour),
            ..self.showing(view)
        }
    }

    /// Feeds what the image is, to a hash.
    pub(crate) fn fingerprint(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.bytes.hash(hasher);
        let view = self.view;
        for edge in [view.left(), view.top(), view.right(), view.bottom()] {
            edge.to_bits().hash(hasher);
        }
        self.panel.hash(hasher);
    }

    pub(crate) fn render(&self, width: u32, height: u32) -> RgbaImage {
        let mut image = RgbaImage::new(width, height);
        let Some(mut pixmap) = tiny_skia::Pixmap::new(width, height) else {
            // Too large for the renderer; left transparent.
            return image;
        };
        if let Some([red, green, blue]) = self.panel {
            pixmap.fill(tiny_skia::Color::from_rgba8(red, green, blue, 255));
        }
        let view = self.view;
        let transform = tiny_skia::Transform::from_scale(
            width as f32 / view.width(),
            height as f32 / view.height(),
        )
        .pre_translate(-view.left(), -view.top());
        resvg::render(&self.tree, transform, &mut pixmap.as_mut());
        // The renderer works in premultiplied alpha, the terminal does not.
        for (pixel, drawn) in image.pixels_mut().zip(pixmap.pixels()) {
            let colour = drawn.demultiply();
            pixel.0 = [colour.red(), colour.green(), colour.blue(), colour.alpha()];
        }
        image
    }
}

impl PartialEq for Svg {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes && self.view == other.view && self.panel == other.panel
    }
}

/// Calls `found` with the bounds of each text in `group` and below, on the
/// canvas.
fn each_text_bounds(group: &usvg::Group, found: &mut impl FnMut(tiny_skia::Rect)) {
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => each_text_bounds(group, found),
            usvg::Node::Text(text) => found(text.abs_stroke_bounding_box()),
            usvg::Node::Path(_) | usvg::Node::Image(_) => {}
        }
    }
}

impl fmt::Debug for Svg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Svg").field("size", &self.size).finish()
    }
}

/// The fonts that text in SVG images is drawn with. They are loaded when
/// the first SVG needs them, so a document without SVGs never reads the
/// system's fonts.
pub struct Fonts {
    load: fn() -> fontdb::Database,
    database: OnceCell<Arc<fontdb::Database>>,
}

impl Fonts {
    /// The fonts installed on the system.
    pub fn system() -> Self {
        Self::new(|| {
            let mut database = fontdb::Database::new();
            database.load_system_fonts();
            choose_generic_families(&mut database);
            database
        })
    }

    /// No fonts at all: text in SVGs is left out.
    pub fn none() -> Self {
        Self::new(fontdb::Database::new)
    }

    fn new(load: fn() -> fontdb::Database) -> Self {
        Self {
            load,
            database: OnceCell::new(),
        }
    }

    /// DejaVu Sans and its oblique face from the test fixtures and nothing
    /// else, so text in SVGs looks the same on every machine.
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self::new(|| fixture_database(&["DejaVuSans.ttf", "DejaVuSans-Oblique.ttf"]))
    }

    /// The fixture fonts that reading mode's tests lay text out in: DejaVu
    /// Sans in its regular, bold and oblique faces, Noto Sans Mono, and a
    /// few of Noto Color Emoji's emoji.
    #[cfg(test)]
    pub(crate) fn reading_fixture() -> Self {
        Self::new(|| {
            fixture_database(&[
                "DejaVuSans.ttf",
                "DejaVuSans-Bold.ttf",
                "DejaVuSans-Oblique.ttf",
                "NotoSansMono.ttf",
                "NotoColorEmoji-subset.ttf",
            ])
        })
    }

    /// Whether an SVG has needed the fonts yet.
    pub fn loaded(&self) -> bool {
        self.database.get().is_some()
    }

    /// The fonts, loaded now if they are not yet.
    pub(crate) fn database(&self) -> Arc<fontdb::Database> {
        Arc::clone(self.database.get_or_init(|| Arc::new((self.load)())))
    }
}

/// A database of the fixture fonts `names` and nothing else.
#[cfg(test)]
fn fixture_database(names: &[&str]) -> fontdb::Database {
    let mut database = fontdb::Database::new();
    let fonts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fonts");
    for name in names {
        database
            .load_font_file(fonts.join(name))
            .expect("the fixture font loads");
    }
    choose_generic_families(&mut database);
    database
}

/// Installed fonts for the generic families, in fontconfig's order of
/// preference and then the usual ones on macOS and Windows.
const SERIF: &[&str] = &[
    "Noto Serif",
    "DejaVu Serif",
    "Liberation Serif",
    "Times New Roman",
    "Nimbus Roman",
    "Times",
];

const SANS_SERIF: &[&str] = &[
    "Noto Sans",
    "DejaVu Sans",
    "Liberation Sans",
    "Verdana",
    "Arial",
    "Nimbus Sans",
    "Helvetica",
];

const MONOSPACE: &[&str] = &[
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Courier New",
    "Nimbus Mono PS",
    "Menlo",
    "Courier",
];

/// Installed fonts for emoji, in the order they are tried.
pub const EMOJI: &[&str] = &[
    "Noto Color Emoji",
    "Apple Color Emoji",
    "Segoe UI Emoji",
    "Twemoji",
    "JoyPixels",
    "Noto Emoji",
];

/// Fonts for the text of each script that the sans-serif font may lack,
/// by its ISO 15924 code, in the order they are tried. A script that is
/// not here falls back to the sans-serif font alone.
pub const SCRIPTS: &[(&str, &[&str])] = &[
    ("Latn", COMMON),
    ("Grek", COMMON),
    ("Cyrl", COMMON),
    // Common characters, such as symbols and punctuation.
    ("Zyyy", SYMBOLS),
    ("Hani", CJK),
    ("Hira", CJK),
    ("Kana", CJK),
    ("Hang", KOREAN),
    (
        "Arab",
        &[
            "Noto Sans Arabic",
            "Noto Naskh Arabic",
            "Droid Arabic Kufi",
            "DejaVu Sans",
        ],
    ),
    (
        "Hebr",
        &["Noto Sans Hebrew", "Droid Sans Hebrew", "DejaVu Sans"],
    ),
    (
        "Deva",
        &[
            "Noto Sans Devanagari",
            "Droid Sans Devanagari",
            "Lohit Devanagari",
        ],
    ),
    ("Beng", &["Noto Sans Bengali", "Lohit Bengali"]),
    ("Guru", &["Noto Sans Gurmukhi", "Lohit Gurmukhi"]),
    ("Gujr", &["Noto Sans Gujarati", "Lohit Gujarati"]),
    ("Taml", &["Noto Sans Tamil", "Lohit Tamil"]),
    ("Telu", &["Noto Sans Telugu", "Lohit Telugu"]),
    ("Knda", &["Noto Sans Kannada", "Lohit Kannada"]),
    ("Mlym", &["Noto Sans Malayalam", "Lohit Malayalam"]),
    ("Sinh", &["Noto Sans Sinhala"]),
    (
        "Thai",
        &["Noto Sans Thai", "Droid Sans Thai", "Loma", "Tlwg Typo"],
    ),
    ("Laoo", &["Noto Sans Lao"]),
    ("Khmr", &["Noto Sans Khmer"]),
    ("Mymr", &["Noto Sans Myanmar"]),
    ("Tibt", &["Noto Serif Tibetan"]),
    ("Geor", &["Noto Sans Georgian", "DejaVu Sans"]),
    ("Armn", &["Noto Sans Armenian", "DejaVu Sans"]),
    ("Ethi", &["Noto Sans Ethiopic"]),
];

const COMMON: &[&str] = &["Noto Sans", "DejaVu Sans", "Liberation Sans", "Arial"];

const SYMBOLS: &[&str] = &[
    "Noto Sans Symbols",
    "Noto Sans Symbols 2",
    "Noto Sans Math",
    "DejaVu Sans",
    "Symbola",
];

const CJK: &[&str] = &[
    "Noto Sans CJK JP",
    "Noto Sans CJK SC",
    "Noto Sans CJK TC",
    "Source Han Sans",
    "Droid Sans Fallback",
    "WenQuanYi Zen Hei",
];

const KOREAN: &[&str] = &[
    "Noto Sans CJK KR",
    "Source Han Sans K",
    "NanumGothic",
    "Droid Sans Fallback",
];

/// Of `candidates`, the fonts that are installed, in their order.
pub fn installed<'a>(candidates: &[&'a str], installed: &HashSet<&str>) -> Vec<&'a str> {
    (candidates.iter().copied())
        .filter(|name| installed.contains(name))
        .collect()
}

/// Makes `serif`, `sans-serif` and `monospace` name installed fonts. The
/// database takes them from fontconfig's aliases, which may name a font
/// that is not installed, and then SVG text would vanish; a browser always
/// finds one.
fn choose_generic_families(database: &mut fontdb::Database) {
    let installed: HashSet<&str> = (database.faces())
        .flat_map(|face| face.families.iter().map(|(name, _)| name.as_str()))
        .collect();
    let choose = |family, candidates| {
        generic_family(database.family_name(&family), candidates, &installed).map(str::to_owned)
    };
    // Text in another generic family falls back to serif, so serif takes
    // any font at all when none of the usual ones is there.
    let any = installed.iter().min().map(|name| (*name).to_owned());
    let serif = choose(fontdb::Family::Serif, SERIF).or(any);
    let sans_serif = choose(fontdb::Family::SansSerif, SANS_SERIF);
    let monospace = choose(fontdb::Family::Monospace, MONOSPACE);
    if let Some(name) = serif {
        database.set_serif_family(name);
    }
    if let Some(name) = sans_serif {
        database.set_sans_serif_family(name);
    }
    if let Some(name) = monospace {
        database.set_monospace_family(name);
    }
}

/// `configured` when it is installed, otherwise the first installed of
/// `candidates`.
fn generic_family<'a>(
    configured: &'a str,
    candidates: &[&'a str],
    installed: &HashSet<&str>,
) -> Option<&'a str> {
    std::iter::once(configured)
        .chain(candidates.iter().copied())
        .find(|name| installed.contains(name))
}

impl fmt::Debug for Fonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fonts")
            .field("loaded", &self.loaded())
            .finish()
    }
}

/// The magic number of gzip, which compresses `.svgz` files.
const GZIP: &[u8] = &[0x1f, 0x8b];

/// SVG has no magic number: it is XML with an `<svg` element, or that
/// compressed with gzip.
pub(crate) fn looks_like_svg(bytes: &[u8]) -> bool {
    let text = bytes.strip_prefix("\u{feff}".as_bytes()).unwrap_or(bytes);
    let text = text.trim_ascii_start();
    bytes.starts_with(GZIP)
        || (text.starts_with(b"<") && text.windows(4).any(|window| window == b"<svg"))
}

pub(crate) fn decode_svg(bytes: &[u8], max_size: u64, fonts: &Fonts) -> Result<Svg, ImageError> {
    // usvg would decompress without a limit: a few kilobytes of gzip can
    // grow to gigabytes.
    let plain;
    let text = if bytes.starts_with(GZIP) {
        plain = gunzip(bytes, max_size)?;
        &plain[..]
    } else {
        bytes
    };
    let text = std::str::from_utf8(text)
        .map_err(|_| ImageError::Corrupt(usvg::Error::NotAnUtf8Str.to_string()))?;
    let embedded = usvg::ImageHrefResolver::default_data_resolver();
    let options = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            // Images embedded in data: URLs show, but not compressed SVGs,
            // which nothing would limit; a browser does not show them
            // either.
            resolve_data: Box::new(move |mime, data, options| {
                (!data.starts_with(GZIP))
                    .then(|| embedded(mime, data, options))
                    .flatten()
            }),
            // Like an image in a browser, an SVG loads no files of its own.
            resolve_string: Box::new(|_, _| None),
        },
        fontdb: fonts.database(),
        ..usvg::Options::default()
    };
    // From text, so usvg never decompresses anything itself.
    let tree = usvg::Tree::from_str(text, &options)
        .map_err(|error| ImageError::Corrupt(error.to_string()))?;
    Ok(Svg::new(tree, bytes))
}

/// `bytes` decompressed with gzip, if that gives at most `limit` bytes.
fn gunzip(bytes: &[u8], limit: u64) -> Result<Vec<u8>, ImageError> {
    match read_at_most(flate2::read::GzDecoder::new(bytes), limit) {
        Ok(Some(plain)) => Ok(plain),
        Ok(None) => Err(ImageError::DecompressedTooLarge { limit }),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Err(ImageError::Truncated),
        Err(error) => Err(ImageError::Corrupt(format!("gzip: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    use crate::images::{DEFAULT_MAX_FILE_SIZE, LoadOptions, Source, decode};
    use crate::load::load_with;
    use crate::load::tests::{fixture, fixtures, open, rgba};
    use crate::load::url::load_url;

    /// `bytes` decoded, which must give an SVG.
    fn svg(bytes: &[u8], fonts: &Fonts) -> Source {
        match decode(bytes, DEFAULT_MAX_FILE_SIZE, fonts) {
            Ok(source @ Source::Svg(_)) => source,
            other => panic!("not an SVG: {other:?}"),
        }
    }

    fn svg_fixture(name: &str, fonts: &Fonts) -> Source {
        svg(
            &std::fs::read(fixtures().join("images").join(name)).unwrap(),
            fonts,
        )
    }

    /// How many pixels differ between two images of the same size.
    fn differing_pixels(a: &RgbaImage, b: &RgbaImage) -> usize {
        assert_eq!(a.dimensions(), b.dimensions());
        a.pixels().zip(b.pixels()).filter(|(x, y)| x != y).count()
    }

    #[test]
    fn decodes_svg_at_its_natural_size_in_css_pixels() {
        let none = Fonts::none();
        let size = |name| svg_fixture(name, &none).size();
        assert_eq!(size("pattern.svg"), (320, 240));
        assert_eq!(size("airfoil-forces.svg"), (791, 333));
        // Millimetres at 96 CSS pixels to the inch, rounded.
        assert_eq!(size("sunset-icon.svg"), (358, 210));
        assert_eq!(size("soccer-ball.svg"), (114, 110));
        assert_eq!(size("planetary-gearset.svg"), (4000, 3000));
        // Without a width and height, the viewBox gives the size.
        assert_eq!(size("speedway.svg"), (3000, 2013));
        assert_eq!(size("silhouette.svg"), (2689, 1560));
    }

    #[test]
    fn draws_an_svg_at_the_size_it_is_shown_with_sharp_edges() {
        let pattern = svg_fixture("pattern.svg", &Fonts::none());
        let large = pattern.render(640, 480);
        assert_eq!(large.dimensions(), (640, 480));
        assert_eq!(rgba(&large, 319, 239), [255, 0, 0, 255]);
        assert_eq!(rgba(&large, 320, 239), [0, 255, 0, 255]);
        assert_eq!(rgba(&large, 319, 240), [0, 0, 255, 255]);
        assert_eq!(rgba(&large, 320, 240), [255, 255, 0, 255]);
        let small = pattern.render(32, 24);
        assert_eq!(rgba(&small, 15, 11), [255, 0, 0, 255]);
        assert_eq!(rgba(&small, 16, 12), [255, 255, 0, 255]);
    }

    #[test]
    fn draws_svg_transparency_unpremultiplied() {
        let half = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
            <rect width="4" height="4" fill="#0000ff" fill-opacity="0.5"/></svg>"##;
        let pixel = rgba(&svg(half, &Fonts::none()).render(4, 4), 1, 1);
        assert_eq!(pixel[..3], [0, 0, 255]);
        assert!((126..=129).contains(&pixel[3]), "{pixel:?}");
    }

    #[test]
    fn decodes_svg_compressed_with_a_bom_or_with_an_xml_prolog() {
        let plain = std::fs::read(fixtures().join("images/pattern.svg")).unwrap();
        let text = String::from_utf8(plain.clone()).unwrap();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        io::Write::write_all(&mut gzip, &plain).unwrap();
        let variants = [
            ("gzip", gzip.finish().unwrap()),
            ("BOM", format!("\u{feff}{text}").into_bytes()),
            (
                "prolog",
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n\
                     <!-- A comment -->\n\
                     <!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \
                     \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n{text}"
                )
                .into_bytes(),
            ),
            (
                "BOM and prolog",
                format!("\u{feff}<?xml version=\"1.0\"?>\n{text}").into_bytes(),
            ),
        ];
        let expected = svg(&plain, &Fonts::none()).render(32, 24);
        for (name, bytes) in variants {
            let image = svg(&bytes, &Fonts::none());
            assert_eq!(image.size(), (320, 240), "{name}");
            assert_eq!(image.render(32, 24), expected, "{name}");
        }
        // A .svgz file on disk.
        let svgz = svg_fixture("pattern.svgz", &Fonts::none());
        assert_eq!(svgz.render(32, 24), expected);
    }

    /// `svg` compressed with gzip.
    fn gzip(svg: &[u8]) -> Vec<u8> {
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        io::Write::write_all(&mut gzip, svg).unwrap();
        gzip.finish().unwrap()
    }

    #[test]
    fn refuses_a_compressed_svg_that_decompresses_past_the_limit() {
        // Just over the default limit of spaces, which compress to a few
        // kilobytes.
        let mut large =
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1">"#.to_vec();
        large.resize(DEFAULT_MAX_FILE_SIZE as usize, b' ');
        large.extend_from_slice(b"</svg>");
        let compressed = gzip(&large);
        assert!(compressed.len() < 64 * 1024, "{}", compressed.len());
        let fonts = Fonts::none();
        assert_eq!(
            decode(&compressed, DEFAULT_MAX_FILE_SIZE, &fonts),
            Err(ImageError::DecompressedTooLarge {
                limit: DEFAULT_MAX_FILE_SIZE
            })
        );
        // At the limit, it still loads.
        let size = large.len() as u64;
        assert_eq!(
            decode(&compressed, size, &fonts).map(|svg| svg.size()),
            Ok((2, 1))
        );
        assert_eq!(
            decode(&compressed[..compressed.len() / 2], size, &fonts),
            Err(ImageError::Truncated)
        );
        // The limit on files applies to data: URLs too.
        let url = format!("data:image/svg+xml;base64,{}", STANDARD.encode(&compressed));
        assert_eq!(
            load_url(&url, Path::new("/"), size - 1, &fonts),
            Err(ImageError::DecompressedTooLarge { limit: size - 1 })
        );
    }

    #[test]
    fn does_not_draw_a_compressed_svg_inside_an_svg() {
        // Nothing would limit how large it grows; a browser does not show
        // it either.
        let red = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
            <rect width="4" height="4" fill="#ff0000"/></svg>"##;
        let nested = |bytes: &[u8]| {
            let outer = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
                <image width="4" height="4" href="data:image/svg+xml;base64,{}"/></svg>"#,
                STANDARD.encode(bytes)
            );
            svg(outer.as_bytes(), &Fonts::none()).render(4, 4)
        };
        assert_eq!(rgba(&nested(red), 2, 2), [255, 0, 0, 255]);
        assert_eq!(rgba(&nested(&gzip(red)), 2, 2)[3], 0);
    }

    #[test]
    fn reports_a_broken_svg() {
        assert!(
            matches!(fixture("truncated.svg"), Err(ImageError::Corrupt(_))),
            "{:?}",
            fixture("truncated.svg")
        );
    }

    #[test]
    fn loads_svg_data_urls() {
        let bytes = std::fs::read(fixtures().join("images/pattern.svg")).unwrap();
        let url = format!("data:image/svg+xml;base64,{}", STANDARD.encode(&bytes));
        let loaded = open(&url, Path::new("/"));
        assert_eq!(loaded, fixture("pattern.svg"));
        assert!(loaded.is_ok());
        let inline = "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' \
            width='2' height='1'/>";
        let loaded = open(inline, Path::new("/"));
        assert_eq!(loaded.map(|source| source.size()), Ok((2, 1)));
    }

    #[test]
    fn loads_fonts_only_for_a_document_with_an_svg() {
        let fonts = Fonts::none();
        let without = "![a](images/pattern.png)\n\n![b](images/missing.svg)\n\n\
            ![c](images/not-an-image.png)\n";
        load_with(
            &crate::typeset::parse::parse(without),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &fonts,
            &LoadOptions::default(),
        );
        assert!(!fonts.loaded());
        let with = "![d](images/pattern.svg)\n";
        load_with(
            &crate::typeset::parse::parse(with),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &fonts,
            &LoadOptions::default(),
        );
        assert!(fonts.loaded());
    }

    #[test]
    fn leaves_out_svg_text_without_fonts() {
        let text = br##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="30">
            <text x="5" y="25" font-size="24" font-family="sans-serif">Kiri</text></svg>"##;
        let image = svg(text, &Fonts::none()).render(80, 30);
        assert!(image.pixels().all(|pixel| pixel.0[3] == 0));
    }

    #[test]
    fn draws_svg_text_with_the_fonts_it_is_given() {
        let fonts = Fonts::fixture();
        // Greek words in sans-serif, set by a style sheet, and English ones
        // in the default font.
        for (name, width, height) in [
            ("airfoil-forces.svg", 791, 333),
            ("planetary-gearset.svg", 800, 600),
        ] {
            let without = svg_fixture(name, &Fonts::none()).render(width, height);
            let with = svg_fixture(name, &fonts).render(width, height);
            let text = differing_pixels(&without, &with);
            assert!(text > 1000, "{name}: {text} pixels of text");
        }
    }

    #[test]
    fn generic_families_take_the_first_installed_font() {
        let installed = HashSet::from(["DejaVu Sans", "Arial", "Noto Serif"]);
        assert_eq!(
            generic_family("Noto Serif", SERIF, &installed),
            Some("Noto Serif")
        );
        assert_eq!(
            generic_family("FreeSans", SANS_SERIF, &installed),
            Some("DejaVu Sans")
        );
        assert_eq!(generic_family("FreeMono", MONOSPACE, &installed), None);
    }

    #[test]
    fn fallbacks_keep_the_installed_fonts_in_their_order() {
        let fonts = HashSet::from(["Noto Sans CJK SC", "Droid Sans Fallback", "DejaVu Sans"]);
        let (_, cjk) = SCRIPTS
            .iter()
            .find(|(script, _)| *script == "Hani")
            .unwrap();
        assert_eq!(
            installed(cjk, &fonts),
            ["Noto Sans CJK SC", "Droid Sans Fallback"]
        );
        assert_eq!(installed(EMOJI, &fonts), Vec::<&str>::new());
    }

    #[test]
    fn does_not_load_files_an_svg_refers_to() {
        // An image in a browser does not load anything further either.
        let path = fixtures().join("images/pattern.png");
        let linked = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24">
            <image width="32" height="24" href="{}"/></svg>"#,
            path.display()
        );
        let image = svg(linked.as_bytes(), &Fonts::none()).render(32, 24);
        assert!(image.pixels().all(|pixel| pixel.0[3] == 0));
    }

    #[test]
    fn draws_images_embedded_in_an_svg() {
        let png = std::fs::read(fixtures().join("images/pattern.png")).unwrap();
        let embedded = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24">
            <image width="32" height="24" href="data:image/png;base64,{}"/></svg>"#,
            STANDARD.encode(&png)
        );
        let image = svg(embedded.as_bytes(), &Fonts::none()).render(32, 24);
        assert_eq!(rgba(&image, 8, 6), [255, 0, 0, 255]);
    }
}
