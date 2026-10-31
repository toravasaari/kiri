//! The setting types: where a value comes from, the names a setting's
//! values have, and the types of the settings that are not the pager's
//! (`Paging`, `SearchCase`, `Follow` and `FollowJump` are in
//! `src/session/settings.rs`).

use std::path::PathBuf;

use crate::render::{FrontMatterStyle, Headings, RenderErrors, TableRows};
use crate::session::settings::{Follow, FollowJump, InitialLoad, Paging, SearchCase};
use crate::typeset::theme::Paper;

/// How the front matter at the start of a Markdown document shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrontMatter {
    /// A table of its keys and values, as GitHub shows it.
    #[default]
    Table,
    /// A code block in its language, as GitLab shows it.
    Code,
    /// Not at all.
    Hide,
    /// Not recognised: the document is Markdown from its first line.
    Off,
}

impl FrontMatter {
    /// How the renderer shows recognised front matter; `None` when it is
    /// not to be recognised.
    pub fn style(self) -> Option<FrontMatterStyle> {
        match self {
            Self::Table => Some(FrontMatterStyle::Table),
            Self::Code => Some(FrontMatterStyle::Code),
            Self::Hide => Some(FrontMatterStyle::Hide),
            Self::Off => None,
        }
    }
}

/// Whether kitty's graphics sequences are wrapped in tmux's passthrough,
/// so that they reach the terminal outside tmux, as `kitten icat
/// --passthrough` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Passthrough {
    /// Wrapped inside tmux, which also gets the pane's `allow-passthrough`
    /// turned on when it is off.
    #[default]
    Detect,
    /// Never wrapped, and tmux is left alone.
    None,
    /// Always wrapped, and tmux is left alone.
    Tmux,
}

/// Where a setting comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// kiri's default.
    Default,
    /// The configuration file.
    File,
    /// The environment variable of this name.
    Environment(&'static str),
    /// The command-line flag of this name.
    Flag(&'static str),
}

/// A setting's value and where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting<T> {
    pub value: T,
    pub origin: Origin,
}

impl<T> Setting<T> {
    pub fn new(value: T, origin: Origin) -> Self {
        Self { value, origin }
    }

    /// `f` of the value, from the same place.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Setting<U> {
        Setting::new(f(self.value), self.origin)
    }
}

/// Where the theme comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSource {
    /// A theme file the user chose; it must exist.
    File(PathBuf),
    /// `theme.toml` in the configuration directory, which exists.
    DefaultFile(PathBuf),
    /// The built-in theme, without a theme file.
    BuiltIn,
    /// A built-in paper theme, from `--light` or `--dark`.
    Paper(Paper),
}

/// A setting whose values have names, such as `table`: the names that the
/// command line, the environment and the configuration file take, and
/// that the `--doctor` report shows.
pub trait Named: Copy + PartialEq + 'static {
    /// Every value, in the order the help and the warnings list them.
    const ALL: &'static [Self];
    /// The name of each value in `ALL`, in the same order.
    const NAMES: &'static [&'static str];
    /// More names that an environment variable takes, such as `1` for
    /// `always`.
    const ALIASES: &'static [(&'static str, Self)] = &[];

    /// The value as kiri writes it.
    fn name(self) -> &'static str {
        let at = Self::ALL.iter().position(|&value| value == self);
        Self::NAMES[at.expect("ALL holds every value")]
    }

    /// The value called `name`, if one is.
    fn named(name: &str) -> Option<Self> {
        let at = Self::NAMES.iter().position(|&known| known == name)?;
        Some(Self::ALL[at])
    }
}

impl Named for Paging {
    const ALL: &'static [Self] = &[Self::Always, Self::Auto, Self::Never];
    const NAMES: &'static [&'static str] = &["always", "auto", "never"];
    const ALIASES: &'static [(&'static str, Self)] = &[("1", Self::Always), ("0", Self::Never)];
}

impl Named for SearchCase {
    const ALL: &'static [Self] = &[Self::Smart, Self::Literal];
    const NAMES: &'static [&'static str] = &["smart", "literal"];
}

impl Named for Follow {
    const ALL: &'static [Self] = &[Self::On, Self::Off, Self::Auto];
    const NAMES: &'static [&'static str] = &["on", "off", "auto"];
    const ALIASES: &'static [(&'static str, Self)] = &[("1", Self::On), ("0", Self::Off)];
}

impl Named for FollowJump {
    const ALL: &'static [Self] = &[Self::First, Self::Last, Self::Most];
    const NAMES: &'static [&'static str] = &["first", "last", "most"];
}

impl Named for InitialLoad {
    const ALL: &'static [Self] = &[Self::Wait, Self::Background];
    const NAMES: &'static [&'static str] = &["wait", "background"];
}

impl Named for Passthrough {
    const ALL: &'static [Self] = &[Self::Detect, Self::None, Self::Tmux];
    const NAMES: &'static [&'static str] = &["detect", "none", "tmux"];
}

impl Named for FrontMatter {
    const ALL: &'static [Self] = &[Self::Table, Self::Code, Self::Hide, Self::Off];
    const NAMES: &'static [&'static str] = &["table", "code", "hide", "off"];
}

impl Named for Headings {
    const ALL: &'static [Self] = &[Self::Auto, Self::Sized, Self::Images, Self::Plain];
    const NAMES: &'static [&'static str] = &["auto", "sized", "images", "plain"];
}

impl Named for TableRows {
    const ALL: &'static [Self] = &[
        Self::Off,
        Self::Zebra,
        Self::Lines,
        Self::Indent,
        Self::Underline,
    ];
    const NAMES: &'static [&'static str] = &["off", "zebra", "lines", "indent", "underline"];
}

impl Named for RenderErrors {
    const ALL: &'static [Self] = &[Self::Inline, Self::Stderr, Self::Both];
    const NAMES: &'static [&'static str] = &["inline", "stderr", "both"];
}

impl Named for crate::term::Mode {
    const ALL: &'static [Self] = &[Self::Auto, Self::Plain, Self::Ansi, Self::Kitty];
    const NAMES: &'static [&'static str] = &["auto", "plain", "ansi", "kitty"];
}

/// An on or off switch, such as `diagrams`.
impl Named for bool {
    const ALL: &'static [Self] = &[true, false];
    const NAMES: &'static [&'static str] = &["on", "off"];
    const ALIASES: &'static [(&'static str, Self)] = &[("1", true), ("0", false)];
}
