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

use crate::images::{Arrived, ImageKey, Job, Loads, Placeholder};
use crate::load::watch::{Reloaded, SourceFiles, Watcher};
use crate::present::encoder::Encoder;
use crate::present::output::{self, Page, Writer};
use crate::present::{cell_query, clipboard};
use crate::render::Inputs;
use crate::session::fade::Rgb;
use crate::session::files::Files;
use crate::session::frame::{Frame, Layout, Status};
use crate::session::help::Placed;
use crate::session::search::Marks;
use crate::session::settings::{self, Animations, FollowJump, SearchCase};
use crate::session::sources::{Reread, SourceId};
use crate::session::state::{Backgrounds, Look, NOTHING_TO_FOLLOW, State, Told};
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
    /// Whether and how often animated images play.
    pub animations: Animations,
}

/// The jobs of the images `content` wants, shown on a terminal with the
/// caps given, with reading mode on or not, in the order of the documents.
/// The pager asks for them again when the document is read again, when
/// reading mode turns on or off and when the cells change, and loads
/// those it does not hold, the ones nearest the view first.
pub type Planner<'a> = Box<dyn FnMut(Content<'_>, &Capabilities, bool) -> Vec<Job> + 'a>;

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
            animations: Animations::Never,
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
    session.assert_fits(content);
    let Options {
        width,
        case,
        background_offset,
        files,
        follow,
        jump,
        warnings,
        reading,
        planner,
        animations,
    } = options;
    assert!(
        session.reread().is_none() || files.is_some(),
        "a session with a file to read comes with a way to read it"
    );
    let mut state = State::new(case);
    state.jump = jump;
    animate::begin(&mut state, animations, &inputs.caps);
    let (theme, reported) = (inputs.theme.background, inputs.caps.background);
    state.background = Backgrounds::new(theme, reported, background_offset);
    state.warn(warnings, terminal.now());
    if session.reread().is_some() {
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
        width,
        session,
        tools: Tools {
            // The reading mode that `M` turns to, on or off.
            other: reading.filter(|_| inputs.reading.is_none()),
            inputs,
            source: Origin {
                files,
                loads,
                started: terminal.now(),
                watcher: Watcher::default(),
                arrived: Vec::new(),
                window: None,
                planner,
                planned: Vec::new(),
                planned_for: None,
            },
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
        // Nothing uses the images yet, so the ones that came go in now.
        for Arrived { key, image, .. } in self.tools.source.arrived.drain(..) {
            self.tools.inputs.images.came(key, image);
        }
        fit(
            &mut self.tools.inputs.caps,
            self.size,
            self.width,
            &mut self.window_cell,
        );
        self.plan();
        let window = usize::from(self.tools.inputs.caps.height.unwrap_or(DEFAULT_ROWS));
        // The bottom row is the status line's, unless the document would
        // have no row left.
        let bar = window > 1;
        let rows = if bar { window - 1 } else { window };
        let Stream { mut lines, blocks } = self.stream(rows);
        animate::plan(self, &lines);
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
        let held = |key: &_| inputs.images.get(key).is_some();
        (self.tools.source).want(held, &lines, shown.visible());
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
        let job = self.session.reread();
        let run = Run {
            can_reload: job.is_some(),
            tmux: writer.through_tmux(),
            started: self.tools.source.started,
            draws_images: inputs.caps.draws_images(),
        };
        let cells = inputs.caps.cell_pixels;
        let mut pager = Pager::new(&mut self.state, &mut frame_layout, run, cells, now);
        let ended = show(
            &mut pager,
            &mut writer,
            self.tools.terminal,
            self.size,
            (&mut self.tools.source, &self.session),
        )?;
        let closed = self.tools.close(&mut pager, content, ended, job.as_ref())?;
        if closed.quit {
            return Ok(false);
        }
        self.reread(closed.document, closed.reread);
        if closed.reuse {
            self.laid_out = Some((self.tools.inputs.caps.clone(), Stream { lines, blocks }));
        }
        if let Some(new) = self.tools.source.window.take() {
            self.size = new;
        }
        self.view = Some(shown);
        Ok(true)
    }

    /// The reread of the layout that closed came to `document`, if it read
    /// one, and the file looked as `reread` says, if it was tried at all:
    /// the source takes the next version, or only the look.
    fn reread(&mut self, document: Option<Document>, reread: Option<(SourceId, Option<Look>)>) {
        let Some((source, look)) = reread else {
            return;
        };
        match document {
            Some(document) => {
                self.reloaded = Some(document);
                self.session.reload(source, look);
            }
            None => self.session.looked(source, look),
        }
    }

    /// Plans the images the shown content wants when the mode or the cells
    /// are not those it was planned for, and gives those that start loading
    /// something to stand in for them.
    fn plan(&mut self) {
        let inputs = &mut *self.tools.inputs;
        let source = &mut self.tools.source;
        if source.planned_for != Some((inputs.reading.is_some(), cell_size(&inputs.caps))) {
            source.plan(shown_content(self.content, &self.reloaded), inputs);
        }
        let keys = source.planned.iter().map(|job| &job.key);
        for key in loading::starting(keys, &inputs.images) {
            inputs.images.wait(key, Placeholder::Unsized);
            self.laid_out = None;
        }
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
}

impl<T: Terminal> Tools<'_, '_, '_, T> {
    /// Carries out the effects that end a layout of `content`, which is
    /// what it showed, and those that follow what they find out; `job` is
    /// what the session has to read again.
    fn close(
        &mut self,
        pager: &mut Pager,
        content: Content,
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
                        let job = job.expect("a source to read again");
                        let (reply, now) = self.reread(content, job, &mut closed);
                        onward = Some(step(pager, Msg::Reloaded(reply, now)));
                        break;
                    }
                    effect => self.close_with(effect)?,
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
    /// document if it was read, and the look of its file.
    fn reread(&mut self, content: Content, job: &Reread, closed: &mut Closed) -> (Reply, Instant) {
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
        self.source.plan(Content::Document(&document), inputs);
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
        (Reply::Read { warnings, changes }, now)
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
    /// Whether reading mode was on, and the cells, when `planned` was made;
    /// `None` before.
    planned_for: Option<(bool, (u16, u16))>,
}

impl Origin<'_, '_> {
    /// Plans the images `content` wants for the mode and the cells of
    /// `inputs`, which are then the images shown, in that order.
    fn plan(&mut self, content: Content, inputs: &mut Inputs) {
        let reading = inputs.reading.is_some();
        self.planned = match self.planner.as_mut() {
            Some(plan) => plan(content, &inputs.caps, reading),
            None => Vec::new(),
        };
        self.planned_for = Some((reading, cell_size(&inputs.caps)));
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
            if let Some(reply) = carry(effect, pager, writer, terminal, tools)? {
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
    (size, source, session): (Option<WindowSize>, &mut Origin, &Session),
) -> io::Result<Option<Msg>> {
    match effect {
        Effect::Read => {
            let bytes = terminal.read(pager.wait(writer.has_bands()))?;
            return Ok(Some(Msg::Input(bytes, terminal.now())));
        }
        Effect::PollWatches => {
            let watches = session.watches(pager.state.follow.on());
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
        Effect::Draw(frame, at) => {
            if let Some(at) = at {
                writer.at(at);
            }
            draw_frame(&frame, pager.layout.view.rows(), writer, terminal)?;
            animate::sent(pager, writer, terminal)?;
            pager.state.sent(writer.check(terminal)?);
            terminal.flush()?;
        }
        Effect::Check => {
            pager.state.sent(writer.check(terminal)?);
            terminal.flush()?;
        }
        Effect::Animate(commands) => animate::carry(&commands, writer, terminal)?,
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

mod animate;
#[cfg(test)]
mod tests;
