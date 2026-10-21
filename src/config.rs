//! Settings from the command line, the environment and the configuration
//! file, read once at start-up. The environment also holds the variables
//! that capability detection reads.
//!
//! Everything here takes the environment as a value, so the rules can be
//! tested without touching the process environment.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess};

use crate::render::{FrontMatterStyle, RenderErrors, TableRows};
use crate::theme::{Paper, Theme};

/// The environment variables kiri reads for its settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    pub home: Option<OsString>,
    pub xdg_config_home: Option<OsString>,
    /// `XDG_CACHE_HOME`: where kiri's cache directory goes.
    pub xdg_cache_home: Option<OsString>,
    /// `TMPDIR`: the system's temporary directory, where the files of the
    /// programs that draw diagrams go when the cache directory cannot be
    /// used.
    pub tmpdir: Option<OsString>,
    /// `KIRI_CONFIG`: the configuration file to read instead of the default.
    pub kiri_config: Option<OsString>,
    /// `KIRI_THEME`: a theme file.
    pub kiri_theme: Option<OsString>,
    /// `KIRI_NO_IMAGES`: images off unless empty or `0`.
    pub kiri_no_images: Option<OsString>,
    /// `KIRI_PAGING`: `always`, `auto` or `never`, or `1` or `0`.
    pub kiri_paging: Option<OsString>,
    /// `KIRI_FRONT_MATTER`: `table`, `code`, `hide` or `off`.
    pub kiri_front_matter: Option<OsString>,
    /// `KIRI_TABLE_ROWS`: `off`, `zebra`, `lines`, `indent` or `underline`.
    pub kiri_table_rows: Option<OsString>,
    /// `KIRI_DIAGRAMS`: `on` or `off`, or `1` or `0`.
    pub kiri_diagrams: Option<OsString>,
    /// `KIRI_RENDER_ERRORS`: `inline`, `stderr` or `both`.
    pub kiri_render_errors: Option<OsString>,
    /// `PATH`, where kiri looks for a program that draws diagrams.
    pub path: Option<OsString>,
    /// `NVM_DIR`: where nvm keeps the Node versions it installs.
    pub nvm_dir: Option<OsString>,
    /// `NPM_CONFIG_PREFIX`: npm's prefix for the programs it installs.
    pub npm_config_prefix: Option<OsString>,
    /// `NO_COLOR`: colours off unless empty.
    pub no_color: Option<OsString>,
    /// `COLORTERM`: 24-bit colour when `truecolor` or `24bit`.
    pub colorterm: Option<OsString>,
    /// `TERM`: the terminal type, which only `--doctor` reports.
    pub term: Option<OsString>,
    /// `TERM_PROGRAM`: names some of the terminals known to support
    /// hyperlinks.
    pub term_program: Option<OsString>,
    /// `VTE_VERSION`: terminals built on VTE, with hyperlinks from 5000 on.
    pub vte_version: Option<OsString>,
}

impl Environment {
    /// The environment of this process.
    pub fn from_process() -> Self {
        Self {
            home: std::env::var_os("HOME"),
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
            xdg_cache_home: std::env::var_os("XDG_CACHE_HOME"),
            tmpdir: std::env::var_os("TMPDIR"),
            kiri_config: std::env::var_os("KIRI_CONFIG"),
            kiri_theme: std::env::var_os("KIRI_THEME"),
            kiri_no_images: std::env::var_os("KIRI_NO_IMAGES"),
            kiri_paging: std::env::var_os("KIRI_PAGING"),
            kiri_front_matter: std::env::var_os("KIRI_FRONT_MATTER"),
            kiri_table_rows: std::env::var_os("KIRI_TABLE_ROWS"),
            kiri_diagrams: std::env::var_os("KIRI_DIAGRAMS"),
            kiri_render_errors: std::env::var_os("KIRI_RENDER_ERRORS"),
            path: std::env::var_os("PATH"),
            nvm_dir: std::env::var_os("NVM_DIR"),
            // npm reads its settings from the environment in either case.
            npm_config_prefix: std::env::var_os("NPM_CONFIG_PREFIX")
                .or_else(|| std::env::var_os("npm_config_prefix")),
            no_color: std::env::var_os("NO_COLOR"),
            colorterm: std::env::var_os("COLORTERM"),
            term: std::env::var_os("TERM"),
            term_program: std::env::var_os("TERM_PROGRAM"),
            vte_version: std::env::var_os("VTE_VERSION"),
        }
    }
}

/// The home directory, from `HOME`; `None` when that is unset, empty or
/// not an absolute path.
pub fn home_directory(env: &Environment) -> Option<PathBuf> {
    (env.home.as_ref().map(PathBuf::from)).filter(|home| home.is_absolute())
}

/// Where the configuration directory comes from: `XDG_CONFIG_HOME`, or
/// the default under the home directory.
pub fn directory_origin(env: &Environment) -> Origin {
    let set = (env.xdg_config_home.as_deref()).is_some_and(|dir| Path::new(dir).is_absolute());
    match set {
        true => Origin::Environment("XDG_CONFIG_HOME"),
        false => Origin::Default,
    }
}

/// kiri's configuration directory: `$XDG_CONFIG_HOME/kiri`, or
/// `~/.config/kiri` when that is unset, empty or relative, which the XDG
/// base directory specification calls invalid. Without either, there is
/// none.
pub fn directory(env: &Environment) -> Option<PathBuf> {
    let config = env
        .xdg_config_home
        .clone()
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| home_directory(env).map(|home| home.join(".config")))?;
    Some(config.join("kiri"))
}

/// kiri's cache directory, for files it may remove at any time, such as
/// those of the programs that draw diagrams: `$XDG_CACHE_HOME/kiri`, or
/// `~/.cache/kiri` when that is unset, empty or relative, which the XDG
/// base directory specification calls invalid.
pub fn cache_directory(env: &Environment) -> Option<PathBuf> {
    let cache = env
        .xdg_cache_home
        .clone()
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| home_directory(env).map(|home| home.join(".cache")))?;
    Some(cache.join("kiri"))
}

/// Where the cache directory comes from: `XDG_CACHE_HOME`, or the
/// default under the home directory.
pub fn cache_directory_origin(env: &Environment) -> Origin {
    let set = (env.xdg_cache_home.as_deref()).is_some_and(|dir| Path::new(dir).is_absolute());
    match set {
        true => Origin::Environment("XDG_CACHE_HOME"),
        false => Origin::Default,
    }
}

/// The system's temporary directory, as the standard library finds it:
/// `TMPDIR`, or `/tmp` when that is unset or empty.
pub fn temp_directory(env: &Environment) -> PathBuf {
    match env.tmpdir.as_ref().filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from("/tmp"),
    }
}

/// Where the temporary directory comes from: `TMPDIR`, or the default.
pub fn temp_directory_origin(env: &Environment) -> Origin {
    match env.tmpdir.as_ref().filter(|dir| !dir.is_empty()) {
        Some(_) => Origin::Environment("TMPDIR"),
        None => Origin::Default,
    }
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
    /// How front matter shows.
    pub front_matter: Option<FrontMatter>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Paging {
    /// Whenever the output is a terminal.
    Always,
    /// When the output is a terminal and the document is longer than the
    /// window, unless it comes from standard input or is the `--doctor`
    /// report.
    Auto,
    /// Never: the document is printed.
    Never,
}

/// How the front matter at the start of a Markdown document shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrontMatter {
    /// A table of its keys and values, as GitHub shows it.
    Table,
    /// A code block in its language, as GitLab shows it.
    Code,
    /// Not at all.
    Hide,
    /// Not recognised: the document is Markdown from its first line.
    Off,
}

impl FrontMatter {
    /// The value as kiri writes it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Code => "code",
            Self::Hide => "hide",
            Self::Off => "off",
        }
    }

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchCase {
    /// A search term with no upper-case letter ignores case; one with an
    /// upper-case letter matches case exactly.
    Smart,
    /// Case matches exactly.
    Literal,
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
    Some(ConfigFile {
        path: directory(env)?.join("config.toml"),
        required: false,
        origin: directory_origin(env),
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
pub fn parse(text: &str, dir: &Path, env: &Environment) -> Result<Config, String> {
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
        front_matter: file.front_matter,
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
    paging: Option<Paging>,
    search_case: Option<SearchCase>,
    front_matter: Option<FrontMatter>,
    table_rows: Option<TableRows>,
    render_errors: Option<RenderErrors>,
    #[serde(default)]
    diagrams: DiagramsToml,
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
}

/// Where the theme comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSource {
    /// A theme file the user chose; it must exist.
    File(PathBuf),
    /// `theme.toml` in the configuration directory, when it exists.
    DefaultFile(PathBuf),
    /// The built-in theme.
    BuiltIn,
    /// A built-in paper theme, from `--light` or `--dark`.
    Paper(Paper),
}

/// Chooses the theme and tells where it comes from, strongest first:
/// `--theme`, a paper theme from `--light` or `--dark`, `KIRI_THEME` when it
/// is not empty, the configuration file, `theme.toml` in the configuration
/// directory, the built-in theme.
pub fn theme_source(
    cli: Option<&Path>,
    paper: Option<Paper>,
    env: &Environment,
    config: &Config,
) -> Setting<ThemeSource> {
    if let Some(path) = cli {
        return Setting::new(ThemeSource::File(path.to_owned()), Origin::Flag("--theme"));
    }
    if let Some(paper) = paper {
        let flag = match paper {
            Paper::Light => "--light",
            Paper::Dark => "--dark",
        };
        return Setting::new(ThemeSource::Paper(paper), Origin::Flag(flag));
    }
    if let Some(path) = env.kiri_theme.as_ref().filter(|path| !path.is_empty()) {
        return Setting::new(
            ThemeSource::File(PathBuf::from(path)),
            Origin::Environment("KIRI_THEME"),
        );
    }
    if let Some(path) = &config.theme {
        return Setting::new(ThemeSource::File(path.clone()), Origin::File);
    }
    let source = match directory(env) {
        Some(dir) => ThemeSource::DefaultFile(dir.join("theme.toml")),
        None => ThemeSource::BuiltIn,
    };
    Setting::new(source, Origin::Default)
}

/// Whether images are shown and where that comes from, strongest first:
/// the command line (`option`, from `--images` or `--no-images`), then
/// `KIRI_NO_IMAGES`, then the configuration file; on unless one turns them
/// off. `KIRI_NO_IMAGES` turns them off when it is set to anything but an
/// empty value or `0`, and `0` turns them on whatever the configuration
/// file says.
pub fn images(option: Option<bool>, env: &Environment, config: &Config) -> Setting<bool> {
    if let Some(on) = option {
        let flag = if on { "--images" } else { "--no-images" };
        return Setting::new(on, Origin::Flag(flag));
    }
    let no_images = env.kiri_no_images.as_ref();
    match (no_images.filter(|value| !value.is_empty()), config.images) {
        (Some(value), _) => Setting::new(value == "0", Origin::Environment("KIRI_NO_IMAGES")),
        (None, Some(on)) => Setting::new(on, Origin::File),
        (None, None) => Setting::new(true, Origin::Default),
    }
}

/// When the document is paged and where that comes from, strongest
/// first: the command line (`option`, from `--paging`, `-p` or `-P`), then
/// `KIRI_PAGING`, then the configuration file; `auto` unless one says
/// otherwise. `KIRI_PAGING` takes `always`, `auto` and `never`, `1` for
/// `always` and `0` for `never`; empty is unset. Any other value is left
/// out with the warning returned beside the setting.
pub fn paging(
    option: Option<Setting<Paging>>,
    env: &Environment,
    config: &Config,
) -> (Setting<Paging>, Option<String>) {
    if let Some(setting) = option {
        return (setting, None);
    }
    let variable = env.kiri_paging.as_ref().filter(|value| !value.is_empty());
    let mut warning = None;
    if let Some(value) = variable {
        let paging = match value.to_str() {
            Some("always" | "1") => Some(Paging::Always),
            Some("auto") => Some(Paging::Auto),
            Some("never" | "0") => Some(Paging::Never),
            _ => None,
        };
        match paging {
            Some(paging) => {
                return (
                    Setting::new(paging, Origin::Environment("KIRI_PAGING")),
                    None,
                );
            }
            None => {
                warning = Some(format!(
                    "KIRI_PAGING: `{}` is not always, auto, never, 1 or 0; left out",
                    value.to_string_lossy()
                ))
            }
        }
    }
    let setting = match config.paging {
        Some(paging) => Setting::new(paging, Origin::File),
        None => Setting::new(Paging::Auto, Origin::Default),
    };
    (setting, warning)
}

/// How front matter shows and where that comes from, strongest first:
/// the command line (`option`, from `--front-matter`), then
/// `KIRI_FRONT_MATTER`, then the configuration file; `table` unless one
/// says otherwise. `KIRI_FRONT_MATTER` takes `table`, `code`, `hide` and
/// `off`; empty is unset. Any other value is left out with the warning
/// returned beside the setting.
pub fn front_matter(
    option: Option<FrontMatter>,
    env: &Environment,
    config: &Config,
) -> (Setting<FrontMatter>, Option<String>) {
    named(
        (option, "--front-matter"),
        ("KIRI_FRONT_MATTER", env.kiri_front_matter.as_ref()),
        config.front_matter,
        FrontMatter::Table,
    )
}

/// How the rows of a table are told apart and where that comes from,
/// strongest first: the command line (`option`, from `--table-rows`),
/// then `KIRI_TABLE_ROWS`, then the configuration file; `zebra` unless one
/// says otherwise. `KIRI_TABLE_ROWS` takes `off`, `zebra`, `lines`,
/// `indent` and `underline`; empty is unset. Any other value is left out
/// with the warning returned beside the setting.
pub fn table_rows(
    option: Option<TableRows>,
    env: &Environment,
    config: &Config,
) -> (Setting<TableRows>, Option<String>) {
    named(
        (option, "--table-rows"),
        ("KIRI_TABLE_ROWS", env.kiri_table_rows.as_ref()),
        config.table_rows,
        TableRows::Zebra,
    )
}

/// Whether diagrams in code blocks are drawn and where that comes from,
/// strongest first: the command line (`option`, from `--diagrams`), then
/// `KIRI_DIAGRAMS`, then the configuration file; on unless one says
/// otherwise. `KIRI_DIAGRAMS` takes `on` and `off`, `1` for `on` and `0`
/// for `off`; empty is unset. Any other value is left out with the warning
/// returned beside the setting.
pub fn diagrams(
    option: Option<bool>,
    env: &Environment,
    config: &Config,
) -> (Setting<bool>, Option<String>) {
    if let Some(on) = option {
        return (Setting::new(on, Origin::Flag("--diagrams")), None);
    }
    let variable = env.kiri_diagrams.as_ref().filter(|value| !value.is_empty());
    let mut warning = None;
    if let Some(value) = variable {
        match value.to_str() {
            Some("on" | "1") => {
                return (
                    Setting::new(true, Origin::Environment("KIRI_DIAGRAMS")),
                    None,
                );
            }
            Some("off" | "0") => {
                return (
                    Setting::new(false, Origin::Environment("KIRI_DIAGRAMS")),
                    None,
                );
            }
            _ => {
                warning = Some(format!(
                    "KIRI_DIAGRAMS: `{}` is not on, off, 1 or 0; left out",
                    value.to_string_lossy()
                ))
            }
        }
    }
    let setting = match config.diagrams {
        Some(on) => Setting::new(on, Origin::File),
        None => Setting::new(true, Origin::Default),
    };
    (setting, warning)
}

/// Where kiri tells why a diagram could not be drawn, and where that
/// comes from, strongest first: the command line (`option`, from
/// `--render-errors`), then `KIRI_RENDER_ERRORS`, then the configuration
/// file; `both` unless one says otherwise. `KIRI_RENDER_ERRORS` takes
/// `inline`, `stderr` and `both`; empty is unset. Any other value is left
/// out with the warning returned beside the setting.
pub fn render_errors(
    option: Option<RenderErrors>,
    env: &Environment,
    config: &Config,
) -> (Setting<RenderErrors>, Option<String>) {
    named(
        (option, "--render-errors"),
        ("KIRI_RENDER_ERRORS", env.kiri_render_errors.as_ref()),
        config.render_errors,
        RenderErrors::Both,
    )
}

/// A setting whose values have names, such as `table`.
trait Named: Copy + 'static {
    /// Every value, in the order the warning lists them.
    const ALL: &'static [Self];
    fn name(self) -> &'static str;
}

impl Named for TableRows {
    const ALL: &'static [Self] = &[
        Self::Off,
        Self::Zebra,
        Self::Lines,
        Self::Indent,
        Self::Underline,
    ];

    fn name(self) -> &'static str {
        TableRows::name(self)
    }
}

impl Named for RenderErrors {
    const ALL: &'static [Self] = &[Self::Inline, Self::Stderr, Self::Both];

    fn name(self) -> &'static str {
        RenderErrors::name(self)
    }
}

impl Named for FrontMatter {
    const ALL: &'static [Self] = &[Self::Table, Self::Code, Self::Hide, Self::Off];

    fn name(self) -> &'static str {
        FrontMatter::name(self)
    }
}

/// A named setting and where it comes from, strongest first: the command
/// line (`option`, from `flag`), then the environment variable `name`
/// with `value`, then the configuration file (`file`), then `default`. The
/// variable takes the values' names; empty is unset. Any other value is
/// left out with the warning returned beside the setting.
fn named<T: Named>(
    (option, flag): (Option<T>, &'static str),
    (name, value): (&'static str, Option<&OsString>),
    file: Option<T>,
    default: T,
) -> (Setting<T>, Option<String>) {
    if let Some(chosen) = option {
        return (Setting::new(chosen, Origin::Flag(flag)), None);
    }
    let mut warning = None;
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        let all = T::ALL.iter().copied();
        match all
            .clone()
            .find(|named| value.to_str() == Some(named.name()))
        {
            Some(chosen) => return (Setting::new(chosen, Origin::Environment(name)), None),
            None => {
                let names: Vec<&str> = all.map(T::name).collect();
                let (last, rest) = names.split_last().expect("a setting has values");
                warning = Some(format!(
                    "{name}: `{}` is not {} or {last}; left out",
                    value.to_string_lossy(),
                    rest.join(", "),
                ));
            }
        }
    }
    let setting = match file {
        Some(chosen) => Setting::new(chosen, Origin::File),
        None => Setting::new(default, Origin::Default),
    };
    (setting, warning)
}

/// What the command line asks for; `None` where it leaves the choice to
/// the environment and the configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// `--theme`.
    pub theme: Option<PathBuf>,
    /// `--light` or `--dark`.
    pub paper: Option<Paper>,
    /// `--no-margins`.
    pub no_margins: bool,
    /// `--images` or `--no-images`.
    pub images: Option<bool>,
    /// `--paging`, `-p` or `-P`, with the flag it came from.
    pub paging: Option<Setting<Paging>>,
    /// `--front-matter`.
    pub front_matter: Option<FrontMatter>,
    /// `--table-rows`.
    pub table_rows: Option<TableRows>,
    /// `--diagrams`.
    pub diagrams: Option<bool>,
    /// `--render-errors`.
    pub render_errors: Option<RenderErrors>,
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
    /// How front matter shows.
    pub front_matter: FrontMatter,
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
    /// How front matter shows.
    pub front_matter: Setting<FrontMatter>,
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
            front_matter: self.front_matter.value,
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
    let (paging, paging_warning) = paging(options.paging.clone(), env, config);
    let (front_matter, front_matter_warning) = front_matter(options.front_matter, env, config);
    let (table_rows, table_rows_warning) = table_rows(options.table_rows, env, config);
    let (diagrams, diagrams_warning) = diagrams(options.diagrams, env, config);
    let (render_errors, render_errors_warning) = render_errors(options.render_errors, env, config);
    Choices {
        theme: theme_source(options.theme.as_deref(), options.paper, env, config),
        paper: options.paper,
        margins: match options.no_margins {
            true => Setting::new(false, Origin::Flag("--no-margins")),
            false => Setting::new(true, Origin::Default),
        },
        images: images(options.images, env, config),
        paging,
        search_case: match config.search_case {
            Some(case) => Setting::new(case, Origin::File),
            None => Setting::new(SearchCase::Smart, Origin::Default),
        },
        front_matter,
        table_rows,
        diagrams,
        diagram_commands: config.diagram_commands.clone(),
        diagram_timeout: match config.diagram_timeout {
            Some(limit) => Setting::new(limit, Origin::File),
            None => Setting::new(crate::diagrams::external::TIME_LIMIT, Origin::Default),
        },
        diagram_panel: match config.diagram_panel {
            Some(on) => Setting::new(on, Origin::File),
            None => Setting::new(true, Origin::Default),
        },
        diagram_no_search: config.diagram_no_search.clone(),
        render_errors,
        warnings: [
            paging_warning,
            front_matter_warning,
            table_rows_warning,
            diagrams_warning,
            render_errors_warning,
        ]
        .into_iter()
        .flatten()
        .collect(),
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

/// The theme that `source` names; the built-in theme when it is a default
/// file that does not exist.
pub fn load_theme(source: &ThemeSource) -> Result<Theme, String> {
    match source {
        ThemeSource::File(path) => crate::theme::load(path),
        ThemeSource::DefaultFile(path) if path.exists() => crate::theme::load(path),
        ThemeSource::DefaultFile(_) | ThemeSource::BuiltIn => Ok(Theme::default()),
        ThemeSource::Paper(paper) => Ok(Theme::paper(*paper)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(home: Option<&str>, xdg: Option<&str>) -> Environment {
        Environment {
            home: home.map(OsString::from),
            xdg_config_home: xdg.map(OsString::from),
            ..Environment::default()
        }
    }

    #[test]
    fn the_directory_is_under_xdg_config_home() {
        assert_eq!(
            directory(&env(Some("/home/u"), Some("/xdg"))),
            Some(PathBuf::from("/xdg/kiri"))
        );
    }

    #[test]
    fn the_directory_falls_back_to_dot_config_in_home() {
        let expected = Some(PathBuf::from("/home/u/.config/kiri"));
        assert_eq!(directory(&env(Some("/home/u"), None)), expected);
        assert_eq!(directory(&env(Some("/home/u"), Some(""))), expected);
    }

    #[test]
    fn the_directory_tells_where_it_comes_from() {
        let origin = |home, xdg| directory_origin(&env(home, xdg));
        assert_eq!(
            origin(Some("/home/u"), Some("/xdg")),
            Origin::Environment("XDG_CONFIG_HOME")
        );
        assert_eq!(
            origin(None, Some("/xdg")),
            Origin::Environment("XDG_CONFIG_HOME")
        );
        assert_eq!(origin(Some("/home/u"), None), Origin::Default);
        assert_eq!(origin(Some("/home/u"), Some("")), Origin::Default);
        assert_eq!(
            file(&env(Some("/home/u"), Some("/xdg"))).map(|file| file.origin),
            Some(Origin::Environment("XDG_CONFIG_HOME"))
        );
    }

    #[test]
    fn there_is_no_directory_without_home_or_xdg_config_home() {
        assert_eq!(directory(&env(None, None)), None);
        assert_eq!(directory(&env(None, Some(""))), None);
    }

    #[test]
    fn the_cache_directory_is_under_xdg_cache_home_or_dot_cache() {
        let env = |home: Option<&str>, cache: Option<&str>| Environment {
            home: home.map(OsString::from),
            xdg_cache_home: cache.map(OsString::from),
            ..Environment::default()
        };
        let cache = |home, cache| cache_directory(&env(home, cache));
        let dot_cache = Some(PathBuf::from("/home/u/.cache/kiri"));
        assert_eq!(
            cache(Some("/home/u"), Some("/c")),
            Some(PathBuf::from("/c/kiri"))
        );
        assert_eq!(cache(None, Some("/c")), Some(PathBuf::from("/c/kiri")));
        assert_eq!(cache(Some("/home/u"), None), dot_cache);
        assert_eq!(cache(Some("/home/u"), Some("")), dot_cache);
        assert_eq!(cache(None, None), None);
    }

    #[test]
    fn a_relative_xdg_config_home_is_ignored() {
        // As XDG_CACHE_HOME is.
        let relative = env(Some("/home/u"), Some("config"));
        assert_eq!(
            directory(&relative),
            Some(PathBuf::from("/home/u/.config/kiri"))
        );
        assert_eq!(directory_origin(&relative), Origin::Default);
        assert_eq!(directory(&env(None, Some("config"))), None);
    }

    #[test]
    fn an_empty_or_relative_home_is_not_set() {
        // As a relative XDG_CONFIG_HOME and XDG_CACHE_HOME are not, so
        // that kiri makes no files under the current directory.
        for home in ["", "home", ".", "./u"] {
            let env = env(Some(home), None);
            assert_eq!(directory(&env), None, "{home:?}");
            assert_eq!(cache_directory(&env), None, "{home:?}");
            let error = parse("theme = \"~/t.toml\"\n", Path::new("/c"), &env).unwrap_err();
            assert!(error.contains("HOME is not set"), "{home:?}: {error}");
            assert_eq!(home_directory(&env), None, "{home:?}");
        }
        let env = env(Some("/home/u"), None);
        assert_eq!(home_directory(&env), Some(PathBuf::from("/home/u")));
    }

    #[test]
    fn a_relative_xdg_cache_home_is_ignored() {
        // The XDG base directory specification calls a relative path
        // invalid.
        let env = |home: Option<&str>| Environment {
            home: home.map(OsString::from),
            xdg_cache_home: Some(OsString::from("cache")),
            ..Environment::default()
        };
        assert_eq!(
            cache_directory(&env(Some("/home/u"))),
            Some(PathBuf::from("/home/u/.cache/kiri"))
        );
        assert_eq!(cache_directory(&env(None)), None);
    }

    #[test]
    fn the_default_file_is_optional_config_toml_in_the_directory() {
        assert_eq!(
            file(&env(Some("/home/u"), None)),
            Some(ConfigFile {
                path: PathBuf::from("/home/u/.config/kiri/config.toml"),
                required: false,
                origin: Origin::Default,
            })
        );
        assert_eq!(file(&env(None, None)), None);
    }

    #[test]
    fn kiri_config_names_a_file_that_must_exist() {
        let mut env = env(Some("/home/u"), Some("/xdg"));
        env.kiri_config = Some("/elsewhere/kiri.toml".into());
        assert_eq!(
            file(&env),
            Some(ConfigFile {
                path: PathBuf::from("/elsewhere/kiri.toml"),
                required: true,
                origin: Origin::Environment("KIRI_CONFIG"),
            })
        );
    }

    #[test]
    fn an_empty_kiri_config_is_unset() {
        let mut env = env(Some("/home/u"), None);
        env.kiri_config = Some("".into());
        assert_eq!(
            file(&env).map(|file| file.path),
            Some(PathBuf::from("/home/u/.config/kiri/config.toml"))
        );
    }

    #[test]
    fn an_empty_file_sets_nothing() {
        assert_eq!(
            parse("", Path::new("/c"), &Environment::default()),
            Ok(Config::default())
        );
    }

    #[test]
    fn a_relative_theme_is_in_the_configuration_files_directory() {
        let config = parse(
            "theme = \"themes/paper.toml\"\n",
            Path::new("/c"),
            &Environment::default(),
        )
        .unwrap();
        assert_eq!(config.theme, Some(PathBuf::from("/c/themes/paper.toml")));
        let config = parse(
            "theme = \"/abs/paper.toml\"\n",
            Path::new("/c"),
            &Environment::default(),
        )
        .unwrap();
        assert_eq!(config.theme, Some(PathBuf::from("/abs/paper.toml")));
    }

    #[test]
    fn a_theme_under_tilde_is_in_the_home_directory() {
        let home = env(Some("/home/u"), Some("/xdg"));
        let config = parse("theme = \"~/themes/paper.toml\"\n", Path::new("/c"), &home).unwrap();
        assert_eq!(
            config.theme,
            Some(PathBuf::from("/home/u/themes/paper.toml"))
        );
        // Only a leading `~/` is the home directory.
        let config = parse("theme = \"~user/t.toml\"\n", Path::new("/c"), &home).unwrap();
        assert_eq!(config.theme, Some(PathBuf::from("/c/~user/t.toml")));
        let config = parse("theme = \"a/~/t.toml\"\n", Path::new("/c"), &home).unwrap();
        assert_eq!(config.theme, Some(PathBuf::from("/c/a/~/t.toml")));
        // A bare `~` names no file under the home directory.
        let config = parse("theme = \"~\"\n", Path::new("/c"), &home).unwrap();
        assert_eq!(config.theme, Some(PathBuf::from("/c/~")));
    }

    #[test]
    fn a_theme_under_tilde_needs_home() {
        for home in [None, Some("")] {
            let error = parse(
                "theme = \"~/t.toml\"\n",
                Path::new("/c"),
                &env(home, Some("/xdg")),
            )
            .unwrap_err();
            assert!(error.contains("HOME"), "{error}");
        }
    }

    #[test]
    fn rejects_unknown_keys_and_wrong_types() {
        let error = parse(
            "colour = \"blue\"\n",
            Path::new("/c"),
            &Environment::default(),
        )
        .unwrap_err();
        assert!(error.contains("unknown field `colour`"), "{error}");
        let error = parse("theme = 1\n", Path::new("/c"), &Environment::default()).unwrap_err();
        assert!(error.contains("invalid type"), "{error}");
    }

    fn with_theme(theme: &str) -> Config {
        Config {
            theme: Some(PathBuf::from(theme)),
            ..Config::default()
        }
    }

    #[test]
    fn the_command_line_theme_is_strongest() {
        let env = env(Some("/home/u"), None);
        assert_eq!(
            theme_source(
                Some(Path::new("cli.toml")),
                None,
                &env,
                &with_theme("/c/t.toml")
            )
            .value,
            ThemeSource::File(PathBuf::from("cli.toml"))
        );
    }

    #[test]
    fn the_configuration_file_theme_comes_before_theme_toml() {
        let env = env(Some("/home/u"), None);
        assert_eq!(
            theme_source(None, None, &env, &with_theme("/c/t.toml")).value,
            ThemeSource::File(PathBuf::from("/c/t.toml"))
        );
    }

    #[test]
    fn the_command_line_theme_comes_before_kiri_theme() {
        let mut env = env(Some("/home/u"), None);
        env.kiri_theme = Some("env.toml".into());
        assert_eq!(
            theme_source(Some(Path::new("cli.toml")), None, &env, &Config::default()).value,
            ThemeSource::File(PathBuf::from("cli.toml"))
        );
    }

    #[test]
    fn kiri_theme_comes_before_the_configuration_file() {
        let mut env = env(Some("/home/u"), None);
        env.kiri_theme = Some("env.toml".into());
        assert_eq!(
            theme_source(None, None, &env, &with_theme("/c/t.toml")).value,
            ThemeSource::File(PathBuf::from("env.toml"))
        );
    }

    #[test]
    fn an_empty_kiri_theme_is_unset() {
        let mut env = env(Some("/home/u"), None);
        env.kiri_theme = Some("".into());
        assert_eq!(
            theme_source(None, None, &env, &with_theme("/c/t.toml")).value,
            ThemeSource::File(PathBuf::from("/c/t.toml"))
        );
    }

    #[test]
    fn a_paper_theme_comes_after_the_command_line_theme() {
        let env = env(Some("/home/u"), None);
        let config = Config::default();
        assert_eq!(
            theme_source(
                Some(Path::new("cli.toml")),
                Some(Paper::Light),
                &env,
                &config
            )
            .value,
            ThemeSource::File(PathBuf::from("cli.toml"))
        );
        assert_eq!(
            theme_source(None, Some(Paper::Dark), &env, &config).value,
            ThemeSource::Paper(Paper::Dark)
        );
    }

    #[test]
    fn a_paper_theme_comes_before_kiri_theme_and_the_configuration_file() {
        let mut env = env(Some("/home/u"), None);
        env.kiri_theme = Some("env.toml".into());
        assert_eq!(
            theme_source(None, Some(Paper::Light), &env, &with_theme("/c/t.toml")).value,
            ThemeSource::Paper(Paper::Light)
        );
    }

    #[test]
    fn theme_toml_in_the_directory_comes_before_the_built_in_theme() {
        assert_eq!(
            theme_source(None, None, &env(Some("/home/u"), None), &Config::default()).value,
            ThemeSource::DefaultFile(PathBuf::from("/home/u/.config/kiri/theme.toml"))
        );
        assert_eq!(
            theme_source(None, None, &env(None, None), &Config::default()).value,
            ThemeSource::BuiltIn
        );
    }

    #[test]
    fn the_images_key_turns_images_on_or_off() {
        let config = parse("images = false\n", Path::new("/c"), &Environment::default()).unwrap();
        assert_eq!(config.images, Some(false));
        let config = parse("images = true\n", Path::new("/c"), &Environment::default()).unwrap();
        assert_eq!(config.images, Some(true));
        assert!(
            parse(
                "images = \"no\"\n",
                Path::new("/c"),
                &Environment::default()
            )
            .is_err()
        );
    }

    fn with_images(on: bool) -> Config {
        Config {
            images: Some(on),
            ..Config::default()
        }
    }

    fn no_images_env(value: &str) -> Environment {
        Environment {
            kiri_no_images: Some(value.into()),
            ..Environment::default()
        }
    }

    #[test]
    fn images_are_on_by_default() {
        assert!(images(None, &Environment::default(), &Config::default()).value);
    }

    #[test]
    fn the_command_line_is_strongest() {
        assert!(!images(Some(false), &no_images_env("0"), &with_images(true)).value);
        assert!(images(Some(true), &no_images_env("1"), &with_images(false)).value);
        assert!(images(Some(true), &Environment::default(), &with_images(false)).value);
    }

    #[test]
    fn kiri_no_images_turns_images_off_with_any_value_but_empty_or_0() {
        for value in ["1", "yes", "false"] {
            assert!(!images(None, &no_images_env(value), &with_images(true)).value);
        }
        assert!(images(None, &no_images_env(""), &Config::default()).value);
        assert!(images(None, &no_images_env("0"), &Config::default()).value);
    }

    #[test]
    fn kiri_no_images_comes_before_the_configuration_file() {
        assert!(!images(None, &no_images_env("1"), &with_images(true)).value);
        assert!(images(None, &no_images_env("0"), &with_images(false)).value);
    }

    #[test]
    fn an_empty_kiri_no_images_leaves_it_to_the_configuration_file() {
        assert!(!images(None, &no_images_env(""), &with_images(false)).value);
        assert!(!images(None, &Environment::default(), &with_images(false)).value);
        assert!(images(None, &Environment::default(), &with_images(true)).value);
    }

    #[test]
    fn the_paging_key_takes_always_auto_or_never() {
        let parsed = |text| parse(text, Path::new("/c"), &Environment::default());
        for (text, paging) in [
            ("paging = \"always\"\n", Paging::Always),
            ("paging = \"auto\"\n", Paging::Auto),
            ("paging = \"never\"\n", Paging::Never),
        ] {
            assert_eq!(parsed(text).unwrap().paging, Some(paging), "{text}");
        }
        assert!(parsed("paging = \"sometimes\"\n").is_err());
        assert!(parsed("paging = true\n").is_err());
    }

    fn with_paging(paging: Paging) -> Config {
        Config {
            paging: Some(paging),
            ..Config::default()
        }
    }

    fn paging_env(value: &str) -> Environment {
        Environment {
            kiri_paging: Some(value.into()),
            ..Environment::default()
        }
    }

    fn flag(paging: Paging, flag: &'static str) -> Option<Setting<Paging>> {
        Some(Setting::new(paging, Origin::Flag(flag)))
    }

    #[test]
    fn paging_is_auto_by_default() {
        let (chosen, warning) = paging(None, &Environment::default(), &Config::default());
        assert_eq!(chosen, Setting::new(Paging::Auto, Origin::Default));
        assert_eq!(warning, None);
    }

    #[test]
    fn the_command_line_paging_is_strongest() {
        let (chosen, _) = paging(
            flag(Paging::Never, "-P"),
            &paging_env("always"),
            &with_paging(Paging::Always),
        );
        assert_eq!(chosen, Setting::new(Paging::Never, Origin::Flag("-P")));
    }

    #[test]
    fn kiri_paging_comes_before_the_configuration_file() {
        for (value, expected) in [
            ("always", Paging::Always),
            ("auto", Paging::Auto),
            ("never", Paging::Never),
            ("1", Paging::Always),
            ("0", Paging::Never),
        ] {
            let (chosen, warning) = paging(None, &paging_env(value), &with_paging(Paging::Auto));
            let origin = Origin::Environment("KIRI_PAGING");
            assert_eq!(chosen, Setting::new(expected, origin), "{value}");
            assert_eq!(warning, None, "{value}");
        }
    }

    #[test]
    fn an_empty_kiri_paging_leaves_it_to_the_configuration_file() {
        let (chosen, warning) = paging(None, &paging_env(""), &with_paging(Paging::Never));
        assert_eq!(chosen, Setting::new(Paging::Never, Origin::File));
        assert_eq!(warning, None);
    }

    #[test]
    fn an_unknown_kiri_paging_is_left_out_with_a_warning() {
        let (chosen, warning) = paging(None, &paging_env("joo"), &with_paging(Paging::Never));
        assert_eq!(chosen, Setting::new(Paging::Never, Origin::File));
        assert_eq!(
            warning.as_deref(),
            Some("KIRI_PAGING: `joo` is not always, auto, never, 1 or 0; left out")
        );
        let (chosen, _) = paging(None, &paging_env("joo"), &Config::default());
        assert_eq!(chosen, Setting::new(Paging::Auto, Origin::Default));
    }

    #[test]
    fn the_choices_carry_the_paging_and_its_warning() {
        let options = Options {
            paging: flag(Paging::Always, "-p"),
            ..Options::default()
        };
        let choices = choose(&options, &paging_env("joo"), &Config::default());
        assert_eq!(
            choices.paging,
            Setting::new(Paging::Always, Origin::Flag("-p"))
        );
        assert!(choices.warnings.is_empty(), "{:?}", choices.warnings);
        let choices = choose(&Options::default(), &paging_env("joo"), &Config::default());
        assert_eq!(choices.warnings.len(), 1);
        assert_eq!(choices.with_theme(Theme::default()).paging, Paging::Auto);
    }

    #[test]
    fn the_search_case_key_takes_smart_or_literal() {
        let parsed = |text| parse(text, Path::new("/c"), &Environment::default());
        for (text, case) in [
            ("search_case = \"smart\"\n", SearchCase::Smart),
            ("search_case = \"literal\"\n", SearchCase::Literal),
        ] {
            assert_eq!(parsed(text).unwrap().search_case, Some(case), "{text}");
        }
        assert!(parsed("search_case = \"ignore\"\n").is_err());
        assert!(parsed("search_case = true\n").is_err());
    }

    #[test]
    fn the_search_case_is_smart_unless_the_configuration_file_says_otherwise() {
        let env = Environment::default();
        let choices = choose(&Options::default(), &env, &Config::default());
        let smart = Setting::new(SearchCase::Smart, Origin::Default);
        assert_eq!(choices.search_case, smart);
        let config = Config {
            search_case: Some(SearchCase::Literal),
            ..Config::default()
        };
        let choices = choose(&Options::default(), &env, &config);
        let literal = Setting::new(SearchCase::Literal, Origin::File);
        assert_eq!(choices.search_case, literal);
        let settings = choices.with_theme(Theme::default());
        assert_eq!(settings.search_case, SearchCase::Literal);
    }

    #[test]
    fn the_front_matter_key_takes_table_code_hide_or_off() {
        let parsed = |text| parse(text, Path::new("/c"), &Environment::default());
        for (text, front_matter) in [
            ("front_matter = \"table\"\n", FrontMatter::Table),
            ("front_matter = \"code\"\n", FrontMatter::Code),
            ("front_matter = \"hide\"\n", FrontMatter::Hide),
            ("front_matter = \"off\"\n", FrontMatter::Off),
        ] {
            let config = parsed(text).unwrap();
            assert_eq!(config.front_matter, Some(front_matter), "{text}");
        }
        assert!(parsed("front_matter = \"list\"\n").is_err());
        assert!(parsed("front_matter = false\n").is_err());
        assert!(parsed("front-matter = \"code\"\n").is_err());
    }

    fn with_front_matter(front_matter: FrontMatter) -> Config {
        Config {
            front_matter: Some(front_matter),
            ..Config::default()
        }
    }

    fn front_matter_env(value: &str) -> Environment {
        Environment {
            kiri_front_matter: Some(value.into()),
            ..Environment::default()
        }
    }

    #[test]
    fn front_matter_is_a_table_by_default() {
        let (chosen, warning) = front_matter(None, &Environment::default(), &Config::default());
        assert_eq!(chosen, Setting::new(FrontMatter::Table, Origin::Default));
        assert_eq!(warning, None);
    }

    #[test]
    fn the_command_line_front_matter_is_strongest() {
        let (chosen, _) = front_matter(
            Some(FrontMatter::Hide),
            &front_matter_env("code"),
            &with_front_matter(FrontMatter::Off),
        );
        let flag = Origin::Flag("--front-matter");
        assert_eq!(chosen, Setting::new(FrontMatter::Hide, flag));
    }

    #[test]
    fn kiri_front_matter_comes_before_the_configuration_file() {
        for (value, expected) in [
            ("table", FrontMatter::Table),
            ("code", FrontMatter::Code),
            ("hide", FrontMatter::Hide),
            ("off", FrontMatter::Off),
        ] {
            let config = with_front_matter(FrontMatter::Code);
            let (chosen, warning) = front_matter(None, &front_matter_env(value), &config);
            let origin = Origin::Environment("KIRI_FRONT_MATTER");
            assert_eq!(chosen, Setting::new(expected, origin), "{value}");
            assert_eq!(warning, None, "{value}");
        }
    }

    #[test]
    fn an_empty_kiri_front_matter_leaves_it_to_the_configuration_file() {
        let config = with_front_matter(FrontMatter::Code);
        let (chosen, warning) = front_matter(None, &front_matter_env(""), &config);
        assert_eq!(chosen, Setting::new(FrontMatter::Code, Origin::File));
        assert_eq!(warning, None);
    }

    #[test]
    fn an_unknown_kiri_front_matter_is_left_out_with_a_warning() {
        let config = with_front_matter(FrontMatter::Hide);
        let (chosen, warning) = front_matter(None, &front_matter_env("Code"), &config);
        assert_eq!(chosen, Setting::new(FrontMatter::Hide, Origin::File));
        assert_eq!(
            warning.as_deref(),
            Some("KIRI_FRONT_MATTER: `Code` is not table, code, hide or off; left out")
        );
    }

    #[test]
    fn the_choices_carry_the_front_matter_and_both_warnings() {
        let env = Environment {
            kiri_paging: Some("joo".into()),
            kiri_front_matter: Some("joo".into()),
            ..Environment::default()
        };
        let config = with_front_matter(FrontMatter::Off);
        let choices = choose(&Options::default(), &env, &config);
        assert_eq!(
            choices.front_matter,
            Setting::new(FrontMatter::Off, Origin::File)
        );
        assert_eq!(choices.warnings.len(), 2, "{:?}", choices.warnings);
        let settings = choices.with_theme(Theme::default());
        assert_eq!(settings.front_matter, FrontMatter::Off);
        assert_eq!(settings.front_matter.style(), None);
        assert_eq!(FrontMatter::Code.style(), Some(FrontMatterStyle::Code));
    }

    #[test]
    fn the_diagrams_key_turns_diagrams_on_or_off() {
        let parsed = |text| parse(text, Path::new("/c"), &Environment::default());
        assert_eq!(parsed("diagrams = false\n").unwrap().diagrams, Some(false));
        assert_eq!(parsed("diagrams = true\n").unwrap().diagrams, Some(true));
        assert!(parsed("diagrams = \"off\"\n").is_err());
    }

    #[test]
    fn the_diagrams_table_turns_diagrams_on_or_off_beside_a_language_table() {
        let parsed = |text| parse(text, Path::new("/c"), &Environment::default());
        let enabled = |text| parsed(text).map(|config| config.diagrams);
        assert_eq!(enabled("[diagrams]\nenabled = false\n"), Ok(Some(false)));
        assert_eq!(enabled("[diagrams]\nenabled = true\n"), Ok(Some(true)));
        assert_eq!(enabled("[diagrams]\n"), Ok(None));
        let both = "[diagrams]\nenabled = false\n\n\
                    [diagrams.mermaid]\ncommand = [\"mmdc\", \"{input}\", \"{output}\"]\n";
        let config = parsed(both).map(|config| (config.diagrams, config.diagram_commands));
        let command = ["mmdc", "{input}", "{output}"].map(String::from).to_vec();
        assert_eq!(
            config,
            Ok((
                Some(false),
                BTreeMap::from([("mermaid".to_owned(), command)])
            ))
        );
        let error = enabled("[diagrams]\nenabled = \"no\"\n").unwrap_err();
        assert!(error.contains("invalid type"), "{error}");
    }

    #[test]
    fn the_diagrams_table_gives_the_time_limit_in_seconds() {
        let parsed = |text: &str| parse(text, Path::new("/c"), &Environment::default());
        let timeout = |text: &str| parsed(text).map(|config| config.diagram_timeout);
        let seconds = |seconds| Ok(Some(Duration::from_secs_f64(seconds)));
        assert_eq!(timeout("[diagrams]\ntimeout = 5\n"), seconds(5.0));
        assert_eq!(timeout("[diagrams]\ntimeout = 2.5\n"), seconds(2.5));
        assert_eq!(timeout("[diagrams]\nenabled = true\n"), Ok(None));
        for wrong in ["0", "-1", "0.0", "nan", "inf", "1e300", "\"5\""] {
            let error = timeout(&format!("[diagrams]\ntimeout = {wrong}\n"));
            let error = error.unwrap_err();
            assert!(error.contains("timeout: give"), "{wrong}: {error}");
        }
        // 20 s unless the file says otherwise.
        let choices =
            |config: &Config| choose(&Options::default(), &Environment::default(), config);
        let default = choices(&Config::default());
        assert_eq!(
            default.diagram_timeout,
            Setting::new(Duration::from_secs(20), Origin::Default)
        );
        let config = parsed("[diagrams]\ntimeout = 3\n").unwrap();
        assert_eq!(choices(&config).diagram_timeout.origin, Origin::File);
        let settings = choices(&config).with_theme(Theme::default());
        assert_eq!(settings.diagram_timeout, Duration::from_secs(3));
    }

    #[test]
    fn the_diagrams_table_turns_the_background_panel_off() {
        let parsed = |text: &str| parse(text, Path::new("/c"), &Environment::default());
        let panel = |text: &str| parsed(text).map(|config| config.diagram_panel);
        assert_eq!(
            panel("[diagrams]\nbackground_panel = false\n"),
            Ok(Some(false))
        );
        assert_eq!(
            panel("[diagrams]\nbackground_panel = true\n"),
            Ok(Some(true))
        );
        assert_eq!(panel("[diagrams]\n"), Ok(None));
        let choices =
            |config: &Config| choose(&Options::default(), &Environment::default(), config);
        assert_eq!(
            choices(&Config::default()).diagram_panel,
            Setting::new(true, Origin::Default)
        );
        let config = parsed("[diagrams]\nbackground_panel = false\n").unwrap();
        assert_eq!(
            choices(&config).diagram_panel,
            Setting::new(false, Origin::File)
        );
        assert!(!choices(&config).with_theme(Theme::default()).diagram_panel);
    }

    #[test]
    fn a_diagram_language_table_turns_the_search_for_its_program_off() {
        let parsed = |text: &str| parse(text, Path::new("/c"), &Environment::default());
        let off = |text: &str| {
            parsed(text).map(|config| (config.diagram_no_search, config.diagram_commands))
        };
        let mermaid = BTreeSet::from(["mermaid".to_owned()]);
        assert_eq!(
            off("[diagrams.mermaid]\nauto_detect = false\n"),
            Ok((mermaid.clone(), BTreeMap::new()))
        );
        assert_eq!(
            off("[diagrams.mermaid]\nauto_detect = true\n"),
            Ok((BTreeSet::new(), BTreeMap::new()))
        );
        let both = "[diagrams.mermaid]\nauto_detect = false\ncommand = [\"m\", \"{input}\", \"{output}\"]\n";
        let command = ["m", "{input}", "{output}"].map(String::from).to_vec();
        assert_eq!(
            off(both),
            Ok((
                mermaid.clone(),
                BTreeMap::from([("mermaid".to_owned(), command)])
            ))
        );
        let config = parsed("[diagrams.mermaid]\nauto_detect = false\n").unwrap();
        let choices = choose(&Options::default(), &Environment::default(), &config);
        assert_eq!(choices.diagram_no_search, mermaid);
        assert_eq!(
            choices.with_theme(Theme::default()).diagram_no_search,
            mermaid
        );
        let error = off("[diagrams.mermaid]\nauto_detect = \"no\"\n").unwrap_err();
        assert!(error.contains("invalid type"), "{error}");
        let error = off("[diagrams.mermaid]\nargs = []\n").unwrap_err();
        assert!(
            error.contains("unknown field `args`, expected `command` or `auto_detect`"),
            "{error}"
        );
    }

    #[test]
    fn a_diagram_language_table_is_named_in_any_case() {
        // As code blocks name the language.
        let parsed = |text| parse(text, Path::new("/c"), &Environment::default());
        let config = parsed(
            "[diagrams.Mermaid]\ncommand = [\"m\", \"{input}\", \"{output}\"]\n\n\
             [diagrams.MERMAID]\nauto_detect = false\n",
        );
        let command = ["m", "{input}", "{output}"].map(String::from).to_vec();
        assert_eq!(
            config.map(|config| (config.diagram_commands, config.diagram_no_search)),
            Ok((
                BTreeMap::from([("mermaid".to_owned(), command)]),
                BTreeSet::from(["mermaid".to_owned()])
            ))
        );
    }

    #[test]
    fn a_diagram_language_table_gives_its_external_command() {
        let parsed = |text| parse(text, Path::new("/c"), &Environment::default());
        let config = parsed(
            "[diagrams.mermaid]\n\
             command = [\"mmdc\", \"-i\", \"{input}\", \"-o\", \"{output}\", \"-t\", \"{theme}\"]\n",
        )
        .unwrap();
        let command = ["mmdc", "-i", "{input}", "-o", "{output}", "-t", "{theme}"];
        assert_eq!(
            config.diagram_commands,
            BTreeMap::from([("mermaid".to_owned(), command.map(String::from).to_vec())])
        );
        // Without a switch of its own.
        assert_eq!(config.diagrams, None);
        let choices = choose(&Options::default(), &Environment::default(), &config);
        assert_eq!(choices.diagram_commands, config.diagram_commands);
        let settings = choices.with_theme(Theme::default());
        assert_eq!(settings.diagram_commands, config.diagram_commands);
        assert!(
            parsed("diagrams = true\n")
                .unwrap()
                .diagram_commands
                .is_empty()
        );
    }

    #[test]
    fn a_diagram_command_must_be_able_to_work() {
        let error = |text: &str| {
            parse(text, Path::new("/c"), &Environment::default())
                .map(|config| format!("{config:?}"))
                .unwrap_err()
        };
        let cases = [
            (
                "[diagrams.dot]\ncommand = [\"dot\", \"{input}\", \"{output}\"]\n",
                "unknown field `dot`, expected one of `enabled`, `timeout`, `background_panel`, `mermaid`",
            ),
            (
                "[diagrams.mermaid]\ncommand = []\n",
                "command: give the program and its arguments",
            ),
            (
                "[diagrams.mermaid]\ncommand = [\"\", \"{input}\", \"{output}\"]\n",
                "command: the program is empty",
            ),
            (
                "[diagrams.mermaid]\ncommand = [\"mmdc\", \"-o\", \"{output}\"]\n",
                "command: give {input} where the program takes its input file",
            ),
            (
                "[diagrams.mermaid]\ncommand = [\"mmdc\", \"-i\", \"{input}\"]\n",
                "command: give {output} where the program takes its output file",
            ),
            ("[diagrams.mermaid]\ncommand = \"mmdc\"\n", "invalid type"),
            ("[diagrams.mermaid]\nargs = []\n", "unknown field `args`"),
            ("diagrams = 1\n", "true or false, or a [diagrams] table"),
        ];
        for (text, expected) in cases {
            let error = error(text);
            assert!(error.contains(expected), "{text}: {error}");
        }
    }

    fn with_diagrams(on: bool) -> Config {
        Config {
            diagrams: Some(on),
            ..Config::default()
        }
    }

    fn diagrams_env(value: &str) -> Environment {
        Environment {
            kiri_diagrams: Some(value.into()),
            ..Environment::default()
        }
    }

    #[test]
    fn diagrams_are_on_by_default() {
        let (chosen, warning) = diagrams(None, &Environment::default(), &Config::default());
        assert_eq!(chosen, Setting::new(true, Origin::Default));
        assert_eq!(warning, None);
    }

    #[test]
    fn the_command_line_diagrams_are_strongest() {
        let flag = Origin::Flag("--diagrams");
        let (chosen, _) = diagrams(Some(false), &diagrams_env("on"), &with_diagrams(true));
        assert_eq!(chosen, Setting::new(false, flag));
        let (chosen, _) = diagrams(Some(true), &diagrams_env("0"), &with_diagrams(false));
        assert_eq!(chosen, Setting::new(true, flag));
    }

    #[test]
    fn kiri_diagrams_comes_before_the_configuration_file() {
        for (value, on) in [("on", true), ("off", false), ("1", true), ("0", false)] {
            let (chosen, warning) = diagrams(None, &diagrams_env(value), &with_diagrams(!on));
            let origin = Origin::Environment("KIRI_DIAGRAMS");
            assert_eq!(chosen, Setting::new(on, origin), "{value}");
            assert_eq!(warning, None, "{value}");
        }
    }

    #[test]
    fn an_empty_kiri_diagrams_leaves_it_to_the_configuration_file() {
        let (chosen, warning) = diagrams(None, &diagrams_env(""), &with_diagrams(false));
        assert_eq!(chosen, Setting::new(false, Origin::File));
        assert_eq!(warning, None);
    }

    #[test]
    fn an_unknown_kiri_diagrams_is_left_out_with_a_warning() {
        let (chosen, warning) = diagrams(None, &diagrams_env("yes"), &with_diagrams(false));
        assert_eq!(chosen, Setting::new(false, Origin::File));
        assert_eq!(
            warning.as_deref(),
            Some("KIRI_DIAGRAMS: `yes` is not on, off, 1 or 0; left out")
        );
        let (chosen, _) = diagrams(None, &diagrams_env("yes"), &Config::default());
        assert_eq!(chosen, Setting::new(true, Origin::Default));
    }

    #[test]
    fn the_choices_carry_the_diagrams_and_their_warning() {
        let options = Options {
            diagrams: Some(false),
            ..Options::default()
        };
        let choices = choose(&options, &diagrams_env("yes"), &Config::default());
        let flag = Origin::Flag("--diagrams");
        assert_eq!(choices.diagrams, Setting::new(false, flag));
        assert!(choices.warnings.is_empty(), "{:?}", choices.warnings);
        assert!(!choices.with_theme(Theme::default()).diagrams);
        let choices = choose(
            &Options::default(),
            &diagrams_env("yes"),
            &with_diagrams(false),
        );
        assert_eq!(choices.warnings.len(), 1, "{:?}", choices.warnings);
        assert!(!choices.with_theme(Theme::default()).diagrams);
    }

    #[test]
    fn the_table_rows_key_takes_each_value() {
        let parsed = |text| parse(text, Path::new("/c"), &Environment::default());
        for (text, rows) in [
            ("table_rows = \"off\"\n", TableRows::Off),
            ("table_rows = \"zebra\"\n", TableRows::Zebra),
            ("table_rows = \"lines\"\n", TableRows::Lines),
            ("table_rows = \"indent\"\n", TableRows::Indent),
            ("table_rows = \"underline\"\n", TableRows::Underline),
        ] {
            assert_eq!(parsed(text).unwrap().table_rows, Some(rows), "{text}");
        }
        assert!(parsed("table_rows = \"stripes\"\n").is_err());
        assert!(parsed("table_rows = true\n").is_err());
    }

    fn table_rows_env(value: &str) -> Environment {
        Environment {
            kiri_table_rows: Some(value.into()),
            ..Environment::default()
        }
    }

    fn with_table_rows(rows: TableRows) -> Config {
        Config {
            table_rows: Some(rows),
            ..Config::default()
        }
    }

    #[test]
    fn table_rows_are_zebra_unless_something_says_otherwise() {
        let (chosen, warning) = table_rows(None, &Environment::default(), &Config::default());
        assert_eq!(chosen, Setting::new(TableRows::Zebra, Origin::Default));
        assert_eq!(warning, None);
        let config = with_table_rows(TableRows::Off);
        let (chosen, _) = table_rows(None, &table_rows_env(""), &config);
        assert_eq!(chosen, Setting::new(TableRows::Off, Origin::File));
        let (chosen, _) = table_rows(None, &table_rows_env("lines"), &config);
        let variable = Origin::Environment("KIRI_TABLE_ROWS");
        assert_eq!(chosen, Setting::new(TableRows::Lines, variable));
        let (chosen, _) = table_rows(Some(TableRows::Zebra), &table_rows_env("lines"), &config);
        let flag = Origin::Flag("--table-rows");
        assert_eq!(chosen, Setting::new(TableRows::Zebra, flag));
    }

    #[test]
    fn an_unknown_kiri_table_rows_is_left_out_with_a_warning() {
        let config = with_table_rows(TableRows::Lines);
        let (chosen, warning) = table_rows(None, &table_rows_env("stripes"), &config);
        assert_eq!(chosen, Setting::new(TableRows::Lines, Origin::File));
        assert_eq!(
            warning.as_deref(),
            Some(
                "KIRI_TABLE_ROWS: `stripes` is not off, zebra, lines, indent or underline; left out"
            )
        );
        let choices = choose(&Options::default(), &table_rows_env("stripes"), &config);
        assert_eq!(choices.warnings.len(), 1);
        assert_eq!(
            choices.with_theme(Theme::default()).table_rows,
            TableRows::Lines
        );
    }

    #[test]
    fn the_render_errors_key_takes_each_value() {
        let parsed = |text| parse(text, Path::new("/c"), &Environment::default());
        for (text, errors) in [
            ("render_errors = \"inline\"\n", RenderErrors::Inline),
            ("render_errors = \"stderr\"\n", RenderErrors::Stderr),
            ("render_errors = \"both\"\n", RenderErrors::Both),
        ] {
            assert_eq!(parsed(text).unwrap().render_errors, Some(errors), "{text}");
        }
        assert_eq!(parsed("").unwrap().render_errors, None);
        assert!(parsed("render_errors = \"never\"\n").is_err());
        assert!(parsed("render_errors = true\n").is_err());
    }

    fn render_errors_env(value: &str) -> Environment {
        Environment {
            kiri_render_errors: Some(value.into()),
            ..Environment::default()
        }
    }

    fn with_render_errors(errors: RenderErrors) -> Config {
        Config {
            render_errors: Some(errors),
            ..Config::default()
        }
    }

    #[test]
    fn render_errors_go_to_both_unless_something_says_otherwise() {
        let (chosen, warning) = render_errors(None, &Environment::default(), &Config::default());
        assert_eq!(chosen, Setting::new(RenderErrors::Both, Origin::Default));
        assert_eq!(warning, None);
        let config = with_render_errors(RenderErrors::Inline);
        let (chosen, _) = render_errors(None, &render_errors_env(""), &config);
        assert_eq!(chosen, Setting::new(RenderErrors::Inline, Origin::File));
        let (chosen, _) = render_errors(None, &render_errors_env("stderr"), &config);
        let variable = Origin::Environment("KIRI_RENDER_ERRORS");
        assert_eq!(chosen, Setting::new(RenderErrors::Stderr, variable));
        let flag = Some(RenderErrors::Both);
        let (chosen, _) = render_errors(flag, &render_errors_env("stderr"), &config);
        let flag = Origin::Flag("--render-errors");
        assert_eq!(chosen, Setting::new(RenderErrors::Both, flag));
    }

    #[test]
    fn an_unknown_kiri_render_errors_is_left_out_with_a_warning() {
        let config = with_render_errors(RenderErrors::Stderr);
        let (chosen, warning) = render_errors(None, &render_errors_env("never"), &config);
        assert_eq!(chosen, Setting::new(RenderErrors::Stderr, Origin::File));
        assert_eq!(
            warning.as_deref(),
            Some("KIRI_RENDER_ERRORS: `never` is not inline, stderr or both; left out")
        );
        let choices = choose(&Options::default(), &render_errors_env("never"), &config);
        assert_eq!(choices.warnings.len(), 1);
        assert_eq!(
            choices.with_theme(Theme::default()).render_errors,
            RenderErrors::Stderr
        );
    }

    fn no_options() -> Options {
        Options::default()
    }

    #[test]
    fn settings_take_the_built_in_theme_without_a_theme_file() {
        let env = env(None, Some("/nonexistent"));
        let settings = settings(&no_options(), &env, &Config::default()).1.unwrap();
        assert_eq!(settings.theme, Theme::default());
        assert_eq!(settings.paper, None);
        assert!(settings.images);
    }

    #[test]
    fn settings_keep_the_paper_and_its_theme() {
        let options = Options {
            paper: Some(Paper::Dark),
            ..no_options()
        };
        let settings = settings(&options, &env(None, None), &Config::default())
            .1
            .unwrap();
        assert_eq!(settings.theme, Theme::paper(Paper::Dark));
        assert_eq!(settings.paper, Some(Paper::Dark));
    }

    #[test]
    fn no_margins_applies_to_whichever_theme_is_chosen() {
        for paper in [None, Some(Paper::Light)] {
            let options = Options {
                paper,
                no_margins: true,
                ..no_options()
            };
            let settings = settings(&options, &env(None, None), &Config::default())
                .1
                .unwrap();
            let theme = paper.map_or_else(Theme::default, Theme::paper);
            assert_eq!(settings.theme, theme.without_margins());
        }
    }

    #[test]
    fn a_theme_file_that_does_not_load_is_an_error_naming_it() {
        let options = Options {
            theme: Some(PathBuf::from("/nonexistent/mine.toml")),
            ..no_options()
        };
        let error = settings(&options, &env(None, None), &Config::default())
            .1
            .unwrap_err();
        assert!(error.starts_with("/nonexistent/mine.toml: "), "{error}");
    }

    #[test]
    fn the_theme_tells_where_it_comes_from() {
        let mut env = env(Some("/home/u"), None);
        let origin = |cli: Option<&str>, paper, env: &Environment, config: &Config| {
            theme_source(cli.map(Path::new), paper, env, config).origin
        };
        let config = with_theme("/c/t.toml");
        assert_eq!(
            origin(None, None, &env, &Config::default()),
            Origin::Default
        );
        assert_eq!(origin(None, None, &env, &config), Origin::File);
        env.kiri_theme = Some("env.toml".into());
        assert_eq!(
            origin(None, None, &env, &config),
            Origin::Environment("KIRI_THEME")
        );
        assert_eq!(
            origin(None, Some(Paper::Light), &env, &config),
            Origin::Flag("--light")
        );
        assert_eq!(
            origin(None, Some(Paper::Dark), &env, &config),
            Origin::Flag("--dark")
        );
        assert_eq!(
            origin(Some("cli.toml"), Some(Paper::Dark), &env, &config),
            Origin::Flag("--theme")
        );
        assert_eq!(
            origin(None, None, &Environment::default(), &Config::default()),
            Origin::Default
        );
    }

    #[test]
    fn images_tell_where_they_come_from() {
        let origin =
            |option, env: &Environment, config: &Config| images(option, env, config).origin;
        let default = Environment::default();
        assert_eq!(origin(None, &default, &Config::default()), Origin::Default);
        assert_eq!(origin(None, &default, &with_images(false)), Origin::File);
        assert_eq!(
            origin(None, &no_images_env("0"), &with_images(false)),
            Origin::Environment("KIRI_NO_IMAGES")
        );
        assert_eq!(
            origin(None, &no_images_env(""), &with_images(false)),
            Origin::File
        );
        assert_eq!(
            origin(Some(true), &no_images_env("1"), &Config::default()),
            Origin::Flag("--images")
        );
        assert_eq!(
            origin(Some(false), &default, &Config::default()),
            Origin::Flag("--no-images")
        );
    }

    #[test]
    fn the_choices_tell_where_they_come_from() {
        let options = Options {
            no_margins: true,
            images: Some(false),
            ..no_options()
        };
        let choices = choose(&options, &env(None, None), &Config::default());
        assert_eq!(
            choices.margins,
            Setting::new(false, Origin::Flag("--no-margins"))
        );
        assert_eq!(
            choices.images,
            Setting::new(false, Origin::Flag("--no-images"))
        );
        assert_eq!(
            choices.theme,
            Setting::new(ThemeSource::BuiltIn, Origin::Default)
        );
        let choices = choose(&no_options(), &env(None, None), &Config::default());
        assert_eq!(choices.margins, Setting::new(true, Origin::Default));
    }

    #[test]
    fn settings_turn_images_off_as_the_environment_says() {
        let settings = settings(&no_options(), &no_images_env("1"), &Config::default())
            .1
            .unwrap();
        assert!(!settings.images);
    }
}
