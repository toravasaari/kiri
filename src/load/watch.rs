//! What the pager asks of the file behind a document: to read it again and
//! to tell whether it changed.
//!
//! The pager's loop (`src/pager/mod.rs`) reaches the file only through
//! [`Reload`], which the frontends implement for the platform's files and
//! the simulated frontend for its disk.

use crate::images::{Images, Loads};
use crate::session::state::{Change, Look};
use crate::typeset::caps::Capabilities;
use crate::typeset::model::Document;

/// What the pager asks for the document again, as `r` does.
pub trait Reload {
    /// The document read again, with `images` brought up to date for it
    /// as it is shown in a window with `caps`: it keeps at once what the
    /// document still shows. What `images` held that the document no
    /// longer shows, or that must load anew, is set aside in it, as
    /// [`crate::images::reload_in_background`] does, which also stops in
    /// `loads` the load of an image whose file changed again. It starts no
    /// load: the pager does, for the images its planner wants and `images`
    /// does not hold. `reading` says
    /// whether reading mode is on now, which wants the images in the middle
    /// of text too, whatever the document was opened with.
    /// It also gives how the file looked when it was read, which is before
    /// it was, so that a change made meanwhile counts; `None` when it
    /// could not be looked at.
    fn reload(
        &mut self,
        images: &mut Images,
        loads: &mut Loads,
        caps: &Capabilities,
        reading: bool,
    ) -> (Option<Look>, Reloaded);

    /// Whether the document changed since it was last read, as the pager
    /// asks while it follows it.
    fn change(&mut self) -> Change;
}

/// What reloading the document came to.
pub enum Reloaded {
    Document(Document),
    /// As `Document`, with warnings about it, the first of which the
    /// pager shows.
    Warned(Document, Vec<crate::typeset::diagnostic::Diagnostic>),
    /// The document could not be read again: why, as the status line says
    /// it.
    Failed(String),
}
