//! Settings from the environment, read once at start-up.
//!
//! Everything here takes the environment as a value, so the rules can be
//! tested without touching the process environment.

use std::ffi::OsString;
use std::path::PathBuf;

/// The environment variables kiri reads for its settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    pub home: Option<OsString>,
    pub xdg_config_home: Option<OsString>,
}

impl Environment {
    /// The environment of this process.
    pub fn from_process() -> Self {
        Self {
            home: std::env::var_os("HOME"),
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn env(home: Option<&str>, xdg: Option<&str>) -> Environment {
        Environment {
            home: home.map(OsString::from),
            xdg_config_home: xdg.map(OsString::from),
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
}
