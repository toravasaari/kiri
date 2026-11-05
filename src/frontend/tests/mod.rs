//! The frontends' tests: the route run in the simulated frontend
//! (`src/frontend/sim.rs`), through the same `Platform` the binary
//! implements for the process. One file for each topic: `route.rs` (one
//! file, several, standard input, a missing file), `paging.rs` (paging
//! and printing), `settings.rs`, `stdin.rs`, `follow.rs`, `reload.rs`, and
//! `close.rs` (how the pager ends), `open.rs` (what a file is shown as),
//! `loads.rs` (the images that load in the
//! background or before the pager opens) and `wait.rs` (the wait for them,
//! which the reader can skip).

mod close;
mod follow;
mod loads;
mod open;
mod paging;
mod reading;
mod reload;
mod route;
mod settings;
mod stdin;
mod wait;

use std::time::Duration;

use crate::frontend::sim::Sim;
use crate::images::{ImageError, Source};
use crate::session::settings::{InitialLoad, Paging};
use crate::typeset::caps::Capabilities;

/// A window of 40 columns and `rows` rows on kitty, which draws images.
fn on_kitty(rows: u16) -> Sim {
    Sim::new(40, rows).caps(Capabilities::kitty(40))
}

/// Paging as `-p` asks, and the images loaded as `initial_load` says.
fn paged(sim: Sim, initial_load: InitialLoad) -> Sim {
    sim.settings(move |settings| {
        settings.paging = Paging::Always;
        settings.initial_load = initial_load;
    })
}

/// A load that comes `after` milliseconds as a red square.
fn red(sim: Sim, url: &str, after: u64) -> Sim {
    let pixel = image::Rgba([255, 0, 0, 255]);
    let square = Source::Raster(image::RgbaImage::from_pixel(20, 20, pixel));
    sim.loads(url, Duration::from_millis(after), Ok(square))
}

/// A load that comes `after` milliseconds as a file that is not there.
fn missing(sim: Sim, url: &str, after: u64) -> Sim {
    let error = ImageError::Missing(url.into());
    sim.loads(url, Duration::from_millis(after), Err(error))
}

/// A document with a line, the picture `url` and a line.
fn pictured(url: &str) -> String {
    format!("Above\n\n![a]({url})\n\nBelow\n")
}

/// `count` paragraphs of one row each, `Line 1` to `Line count`.
fn paragraphs(count: usize) -> String {
    (1..=count).map(|n| format!("Line {n}\n\n")).collect()
}

/// A document longer than a window of 5 rows: `Line 1` to `Line 10`, one
/// row each and a blank row between.
fn long() -> String {
    paragraphs(10)
}

/// The text of `long.md` written anew: ten paragraphs of `Fresh`.
fn fresh() -> String {
    (1..=10).map(|n| format!("Fresh {n}\n\n")).collect()
}
