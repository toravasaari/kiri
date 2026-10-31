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
//! key is a command there, in `src/session/commands.rs`. Work on files and
//! programs belongs to the platform (`src/main.rs` and the rest of `src/`),
//! and bytes for the terminal to the terminal layer (`src/present/`).

use std::io::{self, Write};
use std::ops::Range;
use std::time::{Duration, Instant};

use crate::images::{ImageError, ImageKey, Images, Loads, Part, Source};
use crate::present::encoder::Encoder;
use crate::present::output::{self, Page, Writer};
use crate::present::{cell_query, clipboard};
use crate::render::Inputs;
use crate::session::fade::Rgb;
use crate::session::files::Files;
use crate::session::frame::{Frame, Layout, Status};
use crate::session::help::Placed;
use crate::session::search::Marks;
use crate::session::settings::{self, FollowJump, SearchCase};
use crate::session::state::{
    Change, Changed, FRAME, NEEDS_IMAGES, NOTHING_TO_FOLLOW, POLL, State, Told, Warning,
};
use crate::session::step::{Effect, Msg, Pager, Run, step};
use crate::session::view::View;
use crate::session::{changes, loading, status};
use crate::term::{Screen, WindowSize};
use crate::typeset::block::BlockInfo;
use crate::typeset::caps::Capabilities;
use crate::typeset::compare::{Comparison, Fate, compare};
use crate::typeset::images::size::cell_size;
use crate::typeset::layout::{Content, Stream};
use crate::typeset::model::{Document, File};
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page::Wide;
use crate::typeset::style::{self, Line, Span, Style};

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
        arrived: Vec::new(),
        window: None,
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
        let run = Run {
            can_reload: source.reload.is_some(),
            tmux: writer.through_tmux(),
            started: source.started,
        };
        let cells = inputs.caps.cell_pixels;
        let ended = {
            let mut pager = Pager::new(&mut state, &mut frame_layout, run, cells, terminal.now());
            show(&mut pager, &mut writer, terminal, size, &mut source)?
        };
        // An answer that came in the same read as a key that ended the frame
        // is applied first, so the outcome that key asks for uses the new
        // cells.
        if let Some(cell) = state.cell.take() {
            state.changed = None;
            inputs.caps.cell_pixels = Some(cell);
        }
        match ended {
            Effect::Quit => {
                // The caller waits for the programs, once the screen is
                // left.
                source.loads.halt();
                encoder.delete_all(terminal)?;
                terminal.flush()?;
                return Ok(());
            }
            Effect::Loaded => {
                // The tinted lines move with the image that came.
                if let Some(changed) = state.changed.take() {
                    let blocks = (lines.get(changed.lines).unwrap_or_default().iter())
                        .filter_map(|line| line.block.map(|id| blocks.get(id).clone()))
                        .collect();
                    tinted = Some((changed.at, blocks));
                }
                for (key, image) in source.arrived.drain(..) {
                    inputs.images.came(key, image);
                }
                keep_top = true;
                view = Some(shown);
            }
            Effect::Resized => {
                let new = source.window;
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
            Effect::Cells(cell) => {
                state.changed = None;
                inputs.caps.cell_pixels = Some(cell);
                view = Some(shown);
            }
            // Laid out again with wide blocks the other way, in reading
            // mode as it is and as `M` would turn it on, keeping the
            // block at the top as a resize does.
            Effect::Wide => {
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
            Effect::Bands => {
                laid_out = Some((inputs.caps.clone(), Stream { lines, blocks }));
                keep_top = true;
                view = Some(shown);
            }
            // Laid out again as after a resize, which keeps the block at
            // the top.
            Effect::Reading => {
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
            Effect::Reload => {
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
            _ => unreachable!("a layout ends with the effects that end it"),
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

/// The rows of a window whose size the terminal does not tell.
const DEFAULT_ROWS: u16 = 24;

/// Where the pager gets the document and its images again.
struct Origin<'r, 'l> {
    /// Where `r` gets the document again from; `None` when it cannot.
    reload: Option<&'r mut dyn Reload>,
    /// The images loading in the background after a reload.
    loads: &'l mut Loads,
    /// When the pager started, from when the bands below the images that
    /// load move.
    started: Instant,
    /// The loads that came and wait for the layout to end, as the images
    /// are in use until then.
    arrived: Vec<(ImageKey, Result<Source, ImageError>)>,
    /// The size the window was last found to be of.
    window: Option<WindowSize>,
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

/// Draws `frame` above a status line on the row below `rows`.
fn draw_frame(
    frame: &Frame,
    rows: usize,
    writer: &mut Writer,
    out: &mut impl Write,
) -> io::Result<()> {
    draw(
        &frame.lines,
        rows,
        &frame.status,
        &frame.notice,
        &frame.panel,
        writer,
        out,
    )
}

/// Shows `pager`'s layout on `terminal`, whose window is of `size`, until
/// the layout ends: it waits for a message, steps the pager with it and
/// carries out the effects, one after the other, and tells the effect that
/// ended it.
fn show(
    pager: &mut Pager,
    writer: &mut Writer,
    terminal: &mut impl Terminal,
    size: Option<WindowSize>,
    source: &mut Origin,
) -> io::Result<Effect> {
    let mut msg = Msg::Shown(terminal.now());
    loop {
        let mut next = None;
        for effect in step(pager, msg) {
            if effect.ends() {
                return Ok(effect);
            }
            if let Some(reply) = carry(effect, pager, writer, terminal, size, source)? {
                next = Some(reply);
            }
        }
        msg = next.expect("an effect that asks for the next message");
    }
}

/// Carries out `effect` for `pager` on `terminal`, and gives the message
/// it asks for, if it does.
fn carry(
    effect: Effect,
    pager: &mut Pager,
    writer: &mut Writer,
    terminal: &mut impl Terminal,
    size: Option<WindowSize>,
    source: &mut Origin,
) -> io::Result<Option<Msg>> {
    match effect {
        Effect::Read { fading } => {
            let wait = if fading || writer.has_bands() {
                FRAME
            } else {
                POLL
            };
            let bytes = terminal.read(wait)?;
            return Ok(Some(Msg::Input(bytes, terminal.now())));
        }
        Effect::PollChange => {
            let reload = source.reload.as_deref_mut();
            let change = reload.expect("a source to poll").change();
            return Ok(Some(Msg::Changed(change)));
        }
        Effect::PollLoads => {
            source.arrived.extend(source.loads.poll());
            let came = !source.arrived.is_empty();
            return Ok(Some(Msg::Loads { came }));
        }
        Effect::PollWindow => {
            source.window = terminal.size();
            let changed = source.window != size;
            let bands = writer.has_bands();
            return Ok(Some(Msg::Window { changed, bands }));
        }
        Effect::Prefer(visible) => prefer(source.loads, pager.layout.lines, visible),
        Effect::Draw(frame, at) => {
            if let Some(at) = at {
                writer.at(at);
            }
            draw_frame(&frame, pager.layout.view.rows(), writer, terminal)?;
            pager.state.sent(writer.check(terminal)?);
            terminal.flush()?;
        }
        Effect::Check => {
            pager.state.sent(writer.check(terminal)?);
            terminal.flush()?;
        }
        Effect::Resend(id) => writer.send_again(id, terminal)?,
        Effect::Lose => writer.lost(),
        Effect::LoseSome(sent) => writer.lost_some(&sent),
        Effect::Copy(text) => {
            clipboard::copy(&text, writer.through_tmux(), terminal)?;
            terminal.flush()?;
        }
        Effect::Redraw
        | Effect::Quit
        | Effect::Reload
        | Effect::Reading
        | Effect::Wide
        | Effect::Cells(_)
        | Effect::Bands
        | Effect::Loaded
        | Effect::Resized => unreachable!("an effect that ends the layout, or one a key makes"),
    }
    Ok(None)
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
