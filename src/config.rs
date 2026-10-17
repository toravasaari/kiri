//! Settings from the environment and the configuration file, read once at
//! start-up.
//!
//! Everything here takes the environment as a value, so the rules can be
//! tested without touching the process environment.

use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The environment variables kiri reads for its settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    pub home: Option<OsString>,
    pub xdg_config_home: Option<OsString>,
    /// `KIRI_CONFIG`: the configuration file to read instead of the default.
    pub kiri_config: Option<OsString>,
    /// `KIRI_THEME`: a theme file.
    pub kiri_theme: Option<OsString>,
}

impl Environment {
    /// The environment of this process.
    pub fn from_process() -> Self {
        Self {
            home: std::env::var_os("HOME"),
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
            kiri_config: std::env::var_os("KIRI_CONFIG"),
            kiri_theme: std::env::var_os("KIRI_THEME"),
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
    /// A theme file, relative to the configuration file's directory.
    pub theme: Option<PathBuf>,
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
    parse(&text, dir).map_err(|error| format!("{}: {error}", path.display()))
}

/// Parses a configuration file in `dir`.
pub fn parse(text: &str, dir: &Path) -> Result<Config, String> {
    let file: ConfigToml = toml::from_str(text).map_err(|error| error.to_string())?;
    Ok(Config {
        theme: file.theme.map(|theme| dir.join(theme)),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigToml {
    theme: Option<PathBuf>,
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
}

/// Chooses the theme, strongest first: `--theme`, `KIRI_THEME` when it is
/// not empty, the configuration file, `theme.toml` in the configuration
/// directory, the built-in theme.
pub fn theme_source(cli: Option<&Path>, env: &Environment, config: &Config) -> ThemeSource {
    if let Some(path) = cli {
        return ThemeSource::File(path.to_owned());
    }
    if let Some(path) = env.kiri_theme.as_ref().filter(|path| !path.is_empty()) {
        return ThemeSource::File(PathBuf::from(path));
    }
    if let Some(path) = &config.theme {
        return ThemeSource::File(path.clone());
    }
    match directory(env) {
        Some(dir) => ThemeSource::DefaultFile(dir.join("theme.toml")),
        None => ThemeSource::BuiltIn,
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
        assert_eq!(parse("", Path::new("/c")), Ok(Config::default()));
    }

    #[test]
    fn a_relative_theme_is_in_the_configuration_files_directory() {
        let config = parse("theme = \"themes/paper.toml\"\n", Path::new("/c")).unwrap();
        assert_eq!(config.theme, Some(PathBuf::from("/c/themes/paper.toml")));
        let config = parse("theme = \"/abs/paper.toml\"\n", Path::new("/c")).unwrap();
        assert_eq!(config.theme, Some(PathBuf::from("/abs/paper.toml")));
    }

    #[test]
    fn rejects_unknown_keys_and_wrong_types() {
        let error = parse("colour = \"blue\"\n", Path::new("/c")).unwrap_err();
        assert!(error.contains("unknown field `colour`"), "{error}");
        let error = parse("theme = 1\n", Path::new("/c")).unwrap_err();
        assert!(error.contains("invalid type"), "{error}");
    }

    fn with_theme(theme: &str) -> Config {
        Config {
            theme: Some(PathBuf::from(theme)),
        }
    }

    #[test]
    fn the_command_line_theme_is_strongest() {
        let env = env(Some("/home/u"), None);
        assert_eq!(
            theme_source(Some(Path::new("cli.toml")), &env, &with_theme("/c/t.toml")),
            ThemeSource::File(PathBuf::from("cli.toml"))
        );
    }

    #[test]
    fn the_configuration_file_theme_comes_before_theme_toml() {
        let env = env(Some("/home/u"), None);
        assert_eq!(
            theme_source(None, &env, &with_theme("/c/t.toml")),
            ThemeSource::File(PathBuf::from("/c/t.toml"))
        );
    }

    #[test]
    fn the_command_line_theme_comes_before_kiri_theme() {
        let mut env = env(Some("/home/u"), None);
        env.kiri_theme = Some("env.toml".into());
        assert_eq!(
            theme_source(Some(Path::new("cli.toml")), &env, &Config::default()),
            ThemeSource::File(PathBuf::from("cli.toml"))
        );
    }

    #[test]
    fn kiri_theme_comes_before_the_configuration_file() {
        let mut env = env(Some("/home/u"), None);
        env.kiri_theme = Some("env.toml".into());
        assert_eq!(
            theme_source(None, &env, &with_theme("/c/t.toml")),
            ThemeSource::File(PathBuf::from("env.toml"))
        );
    }

    #[test]
    fn an_empty_kiri_theme_is_unset() {
        let mut env = env(Some("/home/u"), None);
        env.kiri_theme = Some("".into());
        assert_eq!(
            theme_source(None, &env, &with_theme("/c/t.toml")),
            ThemeSource::File(PathBuf::from("/c/t.toml"))
        );
    }

    #[test]
    fn theme_toml_in_the_directory_comes_before_the_built_in_theme() {
        assert_eq!(
            theme_source(None, &env(Some("/home/u"), None), &Config::default()),
            ThemeSource::DefaultFile(PathBuf::from("/home/u/.config/kiri/theme.toml"))
        );
        assert_eq!(
            theme_source(None, &env(None, None), &Config::default()),
            ThemeSource::BuiltIn
        );
    }
}
