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
use crate::session::panels;
use crate::session::settings::{self, FollowJump, SearchCase};
use crate::session::sources::{Reread, SourceId, ViewId};
use crate::session::state::{Look, State};
use crate::session::step::{Effect, Msg, Pager, Reply, Resized, Run, step};
use crate::session::{loading, sources::Session, status};
use crate::term::{Screen, WindowSize};
use crate::typeset::caps::Capabilities;
use crate::typeset::color::Rgb;
use crate::typeset::driver::Content;
use crate::typeset::images::size::cell_size;
use crate::typeset::laid::Laid;
use crate::typeset::layout::layers::Marks;
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
    laid_out: Option<Laid>,
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
    /// What the layout becomes: whether the next one has the same lines,
    /// whether the document read again only grew at its end, and whether a
    /// setting that every view is laid out by changed (reading mode, wide
    /// blocks or the background).
    end: layouts::RoundEnd,
    /// The document read again, if it was.
    document: Option<Document>,
    /// The source that was read again, or tried to, and how its file
    /// looked when it was fetched: `None` when it could not be looked at.
    reread: Option<(SourceId, Option<Look>)>,
    /// A view to open over this one or to go back to, once the layout is
    /// kept.
    turn: Option<Turn>,
    /// Whether the long tables of the document are to be fitted to all
    /// their rows, once it is read again if it is.
    widths: bool,
}

impl<T: Terminal> Driver<'_, '_, '_, T> {
    /// Lays the document out and shows it until it ends; whether to go on
    /// with another layout.
    fn round(&mut self) -> io::Result<bool> {
        let (sheet, view, laid_for) = self.begin();
        let (bar, rows) = rows_of(&self.tools.inputs.caps);
        let shown = self.session.view(view);
        let first = shown.expect("a view on the deck is open").document(0);
        let (stops, place) = (self.views.take_stops(), self.views.place_of(sheet));
        self.layouts
            .ready(self.state.deck.sheets(), &mut self.views, self.tools.inputs);
        let content = self.views.lend_tables(sheet, self.tools.inputs);
        let mut laid = self
            .layouts
            .laid(sheet, content, first, self.tools.inputs, (rows, &place));
        let numbers = first..first + self.session.sources_of(view).len();
        animate::plan(&mut self.tools, &mut self.state, (laid.lines(), numbers));
        self.state.show(content, &self.session);
        let inputs = &mut *self.tools.inputs;
        // Reading mode's bands scrolled and selected as they were.
        bands_applied(&self.state, &mut laid);
        // The images the new lines do not place go with their first frame,
        // and those of the views under this one stay as they are.
        let below = self.layouts.below(sheet);
        let lines = laid.lines();
        self.tools.encoder.keep_views(lines, &below, &inputs.caps);
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
        let (tintable, now) = (tintable(inputs), self.tools.terminal.now());
        let sized = (heights, (laid.lines(), laid.map()), rows);
        let mut shown = (self.state).view_for(place, sized, (tintable, now));
        // The topmost band on the screen is selected before the first
        // frame, which then shows it so.
        if (self.state.document_mut()).refresh_bands(laid.map(), shown.visible()) {
            bands_applied(&self.state, &mut laid);
        }
        let held = |key: &_| inputs.images.get(key).is_some();
        (self.tools.source).want(held, laid.lines(), shown.visible());
        let (name, shown_files) = layouts::named(&self.session, view, content);
        let loading = (self.tools.source.loads.progress())
            .map(|(done, total)| format!("[rendering: {done}/{total}]"));
        let path = layouts::path(&self.session, &self.state);
        let marks = Marks::new(&inputs.theme, &inputs.caps);
        let rgb = Rgb::new(&inputs.theme, &inputs.caps);
        let looks = (&marks, rgb.as_ref(), usize::from(inputs.caps.width));
        let links = inputs.caps.hyperlinks || inputs.reading.is_some();
        let shown_in = Shown::new(content, links);
        let job = self.session.reread(view);
        let run = run_of(&writer, job.is_some(), self.tools.source.started, inputs);
        let cells = inputs.caps.cell_pixels;
        // The layout is shown until it ends. When its view comes to lines
        // that are only measured, those are laid out, as many, and it is
        // shown again as it is; and while no key waits, the stretches near
        // the view are coloured. Nothing else of the round is made anew.
        let (mut again, mut ticked) = (None, now);
        let mut closed = loop {
            let files = Files::new(&shown_files, laid.map(), laid.len());
            let named = (files, path.clone());
            let status = status_for(name, loading.clone(), named, inputs, (self.size, bar));
            let seen = (laid.lines(), laid.map());
            let mut frame_layout = Layout::new(seen, shown_in, looks, &status, &mut shown);
            frame_layout.panel =
                (self.state.deck.panel()).and_then(|open| self.layouts.panel(open.sheet));
            match again {
                None => self.state.lay_out(&frame_layout),
                Some(_) => self.state.extended((laid.map(), laid.lines())),
            }
            let mut pager = Pager::new(&mut self.state, &mut frame_layout, run, cells, now);
            let ended = show(
                &mut pager,
                (&mut writer, &mut self.tools.presenter),
                self.tools.terminal,
                (self.size, (&laid, rows), again),
                (&mut self.tools.source, &self.session),
            )?;
            let idle = match ended {
                Ended::Effects(effects) => {
                    break self.tools.close(&mut pager, effects, job.as_ref())?;
                }
                other => matches!(other, Ended::Idle),
            };
            let near = layouts::near(shown.top(), rows);
            let changed = layouts::went_on(&mut laid, idle, content, inputs, near);
            again = Some(turn_after(changed, &mut ticked, self.tools.terminal.now()));
        };
        if closed.quit {
            return Ok(false);
        }
        closed.end.grown = self.reread(sheet, closed.document, (closed.reread, closed.widths));
        (self.layouts).end_round(sheet, (laid_for, laid), closed.end, self.tools.inputs);
        if let Some(new) = self.tools.source.window.take() {
            self.size = new;
        }
        self.views.left_at(sheet, shown);
        self.turn(closed.turn);
        Ok(true)
    }

    /// The reread of the layout that closed came to `document`, if it read
    /// one, and the file looked as `reread` says, if it was tried at all:
    /// the source takes the next version, or only the look. With `widths`,
    /// the long tables of the document, read again or not, are fitted to
    /// all their rows, as `r` asks. Whether the document only grew at its
    /// end and its layout can go on, which it does not after `r`.
    fn reread(
        &mut self,
        sheet: Sheet,
        document: Option<Document>,
        (reread, widths): (Option<(SourceId, Option<Look>)>, bool),
    ) -> bool {
        let mut grown = false;
        if let Some((source, look)) = reread {
            match document {
                Some(document) => {
                    grown = self.views.read(sheet, document) && !widths;
                    self.session.reload(source, look);
                }
                None => self.session.looked(source, look),
            }
        }
        if widths {
            self.views.fit(sheet, self.tools.inputs);
        }
        grown
    }
}

/// Whether the window of `caps` has a row for the status line, and the rows
/// it has for the document: the bottom row is the status line's, unless
/// the document would have no row left.
fn rows_of(caps: &Capabilities) -> (bool, usize) {
    let window = usize::from(caps.height.unwrap_or(DEFAULT_ROWS));
    let bar = window > 1;
    (bar, if bar { window - 1 } else { window })
}

/// How a layout is shown again at `now`, after its lines `changed` or did
/// not: `ticked` is when the pager last took a turn, and is moved on when
/// it takes one now.
fn turn_after(changed: bool, ticked: &mut Instant, now: Instant) -> Again {
    let due = now.saturating_duration_since(*ticked) >= crate::session::state::POLL;
    if changed || due {
        *ticked = now;
    }
    match (changed, due) {
        (true, _) => Again::Drawn,
        (false, true) => Again::Tick,
        (false, false) => Again::Wait,
    }
}

/// How the pager runs: whether it `can_reload`, when it `started`, and what
/// `writer` and `inputs` say of the terminal.
fn run_of(writer: &Writer, can_reload: bool, started: Instant, inputs: &Inputs) -> Run {
    Run {
        can_reload,
        tmux: writer.through_tmux(),
        sixel: writer.sixel(),
        started,
        draws_images: inputs.caps.draws_images(),
    }
}

/// Whether the terminal of `inputs` can tint the lines of a change.
fn tintable(inputs: &Inputs) -> bool {
    Rgb::new(&inputs.theme, &inputs.caps).is_some_and(|rgb| rgb.tinted(None, 0.0).is_some())
        && inputs.caps.styling
        && inputs.caps.truecolor
}

/// Reading mode's bands of `laid` scrolled and selected as `state` has them.
fn bands_applied(state: &State, laid: &mut Laid) {
    let (lines, map) = laid.parts_mut();
    state.document().apply_bands(lines, map);
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
                        self.presenter.close(self.terminal)?;
                        self.encoder
                            .close(self.terminal, self.inputs.caps.is_sixel())?;
                        self.terminal.flush()?;
                        closed.quit = true;
                        return Ok(closed);
                    }
                    Effect::Relayout { reuse } => {
                        closed.end.reuse = reuse;
                        return Ok(closed);
                    }
                    // The rows are the same if the view cannot be opened.
                    Effect::Turn(turn) => {
                        closed.turn = Some(turn);
                        closed.end.reuse = true;
                        return Ok(closed);
                    }
                    Effect::Widths => closed.widths = true,
                    Effect::Reload => {
                        let job = job.expect("a source to read again");
                        let (reply, now) = self.reread(pager, job, &mut closed);
                        onward = Some(step(pager, Msg::Reloaded(reply, now)));
                        break;
                    }
                    effect => {
                        let mode = matches!(
                            effect,
                            Effect::SwapReading
                                | Effect::FlipWide
                                | Effect::SetSnap(_)
                                | Effect::SetBackground(_)
                        );
                        closed.end.modes |= mode;
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
            // Laid out again with headings on rows or off them, in both
            // reading modes, as for wide blocks.
            Effect::SetSnap(snap) => {
                for reading in [&mut inputs.reading, &mut self.other].into_iter().flatten() {
                    reading.snap = snap;
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
        let shown = match pager.layout.shown.content {
            Content::Document(shown) => Some(shown),
            Content::Files(_) => None,
        };
        let (look, reloaded_now) = files.reread(
            job,
            shown,
            &mut inputs.images,
            self.source.loads,
            &inputs.caps,
        );
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
        width: size.map_or(inputs.caps.window_width, |size| size.columns),
        bar,
        muted: style::muted(&inputs.theme, panel.clone()),
        scrollbar: panels::scrollbar(&inputs.theme, &inputs.caps),
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

/// `caps` for a window of `size`, its layout `width` columns wide when it is
/// given. `window_cell` is the cells the window told before.
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
    caps.window_width = size.columns;
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

/// How a layout is shown again after its lines were laid out or a piece of
/// its work was done.
#[derive(Clone, Copy)]
enum Again {
    /// Its lines changed: it is drawn.
    Drawn,
    /// No line changed, and less than a poll has passed since the pager
    /// last took a turn: it goes on waiting for keys.
    Wait,
    /// No line changed, and a poll has passed: the pager takes a turn, as
    /// after a wait that ran out.
    Tick,
}

/// How showing a layout ended.
enum Ended {
    /// With the effects that end the layout, which the caller carries out.
    Effects(Vec<Effect>),
    /// The view came to lines that are only measured: the caller lays them
    /// out and shows the layout again.
    Measured,
    /// No key waits, and the layout has work to do in the background: the
    /// caller does a piece of it and shows the layout again.
    Idle,
}

/// Shows `pager`'s layout on `terminal`, whose window is of `size`, until
/// the layout ends: it waits for a message, steps the pager with it and
/// carries out the effects, one after the other. `laid` is the layout, in a
/// window of `rows` rows for it, and `again` says that it is shown again
/// after [`Ended::Measured`] or [`Ended::Idle`], and how.
fn show(
    pager: &mut Pager,
    (writer, presenter): (&mut Writer, &mut Presenter),
    terminal: &mut impl Terminal,
    (size, (laid, rows), again): (Option<WindowSize>, (&Laid, usize), Option<Again>),
    (source, session): (&mut Origin, &Session),
) -> io::Result<Ended> {
    let measured = laid.measured();
    // After a piece of work that changed no line, the wait for keys goes
    // on where it was: no time has passed for the pager, so that keys that
    // come in pieces are not taken for whole ones.
    let mut first = match again {
        Some(Again::Wait) => Some(vec![Effect::Read]),
        Some(Again::Tick) => Some(step(pager, Msg::Input(Vec::new(), terminal.now()))),
        _ => None,
    };
    let mut msg = Msg::Shown(terminal.now());
    loop {
        let mut next = None;
        let stepped = match first.take() {
            Some(effects) => effects,
            None => step(pager, msg),
        };
        // A layout that ends is not held up by lines that are measured.
        let ends = stepped.iter().any(Effect::ends);
        let mut effects = stepped.into_iter();
        while let Some(effect) = effects.next() {
            if effect.ends() {
                return Ok(Ended::Effects(
                    std::iter::once(effect).chain(effects).collect(),
                ));
            }
            // A view that has come to lines that are only measured is not
            // drawn: they are laid out first. What the step asked of the
            // animations is still done.
            if matches!(effect, Effect::Draw(..)) && !ends {
                let visible = pager.layout.view.visible();
                let first = measured.partition_point(|lines| lines.end <= visible.start);
                if (measured.get(first)).is_some_and(|lines| lines.start < visible.end) {
                    for effect in effects {
                        if let Effect::Animate(commands) = effect {
                            animate::carry(&commands, pager, writer, source, terminal)?;
                        }
                    }
                    return Ok(Ended::Measured);
                }
            }
            // Before a wait for keys, the layout does what it has left to
            // do, a piece at a time, as long as no key waits.
            let near = layouts::near(pager.layout.view.top(), rows);
            if matches!(effect, Effect::Read) && laid.pending(near) {
                let bytes = terminal.read(Duration::ZERO)?;
                if bytes.is_empty() {
                    return Ok(Ended::Idle);
                }
                next = Some(Msg::Input(bytes, terminal.now()));
                continue;
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
