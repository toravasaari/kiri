//! The command line: the clap definition of kiri's options, and what each
//! of them asks for, as `Options`, with the flag it comes from. Settings
//! are declared in `src/config/registry.rs`; the help of a setting's flag
//! is declared there too, with the setting.

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use clap::builder::{PossibleValue, PossibleValuesParser, TypedValueParser};
use kiri::config::{
    FrontMatter, Megabytes, Named, Options, Origin, Passthrough, Setting, Value, text,
};
use kiri::render::{Headings, RenderErrors, TableRows};
use kiri::session::settings::{Follow, FollowJump, InitialLoad, Paging};
use kiri::term::Mode;
use kiri::typeset::input::Input;

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

    /// Width in cells; defaults to the terminal width
    #[arg(long, value_parser = text::<u16>)]
    pub width: Option<u16>,

    /// Which terminal features to use [default: auto]
    ///
    /// Colours stay off when NO_COLOR is set.
    #[arg(long, value_parser = named_with_help(mode_help))]
    pub mode: Option<Mode>,

    /// Theme file, or a theme's name
    ///
    /// A name, such as dark, is NAME.toml in the themes directory, or the
    /// built-in light or dark paper. It overrides KIRI_THEME and the
    /// configuration file.
    #[arg(long, conflicts_with_all = ["light", "dark"])]
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

    /// Keep the margins, whatever KIRI_MARGINS and the configuration say
    #[arg(long, conflicts_with = "no_margins")]
    pub margins: bool,

    /// Show images as their alt text and diagrams as code
    ///
    /// Images are not read. An image or diagram file given on its own is
    /// still shown.
    #[arg(short = 'I', long)]
    pub no_images: bool,

    /// Show images, whatever KIRI_NO_IMAGES and the configuration say
    ///
    /// Diagrams are shown too, unless they are off.
    #[arg(long, conflicts_with = "no_images")]
    pub images: bool,

    /// When to show the document in the pager
    ///
    /// With auto, the pager opens when the document is longer than the
    /// window and the output is a terminal.
    #[arg(long, value_parser = named::<Paging>(), value_name = "WHEN", conflicts_with_all = ["pager", "no_pager"])]
    pub paging: Option<Paging>,

    /// Show the document in the pager: --paging=always.
    #[arg(short = 'p', conflicts_with = "no_pager")]
    pub pager: bool,

    /// Print the document without the pager: --paging=never.
    #[arg(short = 'P')]
    pub no_pager: bool,

    /// Follow a lone file in the pager
    ///
    /// With on, kiri reads the file again whenever it changes, with the view
    /// kept where it is; with auto it also moves the view to a change; off
    /// unless set. Following pages the file, however short, unless paging is
    /// never. F goes from off to on to auto in the pager.
    #[arg(long, value_parser = named::<Follow>(), value_name = "WHEN", conflicts_with_all = ["follow_on", "follow_auto", "follow_off"])]
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

    /// Which change auto-follow moves the view to
    ///
    /// When a reload changed the document in more than one place: first,
    /// last or most (the one that takes the most rows); first unless set.
    #[arg(long, value_parser = named::<FollowJump>(), value_name = "WHICH")]
    pub follow_jump: Option<FollowJump>,

    /// Whether the pager waits for images before it shows the document
    ///
    /// With wait, the document is shown once its images and diagrams have
    /// loaded; with background, at once, with each image as it comes; wait
    /// unless set.
    /// Only where the pager surely opens: with -p or -f, or with paging on
    /// auto for a document longer than the window, each image counted at
    /// three rows; otherwise kiri waits.
    #[arg(long, value_parser = named::<InitialLoad>(), value_name = "HOW")]
    pub initial_load: Option<InitialLoad>,

    /// Whether kitty's images pass through tmux
    ///
    /// With detect, kiri wraps them for tmux when it runs inside tmux and
    /// turns on the pane's allow-passthrough, as `kitten icat` does; with
    /// tmux, it wraps them always; with none, never; detect unless set.
    #[arg(long, value_parser = named::<Passthrough>(), value_name = "HOW")]
    pub passthrough: Option<Passthrough>,

    /// How to show the front matter
    ///
    /// Front matter is the metadata block at the start of a Markdown file.
    /// The styles are table, code (a code block), hide, or off (read as
    /// Markdown); table unless set.
    #[arg(long, value_parser = named::<FrontMatter>(), value_name = "STYLE")]
    pub front_matter: Option<FrontMatter>,

    /// How headings show
    ///
    /// auto draws them large with text sizing, or as images where the
    /// terminal has no text sizing; sized only with text sizing; images
    /// always where images can be drawn; plain in one row; auto unless
    /// set.
    #[arg(long, value_parser = named::<Headings>(), value_name = "HOW")]
    pub headings: Option<Headings>,

    /// Table row style
    ///
    /// The styles are zebra (every second row shaded where the terminal's
    /// colours are known), lines, indent (the lines after the first of a
    /// wrapped value start two cells in), underline (each row's last line
    /// underlined), or off; zebra unless set.
    #[arg(long, value_parser = named::<TableRows>(), value_name = "STYLE")]
    pub table_rows: Option<TableRows>,

    /// Whether to draw diagrams and formulas
    ///
    /// Diagrams in code blocks, such as Mermaid, and formulas: on (drawn as
    /// images) or off (shown as code); on unless set.
    #[arg(long, value_parser = named::<bool>(), value_name = "SWITCH")]
    pub diagrams: Option<bool>,

    /// How long a program may take to draw one diagram, in seconds
    #[arg(long, value_parser = text::<Duration>, value_name = "SECS")]
    pub diagram_timeout: Option<Duration>,

    /// Whether a diagram in the other page's colours lies on a panel
    ///
    /// on or off; on unless set.
    #[arg(long, value_parser = named::<bool>(), value_name = "SWITCH")]
    pub diagram_panel: Option<bool>,

    /// Where to tell why a diagram could not be drawn
    ///
    /// The places are inline (in the document, below its code), stderr (a
    /// warning) or both; both unless set.
    #[arg(long, value_parser = named::<RenderErrors>(), value_name = "WHERE")]
    pub render_errors: Option<RenderErrors>,

    /// Leave the mouse to the terminal in the pager
    ///
    /// The pager takes the mouse unless set, so that the wheel scrolls it,
    /// also inside tmux. While it has the mouse, kitty selects text with
    /// Shift and the mouse.
    #[arg(long)]
    pub no_mouse: bool,

    /// Let the pager take the mouse
    ///
    /// Whatever KIRI_MOUSE and the configuration say.
    #[arg(long, conflicts_with = "no_mouse")]
    pub mouse: bool,

    /// Experimental: the text in a proportional font, as a browser shows it
    ///
    /// Headings, paragraphs, lists and quotes are drawn as images, so their
    /// text cannot be selected or copied. It needs a terminal that draws
    /// images, such as kitty; elsewhere the document shows as text. The
    /// option's name may still change.
    #[arg(long)]
    pub reading: bool,

    /// Experimental: wide code and tables wrap or scroll in reading mode
    ///
    /// With wrap, a code block or table wider than reading mode's area
    /// wraps to fit it; with scroll, it keeps its lines and words whole and
    /// scrolls sideways in the pager, where W switches between the two.
    /// Printing always wraps. wrap unless set. The option's name may still
    /// change.
    #[arg(long, value_name = "HOW", value_parser = ["wrap", "scroll"])]
    pub wide: Option<String>,

    /// Largest image file to read, in megabytes [default: 10]
    ///
    /// At most 100; larger images show their alt text.
    #[arg(long, value_name = "MB", value_parser = text::<Megabytes>)]
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

/// `value`, when the command line gives it, from the flag `name`: the
/// flag's long name, or its short one where it has no long one, as clap
/// does not tell which of them the user gave.
fn flag<T>(value: Option<T>, name: &'static str) -> Option<Setting<T>> {
    value.map(|value| Setting::new(value, Origin::Flag(name)))
}
