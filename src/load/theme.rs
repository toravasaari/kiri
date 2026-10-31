//! Reading a theme file: the file's text is read here, and turned into a
//! theme by `typeset::theme::parse`, which does no I/O.

use std::path::Path;

use crate::typeset::theme::{Theme, parse};

/// Reads a theme file. Keys it leaves out keep their default values, but
/// it is the user's theme, not the built-in one.
pub fn load(path: &Path) -> Result<Theme, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let theme = parse(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Theme {
        built_in: false,
        ..theme
    })
}
