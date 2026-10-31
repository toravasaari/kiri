//! The setting types: where a value comes from, the names a setting's
//! values have, and the types of the settings that are not the pager's
//! (`Paging`, `SearchCase`, `Follow` and `FollowJump` are in
//! `src/session/settings.rs`).

use std::fmt;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde::de::{self, DeserializeSeed, Deserializer, EnumAccess, VariantAccess};

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

/// Whether a theme value is a name, such as `dark`: it has no path
/// separator, no `.toml` ending and no leading `~`.
pub(super) fn is_name(value: &Path) -> bool {
    let text = value.to_string_lossy();
    let path = text.contains(std::path::is_separator) || text.ends_with(".toml");
    !(text.is_empty() || path || text.starts_with('~'))
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
    /// A theme the user chose by name: `NAME.toml` in the themes directory.
    Named { name: String, path: PathBuf },
    /// A built-in paper theme, `light` or `dark`, chosen by name.
    Paper(Paper),
    /// A name that is no theme; `message` tells which names are.
    Unknown { name: String, message: String },
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

/// The environment variable a setting reads, and whether its value means
/// the opposite, as `KIRI_NO_IMAGES=1` means images off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Env(Option<(&'static str, bool)>);

impl Env {
    /// The setting has no environment variable.
    pub const NONE: Self = Self(None);

    /// The variable `name`, with the setting's own values.
    pub const fn is(name: &'static str) -> Self {
        Self(Some((name, false)))
    }

    /// The variable `name`, whose switch means the opposite of the
    /// setting's.
    pub const fn not(name: &'static str) -> Self {
        Self(Some((name, true)))
    }

    /// The variable's name.
    pub const fn name(self) -> Option<&'static str> {
        match self.0 {
            Some((name, _)) => Some(name),
            None => None,
        }
    }

    /// Whether the variable's value means the opposite.
    pub const fn inverted(self) -> bool {
        matches!(self.0, Some((_, true)))
    }
}

/// A value a setting can have, as the command line and the environment
/// write it in text and as the configuration file writes it.
pub trait Value: Sized + Clone + PartialEq + 'static {
    /// The value that `text` is, if it is one.
    fn from_text(text: &str) -> Option<Self>;

    /// What the values are, for the warning about a wrong one: "one of" is
    /// not part of it, as in `always, auto, never, 1 or 0`.
    fn expected() -> String;

    /// The value in the configuration file, with serde's error when it is
    /// not one.
    fn from_file<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>;

    /// The opposite value, for a variable that says the opposite. Only a
    /// switch has one.
    fn inverted(self) -> Self {
        self
    }
}

/// The value of a setting that takes text, such as `--width`, for the
/// command line's parser.
pub fn text<T: Value>(value: &str) -> Result<T, String> {
    T::from_text(value).ok_or_else(|| format!("give {}", T::expected()))
}

/// `Value` for a type with names, by its `Named` impl.
macro_rules! named_values {
    ($($type:ty),* $(,)?) => {
        $(impl Value for $type {
            fn from_text(text: &str) -> Option<Self> {
                named_text(text)
            }

            fn expected() -> String {
                named_expected::<Self>()
            }

            fn from_file<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                named_file(deserializer)
            }
        })*
    };
}

named_values!(
    Paging,
    SearchCase,
    Follow,
    FollowJump,
    InitialLoad,
    Passthrough,
    FrontMatter,
    Headings,
    TableRows,
    RenderErrors,
    crate::term::Mode,
);

/// The value of `T` that `text` names, or has an alias for.
fn named_text<T: Named>(text: &str) -> Option<T> {
    let alias = || {
        let mut aliases = T::ALIASES.iter();
        aliases
            .find(|(alias, _)| *alias == text)
            .map(|&(_, value)| value)
    };
    T::named(text).or_else(alias)
}

/// The names and the aliases of `T`'s values, as a list that ends in `or`.
fn named_expected<T: Named>() -> String {
    let aliases = T::ALIASES.iter().map(|&(alias, _)| alias);
    let names: Vec<&str> = T::NAMES.iter().copied().chain(aliases).collect();
    let (last, rest) = names.split_last().expect("a setting has values");
    format!("{} or {last}", rest.join(", "))
}

/// An on or off switch: `on` or `off`, or `1` or `0` in text, and true or
/// false in the file.
impl Value for bool {
    fn from_text(text: &str) -> Option<Self> {
        named_text(text)
    }

    fn expected() -> String {
        named_expected::<Self>()
    }

    fn from_file<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        bool::deserialize(deserializer)
    }

    fn inverted(self) -> Self {
        !self
    }
}

/// A setting that may stay unset, such as the width, which is the
/// terminal's unless given.
impl<T: Value> Value for Option<T> {
    fn from_text(text: &str) -> Option<Self> {
        T::from_text(text).map(Some)
    }

    fn expected() -> String {
        T::expected()
    }

    fn from_file<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::from_file(deserializer).map(Some)
    }
}

/// A width in cells.
impl Value for u16 {
    fn from_text(text: &str) -> Option<Self> {
        text.parse().ok()
    }

    fn expected() -> String {
        "a whole number of cells".to_owned()
    }

    fn from_file<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        u16::deserialize(deserializer)
    }
}

/// The largest image file kiri reads, in whole megabytes, from 1 to 100.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Megabytes(pub u64);

impl Value for Megabytes {
    fn from_text(text: &str) -> Option<Self> {
        match text.parse() {
            Ok(megabytes @ 1..=100) => Some(Self(megabytes)),
            _ => None,
        }
    }

    fn expected() -> String {
        "a whole number of MB from 1 to 100".to_owned()
    }

    fn from_file<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let megabytes = u64::deserialize(deserializer)?;
        Self::from_text(&megabytes.to_string())
            .ok_or_else(|| de::Error::custom(format!("max_image_size: give {}", Self::expected())))
    }
}

/// A time in seconds: a positive number, with a fraction if need be.
impl Value for Duration {
    fn from_text(text: &str) -> Option<Self> {
        seconds(text.parse().ok()?)
    }

    fn expected() -> String {
        "a positive number of seconds".to_owned()
    }

    fn from_file<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let seconds = f64::deserialize(deserializer).ok().and_then(seconds);
        let error = "timeout: give a positive number of seconds";
        seconds.ok_or_else(|| de::Error::custom(error))
    }
}

/// `seconds` as a duration, when it is a positive number of them that a
/// duration can hold.
fn seconds(seconds: f64) -> Option<Duration> {
    (seconds > 0.0)
        .then(|| Duration::try_from_secs_f64(seconds).ok())
        .flatten()
}

/// A `T` in the configuration file, for serde.
pub struct File<T>(pub T);

impl<'de, T: Value> Deserialize<'de> for File<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::from_file(deserializer).map(File)
    }
}

/// The value of a key that may be left out, for serde.
pub fn file_value<'de, D: Deserializer<'de>, T: Value>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::from_file(deserializer).map(Some)
}

/// A value of `T` by its name, as serde reads an enum of unit variants.
fn named_file<'de, D: Deserializer<'de>, T: Named>(deserializer: D) -> Result<T, D::Error> {
    struct Visitor<T>(PhantomData<T>);

    impl<'de, T: Named> de::Visitor<'de> for Visitor<T> {
        type Value = T;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("one of ")?;
            f.write_str(&T::NAMES.join(", "))
        }

        fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<T, A::Error> {
            let (value, variant) = data.variant_seed(Name(PhantomData))?;
            variant.unit_variant()?;
            Ok(value)
        }
    }

    /// The name of the variant, which must be one of `T`'s.
    struct Name<T>(PhantomData<T>);

    impl<'de, T: Named> DeserializeSeed<'de> for Name<T> {
        type Value = T;

        fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<T, D::Error> {
            deserializer.deserialize_identifier(self)
        }
    }

    impl<'de, T: Named> de::Visitor<'de> for Name<T> {
        type Value = T;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("variant identifier")
        }

        fn visit_str<E: de::Error>(self, name: &str) -> Result<T, E> {
            T::named(name).ok_or_else(|| E::unknown_variant(name, T::NAMES))
        }
    }

    deserializer.deserialize_enum("", T::NAMES, Visitor(PhantomData))
}
