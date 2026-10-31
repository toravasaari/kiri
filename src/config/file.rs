//! The configuration file: where it is, what it may set, and reading it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::ErrorKind;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde::de::{self, Deserializer, EnumAccess, MapAccess, VariantAccess};

use super::environment::{Environment, directory, home_directory};
use super::types::{FrontMatter, Named, Origin, Passthrough};
use crate::render::{Headings, RenderErrors, TableRows};
use crate::session::settings::{Follow, FollowJump, InitialLoad, Paging, SearchCase};

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
    /// Whether the pager takes the mouse, to scroll on the wheel.
    pub mouse: Option<bool>,
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
pub(super) fn parse(text: &str, dir: &Path, env: &Environment) -> Result<Config, String> {
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
        mouse: file.mouse,
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
    mouse: Option<bool>,
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
