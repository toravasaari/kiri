//! Reading mode: the document's text laid out in a proportional font and
//! drawn as pixels, as a browser shows it. Nothing here knows about
//! terminals; `typeset::reading::engine` shows the pixels.

mod colr;
pub mod engine;
pub mod fonts;
mod octicons;
pub mod page;
pub mod paint;
pub mod text;

pub use fonts::Fonts;
