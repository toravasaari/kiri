//! The registry of settings: each setting is declared once, below, with its
//! key in the configuration file, the module that reads it, what a change of
//! it needs and its medium. The types that hold the settings, and the rule
//! that resolves them, are generated from it, so that a new setting is a
//! new entry and the code of its owner, and nothing else in this module.
//!
//! The generated types are `Config` (what the configuration file sets),
//! `Options` (what the command line asks for), `Choices` (the choice and
//! where it comes from) and `Settings` (the values, with the theme loaded),
//! and `choose`. The environment's table of variables reads the registry
//! for the variables of the settings.
//!
//! A setting that the command line sets also needs its flag in the clap
//! definition in `src/main.rs`, where the help is written, and a line in
//! the function that fills `Options` from it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde::de::MapAccess;

use super::environment::Environment;
use super::resolve::{resolve, theme_source, variable};
use super::types::{Env, File, FrontMatter, Megabytes, Setting, ThemeSource, file_value};
use crate::render::{Headings, RenderErrors, TableRows};
use crate::session::settings::{Follow, FollowJump, InitialLoad, Paging, SearchCase};
use crate::term::Mode;
use crate::typeset::theme::{Paper, Theme};

/// What a change of a setting needs, while kiri runs. The pager does not
/// change settings yet, so this is what the menu will need to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// The next frame: nothing is laid out or loaded again.
    Paint,
    /// The document is laid out again.
    Layout,
    /// The images and diagrams are loaded or drawn again.
    Images,
    /// The terminal is set up again: its mode, its mouse, its passthrough.
    Display,
    /// The next start: the setting is read once.
    Restart,
}

/// What a setting changes on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Medium {
    /// Characters in cells.
    Cells,
    /// Pixels: images, and text that is drawn as one.
    Pixels,
    /// Both.
    Both,
}

/// Where a setting is useful, which decides the levels it belongs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// It matters without the pager too (`-P`): it has a flag, an
    /// environment variable and a key in the file.
    General,
    /// It is about how the pager works: the file is enough.
    Pager,
}

/// One setting, as the registry declares it.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    /// The name of the setting in the code.
    pub field: &'static str,
    /// The key in the configuration file, with the table it is in, such as
    /// `diagrams.timeout`; `None` when the file cannot set it.
    pub key: Option<&'static str>,
    /// The module that reads the setting.
    pub owner: &'static str,
    pub effect: Effect,
    pub medium: Medium,
    pub scope: Scope,
    /// The command-line flag, in its long form.
    pub flag: Option<&'static str>,
    /// The environment variable.
    pub env: Env,
}

/// Declares the settings, and generates what holds and resolves them.
macro_rules! settings {
    (
        // The keys of the file's top level, each resolved the same way.
        top {$(
            $(#[$tdoc:meta])*
            $tfield:ident: $ttype:ty = $tdefault:expr;
            key $tkey:literal, owner $towner:literal, effect $teffect:ident,
            medium $tmedium:ident, scope $tscope:ident, flag $tflag:expr, env $tenv:expr;
        )*}
        // The keys of the `[diagrams]` table.
        diagrams {$(
            $(#[$ddoc:meta])*
            $dfield:ident in $dkey:ident: $dtype:ty = $ddefault:expr;
            key $dfull:literal, owner $downer:literal, effect $deffect:ident,
            medium $dmedium:ident, scope $dscope:ident, flag $dflag:expr, env $denv:expr;
        )*}
        // The settings that are resolved by hand.
        special {$(
            $sfield:ident, key $skey:expr, owner $sowner:literal, effect $seffect:ident,
            medium $smedium:ident, scope $sscope:ident, flag $sflag:expr, env $senv:expr;
        )*}
    ) => {
        /// Every setting, in the order of the help.
        pub const SETTINGS: &[Entry] = &[
            $(Entry {
                field: stringify!($tfield),
                key: Some($tkey),
                owner: $towner,
                effect: Effect::$teffect,
                medium: Medium::$tmedium,
                scope: Scope::$tscope,
                flag: $tflag,
                env: $tenv,
            },)*
            $(Entry {
                field: stringify!($dfield),
                key: Some($dfull),
                owner: $downer,
                effect: Effect::$deffect,
                medium: Medium::$dmedium,
                scope: Scope::$dscope,
                flag: $dflag,
                env: $denv,
            },)*
            $(Entry {
                field: stringify!($sfield),
                key: $skey,
                owner: $sowner,
                effect: Effect::$seffect,
                medium: Medium::$smedium,
                scope: Scope::$sscope,
                flag: $sflag,
                env: $senv,
            },)*
        ];

        /// What the configuration file sets; `None` for what it leaves out.
        #[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct Config {
            /// A theme file, relative to the configuration file's directory
            /// unless it starts with `~/`, the home directory.
            pub theme: Option<PathBuf>,
            $(
                $(#[$tdoc])*
                #[serde(default, deserialize_with = "file_value")]
                pub $tfield: Option<$ttype>,
            )*
            /// The `diagrams` key: a `[diagrams]` table of the settings for
            /// every diagram, beside a table for each language, such as
            /// `[diagrams.mermaid]`; or `true` or `false`, short for
            /// `enabled` in it.
            #[serde(default)]
            pub diagrams: DiagramsFile,
        }

        /// The `[diagrams]` table of the configuration file.
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        pub struct DiagramsFile {
            $(
                $(#[$ddoc])*
                pub $dkey: Option<$dtype>,
            )*
            /// The external command for each diagram language that has one,
            /// from `[diagrams.<language>]`: the program and its arguments,
            /// with `{input}`, `{output}`, `{theme}`, `{config}` and
            /// `{scale}` in them.
            pub commands: BTreeMap<String, Vec<String>>,
            /// The diagram languages whose program kiri is not to look for.
            pub no_search: BTreeSet<String>,
        }

        impl DiagramsFile {
            /// Reads the value of `key` from `map` when it is one of the
            /// table's own settings, not a language; tells whether it was.
            pub(super) fn read_key<'de, A: MapAccess<'de>>(
                &mut self,
                key: &str,
                map: &mut A,
            ) -> Result<bool, A::Error> {
                $(if key == stringify!($dkey) {
                    self.$dkey = Some(map.next_value::<File<$dtype>>()?.0);
                    return Ok(true);
                })*
                let _ = (key, &map);
                Ok(false)
            }
        }

        /// The keys of the `[diagrams]` table other than the languages.
        pub(super) const DIAGRAMS_KEYS: &[&str] = &[$(stringify!($dkey)),*];

        /// What the command line asks for, each with the flag it comes
        /// from; `None` where it leaves the choice to the environment and
        /// the configuration file.
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        pub struct Options {
            /// A theme file.
            pub theme: Option<Setting<PathBuf>>,
            /// A paper theme.
            pub paper: Option<Setting<Paper>>,
            $(
                $(#[$tdoc])*
                pub $tfield: Option<Setting<$ttype>>,
            )*
            $(
                $(#[$ddoc])*
                pub $dfield: Option<Setting<$dtype>>,
            )*
        }

        /// What the options, the environment and the configuration file
        /// choose, before the theme is loaded, with where each choice comes
        /// from.
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct Choices {
            pub theme: Setting<ThemeSource>,
            /// The paper from `--light` or `--dark`.
            pub paper: Option<Paper>,
            $(
                $(#[$tdoc])*
                pub $tfield: Setting<$ttype>,
            )*
            $(
                $(#[$ddoc])*
                pub $dfield: Setting<$dtype>,
            )*
            /// The external command for each diagram language that has one,
            /// all from the configuration file.
            pub diagram_commands: BTreeMap<String, Vec<String>>,
            /// The diagram languages whose program kiri is not to look for,
            /// all from the configuration file.
            pub diagram_no_search: BTreeSet<String>,
            /// What the user should know about settings that were left out.
            pub warnings: Vec<String>,
        }

        /// What the options, the environment and the configuration file
        /// choose together.
        #[derive(Debug)]
        pub struct Settings {
            /// The theme, loaded, without margins if the options say so.
            pub theme: Theme,
            /// The paper from `--light` or `--dark`, which also decides
            /// whether the background counts as dark.
            pub paper: Option<Paper>,
            $(
                $(#[$tdoc])*
                pub $tfield: $ttype,
            )*
            $(
                $(#[$ddoc])*
                pub $dfield: $dtype,
            )*
            /// The external command for each diagram language that has one.
            pub diagram_commands: BTreeMap<String, Vec<String>>,
            /// The diagram languages whose program kiri is not to look for.
            pub diagram_no_search: BTreeSet<String>,
        }

        impl Choices {
            /// The settings these choices make with `theme`, the theme they
            /// chose, loaded.
            pub fn with_theme(&self, theme: Theme) -> Settings {
                Settings {
                    theme: if self.margins.value {
                        theme
                    } else {
                        theme.without_margins()
                    },
                    paper: self.paper,
                    $($tfield: self.$tfield.value.clone(),)*
                    $($dfield: self.$dfield.value.clone(),)*
                    diagram_commands: self.diagram_commands.clone(),
                    diagram_no_search: self.diagram_no_search.clone(),
                }
            }
        }

        /// What `options`, `env` and `config` choose.
        pub fn choose(options: &Options, env: &Environment, config: &Config) -> Choices {
            let mut warnings = Vec::new();
            let w = &mut warnings;
            Choices {
                theme: theme_source(options, env, config),
                paper: options.paper.as_ref().map(|paper| paper.value),
                $($tfield: resolve(
                    options.$tfield.clone(),
                    variable(env, $tenv),
                    config.$tfield.clone(),
                    $tdefault,
                    w,
                ),)*
                $($dfield: resolve(
                    options.$dfield.clone(),
                    variable(env, $denv),
                    config.diagrams.$dkey.clone(),
                    $ddefault,
                    w,
                ),)*
                diagram_commands: config.diagrams.commands.clone(),
                diagram_no_search: config.diagrams.no_search.clone(),
                warnings,
            }
        }
    };
}

settings! {
    top {
        /// Which terminal features to use.
        mode: Mode = Mode::Auto;
        key "mode", owner "term", effect Display, medium Both, scope General,
        flag Some("--mode"), env Env::is("KIRI_MODE");

        /// The width in cells, when not the terminal's.
        width: Option<u16> = None;
        key "width", owner "term", effect Layout, medium Cells, scope General,
        flag Some("--width"), env Env::is("KIRI_WIDTH");

        /// The largest image file to read, in megabytes.
        max_image_size: Megabytes = Megabytes(10);
        key "max_image_size", owner "images", effect Images, medium Pixels, scope General,
        flag Some("--max-image-size"), env Env::is("KIRI_MAX_IMAGE_SIZE");

        /// Whether the theme keeps its margins.
        margins: bool = true;
        key "margins", owner "typeset::theme", effect Layout, medium Cells, scope General,
        flag Some("--margins"), env Env::is("KIRI_MARGINS");

        /// Whether images are shown, or only their alt text.
        images: bool = true;
        key "images", owner "images", effect Images, medium Pixels, scope General,
        flag Some("--images"), env Env::not("KIRI_NO_IMAGES");

        /// When the document is shown in the pager.
        paging: Paging = Paging::default();
        key "paging", owner "session::plan", effect Restart, medium Cells, scope General,
        flag Some("--paging"), env Env::is("KIRI_PAGING");

        /// How a search in the pager treats case.
        search_case: SearchCase = SearchCase::default();
        key "search_case", owner "session::search", effect Paint, medium Cells, scope Pager,
        flag None, env Env::NONE;

        /// Whether the pager follows a file, reading it again whenever it
        /// changes.
        follow: Follow = Follow::default();
        key "follow", owner "pager", effect Paint, medium Cells, scope Pager,
        flag Some("--follow"), env Env::is("KIRI_FOLLOW");

        /// Which change auto-follow moves the view to.
        follow_jump: FollowJump = FollowJump::default();
        key "follow_jump", owner "session::changes", effect Paint, medium Cells, scope Pager,
        flag Some("--follow-jump"), env Env::is("KIRI_FOLLOW_JUMP");

        /// Whether the pager waits for the images before it opens.
        initial_load: InitialLoad = InitialLoad::default();
        key "initial_load", owner "session::plan", effect Restart, medium Pixels, scope Pager,
        flag Some("--initial-load"), env Env::is("KIRI_INITIAL_LOAD");

        /// Whether kitty's graphics are wrapped for tmux.
        passthrough: super::types::Passthrough = super::types::Passthrough::default();
        key "passthrough", owner "term", effect Display, medium Pixels, scope General,
        flag Some("--passthrough"), env Env::is("KIRI_PASSTHROUGH");

        /// How front matter shows.
        front_matter: FrontMatter = FrontMatter::default();
        key "front_matter", owner "render", effect Layout, medium Cells, scope General,
        flag Some("--front-matter"), env Env::is("KIRI_FRONT_MATTER");

        /// How top-level headings show.
        headings: Headings = Headings::default();
        key "headings", owner "render", effect Layout, medium Both, scope General,
        flag Some("--headings"), env Env::is("KIRI_HEADINGS");

        /// How the rows of a table are told apart.
        table_rows: TableRows = TableRows::default();
        key "table_rows", owner "render", effect Layout, medium Cells, scope General,
        flag Some("--table-rows"), env Env::is("KIRI_TABLE_ROWS");

        /// Where kiri tells why a diagram could not be drawn.
        render_errors: RenderErrors = RenderErrors::default();
        key "render_errors", owner "render", effect Images, medium Both, scope General,
        flag Some("--render-errors"), env Env::is("KIRI_RENDER_ERRORS");

        /// Whether the pager takes the mouse, to scroll on the wheel.
        mouse: bool = true;
        key "mouse", owner "term", effect Display, medium Cells, scope Pager,
        flag Some("--mouse"), env Env::is("KIRI_MOUSE");
    }
    diagrams {
        /// Whether diagrams in code blocks are drawn, or shown as code:
        /// `enabled` in the `[diagrams]` table, or `diagrams` on its own.
        diagrams in enabled: bool = true;
        key "diagrams", owner "images::pipeline", effect Images, medium Pixels, scope General,
        flag Some("--diagrams"), env Env::is("KIRI_DIAGRAMS");

        /// How long an external command may take to draw one diagram.
        diagram_timeout in timeout: Duration = crate::diagrams::external::TIME_LIMIT;
        key "diagrams.timeout", owner "diagrams::external", effect Images, medium Pixels,
        scope General, flag Some("--diagram-timeout"), env Env::is("KIRI_DIAGRAM_TIMEOUT");

        /// Whether a diagram in colours for the other page, light or dark,
        /// lies on a panel.
        diagram_background_panel in background_panel: bool = true;
        key "diagrams.background_panel", owner "images::pipeline", effect Images,
        medium Pixels, scope General, flag Some("--diagram-panel"), env Env::is("KIRI_DIAGRAM_BACKGROUND_PANEL");
    }
    special {
        theme, key Some("theme"), owner "typeset::theme", effect Layout, medium Both,
        scope General, flag Some("--theme"), env Env::is("KIRI_THEME");
        paper, key None, owner "typeset::theme", effect Layout, medium Both,
        scope General, flag Some("--light"), env Env::NONE;
        diagram_commands, key Some("diagrams.<language>.command"), owner "diagrams::external",
        effect Images, medium Pixels, scope General, flag None, env Env::NONE;
        diagram_no_search, key Some("diagrams.<language>.auto_detect"), owner "diagrams::find",
        effect Images, medium Pixels, scope General, flag None, env Env::NONE;
    }
}
