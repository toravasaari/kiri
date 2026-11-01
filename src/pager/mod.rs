//! The interactive pager: the document on a screen of its own, scrolled
//! with keys.
//!
//! This is the platform's side of the pager: the loop. It waits for what
//! comes in, such as keys, the answer of a poll or a reload's result, hands
//! it to the state engine's step (`src/session/step.rs`) as a message, and
//! carries out the effects the step asks for: it reads the terminal,
//! draws, polls the document, the loads and the window, reads the
//! document again and makes the layout again.
//!
//! A decision does not go in here, nor a new key, overlay or piece of
//! state of a feature: what a message asks for is decided in the step.
//! File, network or program access the pager reaches only through the
//! traits and handles it is given.
//!
//! Features and keys are in the state engine (`src/session/`), and bytes
//! for the terminal in the terminal layer (`src/present/`).

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
use crate::session::state::{Backgrounds, Change, NOTHING_TO_FOLLOW, State, Told};
use crate::session::step::{Effect, Msg, Pager, Reply, Resized, Run, step};
use crate::session::view::View;
use crate::session::{loading, sources::Session, status};
use crate::term::{Screen, WindowSize};
use crate::typeset::block::BlockInfo;
use crate::typeset::caps::Capabilities;
use crate::typeset::compare::compare;
use crate::typeset::images::size::cell_size;
use crate::typeset::layout::{Content, Stream};
use crate::typeset::model::Document;
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
    Warned(Document, Vec<crate::typeset::diagnostic::Diagnostic>),
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
    /// Whether `B` paints the terminal's background one shade off.
    pub background_offset: bool,
    /// Where `r` gets the document again from; `None` when it cannot.
    pub reload: Option<&'a mut dyn Reload>,
    /// Whether the pager follows the document from the start: reads it
    /// again whenever it changes.
    pub follow: settings::Follow,
    /// Which change following on auto moves the view to.
    pub jump: FollowJump,
    /// The warnings about the document as first read, which the pager
    /// tells as it does those of a reload.
    pub warnings: Vec<crate::typeset::diagnostic::Diagnostic>,
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
            background_offset: true,
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
/// The status line names a lone document and each of several files after
/// their sources in `session`, which is given back as it ended. `laid_out`,
/// when given, is `content` laid out with `inputs` as they are now, used
/// unless the window calls for another. `session` must have a source for
/// each file of `content`, or one at least for a document.
/// A search treats case as `options.case` says.
pub fn run(
    content: Content,
    laid_out: Option<Stream>,
    session: Session,
    inputs: &mut Inputs,
    mut options: Options,
    terminal: &mut impl Terminal,
) -> io::Result<Session> {
    session.assert_fits(content);
    if let Some(loads) = options.loads.take() {
        return page_through(content, laid_out, session, inputs, options, loads, terminal);
    }
    let own = &mut Loads::new();
    let result = page_through(content, laid_out, session, inputs, options, own, terminal);
    own.halt();
    own.wait(crate::images::STOPPING);
    result
}

/// [`run`], with the images of reloads loading in `loads`, which `q`
/// stops without waiting for their programs.
fn page_through(
    content: Content,
    laid_out: Option<Stream>,
    session: Session,
    inputs: &mut Inputs,
    options: Options,
    loads: &mut Loads,
    terminal: &mut impl Terminal,
) -> io::Result<Session> {
    let Options {
        width,
        case,
        background_offset,
        reload,
        follow,
        jump,
        warnings,
        loads: _,
        reading,
        inline,
    } = options;
    let mut state = State::new(case);
    state.jump = jump;
    let (theme, reported) = (inputs.theme.background, inputs.caps.background);
    state.background = Backgrounds::new(theme, reported, background_offset);
    state.warn(warnings, terminal.now());
    if reload.is_some() {
        state.follow.mode = follow;
    } else if follow != settings::Follow::Off {
        state.told = Some(Told::new(NOTHING_TO_FOLLOW));
    }
    let size = terminal.size();
    let mut driver = Driver {
        content,
        reloaded: None,
        laid_out: laid_out.map(|lines| (inputs.caps.clone(), lines)),
        state,
        view: None,
        size,
        // The cells the window told last; only a change in them replaces
        // the cell size detection found.
        window_cell: size.and_then(|size| size.cell_pixels()),
        // The cells the heading images were last loaded for.
        headed: cell_size(&inputs.caps),
        width,
        session,
        tools: Tools {
            // The reading mode that `M` turns to, on or off.
            other: reading.filter(|_| inputs.reading.is_none()),
            inputs,
            source: Origin {
                reload,
                loads,
                started: terminal.now(),
                arrived: Vec::new(),
                window: None,
            },
            inline,
            // What the terminal holds of the images, kept from one layout
            // to the next.
            encoder: Encoder::new(),
            terminal,
        },
    };
    while driver.round()? {}
    Ok(driver.session)
}

/// What the loop reaches to carry out effects: the terminal, the inputs of
/// the layout, the document's source and the loads.
struct Tools<'a, 'r, 'l, T> {
    inputs: &'a mut Inputs,
    terminal: &'a mut T,
    source: Origin<'r, 'l>,
    encoder: Encoder,
    /// The reading mode that `M` turns to, on or off. An exception, not a
    /// model: a key's state goes in the state engine, which sends what to
    /// set as an effect, as `B` does with `Effect::SetBackground`.
    other: Option<Reading>,
    inline: Option<InlineLoader<'r>>,
}

/// The loop: layouts of the document, one after the other, each shown until
/// an effect ends it.
struct Driver<'a, 'r, 'l, T> {
    tools: Tools<'a, 'r, 'l, T>,
    content: Content<'a>,
    /// The document as reloaded, once it has been.
    reloaded: Option<Document>,
    /// A layout of the content that is still good, with the caps it is for.
    laid_out: Option<(Capabilities, Stream)>,
    state: State,
    view: Option<View>,
    /// The window's size as last found.
    size: Option<WindowSize>,
    /// The cells the window told last.
    window_cell: Option<(u16, u16)>,
    headed: (u16, u16),
    width: Option<u16>,
    session: Session,
}

/// How a layout ended.
#[derive(Default)]
struct Closed {
    quit: bool,
    /// Whether the next layout has the same lines.
    reuse: bool,
    /// The document read again, if it was.
    document: Option<Document>,
}

/// What is laid out: the document as reloaded when it has been, and
/// otherwise `content`.
fn shown_content<'a>(content: Content<'a>, reloaded: &'a Option<Document>) -> Content<'a> {
    reloaded.as_ref().map_or(content, Content::Document)
}

impl<T: Terminal> Driver<'_, '_, '_, T> {
    /// Lays the document out and shows it until it ends; whether to go on
    /// with another layout.
    fn round(&mut self) -> io::Result<bool> {
        self.refit();
        let window = usize::from(self.tools.inputs.caps.height.unwrap_or(DEFAULT_ROWS));
        // The bottom row is the status line's, unless the document would
        // have no row left.
        let bar = window > 1;
        let rows = if bar { window - 1 } else { window };
        let Stream { mut lines, blocks } = self.stream(rows);
        let content = shown_content(self.content, &self.reloaded);
        let inputs = &mut *self.tools.inputs;
        let places: Vec<Option<BlockInfo>> = lines
            .iter()
            .map(|line| line.block.map(|id| blocks.get(id).clone()))
            .collect();
        // Reading mode's bands scrolled and selected as they were.
        self.state.bands.apply(&mut lines, &places);
        let band_places = places.clone();
        // The images the new lines do not place go with their first frame.
        self.tools.encoder.keep(&lines, &inputs.caps);
        let page = Page {
            background: inputs.theme.background,
            foreground: inputs.theme.foreground,
        };
        let mut writer = Writer::new(&inputs.images, &inputs.caps, page, &mut self.tools.encoder);
        let heights: Vec<usize> = lines
            .iter()
            .map(|line| usize::from(writer.rows(line)))
            .collect();
        self.state.retint(&places);
        let tintable = Rgb::new(&inputs.theme, &inputs.caps)
            .is_some_and(|rgb| rgb.tinted(None, 0.0).is_some())
            && inputs.caps.styling
            && inputs.caps.truecolor;
        let now = self.tools.terminal.now();
        let mut shown =
            (self.state).view_for(self.view.take(), heights, places, rows, (tintable, now));
        // The topmost band on the screen is selected before the first
        // frame, which then shows it so.
        if (self.state.bands).refresh(&lines, &band_places, shown.visible()) {
            self.state.bands.apply(&mut lines, &band_places);
        }
        self.state.lay_out(&lines, shown.top(), bar);
        let sources = self.session.sources();
        let (name, shown_files) = match self.content {
            Content::Document(_) => (sources[0].name(), &[][..]),
            Content::Files(_) => ("", sources),
        };
        let loading = (self.tools.source.loads.progress())
            .map(|(done, total)| format!("[rendering: {done}/{total}]"));
        let files = Files::new(shown_files, &lines, &blocks);
        let status = status_for(name, loading, files, inputs, (self.size, bar));
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
            can_reload: self.tools.source.reload.is_some(),
            tmux: writer.through_tmux(),
            started: self.tools.source.started,
            draws_images: inputs.caps.draws_images(),
            reading: self.tools.other.is_some() || inputs.reading.is_some(),
            inline: self.tools.inline.is_some(),
        };
        let cells = inputs.caps.cell_pixels;
        let mut pager = Pager::new(&mut self.state, &mut frame_layout, run, cells, now);
        let ended = show(
            &mut pager,
            &mut writer,
            self.tools.terminal,
            self.size,
            &mut self.tools.source,
        )?;
        let closed = self.tools.close(&mut pager, content, ended)?;
        if closed.quit {
            return Ok(false);
        }
        if let Some(document) = closed.document {
            self.reloaded = Some(document);
            // Only a lone file is read again, so its source is the first.
            self.session.reload(self.session.sources()[0].id());
        }
        if closed.reuse {
            self.laid_out = Some((self.tools.inputs.caps.clone(), Stream { lines, blocks }));
        }
        if let Some(new) = self.tools.source.window.take() {
            self.size = new;
        }
        self.view = Some(shown);
        Ok(true)
    }

    /// The lines of the document for a window with `rows` for them: the
    /// layout kept when it is still good, and otherwise a new one.
    fn stream(&mut self, rows: usize) -> Stream {
        let inputs = &*self.tools.inputs;
        let content = shown_content(self.content, &self.reloaded);
        let mut stream = match self.laid_out.take() {
            Some((caps, stream)) if caps == inputs.caps => stream,
            _ => crate::typeset::layout::content(content, inputs),
        };
        // The text sizing protocol cannot draw part of a line, so a line
        // taller than the window is drawn at normal size.
        for line in &mut stream.lines {
            if usize::from(crate::present::output::rows(line, &inputs.caps)) > rows {
                line.to_normal_size();
            }
        }
        stream
    }

    /// Fits the caps to the window, and loads the heading images again
    /// when the cells they were drawn for are not the ones now.
    fn refit(&mut self) {
        let inputs = &mut *self.tools.inputs;
        fit(
            &mut inputs.caps,
            self.size,
            self.width,
            &mut self.window_cell,
        );
        if cell_size(&inputs.caps) != self.headed {
            self.headed = cell_size(&inputs.caps);
            let content = shown_content(self.content, &self.reloaded);
            load_headings(content, inputs, self.tools.source.loads);
        }
    }
}

impl<T: Terminal> Tools<'_, '_, '_, T> {
    /// Carries out the effects that end a layout of `content`, which is
    /// what it showed, and those that follow what they find out.
    fn close(
        &mut self,
        pager: &mut Pager,
        content: Content,
        mut effects: Vec<Effect>,
    ) -> io::Result<Closed> {
        let mut closed = Closed::default();
        loop {
            let mut onward = None;
            for effect in effects {
                match effect {
                    Effect::Quit => {
                        // The caller waits for the programs, once the
                        // screen is left.
                        self.source.loads.halt();
                        self.encoder.delete_all(self.terminal)?;
                        self.terminal.flush()?;
                        closed.quit = true;
                        return Ok(closed);
                    }
                    Effect::Relayout { reuse } => {
                        closed.reuse = reuse;
                        return Ok(closed);
                    }
                    Effect::Reload => {
                        let (reply, now, document) = self.reload(content);
                        closed.document = document.or(closed.document);
                        onward = Some(step(pager, Msg::Reloaded(reply, now)));
                        break;
                    }
                    effect => self.close_with(effect, content)?,
                }
            }
            effects = onward.expect("an effect that ends the layout");
        }
    }

    /// Carries out `effect`, one of those that end a layout, which needs
    /// nothing of the document's source.
    fn close_with(&mut self, effect: Effect, content: Content) -> io::Result<()> {
        let inputs = &mut *self.inputs;
        match effect {
            Effect::SetCells(cell) => inputs.caps.cell_pixels = Some(cell),
            Effect::AskCells => {
                cell_query::ask(inputs.caps.tmux_passthrough, self.terminal)?;
                self.terminal.flush()?;
            }
            // Laid out again with wide blocks the other way, in reading
            // mode as it is and as `M` would turn it on.
            Effect::FlipWide => {
                for reading in [&mut inputs.reading, &mut self.other].into_iter().flatten() {
                    reading.wide = match reading.wide {
                        Wide::Wrap => Wide::Scroll,
                        Wide::Scroll => Wide::Wrap,
                    };
                }
            }
            Effect::SwapReading => std::mem::swap(&mut inputs.reading, &mut self.other),
            Effect::SetBackground(background) => inputs.theme.background = background,
            Effect::StartInline => {
                if let Some(start) = self.inline.as_mut() {
                    start(content, &mut inputs.images, self.source.loads, &inputs.caps);
                }
            }
            Effect::Install => {
                for (key, image) in self.source.arrived.drain(..) {
                    inputs.images.came(key, image);
                }
            }
            _ => unreachable!("an effect that is not one of those that end a layout"),
        }
        Ok(())
    }

    /// Reads the document again, and brings the images up to date for it:
    /// what came of it, when, and the document if it was read.
    fn reload(&mut self, content: Content) -> (Reply, Instant, Option<Document>) {
        // The reload may change the pixels of the images.
        self.encoder.changed();
        let inputs = &mut *self.inputs;
        let reload = (self.source.reload.as_deref_mut()).expect("a source to reload from");
        let now = self.terminal.now();
        let before = inputs.images.take_placeholders();
        // The images in view start first, once the view is known.
        self.source.loads.hold();
        let reloaded_now = reload.reload(
            &mut inputs.images,
            self.source.loads,
            &inputs.caps,
            inputs.reading.is_some(),
        );
        let replaced = inputs.images.take_replaced();
        let (document, warnings) = match reloaded_now {
            Reloaded::Document(document) => (document, Vec::new()),
            Reloaded::Warned(document, warnings) => (document, warnings),
            Reloaded::Failed(why) => {
                for (key, placeholder) in before {
                    if self.source.loads.loading(&key) {
                        inputs.images.wait(key, placeholder);
                    }
                }
                return (Reply::Failed(why), now, None);
            }
        };
        let mut changes = None;
        if let Content::Document(old) = content {
            let comparison = compare(old, &document);
            let left = loading::Reloaded {
                old,
                new: &document,
                changes: &comparison,
                before: &before,
                replaced: &replaced,
                images: &inputs.images,
                diagram_look: crate::typeset::images::diagram_look_for(
                    &inputs.theme,
                    &inputs.caps,
                    inputs.diagram_panel,
                ),
            };
            let keys = self.source.loads.keys().cloned();
            for (key, placeholder) in left.placeholders(keys) {
                inputs.images.wait(key, placeholder);
            }
            changes = Some(comparison);
        }
        (Reply::Read { warnings, changes }, now, Some(document))
    }
}

/// The status line and the panels' look for `inputs`, in a window of `size`
/// that has a row for the status line when `bar`, calling a lone document
/// `name`; `loading` stands in place of the name while images load.
fn status_for<'a>(
    name: &'a str,
    loading: Option<String>,
    files: Files<'a>,
    inputs: &Inputs,
    (size, bar): (Option<WindowSize>, bool),
) -> Status<'a> {
    let panel = Style {
        fg: inputs.theme.foreground,
        bg: inputs.theme.background,
        ..Style::default()
    };
    Status {
        name,
        loading,
        files,
        styles: status::Styles::new(&inputs.theme, &inputs.caps),
        width: size.map_or(inputs.caps.width, |size| size.columns),
        bar,
        muted: style::muted(&inputs.theme, panel.clone()),
        red: Style {
            fg: inputs.theme.error,
            ..panel.clone()
        },
        panel,
        reading: inputs.reading.is_some(),
    }
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
    /// The size the window was found to be of, when it changed.
    window: Option<Option<WindowSize>>,
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
/// carries out the effects, one after the other. It gives the effects that
/// end the layout, which the caller carries out.
fn show(
    pager: &mut Pager,
    writer: &mut Writer,
    terminal: &mut impl Terminal,
    size: Option<WindowSize>,
    source: &mut Origin,
) -> io::Result<Vec<Effect>> {
    let mut msg = Msg::Shown(terminal.now());
    loop {
        let mut next = None;
        let mut effects = step(pager, msg).into_iter();
        while let Some(effect) = effects.next() {
            if effect.ends() {
                return Ok(std::iter::once(effect).chain(effects).collect());
            }
            if let Some(reply) = carry(effect, pager, writer, terminal, (size, &mut *source))? {
                next = Some(reply);
            }
        }
        msg = next.expect("an effect that asks for the next message");
    }
}

/// Carries out `effect` for `pager` on `terminal`, where the window is of
/// `size`, and gives the message it asks for, if it does.
fn carry(
    effect: Effect,
    pager: &mut Pager,
    writer: &mut Writer,
    terminal: &mut impl Terminal,
    (size, source): (Option<WindowSize>, &mut Origin),
) -> io::Result<Option<Msg>> {
    match effect {
        Effect::Read => {
            let bytes = terminal.read(pager.wait(writer.has_bands()))?;
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
            let window = terminal.size();
            let resized = (window != size).then(|| Resized {
                known: window.is_some(),
                tells_cells: window.is_some_and(|window| window.cell_pixels().is_some()),
            });
            if resized.is_some() {
                source.window = Some(window);
            }
            let bands = writer.has_bands();
            return Ok(Some(Msg::Window { resized, bands }));
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
        _ => unreachable!("an effect that ends the layout"),
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
