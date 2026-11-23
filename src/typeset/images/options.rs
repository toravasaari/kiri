//! How the images of a document are loaded, and its diagrams drawn, and the
//! documents shown together whose images are loaded with each other's.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::diagrams::renderer::{NoRenderer, Renderer};
use crate::typeset::diagrams::request::{FormulaColors, Theme};
use crate::typeset::fonts::Fonts;
use crate::typeset::images::heading::{HeadingLook, LevelLook};
use crate::typeset::model::Document;
use crate::typeset::theme::Theme as PageTheme;

/// How the images of a document are loaded, and its diagrams drawn.
#[derive(Clone, Copy)]
pub struct LoadOptions<'a> {
    /// What draws the diagrams; by default nothing, so that every code
    /// block stays code.
    pub renderer: &'a dyn Renderer,
    /// The page the diagrams are drawn for, unless they choose their own
    /// colours; by default a light one.
    pub theme: Theme,
    /// The colours formulas are drawn in; by default none, which leaves
    /// them to the page.
    pub formula: FormulaColors,
    /// Whether a warning names a diagram by its line in the document, or
    /// by its own first line, for a document that has no lines the reader
    /// can look up, such as one piped in; by default by its line.
    pub by_line: bool,
    /// Told how many of a document's diagrams are drawn, and of how many:
    /// first none, and then each time one more is.
    pub progress: Option<&'a (dyn Fn(usize, usize) + Sync)>,
    /// Whether a diagram whose colours are for the other page, light or
    /// dark, lies on a panel of its own background; by default it does.
    pub panel: bool,
    /// The most pixels, wide and high, an image can be shown at: a
    /// diagram drawn in pixels is reduced to fit them as it loads. `None`,
    /// the default, keeps it as drawn.
    pub largest: Option<(u32, u32)>,
    /// How headings are drawn as images; `None`, the default, draws none.
    pub headings: Option<HeadingLook>,
}

impl Default for LoadOptions<'_> {
    fn default() -> Self {
        LoadOptions {
            renderer: &NoRenderer,
            theme: Theme::Light,
            formula: FormulaColors::default(),
            by_line: true,
            progress: None,
            panel: true,
            largest: None,
            headings: None,
        }
    }
}

/// What keys name of how images are drawn, beyond what the images are
/// made from: the creators of a key, the loader and the layout, take them
/// from here, so that a key changes whenever its picture does.
#[derive(Debug, Clone, Default)]
pub struct Looks {
    /// The fingerprint of how diagrams and formulas are drawn, as
    /// [`diagram_look`] has it, without the fonts. A key takes
    /// [`Looks::diagram_look`].
    pub diagram_base: u64,
    /// How headings are drawn as images; `None` draws none.
    pub headings: Option<HeadingLook>,
    /// The fonts that text in the images is drawn with, whose fingerprint
    /// the keys of diagrams, formulas and headings carry; `None` for keys
    /// that name none. They are asked for the fingerprint only when a key
    /// of an image that has text is made, which loads them.
    pub fonts: Option<Arc<Fonts>>,
}

impl Looks {
    /// These looks with `fonts` named in the keys of images that have text.
    pub fn with_fonts(self, fonts: Arc<Fonts>) -> Self {
        Looks {
            fonts: Some(fonts),
            ..self
        }
    }

    /// The fingerprint of how diagrams and formulas are drawn, with the
    /// fonts of their text if those are loaded, for a layout to look a
    /// picture up by. A picture is made only after its key was made by
    /// [`Looks::diagram_look_loading`], which loads the fonts, so a layout
    /// that asks before has nothing to find and does not load them: a
    /// document with a code block in Rust does not read the system's fonts.
    pub fn diagram_look(&self) -> u64 {
        with_fonts(
            self.diagram_base,
            self.fonts.as_deref().filter(|f| f.loaded()),
        )
    }

    /// The fingerprint of how diagrams and formulas are drawn, with the
    /// fonts of their text, which loads them: the key of a load that
    /// draws a diagram.
    pub fn diagram_look_loading(&self) -> u64 {
        with_fonts(self.diagram_base, self.fonts.as_deref())
    }

    /// The fingerprint of the look of headings of `level`, as
    /// [`level_look`] has it, with the fonts of their text if those are
    /// loaded, as [`Looks::diagram_look`] has it; 0 when headings are not
    /// drawn as images.
    pub fn heading(&self, level: u8) -> u64 {
        self.heading_with(level, self.fonts.as_deref().filter(|f| f.loaded()))
    }

    /// The fingerprint of the look of headings of `level` with the fonts of
    /// their text, which loads them, as [`Looks::diagram_look_loading`] has
    /// it; 0 when headings are not drawn as images.
    pub fn heading_loading(&self, level: u8) -> u64 {
        self.heading_with(level, self.fonts.as_deref())
    }

    fn heading_with(&self, level: u8, fonts: Option<&Fonts>) -> u64 {
        match &self.headings {
            Some(look) => with_fonts(level_look(look.level(level)), fonts),
            None => 0,
        }
    }
}

/// `look` with the fingerprint of `fonts`: unchanged for no fonts, and for
/// a database without a font, which draws no text either way.
fn with_fonts(look: u64, fonts: Option<&Fonts>) -> u64 {
    match fonts.map(Fonts::fingerprint) {
        None | Some(0) => look,
        Some(fonts) => {
            let mut hasher = DefaultHasher::new();
            (look, fonts).hash(&mut hasher);
            hasher.finish().max(1)
        }
    }
}

/// A fingerprint of how headings of one level are drawn: the rows, the
/// size, the style and the colours of its text and of its inline code. It
/// holds for a run only.
pub fn level_look(look: &LevelLook) -> u64 {
    let mut hasher = DefaultHasher::new();
    format!("{look:?}").hash(&mut hasher);
    hasher.finish().max(1)
}

impl LoadOptions<'_> {
    /// What the keys of the images these options draw name, except the
    /// fonts of their text, which [`Looks::with_fonts`] adds.
    pub fn looks(&self) -> Looks {
        Looks {
            diagram_base: diagram_look(self.theme, self.formula, self.panel),
            headings: self.headings,
            fonts: None,
        }
    }
}

/// A fingerprint of how a diagram or formula is drawn: the page it is
/// drawn for, the colours of formulas and whether it lies on a panel. It
/// holds for a run only. The defaults, a light page, no formula colours
/// and a panel, are 0, which is also the fingerprint of a key built
/// without options.
///
/// The most pixels an image is shown at, which reduces a raster diagram
/// as it loads, is not in it, on purpose: only a font change moves it, and
/// a new key would draw every diagram again, running its program, with the
/// diagram showing as code until then. A dimmed old drawing standing in
/// meanwhile would make it worth doing.
pub fn diagram_look(theme: Theme, formula: FormulaColors, panel: bool) -> u64 {
    if (theme, formula, panel) == (Theme::Light, FormulaColors::default(), true) {
        return 0;
    }
    let mut hasher = DefaultHasher::new();
    (theme, formula, panel).hash(&mut hasher);
    hasher.finish().max(1)
}

/// [`diagram_look`] for a page in `theme` on a terminal with `caps`, as
/// the loader and the layout both have it: the diagrams are drawn for the
/// terminal's page, with formulas in the colours of [`formula_colors`].
pub fn diagram_look_for(theme: &PageTheme, caps: &Capabilities, panel: bool) -> u64 {
    let page = Theme::for_page(caps.dark_background);
    diagram_look(page, formula_colors(theme, caps), panel)
}

/// The colours formulas are drawn in, as RGB: the theme's for formulas,
/// and otherwise the text's, the theme's or else the terminal's; the
/// first of them known as RGB.
pub fn formula_colors(theme: &PageTheme, caps: &Capabilities) -> FormulaColors {
    let rgb = |color: Option<Color>| match color?.rgb(&caps.palette)? {
        Color::Rgb(red, green, blue) => Some([red, green, blue]),
        Color::Palette(_) => None,
    };
    let text = [theme.formula.text, theme.foreground, caps.foreground];
    FormulaColors {
        text: text.into_iter().find_map(rgb),
        background: rgb(theme.formula.background),
    }
}

/// One of several documents shown together, whose images are loaded with
/// the others'.
#[derive(Clone, Copy)]
pub struct Part<'a> {
    pub document: &'a Document,
    /// Its number in the session, which the keys of its images carry.
    pub number: usize,
    /// The directory that its relative image paths start from.
    pub base: &'a Path,
    /// Whether a warning names a diagram by its line in the document, or
    /// by its own first line, as `LoadOptions::by_line` says.
    pub by_line: bool,
    /// Whether the images in the middle of text are wanted too, as reading
    /// mode draws them; otherwise only an image that is a block of its
    /// own is, as the others show as their alt text.
    pub inline_images: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::images::ImageKey;
    use crate::typeset::images::plan::Wanted;

    const NAVY: Option<[u8; 3]> = Some([0x12, 0x34, 0x56]);

    fn look(theme: Theme, text: Option<[u8; 3]>, panel: bool) -> u64 {
        let formula = FormulaColors {
            text,
            background: None,
        };
        diagram_look(theme, formula, panel)
    }

    #[test]
    fn the_look_of_the_default_options_is_0() {
        assert_eq!(look(Theme::Light, None, true), 0);
        assert_eq!(LoadOptions::default().looks().diagram_look(), 0);
    }

    #[test]
    fn the_page_a_diagram_is_drawn_for_is_in_its_look() {
        let light = look(Theme::Light, NAVY, true);
        assert_eq!(light, look(Theme::Light, NAVY, true));
        assert_ne!(light, look(Theme::Dark, NAVY, true));
    }

    #[test]
    fn the_colours_of_a_formula_are_in_the_look() {
        let navy = look(Theme::Dark, NAVY, true);
        assert_eq!(navy, look(Theme::Dark, NAVY, true));
        assert_ne!(navy, look(Theme::Dark, Some([0, 0, 0]), true));
        assert_ne!(navy, look(Theme::Dark, None, true));
        let on_white = FormulaColors {
            text: NAVY,
            background: Some([255; 3]),
        };
        assert_ne!(navy, diagram_look(Theme::Dark, on_white, true));
    }

    #[test]
    fn the_panel_is_in_the_look() {
        let panel = look(Theme::Dark, None, true);
        assert_eq!(panel, look(Theme::Dark, None, true));
        assert_ne!(panel, look(Theme::Dark, None, false));
    }

    #[test]
    fn the_largest_size_is_not_in_the_look_yet() {
        // A new cell size would make every diagram a new key, and nothing
        // reloads diagrams for one.
        let options = |largest| LoadOptions {
            largest,
            ..LoadOptions::default()
        };
        assert_eq!(
            options(None).looks().diagram_look(),
            options(Some((100, 50))).looks().diagram_look()
        );
    }

    #[test]
    fn the_keys_of_one_diagram_differ_where_its_look_does() {
        let key = |options: LoadOptions<'_>| {
            let looks = options.looks();
            let wanted = Wanted::Diagram {
                language: "mermaid",
                source: "a\n".into(),
                line: 1,
            };
            wanted.key(&looks)
        };
        let dark = LoadOptions {
            theme: Theme::Dark,
            ..LoadOptions::default()
        };
        assert_eq!(key(LoadOptions::default()), key(LoadOptions::default()));
        assert_ne!(key(LoadOptions::default()), key(dark));
    }

    #[test]
    fn the_loader_and_the_layout_have_the_same_look() {
        let caps = Capabilities {
            dark_background: Some(true),
            foreground: Some(Color::Rgb(1, 2, 3)),
            ..Capabilities::kitty(80)
        };
        let theme = PageTheme::default();
        let options = LoadOptions {
            theme: Theme::for_page(caps.dark_background),
            formula: formula_colors(&theme, &caps),
            panel: false,
            largest: Some(crate::typeset::images::size::largest(&caps)),
            ..LoadOptions::default()
        };
        assert_ne!(options.looks().diagram_look(), 0);
        assert_eq!(
            options.looks().diagram_look(),
            diagram_look_for(&theme, &caps, false)
        );
    }

    fn levels(color: [u8; 3], italic: bool) -> Looks {
        let code = crate::typeset::images::heading::CodeLook {
            color: [9, 9, 9],
            background: None,
            padding: false,
        };
        let level = |color| LevelLook {
            rows: 2,
            font_px: 30.0,
            bold: true,
            italic,
            color,
            code,
        };
        let mut levels = [level([0; 3]); 6];
        levels[0] = level(color);
        Looks {
            headings: Some(HeadingLook {
                cell: (10, 20),
                levels,
            }),
            ..Looks::default()
        }
    }

    #[test]
    fn the_look_of_a_heading_s_level_is_in_its_key() {
        let key = |looks: &Looks, level| {
            let wanted = Wanted::Heading {
                level,
                words: Vec::new(),
                cell: (10, 20),
            };
            wanted.key(looks)
        };
        let red = levels([255, 0, 0], false);
        assert_eq!(key(&red, 1), key(&levels([255, 0, 0], false), 1));
        assert_ne!(key(&red, 1), key(&levels([0, 255, 0], false), 1));
        assert_ne!(key(&red, 1), key(&levels([255, 0, 0], true), 1));
        // The look of another level does not change it.
        assert_eq!(key(&red, 2), key(&levels([0, 255, 0], false), 2));
        assert_eq!(Looks::default().heading(1), 0);
    }

    /// `looks` with `fonts` as the fonts of the text in its images.
    fn in_fonts(looks: Looks, fonts: Fonts) -> Looks {
        looks.with_fonts(Arc::new(fonts))
    }

    fn diagram_key(looks: &Looks, language: &str) -> ImageKey {
        let wanted = Wanted::Diagram {
            language,
            source: "a\n".into(),
            line: 1,
        };
        wanted.key(looks)
    }

    #[test]
    fn the_fonts_are_in_the_key_of_a_diagram() {
        let (none, fixture) = (Fonts::none(), Fonts::fixture());
        let drawn = |fonts| diagram_key(&in_fonts(Looks::default(), fonts), "mermaid");
        assert_ne!(drawn(none), drawn(fixture));
        assert_eq!(drawn(Fonts::fixture()), drawn(Fonts::fixture()));
    }

    #[test]
    fn the_fonts_are_in_the_key_of_a_formula() {
        let formula = |fonts| diagram_key(&in_fonts(Looks::default(), fonts), "math");
        assert_ne!(formula(Fonts::none()), formula(Fonts::fixture()));
        assert_eq!(formula(Fonts::fixture()), formula(Fonts::fixture()));
    }

    #[test]
    fn the_fonts_are_in_the_key_of_a_heading() {
        let heading = |fonts| {
            let wanted = Wanted::Heading {
                level: 1,
                words: Vec::new(),
                cell: (10, 20),
            };
            wanted.key(&in_fonts(levels([255, 0, 0], false), fonts))
        };
        assert_ne!(heading(Fonts::none()), heading(Fonts::fixture()));
        assert_eq!(heading(Fonts::fixture()), heading(Fonts::fixture()));
    }

    #[test]
    fn a_key_made_without_fonts_is_that_of_a_database_without_a_font() {
        // Neither draws any text, so a picture is the same under both.
        let none = in_fonts(Looks::default(), Fonts::none());
        assert_eq!(
            diagram_key(&none, "mermaid"),
            diagram_key(&Looks::default(), "mermaid")
        );
        let red = levels([255, 0, 0], false);
        assert_eq!(
            in_fonts(red.clone(), Fonts::none()).heading_loading(1),
            red.heading_loading(1)
        );
    }

    #[test]
    fn only_a_key_of_an_image_with_text_loads_the_fonts() {
        let fonts = Arc::new(Fonts::fixture());
        let looks = Looks {
            fonts: Some(Arc::clone(&fonts)),
            ..Looks::default()
        };
        // The key of an image file, and of a heading when none is drawn as
        // an image, does not ask.
        let url = Wanted::Url("a.png");
        assert_eq!(url.key(&looks), ImageKey::from("a.png"));
        assert_eq!(looks.heading(1), 0);
        assert!(!fonts.loaded());
        diagram_key(&looks, "mermaid");
        assert!(fonts.loaded());
    }

    #[test]
    fn a_layout_looking_a_picture_up_does_not_load_the_fonts() {
        // What it looks for is not there before a load made its key, which
        // loads them; and once they are loaded the keys are the same.
        let fonts = Arc::new(Fonts::fixture());
        let looks = in_fonts(levels([255, 0, 0], false), Fonts::fixture());
        let looks = Looks {
            fonts: Some(Arc::clone(&fonts)),
            ..looks
        };
        let (diagram, heading) = (looks.diagram_look(), looks.heading(1));
        assert!(!fonts.loaded());
        assert_eq!(diagram, 0);
        assert_eq!(heading, levels([255, 0, 0], false).heading(1));
        assert_ne!(looks.diagram_look_loading(), diagram);
        assert!(fonts.loaded());
        assert_eq!(looks.diagram_look(), looks.diagram_look_loading());
        assert_eq!(looks.heading(1), looks.heading_loading(1));
        assert_ne!(looks.heading(1), heading);
    }
}
