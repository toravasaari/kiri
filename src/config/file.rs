//! The configuration file: where it is, what it may set, and reading it.

use std::collections::BTreeMap;
use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess};

use super::environment::{Environment, directory, home_directory};
use super::registry::{Config, DIAGRAMS_KEYS, DiagramsFile};
use super::types::Origin;

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
pub(super) fn parse(text: &str, dir: &Path, env: &Environment) -> Result<Config, String> {
    let mut config: Config = toml::from_str(text).map_err(|error| error.to_string())?;
    config.theme = match config.theme {
        Some(theme) => Some(match theme.strip_prefix("~") {
            Ok(rest) if !rest.as_os_str().is_empty() => match home_directory(env) {
                Some(home) => home.join(rest),
                None => return Err(format!("theme {}: HOME is not set", theme.display())),
            },
            _ => dir.join(theme),
        }),
        None => None,
    };
    Ok(config)
}

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

impl<'de> Deserialize<'de> for DiagramsFile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = DiagramsFile;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("true or false, or a [diagrams] table")
            }

            fn visit_bool<E: de::Error>(self, on: bool) -> Result<DiagramsFile, E> {
                Ok(DiagramsFile {
                    enabled: Some(on),
                    ..DiagramsFile::default()
                })
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<DiagramsFile, A::Error> {
                let mut diagrams = DiagramsFile::default();
                // The key each language's table was given under.
                let mut tables = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    if diagrams.read_key(&key, &mut map)? {
                        continue;
                    }
                    match key.as_str() {
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
