//! Writes a `Scene` to an `impl Write` through the `Writer` as one
//! synchronized update: the lines row by row, then the bars in the order of
//! the scene. It does no I/O of its own. What a scene holds is `scene.rs`'s,
//! and kitty's own sequences are in `kitty/`.

use std::io::{self, Write};

use crate::present::output::{self, Writer};
use crate::present::scene::{Scene, Surface, flatten};
use crate::typeset::wrap::width;

/// Writes scenes. It keeps nothing yet; what it will remember of the
/// screen is added here.
#[derive(Debug, Default)]
pub struct Presenter {}

impl Presenter {
    /// Writes `scene` through `writer` to `out` as one synchronized update.
    pub fn present(
        &mut self,
        scene: &Scene<'_>,
        writer: &mut Writer<'_>,
        out: &mut impl Write,
    ) -> io::Result<()> {
        output::begin_update(out)?;
        writer.delete_unused(out)?;
        writer.clear(out)?;
        let flat = flatten(scene, |line| writer.rows(line));
        for (r, row) in flat.rows.iter().enumerate() {
            let Some((line, at)) = row.line else {
                continue;
            };
            writer.dim(row.dim);
            let mut bytes = Vec::new();
            writer.row(line, at, &mut bytes)?;
            output::move_to(r + 1, 1, out)?;
            out.write_all(&bytes)?;
        }
        writer.dim(false);
        for surface in &scene.surfaces {
            let Surface::Bar { row, column, spans } = surface else {
                continue;
            };
            let Some(under) = row.checked_sub(1).and_then(|r| flat.rows.get(r)) else {
                continue;
            };
            let mut bytes = Vec::new();
            output::move_to(*row, *column, &mut bytes)?;
            // Writing on the top row of a scaled line drops the characters
            // it touches whole; below it, they stay until they are erased.
            if under.scaled && row - 1 != under.slot.start {
                writer.erase_chars(width(spans), &mut bytes)?;
            }
            writer.bar(spans, &mut bytes)?;
            out.write_all(&bytes)?;
        }
        output::end_update(out)
    }
}
