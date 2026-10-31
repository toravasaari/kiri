//! Resolving: the setting that the flags, the environment, the file and the
//! default choose between them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use super::environment::{Environment, directory};
use super::file::Config;
use super::types::{FrontMatter, Named, Origin, Passthrough, Setting, ThemeSource};
use crate::render::{Headings, RenderErrors, TableRows};
use crate::session::settings::{Follow, FollowJump, InitialLoad, Paging, SearchCase};
use crate::typeset::theme::{Paper, Theme};

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

/// Whether images are shown and where that comes from, strongest first:
/// the command line (`flag`, from `--images` or `--no-images`), then
/// `KIRI_NO_IMAGES`, then the configuration file; on unless one turns them
/// off. `KIRI_NO_IMAGES` turns them off when it is set to anything but an
/// empty value or `0`, and `0` turns them on whatever the configuration
/// file says.
pub(super) fn images(
    flag: Option<Setting<bool>>,
    env: &Environment,
    config: &Config,
) -> Setting<bool> {
    let name = "KIRI_NO_IMAGES";
    let variable = |_: &mut Vec<String>| {
        let value = env.get(name).filter(|value| !value.is_empty())?;
        Some(Setting::new(value == "0", Origin::Environment(name)))
    };
    resolve(flag, variable, config.images, true, &mut Vec::new())
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

/// The environment variable `name`, for `resolve`, which takes the names
/// and the aliases of `T`'s values; empty is unset. Any other value is
/// left out with a warning.
pub(super) fn variable<T: Named>(
    env: &Environment,
    name: &'static str,
) -> impl FnOnce(&mut Vec<String>) -> Option<Setting<T>> {
    move |warnings| {
        let value = env.get(name).filter(|value| !value.is_empty())?;
        let alias = |text: &str| {
            let mut aliases = T::ALIASES.iter();
            aliases
                .find(|(alias, _)| *alias == text)
                .map(|&(_, value)| value)
        };
        let chosen = value
            .to_str()
            .and_then(|text| T::named(text).or_else(|| alias(text)));
        if chosen.is_none() {
            let aliases = T::ALIASES.iter().map(|&(alias, _)| alias);
            let names: Vec<&str> = T::NAMES.iter().copied().chain(aliases).collect();
            let (last, rest) = names.split_last().expect("a setting has values");
            warnings.push(format!(
                "{name}: `{}` is not {} or {last}; left out",
                value.to_string_lossy(),
                rest.join(", "),
            ));
        }
        chosen.map(|value| Setting::new(value, Origin::Environment(name)))
    }
}

/// What the command line asks for, each with the flag it comes from;
/// `None` where it leaves the choice to the environment and the
/// configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// A theme file.
    pub theme: Option<Setting<PathBuf>>,
    /// A paper theme.
    pub paper: Option<Setting<Paper>>,
    /// Whether the theme keeps its margins.
    pub margins: Option<Setting<bool>>,
    /// Whether images are shown.
    pub images: Option<Setting<bool>>,
    /// When the document is paged.
    pub paging: Option<Setting<Paging>>,
    /// Whether the pager follows a file.
    pub follow: Option<Setting<Follow>>,
    /// Which change auto-follow moves the view to.
    pub follow_jump: Option<Setting<FollowJump>>,
    /// Whether the pager waits for the images before it opens.
    pub initial_load: Option<Setting<InitialLoad>>,
    /// Whether kitty's graphics are wrapped for tmux.
    pub passthrough: Option<Setting<Passthrough>>,
    /// How front matter shows.
    pub front_matter: Option<Setting<FrontMatter>>,
    /// How top-level headings show.
    pub headings: Option<Setting<Headings>>,
    /// How the rows of a table are told apart.
    pub table_rows: Option<Setting<TableRows>>,
    /// Whether diagrams in code blocks are drawn.
    pub diagrams: Option<Setting<bool>>,
    /// Where kiri tells why a diagram could not be drawn.
    pub render_errors: Option<Setting<RenderErrors>>,
    /// Whether the pager takes the mouse.
    pub mouse: Option<Setting<bool>>,
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
    /// Whether the pager follows a file, reading it again whenever it
    /// changes.
    pub follow: Follow,
    /// Which change auto-follow moves the view to.
    pub follow_jump: FollowJump,
    /// Whether the pager waits for the images before it opens.
    pub initial_load: InitialLoad,
    /// Whether kitty's graphics are wrapped for tmux.
    pub passthrough: Passthrough,
    /// How front matter shows.
    pub front_matter: FrontMatter,
    /// How top-level headings show.
    pub headings: Headings,
    /// How the rows of a table are told apart.
    pub table_rows: TableRows,
    /// Whether diagrams in code blocks are drawn, or shown as code.
    pub diagrams: bool,
    /// The external command for each diagram language that has one.
    pub diagram_commands: BTreeMap<String, Vec<String>>,
    /// How long an external command may take to draw one diagram.
    pub diagram_timeout: Duration,
    /// Whether a diagram in colours for the other page lies on a panel.
    pub diagram_panel: bool,
    /// The diagram languages whose program kiri is not to look for.
    pub diagram_no_search: BTreeSet<String>,
    /// Where kiri tells why a diagram could not be drawn.
    pub render_errors: RenderErrors,
    /// Whether the pager takes the mouse, to scroll on the wheel.
    pub mouse: bool,
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
    /// Whether the pager follows a file.
    pub follow: Setting<Follow>,
    /// Which change auto-follow moves the view to.
    pub follow_jump: Setting<FollowJump>,
    /// Whether the pager waits for the images before it opens.
    pub initial_load: Setting<InitialLoad>,
    /// Whether kitty's graphics are wrapped for tmux.
    pub passthrough: Setting<Passthrough>,
    /// How front matter shows.
    pub front_matter: Setting<FrontMatter>,
    /// How top-level headings show.
    pub headings: Setting<Headings>,
    /// How the rows of a table are told apart.
    pub table_rows: Setting<TableRows>,
    /// Whether diagrams in code blocks are drawn, or shown as code.
    pub diagrams: Setting<bool>,
    /// The external command for each diagram language that has one, all
    /// from the configuration file.
    pub diagram_commands: BTreeMap<String, Vec<String>>,
    /// How long an external command may take to draw one diagram.
    pub diagram_timeout: Setting<Duration>,
    /// Whether a diagram in colours for the other page lies on a panel.
    pub diagram_panel: Setting<bool>,
    /// The diagram languages whose program kiri is not to look for, all
    /// from the configuration file.
    pub diagram_no_search: BTreeSet<String>,
    /// Where kiri tells why a diagram could not be drawn.
    pub render_errors: Setting<RenderErrors>,
    /// Whether the pager takes the mouse, to scroll on the wheel.
    pub mouse: Setting<bool>,
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
            follow: self.follow.value,
            follow_jump: self.follow_jump.value,
            initial_load: self.initial_load.value,
            passthrough: self.passthrough.value,
            front_matter: self.front_matter.value,
            headings: self.headings.value,
            table_rows: self.table_rows.value,
            diagrams: self.diagrams.value,
            diagram_commands: self.diagram_commands.clone(),
            diagram_timeout: self.diagram_timeout.value,
            diagram_panel: self.diagram_panel.value,
            diagram_no_search: self.diagram_no_search.clone(),
            render_errors: self.render_errors.value,
            mouse: self.mouse.value,
        }
    }
}

/// What `options`, `env` and `config` choose.
pub fn choose(options: &Options, env: &Environment, config: &Config) -> Choices {
    let mut warnings = Vec::new();
    let w = &mut warnings;
    let paging = resolve(
        options.paging.clone(),
        variable(env, "KIRI_PAGING"),
        config.paging,
        Paging::default(),
        w,
    );
    let front_matter = resolve(
        options.front_matter.clone(),
        variable(env, "KIRI_FRONT_MATTER"),
        config.front_matter,
        FrontMatter::default(),
        w,
    );
    let headings = resolve(
        options.headings.clone(),
        variable(env, "KIRI_HEADINGS"),
        config.headings,
        Headings::default(),
        w,
    );
    let table_rows = resolve(
        options.table_rows.clone(),
        variable(env, "KIRI_TABLE_ROWS"),
        config.table_rows,
        TableRows::default(),
        w,
    );
    let diagrams = resolve(
        options.diagrams.clone(),
        variable(env, "KIRI_DIAGRAMS"),
        config.diagrams,
        true,
        w,
    );
    let render_errors = resolve(
        options.render_errors.clone(),
        variable(env, "KIRI_RENDER_ERRORS"),
        config.render_errors,
        RenderErrors::default(),
        w,
    );
    let time_limit = crate::diagrams::external::TIME_LIMIT;
    Choices {
        theme: theme_source(options, env, config),
        paper: options.paper.as_ref().map(|paper| paper.value),
        margins: resolve(options.margins.clone(), unset, None, true, w),
        images: images(options.images.clone(), env, config),
        paging,
        search_case: resolve(None, unset, config.search_case, SearchCase::default(), w),
        follow: resolve(
            options.follow.clone(),
            variable(env, "KIRI_FOLLOW"),
            config.follow,
            Follow::default(),
            w,
        ),
        follow_jump: resolve(
            options.follow_jump.clone(),
            variable(env, "KIRI_FOLLOW_JUMP"),
            config.follow_jump,
            FollowJump::default(),
            w,
        ),
        initial_load: resolve(
            options.initial_load.clone(),
            variable(env, "KIRI_INITIAL_LOAD"),
            config.initial_load,
            InitialLoad::default(),
            w,
        ),
        passthrough: resolve(
            options.passthrough.clone(),
            variable(env, "KIRI_PASSTHROUGH"),
            config.passthrough,
            Passthrough::default(),
            w,
        ),
        front_matter,
        headings,
        table_rows,
        diagrams,
        diagram_commands: config.diagram_commands.clone(),
        diagram_timeout: resolve(None, unset, config.diagram_timeout, time_limit, w),
        diagram_panel: resolve(None, unset, config.diagram_panel, true, w),
        diagram_no_search: config.diagram_no_search.clone(),
        render_errors,
        mouse: resolve(
            options.mouse.clone(),
            variable(env, "KIRI_MOUSE"),
            config.mouse,
            true,
            w,
        ),
        warnings,
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
