//! Reading mode: the document's text laid out in a proportional font and
//! drawn as pixels, as a browser shows it. Nothing here knows about
//! terminals; `render::reading` shows the pixels.

pub mod fonts;
pub mod text;

pub use fonts::Fonts;
