//! Resolving: the setting that the flags, the environment, the file and the
//! default choose between them.

use std::path::Path;

use super::environment::{Environment, directory};
use super::registry::{Config, Options, Settings, choose};
use super::types::{Env, Origin, Setting, ThemeSource, Value, is_name};
use crate::typeset::diagnostic::{Code, Diagnostic};
use crate::typeset::theme::{Paper, Theme};

/// What a theme value is: a name when it has no path separator and no
/// `.toml` ending, which is `NAME.toml` in the themes directory, or else
/// the built-in paper theme `light` or `dark`, and else `Unknown`. A value
/// with a path separator or a `.toml` ending is a path.
fn theme_value(value: &Path, env: &Environment) -> ThemeSource {
    let name = value.to_str().filter(|_| is_name(value));
    let Some(name) = name else {
        return ThemeSource::File(value.to_owned());
    };
    let themes = directory(env).map(|dir| dir.value.join("themes"));
    let path = themes.as_ref().map(|dir| dir.join(format!("{name}.toml")));
    if let Some(path) = path.filter(|path| path.is_file()) {
        return ThemeSource::Named {
            name: name.to_owned(),
            path,
        };
    }
    match name {
        "light" => ThemeSource::Paper(Paper::Light),
        "dark" => ThemeSource::Paper(Paper::Dark),
        _ => {
            let mut names = vec!["dark".to_owned(), "light".to_owned()];
            let files = themes.and_then(|dir| std::fs::read_dir(dir).ok());
            for file in files.into_iter().flatten().flatten() {
                let path = file.path();
                let stem = path.file_stem().and_then(|stem| stem.to_str());
                if path.extension().is_some_and(|ext| ext == "toml")
                    && let Some(stem) = stem
                {
                    names.push(stem.to_owned());
                }
            }
            names.sort();
            names.dedup();
            let message = format!(
                "theme `{name}` is not one of the themes: {} \
                 (for a file in this directory, write ./{name})",
                names.join(", ")
            );
            ThemeSource::Unknown {
                name: name.to_owned(),
                message,
            }
        }
    }
}

/// Chooses the theme and tells where it comes from, strongest first:
/// `--theme` (`--light` and `--dark` are `--theme light` and `--theme
/// dark`), `KIRI_THEME` when it is not empty, the configuration file,
/// `theme.toml` in the configuration directory when it exists, the built-in
/// theme. The first three take a name or a path.
pub(super) fn theme_source(
    options: &Options,
    env: &Environment,
    config: &Config,
) -> Setting<ThemeSource> {
    let theme = (options.theme.clone()).map(|theme| theme.map(|theme| theme_value(&theme, env)));
    let name = "KIRI_THEME";
    let variable = |_: &mut Vec<Diagnostic>| {
        let path = env.get(name).filter(|path| !path.is_empty())?;
        let theme = theme_value(Path::new(path), env);
        Some(Setting::new(theme, Origin::Environment(name)))
    };
    let file = directory(env).map(|dir| dir.value.join("theme.toml"));
    let default = match file.filter(|path| path.exists()) {
        Some(path) => ThemeSource::DefaultFile(path),
        None => ThemeSource::BuiltIn,
    };
    let file = config.theme.as_deref().map(|theme| theme_value(theme, env));
    resolve(theme, variable, file, default, &mut Vec::new())
}

/// A setting and where it comes from, strongest first: the command line
/// (`flag`), the environment (`variable`, read only without the flag),
/// the configuration file (`file`), `default`. What the variable has to
/// say about a value it leaves out goes in `warnings`.
pub(super) fn resolve<T>(
    flag: Option<Setting<T>>,
    variable: impl FnOnce(&mut Vec<Diagnostic>) -> Option<Setting<T>>,
    file: Option<T>,
    default: T,
    warnings: &mut Vec<Diagnostic>,
) -> Setting<T> {
    flag.or_else(|| variable(warnings))
        .or_else(|| file.map(|value| Setting::new(value, Origin::File)))
        .unwrap_or_else(|| Setting::new(default, Origin::Default))
}

/// The environment variable `var`, for `resolve`, which takes the names
/// and the aliases of `T`'s values; empty is unset. Any other value is
/// left out with a warning.
pub(super) fn variable<T: Value>(
    env: &Environment,
    var: Env,
) -> impl FnOnce(&mut Vec<Diagnostic>) -> Option<Setting<T>> {
    move |warnings| {
        let name = var.name()?;
        let value = env.get(name).filter(|value| !value.is_empty())?;
        let chosen = value.to_str().and_then(T::from_text);
        if chosen.is_none() {
            warnings.push(Diagnostic::new(
                Code::Config,
                format!(
                    "{name}: `{}` is not {}; left out",
                    value.to_string_lossy(),
                    T::expected(),
                ),
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
) -> (Vec<Diagnostic>, Result<Settings, String>) {
    let mut choices = choose(options, env, config);
    let warnings = std::mem::take(&mut choices.warnings);
    let settings = load_theme(&choices.theme.value).map(|theme| choices.with_theme(theme));
    (warnings, settings)
}

/// The theme that `source` names.
pub fn load_theme(source: &ThemeSource) -> Result<Theme, String> {
    match source {
        ThemeSource::File(path)
        | ThemeSource::DefaultFile(path)
        | ThemeSource::Named { path, .. } => crate::load::theme::load(path),
        ThemeSource::Unknown { message, .. } => Err(message.clone()),
        ThemeSource::BuiltIn => Ok(Theme::default()),
        ThemeSource::Paper(paper) => Ok(Theme::paper(*paper)),
    }
}
