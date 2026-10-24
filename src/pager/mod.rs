//! The interactive pager: the document on a screen of its own, scrolled
//! with keys.

pub mod changes;
pub mod fade;
pub mod files;
pub mod help;
pub mod keys;
pub mod loading;
pub mod prompt;
pub mod search;
pub mod status;
pub mod tint;
pub mod view;
pub mod warnings;

use std::io::{self, Write};
use std::ops::Range;
use std::time::{Duration, Instant, SystemTime};

use crate::caps::Capabilities;
use crate::compare::{Comparison, Fate, compare};
use crate::config::{self, FollowJump, Paging, SearchCase};
use crate::images::{ImageError, ImageKey, Images, Loads, Source};
use crate::model::{Document, File, FrontMatterError};
use crate::render::Inputs;
use crate::render::block::BlockInfo;
use crate::render::encoder::Encoder;
use crate::render::layout::{Content, Stream};
use crate::render::output::{self, Page, Writer};
use crate::render::style::{self, Line, Span, Style};
use crate::render::wrap;
use crate::size::TextSize;
use crate::term::{Screen, WindowSize};
use fade::{
    FADE, FADE_OUT, Fade, HIGHLIGHT_FADE, HIGHLIGHT_SHARE, HIGHLIGHT_STAY, Rgb, STAY, WARNING_STAY,
};
use files::Files;
use help::Placed;
use keys::{Action, Key};
use prompt::{Edit, Prompt};
use search::{Marks, Search};
use view::View;

/// Whether a document of `rows` rows is paged: never when the output is not
/// a terminal, and with `Auto` only when it is longer than the window's
/// `height`, when that is known.
pub fn pages(paging: Paging, terminal: bool, rows: usize, height: Option<u16>) -> bool {
    match paging {
        _ if !terminal => false,
        Paging::Always => true,
        Paging::Auto => height.is_some_and(|height| rows > usize::from(height)),
        Paging::Never => false,
    }
}

/// `paging` for what is paged only when paging is `always`: a document
/// read from standard input, often the output of another command, the
/// `--doctor` report, and images shown on their own.
pub fn only_always(paging: Paging) -> Paging {
    match paging {
        Paging::Always => Paging::Always,
        Paging::Auto | Paging::Never => Paging::Never,
    }
}

/// `paging` for several files: when `images_only`, each of them is shown
/// on its own as an image is, such as an image file or a diagram file, and
/// they are paged only when paging is `always`; with a document among
/// them, the files follow `paging` as one document does.
pub fn for_files(paging: Paging, images_only: bool) -> Paging {
    match images_only {
        true => only_always(paging),
        false => paging,
    }
}

/// The rows `lines` take on a terminal with `caps`.
pub fn rows(lines: &[Line], caps: &Capabilities) -> usize {
    lines
        .iter()
        .map(|line| usize::from(crate::render::output::rows(line, caps)))
        .sum()
}

/// The terminal the pager shows a document on.
pub trait Terminal: Write {
    /// The size of the window now.
    fn size(&self) -> Option<WindowSize>;
    /// The bytes of the keys pressed, waiting at most `timeout` for the
    /// first; empty when none came.
    fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>>;
    /// The time now.
    fn now(&self) -> Instant {
        Instant::now()
    }
}

impl<W: Write> Terminal for Screen<W> {
    fn size(&self) -> Option<WindowSize> {
        Screen::size(self)
    }

    fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        Screen::read(self, timeout)
    }
}

/// What the pager asks for the document again, as `r` does.
pub trait Reload {
    /// The document read again, with `images` brought up to date for it
    /// as it is shown in a window with `caps`: it keeps at once what the
    /// document still shows, and what it needs and does not have loads
    /// in the background in `loads`, which may stop what is loading
    /// there for the document before. What `images` held that the
    /// document no longer shows, or that loads anew, is set aside in it,
    /// as [`crate::images::reload_in_background`] does.
    fn reload(&mut self, images: &mut Images, loads: &mut Loads, caps: &Capabilities) -> Reloaded;

    /// Whether the document changed since it was last read, as the pager
    /// asks while it follows it.
    fn change(&mut self) -> Change;
}

/// Whether the document changed since it was last read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Unchanged,
    /// It changed, and looks as `Look` tells now.
    Changed(Look),
    /// It cannot be looked at, such as a file that is gone: why, as the
    /// status line says it.
    Gone(String),
}

/// What tells one version of a document from another without reading it:
/// for a file, when it was last modified and its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub modified: Option<SystemTime>,
    pub length: u64,
}

/// Something kiri found wrong with a document while it read it, which
/// the pager tells about without stopping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// The line of the file the trouble is at, counting from 1, when
    /// known.
    pub line: Option<usize>,
    /// What is wrong, as one line of text.
    pub message: String,
}

impl From<&FrontMatterError> for Warning {
    fn from(error: &FrontMatterError) -> Self {
        Self {
            line: error.line,
            message: format!("front matter: {}", error.message),
        }
    }
}

/// What reloading the document came to.
pub enum Reloaded {
    Document(Document),
    /// As `Document`, with warnings about it, the first of which the
    /// pager shows.
    Warned(Document, Vec<Warning>),
    /// The document could not be read again: why, as the status line says
    /// it.
    Failed(String),
}

/// What the pager is told besides the document.
pub struct Options<'a> {
    /// The columns the document is laid out in, when the window's are not
    /// to decide.
    pub width: Option<u16>,
    /// How a search treats case.
    pub case: SearchCase,
    /// Where `r` gets the document again from; `None` when it cannot.
    pub reload: Option<&'a mut dyn Reload>,
    /// Whether the pager follows the document from the start: reads it
    /// again whenever it changes.
    pub follow: config::Follow,
    /// Which change following on auto moves the view to.
    pub jump: FollowJump,
    /// The warnings about the document as first read, which the pager
    /// tells as it does those of a reload.
    pub warnings: Vec<Warning>,
}

impl Options<'_> {
    /// A window's width to decide the layout, searches that treat case as
    /// `case` says, and no way to reload.
    pub fn new(case: SearchCase) -> Self {
        Options {
            width: None,
            case,
            reload: None,
            follow: config::Follow::Off,
            jump: FollowJump::First,
            warnings: Vec::new(),
        }
    }
}

/// Shows `content` on `terminal` until the reader quits, laid out with
/// `inputs` for the window's size, or `options.width` columns when it is
/// given.
/// The status line calls a lone document `name`, and names each of
/// several files after itself. `laid_out`, when given, is `content` laid
/// out with `inputs` as they are now, used unless the window calls for
/// another.
/// A search treats case as `options.case` says.
pub fn run(
    content: Content,
    laid_out: Option<Stream>,
    name: &str,
    inputs: &mut Inputs,
    options: Options,
    terminal: &mut impl Terminal,
) -> io::Result<()> {
    let Options {
        width,
        case,
        reload,
        follow,
        jump,
        warnings,
    } = options;
    let mut source = Origin {
        reload,
        loads: Loads::new(),
        started: terminal.now(),
    };
    let page = Page {
        background: inputs.theme.background,
        foreground: inputs.theme.foreground,
    };
    let mut laid_out = laid_out.map(|lines| (inputs.caps.clone(), lines));
    let mut size = terminal.size();
    let mut view: Option<View> = None;
    let mut state = State::new(case);
    state.warn(warnings, terminal.now());
    if source.reload.is_some() {
        state.follow.mode = follow;
    } else if follow != config::Follow::Off {
        state.told = Some(Told::new(NOTHING_TO_FOLLOW));
    }
    // The document as reloaded, once it has been.
    let mut reloaded: Option<Document> = None;
    // How the reloaded document differs from the one before it, until the
    // view has found its place in the new lines.
    let mut changes: Option<Comparison> = None;
    // What the terminal holds of the images, kept from one layout to the
    // next.
    let mut encoder = Encoder::new();
    // Whether the next layout keeps the view's top line, as when an image
    // came in its placeholder's place.
    let mut keep_top = false;
    // The blocks of the lines auto-follow tinted, and when, to find them
    // again in the next layout.
    let mut tinted: Option<(Instant, Vec<BlockInfo>)> = None;
    let shown_files: &[File] = match content {
        Content::Document(_) => &[],
        Content::Files(files) => files,
    };
    loop {
        fit(&mut inputs.caps, size, width);
        let window = usize::from(inputs.caps.height.unwrap_or(DEFAULT_ROWS));
        // The bottom row is the status line's, unless the document would
        // have no row left.
        let bar = window > 1;
        let rows = if bar { window - 1 } else { window };
        let content = match &reloaded {
            Some(document) => Content::Document(document),
            None => content,
        };
        let stream = match laid_out.take() {
            Some((caps, stream)) if caps == inputs.caps => stream,
            _ => crate::render::layout::content(content, inputs),
        };
        let Stream { mut lines, blocks } = stream;
        // The text sizing protocol cannot draw part of a line, so a line
        // taller than the window is drawn at normal size.
        for line in &mut lines {
            if usize::from(crate::render::output::rows(line, &inputs.caps)) > rows {
                line.size = TextSize::NORMAL;
            }
        }
        // The images the new lines do not place go with their first frame.
        encoder.keep(&lines, &inputs.caps);
        let mut writer = Writer::new(&inputs.images, &inputs.caps, page, &mut encoder);
        let heights: Vec<usize> = lines
            .iter()
            .map(|line| usize::from(writer.rows(line)))
            .collect();
        let places: Vec<Option<BlockInfo>> = lines
            .iter()
            .map(|line| line.block.map(|id| blocks.get(id).clone()))
            .collect();
        if let Some((at, tinted)) = tinted.take() {
            let tinted = |place: &Option<BlockInfo>| {
                (place.as_ref()).is_some_and(|place| tinted.iter().any(|t| t.same_place(place)))
            };
            if let (Some(first), Some(last)) = (
                places.iter().position(tinted),
                places.iter().rposition(tinted),
            ) {
                let lines = first..last + 1;
                state.changed = Some(Changed { at, lines });
            }
        }
        let mut shown = match view.take() {
            Some(mut view) => {
                match changes.take() {
                    Some(changes) => {
                        // Following on auto moves on from the place kept to
                        // a change.
                        let change = (state.follow.mode == config::Follow::Auto)
                            .then(|| changes::groups(&changes, &places, &heights))
                            .and_then(|groups| changes::choose(&groups, jump).cloned());
                        view.resize_by(heights, places, rows, |old, new| {
                            follows(&changes, old, new)
                        });
                        if let Some(change) = change {
                            view.show_change(change.lines.clone());
                            // A removal has no lines to tint.
                            let tintable = Rgb::new(&inputs.theme, &inputs.caps)
                                .is_some_and(|rgb| rgb.tinted(None, 0.0).is_some())
                                && inputs.caps.styling
                                && inputs.caps.truecolor;
                            if tintable && !change.lines.is_empty() {
                                state.changed = Some(Changed {
                                    at: terminal.now(),
                                    lines: change.lines,
                                });
                            }
                        }
                    }
                    None if std::mem::take(&mut keep_top) => view.keep(heights, places, rows),
                    None => view.resize(heights, places, rows),
                }
                view
            }
            None => View::new(heights, rows).with_blocks(places),
        };
        state.lay_out(&lines, shown.top(), bar);
        let status = Status {
            name,
            loading: (source.loads.progress())
                .map(|(done, total)| format!("[rendering: {done}/{total}]")),
            files: Files::new(shown_files, &lines, &blocks),
            styles: status::Styles::new(&inputs.theme, &inputs.caps),
            width: size.map_or(inputs.caps.width, |size| size.columns),
            bar,
            panel: Style {
                fg: inputs.theme.foreground,
                bg: inputs.theme.background,
                ..Style::default()
            },
            muted: style::muted(
                &inputs.theme,
                Style {
                    fg: inputs.theme.foreground,
                    bg: inputs.theme.background,
                    ..Style::default()
                },
            ),
            red: Style {
                fg: inputs.theme.error,
                bg: inputs.theme.background,
                ..Style::default()
            },
        };
        let marks = Marks::new(&inputs.theme, &inputs.caps);
        let rgb = Rgb::new(&inputs.theme, &inputs.caps);
        let mut laid_out = Layout {
            lines: &lines,
            marks: &marks,
            rgb: rgb.as_ref(),
            columns: usize::from(inputs.caps.width),
            status: &status,
            view: &mut shown,
        };
        let outcome = show(
            &mut laid_out,
            &mut state,
            &mut writer,
            terminal,
            size,
            &mut source,
        )?;
        match outcome {
            Outcome::Quit => {
                source.loads.stop();
                encoder.delete_all(terminal)?;
                terminal.flush()?;
                return Ok(());
            }
            Outcome::Loaded(came) => {
                // The tinted lines move with the image that came.
                if let Some(changed) = state.changed.take() {
                    let blocks = (lines.get(changed.lines).unwrap_or_default().iter())
                        .filter_map(|line| line.block.map(|id| blocks.get(id).clone()))
                        .collect();
                    tinted = Some((changed.at, blocks));
                }
                for (key, image) in came {
                    inputs.images.insert(key, image);
                }
                keep_top = true;
                view = Some(shown);
            }
            Outcome::Resized(new) => {
                // The lines laid out for the new size are others.
                state.changed = None;
                size = new;
                view = Some(shown);
            }
            Outcome::Reload => {
                state.changed = None;
                view = Some(shown);
                state.follow.seen = None;
                // The reload may change the pixels of the images.
                encoder.changed();
                let reload = source
                    .reload
                    .as_deref_mut()
                    .expect("a source to reload from");
                let now = terminal.now();
                let before = inputs.images.take_placeholders();
                let reloaded_now =
                    reload.reload(&mut inputs.images, &mut source.loads, &inputs.caps);
                let replaced = inputs.images.take_replaced();
                let (document, warnings) = match reloaded_now {
                    Reloaded::Document(document) => (document, Vec::new()),
                    Reloaded::Warned(document, warnings) => (document, warnings),
                    Reloaded::Failed(why) => {
                        for (key, placeholder) in before {
                            if source.loads.loading(&key) {
                                inputs.images.wait(key, placeholder);
                            }
                        }
                        state.told = Some(Told::new(why));
                        continue;
                    }
                };
                if let Content::Document(old) = content {
                    let comparison = compare(old, &document);
                    let left = loading::Reloaded {
                        old,
                        new: &document,
                        changes: &comparison,
                        before: &before,
                        replaced: &replaced,
                        images: &inputs.images,
                    };
                    let keys: Vec<ImageKey> = source.loads.keys().cloned().collect();
                    let placeholders: Vec<_> = (keys.into_iter())
                        .map(|key| {
                            let placeholder = left.placeholder(&key);
                            (key, placeholder)
                        })
                        .collect();
                    for (key, placeholder) in placeholders {
                        inputs.images.wait(key, placeholder);
                    }
                    changes = Some(comparison);
                }
                reloaded = Some(document);
                state.warn(warnings, now);
                state.told = Some(Told::fading("reloaded", now));
            }
        }
    }
}

/// Whether the block `new` of the reloaded document is the block `old` of
/// the one before it, as `changes` found: the block that stayed or changed
/// from where `old` was.
fn follows(changes: &Comparison, old: &BlockInfo, new: &BlockInfo) -> bool {
    old.file() == new.file()
        && matches!(
            changes.fate(new.path()),
            Some(Fate::Stayed { was } | Fate::Changed { was }) if was == old.path()
        )
}

/// What the status line says when there is no file to follow.
const NOTHING_TO_FOLLOW: &str = "nothing to follow";

/// The rows of a window whose size the terminal does not tell.
const DEFAULT_ROWS: u16 = 24;

/// How long the pager waits for a key before it looks at the window size
/// again.
const POLL: Duration = Duration::from_millis(100);

/// How long the pager waits for a key while a fade is under way, so that
/// it draws a step of it each time.
const FRAME: Duration = Duration::from_millis(50);

/// Why the pager stopped showing one layout of the document.
enum Outcome {
    Quit,
    /// The window is now of this size.
    Resized(Option<WindowSize>),
    /// The document is to be read again: the reader asked for it, and the
    /// status line says it is being read, or it changed while followed.
    Reload,
    /// These images, loading in the background, came.
    Loaded(Vec<(ImageKey, Result<Source, ImageError>)>),
}

/// Where the pager gets the document and its images again.
struct Origin<'a> {
    /// Where `r` gets the document again from; `None` when it cannot.
    reload: Option<&'a mut dyn Reload>,
    /// The images loading in the background after a reload.
    loads: Loads,
    /// When the pager started, from when the bands below the images that
    /// load move.
    started: Instant,
}

/// What a key did, besides what the pager does itself.
enum Pressed {
    Nothing,
    Quit,
    /// The reader asked for the document again.
    Reload,
    /// The reader changed how the document is followed.
    Follow,
}

/// What the status line and the help panel show and how, apart from the
/// position.
struct Status<'a> {
    /// What a lone document is called.
    name: &'a str,
    /// What stands in place of the name while images load: how many have
    /// come of how many.
    loading: Option<String>,
    /// The files shown, each named after itself.
    files: Files<'a>,
    styles: status::Styles,
    /// The window's columns.
    width: u16,
    /// Whether the window has a row for the status line besides the
    /// document's.
    bar: bool,
    /// The help panel's style, and that of its notes.
    panel: Style,
    muted: Style,
    /// The style of the warning line: the theme's error colour on the
    /// page.
    red: Style,
}

impl Status<'_> {
    /// The status line for `view` in `state`, with the match number faded
    /// by `fade` after the search went around an end and a message faded
    /// out by `out`: the prompt while it is open.
    ///
    /// The mark for the document's warnings has its number flashing
    /// while `counted` is a fade.
    fn spans(
        &self,
        view: &View,
        state: &State,
        fade: Option<Fade>,
        out: Option<Fade>,
        counted: Option<Fade>,
    ) -> Vec<Span> {
        if !self.bar {
            return Vec::new();
        }
        let width = usize::from(self.width);
        match &state.prompt {
            Some(prompt) => status::prompt('/', prompt.term(), width, &self.styles),
            None if state.colon => status::prompt(':', "", width, &self.styles),
            None => {
                let flash = fade
                    .filter(|_| state.jumped.is_some_and(|jump| jump.around))
                    .map(|fade| self.styles.number(fade));
                let fading = out.and_then(|out| self.styles.fading(out));
                let found = state.found(flash, fading);
                let file = state.file(&self.files, view.top());
                let name = file.map_or(self.name, |file| self.files.name(file));
                let name = self.loading.as_deref().unwrap_or(name);
                let place = (file.filter(|_| self.files.several()))
                    .map(|file| (file + 1, self.files.count()));
                let position = view.position();
                let tags = status::Tags {
                    follow: state.follow.mode,
                    warnings: (!state.warnings.is_empty()).then(|| status::Warned {
                        count: state.warnings.len(),
                        flash: counted.map(|fade| self.styles.number(fade)),
                    }),
                };
                status::spans(name, place, position, tags, found, width, &self.styles)
            }
        }
    }

    /// The warning line over the last row of `view` at `now`, where the
    /// document's warnings in `state` are told; it fades out into the
    /// page with `rgb`'s colours after [`WARNING_STAY`].
    fn notice(&self, view: &View, state: &State, now: Instant, rgb: Option<&Rgb>) -> Vec<Placed> {
        let Some(at) = state.noticed else {
            return Vec::new();
        };
        if !self.bar {
            return Vec::new();
        }
        // The rest of the text, mixed towards the page; the muted style
        // where the colours are not known.
        let mixed = rgb.and_then(|rgb| {
            let (text, page) = rgb.colors(&self.panel)?;
            text.mix(page, 0.4)
        });
        let dim = match mixed {
            Some(color) => Style {
                fg: Some(color),
                ..self.panel.clone()
            },
            None => self.muted.clone(),
        };
        let (mut red, mut dim) = (self.red.clone(), dim);
        if let Some(faded) = now.saturating_duration_since(at).checked_sub(WARNING_STAY) {
            let fade = Fade {
                elapsed: faded,
                rgb,
            };
            for style in [&mut red, &mut dim] {
                let gone = rgb
                    .and_then(|rgb| rgb.colors(style))
                    .map(|(_, page)| Style {
                        fg: Some(page),
                        bg: Some(page),
                        italic: true,
                        ..style.clone()
                    });
                if let Some(faded) = gone.and_then(|gone| fade.mix(style, &gone)) {
                    *style = faded;
                }
            }
        }
        let columns = usize::from(self.width);
        let spans = warnings::spans(&state.warnings, columns, &red, &dim);
        if spans.is_empty() {
            return Vec::new();
        }
        let column = columns.saturating_sub(wrap::width(&spans));
        vec![Placed {
            row: view.rows(),
            column: column.max(1),
            spans,
        }]
    }

    /// The help panel over `view`, when `open`.
    fn panel(&self, view: &View, open: bool) -> Vec<Placed> {
        if open {
            help::panel(
                usize::from(self.width),
                view.rows(),
                &self.panel,
                &self.muted,
                self.files.several(),
            )
        } else {
            Vec::new()
        }
    }
}

/// `caps` for a window of `size`, `width` columns wide when it is given.
fn fit(caps: &mut Capabilities, size: Option<WindowSize>, width: Option<u16>) {
    let Some(size) = size else {
        return;
    };
    caps.width = width.unwrap_or(size.columns);
    caps.height = Some(size.rows).filter(|&rows| rows > 0);
    // A new font size changes the cells, when the window tells its pixels.
    if let Some(cell) = size.cell_pixels() {
        caps.cell_pixels = Some(cell);
    }
}

/// What the reader does in the pager besides scrolling, which stays when
/// the window changes size.
#[derive(Debug)]
struct State {
    /// How a search treats case.
    case: SearchCase,
    /// Whether the help is open.
    help: bool,
    /// The search prompt, while it is open.
    prompt: Option<Prompt>,
    /// The last search.
    search: Option<Search>,
    /// Whether the last search is shown; Escape hides it until `n` or
    /// `N`.
    shown: bool,
    /// Whether the status line tells that the last search found nothing,
    /// which it does until the next key.
    nowhere: bool,
    /// The last jump to a match, while its fade is under way.
    jumped: Option<Jump>,
    /// Whether a colon was typed, so that the next key gives a command.
    colon: bool,
    /// The last move to a file, while the view stays where it took it.
    went: Option<Went>,
    /// What the status line tells about the last key until the next one,
    /// such as that there is no next file, or until it has faded out.
    told: Option<Told>,
    /// Following the document as it changes.
    follow: Follow,
    /// The lines of the change auto-follow went to, while they are
    /// tinted.
    changed: Option<Changed>,
    /// The warnings about the document shown.
    warnings: Vec<Warning>,
    /// When the warning line was last told, while it stays and fades.
    noticed: Option<Instant>,
    /// When the number of warnings last changed, while the number of the
    /// mark on the status line flashes.
    counted: Option<Instant>,
}

/// How much to tint which lines, over how many cells, in what colours.
struct Tint<'a> {
    lines: Range<usize>,
    /// The share of the text colour mixed into the background.
    amount: f32,
    rgb: &'a Rgb,
    columns: usize,
}

/// The change auto-follow moved the view to.
#[derive(Debug, Clone)]
struct Changed {
    at: Instant,
    /// Its lines in the laid-out document.
    lines: Range<usize>,
}

/// Following the document: reading it again whenever it changes.
#[derive(Debug, Default)]
struct Follow {
    mode: config::Follow,
    /// How a change looked on the last poll, and when it was first seen
    /// looking so. It is read once a poll at least [`POLL`] later finds it
    /// looking the same, so that a file caught half written is not shown.
    seen: Option<(Look, Instant)>,
    /// Whether the document could not be looked at on the last poll, which
    /// the status line told.
    gone: bool,
}

/// What a poll of the document while it is followed calls for.
#[derive(Debug, PartialEq, Eq)]
enum Polled {
    Nothing,
    Reload,
    /// The status line is to tell why the document cannot be looked at.
    Gone(String),
}

impl Follow {
    /// Whether the document is followed, on or auto.
    fn on(&self) -> bool {
        self.mode != config::Follow::Off
    }

    /// What `change`, found on a poll at `now`, calls for.
    fn poll(&mut self, change: Change, now: Instant) -> Polled {
        let gone = std::mem::replace(&mut self.gone, false);
        match change {
            Change::Unchanged => {
                self.seen = None;
                Polled::Nothing
            }
            Change::Changed(look) => match self.seen {
                Some((seen, at)) if seen == look => {
                    match now.saturating_duration_since(at) >= POLL {
                        true => Polled::Reload,
                        false => Polled::Nothing,
                    }
                }
                _ => {
                    self.seen = Some((look, now));
                    Polled::Nothing
                }
            },
            Change::Gone(why) => {
                self.seen = None;
                self.gone = true;
                match gone {
                    true => Polled::Nothing,
                    false => Polled::Gone(why),
                }
            }
        }
    }
}

/// A message on the status line.
#[derive(Debug)]
struct Told {
    message: String,
    /// When it was told, for a message that fades out.
    fades: Option<Instant>,
}

impl Told {
    /// `message`, which stays until the next key.
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fades: None,
        }
    }

    /// `message`, told at `at`, which stays for [`STAY`] and then fades
    /// out over [`FADE_OUT`], unless a key clears it before.
    fn fading(message: impl Into<String>, at: Instant) -> Self {
        Self {
            fades: Some(at),
            ..Self::new(message)
        }
    }
}

/// A move to the start of a file with `:n` or `:p`. The view may stop
/// short of the start, at the last page, so the file the move went to is
/// kept until the view moves on.
#[derive(Debug, Clone, Copy)]
struct Went {
    file: usize,
    /// The top of the view after the move.
    top: usize,
}

/// A jump to a search match.
#[derive(Debug, Clone, Copy)]
struct Jump {
    at: Instant,
    /// Whether the search went around an end of the document to it.
    around: bool,
}

impl State {
    /// Nothing done yet, with searches that treat case as `case` says.
    fn new(case: SearchCase) -> Self {
        Self {
            case,
            help: false,
            prompt: None,
            search: None,
            shown: false,
            nowhere: false,
            jumped: None,
            colon: false,
            went: None,
            told: None,
            follow: Follow::default(),
            changed: None,
            warnings: Vec::new(),
            noticed: None,
            counted: None,
        }
    }

    /// Takes `warnings`, about the document as read at `now`, for the
    /// ones before; the number of them flashes when it differs.
    fn warn(&mut self, warnings: Vec<Warning>, now: Instant) {
        if warnings.len() != self.warnings.len() {
            self.counted = Some(now);
        }
        self.noticed = (!warnings.is_empty()).then_some(now);
        self.warnings = warnings;
    }

    /// Does what `key`, pressed at `now`, asks in `layout`, and tells what
    /// is left for the pager to do.
    fn press(&mut self, key: Key, now: Instant, layout: &mut Layout) -> Pressed {
        self.nowhere = false;
        self.told = None;
        // Any key closes the help and does nothing else, but Ctrl+C
        // still quits.
        if self.help && key != Key::Ctrl('c') {
            self.help = false;
            return Pressed::Nothing;
        }
        // While the prompt is open, keys edit it.
        if let Some(prompt) = &mut self.prompt {
            match prompt.edit(key) {
                Edit::Typing => {}
                Edit::Cancel => self.prompt = None,
                Edit::Search(term) => {
                    self.prompt = None;
                    self.find(&term, now, layout);
                }
                // The last search again goes on to its next match, as n
                // does.
                Edit::Repeat => {
                    self.prompt = None;
                    self.step(Search::forward, now, layout);
                }
            }
            return Pressed::Nothing;
        }
        // The key after a colon is a command's, and any other closes it.
        // The commands move between files, so a lone document has none
        // and its colon does nothing.
        let key = match key {
            _ if std::mem::take(&mut self.colon) => match key {
                Key::Char(c) => Key::Command(c),
                _ => return Pressed::Nothing,
            },
            keys::COMMAND if layout.status.files.several() => {
                self.colon = true;
                return Pressed::Nothing;
            }
            key => key,
        };
        match keys::action(key) {
            Some(Action::Quit) => return Pressed::Quit,
            Some(Action::Move(motion)) => layout.view.scroll(motion),
            // The help and the prompt need a row besides the status line's.
            Some(Action::Help) => self.help = layout.status.bar,
            Some(Action::Search) => {
                self.prompt = layout.status.bar.then(Prompt::default);
            }
            Some(Action::Next) => self.step(Search::forward, now, layout),
            Some(Action::Previous) => self.step(Search::backward, now, layout),
            Some(Action::Clear) => self.shown = false,
            Some(Action::NextFile) => {
                let files = layout.status.files;
                if let Some(file) = self.file(&files, layout.view.top()) {
                    self.go_to(files.next(file), "no next file", layout);
                }
            }
            Some(Action::PreviousFile) => {
                let files = layout.status.files;
                let top = layout.view.top();
                if let Some(file) = self.file(&files, top) {
                    self.go_to(files.previous(file, top), "no previous file", layout);
                }
            }
            Some(Action::Reload) => return Pressed::Reload,
            Some(Action::Follow) => return Pressed::Follow,
            None => {}
        }
        Pressed::Nothing
    }

    /// The file the reader is in, of `files`, with the view's top on the
    /// line `top`: the one the last move to a file went to, or else the
    /// one at the top.
    fn file(&self, files: &Files, top: usize) -> Option<usize> {
        self.went.map(|went| went.file).or_else(|| files.at(top))
    }

    /// Moves the view to the start of `file`, when there is one to go to,
    /// and otherwise tells `none` on the status line.
    fn go_to(&mut self, file: Option<usize>, none: &'static str, layout: &mut Layout) {
        let Some(file) = file else {
            self.told = Some(Told::new(none));
            return;
        };
        layout.view.go_to(layout.status.files.start(file));
        self.went = Some(Went {
            file,
            top: layout.view.top(),
        });
    }

    /// Forgets the last move to a file once the view's top, now on the
    /// line `top`, has moved from where the move took it.
    fn moved(&mut self, top: usize) {
        self.went = self.went.filter(|went| went.top == top);
    }

    /// Searches for `term` from the top of the view at `now`, and shows
    /// the first match.
    fn find(&mut self, term: &str, now: Instant, layout: &mut Layout) {
        let search = Search::new(layout.lines, term, self.case, layout.view.top());
        self.jumped = None;
        match search.current() {
            Some(found) => self.jump(found.line, now, false, layout),
            None => self.nowhere = true,
        }
        self.search = Some(search);
        self.shown = true;
    }

    /// Shows the match on line `line`, jumped to at `now`, `around` an
    /// end of the document or not.
    fn jump(&mut self, line: usize, now: Instant, around: bool, layout: &mut Layout) {
        layout.view.reveal(line);
        self.jumped = Some(Jump { at: now, around });
    }

    /// Finds the last search again in `lines`, laid out anew, from the
    /// line `top` of the view, closes the prompt when the window has no
    /// status line, `bar`, to show it on, and moves the last move to a
    /// file along with the view: it is only the new layout that put the
    /// view's top on another line, not a key.
    fn lay_out(&mut self, lines: &[Line], top: usize, bar: bool) {
        if !bar {
            self.prompt = None;
        }
        if let Some(went) = &mut self.went {
            went.top = top;
        }
        if let Some(search) = &mut self.search {
            *search = Search::new(lines, search.term(), self.case, top);
        }
    }

    /// Moves the last search to another match by `step` at `now`, and
    /// shows it.
    fn step(
        &mut self,
        step: fn(&mut Search, Range<usize>) -> bool,
        now: Instant,
        layout: &mut Layout,
    ) {
        let Some(search) = &mut self.search else {
            return;
        };
        let around = step(search, layout.view.visible());
        match search.current().map(|found| found.line) {
            Some(line) => self.jump(line, now, around, layout),
            None => self.nowhere = true,
        }
        self.shown = true;
    }

    /// Ends a fade that is over at `now`, and a message that has faded
    /// out.
    fn tick(&mut self, now: Instant) {
        self.jumped = self
            .jumped
            .filter(|jump| now.saturating_duration_since(jump.at) < FADE);
        self.changed = self.changed.take().filter(|changed| {
            now.saturating_duration_since(changed.at) < HIGHLIGHT_STAY + HIGHLIGHT_FADE
        });
        self.noticed = self
            .noticed
            .filter(|at| now.saturating_duration_since(*at) < WARNING_STAY + FADE);
        self.counted = self
            .counted
            .filter(|at| now.saturating_duration_since(*at) < FADE);
        self.told = self.told.take().filter(|told| {
            told.fades
                .is_none_or(|at| now.saturating_duration_since(at) < STAY + FADE_OUT)
        });
    }

    /// The fade out of the message under way at `now`, mixing colours
    /// with `rgb`.
    fn fade_out<'a>(&self, now: Instant, rgb: Option<&'a Rgb>) -> Option<Fade<'a>> {
        let at = self.told.as_ref()?.fades?;
        let elapsed = now.saturating_duration_since(at).checked_sub(STAY)?;
        Some(Fade { elapsed, rgb })
    }

    /// The tint at `now`, in `rgb`'s colours over `columns` cells: the
    /// lines of the change, and how much of the text colour to mix into
    /// their background: all of [`HIGHLIGHT_SHARE`]
    /// while the tint stays, then less as it fades out.
    fn tint<'a>(&self, now: Instant, rgb: &'a Rgb, columns: usize) -> Option<Tint<'a>> {
        let changed = self.changed.as_ref()?;
        let elapsed = now.saturating_duration_since(changed.at);
        let left = match elapsed.checked_sub(HIGHLIGHT_STAY) {
            None => 1.0,
            Some(faded) => 1.0 - faded.as_secs_f32() / HIGHLIGHT_FADE.as_secs_f32(),
        };
        Some(Tint {
            lines: changed.lines.clone(),
            amount: HIGHLIGHT_SHARE * left.max(0.0),
            rgb,
            columns,
        })
    }

    /// The flash of the warning count under way at `now`, mixing colours
    /// with `rgb`.
    fn count_fade<'a>(&self, now: Instant, rgb: Option<&'a Rgb>) -> Option<Fade<'a>> {
        let at = self.counted?;
        Some(Fade {
            elapsed: now.saturating_duration_since(at),
            rgb,
        })
    }

    /// The fade under way at `now`, mixing colours with `rgb`.
    fn fade<'a>(&self, now: Instant, rgb: Option<&'a Rgb>) -> Option<Fade<'a>> {
        let jump = self.jumped?;
        Some(Fade {
            elapsed: now.saturating_duration_since(jump.at),
            rgb,
        })
    }

    /// `lines`, from line `first` of the document on, with the matches of
    /// the last search marked with `marks` while it is shown, the current
    /// one faded by `fade`, and the lines of `tint` tinted.
    fn marked(
        &self,
        lines: &[Line],
        first: usize,
        marks: &Marks,
        fade: Option<Fade>,
        tint: Option<Tint>,
    ) -> Vec<Line> {
        let search = self.search.as_ref().filter(|_| self.shown);
        (first..)
            .zip(lines)
            .map(|(index, line)| {
                let tinted;
                let line = match &tint {
                    Some(tint) if tint.lines.contains(&index) => {
                        tinted = tint::tint(line, tint.rgb, tint.amount, tint.columns);
                        &tinted
                    }
                    _ => line,
                };
                match search {
                    Some(search) => search::mark(line, &search.on(index), marks, fade),
                    None => line.clone(),
                }
            })
            .collect()
    }

    /// What the status line tells about the last key, in `fading` when
    /// it is given, or else about the last search, with the match number
    /// in `flash` when it is given.
    fn found(&self, flash: Option<Style>, fading: Option<Style>) -> Option<status::Found<'_>> {
        if let Some(told) = &self.told {
            return Some(status::Found::Told {
                message: &told.message,
                fading,
            });
        }
        let search = self.search.as_ref()?;
        if self.nowhere {
            return Some(status::Found::Nowhere(search.term()));
        }
        if !self.shown {
            return None;
        }
        let (current, total) = search.count()?;
        Some(status::Found::At {
            term: search.term(),
            current,
            total,
            flash,
        })
    }
}

/// One layout of the document: its lines, how search matches in them are
/// marked, the view of them and the status line below it.
struct Layout<'a> {
    lines: &'a [Line],
    marks: &'a Marks,
    /// The colours a fade mixes, where they are known.
    rgb: Option<&'a Rgb>,
    /// The cells of the text area, which a tint spans.
    columns: usize,
    status: &'a Status<'a>,
    view: &'a mut View,
}

/// What the screen shows: the lines in view, the status line and the help
/// panel.
#[derive(PartialEq)]
struct Frame {
    lines: Vec<Line>,
    status: Vec<Span>,
    panel: Vec<Placed>,
    /// The warning line, drawn over the lines and under the panel.
    notice: Vec<Placed>,
}

impl Frame {
    /// Draws the frame above a status line on the row below `rows`.
    fn draw(&self, rows: usize, writer: &mut Writer, out: &mut impl Write) -> io::Result<()> {
        draw(
            &self.lines,
            rows,
            &self.status,
            &self.notice,
            &self.panel,
            writer,
            out,
        )
    }
}

impl Layout<'_> {
    /// What the screen shows in `state` at `now`.
    fn frame(&self, state: &State, now: Instant) -> Frame {
        let visible = self.view.visible();
        let fade = state.fade(now, self.rgb);
        let out = state.fade_out(now, self.rgb);
        Frame {
            lines: state.marked(
                &self.lines[visible.clone()],
                visible.start,
                self.marks,
                fade,
                self.rgb.and_then(|rgb| state.tint(now, rgb, self.columns)),
            ),
            status: self
                .status
                .spans(self.view, state, fade, out, state.count_fade(now, self.rgb)),
            panel: self.status.panel(self.view, state.help),
            notice: self.status.notice(self.view, state, now, self.rgb),
        }
    }

    /// Whether something on the screen fades in `state` at `now`, so
    /// that the pager draws it a step at a time.
    fn fades(&self, state: &State, now: Instant) -> bool {
        state.jumped.is_some()
            || state.counted.is_some() && self.status.bar
            || self.status.bar
                && self.rgb.is_some()
                && (state.noticed)
                    .is_some_and(|at| now.saturating_duration_since(at) >= WARNING_STAY)
            || state.changed.is_some() && self.rgb.is_some()
            || self.status.bar
                && (state.fade_out(now, self.rgb))
                    .and_then(|out| self.status.styles.fading(out))
                    .is_some()
    }
}

/// Shows `layout` in `state` on `terminal`, whose window is of `size`, and
/// scrolls it until the reader quits or the window changes size, until
/// the document is to be read again from `source`, if there is one to
/// read it from, or until images loading there come.
fn show(
    layout: &mut Layout,
    state: &mut State,
    writer: &mut Writer,
    terminal: &mut impl Terminal,
    size: Option<WindowSize>,
    source: &mut Origin,
) -> io::Result<Outcome> {
    let reload = &mut source.reload;
    let mut now = terminal.now();
    let mut shown = layout.frame(state, now);
    shown.draw(layout.view.rows(), writer, terminal)?;
    terminal.flush()?;
    let mut decoder = keys::Decoder::default();
    loop {
        let input = terminal.read(if layout.fades(state, now) || writer.has_bands() {
            FRAME
        } else {
            POLL
        })?;
        // Nothing more came for a key the last read cut off.
        let keys = if input.is_empty() {
            decoder.flush()
        } else {
            decoder.keys(&input)
        };
        now = terminal.now();
        state.tick(now);
        for key in keys {
            match state.press(key, now, layout) {
                Pressed::Nothing => {}
                Pressed::Quit => return Ok(Outcome::Quit),
                Pressed::Reload if reload.is_none() => {
                    state.told = Some(Told::new("nothing to reload"));
                }
                // The reader sees that the pager is busy before it is.
                Pressed::Reload => {
                    state.told = Some(Told::new("reloading…"));
                    layout
                        .frame(state, now)
                        .draw(layout.view.rows(), writer, terminal)?;
                    terminal.flush()?;
                    return Ok(Outcome::Reload);
                }
                Pressed::Follow if reload.is_none() => {
                    state.told = Some(Told::new(NOTHING_TO_FOLLOW));
                }
                Pressed::Follow => {
                    state.follow = Follow {
                        mode: match state.follow.mode {
                            config::Follow::Off => config::Follow::On,
                            config::Follow::On => config::Follow::Auto,
                            config::Follow::Auto => config::Follow::Off,
                        },
                        ..Follow::default()
                    }
                }
            }
            state.moved(layout.view.top());
        }
        if state.follow.on()
            && let Some(source) = reload.as_deref_mut()
        {
            match state.follow.poll(source.change(), now) {
                Polled::Nothing => {}
                Polled::Reload => return Ok(Outcome::Reload),
                Polled::Gone(why) => state.told = Some(Told::new(why)),
            }
        }
        let came = source.loads.poll();
        if !came.is_empty() {
            return Ok(Outcome::Loaded(came));
        }
        let window = terminal.size();
        if window != size {
            return Ok(Outcome::Resized(window));
        }
        let frame = layout.frame(state, now);
        if frame != shown {
            shown = frame;
            shown.draw(layout.view.rows(), writer, terminal)?;
            terminal.flush()?;
        }
        if writer.has_bands() {
            writer.animate(now.saturating_duration_since(source.started), terminal)?;
            terminal.flush()?;
        }
    }
}

/// Draws `lines` from the top of the screen, with the rows below them
/// empty, `status` on the row below the view's `rows`, `notice` over the
/// lines and `panel` over them, which it dims, as one update so the screen
/// does not flicker. The images the writer's encoder no longer keeps are
/// deleted in the same update.
pub fn draw(
    lines: &[Line],
    rows: usize,
    status: &[Span],
    notice: &[Placed],
    panel: &[Placed],
    writer: &mut Writer,
    out: &mut impl Write,
) -> io::Result<()> {
    output::begin_update(out)?;
    writer.delete_unused(out)?;
    writer.clear(out)?;
    writer.dim(!panel.is_empty());
    let mut top = 1;
    for line in lines {
        for row in 0..writer.rows(line) {
            output::move_to(top + usize::from(row), 1, out)?;
            writer.row(line, row, out)?;
        }
        top += usize::from(writer.rows(line));
    }
    writer.dim(false);
    if !status.is_empty() {
        output::move_to(rows + 1, 1, out)?;
        writer.bar(status, out)?;
    }
    for placed in notice {
        output::move_to(placed.row, placed.column, out)?;
        writer.bar(&placed.spans, out)?;
    }
    for placed in panel {
        output::move_to(placed.row, placed.column, out)?;
        writer.bar(&placed.spans, out)?;
    }
    output::end_update(out)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::color::Color;
    use crate::model::{Document, File};

    #[test]
    fn auto_pages_a_document_longer_than_the_window() {
        assert!(pages(Paging::Auto, true, 25, Some(24)));
        assert!(!pages(Paging::Auto, true, 24, Some(24)));
        // Without a height, nothing tells that it is longer.
        assert!(!pages(Paging::Auto, true, 1000, None));
    }

    #[test]
    fn always_pages_even_a_short_document() {
        assert!(pages(Paging::Always, true, 1, Some(24)));
        assert!(pages(Paging::Always, true, 1, None));
    }

    #[test]
    fn nothing_is_paged_into_a_pipe_or_with_never() {
        for paging in [Paging::Always, Paging::Auto, Paging::Never] {
            assert!(!pages(paging, false, 100, Some(24)), "{paging:?}");
        }
        assert!(!pages(Paging::Never, true, 100, Some(24)));
    }

    #[test]
    fn standard_input_is_paged_only_when_paging_is_always() {
        assert_eq!(only_always(Paging::Always), Paging::Always);
        assert_eq!(only_always(Paging::Auto), Paging::Never);
        assert_eq!(only_always(Paging::Never), Paging::Never);
    }

    #[test]
    fn files_of_images_alone_are_paged_only_when_paging_is_always() {
        assert_eq!(for_files(Paging::Always, true), Paging::Always);
        assert_eq!(for_files(Paging::Auto, true), Paging::Never);
        assert_eq!(for_files(Paging::Never, true), Paging::Never);
    }

    #[test]
    fn files_with_a_document_among_them_follow_the_setting() {
        for paging in [Paging::Always, Paging::Auto, Paging::Never] {
            assert_eq!(for_files(paging, false), paging);
        }
    }

    /// A terminal that gives the reads in `keys`, one a call, with the
    /// window sizes in `sizes` after each read, and keeps what is written.
    struct Fake {
        keys: VecDeque<Vec<u8>>,
        sizes: VecDeque<WindowSize>,
        size: WindowSize,
        out: Vec<u8>,
        /// The time now, which each read moves on by a poll.
        clock: Instant,
        /// How long each read was asked to wait.
        waits: Vec<Duration>,
        /// How long a read that gives keys takes, when not as long as it
        /// was asked to wait.
        typing: Option<Duration>,
        /// Called with the number of each read, counting from 0, before
        /// it gives its keys.
        hook: Option<Box<dyn FnMut(usize)>>,
    }

    impl Fake {
        fn new(columns: u16, rows: u16, keys: &[&[u8]]) -> Self {
            Self {
                keys: keys.iter().map(|keys| keys.to_vec()).collect(),
                sizes: VecDeque::new(),
                size: window(columns, rows),
                out: Vec::new(),
                clock: Instant::now(),
                waits: Vec::new(),
                typing: None,
                hook: None,
            }
        }

        /// The frames drawn, as text.
        fn frames(&self) -> Vec<String> {
            let out = String::from_utf8(self.out.clone()).unwrap();
            out.split("\x1b[?2026h")
                .skip(1)
                .map(str::to_owned)
                .collect()
        }
    }

    fn window(columns: u16, rows: u16) -> WindowSize {
        WindowSize {
            columns,
            rows,
            width: 0,
            height: 0,
        }
    }

    impl Write for Fake {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.out.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Terminal for Fake {
        fn size(&self) -> Option<WindowSize> {
            Some(self.size)
        }

        fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
            if let Some(hook) = &mut self.hook {
                hook(self.waits.len());
            }
            self.waits.push(timeout);
            if let Some(size) = self.sizes.pop_front() {
                self.size = size;
            }
            // Out of keys, the reader quits, with Ctrl+C, which also
            // closes a prompt left open.
            let keys = self.keys.pop_front().unwrap_or_else(|| b"\x03".to_vec());
            self.clock += match self.typing {
                Some(typing) if !keys.is_empty() => typing,
                _ => timeout,
            };
            Ok(keys)
        }

        fn now(&self) -> Instant {
            self.clock
        }
    }

    fn numbers(count: usize) -> Document {
        let source: String = (1..=count).map(|n| format!("Line {n}\n\n")).collect();
        crate::parse::parse(&source)
    }

    fn show(document: &Document, terminal: &mut Fake) {
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        run(
            Content::Document(document),
            None,
            "notes.md",
            &mut inputs,
            Options::new(SearchCase::Smart),
            terminal,
        )
        .unwrap();
    }

    /// `document` shown with its lines laid out beforehand from `before`,
    /// with `caps`.
    fn show_laid_out(document: &Document, before: &Document, caps: Capabilities) -> Vec<String> {
        let mut terminal = Fake::new(40, 6, &[b"q"]);
        let mut inputs = Inputs::new(caps);
        let stream = crate::render::layout::content(Content::Document(before), &inputs);
        run(
            Content::Document(document),
            Some(stream),
            "notes.md",
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        )
        .unwrap();
        terminal.frames()
    }

    /// What the pager gets when it asks for the document again: the
    /// outcomes in `then`, one a call, and how many times it asked; and
    /// when it asks whether the document changed, the answers in
    /// `changes`, one a call, and unchanged after them.
    struct Then {
        then: VecDeque<Reloaded>,
        asked: usize,
        changes: VecDeque<Change>,
    }

    impl Then {
        fn new(then: impl IntoIterator<Item = Reloaded>) -> Self {
            Self {
                then: then.into_iter().collect(),
                asked: 0,
                changes: VecDeque::new(),
            }
        }

        /// As [`Then::new`], answering `changes` when asked whether the
        /// document changed.
        fn changing(
            then: impl IntoIterator<Item = Reloaded>,
            changes: impl IntoIterator<Item = Change>,
        ) -> Self {
            Self {
                changes: changes.into_iter().collect(),
                ..Self::new(then)
            }
        }
    }

    impl Reload for Then {
        fn reload(&mut self, _: &mut Images, _: &mut Loads, _: &Capabilities) -> Reloaded {
            self.asked += 1;
            self.then.pop_front().expect("an outcome for each reload")
        }

        fn change(&mut self) -> Change {
            self.changes.pop_front().unwrap_or(Change::Unchanged)
        }
    }

    /// The look of a version of a document, told apart by `length`.
    fn look(length: u64) -> Change {
        Change::Changed(Look {
            modified: None,
            length,
        })
    }

    /// `document` followed with `then` to reload it, in a window of
    /// 60 × 6, with `reads` and then `empty` polls without keys.
    fn following(
        document: &Document,
        then: &mut Then,
        reads: &[&[u8]],
        empty: usize,
    ) -> Vec<String> {
        let mut reads = reads.to_vec();
        reads.extend(std::iter::repeat_n(b"".as_slice(), empty));
        let mut terminal = Fake::new(60, 6, &reads);
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        run(
            Content::Document(document),
            None,
            "notes.md",
            &mut inputs,
            Options {
                reload: Some(then),
                follow: config::Follow::On,
                ..Options::new(SearchCase::Smart)
            },
            &mut terminal,
        )
        .unwrap();
        terminal.frames()
    }

    #[test]
    fn following_reloads_a_change_without_a_key_and_keeps_the_block_at_the_top() {
        // The read of the keys, then a change seen on two polls in a row.
        let changes = [Change::Unchanged, look(1), look(1)];
        let mut then = Then::changing([Reloaded::Document(rows(10))], changes);
        let frames = following(&numbers(10), &mut then, &[b"jjjjjj"], 5);
        assert_eq!(then.asked, 1, "{frames:#?}");
        assert_eq!(frames.len(), 3, "{frames:#?}");
        assert!(at_top(&frames[1], "Line 4"), "{frames:#?}");
        assert!(at_top(&frames[2], "Row 4"), "{frames:#?}");
        assert!(
            status_row(&frames[2], 6).contains("reloaded"),
            "{frames:#?}"
        );
    }

    /// `document` followed as `follow` says, jumping to the change `jump`
    /// chooses, with `then` to reload it, in a window of 60 × `rows`, with
    /// `reads` and then `empty` polls without keys.
    /// `document` followed as `follow` says, jumping to the change `jump`
    /// chooses, with `then` to reload it, in a window of 60 × `rows`, with
    /// `reads` and then `empty` polls without keys.
    fn following_in(
        rows: u16,
        how: (config::Follow, FollowJump),
        document: &Document,
        then: &mut Then,
        reads: &[&[u8]],
        empty: usize,
    ) -> Vec<String> {
        let caps = Capabilities::ansi(80);
        following_on(caps, rows, how, document, then, reads, empty).frames()
    }

    /// As [`following_in`], on a terminal with `caps`, which is returned
    /// for what it was asked and what it drew.
    fn following_on(
        caps: Capabilities,
        rows: u16,
        (follow, jump): (config::Follow, FollowJump),
        document: &Document,
        then: &mut Then,
        reads: &[&[u8]],
        empty: usize,
    ) -> Fake {
        let mut reads = reads.to_vec();
        reads.extend(std::iter::repeat_n(b"".as_slice(), empty));
        let mut terminal = Fake::new(60, rows, &reads);
        let mut inputs = Inputs::new(caps);
        run(
            Content::Document(document),
            None,
            "notes.md",
            &mut inputs,
            Options {
                reload: Some(then),
                follow,
                jump,
                ..Options::new(SearchCase::Smart)
            },
            &mut terminal,
        )
        .unwrap();
        terminal
    }

    /// Following on auto, jumping to the first change.
    const AUTO: (config::Follow, FollowJump) = (config::Follow::Auto, FollowJump::First);

    /// The row of `frame` that shows `text`, counted from 1, of the
    /// window's `rows`.
    fn row_of(frame: &str, rows: u16, text: &str) -> Option<u16> {
        let frame = unstyled(frame);
        (1..rows).find(|row| {
            let start = format!("\x1b[{row};1H");
            let end = format!("\x1b[{};1H", row + 1);
            frame
                .split(&start)
                .nth(1)
                .and_then(|rest| rest.split(&end).next())
                .is_some_and(|shown| shown.contains(text))
        })
    }

    /// `numbers(count)` with each line of `changed` written anew.
    fn numbers_changed(count: usize, changed: &[usize]) -> Document {
        let source: String = (1..=count)
            .map(|n| match changed.contains(&n) {
                true => format!("Line {n} changed\n\n"),
                false => format!("Line {n}\n\n"),
            })
            .collect();
        crate::parse::parse(&source)
    }

    #[test]
    fn auto_following_moves_the_view_to_a_change_below_it() {
        // A change seen on two polls, to Line 30, far below the view.
        let changes = [look(1), look(1)];
        let new = numbers_changed(40, &[30]);
        let mut then = Then::changing([Reloaded::Document(new)], changes);
        let frames = following_in(21, AUTO, &numbers(40), &mut then, &[], 3);
        assert_eq!(then.asked, 1, "{frames:#?}");
        assert!(at_top(&frames[0], "Line 1"), "{frames:#?}");
        let last = frames.last().unwrap();
        // Three rows of what comes before it above it.
        assert_eq!(row_of(last, 21, "Line 30 changed"), Some(4), "{frames:#?}");
        assert_eq!(row_of(last, 21, "Line 29"), Some(2), "{frames:#?}");
    }

    /// The SGR codes before `text` in the row of `frame` that shows it.
    fn row_style(frame: &str, text: &str) -> Vec<String> {
        style_before(document_part(frame, 21), text)
    }

    /// The background of the SGR codes `codes`, as `r;g;b`.
    fn background(codes: &[String]) -> Option<String> {
        let at = codes
            .windows(2)
            .position(|pair| pair[0] == "48" && pair[1] == "2")?;
        Some(codes[at + 2..at + 5].join(";"))
    }

    #[test]
    fn auto_following_tints_the_change_it_went_to_and_fades_the_tint_out() {
        let new = numbers_changed(40, &[30]);
        let mut then = Then::new([Reloaded::Document(new)]);
        let terminal = following_on(
            known_colours(),
            21,
            AUTO,
            &numbers(40),
            &mut then,
            &[b"r"],
            90,
        );
        let frames = terminal.frames();
        let tint = |frame: &str, text: &str| background(&row_style(frame, text));
        // Grey text on a black page: 15 % of 200 is 30; a line on the page has
        // no background of its own.
        let first = &frames[2];
        assert_eq!(
            tint(first, "Line 30 changed"),
            Some("30;30;30".to_owned()),
            "{frames:#?}"
        );
        // Only the change: the line above is on the page.
        assert_eq!(tint(first, "Line 29"), None, "{frames:#?}");
        // The tint reaches the end of the row.
        let row = unstyled(first);
        let row = row.split("\x1b[4;1H").nth(1).unwrap();
        let row = row.split("\x1b[5;1H").next().unwrap();
        assert_eq!(row.chars().count(), 60, "{row:?}");
        // A second later it begins to fade, and is gone after two more.
        let tints: Vec<_> = frames[2..]
            .iter()
            .map(|frame| tint(frame, "Line 30 changed"))
            .collect();
        let levels: Vec<u8> = tints
            .iter()
            .map(|tint| {
                tint.as_ref()
                    .map_or(0, |tint| tint.split(';').next().unwrap().parse().unwrap())
            })
            .collect();
        assert!(
            levels.windows(2).all(|pair| pair[0] >= pair[1]),
            "{levels:?}"
        );
        assert_eq!(levels[0], 30);
        assert_eq!(*levels.last().unwrap(), 0);
        // A step of the fade every frame for the three seconds from the
        // reload's read, and polls again after them.
        let waits = &terminal.waits[1..];
        let frames = waits.iter().take_while(|&&wait| wait == FRAME).count();
        assert_eq!(frames, 60, "{waits:?}");
        assert_eq!(waits[frames], POLL, "{waits:?}");
    }

    /// The frames drawn while following `old` as `how` says, reloaded to
    /// `new` by `r`, on a terminal with `caps`, with `empty` polls after it.
    fn reloaded_to(
        caps: Capabilities,
        how: (config::Follow, FollowJump),
        (old, new): (&Document, Document),
        reads: &[&[u8]],
        empty: usize,
    ) -> Fake {
        let mut then = Then::new([Reloaded::Document(new)]);
        following_on(caps, 21, how, old, &mut then, reads, empty)
    }

    #[test]
    fn a_change_already_in_view_is_tinted_too() {
        let new = numbers_changed(40, &[8]);
        let terminal = reloaded_to(known_colours(), AUTO, (&numbers(40), new), &[b"r"], 3);
        let frames = terminal.frames();
        assert!(at_top(&frames[2], "Line 1"), "{frames:#?}");
        let tint = background(&row_style(&frames[2], "Line 8 changed"));
        assert_eq!(tint, Some("30;30;30".to_owned()), "{frames:#?}");
    }

    #[test]
    fn a_removal_is_not_tinted_and_nothing_fades() {
        let terminal = reloaded_to(
            known_colours(),
            AUTO,
            (&numbers(40), numbers(39)),
            &[b"r"],
            90,
        );
        let frames = terminal.frames();
        for frame in &frames {
            assert!(!document_part(frame, 21).contains("48;2"), "{frame:?}");
        }
        assert!(
            !terminal.waits[1..5].contains(&FRAME),
            "{:?}",
            terminal.waits
        );
    }

    #[test]
    fn only_auto_following_tints_a_change() {
        let on = (config::Follow::On, FollowJump::First);
        let new = numbers_changed(40, &[8]);
        let terminal = reloaded_to(known_colours(), on, (&numbers(40), new), &[b"r"], 3);
        for frame in terminal.frames() {
            assert!(!document_part(&frame, 21).contains("48;2"), "{frame:?}");
        }
    }

    #[test]
    fn without_colours_known_as_rgb_nothing_is_tinted() {
        let new = numbers_changed(40, &[8]);
        let terminal = reloaded_to(
            Capabilities::ansi(80),
            AUTO,
            (&numbers(40), new),
            &[b"r"],
            3,
        );
        for frame in terminal.frames() {
            assert!(!document_part(&frame, 21).contains("48;2"), "{frame:?}");
        }
        // Nor does the pager wait a frame for it.
        assert!(
            !terminal.waits[1..5].contains(&FRAME),
            "{:?}",
            terminal.waits
        );
    }

    #[test]
    fn the_tint_stays_on_the_same_lines_when_the_view_scrolls() {
        let new = numbers_changed(40, &[30]);
        let terminal = reloaded_to(
            known_colours(),
            AUTO,
            (&numbers(40), new),
            &[b"r", b"kk"],
            3,
        );
        let frames = terminal.frames();
        let last = frames.last().unwrap();
        // Two rows up, the change is two rows lower than it was.
        assert_eq!(row_of(last, 21, "Line 30 changed"), Some(6), "{frames:#?}");
        assert_eq!(
            background(&row_style(last, "Line 30 changed")),
            Some("30;30;30".to_owned()),
            "{frames:#?}"
        );
        assert_eq!(background(&row_style(last, "Line 31")), None, "{frames:#?}");
    }

    #[test]
    fn a_new_reload_replaces_the_tint_still_fading() {
        let first = numbers_changed(40, &[5]);
        let second = numbers_changed(40, &[5, 7]);
        let mut then = Then::new([Reloaded::Document(first), Reloaded::Document(second)]);
        // The second reload comes half a second after the first.
        let waits = [b"".as_slice(); 9];
        let reads = [&[b"r".as_slice()][..], &waits, &[b"r"]].concat();
        let terminal = following_on(
            known_colours(),
            21,
            AUTO,
            &numbers(40),
            &mut then,
            &reads,
            3,
        );
        let frames = terminal.frames();
        let last = frames.last().unwrap();
        // Line 5 is the same as it was, and not the change of this reload.
        assert_eq!(
            background(&row_style(last, "Line 7 changed")),
            Some("30;30;30".to_owned()),
            "{frames:#?}"
        );
        assert_eq!(
            background(&row_style(last, "Line 5 changed")),
            None,
            "{frames:#?}"
        );
    }

    #[test]
    fn a_tint_is_mixed_over_the_background_a_line_has() {
        let old = crate::parse::parse("Text.\n\n```\ncode\n```\n");
        let new = crate::parse::parse("Text.\n\n```\ncode changed\n```\n");
        let terminal = reloaded_to(known_colours(), AUTO, (&old, new), &[b"r"], 3);
        let frames = terminal.frames();
        let before = background(&row_style(&frames[0], "code"));
        let after = background(&row_style(&frames[2], "code changed"));
        let level = |bg: &Option<String>| -> u8 {
            bg.as_ref()
                .map_or(0, |bg| bg.split(';').next().unwrap().parse().unwrap())
        };
        // The code block's own background, lighter by the tint.
        assert!(before.is_some(), "{frames:#?}");
        assert!(level(&after) > level(&before), "{before:?} {after:?}");
    }

    #[test]
    fn r_moves_to_a_change_while_following_is_auto() {
        let new = numbers_changed(40, &[30]);
        let mut then = Then::new([Reloaded::Document(new)]);
        let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"r"], 1);
        assert_eq!(then.asked, 1, "{frames:#?}");
        let last = frames.last().unwrap();
        assert_eq!(row_of(last, 21, "Line 30 changed"), Some(4), "{frames:#?}");
    }

    #[test]
    fn r_keeps_the_place_while_following_is_on() {
        let new = numbers_changed(40, &[30]);
        let mut then = Then::new([Reloaded::Document(new)]);
        let on = (config::Follow::On, FollowJump::First);
        let frames = following_in(21, on, &numbers(40), &mut then, &[b"r"], 1);
        assert_eq!(then.asked, 1, "{frames:#?}");
        assert!(at_top(frames.last().unwrap(), "Line 1"), "{frames:#?}");
    }

    #[test]
    fn auto_following_does_not_move_to_a_change_in_view() {
        let new = numbers_changed(40, &[8]);
        let mut then = Then::new([Reloaded::Document(new)]);
        let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"jj", b"r"], 1);
        let last = frames.last().unwrap();
        assert!(at_top(last, "Line 2"), "{frames:#?}");
        assert!(row_of(last, 21, "Line 8 changed").is_some(), "{frames:#?}");
    }

    #[test]
    fn auto_following_does_not_move_from_inside_a_change_taller_than_the_window() {
        // Lines 10 to 25 changed: 31 lines in a view of 20 rows. The
        // reader is at Line 15, inside them.
        let changed: Vec<usize> = (10..=25).collect();
        let new = numbers_changed(40, &changed);
        let mut then = Then::new([Reloaded::Document(new)]);
        let down = [b"j".as_slice(); 28].concat();
        let frames = following_in(21, AUTO, &numbers(40), &mut then, &[&down, b"r"], 1);
        assert!(at_top(&frames[1], "Line 15"), "{frames:#?}");
        assert!(
            at_top(frames.last().unwrap(), "Line 15 changed"),
            "{frames:#?}"
        );
    }

    #[test]
    fn auto_following_does_not_move_when_the_start_of_a_change_is_shown() {
        // Lines 8 to 30 changed; Line 8 is on the first page.
        let changed: Vec<usize> = (8..=30).collect();
        let new = numbers_changed(40, &changed);
        let mut then = Then::new([Reloaded::Document(new)]);
        let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"r"], 1);
        assert!(at_top(frames.last().unwrap(), "Line 1"), "{frames:#?}");
    }

    #[test]
    fn auto_following_does_not_move_after_a_reload_that_changed_nothing() {
        let mut then = Then::new([Reloaded::Document(numbers(40))]);
        let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"jjjj", b"r"], 1);
        assert_eq!(then.asked, 1, "{frames:#?}");
        assert!(at_top(frames.last().unwrap(), "Line 3"), "{frames:#?}");
    }

    #[test]
    fn auto_following_moves_to_the_change_the_setting_chooses() {
        let long = "Line 30 changed into a paragraph long enough to take two \
                    lines or more of the window";
        let source: String = (1..=40)
            .map(|n| match n {
                15 => "Line 15 changed\n\n".to_owned(),
                30 => format!("{long}\n\n"),
                n => format!("Line {n}\n\n"),
            })
            .collect();
        for (jump, shown) in [
            (FollowJump::First, "Line 15 changed"),
            (FollowJump::Last, "Line 30 changed"),
            (FollowJump::Most, "Line 30 changed"),
        ] {
            let new = crate::parse::parse(&source);
            let mut then = Then::new([Reloaded::Document(new)]);
            let auto = (config::Follow::Auto, jump);
            let frames = following_in(21, auto, &numbers(40), &mut then, &[b"r"], 1);
            let last = frames.last().unwrap();
            assert_eq!(row_of(last, 21, shown), Some(4), "{jump:?}: {frames:#?}");
        }
        // Most is the first of changes that take as many rows.
        let new = numbers_changed(40, &[15, 30]);
        let mut then = Then::new([Reloaded::Document(new)]);
        let most = (config::Follow::Auto, FollowJump::Most);
        let frames = following_in(21, most, &numbers(40), &mut then, &[b"r"], 1);
        let last = frames.last().unwrap();
        assert_eq!(row_of(last, 21, "Line 15 changed"), Some(4), "{frames:#?}");
    }

    #[test]
    fn auto_following_moves_to_a_removal_as_the_setting_chooses() {
        // Line 15 changed and Line 30 removed.
        let source: String = (1..=40)
            .filter(|&n| n != 30)
            .map(|n| match n {
                15 => "Line 15 changed\n\n".to_owned(),
                n => format!("Line {n}\n\n"),
            })
            .collect();
        for (jump, shown) in [
            // The removal is the last change, at Line 31 now after it.
            (FollowJump::Last, "Line 31"),
            // A removal counts as no rows.
            (FollowJump::Most, "Line 15 changed"),
        ] {
            let new = crate::parse::parse(&source);
            let mut then = Then::new([Reloaded::Document(new)]);
            let auto = (config::Follow::Auto, jump);
            let frames = following_in(21, auto, &numbers(40), &mut then, &[b"r"], 1);
            let last = frames.last().unwrap();
            assert_eq!(row_of(last, 21, shown), Some(4), "{jump:?}: {frames:#?}");
        }
        // A removal alone is the change of the most rows.
        let source: String = (1..=40)
            .filter(|&n| n != 30)
            .map(|n| format!("Line {n}\n\n"))
            .collect();
        let mut then = Then::new([Reloaded::Document(crate::parse::parse(&source))]);
        let most = (config::Follow::Auto, FollowJump::Most);
        let frames = following_in(21, most, &numbers(40), &mut then, &[b"r"], 1);
        let last = frames.last().unwrap();
        assert_eq!(row_of(last, 21, "Line 31"), Some(4), "{frames:#?}");
    }

    #[test]
    fn auto_following_shows_a_change_at_the_end_on_the_last_page() {
        let new = numbers_changed(40, &[40]);
        let mut then = Then::new([Reloaded::Document(new)]);
        let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"r"], 1);
        let last = frames.last().unwrap();
        assert_eq!(row_of(last, 21, "Line 40 changed"), Some(20), "{frames:#?}");
        // A removal at the end, too.
        let mut then = Then::new([Reloaded::Document(numbers(39))]);
        let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"r"], 1);
        let last = frames.last().unwrap();
        assert_eq!(row_of(last, 21, "Line 39"), Some(20), "{frames:#?}");
    }

    /// Whether the status line of `frame`, in a window of 6 rows, tells
    /// that the pager follows the document.
    fn follows(frame: &str) -> bool {
        status_row(frame, 6).contains(" follow   ")
    }

    /// Whether the status line of `frame`, in a window of 6 rows, tells
    /// that the pager follows the document and moves to a change.
    fn follows_auto(frame: &str) -> bool {
        status_row(frame, 6).contains(" auto   ")
    }

    #[test]
    fn capital_f_goes_from_off_to_on_to_auto_and_back_to_off() {
        // F, a change seen on two polls, F to auto, where the next change
        // is seen once, and F to off, where the last one is not asked
        // about.
        let changes = [look(1), look(1), look(2), look(3)];
        let mut then = Then::changing([Reloaded::Document(rows(10))], changes);
        let reads: &[&[u8]] = &[b"F", b"", b"F", b"F", b""];
        let frames = reloading_with(Capabilities::ansi(80), &numbers(10), &mut then, reads, 0);
        assert_eq!(then.asked, 1, "{frames:#?}");
        assert_eq!(then.changes.len(), 1, "{frames:#?}");
        assert_eq!(frames.len(), 5, "{frames:#?}");
        assert!(!follows(&frames[0]), "{frames:#?}");
        assert!(!follows_auto(&frames[0]), "{frames:#?}");
        assert!(follows(&frames[1]), "{frames:#?}");
        assert!(at_top(&frames[2], "Row 1"), "{frames:#?}");
        assert!(follows(&frames[2]), "{frames:#?}");
        assert!(follows_auto(&frames[3]), "{frames:#?}");
        assert!(!follows(&frames[3]), "{frames:#?}");
        assert!(!follows(&frames[4]), "{frames:#?}");
        assert!(!follows_auto(&frames[4]), "{frames:#?}");
    }

    #[test]
    fn the_status_line_tells_following_from_the_start() {
        let mut then = Then::new([]);
        let frames = following(&numbers(10), &mut then, &[], 0);
        assert!(follows(&frames[0]), "{frames:#?}");
    }

    #[test]
    fn without_a_file_to_follow_capital_f_says_so_until_the_next_key() {
        let mut terminal = Fake::new(60, 6, &[b"F", b"j"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 3, "{frames:#?}");
        assert!(status_row(&frames[1], 6).contains("nothing to follow"));
        assert!(!status_row(&frames[2], 6).contains("follow"), "{frames:#?}");
    }

    #[test]
    fn following_from_the_start_without_a_file_says_there_is_nothing_to_follow() {
        let mut terminal = Fake::new(60, 6, &[b"j"]);
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        let options = Options {
            follow: config::Follow::On,
            ..Options::new(SearchCase::Smart)
        };
        let document = numbers(10);
        run(
            Content::Document(&document),
            None,
            "notes.md",
            &mut inputs,
            options,
            &mut terminal,
        )
        .unwrap();
        let frames = terminal.frames();
        assert_eq!(frames.len(), 2, "{frames:#?}");
        assert!(status_row(&frames[0], 6).contains("nothing to follow"));
        assert!(!status_row(&frames[1], 6).contains("follow"), "{frames:#?}");
    }

    #[test]
    fn following_waits_a_poll_s_time_for_a_change_to_look_the_same_while_keys_come_fast() {
        // A change seen on the reads of two keys 5 ms apart, and then on
        // a poll without keys.
        let changes = [look(1), look(1), look(1)];
        let mut then = Then::changing([Reloaded::Document(rows(10))], changes);
        let mut terminal = Fake::new(60, 6, &[b"j", b"j", b""]);
        terminal.typing = Some(Duration::from_millis(5));
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        let document = numbers(10);
        let options = Options {
            reload: Some(&mut then),
            follow: config::Follow::On,
            ..Options::new(SearchCase::Smart)
        };
        run(
            Content::Document(&document),
            None,
            "notes.md",
            &mut inputs,
            options,
            &mut terminal,
        )
        .unwrap();
        let frames = terminal.frames();
        assert_eq!(then.asked, 1, "{frames:#?}");
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert!(at_top(&frames[2], "Line 2"), "{frames:#?}");
        assert!(at_top(&frames[3], "Row 2"), "{frames:#?}");
    }

    #[test]
    fn following_waits_until_a_change_looks_the_same_on_two_polls() {
        // Still being written on the second poll, then done.
        let changes = [look(1), look(2), look(2)];
        let mut then = Then::changing([Reloaded::Document(rows(10))], changes);
        let frames = following(&numbers(10), &mut then, &[], 5);
        assert_eq!(then.asked, 1, "{frames:#?}");
        assert_eq!(frames.len(), 2, "{frames:#?}");
        assert!(at_top(&frames[1], "Row 1"), "{frames:#?}");
        // A change that is not seen again is not read.
        let changes = [look(1), Change::Unchanged, look(2)];
        let mut then = Then::changing([], changes);
        let frames = following(&numbers(10), &mut then, &[], 5);
        assert_eq!(then.asked, 0, "{frames:#?}");
    }

    #[test]
    fn without_following_a_change_is_not_read() {
        let changes = [look(1), look(1), look(1)];
        let mut then = Then::changing([], changes);
        let frames = reloading_with(Capabilities::ansi(80), &numbers(10), &mut then, &[], 5);
        assert_eq!(then.asked, 0, "{frames:#?}");
        assert_eq!(then.changes.len(), 3, "{frames:#?}");
    }

    #[test]
    fn following_a_document_that_is_gone_keeps_it_and_tells_why_once() {
        let gone = || Change::Gone("No such file or directory".to_owned());
        let mut then = Then::changing([], [gone(), gone(), gone(), gone()]);
        let frames = following(&numbers(10), &mut then, &[b"", b"", b"j"], 3);
        assert_eq!(then.asked, 0, "{frames:#?}");
        assert_eq!(frames.len(), 3, "{frames:#?}");
        assert!(at_top(&frames[1], "Line 1"), "{frames:#?}");
        assert!(
            status_row(&frames[1], 6).contains("No such file"),
            "{frames:#?}"
        );
        // A key clears it, and it is not told again while still gone.
        assert!(!status_row(&frames[2], 6).contains("No such file"));
    }

    #[test]
    fn following_reads_a_document_back_once_it_is_there_again() {
        let gone = Change::Gone("No such file or directory".to_owned());
        let changes = [gone, look(1), look(1)];
        let mut then = Then::changing([Reloaded::Document(rows(10))], changes);
        let frames = following(&numbers(10), &mut then, &[], 5);
        assert_eq!(then.asked, 1, "{frames:#?}");
        let last = frames.last().unwrap();
        assert!(at_top(last, "Row 1"), "{frames:#?}");
        assert!(status_row(last, 6).contains("reloaded"), "{frames:#?}");
    }

    #[test]
    fn following_tells_why_a_change_could_not_be_read() {
        let why = Reloaded::Failed("not UTF-8 text".to_owned());
        let mut then = Then::changing([why], [look(1), look(1)]);
        let frames = following(&numbers(10), &mut then, &[], 5);
        assert_eq!(then.asked, 1, "{frames:#?}");
        let last = frames.last().unwrap();
        assert!(at_top(last, "Line 1"), "{frames:#?}");
        assert!(
            status_row(last, 6).contains("not UTF-8 text"),
            "{frames:#?}"
        );
    }

    /// `document` shown with `then` to reload it, in a window of 60 × 6.
    fn show_reloading(document: &Document, then: &mut Then, keys: &[&[u8]]) -> Vec<String> {
        reloading_with(Capabilities::ansi(80), document, then, keys, 0)
    }

    /// As [`show_reloading`], with every read in `reads` as it is, empty
    /// ones included.
    fn show_reloading_with(document: &Document, then: &mut Then, reads: &[&[u8]]) -> Vec<String> {
        let mut terminal = Fake::new(60, 6, reads);
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        let options = Options {
            reload: Some(then),
            ..Options::new(SearchCase::Smart)
        };
        let content = Content::Document(document);
        run(
            content,
            None,
            "notes.md",
            &mut inputs,
            options,
            &mut terminal,
        )
        .unwrap();
        terminal.frames()
    }

    /// As [`show_reloading`], on a terminal with `caps`, with `empty`
    /// polls without keys after `reads`.
    fn reloading_with(
        caps: Capabilities,
        document: &Document,
        then: &mut Then,
        reads: &[&[u8]],
        empty: usize,
    ) -> Vec<String> {
        reloading_in(6, caps, document, then, reads, empty).frames()
    }

    /// As [`reloading_with`], in a window of 60 × `rows`, giving the
    /// terminal after the reader quit.
    fn reloading_in(
        rows: u16,
        caps: Capabilities,
        document: &Document,
        then: &mut Then,
        reads: &[&[u8]],
        empty: usize,
    ) -> Fake {
        let mut reads = reads.to_vec();
        reads.extend(std::iter::repeat_n(b"".as_slice(), empty));
        let mut terminal = Fake::new(60, rows, &reads);
        let mut inputs = Inputs::new(caps);
        run(
            Content::Document(document),
            None,
            "notes.md",
            &mut inputs,
            Options {
                reload: Some(then),
                ..Options::new(SearchCase::Smart)
            },
            &mut terminal,
        )
        .unwrap();
        terminal
    }

    fn rows(count: usize) -> Document {
        let source: String = (1..=count).map(|n| format!("Row {n}\n\n")).collect();
        crate::parse::parse(&source)
    }

    #[test]
    fn r_shows_the_document_read_again_and_tells_so_until_the_next_key() {
        let mut then = Then::new([Reloaded::Document(rows(10))]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"j"]);
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert_eq!(then.asked, 1);
        assert!(at_top(&frames[0], "Line 1"), "{frames:#?}");
        // The old document stays on the screen while it is reloaded.
        assert!(at_top(&frames[1], "Line 1"), "{frames:#?}");
        assert!(
            status_row(&frames[1], 6).contains("reloading"),
            "{frames:#?}"
        );
        assert!(at_top(&frames[2], "Row 1"), "{frames:#?}");
        assert!(
            status_row(&frames[2], 6).contains("reloaded"),
            "{frames:#?}"
        );
        assert!(
            !status_row(&frames[3], 6).contains("reloaded"),
            "{frames:#?}"
        );
    }

    #[test]
    fn a_reload_that_fails_keeps_the_old_document_and_tells_why_until_the_next_key() {
        let why = Reloaded::Failed("notes.md: No such file or directory".to_owned());
        let mut then = Then::new([why]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"j"]);
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert!(at_top(&frames[2], "Line 1"), "{frames:#?}");
        let status = status_row(&frames[2], 6);
        assert!(status.contains("No such file or directory"), "{frames:#?}");
        assert!(!status.contains("reloaded"), "{frames:#?}");
        assert!(
            !status_row(&frames[3], 6).contains("No such file"),
            "{frames:#?}"
        );
    }

    /// A warning at line `line` of the file.
    fn warning(line: usize, message: &str) -> Warning {
        Warning {
            line: Some(line),
            message: message.to_owned(),
        }
    }

    #[test]
    fn a_reload_with_warnings_says_reloaded_and_marks_how_many() {
        let warned = Reloaded::Warned(rows(10), vec![warning(3, "bad"), warning(9, "worse")]);
        let mut then = Then::new([warned]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"j"]);
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert!(at_top(&frames[2], "Row 1"), "{frames:#?}");
        let status = unstyled(&status_row(&frames[2], 6));
        assert!(status.contains("reloaded"), "{frames:#?}");
        assert!(status.contains("\u{26a0} 2"), "{frames:#?}");
        // The mark stays after the next key, and `reloaded` goes.
        let status = unstyled(&status_row(&frames[3], 6));
        assert!(status.contains("\u{26a0} 2"), "{frames:#?}");
        assert!(!status.contains("reloaded"), "{frames:#?}");
    }

    #[test]
    fn a_document_without_warnings_has_no_mark() {
        let mut then = Then::new([Reloaded::Document(rows(10))]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"j"]);
        assert!(!unstyled(&frames[2]).contains('\u{26a0}'), "{frames:#?}");
    }

    #[test]
    fn a_reload_without_warnings_takes_the_mark_away() {
        let warned = Reloaded::Warned(rows(10), vec![warning(3, "bad")]);
        let mut then = Then::new([warned, Reloaded::Document(rows(10))]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"r"]);
        let last = frames.len() - 1;
        let status = unstyled(&status_row(&frames[2], 6));
        assert!(status.contains("\u{26a0} 1"), "{frames:#?}");
        let status = unstyled(&status_row(&frames[last], 6));
        assert!(!status.contains('\u{26a0}'), "{frames:#?}");
        assert!(status.contains("reloaded"), "{frames:#?}");
    }

    #[test]
    fn the_number_of_the_mark_flashes_when_it_changes_and_not_when_it_stays() {
        // Without known colours the flash is bold for half a second.
        let flashing = |frame: &str| status_row(frame, 6).contains("\x1b[0;1m1");
        let warned = || Reloaded::Warned(rows(10), vec![warning(3, "bad")]);
        let mut then = Then::new([warned(), warned()]);
        let reads: Vec<&[u8]> = vec![b"r", b"", b"", b"", b"", b"", b"", b"", b"", b"", b"", b"r"];
        let frames = show_reloading_with(&numbers(10), &mut then, &reads);
        let first = frames.iter().position(|f| f.contains("\u{26a0}"));
        let first = first.unwrap_or(usize::MAX);
        assert!(first < frames.len(), "no mark: {frames:#?}");
        assert!(flashing(&frames[first]), "{frames:#?}");
        let calm = frames
            .iter()
            .position(|f| f.contains("\u{26a0}") && !flashing(f));
        let calm = calm.expect("the flash ends");
        assert!(calm > first, "{frames:#?}");
        // The second reload leaves the count as it was.
        let last = frames.last().unwrap();
        assert!(last.contains("\u{26a0}") && !flashing(last), "{frames:#?}");
    }

    /// The text a frame shows in the warning line, on row 5 of a window
    /// of 6 rows, with the column it starts at; `None` when the frame
    /// writes nothing there but the document's own row.
    fn warning_line(frame: &str) -> Option<(usize, String)> {
        let plain = unstyled(frame);
        let at = plain.rfind("\x1b[5;")?;
        let rest = &plain[at + 4..];
        let (column, text) = rest.split_once('H')?;
        let text = text.split("\x1b").next()?;
        // The document's own row 5 starts in column 1.
        (column != "1").then(|| (column.parse().unwrap(), text.to_owned()))
    }

    /// The red of the warning line in `frame` as RGB, which is where it
    /// has begun to fade.
    fn fade_red(frame: &str) -> Option<u8> {
        let line = &frame[frame.rfind("\x1b[5;")?..];
        let rgb = line.split_once("38;2;")?.1;
        rgb.split(';').next()?.parse().ok()
    }

    fn warned_with(warnings: Vec<Warning>) -> Reloaded {
        Reloaded::Warned(rows(10), warnings)
    }

    #[test]
    fn a_reload_with_warnings_shows_the_first_above_the_status_line_at_the_right_edge() {
        let mut then = Then::new([warned_with(vec![warning(3, "front matter: bad")])]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"r"]);
        let (column, text) = warning_line(&frames[2]).unwrap_or_default();
        assert_eq!(text, "\u{26a0} [:3] front matter: bad", "{frames:#?}");
        // Ends a cell before the edge, as the status line does.
        assert_eq!(column + text.chars().count(), 60, "{frames:#?}");
        // The document's row stays where it is, and the status line too.
        assert!(
            unstyled(&frames[2]).contains("\x1b[5;1H  Row 3"),
            "{frames:#?}"
        );
        assert!(
            status_row(&frames[2], 6).contains("reloaded"),
            "{frames:#?}"
        );
        // Red, as errors are drawn in a document, on the page.
        assert!(frames[2].contains("\x1b[0;31m\u{26a0}"), "{frames:#?}");
    }

    #[test]
    fn the_warning_line_leaves_out_the_line_when_it_is_not_known() {
        let warning = Warning {
            line: None,
            message: "no line".to_owned(),
        };
        let mut then = Then::new([warned_with(vec![warning])]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"r"]);
        let (_, text) = warning_line(&frames[2]).unwrap_or_default();
        assert_eq!(text, "\u{26a0} no line", "{frames:#?}");
    }

    #[test]
    fn the_warning_line_counts_the_other_warnings_in_italics() {
        let many = vec![
            warning(3, "first"),
            warning(4, "second"),
            warning(5, "third"),
        ];
        let mut then = Then::new([warned_with(many)]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"r"]);
        let (_, text) = warning_line(&frames[2]).unwrap_or_default();
        assert_eq!(
            text, "\u{26a0} [:3] first  and 2 more warnings\u{2026}",
            "{frames:#?}"
        );
        assert!(frames[2].contains("\x1b[0;2;3m  and 2"), "{frames:#?}");
        assert!(
            unstyled(&status_row(&frames[2], 6)).contains("\u{26a0} 3"),
            "{frames:#?}"
        );
    }

    #[test]
    fn a_key_does_not_cancel_the_warning_line() {
        let mut then = Then::new([warned_with(vec![warning(3, "bad")])]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"j"]);
        assert!(warning_line(&frames[2]).is_some(), "{frames:#?}");
        assert!(
            warning_line(frames.last().unwrap()).is_some(),
            "{frames:#?}"
        );
    }

    #[test]
    fn the_warning_line_stays_five_seconds_and_then_goes() {
        let mut then = Then::new([warned_with(vec![warning(3, "bad")])]);
        let terminal = reloading_in(
            6,
            Capabilities::ansi(80),
            &numbers(10),
            &mut then,
            &[b"r"],
            80,
        );
        let frames = terminal.frames();
        assert!(warning_line(&frames[2]).is_some(), "{frames:#?}");
        // Seen until the fade out ends, where it cannot fade.
        let last = frames.last().unwrap();
        assert!(warning_line(last).is_none(), "{last:?}");
        // The mark stays, as the document still has the warning.
        assert!(
            unstyled(&status_row(last, 6)).contains("\u{26a0} 1"),
            "{last:?}"
        );
    }

    #[test]
    fn the_warning_line_fades_into_the_page_where_colours_are_known() {
        let mut caps = Capabilities::kitty(80);
        caps.foreground = Some(Color::Rgb(200, 200, 200));
        caps.background = Some(Color::Rgb(0, 0, 0));
        caps.palette[1] = Some(Color::Rgb(200, 0, 0));
        let mut then = Then::new([warned_with(vec![warning(3, "bad")])]);
        let terminal = reloading_in(6, caps, &numbers(10), &mut then, &[b"r"], 90);
        let frames: Vec<String> = terminal
            .frames()
            .into_iter()
            .filter(|frame| warning_line(frame).is_some())
            .collect();
        assert!(frames.len() > 3, "no warning line: {frames:#?}");
        // Red while it stays; the palette's red goes as RGB once it fades.
        assert!(frames[0].contains("\x1b[0;31m\u{26a0}"), "{:?}", frames[0]);
        let reds: Vec<u8> = frames.iter().filter_map(|frame| fade_red(frame)).collect();
        assert!(reds.len() > 5, "{reds:?}\n{frames:#?}");
        assert!(reds[0] > 150 && reds.contains(&120), "{reds:?}");
        assert!(reds.windows(2).all(|pair| pair[1] <= pair[0]), "{reds:?}");
        assert!(*reds.last().unwrap() < 30, "{reds:?}");
        // Fading is drawn at frame rate, and only then.
        let fast = terminal.waits.iter().filter(|wait| **wait == FRAME).count();
        assert!(fast >= 10, "{:?}", terminal.waits);
    }

    #[test]
    fn a_new_reload_replaces_the_warning_line_or_takes_it_away() {
        let mut then = Then::new([
            warned_with(vec![warning(3, "first")]),
            warned_with(vec![warning(4, "second")]),
            Reloaded::Document(rows(10)),
        ]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"r", b"r", b"r"]);
        let lines: Vec<_> = frames
            .iter()
            .filter_map(|frame| warning_line(frame))
            .collect();
        assert!(
            lines.iter().any(|(_, text)| text.contains("first")),
            "{frames:#?}"
        );
        assert!(lines.last().unwrap().1.contains("second"), "{frames:#?}");
        assert!(
            warning_line(frames.last().unwrap()).is_none(),
            "{frames:#?}"
        );
    }

    #[test]
    fn a_window_without_a_row_above_the_status_line_has_no_warning_line() {
        let mut then = Then::new([warned_with(vec![warning(3, "bad")])]);
        let terminal = reloading_in(
            1,
            Capabilities::ansi(80),
            &numbers(10),
            &mut then,
            &[b"r"],
            0,
        );
        for frame in terminal.frames() {
            assert!(!unstyled(&frame).contains("bad"), "{frame:?}");
        }
    }

    #[test]
    fn warnings_given_at_the_start_show_the_mark_and_the_line_at_once() {
        let mut terminal = Fake::new(60, 6, &[b"j"]);
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        let document = numbers(10);
        let options = Options {
            warnings: vec![warning(3, "front matter: bad")],
            ..Options::new(SearchCase::Smart)
        };
        let content = Content::Document(&document);
        run(
            content,
            None,
            "notes.md",
            &mut inputs,
            options,
            &mut terminal,
        )
        .unwrap();
        let frames = terminal.frames();
        let (_, text) = warning_line(&frames[0]).unwrap_or_default();
        assert_eq!(text, "\u{26a0} [:3] front matter: bad", "{frames:#?}");
        assert!(
            unstyled(&status_row(&frames[0], 6)).contains("\u{26a0} 1"),
            "{frames:#?}"
        );
    }

    #[test]
    fn without_a_file_to_reload_r_says_so_until_the_next_key() {
        let mut terminal = Fake::new(60, 6, &[b"r", b"j"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 3, "{frames:#?}");
        let status = status_row(&frames[1], 6);
        assert!(status.contains("nothing to reload"), "{frames:#?}");
        assert!(!status_row(&frames[2], 6).contains("nothing to reload"));
    }

    #[test]
    fn reloaded_stays_four_seconds_then_fades_out_into_the_bar() {
        // A grey bar, mixed from the black page and this grey.
        let mut caps = known_colours();
        caps.palette[8] = Some(Color::Rgb(100, 100, 100));
        let mut then = Then::new([Reloaded::Document(rows(10))]);
        let frames = reloading_with(caps, &numbers(10), &mut then, &[b"r"], 200);
        // The reload, nothing new for four seconds, a frame each
        // twentieth of a second for the three seconds of the fade, and
        // the status line without the message after it.
        assert_eq!(frames.len(), 64, "{frames:#?}");
        let message = |frame: &str| style_before(&status_row(frame, 6), "reloaded");
        let colours = |fg: &str| {
            let mut codes = vec!["0".to_owned()];
            codes.extend(
                format!("38;2;{fg};48;2;50;50;50")
                    .split(';')
                    .map(str::to_owned),
            );
            codes
        };
        // In the bar's colours at first, the grey of the page's text on
        // dark grey, then into the dark grey.
        assert_eq!(message(&frames[2]), ["0", "48", "2", "50", "50", "50"]);
        assert_eq!(message(&frames[3]), colours("200;200;200"));
        assert_eq!(message(&frames[33]), colours("125;125;125"));
        assert_eq!(message(&frames[61]), colours("55;55;55"));
        assert!(
            !status_row(&frames[63], 6).contains("reloaded"),
            "{frames:#?}"
        );
    }

    #[test]
    fn reloaded_goes_at_once_after_seven_seconds_where_colours_are_not_known() {
        // Seven seconds are seventy polls; the reload's read is the first.
        let reloaded = |empty| {
            let mut then = Then::new([Reloaded::Document(rows(10))]);
            let caps = Capabilities::ansi(80);
            reloading_with(caps, &numbers(10), &mut then, &[b"r"], empty)
        };
        let frames = reloaded(69);
        assert_eq!(frames.len(), 3, "{frames:#?}");
        assert!(status_row(&frames[2], 6).contains("reloaded"));
        let frames = reloaded(70);
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert!(
            !status_row(&frames[3], 6).contains("reloaded"),
            "{frames:#?}"
        );
    }

    #[test]
    fn the_pager_waits_a_frame_only_while_reloaded_fades_on_a_shown_status_line() {
        let mut caps = known_colours();
        caps.palette[8] = Some(Color::Rgb(100, 100, 100));
        let waits = |height| {
            let mut then = Then::new([Reloaded::Document(rows(10))]);
            let caps = caps.clone();
            reloading_in(height, caps, &numbers(10), &mut then, &[b"r"], 120).waits
        };
        // The read of r, forty polls of the stay, sixty frames of the
        // fade, and polls again up to the read of Ctrl+C that quits.
        let mut expected = vec![POLL; 41];
        expected.extend([FRAME; 60]);
        expected.extend([POLL; 21]);
        assert_eq!(waits(6), expected);
        // A window of one row has no status line to fade.
        assert_eq!(waits(1), [POLL; 122]);
    }

    #[test]
    fn a_key_clears_reloaded_at_once_while_it_fades() {
        // Forty polls are the four seconds the message stays, and ten
        // frames half a second of its fade.
        let mut caps = known_colours();
        caps.palette[8] = Some(Color::Rgb(100, 100, 100));
        let mut then = Then::new([Reloaded::Document(rows(10))]);
        let mut reads: Vec<&[u8]> = vec![b"r"];
        reads.extend(std::iter::repeat_n(b"".as_slice(), 50));
        reads.push(b"j");
        let frames = reloading_with(caps, &numbers(10), &mut then, &reads, 0);
        let last = frames.last().unwrap();
        // The key moved the view and cleared the message in one frame.
        assert!(!at_top(last, "Row 1"), "{frames:#?}");
        assert!(!status_row(last, 6).contains("reloaded"), "{frames:#?}");
        // The frame before the key showed the message fading.
        let before = status_row(&frames[frames.len() - 2], 6);
        let fading = style_before(&before, "reloaded");
        assert_eq!(fading[..2], ["0", "38"], "{frames:#?}");
    }

    #[test]
    fn other_messages_stay_until_the_next_key() {
        let why = Reloaded::Failed("notes.md: No such file or directory".to_owned());
        let mut then = Then::new([why]);
        let frames = reloading_with(known_colours(), &numbers(10), &mut then, &[b"r"], 200);
        assert_eq!(frames.len(), 3, "{frames:#?}");
        assert!(status_row(&frames[2], 6).contains("No such file"));
        let frames = frames_with(known_colours(), &[b"r"], 200);
        assert_eq!(frames.len(), 2, "{frames:#?}");
        assert!(status_row(&frames[1], 6).contains("nothing to reload"));
    }

    #[test]
    fn a_reload_keeps_the_block_at_the_top() {
        let mut then = Then::new([Reloaded::Document(rows(10))]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"jjjjjj", b"r"]);
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert!(at_top(&frames[1], "Line 4"), "{frames:#?}");
        assert!(at_top(&frames[3], "Row 4"), "{frames:#?}");
    }

    #[test]
    fn a_reload_at_the_end_goes_to_the_new_end() {
        // Ten lines end with Line 8 at the top; twenty rows end with Row
        // 18 there, not Row 8, as a document that grew is followed.
        let mut then = Then::new([Reloaded::Document(rows(20))]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"G", b"r"]);
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert!(at_top(&frames[1], "Line 8"), "{frames:#?}");
        assert!(at_top(&frames[3], "Row 18"), "{frames:#?}");
    }

    #[test]
    fn a_reload_finds_the_search_again_in_the_new_lines() {
        // Line 3 and Line 30 match in 30 lines; Line 3 and Line 30 to
        // Line 35 do in 35.
        let mut then = Then::new([Reloaded::Document(numbers(35))]);
        let keys: &[&[u8]] = &[b"/Line 3\r", b"r", b"n"];
        let frames = show_reloading(&numbers(30), &mut then, keys);
        assert_eq!(frames.len(), 5, "{frames:#?}");
        assert!(
            status_row(&frames[1], 6).contains("/Line 3 1/2"),
            "{frames:#?}"
        );
        assert!(
            status_row(&frames[3], 6).contains("reloaded"),
            "{frames:#?}"
        );
        assert!(
            status_row(&frames[4], 6).contains("/Line 3 2/7"),
            "{frames:#?}"
        );
    }

    #[test]
    fn a_reload_keeps_the_place_when_a_block_is_added_above_it() {
        // Ten lines, scrolled to Line 4; then a block before all of them.
        let mut then = Then::new([Reloaded::Document(crate::parse::parse(&format!(
            "Added\n\n{}",
            (1..=10)
                .map(|n| format!("Line {n}\n\n"))
                .collect::<String>()
        )))]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"jjjjjj", b"r"]);
        assert!(at_top(&frames[1], "Line 4"), "{frames:#?}");
        assert!(at_top(&frames[3], "Line 4"), "{frames:#?}");
    }

    /// A document of `blocks`, one after the other.
    fn blocks(blocks: &[&str]) -> Document {
        crate::parse::parse(&blocks.join("\n\n"))
    }

    #[test]
    fn a_reload_goes_to_the_next_block_when_the_top_block_is_removed() {
        // Scrolled to Line 4; then Line 4 is gone and a block comes first.
        let old: Vec<String> = (1..=10).map(|n| format!("Line {n}")).collect();
        let mut new: Vec<&str> = vec!["Added"];
        new.extend(
            old.iter()
                .map(String::as_str)
                .filter(|&line| line != "Line 4"),
        );
        let mut then = Then::new([Reloaded::Document(blocks(&new))]);
        let frames = show_reloading(&numbers(10), &mut then, &[b"jjjjjj", b"r"]);
        assert!(at_top(&frames[1], "Line 4"), "{frames:#?}");
        assert!(at_top(&frames[3], "Line 5"), "{frames:#?}");
    }

    #[test]
    fn a_reload_keeps_the_offset_in_a_block_that_changed() {
        // A code block of eight lines, scrolled three lines into it; then
        // two of its lines change and a block is added above it.
        let code = |first: &str, last: &str| format!("```\n{first}\nb\nc\nd\ne\nf\ng\n{last}\n```");
        let old = blocks(&["Intro", &code("a", "h"), "Outro"]);
        let new = blocks(&["Added", "Intro", &code("A", "H"), "Outro"]);
        let mut then = Then::new([Reloaded::Document(new)]);
        let frames = show_reloading(&old, &mut then, &[b"jjjjj", b"r"]);
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert_eq!(top_row(&frames[1]), "c", "{frames:#?}");
        assert_eq!(top_row(&frames[3]), "c", "{frames:#?}");
    }

    #[test]
    fn a_reload_keeps_the_place_in_a_details_block() {
        let details = |added: &str| {
            let inner: String = (1..=6).map(|n| format!("Inner {n}\n\n")).collect();
            format!("{added}<details>\n<summary>More</summary>\n\n{inner}</details>\n\nEnd")
        };
        let old = crate::parse::parse(&details(""));
        let new = crate::parse::parse(&details("Added\n\n"));
        let mut then = Then::new([Reloaded::Document(new)]);
        let frames = show_reloading(&old, &mut then, &[b"jjjjjj", b"r"]);
        assert_eq!(frames.len(), 4, "{frames:#?}");
        let inner = top_row(&frames[1]);
        assert!(inner.starts_with("Inner"), "{frames:#?}");
        assert_eq!(top_row(&frames[3]), inner, "{frames:#?}");
    }

    #[test]
    fn a_reload_goes_to_the_next_block_from_a_blank_top_line() {
        // One line down, the blank line after Line 1 is at the top; then a
        // block is added above. As on a resize, the view goes to the start
        // of the next block. A header line only exists with several files,
        // and those cannot be reloaded, so that case is not tested here.
        let mut then = Then::new([Reloaded::Document(blocks(&[
            "Added", "Line 1", "Line 2", "Line 3", "Line 4", "Line 5", "Line 6",
        ]))]);
        let frames = show_reloading(&numbers(6), &mut then, &[b"j", b"r"]);
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert_eq!(top_row(&frames[1]), "", "{frames:#?}");
        assert!(at_top(&frames[3], "Line 2"), "{frames:#?}");
    }

    #[test]
    fn a_reload_goes_to_the_next_block_when_the_top_block_in_a_details_is_removed() {
        let details = |inner: &[&str]| {
            let inner: String = inner.iter().map(|line| format!("{line}\n\n")).collect();
            crate::parse::parse(&format!(
                "<details>\n<summary>More</summary>\n\n{inner}</details>\n\nEnd"
            ))
        };
        let old = details(&[
            "Inner 1", "Inner 2", "Inner 3", "Inner 4", "Inner 5", "Inner 6",
        ]);
        // A block before Inner 1 as well, so that the block numbers alone
        // would put Inner 2 at the top.
        let new = details(&["New", "Inner 1", "Inner 2", "Inner 4", "Inner 5", "Inner 6"]);
        let mut then = Then::new([Reloaded::Document(new)]);
        let frames = show_reloading(&old, &mut then, &[b"jjjjjj", b"r"]);
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert_eq!(top_row(&frames[1]), "Inner 3", "{frames:#?}");
        assert_eq!(top_row(&frames[3]), "Inner 4", "{frames:#?}");
    }

    /// The text on the top row of the document in `frame`.
    fn top_row(frame: &str) -> String {
        let frame = unstyled(frame);
        let row = frame.split("\x1b[1;1H").nth(1).unwrap();
        row.split("\x1b[2;1H").next().unwrap().trim().to_owned()
    }

    fn others() -> Document {
        crate::parse::parse("Other 1\n\nOther 2\n")
    }

    #[test]
    fn shows_the_lines_laid_out_for_the_window() {
        let caps = Capabilities {
            height: Some(6),
            ..Capabilities::ansi(40)
        };
        let frames = show_laid_out(&numbers(10), &others(), caps);
        assert!(frames[0].contains("Other 1"), "{frames:?}");
        assert!(!frames[0].contains("Line 1"), "{frames:?}");
    }

    #[test]
    fn lays_the_document_out_again_for_another_window() {
        let frames = show_laid_out(&numbers(10), &others(), Capabilities::ansi(80));
        assert!(frames[0].contains("Line 1"), "{frames:?}");
        assert!(!frames[0].contains("Other 1"), "{frames:?}");
    }

    #[test]
    fn draws_the_document_and_quits_on_q() {
        let mut terminal = Fake::new(40, 6, &[b"q"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 1, "{frames:?}");
        assert!(frames[0].contains("Line 1"), "{frames:?}");
        assert!(frames[0].contains("Line 3"), "{frames:?}");
        assert!(!frames[0].contains("Line 4"), "{frames:?}");
    }

    #[test]
    fn redraws_after_keys_that_move() {
        // Two lines down, then a key that does nothing, then the end.
        let mut terminal = Fake::new(40, 6, &[b"jj", b"x", b"", b"G"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 3, "{frames:?}");
        // Line 2 is at the top: the first row of the frame.
        assert!(frames[1].contains("\x1b[1;1H  Line 2"), "{frames:?}");
        assert!(frames[2].contains("Line 10"), "{frames:?}");
    }

    #[test]
    fn a_key_split_over_two_reads_moves() {
        // Page Down as an escape character and the rest of its sequence.
        let mut terminal = Fake::new(40, 6, &[b"\x1b", b"[6~"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 2, "{frames:?}");
        assert!(frames[1].contains("\x1b[2;1H  Line 4"), "{frames:?}");
    }

    #[test]
    fn the_status_line_is_on_the_bottom_row() {
        let mut terminal = Fake::new(40, 6, &[b"G"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 2, "{frames:?}");
        // The document takes the rows above it.
        assert!(frames[0].contains("\x1b[5;1H  Line 3"), "{frames:?}");
        assert!(!frames[0].contains("\x1b[6;1H  "), "{frames:?}");
        let status = |frame: &str| frame[frame.find("\x1b[6;1H").unwrap()..].to_owned();
        assert!(status(&frames[0]).contains(" kiri "), "{frames:?}");
        assert!(status(&frames[0]).contains(" notes.md "), "{frames:?}");
        assert!(status(&frames[0]).contains(" help"), "{frames:?}");
        assert!(status(&frames[1]).contains(" kiri "), "{frames:?}");
        assert!(status(&frames[1]).contains("100%"), "{frames:?}");
    }

    #[test]
    fn the_help_opens_over_the_dimmed_document_and_any_key_closes_it() {
        // The help, a key that closes it without moving, the help again,
        // and q, which closes it without quitting, then the end.
        let mut terminal = Fake::new(60, 24, &[b"?", b"j", b"h", b"q", b"G"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 6, "{frames:#?}");
        let help = |frame: &str| frame.contains("down a line");
        assert!(!help(&frames[0]), "{frames:#?}");
        assert!(help(&frames[1]), "{frames:#?}");
        assert!(frames[1].contains("[0;2m  Line 1"), "{frames:#?}");
        assert!(!help(&frames[2]), "{frames:#?}");
        assert!(!frames[2].contains("[0;2m"), "{frames:#?}");
        assert_eq!(frames[0], frames[2]);
        assert!(help(&frames[3]), "{frames:#?}");
        assert_eq!(frames[0], frames[4]);
        assert!(frames[5].contains("100%"), "{frames:#?}");
    }

    #[test]
    fn the_help_s_footer_is_muted_text_on_the_page() {
        let footer = |theme: &str| {
            let mut terminal = Fake::new(60, 24, &[b"?"]);
            let mut inputs = Inputs::new(Capabilities::ansi(80));
            inputs.theme = crate::theme::parse(theme).unwrap();
            let document = numbers(30);
            run(
                Content::Document(&document),
                None,
                "notes.md",
                &mut inputs,
                Options::new(SearchCase::Smart),
                &mut terminal,
            )
            .unwrap();
            let help = terminal.frames()[1].clone();
            let end = help.find("Any key closes this.").unwrap();
            help[help[..end].rfind('\x1b').unwrap()..end].to_owned()
        };
        // In the muted colour, on the page's background, as the 256-colour
        // palette has them.
        let page = "background = \"#ffffff\"\nforeground = \"#000000\"\n";
        let muted = footer(&format!("{page}muted = \"#767676\"\n"));
        assert_eq!(muted, "\x1b[0;38;5;243;48;5;231m");
        // Without a muted colour, the text's colour dimmed.
        let dimmed = footer(&format!("{page}muted = \"none\"\n"));
        assert_eq!(dimmed, "\x1b[0;2;38;5;16;48;5;231m");
    }

    #[test]
    fn keys_without_a_binding_close_the_help_too() {
        // F5 closes the help, then j moves.
        let mut terminal = Fake::new(60, 24, &[b"?", b"\x1b[15~", b"j"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert!(frames[1].contains("down a line"), "{frames:#?}");
        assert_eq!(frames[2], frames[0]);
        assert_ne!(frames[3], frames[0]);
    }

    #[test]
    fn ctrl_c_quits_even_from_the_help() {
        let mut terminal = Fake::new(60, 24, &[b"?", b"", b"G"]);
        show(&numbers(30), &mut terminal);
        assert_eq!(terminal.frames().len(), 2);
    }

    #[test]
    fn a_window_of_one_row_shows_the_document_without_a_status_line() {
        // The help has no room, so q quits at once.
        let mut terminal = Fake::new(40, 1, &[b"?", b"q", b"G"]);
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 1, "{frames:#?}");
        assert!(frames[0].contains("Line 1"), "{frames:#?}");
        assert!(!frames[0].contains(" kiri "), "{frames:#?}");
    }

    #[test]
    fn a_window_shrunk_to_one_row_closes_the_prompt() {
        // Without a status line the prompt cannot show, so G is a
        // binding again and moves to the end.
        let mut terminal = Fake::new(40, 6, &[b"/", b"", b"G"]);
        terminal.sizes = [window(40, 6), window(40, 1)].into();
        show(&numbers(10), &mut terminal);
        let frames = terminal.frames();
        assert!(frames.last().unwrap().contains("Line 10"), "{frames:#?}");
    }

    #[test]
    fn lays_the_document_out_again_for_a_new_window_size() {
        let mut terminal = Fake::new(40, 5, &[b"", b""]);
        terminal.sizes = [window(40, 5), window(8, 5)].into();
        let document = crate::parse::parse("A paragraph of several words\n");
        show(&document, &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 2, "{frames:?}");
        assert!(
            frames[0].contains("A paragraph of several words"),
            "{frames:?}"
        );
        assert!(!frames[1].contains("A paragraph of"), "{frames:?}");
    }

    #[test]
    fn a_heading_taller_than_the_window_is_drawn_at_normal_size() {
        let mut terminal = Fake::new(40, 3, &[b"G"]);
        let document = crate::parse::parse("# Title\n\nText\n");
        let mut inputs = Inputs::new(Capabilities::kitty(80));
        run(
            Content::Document(&document),
            None,
            "notes.md",
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        )
        .unwrap();
        let frames = terminal.frames();
        assert!(frames[0].contains("Title"), "{frames:?}");
        assert!(!frames[0].contains("\x1b]66;"), "{frames:?}");
        // The last page shows the text below it.
        assert!(frames.last().unwrap().contains("Text"), "{frames:?}");
    }

    #[test]
    fn a_given_width_stays_when_the_window_changes() {
        let mut terminal = Fake::new(40, 5, &[b""]);
        terminal.sizes = [window(80, 5)].into();
        let document = crate::parse::parse("A paragraph of several words\n");
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        run(
            Content::Document(&document),
            None,
            "notes.md",
            &mut inputs,
            Options {
                width: Some(10),
                ..Options::new(SearchCase::Smart)
            },
            &mut terminal,
        )
        .unwrap();
        // One frame before the window changed, and one after.
        assert_eq!(terminal.frames().len(), 2);
        for frame in terminal.frames() {
            assert!(!frame.contains("A paragraph of"), "{frame:?}");
        }
    }

    /// The files `a.md` of 3 lines and `b.md` of 10, as [`numbers`] has
    /// them. In a stream, `a.md` takes lines 0 to 8, its header and blank
    /// line included, and `b.md` starts on line 10.
    fn two_files() -> Vec<File> {
        [("a.md", 3), ("b.md", 10)]
            .into_iter()
            .map(|(name, count)| File {
                name: name.to_owned(),
                document: numbers(count),
            })
            .collect()
    }

    /// Shows `files` on `terminal` as the binary does, without a name of
    /// its own.
    fn show_files(files: &[File], terminal: &mut Fake) {
        let mut inputs = Inputs::new(Capabilities::ansi(80));
        run(
            Content::Files(files),
            None,
            "",
            &mut inputs,
            Options::new(SearchCase::Smart),
            terminal,
        )
        .unwrap();
    }

    #[test]
    fn the_status_line_names_the_file_at_the_top_of_the_view() {
        // Down to the blank line above b.md's header, onto the header, and
        // back up.
        let mut terminal = Fake::new(40, 6, &[b"jjjjjjjjj", b"j", b"k"]);
        show_files(&two_files(), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 4, "{frames:#?}");
        let names = |frame: &str| status_row(frame, 6).contains(" a.md ");
        assert!(names(&frames[0]), "{frames:#?}");
        assert!(names(&frames[1]), "{frames:#?}");
        // b.md's header at the top: an empty row of its band, then its name.
        assert!(
            unstyled(&frames[2]).contains("\x1b[2;1H▌ b.md"),
            "{frames:#?}"
        );
        assert!(status_row(&frames[2], 6).contains(" b.md "), "{frames:#?}");
        assert!(!names(&frames[2]), "{frames:#?}");
        assert!(names(&frames[3]), "{frames:#?}");
    }

    #[test]
    fn the_status_line_tells_the_file_s_place_among_the_files() {
        let mut terminal = Fake::new(40, 6, &[b":n"]);
        show_files(&two_files(), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 2, "{frames:#?}");
        assert!(
            status_row(&frames[0], 6).contains(" a.md 1/2 "),
            "{frames:#?}"
        );
        assert!(
            status_row(&frames[1], 6).contains(" b.md 2/2 "),
            "{frames:#?}"
        );
        // A lone document has no place among others.
        let mut terminal = Fake::new(40, 6, &[]);
        show(&numbers(3), &mut terminal);
        let status = status_row(&terminal.frames()[0], 6);
        assert!(status.contains(" notes.md  "), "{status:?}");
        assert!(!status.contains("1/1"), "{status:?}");
    }

    /// Files called `name` with `count` lines each, as [`numbers`] has
    /// them.
    fn files(counts: &[(&str, usize)]) -> Vec<File> {
        counts
            .iter()
            .map(|&(name, count)| File {
                name: name.to_owned(),
                document: numbers(count),
            })
            .collect()
    }

    /// Whether `frame` shows the header of `name` at the top of the view
    /// of `rows` rows, and `name` on the status line below it.
    fn at_file(frame: &str, name: &str, rows: u16) -> bool {
        unstyled(frame).contains(&format!("\x1b[2;1H▌ {name}\x1b"))
            && status_row(frame, rows + 1).contains(&format!(" {name} "))
    }

    #[test]
    fn colon_n_and_colon_p_move_to_the_start_of_the_next_and_previous_file() {
        // b.md starts on line 10 and c.md on line 34. The colon on its own,
        // then n; :n at the last file, which only tells there is none; :p
        // from the start of c.md, and from inside b.md, first to its start.
        let reads: &[&[u8]] = &[b":", b"n", b":n", b":n", b":p", b"jjj", b":p", b":p"];
        let mut terminal = Fake::new(40, 6, reads);
        show_files(
            &files(&[("a.md", 3), ("b.md", 10), ("c.md", 10)]),
            &mut terminal,
        );
        let frames = terminal.frames();
        assert_eq!(frames.len(), 9, "{frames:#?}");
        assert!(at_file(&frames[0], "a.md", 5), "{frames:#?}");
        assert!(
            status_row(&frames[1], 6).contains(" :\u{258f}"),
            "{frames:#?}"
        );
        assert!(at_file(&frames[2], "b.md", 5), "{frames:#?}");
        assert!(at_file(&frames[3], "c.md", 5), "{frames:#?}");
        assert_eq!(document_part(&frames[4], 6), document_part(&frames[3], 6));
        assert!(at_file(&frames[5], "b.md", 5), "{frames:#?}");
        assert!(status_row(&frames[6], 6).contains(" b.md "), "{frames:#?}");
        assert!(at_file(&frames[7], "b.md", 5), "{frames:#?}");
        assert!(at_file(&frames[8], "a.md", 5), "{frames:#?}");
    }

    #[test]
    fn a_new_window_size_moves_the_blank_line_between_files_to_the_next_file() {
        // Line 9 is the blank line between a.md and b.md.
        let reads: &[&[u8]] = &[b"jjjjjjjjj", b""];
        let mut terminal = Fake::new(40, 6, reads);
        terminal.sizes = [window(40, 6), window(50, 6)].into();
        show_files(&two_files(), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 3, "{frames:#?}");
        assert!(at_file(&frames[2], "b.md", 5), "{frames:#?}");
    }

    #[test]
    fn shift_page_down_and_shift_page_up_move_as_colon_n_and_colon_p_do() {
        // As in the test above: past the last file, Shift+PgDn only tells
        // there is none; Shift+PgUp from inside b.md goes first to its
        // start.
        let (down, up): (&[u8], &[u8]) = (b"\x1b[6;2~", b"\x1b[5;2~");
        let reads = &[down, down, down, up, b"jjj", up, up];
        let mut terminal = Fake::new(40, 6, reads);
        show_files(
            &files(&[("a.md", 3), ("b.md", 10), ("c.md", 10)]),
            &mut terminal,
        );
        let frames = terminal.frames();
        assert_eq!(frames.len(), 8, "{frames:#?}");
        assert!(at_file(&frames[0], "a.md", 5), "{frames:#?}");
        assert!(at_file(&frames[1], "b.md", 5), "{frames:#?}");
        assert!(at_file(&frames[2], "c.md", 5), "{frames:#?}");
        assert!(
            status_row(&frames[3], 6).contains("no next file"),
            "{frames:#?}"
        );
        assert!(at_file(&frames[4], "b.md", 5), "{frames:#?}");
        assert!(status_row(&frames[5], 6).contains(" b.md "), "{frames:#?}");
        assert!(at_file(&frames[6], "b.md", 5), "{frames:#?}");
        assert!(at_file(&frames[7], "a.md", 5), "{frames:#?}");
    }

    #[test]
    fn colon_n_names_a_file_on_the_last_page_though_the_view_stops_short() {
        // In 11 rows, the last page starts at b.md, the line 24, and also
        // shows c.md, which starts on line 30.
        let reads: &[&[u8]] = &[b":n", b":n", b":p", b":n", b"k"];
        let mut terminal = Fake::new(40, 12, reads);
        show_files(
            &files(&[("a.md", 10), ("b.md", 1), ("c.md", 1)]),
            &mut terminal,
        );
        let frames = terminal.frames();
        assert_eq!(frames.len(), 6, "{frames:#?}");
        let named = |frame: &str, name: &str| status_row(frame, 12).contains(&format!(" {name} "));
        assert!(at_file(&frames[1], "b.md", 11), "{frames:#?}");
        assert!(named(&frames[2], "c.md"), "{frames:#?}");
        // The view stays at b.md's header.
        assert!(
            unstyled(&frames[2]).contains("\x1b[2;1H▌ b.md\x1b"),
            "{frames:#?}"
        );
        assert!(at_file(&frames[3], "b.md", 11), "{frames:#?}");
        assert!(named(&frames[4], "c.md"), "{frames:#?}");
        // Once the view moves, the file at its top again.
        assert!(named(&frames[5], "a.md"), "{frames:#?}");
    }

    #[test]
    fn colon_n_still_names_its_file_after_a_new_window_size() {
        // As above, but the window then grows by two rows, so the last
        // page starts higher up, and the view still stops short of c.md's
        // header.
        let reads: &[&[u8]] = &[b":n", b":n", b""];
        let mut terminal = Fake::new(40, 12, reads);
        terminal.sizes = [window(40, 12), window(40, 12), window(40, 14)].into();
        show_files(
            &files(&[("a.md", 10), ("b.md", 1), ("c.md", 1)]),
            &mut terminal,
        );
        let frames = terminal.frames();
        assert_eq!(frames.len(), 4, "{frames:#?}");
        let named =
            |frame: &str, name: &str, rows| status_row(frame, rows).contains(&format!(" {name} "));
        assert!(named(&frames[2], "c.md", 12), "{frames:#?}");
        assert!(named(&frames[3], "c.md", 14), "{frames:#?}");
    }

    #[test]
    fn a_key_after_a_new_window_size_makes_colon_n_forget_its_file() {
        // As above, then k: the view moves, so the status line names the
        // file above again.
        let reads: &[&[u8]] = &[b":n", b":n", b"", b"k"];
        let mut terminal = Fake::new(40, 12, reads);
        terminal.sizes = [window(40, 12), window(40, 12), window(40, 14)].into();
        show_files(
            &files(&[("a.md", 10), ("b.md", 1), ("c.md", 1)]),
            &mut terminal,
        );
        let frames = terminal.frames();
        assert_eq!(frames.len(), 5, "{frames:#?}");
        let named = |frame: &str, name: &str| status_row(frame, 14).contains(&format!(" {name} "));
        assert!(named(&frames[3], "c.md"), "{frames:#?}");
        assert!(!named(&frames[4], "c.md"), "{frames:#?}");
    }

    #[test]
    fn a_key_after_the_colon_that_is_no_command_only_closes_it() {
        let reads: &[&[u8]] = &[b":", b"j", b":", b"\x1b", b"", b"j"];
        let mut terminal = Fake::new(40, 6, reads);
        show_files(&two_files(), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 6, "{frames:#?}");
        // j did not move, and the colon is gone.
        assert_eq!(frames[2], frames[0]);
        assert_eq!(frames[4], frames[0]);
        // Down a line, onto the name in a.md's header.
        let top = "\x1b[1;1H▌ a.md\x1b";
        assert!(unstyled(&frames[5]).contains(top), "{frames:#?}");
    }

    #[test]
    fn at_the_ends_of_the_files_the_status_line_tells_there_is_none_until_the_next_key() {
        // :p at the very start, then j; :n to b.md and :n again at the
        // last file, then k; Shift+PgDn back to b.md and once more.
        let down: &[u8] = b"\x1b[6;2~";
        let reads: &[&[u8]] = &[b":p", b"j", b":n", b":n", b"k", down, down];
        let mut terminal = Fake::new(40, 6, reads);
        show_files(&two_files(), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 8, "{frames:#?}");
        let tells = |frame: &str, text: &str| status_row(frame, 6).contains(text);
        assert!(tells(&frames[1], " no previous file "), "{frames:#?}");
        // The view did not move.
        assert_eq!(document_part(&frames[1], 6), document_part(&frames[0], 6));
        assert!(!tells(&frames[2], "no previous file"), "{frames:#?}");
        assert!(at_file(&frames[3], "b.md", 5), "{frames:#?}");
        assert!(!tells(&frames[3], "no next file"), "{frames:#?}");
        assert!(tells(&frames[4], " no next file "), "{frames:#?}");
        assert!(at_file(&frames[4], "b.md", 5), "{frames:#?}");
        assert!(!tells(&frames[5], "no next file"), "{frames:#?}");
        assert!(at_file(&frames[6], "b.md", 5), "{frames:#?}");
        assert!(tells(&frames[7], " no next file "), "{frames:#?}");
        assert!(at_file(&frames[7], "b.md", 5), "{frames:#?}");
    }

    #[test]
    fn a_colon_in_a_lone_document_does_nothing_and_lets_the_next_key_through() {
        // :n there is n, which goes on to the next match.
        let with_colon: &[&[u8]] = &[b":", b"j", b"/Line\r", b":n", b":"];
        let without: &[&[u8]] = &[b"j", b"/Line\r", b"n"];
        let frames = |reads| {
            let mut terminal = Fake::new(40, 6, reads);
            show(&numbers(30), &mut terminal);
            terminal.frames()
        };
        let shown = frames(with_colon);
        assert_eq!(shown, frames(without));
        assert_eq!(shown.len(), 4, "{shown:#?}");
    }

    #[test]
    fn the_help_lists_colon_n_and_colon_p_only_with_several_files() {
        let mut terminal = Fake::new(60, 24, &[b"?"]);
        show_files(&two_files(), &mut terminal);
        let help = terminal.frames()[1].clone();
        assert!(help.contains(":n"), "{help:?}");
        assert!(help.contains("next file"), "{help:?}");
        assert!(help.contains(":p"), "{help:?}");
        assert!(help.contains("previous file"), "{help:?}");
        assert!(help.contains("Shift+PgDn"), "{help:?}");
        assert!(help.contains("Shift+PgUp"), "{help:?}");
        let mut terminal = Fake::new(60, 24, &[b"?"]);
        show(&numbers(30), &mut terminal);
        let help = terminal.frames()[1].clone();
        assert!(help.contains("down a line"), "{help:?}");
        assert!(!help.contains("next file"), "{help:?}");
        assert!(!help.contains("Shift+"), "{help:?}");
    }

    /// The status line of `frame`, on row `row`.
    fn status_row(frame: &str, row: u16) -> String {
        let at = format!("\x1b[{row};1H");
        frame[frame.find(&at).unwrap()..].to_owned()
    }

    /// Whether `frame` shows `line` on its top row, in any style.
    fn at_top(frame: &str, line: &str) -> bool {
        unstyled(frame).contains(&format!("\x1b[1;1H  {line}\x1b"))
    }

    /// `frame` without its SGR sequences.
    fn unstyled(frame: &str) -> String {
        let mut rest = frame;
        let mut kept = String::new();
        while let Some(at) = rest.find("\x1b[") {
            kept.push_str(&rest[..at]);
            let after = &rest[at + 2..];
            let parameters = after
                .find(|c: char| !c.is_ascii_digit() && c != ';' && c != ':')
                .unwrap_or(after.len());
            if after[parameters..].starts_with('m') {
                rest = &after[parameters + 1..];
            } else {
                kept.push_str("\x1b[");
                rest = after;
            }
        }
        kept.push_str(rest);
        kept
    }

    #[test]
    fn a_slash_opens_a_prompt_on_the_status_line() {
        let mut terminal = Fake::new(40, 6, &[b"/", "lä".as_bytes()]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        for (frame, prompt) in [(&frames[1], "/\u{258f}"), (&frames[2], "/lä\u{258f}")] {
            let status = status_row(frame, 6);
            assert!(status.contains(" kiri "), "{status:?}");
            assert!(status.contains(prompt), "{status:?}");
            assert!(!status.contains("notes.md"), "{status:?}");
            assert!(!status.contains("help"), "{status:?}");
        }
    }

    #[test]
    fn enter_moves_to_the_first_match_from_the_top_of_the_view() {
        // Line 3 is above the view after a page down, so Line 30 is the
        // first match.
        let mut terminal = Fake::new(40, 6, &[b"jjjjjjjj", b"/line 3", b"\r"]);
        show(&numbers(40), &mut terminal);
        let frames = terminal.frames();
        assert!(at_top(frames.last().unwrap(), "Line 30"), "{frames:#?}");
        let status = status_row(frames.last().unwrap(), 6);
        assert!(status.contains("notes.md"), "{status:?}");
        assert!(!status.contains('\u{258f}'), "{status:?}");
    }

    #[test]
    fn a_match_already_in_view_does_not_move_it() {
        // Two lines down after the search shows Line 2 at the top.
        let mut terminal = Fake::new(40, 6, &[b"/line 2\r", b"jj"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert!(at_top(frames.last().unwrap(), "Line 2"), "{frames:#?}");
    }

    #[test]
    fn escape_and_ctrl_c_cancel_the_prompt() {
        let reads: &[&[u8]] = &[b"/line 20", b"\x1b", b"", b"/line 20", b"\x03", b"jj"];
        let mut terminal = Fake::new(40, 6, reads);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        let last = frames.last().unwrap();
        // Ctrl+C did not quit, and j moved down.
        assert!(at_top(last, "Line 2"), "{frames:#?}");
        assert!(status_row(last, 6).contains("notes.md"), "{frames:#?}");
    }

    #[test]
    fn backspace_deletes_and_on_an_empty_prompt_cancels() {
        let reads: &[&[u8]] = &[b"/line 2x", b"\x7f0\r", b"/a", b"\x7f\x08", b"jj"];
        let mut terminal = Fake::new(40, 6, reads);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert!(
            frames.iter().any(|frame| at_top(frame, "Line 20")),
            "{frames:#?}"
        );
        assert!(at_top(frames.last().unwrap(), "Line 21"), "{frames:#?}");
    }

    #[test]
    fn enter_on_an_empty_prompt_goes_on_from_the_view_after_scrolling_away() {
        let reads: &[&[u8]] = &[b"/line 20\r", b"g", b"/\r"];
        let mut terminal = Fake::new(60, 6, reads);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert!(at_top(&frames[2], "Line 1"), "{frames:#?}");
        let last = frames.last().unwrap();
        assert!(at_top(last, "Line 20"), "{frames:#?}");
        assert!(status_row(last, 6).contains("/line 20 1/1"), "{frames:#?}");
    }

    #[test]
    fn enter_on_an_empty_prompt_goes_on_to_the_next_match_as_n_does() {
        // Line 2 is in view after the search, so the repeat moves on.
        let reads: &[&[u8]] = &[b"/line 2\r", b"/\r"];
        let mut terminal = Fake::new(60, 6, reads);
        show(&numbers(30), &mut terminal);
        let last = terminal.frames().pop().unwrap();
        assert!(status_row(&last, 6).contains("/line 2 2/11"), "{last:?}");
        assert!(at_top(&last, "Line 20"), "{last:?}");
    }

    #[test]
    fn enter_on_an_empty_prompt_without_a_search_cancels() {
        let mut terminal = Fake::new(40, 6, &[b"/\r", b"jj"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        let last = frames.last().unwrap();
        assert!(at_top(last, "Line 2"), "{frames:#?}");
        assert!(status_row(last, 6).contains("notes.md"), "{frames:#?}");
    }

    #[test]
    fn ctrl_u_clears_the_prompt() {
        let mut terminal = Fake::new(40, 6, &[b"/xyz", b"\x15line 20\r"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert!(at_top(frames.last().unwrap(), "Line 20"), "{frames:#?}");
    }

    #[test]
    fn keys_go_to_the_prompt_while_it_is_open() {
        // q and G are typed, not bindings.
        let mut terminal = Fake::new(40, 6, &[b"/qG"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert!(status_row(&frames[1], 6).contains("/qG"), "{frames:#?}");
        assert!(at_top(&frames[1], "Line 1"), "{frames:#?}");
    }

    #[test]
    fn the_status_line_counts_the_matches_after_a_search() {
        let mut terminal = Fake::new(60, 6, &[b"/line 2\r"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        let status = status_row(frames.last().unwrap(), 6);
        assert!(status.contains("notes.md"), "{status:?}");
        assert!(status.contains("/line 2 1/11"), "{status:?}");
    }

    #[test]
    fn a_term_not_found_is_told_until_the_next_key() {
        let mut terminal = Fake::new(60, 6, &[b"/xyz\r", b"x"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 3, "{frames:#?}");
        assert!(
            status_row(&frames[1], 6).contains("not found: xyz"),
            "{frames:#?}"
        );
        assert!(!status_row(&frames[2], 6).contains("xyz"), "{frames:#?}");
    }

    #[test]
    fn n_and_shift_n_move_between_matches_and_around_the_ends() {
        let reads: &[&[u8]] = &[b"/line 2\r", b"n", b"N", b"N", b"n"];
        let mut terminal = Fake::new(60, 6, reads);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        let shows =
            |frame: &str, line, count| at_top(frame, line) && status_row(frame, 6).contains(count);
        assert_eq!(frames.len(), 6, "{frames:#?}");
        assert!(shows(&frames[1], "Line 1", "/line 2 1/11"), "{frames:#?}");
        assert!(shows(&frames[2], "Line 20", "/line 2 2/11"), "{frames:#?}");
        assert!(shows(&frames[3], "Line 2", "/line 2 1/11"), "{frames:#?}");
        // Around the start to the last match, on the last page, and
        // around the end to the first, each with the number flashing.
        assert!(unstyled(&frames[4]).contains("Line 29"), "{frames:#?}");
        let status = status_row(&frames[4], 6);
        assert!(status.contains("11\x1b[0;7m/11"), "{frames:#?}");
        assert!(at_top(&frames[5], "Line 2"), "{frames:#?}");
        let status = status_row(&frames[5], 6);
        assert!(status.contains("m1\x1b[0;7m/11"), "{frames:#?}");
    }

    #[test]
    fn n_after_scrolling_away_goes_on_from_the_view() {
        // At the first match, then to the end and back: n takes the first
        // match in the view, not the one after the match left behind.
        let reads: &[&[u8]] = &[b"/line 2\r", b"G", b"n"];
        let mut terminal = Fake::new(60, 6, reads);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 4, "{frames:#?}");
        let status = status_row(&frames[3], 6);
        assert!(status.contains("/line 2 10/11"), "{frames:#?}");
        assert!(unstyled(&frames[3]).contains("Line 29"), "{frames:#?}");
    }

    #[test]
    fn n_without_a_search_does_nothing() {
        let mut terminal = Fake::new(60, 6, &[b"nN", b"jj"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 2, "{frames:#?}");
    }

    #[test]
    fn n_after_a_term_not_found_tells_it_again() {
        let mut terminal = Fake::new(60, 6, &[b"/xyz\r", b"j", b"n"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        let last = status_row(frames.last().unwrap(), 6);
        assert!(last.contains("not found: xyz"), "{frames:#?}");
    }

    #[test]
    fn escape_hides_the_search_and_n_takes_it_up_again() {
        let reads: &[&[u8]] = &[b"/line 2\r", b"\x1b", b"", b"n"];
        let mut terminal = Fake::new(60, 6, reads);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert!(
            status_row(&frames[1], 6).contains("/line 2 1/11"),
            "{frames:#?}"
        );
        assert!(!status_row(&frames[2], 6).contains("line 2"), "{frames:#?}");
        assert!(at_top(&frames[3], "Line 20"), "{frames:#?}");
        assert!(
            status_row(&frames[3], 6).contains("/line 2 2/11"),
            "{frames:#?}"
        );
    }

    #[test]
    fn a_new_window_size_finds_the_matches_again_from_the_top_of_the_view() {
        let reads: &[&[u8]] = &[b"/line\r", b"jjjjjjjj", b""];
        let mut terminal = Fake::new(60, 6, reads);
        terminal.sizes = [window(60, 6), window(60, 6), window(70, 6)].into();
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 4, "{frames:#?}");
        assert!(
            status_row(&frames[2], 6).contains("/line 1/30"),
            "{frames:#?}"
        );
        // Line 5 is at the top, and the view stays.
        assert!(at_top(&frames[3], "Line 5"), "{frames:#?}");
        assert!(
            status_row(&frames[3], 6).contains("/line 5/30"),
            "{frames:#?}"
        );
    }

    #[test]
    fn without_colours_going_around_an_end_flashes_the_match_number() {
        // N goes around to the last match, then `empty` polls without keys.
        let frames = |empty| {
            let mut caps = Capabilities::ansi(80);
            caps.color = false;
            frames_with(caps, &[b"/line 2\r", b"N"], empty)
        };
        // Bold, without the bar's reverse video, for half a second.
        let flashed = "\x1b[0;1m11\x1b[0;7m/11";
        // While the match fades, each poll is a twentieth of a second.
        let before = frames(9);
        assert_eq!(before.len(), 3, "{before:#?}");
        assert!(status_row(&before[2], 6).contains(flashed), "{before:#?}");
        let after = frames(10);
        assert_eq!(after.len(), 4, "{after:#?}");
        assert!(!status_row(&after[3], 6).contains(flashed), "{after:#?}");
        assert!(status_row(&after[3], 6).contains(" 11/11"), "{after:#?}");
    }

    /// The codes of the last SGR sequence before `text` in `frame`; none
    /// when there is none.
    fn style_before(frame: &str, text: &str) -> Vec<String> {
        let at = frame
            .find(text)
            .unwrap_or_else(|| panic!("{text:?} in {frame:?}"));
        let Some(start) = frame[..at].rfind("\x1b[0") else {
            return Vec::new();
        };
        let start = start + 2;
        let end = start + frame[start..].find('m').unwrap();
        frame[start..end].split(';').map(str::to_owned).collect()
    }

    /// `frame` without its status line on row `row`.
    fn document_part(frame: &str, row: u16) -> &str {
        &frame[..frame.find(&format!("\x1b[{row};1H")).unwrap()]
    }

    #[test]
    fn matches_in_view_are_marked_and_the_current_one_apart() {
        // Without colours known as RGB, a jump marks the match at once.
        let wait = [b"".as_slice(); 30];
        let reads = [
            &[b"/line 2\r".as_slice()],
            &wait[..],
            &[b"n"],
            &wait,
            &[b"\x1b", b""],
        ];
        let mut terminal = Fake::new(60, 6, &reads.concat());
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert_eq!(frames.len(), 4, "{frames:#?}");
        // Line 2 is the current match, below Line 1.
        let found = document_part(&frames[1], 6);
        assert_eq!(style_before(found, "Line 2\x1b"), ["0", "1", "7"]);
        assert!(found.contains("  Line 1\x1b"), "{found:?}");
        // Line 20 is the current match, and Line 21 another.
        let next = document_part(&frames[2], 6);
        assert_eq!(style_before(next, "Line 2\x1b[0m0"), ["0", "1", "7"]);
        assert_eq!(style_before(next, "Line 2\x1b[0m1"), ["0", "7"]);
        // Escape takes the marks away.
        let hidden = document_part(&frames[3], 6);
        assert!(
            !hidden.contains(";7m") && !hidden.contains("[0;7"),
            "{hidden:?}"
        );
        assert!(at_top(&frames[3], "Line 20"), "{frames:#?}");
    }

    #[test]
    fn matches_are_marked_in_large_headings_and_in_tables() {
        let mut reads = vec![b"/kiri\r".as_slice()];
        // After the fade of the jump.
        reads.extend([b"".as_slice(); 30]);
        let mut terminal = Fake::new(60, 12, &reads);
        let source = "# Kiri title\n\n| a | b |\n|---|---|\n| c | kiri |\n";
        let mut inputs = Inputs::new(Capabilities::kitty(80));
        run(
            Content::Document(&crate::parse::parse(source)),
            None,
            "notes.md",
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        )
        .unwrap();
        let frames = terminal.frames();
        let found = document_part(frames.last().unwrap(), 12);
        // The heading is split where the match ends, both parts scaled.
        let heading = style_before(found, "\x1b]66;s=3;Kiri\x1b\\");
        assert!(heading.contains(&"1".into()) && heading.contains(&"7".into()));
        assert!(found.contains("\x1b]66;s=3; title\x1b"), "{found:?}");
        let cell = style_before(found, "kiri");
        assert!(cell.contains(&"7".into()) && !cell.contains(&"1".into()));
        // Without the search, neither is marked.
        assert!(!document_part(&frames[0], 12).contains(";7"), "{frames:#?}");
    }

    #[test]
    fn n_without_going_around_does_not_flash() {
        let mut terminal = Fake::new(60, 6, &[b"/line 2\r", b"n"]);
        show(&numbers(30), &mut terminal);
        let frames = terminal.frames();
        assert!(
            status_row(&frames[2], 6).contains("/line 2 2/11"),
            "{frames:#?}"
        );
    }

    /// The pixels of the image the tests of images show: a square of
    /// 20 pixels in `rgba`.
    fn square(rgba: [u8; 4]) -> crate::images::Source {
        crate::images::Source::Raster(image::RgbaImage::from_pixel(20, 20, image::Rgba(rgba)))
    }

    /// A document that shows the image `square.png`, and inputs for kitty
    /// with its pixels in `rgba`.
    fn pictured(rgba: [u8; 4]) -> (Document, Inputs) {
        let document = crate::parse::parse("Above\n\n![a square](square.png)\n\nBelow\n");
        let mut inputs = Inputs::new(Capabilities::kitty(80));
        inputs.images.insert("square.png", Ok(square(rgba)));
        (document, inputs)
    }

    /// All the pager writes for `document` with `inputs` on `terminal`,
    /// reloading it from `then` when there is one.
    fn written(
        document: &Document,
        mut inputs: Inputs,
        terminal: &mut Fake,
        then: Option<&mut Then>,
    ) -> String {
        run(
            Content::Document(document),
            None,
            "notes.md",
            &mut inputs,
            Options {
                reload: then.map(|then| then as &mut dyn Reload),
                ..Options::new(SearchCase::Smart)
            },
            terminal,
        )
        .unwrap();
        String::from_utf8(terminal.out.clone()).unwrap()
    }

    /// How many images `out` sends.
    fn transmits(out: &str) -> usize {
        out.matches("\x1b_Ga=T,").count()
    }

    #[test]
    fn an_image_is_sent_before_its_first_row() {
        let (document, inputs) = pictured([255, 0, 0, 255]);
        let mut terminal = Fake::new(60, 6, &[b"q"]);
        let out = written(&document, inputs, &mut terminal, None);
        assert_eq!(transmits(&out), 1, "{out:?}");
        let sent = out.find("\x1b_Ga=T,").unwrap();
        let shown = out.find(crate::kitty::graphics::PLACEHOLDER).unwrap();
        assert!(sent < shown, "{out:?}");
    }

    /// The ids of the images `out` sends, in order.
    fn sent_ids(out: &str) -> Vec<u32> {
        ids_after(out, "\x1b_Ga=T,U=1,q=2,f=32,o=z,i=")
    }

    /// The ids of the images `out` deletes, in order.
    fn deleted_ids(out: &str) -> Vec<u32> {
        ids_after(out, "\x1b_Ga=d,d=I,i=")
    }

    fn ids_after(out: &str, prefix: &str) -> Vec<u32> {
        (out.split(prefix).skip(1))
            .map(|rest| {
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().unwrap()
            })
            .collect()
    }

    /// Reloads `document` with the image `square.png` in `rgba`.
    struct Repaint {
        document: Document,
        rgba: [u8; 4],
    }

    impl Reload for Repaint {
        fn reload(&mut self, images: &mut Images, _: &mut Loads, _: &Capabilities) -> Reloaded {
            images.insert("square.png", Ok(square(self.rgba)));
            Reloaded::Document(self.document.clone())
        }

        fn change(&mut self) -> Change {
            Change::Unchanged
        }
    }

    /// All the pager writes for `document`, whose image is `square.png`
    /// in `before`, when `r` reloads it with the image in `after`.
    fn repainted(document: &Document, before: [u8; 4], after: [u8; 4]) -> String {
        let (_, mut inputs) = pictured(before);
        let mut terminal = Fake::new(60, 6, &[b"r"]);
        let mut repaint = Repaint {
            document: document.clone(),
            rgba: after,
        };
        let reload: &mut dyn Reload = &mut repaint;
        run(
            Content::Document(document),
            None,
            "notes.md",
            &mut inputs,
            Options {
                reload: Some(reload),
                ..Options::new(SearchCase::Smart)
            },
            &mut terminal,
        )
        .unwrap();
        String::from_utf8(terminal.out).unwrap()
    }

    #[test]
    fn an_image_the_terminal_holds_is_not_sent_again_after_a_resize() {
        let (document, inputs) = pictured([255, 0, 0, 255]);
        let mut terminal = Fake::new(60, 6, &[b""]);
        // Wider, and the image at the same size.
        terminal.sizes.push_back(window(70, 6));
        let out = written(&document, inputs, &mut terminal, None);
        assert_eq!(terminal.frames().len(), 2, "{out:?}");
        assert_eq!(out.matches(crate::kitty::graphics::PLACEHOLDER).count(), 4);
        assert_eq!(transmits(&out), 1, "{out:?}");
    }

    #[test]
    fn a_reload_that_leaves_the_image_as_it_was_does_not_send_it_again() {
        let (document, _) = pictured([255, 0, 0, 255]);
        let out = repainted(&document, [255, 0, 0, 255], [255, 0, 0, 255]);
        assert_eq!(transmits(&out), 1, "{out:?}");
    }

    #[test]
    fn a_changed_image_is_sent_anew_and_the_old_one_deleted() {
        let (document, _) = pictured([255, 0, 0, 255]);
        let out = repainted(&document, [255, 0, 0, 255], [0, 0, 255, 255]);
        let sent = sent_ids(&out);
        assert_eq!(sent.len(), 2, "{out:?}");
        assert_ne!(sent[0], sent[1]);
        // The old image goes before the new one comes, and the new one on
        // quitting.
        assert_eq!(deleted_ids(&out), sent, "{out:?}");
        let deleted = out.find("\x1b_Ga=d,").unwrap();
        assert!(deleted < out.rfind("\x1b_Ga=T,").unwrap(), "{out:?}");
    }

    #[test]
    fn a_layout_without_the_image_deletes_it_as_its_frame_is_drawn() {
        let (document, inputs) = pictured([255, 0, 0, 255]);
        let without = crate::parse::parse("Above\n\nBelow\n");
        let mut then = Then::new([Reloaded::Document(without)]);
        let mut terminal = Fake::new(60, 6, &[b"r"]);
        let out = written(&document, inputs, &mut terminal, Some(&mut then));
        let sent = sent_ids(&out);
        assert_eq!(sent.len(), 1, "{out:?}");
        assert_eq!(deleted_ids(&out), sent, "{out:?}");
        // In the frame of the new layout, before anything else in it.
        let frames = terminal.frames();
        let last = frames.last().unwrap();
        assert!(last.starts_with("\x1b_Ga=d,"), "{frames:#?}");
    }

    #[test]
    fn a_resize_that_shows_the_image_smaller_deletes_the_larger_one() {
        let (document, mut inputs) = pictured([255, 0, 0, 255]);
        let wide = image::RgbaImage::from_pixel(400, 20, image::Rgba([0, 255, 0, 255]));
        let wide = crate::images::Source::Raster(wide);
        inputs.images.insert("square.png", Ok(wide));
        let mut terminal = Fake::new(60, 6, &[b""]);
        terminal.sizes.push_back(window(30, 6));
        let out = written(&document, inputs, &mut terminal, None);
        let sent = sent_ids(&out);
        assert_eq!(sent.len(), 2, "{out:?}");
        // The smaller one goes on quitting.
        assert_eq!(deleted_ids(&out), sent, "{out:?}");
    }

    #[test]
    fn quitting_deletes_every_image_the_terminal_holds() {
        // q, and Ctrl+C once the keys run out, each after a resize.
        for keys in [&[b"".as_slice(), b"q"][..], &[b""]] {
            let (document, mut inputs) = pictured([255, 0, 0, 255]);
            let wide = image::RgbaImage::from_pixel(400, 20, image::Rgba([0, 255, 0, 255]));
            let wide = crate::images::Source::Raster(wide);
            inputs.images.insert("square.png", Ok(wide));
            let mut terminal = Fake::new(60, 6, keys);
            // The image smaller in a narrower window.
            terminal.sizes.push_back(window(30, 6));
            let out = written(&document, inputs, &mut terminal, None);
            let sent = sent_ids(&out);
            assert_eq!(sent.len(), 2, "{out:?}");
            // The wide image as the window narrows, the narrow one on
            // quitting, after the last frame.
            assert_eq!(deleted_ids(&out), sent, "{out:?}");
            let last = out.rsplit("\x1b[?2026l").next().unwrap();
            let quit = format!("\x1b_Ga=d,d=I,i={},q=2\x1b\\", sent[1]);
            assert_eq!(last, quit, "{out:?}");
        }
    }

    /// All the pager writes for a document that shows `square.png` and
    /// `twin.png`, both of the same red pixels and so of the same id,
    /// when `r` reloads it as `after` with `square.png` in `rgba`.
    fn twins_reloaded(after: &str, rgba: [u8; 4]) -> String {
        let red = [255, 0, 0, 255];
        let document = crate::parse::parse("![a](square.png)\n\n![b](twin.png)\n");
        let (_, mut inputs) = pictured(red);
        inputs.images.insert("twin.png", Ok(square(red)));
        let mut repaint = Repaint {
            document: crate::parse::parse(after),
            rgba,
        };
        let reload: &mut dyn Reload = &mut repaint;
        let mut terminal = Fake::new(60, 6, &[b"r"]);
        run(
            Content::Document(&document),
            None,
            "notes.md",
            &mut inputs,
            Options {
                reload: Some(reload),
                ..Options::new(SearchCase::Smart)
            },
            &mut terminal,
        )
        .unwrap();
        String::from_utf8(terminal.out).unwrap()
    }

    /// `out` before and after the end of its last frame, where the pager
    /// quits.
    fn until_quit(out: &str) -> (&str, &str) {
        let end = out.rfind("\x1b[?2026l").unwrap();
        out.split_at(end)
    }

    fn sorted(mut ids: Vec<u32>) -> Vec<u32> {
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    #[test]
    fn a_changed_image_keeps_the_id_another_placement_still_shows() {
        let out = twins_reloaded("![a](square.png)\n\n![b](twin.png)\n", [0, 0, 255, 255]);
        let sent = sent_ids(&out);
        // Both, of one id, and `square.png` anew.
        assert_eq!(sent.len(), 3, "{out:?}");
        assert_eq!(sent[0], sent[1], "{out:?}");
        assert_ne!(sent[1], sent[2], "{out:?}");
        // `twin.png` still shows the red one until the pager quits.
        let (running, quitting) = until_quit(&out);
        assert_eq!(deleted_ids(running), [0; 0], "{out:?}");
        assert_eq!(sorted(deleted_ids(quitting)), sorted(sent), "{out:?}");
    }

    #[test]
    fn a_layout_without_one_placement_keeps_the_id_the_other_shows() {
        let out = twins_reloaded("![b](twin.png)\n", [255, 0, 0, 255]);
        let sent = sent_ids(&out);
        assert_eq!(sent.len(), 2, "{out:?}");
        assert_eq!(sent[0], sent[1], "{out:?}");
        let (running, quitting) = until_quit(&out);
        assert_eq!(deleted_ids(running), [0; 0], "{out:?}");
        assert_eq!(deleted_ids(quitting), [sent[0]], "{out:?}");
    }

    #[test]
    fn new_cell_pixels_send_the_image_again_and_delete_the_old_one() {
        let (document, inputs) = pictured([255, 0, 0, 255]);
        let mut terminal = Fake::new(60, 6, &[b""]);
        // Wider cells, as after another font: 12 × 20 pixels. The image
        // is as large and covers as many cells, but they are others.
        terminal.sizes.push_back(WindowSize {
            width: 720,
            height: 120,
            ..window(60, 6)
        });
        let out = written(&document, inputs, &mut terminal, None);
        let sent = sent_ids(&out);
        assert_eq!(sent.len(), 2, "{out:?}");
        assert_ne!(sent[0], sent[1], "{out:?}");
        // The old one goes as the new layout's frame starts, before the
        // screen is cleared, and the new one on quitting.
        let frames = terminal.frames();
        let first = format!("\x1b_Ga=d,d=I,i={},q=2\x1b\\\x1b[H", sent[0]);
        assert!(frames[1].starts_with(&first), "{frames:#?}");
        assert_eq!(deleted_ids(&out), sent, "{out:?}");
    }

    /// The terminal colours of the fade tests: grey text on black, with
    /// black and yellow in the palette.
    fn known_colours() -> Capabilities {
        let mut caps = Capabilities::ansi(80);
        caps.truecolor = true;
        caps.foreground = Some(Color::Rgb(200, 200, 200));
        caps.background = Some(Color::Rgb(0, 0, 0));
        caps.palette[0] = Some(Color::Rgb(0, 0, 0));
        caps.palette[3] = Some(Color::Rgb(200, 200, 0));
        caps
    }

    /// The frames drawn on a terminal with `caps` for `reads`, then
    /// `empty` polls without keys.
    fn frames_with(caps: Capabilities, reads: &[&[u8]], empty: usize) -> Vec<String> {
        frames_of(&numbers(30), Inputs::new(caps), reads, empty)
    }

    /// The frames drawn for `document` with `inputs` for `reads`, then
    /// `empty` polls without keys.
    fn frames_of(
        document: &Document,
        mut inputs: Inputs,
        reads: &[&[u8]],
        empty: usize,
    ) -> Vec<String> {
        let mut reads = reads.to_vec();
        reads.extend(std::iter::repeat_n(b"".as_slice(), empty));
        let mut terminal = Fake::new(60, 6, &reads);
        run(
            Content::Document(document),
            None,
            "notes.md",
            &mut inputs,
            Options::new(SearchCase::Smart),
            &mut terminal,
        )
        .unwrap();
        terminal.frames()
    }

    /// The SGR codes of bold text in `fg` on `bg`, each as `r;g;b`.
    fn colours(fg: &str, bg: &str) -> Vec<String> {
        let mut codes = vec!["0".to_owned(), "1".to_owned()];
        codes.extend(format!("38;2;{fg};48;2;{bg}").split(';').map(str::to_owned));
        codes
    }

    #[test]
    fn a_jump_fades_the_match_from_its_own_colours_into_reverse_video() {
        let frames = frames_with(known_colours(), &[b"/line 2\r"], 40);
        // The jump, a frame each twentieth of a second after it until
        // three quarters of a second, and no more after the fade.
        assert_eq!(frames.len(), 17, "{frames:#?}");
        let current = |frame: &str| style_before(document_part(frame, 6), "Line 2\x1b");
        // Grey on black, the text's own, into black on grey.
        assert_eq!(current(&frames[1]), colours("200;200;200", "0;0;0"));
        assert_eq!(current(&frames[7]), colours("120;120;120", "80;80;80"));
        assert_eq!(current(&frames[15]), colours("13;13;13", "187;187;187"));
        assert_eq!(current(&frames[16]), ["0", "1", "7"]);
    }

    #[test]
    fn a_match_in_yellow_text_fades_into_its_reverse() {
        // The title of a warning is in palette colour 3, a yellow.
        let mut inputs = Inputs::new(known_colours());
        inputs.theme.alerts.colors = crate::theme::AlertColors::Terminal;
        let document = crate::parse::parse("> [!WARNING]\n> Mind the step.\n");
        let frames = frames_of(&document, inputs, &[b"/warning\r"], 40);
        assert_eq!(frames.len(), 17, "{frames:#?}");
        let current = |frame: &str| style_before(document_part(frame, 6), "Warning\x1b");
        // Yellow on the alert's yellow tint at the jump, into the tint on
        // yellow.
        assert_eq!(current(&frames[1]), colours("200;200;0", "30;30;0"));
        assert_eq!(current(&frames[7]), colours("132;132;0", "98;98;0"));
        assert_eq!(current(&frames[15]), colours("41;41;0", "189;189;0"));
        let end = current(&frames[16]);
        assert!(end.contains(&"7".to_owned()), "{end:?}");
    }

    #[test]
    fn n_and_shift_n_fade_the_match_too() {
        let frames = frames_with(known_colours(), &[b"/line 2\r", b"n"], 0);
        let current = style_before(document_part(&frames[2], 6), "Line 2\x1b[0m0");
        assert_eq!(current, colours("200;200;200", "0;0;0"), "{frames:#?}");
    }

    #[test]
    fn colours_not_known_as_rgb_mark_the_match_at_once() {
        // The terminal tells no colours: the current match's reverse
        // video at once, and no frame after it.
        let frames = frames_with(Capabilities::ansi(80), &[b"/line 2\r"], 30);
        assert_eq!(frames.len(), 2, "{frames:#?}");
        let current = |frame: &str| style_before(document_part(frame, 6), "Line 2\x1b");
        assert_eq!(current(&frames[1]), ["0", "1", "7"]);
        // Nor without colours.
        let mut caps = known_colours();
        caps.color = false;
        let frames = frames_with(caps, &[b"/line 2\r"], 30);
        assert_eq!(frames.len(), 2, "{frames:#?}");
        assert_eq!(current(&frames[1]), ["0", "1", "7"]);
    }

    #[test]
    fn going_around_an_end_fades_the_match_number_from_the_bar_s_reverse() {
        // A grey bar, mixed from the black page and this grey.
        let mut caps = known_colours();
        caps.palette[8] = Some(Color::Rgb(100, 100, 100));
        let frames = frames_with(caps, &[b"/line 2\r", b"N"], 40);
        // The search, then N, which fades for three quarters of a second.
        assert_eq!(frames.len(), 18, "{frames:#?}");
        let number = |frame: &str| style_before(&status_row(frame, 6), "11\x1b");
        let colours = |fg: &str, bg: &str| {
            let mut codes = vec!["0".to_owned()];
            codes.extend(format!("38;2;{fg};48;2;{bg}").split(';').map(str::to_owned));
            codes
        };
        // Dark grey on grey back into the bar's grey on dark grey.
        assert_eq!(number(&frames[2]), colours("50;50;50", "200;200;200"));
        assert_eq!(number(&frames[8]), colours("110;110;110", "140;140;140"));
        assert_eq!(number(&frames[16]), colours("190;190;190", "60;60;60"));
        // At the end, in the bar's colours, with the rest of the search.
        let status = status_row(&frames[17], 6);
        assert_eq!(
            style_before(&status, "/line 2 11/11"),
            ["0", "48", "2", "50", "50", "50"]
        );
    }

    #[test]
    fn going_around_an_end_flashes_the_match_number_where_colours_are_not_known() {
        // Bold, without the bar's reverse video, for half a second.
        let frames = frames_with(Capabilities::ansi(80), &[b"/line 2\r", b"N"], 30);
        assert_eq!(frames.len(), 4, "{frames:#?}");
        let number = |frame: &str| style_before(&status_row(frame, 6), "11\x1b");
        assert_eq!(number(&frames[2]), ["0", "1"]);
        assert!(status_row(&frames[3], 6).contains(" 11/11"), "{frames:#?}");
    }

    /// Images loading in a test, each with what it comes as.
    type Coming =
        std::rc::Rc<std::cell::RefCell<Vec<(crate::images::Arrival, crate::images::Source)>>>;

    /// Reloads the document as `document`, with the images under `wanted`
    /// keys loading in the background, to come as their pixels, and the
    /// ones under `aside` set aside as no longer shown.
    struct Background {
        document: Document,
        wanted: Vec<(ImageKey, crate::images::Source)>,
        aside: Vec<ImageKey>,
        coming: Coming,
    }

    impl Reload for Background {
        fn reload(&mut self, images: &mut Images, loads: &mut Loads, _: &Capabilities) -> Reloaded {
            for key in &self.aside {
                images.set_aside(key);
            }
            for (key, image) in self.wanted.drain(..) {
                let arrival = loads.expect(key);
                self.coming.borrow_mut().push((arrival, image));
            }
            Reloaded::Document(self.document.clone())
        }

        fn change(&mut self) -> Change {
            Change::Unchanged
        }
    }

    /// The pixels of an image `width` × `height` in `rgba`.
    fn filled(width: u32, height: u32, rgba: [u8; 4]) -> crate::images::Source {
        crate::images::Source::Raster(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba(rgba),
        ))
    }

    /// A pager for kitty in a window of 60 × `rows` showing `before`,
    /// with the images in `images`, that reloads it on `r` as `reload`
    /// says, with the reads `reads`; the images loading come in order,
    /// one at each read numbered in `comes`.
    fn loading_in(
        rows: u16,
        before: &Document,
        images: Vec<(&str, crate::images::Source)>,
        reload: &mut Background,
        reads: &[&[u8]],
        comes: &[usize],
        follow: config::Follow,
    ) -> Fake {
        let caps = Capabilities::kitty(80);
        loading_with(caps, rows, before, images, reload, reads, comes, follow)
    }

    /// As [`loading_in`], on a terminal with `caps`.
    #[allow(clippy::too_many_arguments)]
    fn loading_with(
        caps: Capabilities,
        rows: u16,
        before: &Document,
        images: Vec<(&str, crate::images::Source)>,
        reload: &mut Background,
        reads: &[&[u8]],
        comes: &[usize],
        follow: config::Follow,
    ) -> Fake {
        let mut inputs = Inputs::new(caps);
        for (url, image) in images {
            inputs.images.insert(url, Ok(image));
        }
        let mut terminal = Fake::new(60, rows, reads);
        let coming = std::rc::Rc::clone(&reload.coming);
        let comes = comes.to_vec();
        terminal.hook = Some(Box::new(move |read| {
            if comes.contains(&read) {
                let (arrival, image) = coming.borrow_mut().remove(0);
                arrival.come(Ok(image));
            }
        }));
        run(
            Content::Document(before),
            None,
            "notes.md",
            &mut inputs,
            Options {
                reload: Some(reload),
                follow,
                ..Options::new(SearchCase::Smart)
            },
            &mut terminal,
        )
        .unwrap();
        terminal
    }

    /// A document with `above` lines above the image `url` and three
    /// below.
    fn around(above: usize, url: &str) -> Document {
        let mut source: String = (1..=above).map(|n| format!("Line {n}\n\n")).collect();
        source.push_str(&format!(
            "![an image]({url})\n\nAfter 1\n\nAfter 2\n\nAfter 3\n"
        ));
        crate::parse::parse(&source)
    }

    /// Reloads `around(2, "a.png")`, whose image is a red square, as
    /// `around(2, "b.png")` whose image is `b` loading.
    fn red_then(b: crate::images::Source) -> Background {
        Background {
            document: around(2, "b.png"),
            wanted: vec![(ImageKey::from("b.png"), b)],
            aside: vec![ImageKey::from("a.png")],
            coming: Default::default(),
        }
    }

    /// The text of the status line of `frame` in a window of `rows`.
    fn status_text(frame: &str, rows: u16) -> String {
        unstyled(&status_row(frame, rows))
    }

    #[test]
    fn a_reload_shows_the_new_text_at_once_and_the_image_when_it_comes() {
        let mut reload = red_then(filled(20, 20, [0, 0, 255, 255]));
        let red = filled(20, 20, [255, 0, 0, 255]);
        let terminal = loading_in(
            12,
            &around(2, "a.png"),
            vec![("a.png", red)],
            &mut reload,
            &[b"r", b"", b"", b""],
            &[3],
            config::Follow::Off,
        );
        let frames = terminal.frames();
        // The first frame, then the reload's, then the image's.
        assert_eq!(frames.len(), 4, "{frames:#?}");
        let reloaded = &frames[2];
        assert!(unstyled(reloaded).contains("After 2"), "{reloaded:?}");
        assert!(
            status_text(reloaded, 12).contains("[rendering: 0/1]"),
            "{reloaded:?}"
        );
        assert!(
            !status_text(reloaded, 12).contains("notes.md"),
            "{reloaded:?}"
        );
        let came = &frames[3];
        assert!(status_text(came, 12).contains("notes.md"), "{came:?}");
        assert!(!status_text(came, 12).contains("rendering"), "{came:?}");
        // Red, red dimmed and the band, then blue.
        let out = String::from_utf8(terminal.out.clone()).unwrap();
        let sent = sent_ids(&out);
        let unique = sorted(sent.clone());
        assert_eq!(unique.len(), 4, "{sent:?}");
        // The band went with the last placeholder, and the rest on quitting.
        assert_eq!(sorted(deleted_ids(&out)), unique, "{out:?}");
    }

    #[test]
    fn the_band_moves_on_at_each_frame_while_an_image_loads() {
        let mut reload = red_then(filled(20, 20, [0, 0, 255, 255]));
        let red = filled(20, 20, [255, 0, 0, 255]);
        let terminal = loading_in(
            12,
            &around(2, "a.png"),
            vec![("a.png", red)],
            &mut reload,
            &[b"r", b"", b"", b"", b"", b""],
            &[5],
            config::Follow::Off,
        );
        // Waits a frame's time while the image loads, and a poll's after.
        assert_eq!(terminal.waits[1..6], [FRAME; 5], "{:?}", terminal.waits);
        assert_eq!(terminal.waits[6], POLL, "{:?}", terminal.waits);
        let out = String::from_utf8(terminal.out.clone()).unwrap();
        let sent = sent_ids(&out);
        let band = sent[2];
        // Sent with the frame that shows it, then once after each of the
        // four reads before the image came.
        assert_eq!(sent.iter().filter(|&&id| id == band).count(), 5, "{sent:?}");
    }

    #[test]
    fn a_new_image_without_an_old_one_shows_its_alt_text_with_the_band_below() {
        let mut reload = Background {
            document: around(2, "b.png"),
            wanted: vec![(ImageKey::from("b.png"), filled(20, 20, [0, 0, 255, 255]))],
            aside: Vec::new(),
            coming: Default::default(),
        };
        let terminal = loading_in(
            12,
            &numbers(2),
            Vec::new(),
            &mut reload,
            &[b"r"],
            &[],
            config::Follow::Off,
        );
        let frames = terminal.frames();
        let reloaded = unstyled(&frames[2]);
        assert!(reloaded.contains("an image"), "{reloaded:?}");
        // The band's row of placeholders.
        assert_eq!(
            reloaded
                .matches(crate::kitty::graphics::PLACEHOLDER)
                .count(),
            56
        );
    }

    #[test]
    fn an_image_that_comes_keeps_the_top_line_in_place() {
        // Ten rows for one: below the top line, and above it.
        let short = crate::parse::parse("Line 1\n\nLine 2\n\n![an image](b.png)\n\nAfter\n");
        for (document, rows, keys, top) in [
            // At the end, which the view would follow down otherwise.
            (short, 6, b"G".as_slice(), ""),
            (around(2, "b.png"), 4, b"jjjjjj", "After 1"),
        ] {
            let mut reload = red_then(filled(20, 200, [0, 0, 255, 255]));
            reload.document = document;
            let red = filled(20, 20, [255, 0, 0, 255]);
            let terminal = loading_in(
                rows,
                &around(2, "a.png"),
                vec![("a.png", red)],
                &mut reload,
                &[keys, b"r", b"", b""],
                &[3],
                config::Follow::Off,
            );
            let frames = terminal.frames();
            let (before, after) = (&frames[frames.len() - 2], frames.last().unwrap());
            assert!(
                status_text(before, rows).contains("rendering"),
                "{frames:#?}"
            );
            assert!(
                !status_text(after, rows).contains("rendering"),
                "{frames:#?}"
            );
            assert_eq!(top_row(before), top, "{frames:#?}");
            assert_eq!(top_row(after), top, "{frames:#?}");
        }
    }

    #[test]
    fn auto_following_jumps_when_the_text_comes_and_not_when_the_image_does() {
        let before = numbers(30);
        let mut changed: String = (1..=30).map(|n| format!("Line {n}\n\n")).collect();
        changed = changed.replace("Line 20\n", "![changed](b.png)\n");
        let mut reload = Background {
            document: crate::parse::parse(&changed),
            wanted: vec![(ImageKey::from("b.png"), filled(20, 200, [0, 0, 255, 255]))],
            aside: Vec::new(),
            coming: Default::default(),
        };
        let terminal = loading_in(
            8,
            &before,
            Vec::new(),
            &mut reload,
            &[b"r", b"", b""],
            &[2],
            config::Follow::Auto,
        );
        let frames = terminal.frames();
        let top = |frame: &str| top_row(frame);
        let jumped = top(&frames[2]);
        assert_ne!(jumped, top(&frames[0]), "{frames:#?}");
        assert_eq!(top(frames.last().unwrap()), jumped, "{frames:#?}");
    }

    #[test]
    fn the_status_line_counts_the_images_that_came_until_all_have() {
        let mut reload = Background {
            document: crate::parse::parse("![a](a.png)\n\n![b](b.png)\n"),
            wanted: vec![
                (ImageKey::from("a.png"), filled(20, 20, [0, 0, 255, 255])),
                (ImageKey::from("b.png"), filled(20, 20, [0, 255, 0, 255])),
            ],
            aside: Vec::new(),
            coming: Default::default(),
        };
        let terminal = loading_in(
            12,
            &numbers(2),
            Vec::new(),
            &mut reload,
            &[b"r", b"", b"", b""],
            &[2, 3],
            config::Follow::Off,
        );
        let statuses: Vec<String> = (terminal.frames().iter())
            .map(|frame| status_text(frame, 12))
            .collect();
        // After the row's cursor move and kiri's name.
        let named = |status: &String| status.split_whitespace().nth(2).unwrap_or("").to_owned();
        let names: Vec<String> = statuses.iter().map(named).collect();
        assert_eq!(
            names,
            [
                "notes.md",
                "notes.md",
                "[rendering:",
                "[rendering:",
                "notes.md"
            ],
            "{statuses:#?}"
        );
        assert!(statuses[2].contains("[rendering: 0/2]"), "{statuses:#?}");
        assert!(statuses[3].contains("[rendering: 1/2]"), "{statuses:#?}");
    }

    #[test]
    fn the_tint_stays_on_its_lines_when_an_image_above_comes_taller() {
        let source = |changed: bool| {
            let mut source = String::from("Line 1\n\n![a](a.png)\n\n");
            for n in 2..=30 {
                match n == 25 && changed {
                    true => source.push_str("Line 25 changed\n\n"),
                    false => source.push_str(&format!("Line {n}\n\n")),
                }
            }
            crate::parse::parse(&source)
        };
        // The file of the image changed too, and loads again, taller.
        let mut reload = Background {
            document: source(true),
            wanted: vec![(ImageKey::from("a.png"), filled(20, 100, [0, 0, 255, 255]))],
            aside: vec![ImageKey::from("a.png")],
            coming: Default::default(),
        };
        let mut caps = known_colours();
        caps.graphics = true;
        caps.max_image_cells = crate::kitty::graphics::MAX_CELLS;
        let terminal = loading_with(
            caps,
            21,
            &source(false),
            vec![("a.png", filled(20, 20, [255, 0, 0, 255]))],
            &mut reload,
            &[b"r", b""],
            &[1],
            config::Follow::Auto,
        );
        let frames = terminal.frames();
        let tint = |frame: &str, text: &str| background(&row_style(frame, text));
        assert_eq!(frames.len(), 4, "{frames:#?}");
        for frame in &frames[2..] {
            assert_eq!(tint(frame, "Line 25 changed"), Some("30;30;30".to_owned()));
            assert_eq!(tint(frame, "Line 24"), None);
        }
        // The image came, above the view, which kept its place.
        assert!(status_text(&frames[2], 21).contains("[rendering: 0/1]"));
        assert!(status_text(&frames[3], 21).contains("notes.md"));
        let row = |frame: &str| row_of(frame, 21, "Line 25 changed");
        assert_eq!(row(&frames[3]), row(&frames[2]));
    }
}
