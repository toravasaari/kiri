//! The state engine: the pager's state, its decisions, and the components
//! and panels that it is made of. It is pure: the loop that reads keys and
//! draws frames is the platform's, in `src/pager/`, and it reaches this
//! part through messages in and effects out.

pub(crate) mod bands;
pub mod changes;
pub(crate) mod commands;
pub mod fade;
pub mod files;
pub mod help;
pub(crate) mod layers;
pub(crate) mod links;
pub mod loading;
pub mod plan;
pub mod prompt;
pub mod search;
pub mod settings;
pub mod status;
pub mod tint;
pub mod view;
pub mod warnings;
