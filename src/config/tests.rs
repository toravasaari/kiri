use super::*;
use crate::session::settings::{Follow, FollowJump, InitialLoad, Paging, SearchCase};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::render::{FrontMatterStyle, Headings, RenderErrors, TableRows};
use crate::term::Mode;
use crate::typeset::theme::{Paper, Theme};

fn env(home: Option<&str>, xdg: Option<&str>) -> Environment {
    Environment {
        home: home.map(OsString::from),
        xdg_config_home: xdg.map(OsString::from),
        ..Environment::default()
    }
}

#[test]
fn each_variable_is_read_and_got_by_its_name() {
    let env = Environment::read(|name| Some(OsString::from(format!("<{name}>"))));
    for (name, field) in [
        ("HOME", &env.home),
        ("XDG_CONFIG_HOME", &env.xdg_config_home),
        ("TMPDIR", &env.tmpdir),
        ("NPM_CONFIG_PREFIX", &env.npm_config_prefix),
        ("VTE_VERSION", &env.vte_version),
    ] {
        let value = OsString::from(format!("<{name}>"));
        assert_eq!(field.as_ref(), Some(&value), "{name}");
        assert_eq!(env.get(name), Some(value.as_os_str()), "{name}");
    }
    assert_eq!(env.get("SHELL"), None);
}

#[test]
fn the_variables_of_the_settings_come_from_the_registry() {
    let env = Environment::read(|name| Some(OsString::from(format!("<{name}>"))));
    let names: Vec<&str> = SETTINGS
        .iter()
        .filter_map(|entry| entry.env.name())
        .collect();
    assert!(names.contains(&"KIRI_PAGING") && names.contains(&"KIRI_NO_IMAGES"));
    for name in names {
        let value = OsString::from(format!("<{name}>"));
        assert_eq!(env.get(name), Some(value.as_os_str()), "{name}");
    }
}

#[test]
fn a_setting_is_named_after_its_key() {
    // `diagrams.timeout` is `diagram_timeout`, and the key `diagrams` alone
    // is `diagrams`. A key with a `<language>` in it is a table of
    // languages, whose settings are maps.
    for entry in SETTINGS {
        let Some(key) = entry.key.filter(|key| !key.contains('<')) else {
            continue;
        };
        let name = key.replace("diagrams.", "diagram_");
        assert_eq!(entry.field, name, "{key}");
    }
}

#[test]
fn the_registry_has_each_setting_once() {
    let mut fields: Vec<&str> = SETTINGS.iter().map(|entry| entry.field).collect();
    fields.sort_unstable();
    let count = fields.len();
    fields.dedup();
    assert_eq!(fields.len(), count);
    let mut flags: Vec<&str> = SETTINGS.iter().filter_map(|entry| entry.flag).collect();
    flags.sort_unstable();
    let count = flags.len();
    flags.dedup();
    assert_eq!(flags.len(), count);
}

#[test]
fn the_directory_is_under_xdg_config_home() {
    assert_eq!(
        directory(&env(Some("/home/u"), Some("/xdg"))).map(|dir| dir.value),
        Some(PathBuf::from("/xdg/kiri"))
    );
}

#[test]
fn the_directory_falls_back_to_dot_config_in_home() {
    let expected = Some(PathBuf::from("/home/u/.config/kiri"));
    let dir = |home, xdg| directory(&env(home, xdg)).map(|dir| dir.value);
    assert_eq!(dir(Some("/home/u"), None), expected);
    assert_eq!(dir(Some("/home/u"), Some("")), expected);
}

#[test]
fn the_directory_tells_where_it_comes_from() {
    let origin = |home, xdg| directory(&env(home, xdg)).unwrap().origin;
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
        Some(Setting::new(
            PathBuf::from("/home/u/.config/kiri"),
            Origin::Default
        ))
    );
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
fn the_cache_and_temporary_directories_tell_where_they_come_from() {
    let env = |cache: Option<&str>, tmp: Option<&str>| Environment {
        home: Some("/home/u".into()),
        xdg_cache_home: cache.map(OsString::from),
        tmpdir: tmp.map(OsString::from),
        ..Environment::default()
    };
    let cache = |cache| cache_directory_setting(&env(cache, None));
    let variable = Origin::Environment("XDG_CACHE_HOME");
    let dot_cache = PathBuf::from("/home/u/.cache/kiri");
    assert_eq!(
        cache(Some("/c")),
        Some(Setting::new(PathBuf::from("/c/kiri"), variable))
    );
    assert_eq!(
        cache(Some("c")),
        Some(Setting::new(dot_cache, Origin::Default))
    );
    let temp = |tmp| temp_directory_setting(&env(None, tmp));
    let variable = Origin::Environment("TMPDIR");
    // Relative is not left out, as the standard library does not.
    assert_eq!(temp(Some("t")), Setting::new(PathBuf::from("t"), variable));
    assert_eq!(
        temp(Some("/t")),
        Setting::new(PathBuf::from("/t"), variable)
    );
    let tmp = Setting::new(PathBuf::from("/tmp"), Origin::Default);
    assert_eq!(temp(Some("")), tmp);
    assert_eq!(temp(None), tmp);
    assert_eq!(temp_directory(&env(None, Some("/t"))), PathBuf::from("/t"));
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

/// No environment variable, for `resolve`.
fn unset<T>(_: &mut Vec<String>) -> Option<Setting<T>> {
    None
}

/// `value` from the flag `name`, as the command line gives it.
fn flag<T>(value: T, name: &'static str) -> Option<Setting<T>> {
    Some(Setting::new(value, Origin::Flag(name)))
}

/// The theme from `--theme` with `cli` or else the flag for `paper`, `env`
/// and `config`.
fn theme_source(
    cli: Option<&Path>,
    paper: Option<Paper>,
    env: &Environment,
    config: &Config,
) -> Setting<ThemeSource> {
    // `--light` and `--dark` are `--theme light` and `--theme dark`, and
    // clap does not take them with `--theme`.
    let theme = match paper {
        Some(Paper::Light) => flag(PathBuf::from("light"), "--light"),
        Some(Paper::Dark) => flag(PathBuf::from("dark"), "--dark"),
        None => cli.and_then(|path| flag(path.to_owned(), "--theme")),
    };
    let options = Options {
        theme,
        ..Options::default()
    };
    super::theme_source(&options, env, config)
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
    env.variables.insert("KIRI_THEME", "env.toml".into());
    assert_eq!(
        theme_source(Some(Path::new("cli.toml")), None, &env, &Config::default()).value,
        ThemeSource::File(PathBuf::from("cli.toml"))
    );
}

#[test]
fn kiri_theme_comes_before_the_configuration_file() {
    let mut env = env(Some("/home/u"), None);
    env.variables.insert("KIRI_THEME", "env.toml".into());
    assert_eq!(
        theme_source(None, None, &env, &with_theme("/c/t.toml")).value,
        ThemeSource::File(PathBuf::from("env.toml"))
    );
}

#[test]
fn an_empty_kiri_theme_is_unset() {
    let mut env = env(Some("/home/u"), None);
    env.variables.insert("KIRI_THEME", "".into());
    assert_eq!(
        theme_source(None, None, &env, &with_theme("/c/t.toml")).value,
        ThemeSource::File(PathBuf::from("/c/t.toml"))
    );
}

#[test]
fn a_paper_flag_chooses_the_paper_theme() {
    let env = env(Some("/home/u"), None);
    let config = Config::default();
    assert_eq!(
        theme_source(None, Some(Paper::Dark), &env, &config).value,
        ThemeSource::Paper(Paper::Dark)
    );
}

#[test]
fn a_paper_theme_comes_before_kiri_theme_and_the_configuration_file() {
    let mut env = env(Some("/home/u"), None);
    env.variables.insert("KIRI_THEME", "env.toml".into());
    assert_eq!(
        theme_source(None, Some(Paper::Light), &env, &with_theme("/c/t.toml")).value,
        ThemeSource::Paper(Paper::Light)
    );
}

#[test]
fn theme_toml_in_the_directory_comes_before_the_built_in_theme() {
    let scratch = crate::testing::Scratch::new("config-theme-toml");
    let home = scratch.to_str().unwrap();
    let source = |home| theme_source(None, None, &env(home, None), &Config::default());
    // Only when it exists.
    let built_in = Setting::new(ThemeSource::BuiltIn, Origin::Default);
    assert_eq!(source(Some(home)), built_in);
    assert_eq!(source(None), built_in);
    let theme = scratch.join(".config/kiri/theme.toml");
    std::fs::create_dir_all(theme.parent().unwrap()).unwrap();
    std::fs::write(&theme, "").unwrap();
    let default = Setting::new(ThemeSource::DefaultFile(theme), Origin::Default);
    assert_eq!(source(Some(home)), default);
}

#[test]
fn the_images_key_turns_images_on_or_off() {
    check_key(
        |config| config.images,
        &[("images = false\n", false), ("images = true\n", true)],
        &[],
    );
}

#[test]
fn the_paging_key_takes_always_auto_or_never() {
    check_key(
        |config| config.paging,
        &[
            ("paging = \"always\"\n", Paging::Always),
            ("paging = \"auto\"\n", Paging::Auto),
            ("paging = \"never\"\n", Paging::Never),
        ],
        &[],
    );
}

#[test]
fn the_search_case_key_takes_smart_or_literal() {
    check_key(
        |config| config.search_case,
        &[
            ("search_case = \"smart\"\n", SearchCase::Smart),
            ("search_case = \"literal\"\n", SearchCase::Literal),
        ],
        &["search_case = true\n"],
    );
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
fn the_follow_key_takes_on_off_or_auto() {
    check_key(
        |config| config.follow,
        &[
            ("follow = \"on\"\n", Follow::On),
            ("follow = \"off\"\n", Follow::Off),
            ("follow = \"auto\"\n", Follow::Auto),
        ],
        &["follow = true\n"],
    );
}

#[test]
fn the_follow_jump_key_takes_first_last_or_most() {
    check_key(
        |config| config.follow_jump,
        &[
            ("follow_jump = \"first\"\n", FollowJump::First),
            ("follow_jump = \"last\"\n", FollowJump::Last),
            ("follow_jump = \"most\"\n", FollowJump::Most),
        ],
        &["follow_jump = \"middle\"\n"],
    );
}

#[test]
fn the_initial_load_key_takes_wait_or_background() {
    check_key(
        |config| config.initial_load,
        &[
            ("initial_load = \"wait\"\n", InitialLoad::Wait),
            ("initial_load = \"background\"\n", InitialLoad::Background),
        ],
        &["initial_load = \"later\"\n", "initial_load = true\n"],
    );
}

#[test]
fn the_initial_load_waits_unless_told_otherwise() {
    let env = Environment::default();
    let choices = choose(&Options::default(), &env, &Config::default());
    let wait = Setting::new(InitialLoad::Wait, Origin::Default);
    assert_eq!(choices.initial_load, wait);
    let config = Config {
        initial_load: Some(InitialLoad::Background),
        ..Config::default()
    };
    let choices = choose(&Options::default(), &env, &config);
    let file = Setting::new(InitialLoad::Background, Origin::File);
    assert_eq!(choices.initial_load, file);
    let settings = choices.with_theme(Theme::default());
    assert_eq!(settings.initial_load, InitialLoad::Background);
    let flag = Setting::new(InitialLoad::Wait, Origin::Flag("--initial-load"));
    let options = Options {
        initial_load: Some(flag.clone()),
        ..Options::default()
    };
    let variable = vars(&[("KIRI_INITIAL_LOAD", "background")]);
    assert_eq!(choose(&options, &variable, &config).initial_load, flag);
}

#[test]
fn the_passthrough_key_takes_detect_none_or_tmux() {
    check_key(
        |config| config.passthrough,
        &[
            ("passthrough = \"detect\"\n", Passthrough::Detect),
            ("passthrough = \"none\"\n", Passthrough::None),
            ("passthrough = \"tmux\"\n", Passthrough::Tmux),
        ],
        &["passthrough = \"always\"\n", "passthrough = true\n"],
    );
}

#[test]
fn passthrough_detects_unless_told_otherwise() {
    let env = Environment::default();
    let choices = choose(&Options::default(), &env, &Config::default());
    let detect = Setting::new(Passthrough::Detect, Origin::Default);
    assert_eq!(choices.passthrough, detect);
    let config = Config {
        passthrough: Some(Passthrough::None),
        ..Config::default()
    };
    let choices = choose(&Options::default(), &env, &config);
    let file = Setting::new(Passthrough::None, Origin::File);
    assert_eq!(choices.passthrough, file);
    let settings = choices.with_theme(Theme::default());
    assert_eq!(settings.passthrough, Passthrough::None);
    let flag = Setting::new(Passthrough::Tmux, Origin::Flag("--passthrough"));
    let options = Options {
        passthrough: Some(flag.clone()),
        ..Options::default()
    };
    let variable = vars(&[("KIRI_PASSTHROUGH", "detect")]);
    assert_eq!(choose(&options, &variable, &config).passthrough, flag);
}

#[test]
fn auto_follow_jumps_to_the_first_change_unless_told_otherwise() {
    let env = Environment::default();
    let choices = choose(&Options::default(), &env, &Config::default());
    let first = Setting::new(FollowJump::First, Origin::Default);
    assert_eq!(choices.follow_jump, first);
    let config = Config {
        follow_jump: Some(FollowJump::Most),
        ..Config::default()
    };
    let choices = choose(&Options::default(), &env, &config);
    assert_eq!(
        choices.follow_jump,
        Setting::new(FollowJump::Most, Origin::File)
    );
    let settings = choices.with_theme(Theme::default());
    assert_eq!(settings.follow_jump, FollowJump::Most);
    let flag = Setting::new(FollowJump::Last, Origin::Flag("--follow-jump"));
    let options = Options {
        follow_jump: Some(flag.clone()),
        ..Options::default()
    };
    assert_eq!(choose(&options, &env, &config).follow_jump, flag);
}

#[test]
fn following_is_off_unless_the_configuration_file_or_a_flag_says_otherwise() {
    let env = Environment::default();
    let choices = choose(&Options::default(), &env, &Config::default());
    assert_eq!(choices.follow, Setting::new(Follow::Off, Origin::Default));
    let config = Config {
        follow: Some(Follow::Auto),
        ..Config::default()
    };
    let choices = choose(&Options::default(), &env, &config);
    assert_eq!(choices.follow, Setting::new(Follow::Auto, Origin::File));
    let settings = choices.with_theme(Theme::default());
    assert_eq!(settings.follow, Follow::Auto);
    let options = Options {
        follow: Some(Setting::new(Follow::Off, Origin::Flag("-F"))),
        ..Options::default()
    };
    let choices = choose(&options, &env, &config);
    assert_eq!(
        choices.follow,
        Setting::new(Follow::Off, Origin::Flag("-F"))
    );
}

#[test]
fn the_front_matter_key_takes_table_code_hide_or_off() {
    check_key(
        |config| config.front_matter,
        &[
            ("front_matter = \"table\"\n", FrontMatter::Table),
            ("front_matter = \"code\"\n", FrontMatter::Code),
            ("front_matter = \"hide\"\n", FrontMatter::Hide),
            ("front_matter = \"off\"\n", FrontMatter::Off),
        ],
        &["front-matter = \"code\"\n"],
    );
}

#[test]
fn the_headings_key_takes_auto_sized_images_or_plain() {
    check_key(
        |config| config.headings,
        &[
            ("headings = \"auto\"\n", Headings::Auto),
            ("headings = \"sized\"\n", Headings::Sized),
            ("headings = \"images\"\n", Headings::Images),
            ("headings = \"plain\"\n", Headings::Plain),
        ],
        &["headings = \"big\"\n"],
    );
}

#[test]
fn the_mouse_key_turns_the_mouse_on_or_off() {
    check_key(
        |config| config.mouse,
        &[("mouse = false\n", false), ("mouse = true\n", true)],
        &["mouse = \"off\"\n"],
    );
}

#[test]
fn the_diagrams_key_turns_diagrams_on_or_off() {
    check_key(
        |config| config.diagrams.enabled,
        &[("diagrams = false\n", false), ("diagrams = true\n", true)],
        &[],
    );
}

#[test]
fn the_diagrams_table_turns_diagrams_on_or_off_beside_a_language_table() {
    let parsed = |text| parse(text, Path::new("/c"), &Environment::default());
    let enabled = |text| parsed(text).map(|config| config.diagrams.enabled);
    assert_eq!(enabled("[diagrams]\nenabled = false\n"), Ok(Some(false)));
    assert_eq!(enabled("[diagrams]\nenabled = true\n"), Ok(Some(true)));
    assert_eq!(enabled("[diagrams]\n"), Ok(None));
    let both = "[diagrams]\nenabled = false\n\n\
                    [diagrams.mermaid]\ncommand = [\"mmdc\", \"{input}\", \"{output}\"]\n";
    let config = parsed(both).map(|config| (config.diagrams.enabled, config.diagrams.commands));
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
    let timeout = |text: &str| parsed(text).map(|config| config.diagrams.timeout);
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
    let choices = |config: &Config| choose(&Options::default(), &Environment::default(), config);
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
    let panel = |text: &str| parsed(text).map(|config| config.diagrams.background_panel);
    assert_eq!(
        panel("[diagrams]\nbackground_panel = false\n"),
        Ok(Some(false))
    );
    assert_eq!(
        panel("[diagrams]\nbackground_panel = true\n"),
        Ok(Some(true))
    );
    assert_eq!(panel("[diagrams]\n"), Ok(None));
    let choices = |config: &Config| choose(&Options::default(), &Environment::default(), config);
    assert_eq!(
        choices(&Config::default()).diagram_background_panel,
        Setting::new(true, Origin::Default)
    );
    let config = parsed("[diagrams]\nbackground_panel = false\n").unwrap();
    assert_eq!(
        choices(&config).diagram_background_panel,
        Setting::new(false, Origin::File)
    );
    assert!(
        !choices(&config)
            .with_theme(Theme::default())
            .diagram_background_panel
    );
}

#[test]
fn a_diagram_language_table_turns_the_search_for_its_program_off() {
    let parsed = |text: &str| parse(text, Path::new("/c"), &Environment::default());
    let off = |text: &str| {
        parsed(text).map(|config| (config.diagrams.no_search, config.diagrams.commands))
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
    let both =
        "[diagrams.mermaid]\nauto_detect = false\ncommand = [\"m\", \"{input}\", \"{output}\"]\n";
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
        "[diagrams.Mermaid]\ncommand = [\"m\", \"{input}\", \"{output}\"]\n\
             auto_detect = false\n",
    );
    let command = ["m", "{input}", "{output}"].map(String::from).to_vec();
    assert_eq!(
        config.map(|config| (config.diagrams.commands, config.diagrams.no_search)),
        Ok((
            BTreeMap::from([("mermaid".to_owned(), command)]),
            BTreeSet::from(["mermaid".to_owned()])
        ))
    );
}

#[test]
fn two_tables_for_one_language_are_an_error_naming_both() {
    // Keys that differ only in case, which TOML takes as two.
    let text = "[diagrams.Mermaid]\ncommand = [\"m\", \"{input}\", \"{output}\"]\n\n\
                    [diagrams.mermaid]\nauto_detect = false\n";
    let parsed = parse(text, Path::new("/c"), &Environment::default()).map(|_| ());
    let expected = "`Mermaid` and `mermaid` are tables for the same diagram language; \
                        give only one";
    assert!(
        parsed.as_ref().is_err_and(|error| error.contains(expected)),
        "{parsed:?}"
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
        config.diagrams.commands,
        BTreeMap::from([("mermaid".to_owned(), command.map(String::from).to_vec())])
    );
    // Without a switch of its own.
    assert_eq!(config.diagrams.enabled, None);
    let choices = choose(&Options::default(), &Environment::default(), &config);
    assert_eq!(choices.diagram_commands, config.diagrams.commands);
    let settings = choices.with_theme(Theme::default());
    assert_eq!(settings.diagram_commands, config.diagrams.commands);
    assert!(
        parsed("diagrams = true\n")
            .unwrap()
            .diagrams
            .commands
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

#[test]
fn the_table_rows_key_takes_each_value() {
    check_key(
        |config| config.table_rows,
        &[
            ("table_rows = \"off\"\n", TableRows::Off),
            ("table_rows = \"zebra\"\n", TableRows::Zebra),
            ("table_rows = \"lines\"\n", TableRows::Lines),
            ("table_rows = \"indent\"\n", TableRows::Indent),
            ("table_rows = \"underline\"\n", TableRows::Underline),
        ],
        &["table_rows = true\n"],
    );
}

#[test]
fn the_render_errors_key_takes_each_value() {
    check_key(
        |config| config.render_errors,
        &[
            ("render_errors = \"inline\"\n", RenderErrors::Inline),
            ("render_errors = \"stderr\"\n", RenderErrors::Stderr),
            ("render_errors = \"both\"\n", RenderErrors::Both),
        ],
        &["render_errors = true\n"],
    );
    let empty = parse("", Path::new("/c"), &Environment::default()).unwrap();
    assert_eq!(empty.render_errors, None);
}

fn no_options() -> Options {
    Options::default()
}

/// An environment with only the variables `vars`.
fn vars(vars: &[(&str, &str)]) -> Environment {
    Environment::read(|name| {
        let mut found = vars.iter().filter(|(var, _)| *var == name);
        found.next().map(|(_, value)| OsString::from(value))
    })
}

#[test]
fn a_setting_comes_from_the_flag_the_variable_the_file_or_the_default() {
    let flag = || Some(Setting::new(1, Origin::Flag("--n")));
    let variable = |warnings: &mut Vec<String>| {
        warnings.push("read".to_owned());
        Some(Setting::new(2, Origin::Environment("N")))
    };
    let mut warnings = Vec::new();
    let chosen = resolve(flag(), variable, Some(3), 4, &mut warnings);
    assert_eq!(chosen, Setting::new(1, Origin::Flag("--n")));
    // The variable is read only without the flag, so it warns only then.
    assert!(warnings.is_empty(), "{warnings:?}");
    let chosen = resolve(None, variable, Some(3), 4, &mut warnings);
    assert_eq!(chosen, Setting::new(2, Origin::Environment("N")));
    assert_eq!(warnings, ["read"]);
    let chosen = resolve(None, unset, Some(3), 4, &mut warnings);
    assert_eq!(chosen, Setting::new(3, Origin::File));
    let chosen = resolve(None, unset, None, 4, &mut warnings);
    assert_eq!(chosen, Setting::new(4, Origin::Default));
    assert_eq!(warnings, ["read"]);
}

#[test]
fn a_variable_takes_the_names_and_the_aliases_of_the_values() {
    let read = |value: &str| {
        let env = vars(&[("KIRI_PAGING", value)]);
        let mut warnings = Vec::new();
        let chosen = variable::<Paging>(&env, Env::is("KIRI_PAGING"))(&mut warnings);
        (chosen, warnings)
    };
    let origin = Origin::Environment("KIRI_PAGING");
    for (value, paging) in [
        ("always", Paging::Always),
        ("auto", Paging::Auto),
        ("never", Paging::Never),
        ("1", Paging::Always),
        ("0", Paging::Never),
    ] {
        let chosen = Some(Setting::new(paging, origin));
        assert_eq!(read(value), (chosen, vec![]), "{value}");
    }
    // Empty is unset, and so is a variable that is not there.
    assert_eq!(read(""), (None, vec![]));
    let env = Environment::default();
    assert_eq!(
        variable::<Paging>(&env, Env::is("KIRI_PAGING"))(&mut vec![]),
        None
    );
    let warning = "KIRI_PAGING: `Auto` is not always, auto, never, 1 or 0; left out";
    assert_eq!(read("Auto"), (None, vec![warning.to_owned()]));
}

#[test]
fn each_value_has_one_name() {
    fn check<T: Named + std::fmt::Debug>() {
        assert_eq!(T::ALL.len(), T::NAMES.len());
        for &value in T::ALL {
            assert_eq!(T::named(value.name()), Some(value), "{value:?}");
        }
        assert_eq!(T::named("nothing"), None);
    }
    check::<Paging>();
    check::<SearchCase>();
    check::<Follow>();
    check::<FollowJump>();
    check::<FrontMatter>();
    check::<Headings>();
    check::<TableRows>();
    check::<RenderErrors>();
    check::<crate::term::Mode>();
    check::<bool>();
}

/// Fails unless the configuration file text of each of `valid` sets `field`
/// of the parsed configuration to its value, and each of `invalid` is an
/// error.
fn check_key<T: PartialEq + std::fmt::Debug + Copy>(
    field: fn(&Config) -> Option<T>,
    valid: &[(&str, T)],
    invalid: &[&str],
) {
    let parsed = |text| parse(text, Path::new("/c"), &Environment::default());
    for &(text, expected) in valid {
        assert_eq!(field(&parsed(text).unwrap()), Some(expected), "{text}");
    }
    for text in invalid {
        assert!(parsed(text).is_err(), "{text}");
    }
}

/// Fails unless the variable `name` chooses `field` of the choices as
/// `values` say, before the configuration file's `file`, and leaves out
/// `wrong` with `warning`.
fn check_variable<T: Named + std::fmt::Debug>(
    field: fn(&Choices) -> &Setting<T>,
    name: &'static str,
    values: &[(&str, T)],
    file: Config,
    (wrong, warning): (&str, &str),
) {
    let choices =
        |value: &str, config: &Config| choose(&Options::default(), &vars(&[(name, value)]), config);
    for &(value, expected) in values {
        let chosen = choices(value, &file);
        let variable = Setting::new(expected, Origin::Environment(name));
        assert_eq!(field(&chosen), &variable, "{value}");
        assert!(chosen.warnings.is_empty(), "{value}: {:?}", chosen.warnings);
    }
    assert_eq!(field(&choices("", &file)).origin, Origin::File);
    let chosen = choices(wrong, &file);
    assert_eq!(field(&chosen).origin, Origin::File);
    assert_eq!(chosen.warnings, [warning]);
    let chosen = choices(wrong, &Config::default());
    assert_eq!(field(&chosen).origin, Origin::Default);
}

#[test]
fn kiri_paging_chooses_the_paging() {
    check_variable(
        |choices| &choices.paging,
        "KIRI_PAGING",
        &[
            ("always", Paging::Always),
            ("auto", Paging::Auto),
            ("never", Paging::Never),
            ("1", Paging::Always),
            ("0", Paging::Never),
        ],
        Config {
            paging: Some(Paging::Never),
            ..Config::default()
        },
        (
            "joo",
            "KIRI_PAGING: `joo` is not always, auto, never, 1 or 0; left out",
        ),
    );
}

#[test]
fn kiri_follow_chooses_following() {
    check_variable(
        |choices| &choices.follow,
        "KIRI_FOLLOW",
        &[
            ("on", Follow::On),
            ("off", Follow::Off),
            ("auto", Follow::Auto),
            ("1", Follow::On),
            ("0", Follow::Off),
        ],
        Config {
            follow: Some(Follow::Off),
            ..Config::default()
        },
        (
            "joo",
            "KIRI_FOLLOW: `joo` is not on, off, auto, 1 or 0; left out",
        ),
    );
}

#[test]
fn kiri_follow_jump_chooses_where_auto_follow_jumps() {
    check_variable(
        |choices| &choices.follow_jump,
        "KIRI_FOLLOW_JUMP",
        &[
            ("first", FollowJump::First),
            ("last", FollowJump::Last),
            ("most", FollowJump::Most),
        ],
        Config {
            follow_jump: Some(FollowJump::Last),
            ..Config::default()
        },
        (
            "joo",
            "KIRI_FOLLOW_JUMP: `joo` is not first, last or most; left out",
        ),
    );
}

#[test]
fn kiri_mouse_chooses_whether_the_pager_takes_the_mouse() {
    check_variable(
        |choices| &choices.mouse,
        "KIRI_MOUSE",
        &[("on", true), ("off", false), ("1", true), ("0", false)],
        Config {
            mouse: Some(false),
            ..Config::default()
        },
        ("no", "KIRI_MOUSE: `no` is not on, off, 1 or 0; left out"),
    );
}

#[test]
fn kiri_initial_load_chooses_how_the_pager_initially_loads_the_images() {
    check_variable(
        |choices| &choices.initial_load,
        "KIRI_INITIAL_LOAD",
        &[
            ("wait", InitialLoad::Wait),
            ("background", InitialLoad::Background),
        ],
        Config {
            initial_load: Some(InitialLoad::Wait),
            ..Config::default()
        },
        (
            "later",
            "KIRI_INITIAL_LOAD: `later` is not wait or background; left out",
        ),
    );
}

#[test]
fn kiri_passthrough_chooses_the_tmux_passthrough() {
    check_variable(
        |choices| &choices.passthrough,
        "KIRI_PASSTHROUGH",
        &[
            ("detect", Passthrough::Detect),
            ("none", Passthrough::None),
            ("tmux", Passthrough::Tmux),
        ],
        Config {
            passthrough: Some(Passthrough::None),
            ..Config::default()
        },
        (
            "always",
            "KIRI_PASSTHROUGH: `always` is not detect, none or tmux; left out",
        ),
    );
}

#[test]
fn kiri_front_matter_chooses_the_front_matter() {
    check_variable(
        |choices| &choices.front_matter,
        "KIRI_FRONT_MATTER",
        &[
            ("table", FrontMatter::Table),
            ("code", FrontMatter::Code),
            ("hide", FrontMatter::Hide),
            ("off", FrontMatter::Off),
        ],
        Config {
            front_matter: Some(FrontMatter::Hide),
            ..Config::default()
        },
        (
            "Code",
            "KIRI_FRONT_MATTER: `Code` is not table, code, hide or off; left out",
        ),
    );
}

#[test]
fn kiri_headings_chooses_how_headings_show() {
    check_variable(
        |choices| &choices.headings,
        "KIRI_HEADINGS",
        &[
            ("auto", Headings::Auto),
            ("sized", Headings::Sized),
            ("images", Headings::Images),
            ("plain", Headings::Plain),
        ],
        Config {
            headings: Some(Headings::Sized),
            ..Config::default()
        },
        (
            "big",
            "KIRI_HEADINGS: `big` is not auto, sized, images or plain; left out",
        ),
    );
}

#[test]
fn headings_are_auto_unless_told_otherwise() {
    let choices = choose(
        &Options::default(),
        &Environment::default(),
        &Config::default(),
    );
    let auto = Setting::new(Headings::Auto, Origin::Default);
    assert_eq!(choices.headings, auto);
    let options = Options {
        headings: flag(Headings::Plain, "--headings"),
        ..Options::default()
    };
    let env = vars(&[("KIRI_HEADINGS", "images")]);
    let config = Config {
        headings: Some(Headings::Sized),
        ..Config::default()
    };
    let choices = choose(&options, &env, &config);
    assert_eq!(Some(choices.headings.clone()), options.headings);
    let settings = choices.with_theme(Theme::default());
    assert_eq!(settings.headings, Headings::Plain);
}

#[test]
fn kiri_table_rows_chooses_the_table_rows() {
    check_variable(
        |choices| &choices.table_rows,
        "KIRI_TABLE_ROWS",
        &[
            ("off", TableRows::Off),
            ("zebra", TableRows::Zebra),
            ("lines", TableRows::Lines),
            ("indent", TableRows::Indent),
            ("underline", TableRows::Underline),
        ],
        Config {
            table_rows: Some(TableRows::Lines),
            ..Config::default()
        },
        (
            "stripes",
            "KIRI_TABLE_ROWS: `stripes` is not off, zebra, lines, indent or underline; left out",
        ),
    );
}

#[test]
fn kiri_diagrams_turns_diagrams_on_or_off() {
    check_variable(
        |choices| &choices.diagrams,
        "KIRI_DIAGRAMS",
        &[("on", true), ("off", false), ("1", true), ("0", false)],
        Config {
            diagrams: DiagramsFile {
                enabled: Some(false),
                ..DiagramsFile::default()
            },
            ..Config::default()
        },
        (
            "yes",
            "KIRI_DIAGRAMS: `yes` is not on, off, 1 or 0; left out",
        ),
    );
}

#[test]
fn kiri_render_errors_chooses_where_render_errors_go() {
    check_variable(
        |choices| &choices.render_errors,
        "KIRI_RENDER_ERRORS",
        &[
            ("inline", RenderErrors::Inline),
            ("stderr", RenderErrors::Stderr),
            ("both", RenderErrors::Both),
        ],
        Config {
            render_errors: Some(RenderErrors::Stderr),
            ..Config::default()
        },
        (
            "never",
            "KIRI_RENDER_ERRORS: `never` is not inline, stderr or both; left out",
        ),
    );
}

#[test]
fn kiri_no_images_takes_a_switch_and_warns_about_any_other_value() {
    let choose = |value: &str, file: Option<bool>| {
        let config = Config {
            images: file,
            ..Config::default()
        };
        let env = vars(&[("KIRI_NO_IMAGES", value)]);
        choose(&Options::default(), &env, &config)
    };
    let variable = Origin::Environment("KIRI_NO_IMAGES");
    // 1 and on turn images off, 0 and off turn them on, whatever the file
    // says.
    for value in ["1", "on"] {
        let choices = choose(value, Some(true));
        assert_eq!(choices.images, Setting::new(false, variable), "{value}");
        assert!(choices.warnings.is_empty(), "{value}");
    }
    for value in ["0", "off"] {
        let choices = choose(value, Some(false));
        assert_eq!(choices.images, Setting::new(true, variable), "{value}");
        assert!(choices.warnings.is_empty(), "{value}");
    }
    // Empty is unset.
    let choices = choose("", Some(false));
    assert_eq!(choices.images, Setting::new(false, Origin::File));
    assert!(choices.warnings.is_empty());
    assert_eq!(choose("", None).images, Setting::new(true, Origin::Default));
    // Any other value is left out, with a warning.
    for value in ["yes", "false"] {
        let choices = choose(value, Some(false));
        assert_eq!(choices.images, Setting::new(false, Origin::File), "{value}");
        let warning = format!("KIRI_NO_IMAGES: `{value}` is not on, off, 1 or 0; left out");
        assert_eq!(choices.warnings, [warning], "{value}");
    }
    assert_eq!(choose("yes", None).images.origin, Origin::Default);
}

#[test]
fn the_settings_have_their_defaults() {
    let choices = choose(
        &Options::default(),
        &Environment::default(),
        &Config::default(),
    );
    let default = |value| Setting::new(value, Origin::Default);
    assert_eq!(choices.margins, default(true));
    assert_eq!(choices.images, default(true));
    assert_eq!(choices.paging, Setting::new(Paging::Auto, Origin::Default));
    let front_matter = Setting::new(FrontMatter::Table, Origin::Default);
    assert_eq!(choices.front_matter, front_matter);
    let table_rows = Setting::new(TableRows::Zebra, Origin::Default);
    assert_eq!(choices.table_rows, table_rows);
    assert_eq!(choices.diagrams, default(true));
    let render_errors = Setting::new(RenderErrors::Both, Origin::Default);
    assert_eq!(choices.render_errors, render_errors);
    assert!(choices.warnings.is_empty());
}

#[test]
fn the_flags_come_before_the_variables() {
    let env = vars(&[
        ("KIRI_NO_IMAGES", "0"),
        ("KIRI_PAGING", "joo"),
        ("KIRI_FRONT_MATTER", "code"),
        ("KIRI_TABLE_ROWS", "lines"),
        ("KIRI_DIAGRAMS", "on"),
        ("KIRI_RENDER_ERRORS", "stderr"),
    ]);
    let options = Options {
        images: flag(false, "--no-images"),
        paging: flag(Paging::Never, "-P"),
        front_matter: flag(FrontMatter::Hide, "--front-matter"),
        table_rows: flag(TableRows::Off, "--table-rows"),
        diagrams: flag(false, "--diagrams"),
        render_errors: flag(RenderErrors::Inline, "--render-errors"),
        ..Options::default()
    };
    let choices = choose(&options, &env, &Config::default());
    assert_eq!(Some(choices.images), options.images);
    assert_eq!(Some(choices.paging), options.paging);
    assert_eq!(Some(choices.front_matter), options.front_matter);
    assert_eq!(Some(choices.table_rows), options.table_rows);
    assert_eq!(Some(choices.diagrams), options.diagrams);
    assert_eq!(Some(choices.render_errors), options.render_errors);
    // Without a word about the variable the flag stands in for.
    assert!(choices.warnings.is_empty(), "{:?}", choices.warnings);
    let options = Options {
        images: flag(true, "--images"),
        ..Options::default()
    };
    let choices = choose(
        &options,
        &vars(&[("KIRI_NO_IMAGES", "1")]),
        &Config::default(),
    );
    assert_eq!(Some(choices.images), options.images);
}

#[test]
fn the_settings_carry_the_values_of_the_choices() {
    let env = vars(&[("KIRI_PAGING", "joo"), ("KIRI_FRONT_MATTER", "joo")]);
    let config = Config {
        front_matter: Some(FrontMatter::Off),
        table_rows: Some(TableRows::Lines),
        diagrams: DiagramsFile {
            enabled: Some(false),
            ..DiagramsFile::default()
        },
        render_errors: Some(RenderErrors::Stderr),
        ..Config::default()
    };
    let (warnings, settings) = settings(&Options::default(), &env, &config);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    let settings = settings.unwrap();
    assert_eq!(settings.paging, Paging::Auto);
    assert_eq!(settings.front_matter, FrontMatter::Off);
    assert_eq!(settings.front_matter.style(), None);
    assert_eq!(FrontMatter::Code.style(), Some(FrontMatterStyle::Code));
    assert_eq!(settings.table_rows, TableRows::Lines);
    assert!(!settings.diagrams);
    assert_eq!(settings.render_errors, RenderErrors::Stderr);
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
        theme: flag(PathBuf::from("dark"), "--dark"),
        ..no_options()
    };
    let settings = settings(&options, &env(None, None), &Config::default())
        .1
        .unwrap();
    assert_eq!(settings.theme, Theme::paper(Paper::Dark));
    assert_eq!(settings.paper, Some(Paper::Dark));
}

#[test]
fn the_paper_comes_from_a_paper_theme_chosen_at_any_level() {
    let (_scratch, mut env) = home_with_themes("paper-levels", &["mine"]);
    let paper = |env: &Environment, theme: Option<&str>, config: &Config| {
        let options = Options {
            theme: theme.and_then(|name| flag(PathBuf::from(name), "--theme")),
            ..no_options()
        };
        settings(&options, env, config).1.unwrap().paper
    };
    let none = Config::default();
    assert_eq!(paper(&env, Some("dark"), &none), Some(Paper::Dark));
    assert_eq!(paper(&env, None, &with_theme("light")), Some(Paper::Light));
    assert_eq!(paper(&env, None, &none), None);
    assert_eq!(paper(&env, Some("mine"), &none), None);
    env.variables.insert("KIRI_THEME", "dark".into());
    assert_eq!(paper(&env, None, &none), Some(Paper::Dark));
    env.variables.insert("KIRI_THEME", "mine".into());
    assert_eq!(paper(&env, None, &with_theme("dark")), None);
}

#[test]
fn no_margins_applies_to_whichever_theme_is_chosen() {
    for paper in [None, Some(Paper::Light)] {
        let options = Options {
            theme: paper.and_then(|_| flag(PathBuf::from("light"), "--light")),
            margins: flag(false, "--no-margins"),
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
        theme: flag(PathBuf::from("/nonexistent/mine.toml"), "--theme"),
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
    env.variables.insert("KIRI_THEME", "env.toml".into());
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
        origin(Some("cli.toml"), None, &env, &config),
        Origin::Flag("--theme")
    );
    assert_eq!(
        origin(None, None, &Environment::default(), &Config::default()),
        Origin::Default
    );
}

#[test]
fn the_choices_tell_where_they_come_from() {
    let options = Options {
        margins: flag(false, "--no-margins"),
        images: flag(false, "--no-images"),
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
    let settings = settings(
        &no_options(),
        &vars(&[("KIRI_NO_IMAGES", "1")]),
        &Config::default(),
    )
    .1
    .unwrap();
    assert!(!settings.images);
}

#[test]
fn a_wrong_value_in_the_file_names_the_values_it_takes() {
    let error = |text: &str| parse(text, Path::new("/c"), &Environment::default()).unwrap_err();
    let errors: Vec<String> = [
        "paging = \"sometimes\"\n",
        "paging = true\n",
        "paging = \"1\"\n",
        "search_case = \"ignore\"\n",
        "search_case = 1\n",
        "front_matter = \"list\"\n",
        "front_matter = false\n",
        "headings = \"big\"\n",
        "table_rows = \"stripes\"\n",
        "table_rows = [\"off\"]\n",
        "render_errors = \"never\"\n",
        "render_errors = { a = 1 }\n",
        "diagrams = \"off\"\n",
        "images = \"no\"\n",
    ]
    .into_iter()
    .map(error)
    .collect();
    insta::assert_snapshot!(errors.join("---\n"));
}

#[test]
fn only_the_built_in_theme_is_marked_built_in() {
    assert!(load_theme(&ThemeSource::BuiltIn).unwrap().built_in);
    let paper = load_theme(&ThemeSource::Paper(Paper::Dark)).unwrap();
    assert!(!paper.built_in);
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes/opaque.toml");
    for source in [
        ThemeSource::File(file.clone()),
        ThemeSource::DefaultFile(file),
    ] {
        assert!(!load_theme(&source).unwrap().built_in);
    }
}

/// The general settings, which matter without the pager too, are on every
/// level: a flag, an environment variable and a key in the file. The
/// per-language diagram tables are not one value a flag or a variable can
/// take, so they are left as they are.
#[test]
fn a_general_setting_has_a_flag_a_variable_and_a_key() {
    let exempt = ["diagram_commands", "diagram_no_search"];
    let mut missing = Vec::new();
    for entry in SETTINGS
        .iter()
        .filter(|entry| entry.scope == Scope::General)
    {
        if exempt.contains(&entry.field) {
            continue;
        }
        let levels = [
            ("flag", entry.flag.is_some()),
            ("variable", entry.env.name().is_some()),
            ("key", entry.key.is_some()),
        ];
        for (level, has) in levels {
            if !has {
                missing.push(format!("{}: no {level}", entry.field));
            }
        }
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

#[test]
fn a_flag_and_a_variable_are_named_after_their_setting() {
    // The older names stay: `--no-images` and `KIRI_NO_IMAGES` say the
    // opposite of their setting.
    // `--diagram-panel` is shorter than its setting, so that `-h` keeps
    // its two columns.
    let short = [("diagram_background_panel", "--diagram-panel")];
    for entry in SETTINGS {
        let flag = format!("--{}", entry.field.replace('_', "-"));
        let flag = short
            .iter()
            .find(|(field, _)| *field == entry.field)
            .map_or(flag, |s| s.1.to_owned());
        if let Some(name) = entry.flag {
            assert_eq!(name, flag, "{}", entry.field);
        }
        let variable = format!("KIRI_{}", entry.field.to_uppercase());
        if let Some(name) = entry.env.name().filter(|_| entry.field != "images") {
            assert_eq!(name, variable, "{}", entry.field);
        }
    }
}

/// Parses `text`, a configuration file, and chooses with `env` and `flag`.
fn chosen(text: &str, env: &[(&str, &str)], options: Options) -> Choices {
    let config = parse(text, Path::new("/c"), &Environment::default()).unwrap();
    choose(&options, &vars(env), &config)
}

#[test]
fn the_width_comes_from_the_flag_the_variable_or_the_file() {
    let width = |value, origin| Setting::new(Some(value), origin);
    let file = "width = 70\n";
    let env = [("KIRI_WIDTH", "80")];
    let options = Options {
        width: flag(Some(90), "--width"),
        ..Options::default()
    };
    let origin = Origin::Environment("KIRI_WIDTH");
    assert_eq!(
        chosen(file, &env, options).width,
        width(90, Origin::Flag("--width"))
    );
    assert_eq!(
        chosen(file, &env, Options::default()).width,
        width(80, origin)
    );
    assert_eq!(
        chosen(file, &[], Options::default()).width,
        width(70, Origin::File)
    );
    let none = Setting::new(None, Origin::Default);
    assert_eq!(chosen("", &[], Options::default()).width, none);
    let wrong = chosen("", &[("KIRI_WIDTH", "wide")], Options::default());
    assert_eq!(wrong.width, none);
    assert_eq!(
        wrong.warnings,
        ["KIRI_WIDTH: `wide` is not a whole number of cells; left out"]
    );
}

#[test]
fn the_mode_comes_from_the_flag_the_variable_or_the_file() {
    let file = "mode = \"ansi\"\n";
    let env = [("KIRI_MODE", "plain")];
    let options = Options {
        mode: flag(Mode::Kitty, "--mode"),
        ..Options::default()
    };
    let mode = |choices: Choices| (choices.mode.value, choices.mode.origin);
    assert_eq!(
        mode(chosen(file, &env, options)),
        (Mode::Kitty, Origin::Flag("--mode"))
    );
    let origin = Origin::Environment("KIRI_MODE");
    assert_eq!(
        mode(chosen(file, &env, Options::default())),
        (Mode::Plain, origin)
    );
    assert_eq!(
        mode(chosen(file, &[], Options::default())),
        (Mode::Ansi, Origin::File)
    );
    assert_eq!(
        mode(chosen("", &[], Options::default())),
        (Mode::Auto, Origin::Default)
    );
    let wrong = chosen("", &[("KIRI_MODE", "fancy")], Options::default());
    assert_eq!(
        wrong.warnings,
        ["KIRI_MODE: `fancy` is not auto, plain, ansi or kitty; left out"]
    );
}

#[test]
fn the_largest_image_comes_from_the_variable_or_the_file_within_1_to_100() {
    let size = |text, env: &[(&str, &str)]| chosen(text, env, Options::default()).max_image_size;
    assert_eq!(size("", &[]), Setting::new(Megabytes(10), Origin::Default));
    assert_eq!(
        size("max_image_size = 20\n", &[]),
        Setting::new(Megabytes(20), Origin::File)
    );
    let origin = Origin::Environment("KIRI_MAX_IMAGE_SIZE");
    let env = [("KIRI_MAX_IMAGE_SIZE", "30")];
    assert_eq!(
        size("max_image_size = 20\n", &env),
        Setting::new(Megabytes(30), origin)
    );
    let wrong = chosen("", &[("KIRI_MAX_IMAGE_SIZE", "500")], Options::default());
    assert_eq!(wrong.max_image_size.origin, Origin::Default);
    let warning = "KIRI_MAX_IMAGE_SIZE: `500` is not a whole number of MB from 1 to 100; left out";
    assert_eq!(wrong.warnings, [warning]);
    let error = parse(
        "max_image_size = 500\n",
        Path::new("/c"),
        &Environment::default(),
    );
    assert!(
        error
            .unwrap_err()
            .contains("give a whole number of MB from 1 to 100")
    );
}

#[test]
fn the_margins_come_from_the_flag_the_variable_or_the_file() {
    let margins = |text, env: &[(&str, &str)], options| chosen(text, env, options).margins;
    let flagged = Options {
        margins: flag(false, "--no-margins"),
        ..Options::default()
    };
    let origin = Origin::Environment("KIRI_MARGINS");
    let env = [("KIRI_MARGINS", "off")];
    assert_eq!(
        margins("margins = true\n", &env, flagged),
        Setting::new(false, Origin::Flag("--no-margins"))
    );
    assert_eq!(
        margins("margins = true\n", &env, Options::default()),
        Setting::new(false, origin)
    );
    assert_eq!(
        margins("margins = false\n", &[], Options::default()),
        Setting::new(false, Origin::File)
    );
    assert_eq!(
        margins("", &[], Options::default()),
        Setting::new(true, Origin::Default)
    );
}

#[test]
fn the_diagram_timeout_and_panel_come_from_a_flag_or_a_variable() {
    let env = [
        ("KIRI_DIAGRAM_TIMEOUT", "2.5"),
        ("KIRI_DIAGRAM_BACKGROUND_PANEL", "off"),
    ];
    let file = "[diagrams]\ntimeout = 9\nbackground_panel = true\n";
    let choices = chosen(file, &env, Options::default());
    let timeout = Origin::Environment("KIRI_DIAGRAM_TIMEOUT");
    let panel = Origin::Environment("KIRI_DIAGRAM_BACKGROUND_PANEL");
    assert_eq!(
        choices.diagram_timeout,
        Setting::new(Duration::from_secs_f64(2.5), timeout)
    );
    assert_eq!(choices.diagram_background_panel, Setting::new(false, panel));
    let options = Options {
        diagram_timeout: flag(Duration::from_secs(4), "--diagram-timeout"),
        diagram_background_panel: flag(true, "--diagram-panel"),
        ..Options::default()
    };
    let choices = chosen(file, &env, options);
    assert_eq!(
        choices.diagram_timeout.origin,
        Origin::Flag("--diagram-timeout")
    );
    assert!(choices.diagram_background_panel.value);
    let wrong = chosen("", &[("KIRI_DIAGRAM_TIMEOUT", "-1")], Options::default());
    let warning = "KIRI_DIAGRAM_TIMEOUT: `-1` is not a positive number of seconds; left out";
    assert_eq!(wrong.warnings, [warning]);
}

#[test]
fn the_docs_name_every_variable_flag_and_key_of_the_settings() {
    let config = include_str!("../../docs/config.md");
    let options = include_str!("../../docs/options.md");
    let mut missing = Vec::new();
    for entry in SETTINGS {
        let variable = entry.env.name();
        if variable.is_some_and(|name| !config.contains(&format!("`{name}`"))) {
            missing.push(format!(
                "{}: the variable is not in docs/config.md",
                entry.field
            ));
        }
        let flag = |flag: &str| {
            options.contains(&format!("`{flag}")) || options.contains(&format!(", `{flag}"))
        };
        if entry.flag.is_some_and(|name| !flag(name)) {
            missing.push(format!(
                "{}: the flag is not in docs/options.md",
                entry.field
            ));
        }
        let key = entry.key.filter(|key| !key.contains('<'));
        let key = key.and_then(|key| key.rsplit('.').next());
        let listed = |key: &str| {
            config.contains(&format!("\n{key} = ")) || config.contains(&format!("`{key} = "))
        };
        if key.is_some_and(|key| !listed(key)) {
            missing.push(format!("{}: the key is not in docs/config.md", entry.field));
        }
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

/// A home directory with the themes `names` in its themes directory, for
/// `theme_source` to look in.
fn home_with_themes(label: &str, names: &[&str]) -> (crate::testing::Scratch, Environment) {
    let scratch = crate::testing::Scratch::new(label);
    let themes = scratch.join(".config/kiri/themes");
    std::fs::create_dir_all(&themes).unwrap();
    for name in names {
        std::fs::write(themes.join(format!("{name}.toml")), "").unwrap();
    }
    let env = env(Some(scratch.to_str().unwrap()), None);
    (scratch, env)
}

#[test]
fn a_theme_name_is_a_file_in_the_themes_directory() {
    let (scratch, env) = home_with_themes("theme-name", &["mine"]);
    let path = scratch.join(".config/kiri/themes/mine.toml");
    let named = ThemeSource::Named {
        name: "mine".to_owned(),
        path,
    };
    let flag = |name: &str| {
        let options = Options {
            theme: flag(PathBuf::from(name), "--theme"),
            ..Options::default()
        };
        super::theme_source(&options, &env, &Config::default())
    };
    assert_eq!(
        flag("mine"),
        Setting::new(named.clone(), Origin::Flag("--theme"))
    );
    // A separator or a .toml ending makes it a path, as before.
    let path = ThemeSource::File(PathBuf::from("./mine"));
    assert_eq!(flag("./mine").value, path);
    let path = ThemeSource::File(PathBuf::from("mine.toml"));
    assert_eq!(flag("mine.toml").value, path);
    // The variable and the file take names too.
    let mut with_variable = env.clone();
    with_variable.variables.insert("KIRI_THEME", "mine".into());
    let source = theme_source(None, None, &with_variable, &Config::default());
    assert_eq!(
        source,
        Setting::new(named.clone(), Origin::Environment("KIRI_THEME"))
    );
    let source = theme_source(None, None, &env, &with_theme("mine"));
    assert_eq!(source, Setting::new(named, Origin::File));
}

#[test]
fn light_and_dark_are_the_paper_themes_unless_a_file_has_the_name() {
    let (scratch, env) = home_with_themes("theme-paper-names", &[]);
    let name =
        |name: &str| theme_source(Some(Path::new(name)), None, &env, &Config::default()).value;
    assert_eq!(name("light"), ThemeSource::Paper(Paper::Light));
    assert_eq!(name("dark"), ThemeSource::Paper(Paper::Dark));
    let path = scratch.join(".config/kiri/themes/dark.toml");
    std::fs::write(&path, "").unwrap();
    assert_eq!(
        name("dark"),
        ThemeSource::Named {
            name: "dark".to_owned(),
            path
        }
    );
    assert_eq!(name("light"), ThemeSource::Paper(Paper::Light));
}

#[test]
fn an_unknown_theme_name_is_an_error_listing_the_names() {
    let (_scratch, env) = home_with_themes("theme-unknown", &["mine", "yours"]);
    let source = theme_source(Some(Path::new("minee")), None, &env, &Config::default()).value;
    let ThemeSource::Unknown { name, message } = &source else {
        panic!("{source:?}");
    };
    assert_eq!(name, "minee");
    assert_eq!(
        message,
        "theme `minee` is not one of the themes: dark, light, mine, yours \
         (a file in this directory is ./minee)"
    );
    assert_eq!(load_theme(&source), Err(message.clone()));
}

#[test]
fn a_theme_name_in_the_file_is_not_made_relative_to_the_file() {
    let parsed = |text| parse(text, Path::new("/c"), &Environment::default()).unwrap();
    assert_eq!(
        parsed("theme = \"dark\"\n").theme,
        Some(PathBuf::from("dark"))
    );
    assert_eq!(
        parsed("theme = \"t.toml\"\n").theme,
        Some(PathBuf::from("/c/t.toml"))
    );
}

#[test]
fn a_width_of_0_is_the_terminals_width_at_every_level() {
    // The flag is read by `main`, which asks `is_unset`; the tests in
    // tests/cli_config.rs run it.
    let width = |text: &str, env: &[(&str, &str)]| chosen(text, env, Options::default()).width;
    // The origin stays, so that `--doctor` says where the 0 came from.
    let none = |origin| Setting::new(None, origin);
    let origin = Origin::Environment("KIRI_WIDTH");
    assert_eq!(width("", &[("KIRI_WIDTH", "0")]), none(origin));
    assert_eq!(width("width = 0\n", &[]), none(Origin::File));
    // 0 in the file is a value, so a variable still beats it.
    assert_eq!(
        width("width = 0\n", &[("KIRI_WIDTH", "60")]).value,
        Some(60)
    );
}
