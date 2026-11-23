//! The one font database that text is drawn with, in SVG images and in reading
//! mode, and the lists of the families kiri falls back to. The database is
//! loaded lazily from a loader that is handed in. Scanning the system for fonts
//! is the platform's (`src/load/fonts.rs`); everything here is pure given the
//! database.
//!
//! `Fonts` is the one database, loaded when text first needs it
//! (`Fonts::lazy`), and has a `fingerprint`. The lists of families are `SERIF`,
//! `SANS_SERIF`, `MONOSPACE`, `EMOJI` and `SCRIPTS`.

use std::collections::HashSet;
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, OnceLock};

use resvg::usvg::fontdb;

/// The fonts that text is drawn with. They are loaded when the first text
/// needs them, so a document without SVGs or reading mode never reads the
/// system's fonts. One `Fonts` is shared by everything that draws text, by
/// reference or in an `Arc`, also between threads.
pub struct Fonts {
    load: fn() -> fontdb::Database,
    database: OnceLock<Arc<fontdb::Database>>,
    fingerprint: OnceLock<u64>,
}

impl Fonts {
    /// The fonts that `load` gives, called when they are first needed; the
    /// generic families (`serif`, `sans-serif`, `monospace`) are then made
    /// to name installed fonts.
    pub fn lazy(load: fn() -> fontdb::Database) -> Self {
        Self {
            load,
            database: OnceLock::new(),
            fingerprint: OnceLock::new(),
        }
    }

    /// No fonts at all: text in SVGs is left out.
    pub fn none() -> Self {
        Self::lazy(fontdb::Database::new)
    }

    /// DejaVu Sans and its oblique face from the test fixtures and nothing
    /// else, so text in SVGs looks the same on every machine.
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self::lazy(|| fixture_database(&["DejaVuSans.ttf", "DejaVuSans-Oblique.ttf"]))
    }

    /// The fixture fonts that reading mode's tests lay text out in: DejaVu
    /// Sans in its regular, bold and oblique faces, Noto Sans Mono, and a
    /// few of Noto Color Emoji's emoji.
    #[cfg(test)]
    pub(crate) fn reading_fixture() -> Self {
        Self::lazy(|| {
            fixture_database(&[
                "DejaVuSans.ttf",
                "DejaVuSans-Bold.ttf",
                "DejaVuSans-Oblique.ttf",
                "NotoSansMono.ttf",
                "NotoColorEmoji-subset.ttf",
            ])
        })
    }

    /// Whether anything has needed the fonts yet.
    pub fn loaded(&self) -> bool {
        self.database.get().is_some()
    }

    /// A number that names what is in the database, for the keys of what
    /// is drawn with it: each face's family names, weight, style, stretch
    /// and source (its path, or that it is in memory), whatever the order
    /// they were found in, and the fonts the generic families name. The
    /// same for the same fonts in any run of the same build; 0 for a
    /// database without a font, which draws no text. Worked out on the
    /// first call, which loads the fonts, and kept.
    pub fn fingerprint(&self) -> u64 {
        *self.fingerprint.get_or_init(|| {
            let database = self.database();
            let mut faces: Vec<String> = database.faces().map(face_description).collect();
            if faces.is_empty() {
                return 0;
            }
            faces.sort_unstable();
            let mut hasher = DefaultHasher::new();
            faces.hash(&mut hasher);
            for family in [
                fontdb::Family::Serif,
                fontdb::Family::SansSerif,
                fontdb::Family::Monospace,
            ] {
                database.family_name(&family).hash(&mut hasher);
            }
            hasher.finish().max(1)
        })
    }

    /// The fonts, loaded now if they are not yet.
    pub(crate) fn database(&self) -> Arc<fontdb::Database> {
        Arc::clone(self.database.get_or_init(|| {
            let mut database = (self.load)();
            choose_generic_families(&mut database);
            Arc::new(database)
        }))
    }
}

/// What tells a face from another, as text: its family names, weight, style
/// and stretch, and where it comes from.
fn face_description(face: &fontdb::FaceInfo) -> String {
    let mut names: Vec<&str> = face
        .families
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    names.sort_unstable();
    let source = match &face.source {
        fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => {
            path.display().to_string()
        }
        fontdb::Source::Binary(_) => "in memory".to_owned(),
    };
    format!(
        "{names:?} {} {:?} {:?} {source} {}",
        face.weight.0, face.style, face.stretch, face.index
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_databases_of_the_same_fonts_have_the_same_fingerprint() {
        assert_eq!(
            Fonts::fixture().fingerprint(),
            Fonts::fixture().fingerprint()
        );
        assert_eq!(
            Fonts::reading_fixture().fingerprint(),
            Fonts::reading_fixture().fingerprint()
        );
    }

    #[test]
    fn other_fonts_have_another_fingerprint() {
        let fixture = Fonts::fixture().fingerprint();
        assert_ne!(fixture, 0);
        assert_ne!(fixture, Fonts::reading_fixture().fingerprint());
        assert_ne!(fixture, Fonts::none().fingerprint());
    }

    #[test]
    fn a_database_without_a_font_has_the_fingerprint_0() {
        // Text is drawn nowhere, as in a key made without fonts.
        assert_eq!(Fonts::none().fingerprint(), 0);
    }

    #[test]
    fn the_fingerprint_loads_the_fonts_when_it_is_first_asked_for() {
        let fonts = Fonts::fixture();
        assert!(!fonts.loaded());
        let fingerprint = fonts.fingerprint();
        assert!(fonts.loaded());
        assert_eq!(fonts.fingerprint(), fingerprint);
    }

    #[test]
    fn the_source_of_a_face_is_in_the_fingerprint() {
        // The same faces from files and from memory are not the same
        // fonts: nothing says the bytes are the same.
        let from_files = Fonts::lazy(|| fixture_database(&["DejaVuSans.ttf"]));
        let from_memory = Fonts::lazy(|| {
            let mut database = fontdb::Database::new();
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/fonts/DejaVuSans.ttf");
            database.load_font_data(std::fs::read(path).unwrap());
            database
        });
        assert_ne!(from_files.fingerprint(), from_memory.fingerprint());
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
}
