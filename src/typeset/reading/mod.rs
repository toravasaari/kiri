//! Reading mode: the document's text laid out in a proportional font and
//! drawn as pixels, as a browser shows it. Nothing here knows about
//! terminals; `typeset::reading::engine` shows the pixels.

pub mod animated;
mod colr;
pub mod engine;
pub mod fonts;
mod inlines;
mod look;
mod octicons;
pub mod page;
pub mod paint;
pub(crate) mod places;
pub mod text;

pub use fonts::Fonts;
pub(crate) use look::page_look;
