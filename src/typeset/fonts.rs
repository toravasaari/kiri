//! The fonts that text is drawn with, in SVG images and in reading mode:
//! one font database, loaded lazily from a loader that is handed in, and
//! the lists of the families kiri falls back to. Scanning the system for
//! fonts is the platform's (`src/load/fonts.rs`); everything here is pure
//! given the database.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, OnceLock};

use resvg::usvg::fontdb;

/// The fonts that text is drawn with. They are loaded when the first text
/// needs them, so a document without SVGs or reading mode never reads the
/// system's fonts. One `Fonts` is shared by everything that draws text, by
/// reference or in an `Arc`, also between threads.
pub struct Fonts {
    load: fn() -> fontdb::Database,
    database: OnceLock<Arc<fontdb::Database>>,
}

impl Fonts {
    /// The fonts that `load` gives, called when they are first needed; the
    /// generic families (`serif`, `sans-serif`, `monospace`) are then made
    /// to name installed fonts.
    pub fn lazy(load: fn() -> fontdb::Database) -> Self {
        Self {
            load,
            database: OnceLock::new(),
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

    /// The fonts, loaded now if they are not yet.
    pub(crate) fn database(&self) -> Arc<fontdb::Database> {
        Arc::clone(self.database.get_or_init(|| {
            let mut database = (self.load)();
            choose_generic_families(&mut database);
            Arc::new(database)
        }))
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
