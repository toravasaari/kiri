//! The interactive pager: the document on a screen of its own, scrolled
//! with keys.
//!
//! This is the hub of the pager: the state of a pager run and the loop
//! that reads keys, applies reloads and loads, and draws. The features
//! live in the files beside it (search, links, bands, status, files and
//! the rest); this file ties them together and should not grow with them.
//!
//! A new feature does not go in here, and nor does a new key, a new
//! overlay or a new piece of state of one feature. Neither does file,
//! network or program access, which the pager reaches only through the
//! traits and handles it is given.
//!
//! A feature is a file of its own in the state engine (`src/session/`); a
//! key is a command there, in `src/session/commands.rs`. Work on files and programs belongs to the
//! platform (`src/main.rs` and the rest of `src/`), and bytes for the
//! terminal to the terminal layer (`src/present/`).

use std::io::{self, Write};
use std::ops::Range;
use std::time::{Duration, Instant, SystemTime};

use crate::images::{ImageError, ImageKey, Images, Loads, Part, Source};
use crate::present::encoder::Encoder;
use crate::present::keys::{Decoder, Key};
use crate::present::kitty::graphics::ImageId;
use crate::present::output::{self, Page, Writer};
use crate::present::{cell_query, clipboard};
use crate::render::Inputs;
use crate::session::commands::{self, Action};
use crate::session::fade::{
    FADE, FADE_OUT, Fade, HIGHLIGHT_FADE, HIGHLIGHT_SHARE, HIGHLIGHT_STAY, Rgb, STAY, WARNING_STAY,
};
use crate::session::files::Files;
use crate::session::help::Placed;
use crate::session::prompt::{Edit, Prompt};
use crate::session::search::{Marks, Search};
use crate::session::settings::{self, FollowJump, SearchCase};
use crate::session::view::{Motion, View};
use crate::session::{bands, changes, help, links, loading, search, status, tint, warnings};
use crate::term::{Screen, WindowSize};
use crate::typeset::block::BlockInfo;
use crate::typeset::caps::Capabilities;
use crate::typeset::compare::{Comparison, Fate, compare};
use crate::typeset::images::size::cell_size;
use crate::typeset::layout::{Content, Stream};
use crate::typeset::model::{Document, File, FrontMatterError};
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page::Wide;
use crate::typeset::style::{self, Line, Span, Style};
use crate::typeset::wrap;

/// The rows `lines` take on a terminal with `caps`.
pub fn rows(lines: &[Line], caps: &Capabilities) -> usize {
    lines
        .iter()
        .map(|line| usize::from(crate::present::output::rows(line, caps)))
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
    /// as [`crate::images::reload_in_background`] does. `reading` says
    /// whether reading mode is on now, which wants the images in the middle
    /// of text too, whatever the document was opened with.
    fn reload(
        &mut self,
        images: &mut Images,
        loads: &mut Loads,
        caps: &Capabilities,
        reading: bool,
    ) -> Reloaded;

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
pub struct Options<'a, 'l> {
    /// The columns the document is laid out in, when the window's are not
    /// to decide.
    pub width: Option<u16>,
    /// How a search treats case.
    pub case: SearchCase,
    /// Where `r` gets the document again from; `None` when it cannot.
    pub reload: Option<&'a mut dyn Reload>,
    /// Whether the pager follows the document from the start: reads it
    /// again whenever it changes.
    pub follow: settings::Follow,
    /// Which change following on auto moves the view to.
    pub jump: FollowJump,
    /// The warnings about the document as first read, which the pager
    /// tells as it does those of a reload.
    pub warnings: Vec<Warning>,
    /// Where the images of a reload load in the background, when the
    /// caller keeps them, so that it can wait for their programs once
    /// the pager has left its screen; the pager only stops them.
    /// Otherwise the pager keeps its own, and waits for their programs
    /// itself before it returns, after it deleted the images but with
    /// the screen still open.
    pub loads: Option<&'l mut Loads>,
    /// The reading mode that `M` turns on, when the document is shown
    /// without it; `M` turns reading mode off when it is on.
    pub reading: Option<Reading>,
    /// Starts loading what reading mode shows that the document does not
    /// without it, such as the images in the middle of text, when `M`
    /// turns it on: it is given the content shown, which is the document
    /// as it was last reloaded, the images, where the loads go and the
    /// terminal's capabilities, and starts only what is not held or
    /// loading already.
    pub inline: Option<InlineLoader<'a>>,
}

/// See [`Options::inline`].
pub type InlineLoader<'a> =
    Box<dyn FnMut(Content<'_>, &mut Images, &mut Loads, &Capabilities) + 'a>;

impl Options<'_, '_> {
    /// A window's width to decide the layout, searches that treat case as
    /// `case` says, and no way to reload.
    pub fn new(case: SearchCase) -> Self {
        Options {
            width: None,
            case,
            reload: None,
            follow: settings::Follow::Off,
            jump: FollowJump::First,
            warnings: Vec::new(),
            loads: None,
            reading: None,
            inline: None,
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
    mut options: Options,
    terminal: &mut impl Terminal,
) -> io::Result<()> {
    if let Some(loads) = options.loads.take() {
        return page_through(content, laid_out, name, inputs, options, loads, terminal);
    }
    let mut own = Loads::new();
    let result = page_through(content, laid_out, name, inputs, options, &mut own, terminal);
    own.halt();
    own.wait(crate::images::STOPPING);
    result
}

/// [`run`], with the images of reloads loading in `loads`, which `q`
/// stops without waiting for their programs.
#[expect(clippy::too_many_lines)]
fn page_through(
    content: Content,
    laid_out: Option<Stream>,
    name: &str,
    inputs: &mut Inputs,
    options: Options,
    loads: &mut Loads,
    terminal: &mut impl Terminal,
) -> io::Result<()> {
    let Options {
        width,
        case,
        reload,
        follow,
        jump,
        warnings,
        loads: _,
        reading,
        inline,
    } = options;
    let mut inline = inline;
    // The reading mode that `M` turns to, on or off.
    let mut other = reading.filter(|_| inputs.reading.is_none());
    let mut source = Origin {
        reload,
        loads,
        started: terminal.now(),
    };
    let page = Page {
        background: inputs.theme.background,
        foreground: inputs.theme.foreground,
    };
    let mut laid_out = laid_out.map(|lines| (inputs.caps.clone(), lines));
    let mut size = terminal.size();
    // The cells the window told last; only a change in them replaces the
    // cell size detection found.
    let mut window_cell = size.and_then(|size| size.cell_pixels());
    let mut view: Option<View> = None;
    let mut state = State::new(case);
    state.warn(warnings, terminal.now());
    if source.reload.is_some() {
        state.follow.mode = follow;
    } else if follow != settings::Follow::Off {
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
    // The cells the heading images were last loaded for.
    let mut headed = cell_size(&inputs.caps);
    loop {
        fit(&mut inputs.caps, size, width, &mut window_cell);
        if cell_size(&inputs.caps) != headed {
            headed = cell_size(&inputs.caps);
            let content = match &reloaded {
                Some(document) => Content::Document(document),
                None => content,
            };
            load_headings(content, inputs, source.loads);
        }
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
            _ => crate::typeset::layout::content(content, inputs),
        };
        let Stream { mut lines, blocks } = stream;
        // The text sizing protocol cannot draw part of a line, so a line
        // taller than the window is drawn at normal size.
        for line in &mut lines {
            if usize::from(crate::present::output::rows(line, &inputs.caps)) > rows {
                line.to_normal_size();
            }
        }
        let places: Vec<Option<BlockInfo>> = lines
            .iter()
            .map(|line| line.block.map(|id| blocks.get(id).clone()))
            .collect();
        // Reading mode's bands scrolled and selected as they were.
        state.bands.apply(&mut lines, &places);
        let band_places = places.clone();
        // The images the new lines do not place go with their first frame.
        encoder.keep(&lines, &inputs.caps);
        let mut writer = Writer::new(&inputs.images, &inputs.caps, page, &mut encoder);
        let heights: Vec<usize> = lines
            .iter()
            .map(|line| usize::from(writer.rows(line)))
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
                        let change = (state.follow.mode == settings::Follow::Auto)
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
        // The topmost band on the screen is selected before the first
        // frame, which then shows it so.
        if (state.bands).refresh(&lines, &band_places, shown.visible()) {
            state.bands.apply(&mut lines, &band_places);
        }
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
            reading: inputs.reading.is_some(),
        };
        let marks = Marks::new(&inputs.theme, &inputs.caps);
        let rgb = Rgb::new(&inputs.theme, &inputs.caps);
        let mut frame_layout = Layout {
            lines: &lines,
            places: &band_places,
            marks: &marks,
            rgb: rgb.as_ref(),
            columns: usize::from(inputs.caps.width),
            status: &status,
            view: &mut shown,
        };
        let outcome = show(
            &mut frame_layout,
            &mut state,
            &mut writer,
            terminal,
            size,
            inputs.caps.cell_pixels,
            &mut source,
        )?;
        // An answer that came in the same read as a key that ended the frame
        // is applied first, so the outcome that key asks for uses the new
        // cells.
        if let Some(cell) = state.cell.take() {
            state.changed = None;
            inputs.caps.cell_pixels = Some(cell);
        }
        match outcome {
            Outcome::Quit => {
                // The caller waits for the programs, once the screen is
                // left.
                source.loads.halt();
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
                    inputs.images.came(key, image);
                }
                keep_top = true;
                view = Some(shown);
            }
            Outcome::Resized(new) => {
                // The lines laid out for the new size are others.
                state.changed = None;
                size = new;
                view = Some(shown);
                // A new font size changes the cells, which the window does
                // not always tell: it tells none, or inside tmux it may
                // tell tmux's guess. The terminal's answer comes among the
                // keys; until then the cells known are used.
                let tells_pixels = new.is_some_and(|new| new.cell_pixels().is_some());
                if new.is_some() && (inputs.caps.tmux_passthrough || !tells_pixels) {
                    cell_query::ask(inputs.caps.tmux_passthrough, terminal)?;
                    terminal.flush()?;
                }
            }
            // Laid out again for the cells the terminal answered, as after
            // a resize.
            Outcome::Cell(cell) => {
                state.changed = None;
                inputs.caps.cell_pixels = Some(cell);
                view = Some(shown);
            }
            // Laid out again with wide blocks the other way, in reading
            // mode as it is and as `M` would turn it on, keeping the
            // block at the top as a resize does.
            Outcome::Wide => {
                for reading in [&mut inputs.reading, &mut other].into_iter().flatten() {
                    reading.wide = match reading.wide {
                        Wide::Wrap => Wide::Scroll,
                        Wide::Scroll => Wide::Wrap,
                    };
                }
                state.changed = None;
                view = Some(shown);
            }
            // The same lines, with the band shown anew.
            Outcome::Bands => {
                laid_out = Some((inputs.caps.clone(), Stream { lines, blocks }));
                keep_top = true;
                view = Some(shown);
            }
            // Laid out again as after a resize, which keeps the block at
            // the top.
            Outcome::Reading => {
                if inputs.caps.draws_images() {
                    std::mem::swap(&mut inputs.reading, &mut other);
                    state.changed = None;
                    // Turned on: what it shows that the text does not
                    // starts loading, and lays the page out again as it
                    // comes.
                    if let (Some(_), Some(start)) = (&inputs.reading, inline.as_mut()) {
                        start(content, &mut inputs.images, source.loads, &inputs.caps);
                    }
                } else {
                    state.told = Some(Told::new(NEEDS_IMAGES));
                }
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
                // The images in view start first, once the view is known.
                source.loads.hold();
                let reloaded_now = reload.reload(
                    &mut inputs.images,
                    source.loads,
                    &inputs.caps,
                    inputs.reading.is_some(),
                );
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
    /// The terminal answered that its cells are of this size, width and
    /// height in pixels, which are not the ones in use.
    Cell((u16, u16)),
    /// The document is to be read again: the reader asked for it, and the
    /// status line says it is being read, or it changed while followed.
    Reload,
    /// These images, loading in the background, came.
    Loaded(Vec<(ImageKey, Result<Source, ImageError>)>),
    /// Reading mode is to be turned on or off.
    Reading,
    /// A band is scrolled or selected otherwise: the same lines show it
    /// anew.
    Bands,
    /// Wide blocks are to switch between wrapping and scrolling.
    Wide,
}

/// Where the pager gets the document and its images again.
struct Origin<'r, 'l> {
    /// Where `r` gets the document again from; `None` when it cannot.
    reload: Option<&'r mut dyn Reload>,
    /// The images loading in the background after a reload.
    loads: &'l mut Loads,
    /// When the pager started, from when the bands below the images that
    /// load move.
    started: Instant,
}

/// Why `M` does not turn reading mode on.
const NEEDS_IMAGES: &str = "reading mode needs a terminal that draws images";

/// What a key did, besides what the pager does itself.
enum Pressed {
    Nothing,
    Quit,
    /// The reader asked for the document again.
    Reload,
    /// The reader changed how the document is followed.
    Follow,
    /// The reader asked for the screen and its images again.
    Redraw,
    /// The reader turned reading mode on or off.
    Reading,
    /// The reader scrolled a band sideways or selected another.
    Bands,
    /// The reader switched wide blocks between wrapping and scrolling.
    Wide,
    /// The reader asked for this to go on the clipboard.
    Copy(String),
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
    /// Whether the document is shown in reading mode.
    reading: bool,
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
        (fade, out, counted): (Option<Fade>, Option<Fade>, Option<Fade>),
        link: Option<&str>,
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
                let found = state.found(flash, fading, link);
                let file = state.file(&self.files, view.top());
                let name = file.map_or(self.name, |file| self.files.name(file));
                let name = self.loading.as_deref().unwrap_or(name);
                let place = (file.filter(|_| self.files.several()))
                    .map(|file| (file + 1, self.files.count()));
                let position = view.position();
                let tags = status::Tags {
                    follow: state.follow.mode,
                    reading: self.reading,
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

/// Starts loading the heading images of `content` for the cells `inputs`
/// now have, in `loads`, when headings show as images: those drawn for
/// other cells have their words on other columns.
fn load_headings(content: Content, inputs: &mut Inputs, loads: &mut Loads) {
    let Inputs {
        caps,
        theme,
        headings,
        ..
    } = &*inputs;
    let Some(look) = crate::render::heading_look(*headings, false, theme, caps) else {
        return;
    };
    let base = std::path::Path::new("");
    let part = |(number, document)| Part {
        document,
        number,
        base,
        by_line: true,
        inline_images: false,
    };
    let parts: Vec<Part> = match content {
        Content::Document(document) => vec![part((0, document))],
        Content::Files(files) => (files.iter().map(|file| &file.document))
            .enumerate()
            .map(part)
            .collect(),
    };
    crate::images::load_headings_in_background(&parts, &look, &mut inputs.images, loads);
}

/// `caps` for a window of `size`, `width` columns wide when it is given.
/// `window_cell` is the cells the window told before.
fn fit(
    caps: &mut Capabilities,
    size: Option<WindowSize>,
    width: Option<u16>,
    window_cell: &mut Option<(u16, u16)>,
) {
    let Some(size) = size else {
        return;
    };
    caps.width = width.unwrap_or(size.columns);
    caps.height = Some(size.rows).filter(|&rows| rows > 0);
    // A new font size changes the cells, when the window tells its pixels.
    // Cells that stay the same keep the size detection found, which inside
    // tmux may be the terminal's where the window has tmux's guess.
    let cell = size.cell_pixels();
    if cell != *window_cell {
        *window_cell = cell;
        if cell.is_some() {
            caps.cell_pixels = cell;
        }
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
    /// The selected link.
    links: links::Links,
    /// Whether a jump to a match scrolled or selected a band, which is
    /// shown anew once the key is done.
    bands_moved: bool,
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
    /// Where the focus is, as the terminal last told.
    focus: Focus,
    /// The keys read, with what a read cut off waiting for the rest,
    /// also when a layout ends between the two reads.
    decoder: Decoder,
    /// How far reading mode's bands are scrolled, and which is selected.
    bands: bands::Bands,
    /// The cells the terminal last answered, until `page_through` applies
    /// them when `show` returns.
    cell: Option<(u16, u16)>,
}

/// Where the focus is, as the terminal tells with focus reporting: under
/// tmux, whether the pane is in the window shown.
#[derive(Debug)]
enum Focus {
    /// Not told yet, as when kiri started in a pane not shown.
    Unknown,
    In,
    /// Gone, with the images sent since, which tmux may have dropped.
    Out(Vec<ImageId>),
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
    mode: settings::Follow,
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
        self.mode != settings::Follow::Off
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
            links: links::Links::default(),
            bands_moved: false,
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
            focus: Focus::Unknown,
            decoder: Decoder::default(),
            bands: bands::Bands::default(),
            cell: None,
        }
    }

    /// Notes that the images `sent` went to the terminal, which tmux
    /// may drop while the focus is gone.
    fn sent(&mut self, sent: Vec<ImageId>) {
        if let Focus::Out(away) = &mut self.focus {
            away.extend(sent);
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
        match self.pressed(key, now, layout) {
            Pressed::Nothing if std::mem::take(&mut self.bands_moved) => Pressed::Bands,
            pressed => pressed,
        }
    }

    fn pressed(&mut self, key: Key, now: Instant, layout: &mut Layout) -> Pressed {
        let mut action = commands::action(key);
        // A click and the sideways wheel act on the band under them, and
        // never reach the help, the prompt or the colon.
        if let Some(pointer) = action.filter(|action| action.is_pointer()) {
            match self.pointed(pointer, layout) {
                Ok(pressed) => return pressed,
                Err(fallback) => action = Some(fallback),
            }
        }
        // Any other mouse event does nothing at all.
        if key == Key::Mouse {
            return Pressed::Nothing;
        }
        self.nowhere = false;
        self.told = None;
        if action == Some(Action::Dismiss) {
            self.dismiss();
            return Pressed::Nothing;
        }
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
        if std::mem::take(&mut self.colon) {
            action = match key {
                Key::Char(c) => commands::action(Key::Command(c)),
                _ => return Pressed::Nothing,
            };
        } else if key == commands::COMMAND && layout.status.files.several() {
            self.colon = true;
            return Pressed::Nothing;
        }
        match action {
            Some(Action::Quit) => return Pressed::Quit,
            // Enter copies the link selected on the screen, and otherwise
            // goes down a line.
            Some(Action::Enter) => match self.link_on_screen(layout) {
                Some(link) => {
                    self.told = Some(Told::fading("link copied", now));
                    return Pressed::Copy(link.url);
                }
                None => layout.view.scroll(Motion::Down(1)),
            },
            Some(Action::Move(motion)) => layout.view.scroll(motion),
            // The help and the prompt need a row besides the status line's.
            Some(Action::Help) => self.help = layout.status.bar,
            Some(Action::Search) => self.prompt = layout.status.bar.then(Prompt::default),
            Some(Action::Next) => self.step(Search::forward, now, layout),
            Some(Action::Previous) => self.step(Search::backward, now, layout),
            Some(Action::NextFile) => self.step_file(true, layout),
            Some(Action::PreviousFile) => self.step_file(false, layout),
            Some(Action::Reload) => return Pressed::Reload,
            Some(Action::Redraw) => return Pressed::Redraw,
            Some(Action::Follow) => return Pressed::Follow,
            Some(Action::Reading) => return Pressed::Reading,
            Some(Action::Wide) => return Pressed::Wide,
            Some(Action::Sideways(step)) => {
                if self.bands.scroll(layout.lines, layout.places, step) {
                    return Pressed::Bands;
                }
            }
            Some(Action::SelectLink { back }) => {
                let visible = layout.view.visible();
                let (lines, places) = (layout.lines, layout.places);
                let scrolled =
                    (self.links).select_scrolling(&mut self.bands, lines, places, visible, back);
                self.bands_moved |= scrolled;
            }
            // Answered above, or in `pointed`.
            Some(Action::Dismiss | Action::Click { .. } | Action::Wheel { .. }) | None => {}
        }
        Pressed::Nothing
    }

    /// Answers Escape: closes what is open, the innermost first, and when
    /// nothing is open takes away the search's marks and the selected link.
    /// The jump mark, the colouring after a reload and `went` stay.
    fn dismiss(&mut self) {
        if self.help {
            self.help = false;
        } else if self.prompt.take().is_none() && !std::mem::take(&mut self.colon) {
            self.shown = false;
            self.links.clear();
        }
    }

    /// A click or a notch of the sideways wheel: the answer, or for the
    /// wheel with Shift away from a band the wheel's action. While the
    /// help or the prompt is open the mouse does nothing.
    fn pointed(&mut self, action: Action, layout: &Layout) -> Result<Pressed, Action> {
        let bands = |moved| match moved {
            true => Pressed::Bands,
            false => Pressed::Nothing,
        };
        match action {
            _ if self.help || self.prompt.is_some() => Ok(Pressed::Nothing),
            Action::Click { row } => {
                let line = Self::row_line(row, layout);
                Ok(bands(self.bands.click(layout.lines, layout.places, line)))
            }
            Action::Wheel { right, shift, row } => match self.bands.wheel(
                layout.lines,
                layout.places,
                Self::row_line(row, layout),
                right,
            ) {
                Some(moved) => Ok(bands(moved)),
                None if shift && right => Err(Action::Move(Motion::Down(commands::WHEEL))),
                None if shift => Err(Action::Move(Motion::Up(commands::WHEEL))),
                None => Ok(Pressed::Nothing),
            },
            action => Err(action),
        }
    }

    /// Moves to the start of the next file, or of the previous one.
    fn step_file(&mut self, next: bool, layout: &mut Layout) {
        let files = layout.status.files;
        let top = layout.view.top();
        let Some(file) = self.file(&files, top) else {
            return;
        };
        match next {
            true => self.go_to(files.next(file), "no next file", layout),
            false => self.go_to(files.previous(file, top), "no previous file", layout),
        }
    }

    /// The selected link, when it is on the screen and has an address to
    /// copy.
    fn link_on_screen(&self, layout: &Layout) -> Option<links::Found> {
        let visible = layout.view.visible();
        (self.links.selected_on(layout.lines, layout.places, visible))
            .filter(|link| !link.url.is_empty())
    }

    /// The line of the document on row `row` of the window, counted from 1.
    fn row_line(row: u16, layout: &Layout) -> Option<usize> {
        layout.view.line_at(usize::from(row).checked_sub(1)?)
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
        let line = search.current().map(|found| found.line);
        self.search = Some(search);
        match line {
            Some(line) => self.jump(line, now, false, layout),
            None => self.nowhere = true,
        }
        self.shown = true;
    }

    /// Shows the match on line `line`, jumped to at `now`, `around` an
    /// end of the document or not.
    fn jump(&mut self, line: usize, now: Instant, around: bool, layout: &mut Layout) {
        layout.view.reveal(line);
        self.jumped = Some(Jump { at: now, around });
        // A match in a band scrolls the band to it, and selects it.
        let band = (self.search.as_ref().and_then(Search::current))
            .and_then(|found| found.place.as_ref()?.band.clone());
        if let Some((_, x)) = band
            && self.bands.reveal(layout.lines, layout.places, line, x) == Some(true)
        {
            self.bands_moved = true;
        }
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
            *search = search.again(lines, self.case, top);
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
        link: Option<&links::Found>,
    ) -> Vec<Line> {
        let search = self.search.as_ref().filter(|_| self.shown);
        let mut marked: Vec<Line> = (first..)
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
                // The selected link's spans on the line, when it has some.
                let selected = (link.iter().flat_map(|link| &link.runs))
                    .find(|(at, _)| *at == index)
                    .map(|(_, spans)| spans.clone());
                match (search, selected) {
                    (Some(search), selected) => {
                        search::mark(line, &search.on(index), marks, fade, selected)
                    }
                    (None, Some(selected)) => search::mark(line, &[], marks, fade, Some(selected)),
                    (None, None) => line.clone(),
                }
            })
            .collect();
        // The matches in a page of reading mode and the selected link are
        // drawn into its image.
        for page in crate::typeset::reading::engine::pages(&marked) {
            let address = std::rc::Rc::as_ptr(&page.parts) as usize;
            let marks = search.map_or_else(Vec::new, |search| search.in_page(address));
            let link = (link.and_then(|link| link.link))
                .filter(|link| link.0 == address)
                .map(|link| link.1);
            if let Some(overlay) = page.parts.overlay(marks, link) {
                crate::typeset::reading::engine::set_overlay(
                    &mut marked[page.lines],
                    Some(&overlay),
                );
            }
        }
        marked
    }

    /// What the status line tells about the last key, in `fading` when
    /// it is given, or else about the last search, with the match number
    /// in `flash` when it is given.
    fn found<'a>(
        &'a self,
        flash: Option<Style>,
        fading: Option<Style>,
        link: Option<&'a str>,
    ) -> Option<status::Found<'a>> {
        if let Some(told) = &self.told {
            return Some(status::Found::Told {
                message: &told.message,
                fading,
            });
        }
        // Where the selected link goes.
        if let Some(url) = link {
            return Some(status::Found::Link(url));
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
    /// The blocks the lines come from.
    places: &'a [Option<BlockInfo>],
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
        let link = (state.links).selected_on(self.lines, self.places, visible.clone());
        Frame {
            lines: state.marked(
                &self.lines[visible.clone()],
                visible.start,
                self.marks,
                fade,
                self.rgb.and_then(|rgb| state.tint(now, rgb, self.columns)),
                link.as_ref(),
            ),
            status: self.status.spans(
                self.view,
                state,
                (fade, out, state.count_fade(now, self.rgb)),
                link.as_ref().map(|link| link.url.as_str()),
            ),
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

/// Keeps the last cell size answer among the keys a read still has behind
/// a key that ends the frame. The other keys behind it are dropped, but the
/// answer says what the terminal is, so the next layout applies it.
fn keep_last_answer(state: &mut State, rest: &[Key]) {
    let last = rest.iter().rev().find_map(|key| match key {
        Key::CellSize(width, height) => Some((*width, *height)),
        _ => None,
    });
    state.cell = last.or(state.cell);
}

/// Shows `layout` in `state` on `terminal`, whose window is of `size`, and
/// scrolls it until the reader quits or the window changes size, until
/// the document is to be read again from `source`, if there is one to
/// read it from, or until images loading there come.
#[expect(clippy::too_many_lines)]
fn show(
    layout: &mut Layout,
    state: &mut State,
    writer: &mut Writer,
    terminal: &mut impl Terminal,
    size: Option<WindowSize>,
    cell: Option<(u16, u16)>,
    source: &mut Origin,
) -> io::Result<Outcome> {
    let reload = &mut source.reload;
    // The images loading start nearest to the lines in view.
    let mut told = layout.view.visible();
    prefer(source.loads, layout.lines, told.clone());
    let mut now = terminal.now();
    let mut shown = layout.frame(state, now);
    writer.at(now.saturating_duration_since(source.started));
    shown.draw(layout.view.rows(), writer, terminal)?;
    state.sent(writer.check(terminal)?);
    terminal.flush()?;
    loop {
        let input = terminal.read(if layout.fades(state, now) || writer.has_bands() {
            FRAME
        } else {
            POLL
        })?;
        // Nothing more came for a key the last read cut off.
        let keys = if input.is_empty() {
            state.decoder.flush()
        } else {
            state.decoder.keys(&input)
        };
        now = terminal.now();
        state.tick(now);
        // Whether the frame is drawn again even if it has not changed.
        let mut again = false;
        // Whether a frame was drawn or images sent again, which goes out
        // with a check of the images it sent.
        let mut written = false;
        // Whether a key scrolled a band or selected another.
        let mut bands = false;
        let mut keys = keys.into_iter();
        while let Some(key) = keys.next() {
            match key {
                // tmux passed no images while the pane was not shown:
                // those sent since the focus went, or any when it is not
                // known when that was.
                Key::FocusIn if writer.through_tmux() => {
                    match std::mem::replace(&mut state.focus, Focus::In) {
                        Focus::Unknown => writer.lost(),
                        Focus::Out(sent) if !sent.is_empty() => writer.lost_some(&sent),
                        Focus::Out(_) | Focus::In => continue,
                    }
                    again = true;
                    continue;
                }
                // A second one, as from kitty and then tmux, keeps the
                // images sent while away.
                Key::FocusOut => {
                    if !matches!(state.focus, Focus::Out(_)) {
                        state.focus = Focus::Out(Vec::new());
                    }
                    continue;
                }
                Key::FocusIn | Key::Checked(_, true) => continue,
                // The last answer of the read is the cells now; the keys
                // after it still count.
                Key::CellSize(width, height) => {
                    state.cell = Some((width, height));
                    continue;
                }
                // tmux dropped it: its placeholders are on the screen.
                Key::Checked(id, false) => {
                    writer.send_again(id, terminal)?;
                    written = true;
                    continue;
                }
                _ => {}
            }
            match state.press(key, now, layout) {
                Pressed::Nothing => {}
                Pressed::Quit => {
                    keep_last_answer(state, keys.as_slice());
                    return Ok(Outcome::Quit);
                }
                Pressed::Reload if reload.is_none() => {
                    state.told = Some(Told::new("nothing to reload"));
                }
                // The reader sees that the pager is busy before it is.
                Pressed::Reload => {
                    state.told = Some(Told::new("reloading…"));
                    layout
                        .frame(state, now)
                        .draw(layout.view.rows(), writer, terminal)?;
                    state.sent(writer.check(terminal)?);
                    terminal.flush()?;
                    keep_last_answer(state, keys.as_slice());
                    return Ok(Outcome::Reload);
                }
                // As `less` repaints, also the images, which the
                // terminal may have lost.
                Pressed::Redraw => {
                    writer.lost();
                    again = true;
                }
                Pressed::Reading => {
                    keep_last_answer(state, keys.as_slice());
                    return Ok(Outcome::Reading);
                }
                // The keys after it in the same read still count, as
                // they would read one by one.
                Pressed::Bands => bands = true,
                Pressed::Wide => {
                    keep_last_answer(state, keys.as_slice());
                    return Ok(Outcome::Wide);
                }
                Pressed::Copy(text) => {
                    clipboard::copy(&text, writer.through_tmux(), terminal)?;
                    terminal.flush()?;
                }
                Pressed::Follow if reload.is_none() => {
                    state.told = Some(Told::new(NOTHING_TO_FOLLOW));
                }
                Pressed::Follow => {
                    state.follow = Follow {
                        mode: match state.follow.mode {
                            settings::Follow::Off => settings::Follow::On,
                            settings::Follow::On => settings::Follow::Auto,
                            settings::Follow::Auto => settings::Follow::Off,
                        },
                        ..Follow::default()
                    }
                }
            }
            state.moved(layout.view.top());
        }
        if let Some(new) = state.cell.take()
            && Some(new) != cell
        {
            return Ok(Outcome::Cell(new));
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
        if layout.view.visible() != told {
            told = layout.view.visible();
            prefer(source.loads, layout.lines, told.clone());
        }
        // The band selected scrolled away: the topmost on the screen takes
        // its place.
        let refreshed = (state.bands).refresh(layout.lines, layout.places, layout.view.visible());
        if bands || refreshed {
            return Ok(Outcome::Bands);
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
        // A band on the screen moves on with each frame drawn.
        if frame != shown || writer.has_bands() || again {
            shown = frame;
            writer.at(now.saturating_duration_since(source.started));
            shown.draw(layout.view.rows(), writer, terminal)?;
            written = true;
        }
        if written {
            state.sent(writer.check(terminal)?);
            terminal.flush()?;
        }
    }
}

/// Tells `loads` to load the images of `lines` nearest to the lines
/// `visible` first, and starts what it holds; with nothing loading,
/// there is nothing to order.
fn prefer(loads: &mut Loads, lines: &[Line], visible: Range<usize>) {
    let keys = match loads.is_empty() {
        true => Vec::new(),
        false => loading::nearest(lines, visible),
    };
    loads.prefer(keys);
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
mod tests;
