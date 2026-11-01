//! The command line: the clap definition of kiri's options, and what each
//! of them asks for, as `Options`, with the flag it comes from. Settings
//! are declared in `src/config/registry.rs`; the help of a setting's flag
//! is declared there too, with the setting.

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use clap::builder::{PossibleValue, PossibleValuesParser, TypedValueParser};
use kiri::config::{
    FrontMatter, Megabytes, Named, Options, Origin, Passthrough, Setting, Value, help_text,
    long_help_text, text,
};
use kiri::render::{Headings, RenderErrors, TableRows};
use kiri::session::settings::{Follow, FollowJump, InitialLoad, Paging};
use kiri::term::Mode;
use kiri::typeset::input::Input;
use kiri::typeset::reading::page::Wide;

/// Render a Markdown document, or show an image, in the terminal.
#[derive(Parser)]
#[command(
    version,
    // Longer lines are hard to read, also in a wide terminal.
    max_term_width = 100,
    after_help = "Images and diagrams are drawn only in terminals that support \
                  kitty's graphics protocol with Unicode placeholders, such as \
                  kitty 0.28 or later and Ghostty 1.0 or later; elsewhere images \
                  show their alt text and diagrams their code."
)]
pub struct Cli {
    /// Markdown, CSV or TSV, an image, a Mermaid diagram, or a directory
    ///
    /// A Markdown file is rendered, a CSV or TSV file shown as a table, an
    /// image or a Mermaid diagram (.mmd) shown on its own, and a directory
    /// shows its README.
    /// Several files are shown one after another, each under a header
    /// that names it; one that cannot be shown is reported, and the others
    /// are still shown.
    /// Without one, or with -, kiri reads standard input unless it is a
    /// terminal; - can be given only once.
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// Show what kiri finds out about the terminal, for a bug report
    ///
    /// The report also tells which settings kiri uses. It is Markdown when
    /// written to a file.
    #[arg(long, conflicts_with_all = ["files", "format"])]
    pub doctor: bool,

    /// List the emoji shortcodes, such as :smile:
    ///
    /// A terminal shows them as a table; a pipe or a file gets one line per
    /// emoji.
    #[arg(long, conflicts_with_all = ["files", "format", "doctor"])]
    pub list_emojis: bool,

    /// With --doctor: the report as Markdown, also in a terminal
    ///
    /// The report then leaves out its sample, so it can be copied into a
    /// bug report.
    #[arg(long)]
    pub raw: bool,

    /// What the file is, such as markdown, csv or png
    ///
    /// The formats are markdown, csv, tsv, mermaid (a diagram shown on its
    /// own), text (the file as it is, without rendering it), image, or an
    /// image format such as svg, png or jpg. By default its extension and
    /// content tell.
    #[arg(long, value_parser = format)]
    pub format: Option<Input>,

    #[arg(help = help_text("--width"), long_help = long_help_text("--width"), long, value_parser = text::<u16>)]
    pub width: Option<u16>,

    #[arg(help = help_text("--mode"), long_help = long_help_text("--mode"), long, value_parser = named_with_help(mode_help))]
    pub mode: Option<Mode>,

    #[arg(help = help_text("--theme"), long_help = long_help_text("--theme"), long, conflicts_with_all = ["light", "dark"])]
    pub theme: Option<PathBuf>,

    /// A light page in greys, as --theme light
    ///
    /// Whatever the terminal's colours. Code gets colours for a light background.
    #[arg(long, conflicts_with = "dark")]
    pub light: bool,

    /// A dark page in greys, as --theme dark
    ///
    /// Whatever the terminal's colours. Code gets colours for a dark background.
    #[arg(long)]
    pub dark: bool,

    /// No margins, indents or padding, for copying text
    ///
    /// Leaves out the margins, heading indents and code block padding.
    #[arg(short = 'M', long)]
    pub no_margins: bool,

    #[arg(help = help_text("--margins"), long_help = long_help_text("--margins"), long, conflicts_with = "no_margins")]
    pub margins: bool,

    /// Show images as their alt text and diagrams as code
    ///
    /// Images are not read. An image or diagram file given on its own is
    /// still shown.
    #[arg(short = 'I', long)]
    pub no_images: bool,

    #[arg(help = help_text("--images"), long_help = long_help_text("--images"), long, conflicts_with = "no_images")]
    pub images: bool,

    #[arg(help = help_text("--paging"), long_help = long_help_text("--paging"), long, value_parser = named::<Paging>(), value_name = "WHEN", conflicts_with_all = ["pager", "no_pager"])]
    pub paging: Option<Paging>,

    /// Show the document in the pager: --paging=always.
    #[arg(short = 'p', conflicts_with = "no_pager")]
    pub pager: bool,

    /// Print the document without the pager: --paging=never.
    #[arg(short = 'P')]
    pub no_pager: bool,

    #[arg(help = help_text("--follow"), long_help = long_help_text("--follow"), long, value_parser = named::<Follow>(), value_name = "WHEN", conflicts_with_all = ["follow_on", "follow_auto", "follow_off"])]
    pub follow: Option<Follow>,

    /// Follow the file, in the pager: --follow=on.
    #[arg(short = 'f', conflicts_with_all = ["follow_auto", "follow_off"])]
    pub follow_on: bool,

    /// Follow the file and move to a change: --follow=auto.
    #[arg(long = "fa", conflicts_with = "follow_off")]
    pub follow_auto: bool,

    /// Do not follow the file: --follow=off.
    #[arg(short = 'F')]
    pub follow_off: bool,

    #[arg(help = help_text("--follow-jump"), long_help = long_help_text("--follow-jump"), long, value_parser = named::<FollowJump>(), value_name = "WHICH")]
    pub follow_jump: Option<FollowJump>,

    #[arg(help = help_text("--initial-load"), long_help = long_help_text("--initial-load"), long, value_parser = named::<InitialLoad>(), value_name = "HOW")]
    pub initial_load: Option<InitialLoad>,

    #[arg(help = help_text("--passthrough"), long_help = long_help_text("--passthrough"), long, value_parser = named::<Passthrough>(), value_name = "HOW")]
    pub passthrough: Option<Passthrough>,

    #[arg(help = help_text("--front-matter"), long_help = long_help_text("--front-matter"), long, value_parser = named::<FrontMatter>(), value_name = "STYLE")]
    pub front_matter: Option<FrontMatter>,

    #[arg(help = help_text("--headings"), long_help = long_help_text("--headings"), long, value_parser = named::<Headings>(), value_name = "HOW")]
    pub headings: Option<Headings>,

    #[arg(help = help_text("--table-rows"), long_help = long_help_text("--table-rows"), long, value_parser = named::<TableRows>(), value_name = "STYLE")]
    pub table_rows: Option<TableRows>,

    #[arg(help = help_text("--diagrams"), long_help = long_help_text("--diagrams"), long, value_parser = named::<bool>(), value_name = "SWITCH")]
    pub diagrams: Option<bool>,

    #[arg(help = help_text("--diagram-timeout"), long_help = long_help_text("--diagram-timeout"), long, value_parser = text::<Duration>, value_name = "SECS")]
    pub diagram_timeout: Option<Duration>,

    #[arg(help = help_text("--diagram-panel"), long_help = long_help_text("--diagram-panel"), long, value_parser = named::<bool>(), value_name = "SWITCH")]
    pub diagram_panel: Option<bool>,

    #[arg(help = help_text("--render-errors"), long_help = long_help_text("--render-errors"), long, value_parser = named::<RenderErrors>(), value_name = "WHERE")]
    pub render_errors: Option<RenderErrors>,

    /// Leave the mouse to the terminal in the pager
    ///
    /// The pager takes the mouse unless set, so that the wheel scrolls it,
    /// also inside tmux. While it has the mouse, kitty selects text with
    /// Shift and the mouse.
    #[arg(long)]
    pub no_mouse: bool,

    #[arg(help = help_text("--mouse"), long_help = long_help_text("--mouse"), long, conflicts_with = "no_mouse")]
    pub mouse: bool,

    #[arg(help = help_text("--reading"), long_help = long_help_text("--reading"), long, conflicts_with = "no_reading")]
    pub reading: bool,

    /// Reading mode off, whatever KIRI_READING and the configuration say
    #[arg(long, conflicts_with = "reading")]
    pub no_reading: bool,

    #[arg(help = help_text("--wide"), long_help = long_help_text("--wide"), long, value_name = "HOW", value_parser = named::<Wide>())]
    pub wide: Option<Wide>,

    #[arg(help = help_text("--max-image-size"), long_help = long_help_text("--max-image-size"), long, value_name = "MB", value_parser = text::<Megabytes>)]
    pub max_image_size: Option<Megabytes>,
}

/// The `--format` value.
fn format(value: &str) -> Result<Input, String> {
    kiri::typeset::input::format(value).ok_or_else(|| {
        "give markdown, csv, tsv, mermaid, text, image, or an image format such as svg, png \
         or jpg"
            .to_owned()
    })
}

/// A parser for the values of `T`, by their names.
fn named<T: Named + Send + Sync>() -> impl TypedValueParser<Value = T> {
    named_with_help(|_| None)
}

/// A parser for the values of `T`, by their names, with the help on each
/// value that `help` gives.
fn named_with_help<T: Named + Send + Sync>(
    help: fn(T) -> Option<&'static str>,
) -> impl TypedValueParser<Value = T> {
    let values = (T::ALL.iter()).map(|&value| PossibleValue::new(value.name()).help(help(value)));
    PossibleValuesParser::new(values).map(|name| T::named(&name).expect("a value's name"))
}

/// What `--help` tells of each mode.
fn mode_help(mode: Mode) -> Option<&'static str> {
    Some(match mode {
        Mode::Auto => "Ask the terminal; plain text in a pipe",
        Mode::Plain => "Text only, no escape sequences",
        Mode::Ansi => "Colours and attributes, no kitty extensions",
        Mode::Kitty => "All kitty features, without asking the terminal",
    })
}

/// What the command line asks for, each with the flag it comes from.
pub fn options(cli: &Cli) -> Options {
    Options {
        width: flag(cli.width.map(Value::or_unset), "--width"),
        mode: flag(cli.mode, "--mode"),
        max_image_size: flag(cli.max_image_size, "--max-image-size"),
        diagram_timeout: flag(cli.diagram_timeout, "--diagram-timeout"),
        diagram_background_panel: flag(cli.diagram_panel, "--diagram-panel"),
        theme: (flag(cli.theme.clone(), "--theme"))
            .or(flag(cli.light.then(|| PathBuf::from("light")), "--light"))
            .or(flag(cli.dark.then(|| PathBuf::from("dark")), "--dark")),
        margins: (flag(cli.margins.then_some(true), "--margins"))
            .or(flag(cli.no_margins.then_some(false), "--no-margins")),
        images: (flag(cli.images.then_some(true), "--images"))
            .or(flag(cli.no_images.then_some(false), "--no-images")),
        paging: (flag(cli.paging, "--paging"))
            .or(flag(cli.pager.then_some(Paging::Always), "-p"))
            .or(flag(cli.no_pager.then_some(Paging::Never), "-P")),
        follow: follow(cli),
        follow_jump: flag(cli.follow_jump, "--follow-jump"),
        initial_load: flag(cli.initial_load, "--initial-load"),
        passthrough: flag(cli.passthrough, "--passthrough"),
        front_matter: flag(cli.front_matter, "--front-matter"),
        headings: flag(cli.headings, "--headings"),
        table_rows: flag(cli.table_rows, "--table-rows"),
        diagrams: flag(cli.diagrams, "--diagrams"),
        render_errors: flag(cli.render_errors, "--render-errors"),
        mouse: (flag(cli.mouse.then_some(true), "--mouse"))
            .or(flag(cli.no_mouse.then_some(false), "--no-mouse")),
        reading: (flag(cli.reading.then_some(true), "--reading"))
            .or(flag(cli.no_reading.then_some(false), "--no-reading")),
        wide: flag(cli.wide, "--wide"),
        ..Options::default()
    }
}

/// How the command line asks to follow the file, with the flag it comes
/// from.
pub fn follow(cli: &Cli) -> Option<Setting<Follow>> {
    (flag(cli.follow, "--follow"))
        .or(flag(cli.follow_on.then_some(Follow::On), "-f"))
        .or(flag(cli.follow_auto.then_some(Follow::Auto), "--fa"))
        .or(flag(cli.follow_off.then_some(Follow::Off), "-F"))
}

/// How the command line asks to follow the file, `Off` when it does not.
pub fn followed(cli: &Cli) -> Follow {
    follow(cli).map_or(Follow::Off, |follow| follow.value)
}

/// `value`, when the command line gives it, from the flag `name`: the
/// flag's long name, or its short one where it has no long one, as clap
/// does not tell which of them the user gave.
fn flag<T>(value: Option<T>, name: &'static str) -> Option<Setting<T>> {
    value.map(|value| Setting::new(value, Origin::Flag(name)))
}
