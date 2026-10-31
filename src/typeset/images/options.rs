//! How the images of a document are loaded, and its diagrams drawn, and the
//! documents shown together whose images are loaded with each other's.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::diagrams::renderer::{NoRenderer, Renderer};
use crate::typeset::diagrams::request::{FormulaColors, Theme};
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
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Looks {
    /// The fingerprint of how diagrams and formulas are drawn, as
    /// [`diagram_look`] has it.
    pub diagram: u64,
    /// How headings are drawn as images; `None` draws none.
    pub headings: Option<HeadingLook>,
}

impl Looks {
    /// The fingerprint of the look of headings of `level`, as
    /// [`level_look`] has it; 0 when headings are not drawn as images.
    pub fn heading(&self, level: u8) -> u64 {
        self.headings
            .map_or(0, |look| level_look(look.level(level)))
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
    /// What the keys of the images these options draw name.
    pub fn looks(&self) -> Looks {
        Looks {
            diagram: diagram_look(self.theme, self.formula, self.panel),
            headings: self.headings,
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
/// as it loads, is not in it yet: a new cell size would then make every
/// diagram a new key, and nothing reloads diagrams for one. That comes
/// with reconciling the wanted keys, which starts the loads a key asks for.
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
    /// Its number among the documents shown together, which the keys of
    /// its images carry.
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
        assert_eq!(LoadOptions::default().looks().diagram, 0);
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
            options(None).looks().diagram,
            options(Some((100, 50))).looks().diagram
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
        assert_ne!(options.looks().diagram, 0);
        assert_eq!(
            options.looks().diagram,
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
}
