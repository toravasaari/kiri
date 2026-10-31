//! Resolving: the setting that the flags, the environment, the file and the
//! default choose between them.

use std::path::PathBuf;

use super::environment::{Environment, directory};
use super::registry::{Config, Options, Settings, choose};
use super::types::{Env, Origin, Setting, ThemeSource, Value};
use crate::typeset::theme::Theme;

/// Chooses the theme and tells where it comes from, strongest first:
/// `--theme`, a paper theme from `--light` or `--dark`, `KIRI_THEME` when it
/// is not empty, the configuration file, `theme.toml` in the configuration
/// directory when it exists, the built-in theme.
pub(super) fn theme_source(
    options: &Options,
    env: &Environment,
    config: &Config,
) -> Setting<ThemeSource> {
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

/// A setting and where it comes from, strongest first: the command line
/// (`flag`), the environment (`variable`, read only without the flag),
/// the configuration file (`file`), `default`. What the variable has to
/// say about a value it leaves out goes in `warnings`.
pub(super) fn resolve<T>(
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
pub(super) fn unset<T>(_: &mut Vec<String>) -> Option<Setting<T>> {
    None
}

/// The environment variable `var`, for `resolve`, which takes the names
/// and the aliases of `T`'s values; empty is unset. Any other value is
/// left out with a warning.
pub(super) fn variable<T: Value>(
    env: &Environment,
    var: Env,
) -> impl FnOnce(&mut Vec<String>) -> Option<Setting<T>> {
    move |warnings| {
        let name = var.name()?;
        let value = env.get(name).filter(|value| !value.is_empty())?;
        let chosen = value.to_str().and_then(T::from_text);
        if chosen.is_none() {
            warnings.push(format!(
                "{name}: `{}` is not {}; left out",
                value.to_string_lossy(),
                T::expected(),
            ));
        }
        let invert = |value: T| {
            if var.inverted() {
                value.inverted()
            } else {
                value
            }
        };
        chosen.map(|value| Setting::new(invert(value), Origin::Environment(name)))
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
        ThemeSource::File(path) | ThemeSource::DefaultFile(path) => {
            crate::typeset::theme::load(path)
        }
        ThemeSource::BuiltIn => Ok(Theme::default()),
        ThemeSource::Paper(paper) => Ok(Theme::paper(*paper)),
    }
}
