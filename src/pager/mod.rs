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
use std::path::Path;
use std::time::{Duration, Instant};

use self::layouts::{Layouts, Views};
use crate::images::{Arrived, ImageKey, Job, Loads};
use crate::load::watch::{Reloaded, SourceFiles, Watcher};
use crate::present::encoder::Encoder;
use crate::present::output::{Page, Writer};
use crate::present::presenter::Presenter;
use crate::present::{cell_query, clipboard};
use crate::render::Inputs;
use crate::session::animation::Pacing;
use crate::session::deck::Sheet;
use crate::session::files::Files;
use crate::session::follow::Turn;
use crate::session::frame::{Layout, Shown, Status};
use crate::session::settings::{self, FollowJump, SearchCase};
use crate::session::sources::{Reread, SourceId, ViewId};
use crate::session::state::{Look, State};
use crate::session::step::{Effect, Msg, Pager, Reply, Resized, Run, step};
use crate::session::{loading, sources::Session, status};
use crate::term::{Screen, WindowSize};
use crate::typeset::caps::Capabilities;
use crate::typeset::color::Rgb;
use crate::typeset::driver::{Content, Stream};
use crate::typeset::images::size::cell_size;
use crate::typeset::layout::layers::Marks;
use crate::typeset::locate::Map;
use crate::typeset::model::Document;
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page::Wide;
use crate::typeset::style::{self, Line, Style};

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
    /// Told what a frame shows before it is written; only tests use it.
    fn drew(&mut self, _: &crate::present::scene::Scene<'_>, _: &dyn Fn(&Line) -> u8) {}
}

impl<W: Write> Terminal for Screen<W> {
    fn size(&self) -> Option<WindowSize> {
        Screen::size(self)
    }

    fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        Screen::read(self, timeout)
    }
}

/// What the pager is told besides the document.
pub struct Options<'a> {
    /// The columns the document is laid out in, when the window's are not
    /// to decide.
    pub width: Option<u16>,
    /// How a search treats case.
    pub case: SearchCase,
    /// Whether `B` paints the terminal's background one shade off.
    pub background_offset: bool,
    /// The files behind the session's sources, which the pager reads again
    /// and looks at; `None` when no source has one.
    pub files: Option<&'a mut dyn SourceFiles>,
    /// Whether the pager follows the document from the start: reads it
    /// again whenever it changes.
    pub follow: settings::Follow,
    /// Which change following on auto moves the view to.
    pub jump: FollowJump,
    /// The warnings about the document as first read, which the pager
    /// tells as it does those of a reload.
    pub warnings: Vec<crate::typeset::diagnostic::Diagnostic>,
    /// The reading mode that `M` turns on, when the document is shown
    /// without it; `M` turns reading mode off when it is on.
    pub reading: Option<Reading>,
    /// What the images of the content shown are, which the pager loads in
    /// the background: nothing is wanted without it.
    pub planner: Option<Planner<'a>>,
    /// How animated images play: how many times at most, and the least time
    /// the waiting mark shows.
    pub animation: Pacing,
}

/// What a planner plans for: the content of a view, the number the keys
/// of its first document's images carry, and the file each document was
/// read from.
pub struct Planned<'a> {
    pub content: Content<'a>,
    pub first: usize,
    pub paths: Vec<Option<&'a Path>>,
}

/// The jobs of the images `planned` wants, shown on a terminal with the
/// caps given, with reading mode on or not, in the order of the documents.
/// The pager asks for them again when the document is read again, when
/// reading mode turns on or off and when the cells change, and loads
/// those it does not hold, the ones nearest the view first.
pub type Planner<'a> = Box<dyn FnMut(Planned<'_>, &Capabilities, bool) -> Vec<Job> + 'a>;

impl Options<'_> {
    /// A window's width to decide the layout, searches that treat case as
    /// `case` says, and no files to read again.
    pub fn new(case: SearchCase) -> Self {
        Options {
            width: None,
            case,
            background_offset: true,
            files: None,
            follow: settings::Follow::Off,
            jump: FollowJump::First,
            warnings: Vec::new(),
            reading: None,
            planner: None,
            animation: Pacing::quick(),
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
/// The images the planner wants, and the loads the caller hands over in
/// `loads`, load there; `q` stops them without waiting for their programs:
/// the caller waits for them once the pager has left its screen.
pub fn run(
    content: Content,
    laid_out: Option<Stream>,
    session: Session,
    inputs: &mut Inputs,
    options: Options,
    loads: &mut Loads,
    terminal: &mut impl Terminal,
) -> io::Result<Session> {
    let mut driver = Driver::new(content, laid_out, session, inputs, options, loads, terminal);
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
    /// What writes the frames, kept from one layout to the next.
    presenter: Presenter,
    /// The reading mode that `M` turns to, on or off. An exception, not a
    /// model: a key's state goes in the state engine, which sends what to
    /// set as an effect, as `B` does with `Effect::SetBackground`.
    other: Option<Reading>,
}

/// The loop: layouts of the document on top, one after the other, each
/// shown until an effect ends it.
struct Driver<'a, 'r, 'l, T> {
    tools: Tools<'a, 'r, 'l, T>,
    /// What each view shows and where it was.
    views: Views<'a>,
    /// The layouts that are still good.
    layouts: Layouts,
    state: State,
    /// The window's size as last found.
    size: Option<WindowSize>,
    /// The cells the window told last.
    window_cell: Option<(u16, u16)>,
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
    /// The source that was read again, or tried to, and how its file
    /// looked when it was fetched: `None` when it could not be looked at.
    reread: Option<(SourceId, Option<Look>)>,
    /// Whether a setting that every view is laid out by changed: reading
    /// mode, wide blocks or the background.
    modes: bool,
    /// A view to open over this one or to go back to, once the layout is
    /// kept.
    turn: Option<Turn>,
}

impl<T: Terminal> Driver<'_, '_, '_, T> {
    /// Lays the document out and shows it until it ends; whether to go on
    /// with another layout.
    fn round(&mut self) -> io::Result<bool> {
        let (sheet, view, laid_for) = self.begin();
        let window = usize::from(self.tools.inputs.caps.height.unwrap_or(DEFAULT_ROWS));
        // The bottom row is the status line's, unless the document would
        // have no row left.
        let bar = window > 1;
        let rows = if bar { window - 1 } else { window };
        let shown = self.session.view(view);
        let first = shown.expect("a view on the deck is open").document(0);
        let (stops, place) = (self.views.take_stops(), self.views.place_of(sheet));
        self.layouts
            .ready(self.state.deck.sheets(), &mut self.views, self.tools.inputs);
        let content = self.views.content(sheet);
        let (stream, map) = (self.layouts).stream(sheet, content, first, self.tools.inputs, rows);
        let Stream { mut lines, blocks } = stream;
        let map = map.unwrap_or_else(|| Map::new(&lines, &blocks));
        let numbers = first..first + self.session.sources_of(view).len();
        animate::plan(&mut self.tools, &mut self.state, (&lines, numbers));
        self.state.show(content, &self.session);
        let inputs = &mut *self.tools.inputs;
        // Reading mode's bands scrolled and selected as they were.
        self.state.document().apply_bands(&mut lines, &map);
        // The images the new lines do not place go with their first frame,
        // and those of the views under this one stay as they are.
        let below = self.layouts.below(sheet);
        self.tools.encoder.keep_views(&lines, &below, &inputs.caps);
        let page = Page {
            background: inputs.theme.background,
            foreground: inputs.theme.foreground,
        };
        let mut writer = Writer::new(&inputs.images, &inputs.caps, page, &mut self.tools.encoder);
        animate::stop_below(
            stops,
            &mut writer,
            &mut self.tools.source,
            self.tools.terminal,
        )?;
        let heights: Vec<usize> = lines
            .iter()
            .map(|line| usize::from(writer.rows(line)))
            .collect();
        let tintable = Rgb::new(&inputs.theme, &inputs.caps)
            .is_some_and(|rgb| rgb.tinted(None, 0.0).is_some())
            && inputs.caps.styling
            && inputs.caps.truecolor;
        let now = self.tools.terminal.now();
        let laid = (heights, (&lines[..], &map), rows);
        let mut shown = (self.state).view_for(place, laid, (tintable, now));
        // The topmost band on the screen is selected before the first
        // frame, which then shows it so.
        if self
            .state
            .document_mut()
            .refresh_bands(&map, shown.visible())
        {
            self.state.document().apply_bands(&mut lines, &map);
        }
        let held = |key: &_| inputs.images.get(key).is_some();
        (self.tools.source).want(held, &lines, shown.visible());
        let (name, shown_files) = layouts::named(&self.session, view, content);
        let loading = (self.tools.source.loads.progress())
            .map(|(done, total)| format!("[rendering: {done}/{total}]"));
        let files = Files::new(&shown_files, &map, lines.len());
        let path = layouts::path(&self.session, &self.state);
        let status = status_for(name, loading, (files, path), inputs, (self.size, bar));
        let marks = Marks::new(&inputs.theme, &inputs.caps);
        let rgb = Rgb::new(&inputs.theme, &inputs.caps);
        let looks = (&marks, rgb.as_ref(), usize::from(inputs.caps.width));
        let links = inputs.caps.hyperlinks || inputs.reading.is_some();
        let shown_in = Shown::new(content, links);
        let mut frame_layout = Layout::new((&lines, &map), shown_in, looks, &status, &mut shown);
        frame_layout.panel =
            (self.state.deck.panel()).and_then(|open| self.layouts.panel(open.sheet));
        let job = self.session.reread(view);
        let run = Run {
            can_reload: job.is_some(),
            tmux: writer.through_tmux(),
            sixel: writer.sixel(),
            started: self.tools.source.started,
            draws_images: inputs.caps.draws_images(),
        };
        let cells = inputs.caps.cell_pixels;
        self.state.lay_out(&frame_layout);
        let mut pager = Pager::new(&mut self.state, &mut frame_layout, run, cells, now);
        let ended = show(
            &mut pager,
            (&mut writer, &mut self.tools.presenter),
            self.tools.terminal,
            self.size,
            (&mut self.tools.source, &self.session),
        )?;
        let closed = self.tools.close(&mut pager, ended, job.as_ref())?;
        if closed.quit {
            return Ok(false);
        }
        self.reread(sheet, closed.document, closed.reread);
        if closed.reuse {
            let stream = Stream { lines, blocks };
            self.layouts.keep(sheet, laid_for, stream, Some(map));
        } else {
            self.layouts.drop_stale(closed.modes);
        }
        if let Some(new) = self.tools.source.window.take() {
            self.size = new;
        }
        self.views.left_at(sheet, shown);
        self.turn(closed.turn);
        Ok(true)
    }

    /// The reread of the layout that closed came to `document`, if it read
    /// one, and the file looked as `reread` says, if it was tried at all:
    /// the source takes the next version, or only the look.
    fn reread(
        &mut self,
        sheet: Sheet,
        document: Option<Document>,
        reread: Option<(SourceId, Option<Look>)>,
    ) {
        let Some((source, look)) = reread else {
            return;
        };
        match document {
            Some(document) => {
                self.views.read(sheet, document);
                self.session.reload(source, look);
            }
            None => self.session.looked(source, look),
        }
    }
}

impl<T: Terminal> Tools<'_, '_, '_, T> {
    /// Carries out the effects that end the layout `pager` showed, and
    /// those that follow what they find out; `job` is what the session has
    /// to read again.
    fn close(
        &mut self,
        pager: &mut Pager,
        mut effects: Vec<Effect>,
        job: Option<&Reread>,
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
                        self.encoder
                            .close(self.terminal, self.inputs.caps.is_sixel())?;
                        self.terminal.flush()?;
                        closed.quit = true;
                        return Ok(closed);
                    }
                    Effect::Relayout { reuse } => {
                        closed.reuse = reuse;
                        return Ok(closed);
                    }
                    // The rows are the same if the view cannot be opened.
                    Effect::Turn(turn) => {
                        closed.turn = Some(turn);
                        closed.reuse = true;
                        return Ok(closed);
                    }
                    Effect::Reload => {
                        let job = job.expect("a source to read again");
                        let (reply, now) = self.reread(pager, job, &mut closed);
                        onward = Some(step(pager, Msg::Reloaded(reply, now)));
                        break;
                    }
                    effect => {
                        let mode = matches!(
                            effect,
                            Effect::SwapReading | Effect::FlipWide | Effect::SetBackground(_)
                        );
                        closed.modes |= mode;
                        self.close_with(effect)?;
                    }
                }
            }
            effects = onward.expect("an effect that ends the layout");
        }
    }

    /// Carries out `effect`, one of those that end a layout, which needs
    /// nothing of the document's source.
    fn close_with(&mut self, effect: Effect) -> io::Result<()> {
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
            _ => unreachable!("an effect that is not one of those that end a layout"),
        }
        Ok(())
    }

    /// Runs `job`, which reads the document again and brings the images up
    /// to date for it: what came of it, and when. `closed` gets the
    /// document if it was read, and the look of its file; `pager` shows
    /// the document that is replaced.
    fn reread(&mut self, pager: &Pager, job: &Reread, closed: &mut Closed) -> (Reply, Instant) {
        // The reread may change the pixels of the images.
        self.encoder.changed();
        let inputs = &mut *self.inputs;
        let files = (self.source.files.as_deref_mut()).expect("files to read the source from");
        let now = self.terminal.now();
        let before = inputs.images.take_placeholders();
        let (look, reloaded_now) =
            files.reread(job, &mut inputs.images, self.source.loads, &inputs.caps);
        closed.reread = Some((job.source, look));
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
                return (Reply::Failed(why), now);
            }
        };
        // What the new document wants is planned before the images that
        // load get something to stand in for them.
        let planned = Planned {
            content: Content::Document(&document),
            first: job.document,
            paths: vec![Some(job.path.as_path())],
        };
        self.source
            .plan(planned, pager.state.document().view, inputs);
        let hashes = vec![document.hashes()];
        let mut changes = None;
        if let Content::Document(old) = pager.layout.shown.content {
            let comparison = pager.state.document().compared(old, &document, &hashes[0]);
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
            let (images, loads) = (&inputs.images, &*self.source.loads);
            let keys = (self.source.planned.iter().map(|job| &job.key))
                .filter(|key| images.get(key).is_none() || loads.loading(key))
                .cloned();
            for (key, placeholder) in left.placeholders(keys) {
                inputs.images.wait(key, placeholder);
            }
            changes = Some(comparison);
        }
        closed.document = Some(document);
        (Reply::read(warnings, changes, hashes), now)
    }
}

/// The status line and the panels' look for `inputs`, in a window of `size`
/// that has a row for the status line when `bar`, calling a lone document
/// `name`; `loading` stands in place of the name while images load.
fn status_for<'a>(
    name: &'a str,
    loading: Option<String>,
    (files, path): (Files<'a>, Vec<&'a str>),
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
        path,
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
    /// The files behind the sources; `None` when no source has one.
    files: Option<&'r mut dyn SourceFiles>,
    /// The images loading in the background after a reload.
    loads: &'l mut Loads,
    /// When the pager started, from when the bands below the images that
    /// load move.
    started: Instant,
    /// The loads that came, which go into the images as the next layout
    /// starts, as the images are in use until then.
    arrived: Vec<Arrived>,
    /// The size the window was found to be of, when it changed.
    window: Option<Option<WindowSize>>,
    /// When each watch of the session was last looked at.
    watcher: Watcher,
    /// What gives the jobs of the images the shown content wants.
    planner: Option<Planner<'r>>,
    /// Those jobs, in the order of the documents: the images that are
    /// loading or held, and no others.
    planned: Vec<Job>,
    /// The view, whether reading mode was on and the cells when `planned`
    /// was made; `None` before.
    planned_for: Option<(ViewId, bool, (u16, u16))>,
}

impl Origin<'_, '_> {
    /// Plans the images the content of `view` wants for the mode and the
    /// cells of `inputs`, which are then the images shown, in that order.
    fn plan(&mut self, planned: Planned, view: ViewId, inputs: &mut Inputs) {
        let reading = inputs.reading.is_some();
        self.planned = match self.planner.as_mut() {
            Some(plan) => plan(planned, &inputs.caps, reading),
            None => Vec::new(),
        };
        self.planned_for = Some((view, reading, cell_size(&inputs.caps)));
        inputs.images.shown = self.planned.iter().map(|job| job.key.clone()).collect();
    }

    /// Has the loads load the planned images that `held` does not say are
    /// held, those nearest to the lines `visible` in `lines` first and the
    /// others in the order of the documents, and stop the others. Called
    /// after each layout and at each poll, as the view may have moved.
    fn want(&mut self, held: impl Fn(&ImageKey) -> bool, lines: &[Line], visible: Range<usize>) {
        debug_assert!(self.arrived.is_empty(), "the images that came go in first");
        let keys = self.planned.iter().map(|job| &job.key);
        let wanted = loading::wanted(keys, &loading::nearest(lines, visible), held);
        self.loads.want(&self.planned, wanted);
    }
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
    caps.height = size.height_in_rows();
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

/// Shows `pager`'s layout on `terminal`, whose window is of `size`, until
/// the layout ends: it waits for a message, steps the pager with it and
/// carries out the effects, one after the other. It gives the effects that
/// end the layout, which the caller carries out.
fn show(
    pager: &mut Pager,
    (writer, presenter): (&mut Writer, &mut Presenter),
    terminal: &mut impl Terminal,
    size: Option<WindowSize>,
    (source, session): (&mut Origin, &Session),
) -> io::Result<Vec<Effect>> {
    let mut msg = Msg::Shown(terminal.now());
    loop {
        let mut next = None;
        let mut effects = step(pager, msg).into_iter();
        while let Some(effect) = effects.next() {
            if effect.ends() {
                return Ok(std::iter::once(effect).chain(effects).collect());
            }
            let tools = (size, &mut *source, session);
            if let Some(reply) = carry(effect, pager, (writer, presenter), terminal, tools)? {
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
    (writer, presenter): (&mut Writer, &mut Presenter),
    terminal: &mut impl Terminal,
    (size, source, session): (Option<WindowSize>, &mut Origin, &Session),
) -> io::Result<Option<Msg>> {
    match effect {
        Effect::Read => {
            let bytes = terminal.read(pager.wait(writer.has_bands()))?;
            return Ok(Some(Msg::Input(bytes, terminal.now())));
        }
        Effect::PollWatches => {
            let view = pager.state.document().view;
            let watches = session.watches(view, pager.state.follow.on());
            let files = source.files.as_deref_mut().expect("files to poll");
            let looks = (source.watcher).due(&watches, pager.now(), |path| files.look(path));
            return Ok(Some(Msg::Changed(looks)));
        }
        Effect::PollLoads => {
            // Held or loading, whatever is planned: only the order moves.
            if !source.loads.is_empty() {
                let (lines, visible) = (pager.layout.lines, pager.layout.view.visible());
                source.want(|_| true, lines, visible);
            }
            return Ok(Some(animate::poll(source, writer)));
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
        Effect::Draw(frame, at) => {
            if let Some(at) = at {
                writer.at(at);
            }
            animate::draw(&frame, pager, (writer, presenter), source, terminal)?;
            pager.state.sent(writer.check(terminal)?);
            terminal.flush()?;
        }
        Effect::Check => {
            pager.state.sent(writer.check(terminal)?);
            terminal.flush()?;
        }
        Effect::Animate(commands) => animate::carry(&commands, pager, writer, source, terminal)?,
        Effect::Answered(id) => {
            writer.answered(id, terminal)?;
            terminal.flush()?;
        }
        Effect::Resend(id) => writer.send_again(id, terminal)?,
        Effect::Lose => presenter.lost(writer),
        Effect::LoseSome(sent) => presenter.lost_some(writer, &sent),
        Effect::Copy(text) => {
            clipboard::copy(&text, writer.through_tmux(), terminal)?;
            terminal.flush()?;
        }
        _ => unreachable!("an effect that ends the layout"),
    }
    Ok(None)
}

mod animate;
mod layouts;
#[cfg(test)]
mod tests;
