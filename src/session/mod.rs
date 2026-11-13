//! The state engine: the pager's state, its decisions, and the components
//! and panels that it is made of. It is pure: the loop that reads keys and
//! draws frames is the platform's, in `src/pager/`, and it reaches this
//! part through messages in and effects out.

pub mod animation;
pub(crate) mod bands;
pub mod changes;
pub(crate) mod commands;
pub(crate) mod edition;
pub mod fade;
pub mod files;
pub mod frame;
pub mod help;
pub(crate) mod layers;
pub(crate) mod links;
pub mod loading;
pub mod plan;
pub mod prompt;
pub(crate) mod reveal;
pub mod search;
pub mod settings;
pub mod sources;
pub mod state;
pub mod status;
pub(crate) mod step;
pub mod tint;
pub mod view;
pub mod warnings;
