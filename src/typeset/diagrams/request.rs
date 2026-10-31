//! What a diagram renderer is spoken to in: the page's theme, a request to
//! draw, and the drawing that comes back.

use std::sync::atomic::AtomicBool;

/// The colours a diagram is drawn in when it does not choose its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Theme {
    /// For a light page.
    #[default]
    Light,
    /// For a dark page.
    Dark,
}

impl Theme {
    /// The theme for a page whose background is dark or not; a page whose
    /// background is unknown counts as dark, as it does for code blocks.
    pub fn for_page(dark_background: Option<bool>) -> Self {
        match dark_background {
            Some(true) | None => Theme::Dark,
            Some(false) => Theme::Light,
        }
    }
}

/// The colours a formula is drawn in, as RGB.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FormulaColors {
    /// Its letters and lines: the colour of the text around it, unless
    /// the theme gives formulas their own; `None` when it is not known.
    pub text: Option<[u8; 3]>,
    /// Behind it; `None` for no background of its own.
    pub background: Option<[u8; 3]>,
}

/// A diagram drawn as an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawn {
    /// The image file: SVG, or a raster image such as PNG, told apart by
    /// its content.
    pub image: Vec<u8>,
    /// How many pixels of a raster image make one CSS pixel, as a browser
    /// draws for a display of that density. An SVG has its own size.
    pub scale: u32,
    /// The page its colours are for: the theme it was asked for, unless
    /// the diagram chose colours of its own.
    pub theme: Theme,
    /// The background colour of its theme, when the renderer tells it.
    pub background: Option<[u8; 3]>,
}

impl Drawn {
    /// A diagram drawn as the SVG document `svg`, for a page of `theme`,
    /// with the background colour `background` when the renderer tells
    /// it.
    pub fn svg(svg: impl Into<String>, theme: Theme, background: Option<[u8; 3]>) -> Self {
        Drawn {
            image: svg.into().into_bytes(),
            scale: 1,
            theme,
            background,
        }
    }

    /// The image as text, as an SVG is.
    #[cfg(test)]
    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.image).into_owned()
    }
}

/// A diagram to draw.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// Its language, as the info string of its code block gives it.
    pub language: &'a str,
    pub source: &'a str,
    /// The page it is drawn for, unless it chooses its own colours.
    pub theme: Theme,
    /// The colours a formula is drawn in.
    pub formula: FormulaColors,
    /// Set when this drawing alone is to stop, as when the diagram is no
    /// longer wanted: a program drawing it is stopped, and one that has
    /// not started does not. A renderer built into kiri cannot stop.
    pub stop: Option<&'a AtomicBool>,
}

impl<'a> Request<'a> {
    pub fn new(language: &'a str, source: &'a str, theme: Theme) -> Self {
        Request {
            language,
            source,
            theme,
            formula: FormulaColors::default(),
            stop: None,
        }
    }

    /// The same request, with a formula drawn in `formula`.
    pub fn in_colors(self, formula: FormulaColors) -> Self {
        Request { formula, ..self }
    }

    /// The same request, stopped when `stop` is set.
    pub fn stopped_by(self, stop: &'a AtomicBool) -> Self {
        Request {
            stop: Some(stop),
            ..self
        }
    }
}

/// The language of a formula in the middle of text, drawn in text style
/// at the size of the text around it.
pub const INLINE_FORMULA: &str = "math-inline";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_page_counts_as_dark() {
        // As for code blocks and quotes.
        assert_eq!(Theme::for_page(None), Theme::Dark);
        assert_eq!(Theme::for_page(Some(false)), Theme::Light);
        assert_eq!(Theme::for_page(Some(true)), Theme::Dark);
    }
}
