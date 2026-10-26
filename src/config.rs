//! Settings from the command line, the environment and the configuration
//! file, read once at start-up. The environment also holds the variables
//! that capability detection reads.
//!
//! Everything here takes the environment as a value, so the rules can be
//! tested without touching the process environment.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::ErrorKind;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde::de::{self, Deserializer, EnumAccess, MapAccess, VariantAccess};

use crate::render::{FrontMatterStyle, Headings, RenderErrors, TableRows};
use crate::theme::{Paper, Theme};

/// `Environment`, with a field for each variable, from one table, so that
/// a variable cannot be read without `get` knowing it.
macro_rules! environment {
    ($($(#[$doc:meta])* $field:ident: $name:literal,)*) => {
        /// The environment variables kiri reads for its settings.
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        pub struct Environment {
            $($(#[$doc])* pub $field: Option<OsString>,)*
        }

        impl Environment {
            /// The environment with each variable from `var`.
            fn read(var: impl Fn(&'static str) -> Option<OsString>) -> Self {
                Self {
                    $($field: var($name),)*
                }
            }

            /// The value of the variable `name`, of those kiri reads.
            pub fn get(&self, name: &str) -> Option<&OsStr> {
                match name {
                    $($name => self.$field.as_deref(),)*
                    _ => None,
                }
            }
        }
    };
}

environment! {
    /// `HOME`.
    home: "HOME",
    /// `XDG_CONFIG_HOME`: where kiri's configuration directory goes.
    xdg_config_home: "XDG_CONFIG_HOME",
    /// `XDG_CACHE_HOME`: where kiri's cache directory goes.
    xdg_cache_home: "XDG_CACHE_HOME",
    /// `TMPDIR`: the system's temporary directory, where the files of the
    /// programs that draw diagrams go when the cache directory cannot be
    /// used.
    tmpdir: "TMPDIR",
    /// `KIRI_CONFIG`: the configuration file to read instead of the default.
    kiri_config: "KIRI_CONFIG",
    /// `KIRI_THEME`: a theme file.
    kiri_theme: "KIRI_THEME",
    /// `KIRI_NO_IMAGES`: images off unless empty or `0`.
    kiri_no_images: "KIRI_NO_IMAGES",
    /// `KIRI_PAGING`: `always`, `auto` or `never`, or `1` or `0`.
    kiri_paging: "KIRI_PAGING",
    /// `KIRI_FOLLOW`: `on`, `off` or `auto`, or `1` or `0`.
    kiri_follow: "KIRI_FOLLOW",
    /// `KIRI_FOLLOW_JUMP`: `first`, `last` or `most`.
    kiri_follow_jump: "KIRI_FOLLOW_JUMP",
    /// `KIRI_INITIAL_LOAD`: `wait` or `background`.
    kiri_initial_load: "KIRI_INITIAL_LOAD",
    /// `KIRI_PASSTHROUGH`: `detect`, `none` or `tmux`.
    kiri_passthrough: "KIRI_PASSTHROUGH",
    /// `KIRI_FRONT_MATTER`: `table`, `code`, `hide` or `off`.
    kiri_front_matter: "KIRI_FRONT_MATTER",
    /// `KIRI_HEADINGS`: `auto`, `sized`, `images` or `plain`.
    kiri_headings: "KIRI_HEADINGS",
    /// `KIRI_TABLE_ROWS`: `off`, `zebra`, `lines`, `indent` or `underline`.
    kiri_table_rows: "KIRI_TABLE_ROWS",
    /// `KIRI_DIAGRAMS`: `on` or `off`, or `1` or `0`.
    kiri_diagrams: "KIRI_DIAGRAMS",
    /// `KIRI_RENDER_ERRORS`: `inline`, `stderr` or `both`.
    kiri_render_errors: "KIRI_RENDER_ERRORS",
    /// `PATH`, where kiri looks for a program that draws diagrams.
    path: "PATH",
    /// `NVM_DIR`: where nvm keeps the Node versions it installs.
    nvm_dir: "NVM_DIR",
    /// `NPM_CONFIG_PREFIX`, or `npm_config_prefix`: npm's prefix for the
    /// programs it installs.
    npm_config_prefix: "NPM_CONFIG_PREFIX",
    /// `NO_COLOR`: colours off unless empty.
    no_color: "NO_COLOR",
    /// `COLORTERM`: 24-bit colour when `truecolor` or `24bit`.
    colorterm: "COLORTERM",
    /// `TERM`: the terminal type, which only `--doctor` reports.
    term: "TERM",
    /// `TERM_PROGRAM`: names some of the terminals known to support
    /// hyperlinks.
    term_program: "TERM_PROGRAM",
    /// `VTE_VERSION`: terminals built on VTE, with hyperlinks from 5000 on.
    vte_version: "VTE_VERSION",
    /// `TMUX`: set and not empty inside tmux.
    tmux: "TMUX",
    /// `TMUX_PANE`: the tmux pane kiri runs in, such as `%3`.
    tmux_pane: "TMUX_PANE",
}

impl Environment {
    /// The environment of this process.
    pub fn from_process() -> Self {
        let mut env = Self::read(std::env::var_os);
        // npm reads its settings from the environment in either case.
        if env.npm_config_prefix.is_none() {
            env.npm_config_prefix = std::env::var_os("npm_config_prefix");
        }
        env
    }
}

/// The home directory, from `HOME`; `None` when that is unset, empty or
/// not an absolute path.
pub fn home_directory(env: &Environment) -> Option<PathBuf> {
    (env.home.as_ref().map(PathBuf::from)).filter(|home| home.is_absolute())
}

/// A directory and where it comes from: the variable `name` when it is
/// set to a path that `valid` takes, else `fallback`, kiri's default.
fn directory_in(
    env: &Environment,
    name: &'static str,
    valid: fn(&Path) -> bool,
    fallback: Option<PathBuf>,
) -> Option<Setting<PathBuf>> {
    match env.get(name).map(Path::new).filter(|dir| valid(dir)) {
        Some(dir) => Some(Setting::new(dir.to_owned(), Origin::Environment(name))),
        None => fallback.map(|dir| Setting::new(dir, Origin::Default)),
    }
}

/// kiri's configuration directory and where it comes from:
/// `$XDG_CONFIG_HOME/kiri`, or `~/.config/kiri` when that is unset, empty
/// or relative, which the XDG base directory specification calls invalid.
/// Without either, there is none.
pub fn directory(env: &Environment) -> Option<Setting<PathBuf>> {
    let home = home_directory(env).map(|home| home.join(".config"));
    let config = directory_in(env, "XDG_CONFIG_HOME", Path::is_absolute, home)?;
    Some(Setting::new(config.value.join("kiri"), config.origin))
}

/// kiri's cache directory, for files it may remove at any time, such as
/// those of the programs that draw diagrams.
pub fn cache_directory(env: &Environment) -> Option<PathBuf> {
    cache_directory_setting(env).map(|dir| dir.value)
}

/// kiri's cache directory and where it comes from: `$XDG_CACHE_HOME/kiri`,
/// or `~/.cache/kiri` when that is unset, empty or relative, which the XDG
/// base directory specification calls invalid.
pub fn cache_directory_setting(env: &Environment) -> Option<Setting<PathBuf>> {
    let home = home_directory(env).map(|home| home.join(".cache"));
    let cache = directory_in(env, "XDG_CACHE_HOME", Path::is_absolute, home)?;
    Some(Setting::new(cache.value.join("kiri"), cache.origin))
}

/// The system's temporary directory.
pub fn temp_directory(env: &Environment) -> PathBuf {
    temp_directory_setting(env).value
}

/// The system's temporary directory, as the standard library finds it,
/// and where it comes from: `TMPDIR`, or `/tmp` when that is unset or
/// empty.
pub fn temp_directory_setting(env: &Environment) -> Setting<PathBuf> {
    let not_empty = |dir: &Path| !dir.as_os_str().is_empty();
    let tmp = Some(PathBuf::from("/tmp"));
    directory_in(env, "TMPDIR", not_empty, tmp).expect("/tmp is a fallback")
}

/// What the configuration file sets; `None` for what it leaves out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    /// A theme file, relative to the configuration file's directory unless
    /// it starts with `~/`, the home directory.
    pub theme: Option<PathBuf>,
    /// Whether images are shown, or only their alt text.
    pub images: Option<bool>,
    /// When the document is shown in the pager.
    pub paging: Option<Paging>,
    /// How a search in the pager treats case.
    pub search_case: Option<SearchCase>,
    /// Whether the pager follows a file, reading it again whenever it
    /// changes.
    pub follow: Option<Follow>,
    /// Which change auto-follow moves the view to.
    pub follow_jump: Option<FollowJump>,
    /// Whether the pager waits for the images before it opens.
    pub initial_load: Option<InitialLoad>,
    /// Whether kitty's graphics are wrapped for tmux.
    pub passthrough: Option<Passthrough>,
    /// How front matter shows.
    pub front_matter: Option<FrontMatter>,
    /// How top-level headings show.
    pub headings: Option<Headings>,
    /// How the rows of a table are told apart.
    pub table_rows: Option<TableRows>,
    /// Whether diagrams in code blocks are drawn, or shown as code:
    /// `enabled` in the `[diagrams]` table, or `diagrams` on its own.
    pub diagrams: Option<bool>,
    /// The external command for each diagram language that has one, from
    /// `[diagrams.<language>]`: the program and its arguments, with
    /// `{input}`, `{output}`, `{theme}`, `{config}` and `{scale}` in them.
    pub diagram_commands: BTreeMap<String, Vec<String>>,
    /// How long an external command may take to draw one diagram.
    pub diagram_timeout: Option<Duration>,
    /// Whether a diagram in colours for the other page, light or dark,
    /// lies on a panel: `background_panel` in the `[diagrams]` table.
    pub diagram_panel: Option<bool>,
    /// The diagram languages whose program kiri is not to look for.
    pub diagram_no_search: BTreeSet<String>,
    /// Where kiri tells why a diagram could not be drawn.
    pub render_errors: Option<RenderErrors>,
}

/// When the document is shown in the pager rather than printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Paging {
    /// Whenever the output is a terminal.
    Always,
    /// When the output is a terminal and the document is longer than the
    /// window, unless it comes from standard input or is the `--doctor`
    /// report.
    #[default]
    Auto,
    /// Never: the document is printed.
    Never,
}

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

/// How a search in the pager treats upper and lower case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchCase {
    /// A search term with no upper-case letter ignores case; one with an
    /// upper-case letter matches case exactly.
    #[default]
    Smart,
    /// Case matches exactly.
    Literal,
}

/// Whether the pager follows a lone file, reading it again whenever it
/// changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Follow {
    #[default]
    Off,
    /// Read again, with the view kept where it is.
    On,
    /// Read again, with the view moved to a change.
    Auto,
}

/// Which change auto-follow moves the view to, after a reload that
/// changed the document in more than one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FollowJump {
    /// The first change in the document.
    #[default]
    First,
    /// The last change in the document.
    Last,
    /// The change that takes the most rows.
    Most,
}

/// Whether the pager, when it surely opens, waits for the images and
/// diagrams of the document before it shows it, or shows the text at
/// once while they load in the background, as after a reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InitialLoad {
    /// Every image is loaded before the document shows.
    #[default]
    Wait,
    /// The text shows at once, and the images as they come.
    Background,
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

/// Where the configuration file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFile {
    pub path: PathBuf,
    /// The user named the file, so it must exist; the default file need
    /// not.
    pub required: bool,
    /// Where the path comes from.
    pub origin: Origin,
}

/// `KIRI_CONFIG` when it is set and not empty, else `config.toml` in the
/// configuration directory.
pub fn file(env: &Environment) -> Option<ConfigFile> {
    if let Some(path) = env.kiri_config.as_ref().filter(|path| !path.is_empty()) {
        return Some(ConfigFile {
            path: PathBuf::from(path),
            required: true,
            origin: Origin::Environment("KIRI_CONFIG"),
        });
    }
    let dir = directory(env)?;
    Some(ConfigFile {
        path: dir.value.join("config.toml"),
        required: false,
        origin: dir.origin,
    })
}

/// Reads the configuration file, if there is one. A file that `KIRI_CONFIG`
/// names must exist; a mistake in any file is an error.
pub fn load(env: &Environment) -> Result<Config, String> {
    let Some(file) = file(env) else {
        return Ok(Config::default());
    };
    let path = &file.path;
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound && !file.required => {
            return Ok(Config::default());
        }
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let dir = path.parent().unwrap_or(Path::new(""));
    parse(&text, dir, env).map_err(|error| format!("{}: {error}", path.display()))
}

/// Parses a configuration file in `dir`.
fn parse(text: &str, dir: &Path, env: &Environment) -> Result<Config, String> {
    let file: ConfigToml = toml::from_str(text).map_err(|error| error.to_string())?;
    let theme = match file.theme {
        Some(theme) => Some(match theme.strip_prefix("~") {
            Ok(rest) if !rest.as_os_str().is_empty() => match home_directory(env) {
                Some(home) => home.join(rest),
                None => return Err(format!("theme {}: HOME is not set", theme.display())),
            },
            _ => dir.join(theme),
        }),
        None => None,
    };
    Ok(Config {
        theme,
        images: file.images,
        paging: file.paging,
        search_case: file.search_case,
        follow: file.follow,
        follow_jump: file.follow_jump,
        initial_load: file.initial_load,
        passthrough: file.passthrough,
        front_matter: file.front_matter,
        headings: file.headings,
        table_rows: file.table_rows,
        diagrams: file.diagrams.enabled,
        diagram_commands: file.diagrams.commands,
        diagram_timeout: file.diagrams.timeout,
        diagram_panel: file.diagrams.background_panel,
        diagram_no_search: file.diagrams.no_search,
        render_errors: file.render_errors,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigToml {
    theme: Option<PathBuf>,
    images: Option<bool>,
    #[serde(default, deserialize_with = "named")]
    paging: Option<Paging>,
    #[serde(default, deserialize_with = "named")]
    search_case: Option<SearchCase>,
    #[serde(default, deserialize_with = "named")]
    follow: Option<Follow>,
    #[serde(default, deserialize_with = "named")]
    follow_jump: Option<FollowJump>,
    #[serde(default, deserialize_with = "named")]
    initial_load: Option<InitialLoad>,
    #[serde(default, deserialize_with = "named")]
    passthrough: Option<Passthrough>,
    #[serde(default, deserialize_with = "named")]
    front_matter: Option<FrontMatter>,
    #[serde(default, deserialize_with = "named")]
    headings: Option<Headings>,
    #[serde(default, deserialize_with = "named")]
    table_rows: Option<TableRows>,
    #[serde(default, deserialize_with = "named")]
    render_errors: Option<RenderErrors>,
    #[serde(default)]
    diagrams: DiagramsToml,
}

/// A value of `T` by its name, as serde reads an enum of unit variants.
fn named<'de, D: Deserializer<'de>, T: Named>(deserializer: D) -> Result<Option<T>, D::Error> {
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

    impl<'de, T: Named> de::DeserializeSeed<'de> for Name<T> {
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

    deserializer
        .deserialize_enum("", T::NAMES, Visitor(PhantomData))
        .map(Some)
}

/// The `diagrams` key: a `[diagrams]` table of the settings for every
/// diagram, beside a table for each language, such as
/// `[diagrams.mermaid]`; or `true` or `false`, short for `enabled` in it.
#[derive(Debug, Default)]
struct DiagramsToml {
    enabled: Option<bool>,
    timeout: Option<Duration>,
    background_panel: Option<bool>,
    commands: BTreeMap<String, Vec<String>>,
    no_search: BTreeSet<String>,
}

/// The keys of the `[diagrams]` table other than the languages.
const DIAGRAMS_KEYS: &[&str] = &["enabled", "timeout", "background_panel"];

/// The key in a diagram language's table that turns the search for its
/// program off or on, such as `mmdc` for Mermaid in a build without merman.
pub const AUTO_DETECT: &str = "auto_detect";

/// The keys of a diagram language's table.
const LANGUAGE_KEYS: &[&str] = &["command", AUTO_DETECT];

/// One language's table under `diagrams`.
#[derive(Debug, Default)]
struct DiagramToml {
    command: Option<Vec<String>>,
    auto_detect: Option<bool>,
}

impl<'de> Deserialize<'de> for DiagramToml {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = DiagramToml;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a table such as [diagrams.mermaid]")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<DiagramToml, A::Error> {
                let mut table = DiagramToml::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "command" => {
                            let command: Vec<String> = map.next_value()?;
                            check_command(&command).map_err(de::Error::custom)?;
                            table.command = Some(command);
                        }
                        key if key == AUTO_DETECT => table.auto_detect = Some(map.next_value()?),
                        key => return Err(de::Error::unknown_field(key, LANGUAGE_KEYS)),
                    }
                }
                Ok(table)
            }
        }

        deserializer.deserialize_map(Visitor)
    }
}

impl<'de> Deserialize<'de> for DiagramsToml {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = DiagramsToml;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("true or false, or a [diagrams] table")
            }

            fn visit_bool<E: de::Error>(self, on: bool) -> Result<DiagramsToml, E> {
                Ok(DiagramsToml {
                    enabled: Some(on),
                    ..DiagramsToml::default()
                })
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<DiagramsToml, A::Error> {
                let mut diagrams = DiagramsToml::default();
                // The key each language's table was given under.
                let mut tables = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "enabled" => diagrams.enabled = Some(map.next_value()?),
                        "background_panel" => diagrams.background_panel = Some(map.next_value()?),
                        "timeout" => {
                            let seconds = map.next_value().ok().and_then(seconds);
                            let error = "timeout: give a positive number of seconds";
                            let seconds = seconds.ok_or_else(|| de::Error::custom(error))?;
                            diagrams.timeout = Some(seconds);
                        }
                        // In any case, or by an alias, as code blocks name
                        // it; kept under the language's own name.
                        key if let Some(language) = crate::diagrams::language(key) => {
                            if let Some(earlier) = tables.insert(language.name, key.to_owned()) {
                                return Err(de::Error::custom(format!(
                                    "`{earlier}` and `{key}` are tables for the same diagram \
                                     language; give only one"
                                )));
                            }
                            let language = language.name.to_owned();
                            let table: DiagramToml = map.next_value()?;
                            if table.auto_detect == Some(false) {
                                diagrams.no_search.insert(language.clone());
                            }
                            if let Some(command) = table.command {
                                diagrams.commands.insert(language, command);
                            }
                        }
                        _ => {
                            let languages = crate::diagrams::LANGUAGES.iter().map(|l| l.name);
                            let keys = DIAGRAMS_KEYS.iter().copied().chain(languages);
                            let keys: Vec<String> = keys.map(|key| format!("`{key}`")).collect();
                            return Err(de::Error::custom(format!(
                                "unknown field `{key}`, expected one of {}",
                                keys.join(", ")
                            )));
                        }
                    }
                }
                Ok(diagrams)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// `seconds` as a duration, when it is a positive number of them that a
/// duration can hold.
fn seconds(seconds: f64) -> Option<Duration> {
    (seconds > 0.0)
        .then(|| Duration::try_from_secs_f64(seconds).ok())
        .flatten()
}

/// Why `command`, a diagram language's external command, cannot work.
fn check_command(command: &[String]) -> Result<(), String> {
    match command.first() {
        None => Err("command: give the program and its arguments".to_owned()),
        Some(program) if program.is_empty() => Err("command: the program is empty".to_owned()),
        Some(_) => {
            let has = |name: &str| command.iter().any(|arg| arg.contains(name));
            match ["{input}", "{output}"].into_iter().find(|name| !has(name)) {
                Some(missing) => Err(format!(
                    "command: give {missing} where the program takes its {} file",
                    &missing[1..missing.len() - 1]
                )),
                None => Ok(()),
            }
        }
    }
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

/// Chooses the theme and tells where it comes from, strongest first:
/// `--theme`, a paper theme from `--light` or `--dark`, `KIRI_THEME` when it
/// is not empty, the configuration file, `theme.toml` in the configuration
/// directory when it exists, the built-in theme.
fn theme_source(options: &Options, env: &Environment, config: &Config) -> Setting<ThemeSource> {
    let theme = (options.theme.clone()).map(|theme| theme.map(ThemeSource::File));
    let paper = (options.paper.clone()).map(|paper| paper.map(ThemeSource::Paper));
    let name = "KIRI_THEME";
    let variable = |_: &mut Vec<String>| {
        let path = env.get(name).filter(|path| !path.is_empty())?;
        let theme = ThemeSource::File(PathBuf::from(path));
        Some(Setting::new(theme, Origin::Environment(name)))
    };
    let file = directory(env).map(|dir| dir.value.join("theme.toml"));
    let default = match file.filter(|path| path.exists()) {
        Some(path) => ThemeSource::DefaultFile(path),
        None => ThemeSource::BuiltIn,
    };
    let file = config.theme.clone().map(ThemeSource::File);
    resolve(theme.or(paper), variable, file, default, &mut Vec::new())
}

/// Whether images are shown and where that comes from, strongest first:
/// the command line (`flag`, from `--images` or `--no-images`), then
/// `KIRI_NO_IMAGES`, then the configuration file; on unless one turns them
/// off. `KIRI_NO_IMAGES` turns them off when it is set to anything but an
/// empty value or `0`, and `0` turns them on whatever the configuration
/// file says.
fn images(flag: Option<Setting<bool>>, env: &Environment, config: &Config) -> Setting<bool> {
    let name = "KIRI_NO_IMAGES";
    let variable = |_: &mut Vec<String>| {
        let value = env.get(name).filter(|value| !value.is_empty())?;
        Some(Setting::new(value == "0", Origin::Environment(name)))
    };
    resolve(flag, variable, config.images, true, &mut Vec::new())
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

/// A setting and where it comes from, strongest first: the command line
/// (`flag`), the environment (`variable`, read only without the flag),
/// the configuration file (`file`), `default`. What the variable has to
/// say about a value it leaves out goes in `warnings`.
fn resolve<T>(
    flag: Option<Setting<T>>,
    variable: impl FnOnce(&mut Vec<String>) -> Option<Setting<T>>,
    file: Option<T>,
    default: T,
    warnings: &mut Vec<String>,
) -> Setting<T> {
    flag.or_else(|| variable(warnings))
        .or_else(|| file.map(|value| Setting::new(value, Origin::File)))
        .unwrap_or_else(|| Setting::new(default, Origin::Default))
}

/// No environment variable, for `resolve`.
fn unset<T>(_: &mut Vec<String>) -> Option<Setting<T>> {
    None
}

/// The environment variable `name`, for `resolve`, which takes the names
/// and the aliases of `T`'s values; empty is unset. Any other value is
/// left out with a warning.
fn variable<T: Named>(
    env: &Environment,
    name: &'static str,
) -> impl FnOnce(&mut Vec<String>) -> Option<Setting<T>> {
    move |warnings| {
        let value = env.get(name).filter(|value| !value.is_empty())?;
        let alias = |text: &str| {
            let mut aliases = T::ALIASES.iter();
            aliases
                .find(|(alias, _)| *alias == text)
                .map(|&(_, value)| value)
        };
        let chosen = value
            .to_str()
            .and_then(|text| T::named(text).or_else(|| alias(text)));
        if chosen.is_none() {
            let aliases = T::ALIASES.iter().map(|&(alias, _)| alias);
            let names: Vec<&str> = T::NAMES.iter().copied().chain(aliases).collect();
            let (last, rest) = names.split_last().expect("a setting has values");
            warnings.push(format!(
                "{name}: `{}` is not {} or {last}; left out",
                value.to_string_lossy(),
                rest.join(", "),
            ));
        }
        chosen.map(|value| Setting::new(value, Origin::Environment(name)))
    }
}

/// What the command line asks for, each with the flag it comes from;
/// `None` where it leaves the choice to the environment and the
/// configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// A theme file.
    pub theme: Option<Setting<PathBuf>>,
    /// A paper theme.
    pub paper: Option<Setting<Paper>>,
    /// Whether the theme keeps its margins.
    pub margins: Option<Setting<bool>>,
    /// Whether images are shown.
    pub images: Option<Setting<bool>>,
    /// When the document is paged.
    pub paging: Option<Setting<Paging>>,
    /// Whether the pager follows a file.
    pub follow: Option<Setting<Follow>>,
    /// Which change auto-follow moves the view to.
    pub follow_jump: Option<Setting<FollowJump>>,
    /// Whether the pager waits for the images before it opens.
    pub initial_load: Option<Setting<InitialLoad>>,
    /// Whether kitty's graphics are wrapped for tmux.
    pub passthrough: Option<Setting<Passthrough>>,
    /// How front matter shows.
    pub front_matter: Option<Setting<FrontMatter>>,
    /// How top-level headings show.
    pub headings: Option<Setting<Headings>>,
    /// How the rows of a table are told apart.
    pub table_rows: Option<Setting<TableRows>>,
    /// Whether diagrams in code blocks are drawn.
    pub diagrams: Option<Setting<bool>>,
    /// Where kiri tells why a diagram could not be drawn.
    pub render_errors: Option<Setting<RenderErrors>>,
}

/// What the options, the environment and the configuration file choose
/// together.
#[derive(Debug)]
pub struct Settings {
    /// The theme, loaded, without margins if the options say so.
    pub theme: Theme,
    /// The paper from `--light` or `--dark`, which also decides whether
    /// the background counts as dark.
    pub paper: Option<Paper>,
    /// Whether images are shown.
    pub images: bool,
    /// When the document is paged.
    pub paging: Paging,
    /// How a search in the pager treats case.
    pub search_case: SearchCase,
    /// Whether the pager follows a file, reading it again whenever it
    /// changes.
    pub follow: Follow,
    /// Which change auto-follow moves the view to.
    pub follow_jump: FollowJump,
    /// Whether the pager waits for the images before it opens.
    pub initial_load: InitialLoad,
    /// Whether kitty's graphics are wrapped for tmux.
    pub passthrough: Passthrough,
    /// How front matter shows.
    pub front_matter: FrontMatter,
    /// How top-level headings show.
    pub headings: Headings,
    /// How the rows of a table are told apart.
    pub table_rows: TableRows,
    /// Whether diagrams in code blocks are drawn, or shown as code.
    pub diagrams: bool,
    /// The external command for each diagram language that has one.
    pub diagram_commands: BTreeMap<String, Vec<String>>,
    /// How long an external command may take to draw one diagram.
    pub diagram_timeout: Duration,
    /// Whether a diagram in colours for the other page lies on a panel.
    pub diagram_panel: bool,
    /// The diagram languages whose program kiri is not to look for.
    pub diagram_no_search: BTreeSet<String>,
    /// Where kiri tells why a diagram could not be drawn.
    pub render_errors: RenderErrors,
}

/// What the options, the environment and the configuration file choose,
/// before the theme is loaded, with where each choice comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choices {
    pub theme: Setting<ThemeSource>,
    /// The paper from `--light` or `--dark`.
    pub paper: Option<Paper>,
    /// Whether the theme keeps its margins.
    pub margins: Setting<bool>,
    /// Whether images are shown.
    pub images: Setting<bool>,
    /// When the document is paged.
    pub paging: Setting<Paging>,
    /// How a search in the pager treats case.
    pub search_case: Setting<SearchCase>,
    /// Whether the pager follows a file.
    pub follow: Setting<Follow>,
    /// Which change auto-follow moves the view to.
    pub follow_jump: Setting<FollowJump>,
    /// Whether the pager waits for the images before it opens.
    pub initial_load: Setting<InitialLoad>,
    /// Whether kitty's graphics are wrapped for tmux.
    pub passthrough: Setting<Passthrough>,
    /// How front matter shows.
    pub front_matter: Setting<FrontMatter>,
    /// How top-level headings show.
    pub headings: Setting<Headings>,
    /// How the rows of a table are told apart.
    pub table_rows: Setting<TableRows>,
    /// Whether diagrams in code blocks are drawn, or shown as code.
    pub diagrams: Setting<bool>,
    /// The external command for each diagram language that has one, all
    /// from the configuration file.
    pub diagram_commands: BTreeMap<String, Vec<String>>,
    /// How long an external command may take to draw one diagram.
    pub diagram_timeout: Setting<Duration>,
    /// Whether a diagram in colours for the other page lies on a panel.
    pub diagram_panel: Setting<bool>,
    /// The diagram languages whose program kiri is not to look for, all
    /// from the configuration file.
    pub diagram_no_search: BTreeSet<String>,
    /// Where kiri tells why a diagram could not be drawn.
    pub render_errors: Setting<RenderErrors>,
    /// What the user should know about settings that were left out.
    pub warnings: Vec<String>,
}

impl Choices {
    /// The settings these choices make with `theme`, the theme they chose,
    /// loaded.
    pub fn with_theme(&self, theme: Theme) -> Settings {
        Settings {
            theme: if self.margins.value {
                theme
            } else {
                theme.without_margins()
            },
            paper: self.paper,
            images: self.images.value,
            paging: self.paging.value,
            search_case: self.search_case.value,
            follow: self.follow.value,
            follow_jump: self.follow_jump.value,
            initial_load: self.initial_load.value,
            passthrough: self.passthrough.value,
            front_matter: self.front_matter.value,
            headings: self.headings.value,
            table_rows: self.table_rows.value,
            diagrams: self.diagrams.value,
            diagram_commands: self.diagram_commands.clone(),
            diagram_timeout: self.diagram_timeout.value,
            diagram_panel: self.diagram_panel.value,
            diagram_no_search: self.diagram_no_search.clone(),
            render_errors: self.render_errors.value,
        }
    }
}

/// What `options`, `env` and `config` choose.
pub fn choose(options: &Options, env: &Environment, config: &Config) -> Choices {
    let mut warnings = Vec::new();
    let w = &mut warnings;
    let paging = resolve(
        options.paging.clone(),
        variable(env, "KIRI_PAGING"),
        config.paging,
        Paging::default(),
        w,
    );
    let front_matter = resolve(
        options.front_matter.clone(),
        variable(env, "KIRI_FRONT_MATTER"),
        config.front_matter,
        FrontMatter::default(),
        w,
    );
    let headings = resolve(
        options.headings.clone(),
        variable(env, "KIRI_HEADINGS"),
        config.headings,
        Headings::default(),
        w,
    );
    let table_rows = resolve(
        options.table_rows.clone(),
        variable(env, "KIRI_TABLE_ROWS"),
        config.table_rows,
        TableRows::default(),
        w,
    );
    let diagrams = resolve(
        options.diagrams.clone(),
        variable(env, "KIRI_DIAGRAMS"),
        config.diagrams,
        true,
        w,
    );
    let render_errors = resolve(
        options.render_errors.clone(),
        variable(env, "KIRI_RENDER_ERRORS"),
        config.render_errors,
        RenderErrors::default(),
        w,
    );
    let time_limit = crate::diagrams::external::TIME_LIMIT;
    Choices {
        theme: theme_source(options, env, config),
        paper: options.paper.as_ref().map(|paper| paper.value),
        margins: resolve(options.margins.clone(), unset, None, true, w),
        images: images(options.images.clone(), env, config),
        paging,
        search_case: resolve(None, unset, config.search_case, SearchCase::default(), w),
        follow: resolve(
            options.follow.clone(),
            variable(env, "KIRI_FOLLOW"),
            config.follow,
            Follow::default(),
            w,
        ),
        follow_jump: resolve(
            options.follow_jump.clone(),
            variable(env, "KIRI_FOLLOW_JUMP"),
            config.follow_jump,
            FollowJump::default(),
            w,
        ),
        initial_load: resolve(
            options.initial_load.clone(),
            variable(env, "KIRI_INITIAL_LOAD"),
            config.initial_load,
            InitialLoad::default(),
            w,
        ),
        passthrough: resolve(
            options.passthrough.clone(),
            variable(env, "KIRI_PASSTHROUGH"),
            config.passthrough,
            Passthrough::default(),
            w,
        ),
        front_matter,
        headings,
        table_rows,
        diagrams,
        diagram_commands: config.diagram_commands.clone(),
        diagram_timeout: resolve(None, unset, config.diagram_timeout, time_limit, w),
        diagram_panel: resolve(None, unset, config.diagram_panel, true, w),
        diagram_no_search: config.diagram_no_search.clone(),
        render_errors,
        warnings,
    }
}

/// The warnings about settings that were left out, and the settings that
/// `options`, `env` and `config` choose, with the theme loaded. The
/// warnings come back even when the theme does not load.
pub fn settings(
    options: &Options,
    env: &Environment,
    config: &Config,
) -> (Vec<String>, Result<Settings, String>) {
    let mut choices = choose(options, env, config);
    let warnings = std::mem::take(&mut choices.warnings);
    let settings = load_theme(&choices.theme.value).map(|theme| choices.with_theme(theme));
    (warnings, settings)
}

/// The theme that `source` names.
pub fn load_theme(source: &ThemeSource) -> Result<Theme, String> {
    match source {
        ThemeSource::File(path) | ThemeSource::DefaultFile(path) => crate::theme::load(path),
        ThemeSource::BuiltIn => Ok(Theme::default()),
        ThemeSource::Paper(paper) => Ok(Theme::paper(*paper)),
    }
}

#[cfg(test)]
mod tests;
