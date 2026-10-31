//! The process's environment: the variables kiri reads, from one table, and
//! the directories that `HOME` and the XDG variables give.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::types::{Origin, Setting};

/// `Environment`, with a field for each variable, from one table, so that
/// a variable cannot be read without `get` knowing it.
macro_rules! environment {
    ($($(#[$doc:meta])* $field:ident: $name:literal,)*) => {
        /// The environment variables kiri reads for its settings.
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        pub struct Environment {
            $($(#[$doc])* pub $field: Option<OsString>,)*
        }

        impl Environment {
            /// The environment with each variable from `var`.
            pub(super) fn read(var: impl Fn(&'static str) -> Option<OsString>) -> Self {
                Self {
                    $($field: var($name),)*
                }
            }

            /// The value of the variable `name`, of those kiri reads.
            pub fn get(&self, name: &str) -> Option<&OsStr> {
                match name {
                    $($name => self.$field.as_deref(),)*
                    _ => None,
                }
            }
        }
    };
}

environment! {
    /// `HOME`.
    home: "HOME",
    /// `XDG_CONFIG_HOME`: where kiri's configuration directory goes.
    xdg_config_home: "XDG_CONFIG_HOME",
    /// `XDG_CACHE_HOME`: where kiri's cache directory goes.
    xdg_cache_home: "XDG_CACHE_HOME",
    /// `TMPDIR`: the system's temporary directory, where the files of the
    /// programs that draw diagrams go when the cache directory cannot be
    /// used.
    tmpdir: "TMPDIR",
    /// `KIRI_CONFIG`: the configuration file to read instead of the default.
    kiri_config: "KIRI_CONFIG",
    /// `KIRI_THEME`: a theme file.
    kiri_theme: "KIRI_THEME",
    /// `KIRI_NO_IMAGES`: images off unless empty or `0`.
    kiri_no_images: "KIRI_NO_IMAGES",
    /// `KIRI_PAGING`: `always`, `auto` or `never`, or `1` or `0`.
    kiri_paging: "KIRI_PAGING",
    /// `KIRI_FOLLOW`: `on`, `off` or `auto`, or `1` or `0`.
    kiri_follow: "KIRI_FOLLOW",
    /// `KIRI_FOLLOW_JUMP`: `first`, `last` or `most`.
    kiri_follow_jump: "KIRI_FOLLOW_JUMP",
    /// `KIRI_INITIAL_LOAD`: `wait` or `background`.
    kiri_initial_load: "KIRI_INITIAL_LOAD",
    /// `KIRI_PASSTHROUGH`: `detect`, `none` or `tmux`.
    kiri_passthrough: "KIRI_PASSTHROUGH",
    /// `KIRI_FRONT_MATTER`: `table`, `code`, `hide` or `off`.
    kiri_front_matter: "KIRI_FRONT_MATTER",
    /// `KIRI_HEADINGS`: `auto`, `sized`, `images` or `plain`.
    kiri_headings: "KIRI_HEADINGS",
    /// `KIRI_TABLE_ROWS`: `off`, `zebra`, `lines`, `indent` or `underline`.
    kiri_table_rows: "KIRI_TABLE_ROWS",
    /// `KIRI_DIAGRAMS`: `on` or `off`, or `1` or `0`.
    kiri_diagrams: "KIRI_DIAGRAMS",
    /// `KIRI_RENDER_ERRORS`: `inline`, `stderr` or `both`.
    kiri_render_errors: "KIRI_RENDER_ERRORS",
    /// `KIRI_MOUSE`: `on` or `off`, or `1` or `0`.
    kiri_mouse: "KIRI_MOUSE",
    /// `PATH`, where kiri looks for a program that draws diagrams.
    path: "PATH",
    /// `NVM_DIR`: where nvm keeps the Node versions it installs.
    nvm_dir: "NVM_DIR",
    /// `NPM_CONFIG_PREFIX`, or `npm_config_prefix`: npm's prefix for the
    /// programs it installs.
    npm_config_prefix: "NPM_CONFIG_PREFIX",
    /// `NO_COLOR`: colours off unless empty.
    no_color: "NO_COLOR",
    /// `COLORTERM`: 24-bit colour when `truecolor` or `24bit`.
    colorterm: "COLORTERM",
    /// `TERM`: the terminal type, which only `--doctor` reports.
    term: "TERM",
    /// `TERM_PROGRAM`: names some of the terminals known to support
    /// hyperlinks.
    term_program: "TERM_PROGRAM",
    /// `VTE_VERSION`: terminals built on VTE, with hyperlinks from 5000 on.
    vte_version: "VTE_VERSION",
    /// `TMUX`: set and not empty inside tmux.
    tmux: "TMUX",
    /// `TMUX_PANE`: the tmux pane kiri runs in, such as `%3`.
    tmux_pane: "TMUX_PANE",
}

impl Environment {
    /// The environment of this process.
    pub fn from_process() -> Self {
        let mut env = Self::read(std::env::var_os);
        // npm reads its settings from the environment in either case.
        if env.npm_config_prefix.is_none() {
            env.npm_config_prefix = std::env::var_os("npm_config_prefix");
        }
        env
    }
}

/// The home directory, from `HOME`; `None` when that is unset, empty or
/// not an absolute path.
pub fn home_directory(env: &Environment) -> Option<PathBuf> {
    (env.home.as_ref().map(PathBuf::from)).filter(|home| home.is_absolute())
}

/// A directory and where it comes from: the variable `name` when it is
/// set to a path that `valid` takes, else `fallback`, kiri's default.
fn directory_in(
    env: &Environment,
    name: &'static str,
    valid: fn(&Path) -> bool,
    fallback: Option<PathBuf>,
) -> Option<Setting<PathBuf>> {
    match env.get(name).map(Path::new).filter(|dir| valid(dir)) {
        Some(dir) => Some(Setting::new(dir.to_owned(), Origin::Environment(name))),
        None => fallback.map(|dir| Setting::new(dir, Origin::Default)),
    }
}

/// kiri's configuration directory and where it comes from:
/// `$XDG_CONFIG_HOME/kiri`, or `~/.config/kiri` when that is unset, empty
/// or relative, which the XDG base directory specification calls invalid.
/// Without either, there is none.
pub fn directory(env: &Environment) -> Option<Setting<PathBuf>> {
    let home = home_directory(env).map(|home| home.join(".config"));
    let config = directory_in(env, "XDG_CONFIG_HOME", Path::is_absolute, home)?;
    Some(Setting::new(config.value.join("kiri"), config.origin))
}

/// kiri's cache directory, for files it may remove at any time, such as
/// those of the programs that draw diagrams.
pub fn cache_directory(env: &Environment) -> Option<PathBuf> {
    cache_directory_setting(env).map(|dir| dir.value)
}

/// kiri's cache directory and where it comes from: `$XDG_CACHE_HOME/kiri`,
/// or `~/.cache/kiri` when that is unset, empty or relative, which the XDG
/// base directory specification calls invalid.
pub fn cache_directory_setting(env: &Environment) -> Option<Setting<PathBuf>> {
    let home = home_directory(env).map(|home| home.join(".cache"));
    let cache = directory_in(env, "XDG_CACHE_HOME", Path::is_absolute, home)?;
    Some(Setting::new(cache.value.join("kiri"), cache.origin))
}

/// The system's temporary directory.
pub fn temp_directory(env: &Environment) -> PathBuf {
    temp_directory_setting(env).value
}

/// The system's temporary directory, as the standard library finds it,
/// and where it comes from: `TMPDIR`, or `/tmp` when that is unset or
/// empty.
pub fn temp_directory_setting(env: &Environment) -> Setting<PathBuf> {
    let not_empty = |dir: &Path| !dir.as_os_str().is_empty();
    let tmp = Some(PathBuf::from("/tmp"));
    directory_in(env, "TMPDIR", not_empty, tmp).expect("/tmp is a fallback")
}
