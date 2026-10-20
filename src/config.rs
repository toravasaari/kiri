//! Settings from the command line, the environment and the configuration
//! file, read once at start-up. The environment also holds the variables
//! that capability detection reads.
//!
//! Everything here takes the environment as a value, so the rules can be
//! tested without touching the process environment.

use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::render::{FrontMatterStyle, TableRows};
use crate::theme::{Paper, Theme};

/// The environment variables kiri reads for its settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    pub home: Option<OsString>,
    pub xdg_config_home: Option<OsString>,
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
            kiri_config: std::env::var_os("KIRI_CONFIG"),
            kiri_theme: std::env::var_os("KIRI_THEME"),
            kiri_no_images: std::env::var_os("KIRI_NO_IMAGES"),
            kiri_paging: std::env::var_os("KIRI_PAGING"),
            kiri_front_matter: std::env::var_os("KIRI_FRONT_MATTER"),
            kiri_table_rows: std::env::var_os("KIRI_TABLE_ROWS"),
            no_color: std::env::var_os("NO_COLOR"),
            colorterm: std::env::var_os("COLORTERM"),
            term: std::env::var_os("TERM"),
            term_program: std::env::var_os("TERM_PROGRAM"),
            vte_version: std::env::var_os("VTE_VERSION"),
        }
    }
}

/// Where the configuration directory comes from: `XDG_CONFIG_HOME`, or
/// the default under the home directory.
pub fn directory_origin(env: &Environment) -> Origin {
    match env.xdg_config_home.as_ref().filter(|dir| !dir.is_empty()) {
        Some(_) => Origin::Environment("XDG_CONFIG_HOME"),
        None => Origin::Default,
    }
}

/// kiri's configuration directory: `$XDG_CONFIG_HOME/kiri`, or
/// `~/.config/kiri` when that is unset or empty.
pub fn directory(env: &Environment) -> Option<PathBuf> {
    let config = env
        .xdg_config_home
        .clone()
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env.home
                .as_ref()
                .map(|home| PathBuf::from(home).join(".config"))
        })?;
    Some(config.join("kiri"))
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
            Ok(rest) if !rest.as_os_str().is_empty() => {
                match env.home.as_ref().filter(|home| !home.is_empty()) {
                    Some(home) => Path::new(home).join(rest),
                    None => return Err(format!("theme {}: HOME is not set", theme.display())),
                }
            }
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
        }
    }
}

/// What `options`, `env` and `config` choose.
pub fn choose(options: &Options, env: &Environment, config: &Config) -> Choices {
    let (paging, paging_warning) = paging(options.paging.clone(), env, config);
    let (front_matter, front_matter_warning) = front_matter(options.front_matter, env, config);
    let (table_rows, table_rows_warning) = table_rows(options.table_rows, env, config);
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
        warnings: [paging_warning, front_matter_warning, table_rows_warning]
            .into_iter()
            .flatten()
            .collect(),
    }
}

/// The settings that `options`, `env` and `config` choose, with the theme
/// loaded.
pub fn settings(options: &Options, env: &Environment, config: &Config) -> Result<Settings, String> {
    let choices = choose(options, env, config);
    let theme = load_theme(&choices.theme.value)?;
    Ok(choices.with_theme(theme))
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

    fn no_options() -> Options {
        Options::default()
    }

    #[test]
    fn settings_take_the_built_in_theme_without_a_theme_file() {
        let env = env(None, Some("/nonexistent"));
        let settings = settings(&no_options(), &env, &Config::default()).unwrap();
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
        let settings = settings(&options, &env(None, None), &Config::default()).unwrap();
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
            let settings = settings(&options, &env(None, None), &Config::default()).unwrap();
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
        let error = settings(&options, &env(None, None), &Config::default()).unwrap_err();
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
        let settings = settings(&no_options(), &no_images_env("1"), &Config::default()).unwrap();
        assert!(!settings.images);
    }
}
