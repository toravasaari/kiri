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
    /// `NO_COLOR`: colours off unless empty.
    pub no_color: Option<OsString>,
    /// `COLORTERM`: 24-bit colour when `truecolor` or `24bit`.
    pub colorterm: Option<OsString>,
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
            no_color: std::env::var_os("NO_COLOR"),
            colorterm: std::env::var_os("COLORTERM"),
            term_program: std::env::var_os("TERM_PROGRAM"),
            vte_version: std::env::var_os("VTE_VERSION"),
        }
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
}

/// Where the configuration file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFile {
    pub path: PathBuf,
    /// The user named the file, so it must exist; the default file need
    /// not.
    pub required: bool,
}

/// `KIRI_CONFIG` when it is set and not empty, else `config.toml` in the
/// configuration directory.
pub fn file(env: &Environment) -> Option<ConfigFile> {
    if let Some(path) = env.kiri_config.as_ref().filter(|path| !path.is_empty()) {
        return Some(ConfigFile {
            path: PathBuf::from(path),
            required: true,
        });
    }
    Some(ConfigFile {
        path: directory(env)?.join("config.toml"),
        required: false,
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
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigToml {
    theme: Option<PathBuf>,
    images: Option<bool>,
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
    fn new(value: T, origin: Origin) -> Self {
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
/// off. `KIRI_NO_IMAGES` turns
/// them off when it is set to anything but an empty value or `0`, and `0`
/// turns them on whatever the configuration file says.
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
}

/// What `options`, `env` and `config` choose.
pub fn choose(options: &Options, env: &Environment, config: &Config) -> Choices {
    Choices {
        theme: theme_source(options.theme.as_deref(), options.paper, env, config),
        paper: options.paper,
        margins: match options.no_margins {
            true => Setting::new(false, Origin::Flag("--no-margins")),
            false => Setting::new(true, Origin::Default),
        },
        images: images(options.images, env, config),
    }
}

/// The settings that `options`, `env` and `config` choose, with the theme
/// loaded.
pub fn settings(options: &Options, env: &Environment, config: &Config) -> Result<Settings, String> {
    let choices = choose(options, env, config);
    let theme = load_theme(&choices.theme.value)?;
    Ok(Settings {
        theme: if choices.margins.value {
            theme
        } else {
            theme.without_margins()
        },
        paper: choices.paper,
        images: choices.images.value,
    })
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
