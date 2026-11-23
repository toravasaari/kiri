//! The system's fonts: the one place that scans them. Everything that
//! draws text gets the `Fonts` this builds, handed in once by the process's
//! platform; the database itself, its laziness and its choice of generic
//! families are the typesetting engine's (`src/typeset/fonts.rs`).

use std::sync::Arc;

use resvg::usvg::fontdb;

use crate::typeset::fonts::Fonts;

/// The fonts installed on the system, scanned when the first text needs
/// them: a run that draws no text never reads them.
pub fn system() -> Arc<Fonts> {
    Arc::new(Fonts::lazy(scan))
}

/// Reads the font files the system has.
fn scan() -> fontdb::Database {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    database
}
