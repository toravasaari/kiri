//! kitty's escape sequences: the graphics protocol, the text sizing protocol,
//! and the queries that tell whether a terminal speaks them.
//!
//! Every kitty-specific sequence lives in this directory.

pub mod graphics;
pub mod query;
pub mod text;
