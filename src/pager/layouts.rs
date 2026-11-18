//! What the loop keeps by the sheet of the deck: the layouts, which a
//! reuse carries from one round to the next, for the document of each
//! view and, laid out at their own width, for the panels; and for each
//! view what it shows and where its view was. Opening a view and going
//! back to one are here too, as they work on these.

use std::collections::HashMap;
use std::path::Path;

use super::{Driver, Options, Origin, Planned, Terminal, Tools, animate, cell_size, fit, loading};
use crate::images::{Arrived, Loads, Placeholder};
use crate::load::watch::{Reloaded, Watcher};
use crate::present::encoder::{Command, Encoder};
use crate::present::presenter::Presenter;
use crate::render::Inputs;
use crate::session::commands::Topic;
use crate::session::deck::Sheet;
use crate::session::follow::{Left, Turn};
use crate::session::history::Ahead;
use crate::session::panels::{self, Panel, help};
use crate::session::settings;
use crate::session::sources::{Session, Source, SourceFile, ViewId};
use crate::session::state::{Backgrounds, NOTHING_TO_FOLLOW, State, Told};
use crate::session::view::View;
use crate::typeset::caps::Capabilities;
use crate::typeset::driver::{self, Content};
use crate::typeset::input::Input;
use crate::typeset::laid::Laid;
use crate::typeset::model::Document;
use crate::typeset::stretch::Tables;
use crate::typeset::style::{Line, text};

/// How many lines above and below the view the stretches of a large block
/// are laid out, when a layout is made or its view has come to lines that
/// are only measured.
const NEAR: usize = 256;

/// The lines near a view of `rows` rows whose top line is `top`: the
/// stretches of a large block that hold one of them are laid out.
pub(super) fn near(top: usize, rows: usize) -> std::ops::Range<usize> {
    top.saturating_sub(NEAR)..top.saturating_add(rows + NEAR)
}

/// Goes on with `laid`, the layout of `content` with `inputs`, whose view
/// is among the lines `near`: when the pager was `idle`, a piece of the
/// work the layout has left is done, and otherwise the view came to lines
/// that are only measured, which are laid out. Whether a line changed.
pub(super) fn went_on(
    laid: &mut Laid,
    idle: bool,
    content: Content,
    inputs: &Inputs,
    near: std::ops::Range<usize>,
) -> bool {
    match idle {
        true => laid.work(content, inputs, near),
        false => laid.show(content, inputs, near),
    }
}

/// A layout that is still good: the caps it is for and what it is laid
/// out as, whose map, once it was shown, stays good with the lines.
struct Kept {
    caps: Capabilities,
    laid: Laid,
}

/// A panel's layout that is still good, for the caps it is for.
struct Aside {
    caps: Capabilities,
    panel: Panel,
}

/// The layouts the loop keeps: of the document of each view, by its
/// sheet, and the panels' by theirs.
#[derive(Default)]
pub(super) struct Layouts {
    kept: HashMap<Sheet, Kept>,
    panels: HashMap<Sheet, Aside>,
}

impl Layouts {
    /// The layouts at the start: the document laid out beforehand for
    /// `caps`, if it was.
    pub(super) fn starting(laid_out: Option<Laid>, caps: &Capabilities) -> Self {
        let mut layouts = Self::default();
        if let Some(laid) = laid_out {
            layouts.keep(Sheet::Base, caps.clone(), laid);
        }
        layouts
    }

    /// The lines of the document of `sheet`, whose images' keys carry the
    /// numbers from `first`, for a window with `rows` for them: the layout
    /// kept when it is still good, with its map if it has one, and
    /// otherwise a new one of `content`.
    pub(super) fn laid(
        &mut self,
        sheet: Sheet,
        content: Content,
        first: usize,
        inputs: &Inputs,
        (rows, place): (usize, &Option<View>),
    ) -> Laid {
        let top = place.as_ref().map_or(0, View::top);
        let mut laid = match self.kept.remove(&sheet) {
            Some(kept) if kept.caps == inputs.caps => kept.laid,
            _ => driver::laid_measured(content, first, inputs),
        };
        // The stretches of a large block near the view get their lines;
        // the others are only measured.
        laid.show(content, inputs, near(top, rows));
        // The text sizing protocol cannot draw part of a line, so a line
        // taller than the window is drawn at normal size.
        for line in laid.lines_mut() {
            if usize::from(crate::present::output::rows(line, &inputs.caps)) > rows {
                line.to_normal_size();
            }
        }
        laid
    }

    /// Keeps the layout of the document of `sheet`, with its map, for the
    /// next round.
    pub(super) fn keep(&mut self, sheet: Sheet, caps: Capabilities, laid: Laid) {
        self.kept.insert(sheet, Kept { caps, laid });
    }

    /// Forgets the layout of the document of `sheet`: images it waits for
    /// began to load, or its view was closed.
    pub(super) fn drop_layout(&mut self, sheet: Sheet) {
        self.kept.remove(&sheet);
    }

    /// The lines of the documents under `top`, as last kept: what the
    /// terminal holds the images of while they are covered. The layout of
    /// `top` is not among them, as it is the one being made.
    pub(super) fn below(&self, top: Sheet) -> Vec<&[Line]> {
        let others = self.kept.iter().filter(|(sheet, _)| **sheet != top);
        others.map(|(_, kept)| kept.laid.lines()).collect()
    }

    /// Forgets the panels' layouts, which a new theme, background or
    /// reading mode makes out of date, and with `modes`, which says one
    /// of those changed, the layouts of the documents too: the views under
    /// the one on top are laid out anew when the reader comes back.
    pub(super) fn drop_stale(&mut self, modes: bool) {
        self.panels.clear();
        if modes {
            self.kept.clear();
        }
    }

    /// Makes the layouts of `sheets` ready: those the deck no longer lists
    /// are let go, with what `views` hold of them, and a panel that has
    /// none, or whose caps changed, is laid out at its width. A document's
    /// layout is made when its round comes, and is kept while its view is
    /// on the deck, though its caps may be out of date: it is only the
    /// terminal's ids it serves then.
    pub(super) fn ready(
        &mut self,
        sheets: impl Iterator<Item = Sheet>,
        views: &mut Views,
        inputs: &Inputs,
    ) {
        let listed: Vec<Sheet> = sheets.collect();
        views.retain(&listed);
        self.kept.retain(|sheet, _| listed.contains(sheet));
        self.panels.retain(|sheet, _| listed.contains(sheet));
        for sheet in listed {
            if (self.panels.get(&sheet)).is_some_and(|aside| aside.caps == inputs.caps) {
                continue;
            }
            let width = panels::width(usize::from(inputs.caps.width), help::fit());
            let width = u16::try_from(width).unwrap_or(u16::MAX);
            let panel = match sheet {
                Sheet::Base | Sheet::View(_) => continue,
                Sheet::Help => {
                    let look = help::Look::new(&inputs.theme, &inputs.caps);
                    let lines = help::lines(usize::from(width), &look);
                    Panel {
                        topics: topics(&lines),
                        width: usize::from(width),
                        lines,
                    }
                }
            };
            let caps = inputs.caps.clone();
            self.panels.insert(sheet, Aside { caps, panel });
        }
    }

    /// The panel `sheet` is, when it is laid out.
    pub(super) fn panel(&self, sheet: Sheet) -> Option<&Panel> {
        self.panels.get(&sheet).map(|aside| &aside.panel)
    }
}

/// The line where each topic of the help starts: its heading, the one
/// line of text that is the heading and nothing else (the rows of
/// commands begin with their keys).
fn topics(lines: &[Line]) -> Vec<(Topic, usize)> {
    let at = |heading: &str| {
        lines
            .iter()
            .position(|line| text(&line.spans).trim() == heading)
    };
    let found = help::TOPICS
        .iter()
        .map(|&(topic, heading)| (topic, at(heading)));
    // The first heading has only the blank row that opens the help above
    // it: the help opens on that row, as it opens at its top.
    found
        .map(|(topic, line)| {
            (
                topic,
                line.map_or(0, |line| line.saturating_sub(usize::from(line == 1))),
            )
        })
        .collect()
}

/// What the status line names for `content`, which `view` shows: a lone
/// document by its source, with no files, and each of several files by
/// its source.
pub(super) fn named<'s>(
    session: &'s Session,
    view: ViewId,
    content: Content,
) -> (&'s str, Vec<&'s Source>) {
    let sources = session.sources_of(view);
    match content {
        Content::Document(_) => (sources[0].name(), Vec::new()),
        Content::Files(_) => ("", sources),
    }
}

/// The names of the views on the deck, from the base up, which the status
/// line tells as a path of bare file names; none while the deck holds the
/// base alone. The base is named by the file the link was followed in.
pub(super) fn path<'s>(session: &'s Session, state: &State) -> Vec<&'s str> {
    let base = session.base().id();
    let views = state.deck.sheets().filter_map(|sheet| match sheet {
        Sheet::Base => Some(base),
        Sheet::View(view) => Some(view),
        Sheet::Help => None,
    });
    let source = |view| {
        let sources = session.sources_of(view);
        let file = if view == base {
            state.base_file()
        } else {
            None
        };
        sources.get(file.unwrap_or(0)).or(sources.first()).copied()
    };
    let bare = |source: &'s Source| {
        let name = source.name();
        Path::new(name)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or(name)
    };
    let names: Vec<&str> = views.filter_map(source).map(bare).collect();
    match names.len() {
        0 | 1 => Vec::new(),
        _ => names,
    }
}

/// What a view shows: the content lent at the start, or a document the
/// loop read, as a reload or an open.
pub(super) enum Shows<'a> {
    Lent(Content<'a>),
    Read(Document),
}

impl Shows<'_> {
    fn content(&self) -> Content<'_> {
        match self {
            Shows::Lent(content) => *content,
            Shows::Read(document) => Content::Document(document),
        }
    }
}

/// The document sheets the loop keeps besides their layouts: what each
/// shows and where its view was.
pub(super) struct Views<'a> {
    shows: HashMap<Sheet, Shows<'a>>,
    /// What the columns of the long tables of each document asked for over
    /// all their rows, when `r` or `:w` measured them: kept through an
    /// automatic reload, which does not measure them again.
    tables: HashMap<Sheet, Tables>,
    places: HashMap<Sheet, View>,
    /// The commands that stop the animations of a view that was covered,
    /// sent once the next layout has a writer.
    stops: Vec<Command>,
}

impl<'a> Views<'a> {
    /// The views at the start: the base shows `content`, and keeps
    /// `tables`, what was measured of its long tables for the layout made
    /// before the pager.
    pub(super) fn starting_with(content: Content<'a>, tables: Tables) -> Self {
        Views {
            shows: HashMap::from([(Sheet::Base, Shows::Lent(content))]),
            tables: HashMap::from([(Sheet::Base, tables)]),
            places: HashMap::new(),
            stops: Vec::new(),
        }
    }

    /// What the document of `sheet` shows.
    pub(super) fn content(&self, sheet: Sheet) -> Content<'_> {
        self.shows[&sheet].content()
    }

    /// What the document of `sheet` shows, with `inputs` carrying what its
    /// long tables were measured to ask for, for the layout of its round. A
    /// long CSV that nothing is kept for, as at its first round, takes what
    /// its parse found of all its rows, and keeps it.
    pub(super) fn lend_tables(&mut self, sheet: Sheet, inputs: &mut Inputs) -> Content<'_> {
        let content = self.shows[&sheet].content();
        let kept = self.tables.entry(sheet).or_default();
        driver::scanned(content, inputs, kept);
        inputs.tables = Some(kept.clone());
        content
    }

    /// Measures the long tables of the document of `sheet` over all their
    /// rows, with `inputs`, and keeps what they ask for.
    pub(super) fn fit(&mut self, sheet: Sheet, inputs: &Inputs) {
        let fitted = driver::fitted(self.content(sheet), inputs);
        self.tables.insert(sheet, fitted);
    }

    /// The document of `sheet` was read again: it shows `document` now.
    pub(super) fn read(&mut self, sheet: Sheet, document: Document) {
        self.shows.insert(sheet, Shows::Read(document));
    }

    /// Where the view of `sheet` was when its round ended, taken; `None`
    /// for a sheet that has not had a round.
    pub(super) fn place_of(&mut self, sheet: Sheet) -> Option<View> {
        self.places.remove(&sheet)
    }

    /// The view of `sheet` was at `view` when its round ended.
    pub(super) fn left_at(&mut self, sheet: Sheet, view: View) {
        self.places.insert(sheet, view);
    }

    /// The commands that stop the animations of the view that was covered,
    /// once, as the next round sends them.
    pub(super) fn take_stops(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.stops)
    }

    /// Lets go of what is held of the sheets that are not `listed`.
    fn retain(&mut self, listed: &[Sheet]) {
        self.shows.retain(|sheet, _| listed.contains(sheet));
        self.places.retain(|sheet, _| listed.contains(sheet));
        self.tables.retain(|sheet, _| listed.contains(sheet));
    }
}

impl<'a, 'r, 'l, T: Terminal> Driver<'a, 'r, 'l, T> {
    /// The loop at the start: `content` laid out for `inputs` as they are
    /// now, or `laid_out` when it is, shown on `terminal` as `options` say.
    /// `session` must have a source for each file of `content`, or one at
    /// least for a document.
    pub(super) fn new(
        content: Content<'a>,
        laid_out: Option<Laid>,
        session: Session,
        inputs: &'a mut Inputs,
        options: Options<'r>,
        loads: &'l mut Loads,
        terminal: &'a mut T,
    ) -> Self {
        // The pager's cell layout shows every row of a long table, in
        // stretches, as the layout made before it does.
        inputs.tables.get_or_insert_with(Default::default);
        let base = session.base().id();
        let scrolls = inputs.caps.scroll;
        session.assert_fits(base, content);
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
            animation,
        } = options;
        assert!(
            session.reread(base).is_none() || files.is_some(),
            "a session with a file to read comes with a way to read it"
        );
        let mut state = State::new(case);
        state.jump = jump;
        animate::begin(&mut state, animation, &inputs.caps);
        let (theme, reported) = (inputs.theme.background, inputs.caps.background);
        state.background = Backgrounds::new(theme, reported, background_offset);
        state.snap =
            (inputs.reading.as_ref().or(reading.as_ref())).is_some_and(|reading| reading.snap);
        state.document_mut().warn(warnings, terminal.now());
        if session.reread(base).is_some() {
            state.follow.mode = follow;
        } else if follow != settings::Follow::Off {
            state.told = Some(Told::new(NOTHING_TO_FOLLOW));
        }
        let size = terminal.size();
        Driver {
            views: Views::starting_with(content, inputs.tables.clone().unwrap_or_default()),
            layouts: Layouts::starting(laid_out, &inputs.caps),
            state,
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
                presenter: Presenter::scrolling(scrolls),
                terminal,
            },
        }
    }

    /// What a round starts with: the images that came go in, the window's
    /// size is taken in and the images of the document on top are planned.
    /// Gives the sheet on top, its view, and the capabilities the layout
    /// is made for.
    pub(super) fn begin(&mut self) -> (Sheet, ViewId, Capabilities) {
        // Nothing uses the images yet, so the ones that came go in now.
        for Arrived { key, image, .. } in self.tools.source.arrived.drain(..) {
            self.tools.inputs.images.came(key, image);
        }
        let caps = &mut self.tools.inputs.caps;
        fit(caps, self.size, self.width, &mut self.window_cell);
        let (sheet, view) = self.top();
        self.plan(sheet, view);
        (sheet, view, self.tools.inputs.caps.clone())
    }

    /// The sheet of the document on top, and its view.
    pub(super) fn top(&self) -> (Sheet, ViewId) {
        let sheet = self.state.deck.document_sheet();
        match sheet {
            Sheet::View(view) => (sheet, view),
            Sheet::Base => (sheet, self.session.base().id()),
            Sheet::Help => unreachable!("the sheet of a document is never the help"),
        }
    }

    /// Plans the images the document on top shows, when the mode or the
    /// cells are not those it was planned for, or another view was
    /// planned last, and gives those that start loading something to
    /// stand in for them.
    pub(super) fn plan(&mut self, sheet: Sheet, view: ViewId) {
        let inputs = &mut *self.tools.inputs;
        let source = &mut self.tools.source;
        let now = (view, inputs.reading.is_some(), cell_size(&inputs.caps));
        if source.planned_for != Some(now) {
            let shown = self.session.view(view).expect("the view on top is open");
            let sources = self.session.sources_of(view);
            let planned = Planned {
                content: self.views.content(sheet),
                first: shown.document(0),
                paths: sources.iter().map(|source| source.path()).collect(),
            };
            source.plan(planned, view, inputs);
        }
        let keys = source.planned.iter().map(|job| &job.key);
        for key in loading::starting(keys, &inputs.images) {
            inputs.images.wait(key, Placeholder::Unsized);
            self.layouts.drop_layout(sheet);
        }
    }

    /// Does the turn a layout ended with, between layouts: opens the file a
    /// link names over the view on top, or goes back to a view below, and
    /// tells the state how it went.
    pub(super) fn turn(&mut self, turn: Option<Turn>) {
        match turn {
            None => {}
            Some(Turn::Open(opening)) => {
                let result = self.open_view(&opening.path, Input::Markdown);
                self.state.opened(*opening, result);
            }
            Some(Turn::Back(back)) => {
                let ahead = self.ahead_of(back.left);
                self.back_to_view(back.view);
                self.state.went_back(ahead);
                self.state.returned(back.place);
            }
        }
    }

    /// The files of the views that going back closes, with where each was
    /// left, for going forward, the one that leaves first first. Taken
    /// before the views are closed; a view with no path has nothing to
    /// open again.
    fn ahead_of(&self, left: Vec<Left>) -> Vec<Ahead> {
        let path = |view| {
            let sources = self.session.sources_of(view);
            sources.first()?.path().map(Path::to_path_buf)
        };
        (left.into_iter())
            .filter_map(|Left { view, place }| {
                Some(Ahead {
                    path: path(view)?,
                    place,
                })
            })
            .collect()
    }

    /// Opens the file at `path`, read as `input`, as a view of its own over
    /// the document on top: its name is the path as given. The view is
    /// pushed once the document is read; when it cannot be, the session
    /// has no trace of it and the reason is given.
    pub(super) fn open_view(&mut self, path: &Path, input: Input) -> Result<ViewId, String> {
        let file = SourceFile {
            path: path.to_path_buf(),
            input,
            look: None,
        };
        let view = self.session.open(path.display().to_string(), file);
        let document = (self.session.view(view)).expect("the view was just opened");
        let document = document.document(0);
        let files = self.tools.source.files.as_deref_mut();
        let Some(files) = files else {
            self.session.close(view);
            return Err("cannot be opened here".to_owned());
        };
        let inputs = &mut *self.tools.inputs;
        let loads = &mut *self.tools.source.loads;
        let (look, reloaded) = files.open(
            path,
            input,
            document,
            &mut inputs.images,
            loads,
            &inputs.caps,
        );
        let (document, warnings) = match reloaded {
            Reloaded::Document(document) => (document, Vec::new()),
            Reloaded::Warned(document, warnings) => (document, warnings),
            Reloaded::Failed(why) => {
                self.session.close(view);
                return Err(why);
            }
        };
        let sources = self.session.sources_of(view);
        if let Some(source) = sources.first() {
            let id = source.id();
            self.session.looked(id, look);
        }
        self.views.read(Sheet::View(view), document);
        let now = self.tools.terminal.now();
        let stops = self.state.push_view(view, warnings, now);
        self.views.stops.extend(stops);
        Ok(view)
    }

    /// Goes back to `view`, which is on the deck, closing the views over
    /// it: what is held of their layouts, images and files goes, and the
    /// next round lets go of what they showed.
    pub(super) fn back_to_view(&mut self, view: ViewId) {
        for gone in self.state.back_to(view) {
            let Some(shown) = self.session.view(gone) else {
                continue;
            };
            let number = shown.document(0);
            self.layouts.drop_layout(Sheet::View(gone));
            self.tools.inputs.images.forget(number);
            if let Some(files) = self.tools.source.files.as_deref_mut() {
                files.forget(number);
            }
            self.session.close(gone);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::typeset::reading::Fonts;
    use crate::typeset::reading::engine::Reading;
    use crate::typeset::reading::page::Wide;
    use crate::typeset::size::TextSize;
    use crate::typeset::style::Span;

    fn kitty(width: u16) -> Inputs {
        Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(width)
        })
    }

    fn views() -> Views<'static> {
        Views::starting_with(Content::Files(&[]), Tables::default())
    }

    fn help_lines(layouts: &Layouts) -> &[Line] {
        let panel = layouts.panel(Sheet::Help);
        &panel.expect("the help is laid out").lines
    }

    #[test]
    fn each_view_has_its_own_place_which_is_taken_once() {
        let mut views = Views::starting_with(Content::Files(&[]), Tables::default());
        let at = |top| View::at(vec![1; 20], 5, top);
        let other = Sheet::View(crate::session::sources::ViewId::BASE);
        views.left_at(Sheet::Base, at(3));
        views.left_at(other, at(7));
        assert_eq!(views.place_of(Sheet::Base).map(|view| view.top()), Some(3));
        assert!(views.place_of(Sheet::Base).is_none(), "taken once");
        assert_eq!(views.place_of(other).map(|view| view.top()), Some(7));
    }

    #[test]
    fn making_the_layouts_ready_lets_go_of_the_views_off_the_deck() {
        let mut views = views();
        let at = |top| View::at(vec![1; 20], 5, top);
        let gone = Sheet::View(crate::session::sources::ViewId::BASE);
        views.read(gone, crate::typeset::parse::parse("a\n"));
        views.left_at(gone, at(7));
        views.left_at(Sheet::Base, at(3));
        let mut layouts = Layouts::default();
        layouts.ready([Sheet::Base].into_iter(), &mut views, &kitty(80));
        assert!(!views.shows.contains_key(&gone));
        assert!(views.place_of(gone).is_none());
        assert!(views.shows.contains_key(&Sheet::Base));
        assert_eq!(views.place_of(Sheet::Base).map(|view| view.top()), Some(3));
    }

    #[test]
    fn the_help_is_laid_out_as_plain_text_in_any_terminal() {
        // A terminal with text sizing, pictures and reading mode.
        let mut inputs = kitty(100);
        inputs.caps.text_sizing = true;
        inputs.reading = Some(Reading {
            pictures: Default::default(),
            fonts: Rc::new(Fonts::fixture()),
            wide: Wide::Scroll,
            snap: false,
        });
        let mut layouts = Layouts::default();
        layouts.ready(
            [Sheet::Base, Sheet::Help].into_iter(),
            &mut views(),
            &inputs,
        );
        let lines = help_lines(&layouts);
        assert!(!lines.is_empty());
        for line in lines {
            let spans = [&line.lead[..], &line.spans[..], &line.trail[..]].concat();
            assert!(
                crate::typeset::wrap::width(&spans) <= help::fit(),
                "{line:?}"
            );
            assert!(line.image.is_none(), "{line:?}");
            assert!(spans.iter().all(|span| span.embed.is_none()), "{line:?}");
            let normal = |span: &Span| span.style.size == TextSize::NORMAL;
            assert!(spans.iter().all(normal), "{line:?}");
        }
        assert!(
            lines
                .iter()
                .any(|line| text(&line.spans).contains("Moving"))
        );
    }

    #[test]
    fn each_topic_starts_at_its_heading() {
        let mut layouts = Layouts::default();
        layouts.ready([Sheet::Help].into_iter(), &mut views(), &kitty(80));
        let panel = layouts.panel(Sheet::Help).unwrap();
        let mut at = 0;
        for (topic, heading) in help::TOPICS {
            let line = panel.line_of(topic);
            // The first topic starts on the blank row above its heading.
            let heading_line = line + usize::from(topic == Topic::Moving);
            assert_eq!(text(&panel.lines[heading_line].spans).trim(), heading);
            assert!(line >= at, "{heading}: the topics come in order");
            at = line;
        }
        assert_eq!(panel.line_of(Topic::Moving), 0);
    }

    #[test]
    fn the_panel_is_as_wide_as_the_window_allows() {
        let mut layouts = Layouts::default();
        layouts.ready([Sheet::Help].into_iter(), &mut views(), &kitty(30));
        for line in help_lines(&layouts) {
            assert!(crate::typeset::wrap::width(&line.spans) <= 25, "{line:?}");
        }
        assert_eq!(layouts.panel(Sheet::Help).unwrap().width, 25);
    }

    #[test]
    fn a_sheet_the_deck_no_longer_lists_is_dropped() {
        let inputs = kitty(80);
        let mut layouts = Layouts::default();
        layouts.ready(
            [Sheet::Base, Sheet::Help].into_iter(),
            &mut views(),
            &inputs,
        );
        assert!(layouts.panel(Sheet::Help).is_some());
        layouts.ready([Sheet::Base].into_iter(), &mut views(), &inputs);
        assert!(layouts.panel(Sheet::Help).is_none());
    }

    #[test]
    fn the_document_s_layout_stays_when_the_panels_go() {
        let inputs = kitty(80);
        let mut layouts = Layouts::starting(Some(Laid::default()), &inputs.caps);
        layouts.ready(
            [Sheet::Base, Sheet::Help].into_iter(),
            &mut views(),
            &inputs,
        );
        layouts.drop_stale(false);
        assert!(layouts.panel(Sheet::Help).is_none());
        assert!(layouts.kept.contains_key(&Sheet::Base));
        layouts.ready([Sheet::Base].into_iter(), &mut views(), &inputs);
        assert!(layouts.kept.contains_key(&Sheet::Base));
    }

    #[test]
    fn a_change_of_caps_lays_the_help_out_anew() {
        let mut layouts = Layouts::default();
        layouts.ready([Sheet::Help].into_iter(), &mut views(), &kitty(80));
        let wide = help_lines(&layouts)[0].spans.clone();
        // The same caps keep the layout; another width makes a new one.
        layouts.ready([Sheet::Help].into_iter(), &mut views(), &kitty(80));
        assert_eq!(help_lines(&layouts)[0].spans, wide);
        layouts.ready([Sheet::Help].into_iter(), &mut views(), &kitty(20));
        let rows = |lines: &[Line]| {
            let width = |line: &Line| crate::typeset::wrap::width(&line.spans);
            lines.iter().map(width).max().unwrap_or(0)
        };
        assert!(rows(help_lines(&layouts)) <= 16);
    }
}
