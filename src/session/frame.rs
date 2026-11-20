//! What the screen shows of one layout of the document: its lines, the
//! view of them, the status line and the panels, as a frame to draw.

use std::time::Instant;

use unicode_width::UnicodeWidthStr;

use crate::present::scene::{Area, Scene, Surface};
use crate::session::animation;
use crate::session::areas::{Areas, Role};
use crate::session::deck::StatusLine;
use crate::session::fade::{Fade, WARNING_STAY};
use crate::session::files::Files;
use crate::session::links::Kind;
use crate::session::panels::{self, Panel, Placed, Shape};
use crate::session::reveal::Seen;
use crate::session::sidebar;
use crate::session::state::State;
use crate::session::status::path::Named;
use crate::session::view::View;
use crate::session::{status, warnings};
use crate::typeset::color::{Color, Rgb};
use crate::typeset::driver::Content;
use crate::typeset::layout::layers::Marks;
use crate::typeset::locate::Map;
use crate::typeset::style::{Line, Span, Style};
use crate::typeset::wrap;

/// What the status line and the help panel show and how, apart from the
/// position.
pub(crate) struct Status<'a> {
    /// What a lone document is called.
    pub(crate) name: &'a str,
    /// What stands in place of the name while images load: how many have
    /// come of how many.
    pub(crate) loading: Option<String>,
    /// The files shown, each named after itself.
    pub(crate) files: Files<'a>,
    /// The names of the views on the deck, the base first and the view
    /// shown last; empty while the deck holds one view.
    pub(crate) path: Vec<&'a str>,
    /// The columns of the contents bar at the window's left; none when it
    /// is not shown.
    pub(crate) contents: u16,
    /// The look of the bar's rows.
    pub(crate) rows: sidebar::Looks,
    pub(crate) styles: status::Styles,
    /// The window's columns.
    pub(crate) width: u16,
    /// Whether the window has a row for the status line besides the
    /// document's.
    pub(crate) bar: bool,
    /// The help panel's style, and that of its notes.
    pub(crate) panel: Style,
    pub(crate) muted: Style,
    /// The bar of a panel that scrolls: the track as its background, the
    /// bar as its foreground, for the cells of whole and half blocks.
    pub(crate) scrollbar: Style,
    /// The style of the warning line: the theme's error colour on the
    /// page.
    pub(crate) red: Style,
    /// Whether the document is shown in reading mode.
    pub(crate) reading: bool,
}

impl Status<'_> {
    /// The status line for `view` in `state`, with the match number faded
    /// by `fade` after the search went around an end and a message faded
    /// out by `out`: the prompt while it is open.
    ///
    /// The mark for the document's warnings has its number flashing
    /// while `counted` is a fade.
    pub(crate) fn spans(
        &self,
        view: &View,
        state: &State,
        (fade, out, counted): (Option<Fade>, Option<Fade>, Option<Fade>),
        (selected, frame): (Option<&Kind>, Color),
        now: Instant,
    ) -> (Vec<Span>, Vec<Named>) {
        if !self.bar {
            return (Vec::new(), Vec::new());
        }
        let width = usize::from(self.width);
        match state.deck.status_line() {
            StatusLine::Prompt(term) => {
                (status::prompt('/', term, width, &self.styles), Vec::new())
            }
            StatusLine::Colon => (status::prompt(':', "", width, &self.styles), Vec::new()),
            // While the contents bar, when the window shows one, has the focus,
            // the line names its keys.
            StatusLine::Document if self.contents > 0 && state.selected_entry().is_some() => (
                status::contents(view.position(), width, &self.styles),
                Vec::new(),
            ),
            StatusLine::Document => {
                let flash = fade
                    .filter(|_| state.document().jumped.is_some_and(|jump| jump.around))
                    .map(|fade| self.styles.number(fade));
                let fading = out.and_then(|out| self.styles.fading(out));
                let found = state.found(flash, fading);
                let count = state.document().warnings.len() + state.document().notes.len();
                let file = state.document().file(&self.files, view.top());
                let name = file.map_or(self.name, |file| self.files.name(file));
                let name = self.loading.as_deref().unwrap_or(name);
                let place = (file.filter(|_| self.files.several()))
                    .map(|file| (file + 1, self.files.count()));
                let position = view.position();
                // The path of the views; while images load, that is told in
                // the name's place.
                let (name, trail) = match self.path.split_last() {
                    Some((last, below)) if self.loading.is_none() => {
                        (*last, below.iter().map(|name| (*name).to_owned()).collect())
                    }
                    _ => (name, Vec::new()),
                };
                let tags = status::Tags {
                    trail,
                    follow: state.follow.mode,
                    reading: self.reading,
                    warnings: (count > 0).then(|| status::Warned {
                        count,
                        flash: counted.map(|fade| self.styles.number(fade)),
                    }),
                    pill: state.document().animation.pill_drawn(now),
                    // The picture Tab selected, also when it is the one the
                    // pill tells of: the text goes with the frame.
                    selected: match selected {
                        // The animation Tab selected, in the colour of its
                        // frame.
                        Some(Kind::Picture { name, .. }) => Some(status::Selected {
                            name: name.clone(),
                            style: Style {
                                fg: Some(frame),
                                ..self.styles.bar.clone()
                            },
                            url: false,
                        }),
                        // The link that is selected, where it goes, in the
                        // colour of the links.
                        // Its title when it has one, which the writer meant
                        // to say what it is; else its address.
                        Some(Kind::Link { url, title, .. }) => Some(status::Selected {
                            name: title.as_ref().unwrap_or(url).clone(),
                            style: self.styles.link.clone(),
                            url: title.is_none(),
                        }),
                        None => None,
                    },
                };
                status::spans_named(name, place, position, tags, found, width, &self.styles)
            }
        }
    }

    /// The warning line over the last row of `view` at `now`, where the
    /// document's warnings in `state` are told; it fades out into the
    /// page with `rgb`'s colours after [`WARNING_STAY`].
    pub(crate) fn notice(
        &self,
        view: &View,
        state: &State,
        now: Instant,
        rgb: Option<&Rgb>,
    ) -> Vec<Placed> {
        let Some(at) = state.document().noticed else {
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
        let spans = warnings::spans(&state.document().all_warnings(), columns, &red, &dim);
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
}

/// What a layout shows, as a search looks into it: the document or files
/// laid out.
#[derive(Clone, Copy)]
pub(crate) struct Shown<'a> {
    pub(crate) content: Content<'a>,
    /// Whether the lines have links to select: the pages of reading mode do,
    /// and text does where the terminal shows hyperlinks. Where it does not,
    /// the text shows the address after the link, and there is nothing to
    /// select.
    pub(crate) links: bool,
}

impl<'a> Shown<'a> {
    /// `content` laid out, with links to select or not.
    pub(crate) fn new(content: Content<'a>, links: bool) -> Self {
        Self { content, links }
    }
}

/// One layout of the document: its lines, how search matches in them are
/// marked, the view of them and the status line below it.
pub(crate) struct Layout<'a> {
    pub(crate) lines: &'a [Line],
    /// What the lines show.
    pub(crate) shown: Shown<'a>,
    pub(crate) marks: &'a Marks,
    /// The colours a fade mixes, where they are known.
    pub(crate) rgb: Option<&'a Rgb>,
    /// The cells of the text area, which a tint spans.
    pub(crate) columns: usize,
    pub(crate) status: &'a Status<'a>,
    pub(crate) view: &'a mut View,
    /// Where the places of the lines are.
    pub(crate) map: &'a Map,
    /// The lines of the help, laid out, while the deck lists them.
    pub(crate) panel: Option<&'a Panel>,
}

impl<'a> Layout<'a> {
    /// The layout of `lines`, of which `map` is the map and `shown` says
    /// what they show; `marks`, the colours a fade mixes and the cells of
    /// the text area are `looks`; `status` below it and `view` of it.
    pub(crate) fn new(
        (lines, map): (&'a [Line], &'a Map),
        shown: Shown<'a>,
        (marks, rgb, columns): (&'a Marks, Option<&'a Rgb>, usize),
        status: &'a Status<'a>,
        view: &'a mut View,
    ) -> Self {
        Self {
            lines,
            shown,
            marks,
            rgb,
            columns,
            status,
            view,
            map,
            panel: None,
        }
    }

    /// What the lines say of where a range is: the map and the lines.
    pub(crate) fn seen(&self) -> Seen<'_> {
        Seen {
            map: self.map,
            lines: self.lines,
        }
    }
}

/// The cells of a laid-out line, as one run of spans.
fn spans_of(line: &Line) -> Vec<Span> {
    let mut spans = line.lead.clone();
    spans.extend(line.spans.iter().cloned());
    spans.extend(line.trail.iter().cloned());
    spans
}

/// `lines` from line `scroll`, in a frame of `style` titled `title`, as
/// `shape` says, with nothing on its bottom edge, and a bar of
/// `scrollbar`'s colours in a column of its own when the lines do not all
/// show. A window too short for the frame shows its top rows.
fn framed(
    lines: &[Line],
    scroll: usize,
    title: &str,
    (style, scrollbar): (&Style, &Style),
    shape: &Shape,
) -> Vec<Placed> {
    let span = |text: &str, style: &Style| Span::new(text, style.clone());
    let bold = Style {
        bold: true,
        ..style.clone()
    };
    let (columns, rows) = shape.window;
    let (inner, width) = (shape.text, shape.width);
    let rule = |count: usize| "─".repeat(count);
    // The title leaves room for the corners and a rule.
    let title = {
        let cut = wrap::cut(
            vec![span(&format!(" {title} "), style)],
            width.saturating_sub(4),
        );
        crate::typeset::style::text(&cut)
    };
    let thumb = shape.thumb(scroll);
    let side = |at: usize, spans: Vec<Span>| {
        let spans = wrap::cut(spans, inner);
        let used = wrap::width(&spans);
        let mut row = vec![span("│ ", style)];
        row.extend(spans);
        row.push(span(&" ".repeat(inner - used), style));
        match thumb {
            Some(thumb) => {
                row.push(span(" ", style));
                row.push(span(panels::bar_cell(at, thumb), scrollbar));
                row.push(span("│", style));
            }
            None => row.push(span(" │", style)),
        }
        row
    };
    let mut rows_of = vec![vec![
        span("╭─", style),
        span(&title, &bold),
        span(&rule((width - 3).saturating_sub(title.width())), style),
        span("╮", style),
    ]];
    let shown = lines.iter().skip(scroll).take(shape.shown);
    let empty = Line::new(Vec::new());
    let shown = shown.chain(std::iter::repeat(&empty));
    let shown = shown.take(shape.shown).enumerate();
    rows_of.extend(shown.map(|(at, line)| side(at, spans_of(line))));
    rows_of.push(vec![
        span("╰", style),
        span(&rule(width - 2), style),
        span("╯", style),
    ]);

    rows_of
        .into_iter()
        .take(rows)
        .enumerate()
        .map(|(i, spans)| Placed {
            row: shape.top + i,
            column: shape.left,
            spans: wrap::cut(spans, columns),
        })
        .collect()
}

/// The cells of the top `rows` rows of a window `columns` wide that the
/// panel `placed` leaves open round its frame: the rows above it and
/// below it across the window, and the cells left and right of it. None
/// without a panel.
fn shade_round(placed: &[Placed], rows: usize, columns: usize) -> Vec<Area> {
    let Some(first) = placed.first() else {
        return Vec::new();
    };
    let (left, top) = (first.column, first.row);
    let width = wrap::width(&first.spans);
    let bottom = (top + placed.len()).min(rows + 1);
    let right = (left + width).min(columns + 1);
    let areas = [
        (1, 1, columns, top.saturating_sub(1)),
        (1, bottom, columns, (rows + 1).saturating_sub(bottom)),
        (1, top, left.saturating_sub(1), bottom - top),
        (
            right,
            top,
            (columns + 1).saturating_sub(right),
            bottom - top,
        ),
    ];
    let area = |(column, row, columns, rows)| Area {
        column,
        row,
        columns,
        rows,
    };
    (areas.into_iter())
        .filter(|(_, _, columns, rows)| *columns > 0 && *rows > 0)
        .map(area)
        .collect()
}

/// The colour of the frame round the animation selected: the colour of the
/// current search match, or yellow.
fn selected_frame(marks: &Marks) -> Color {
    let color = marks.current.bg.or(marks.current.fg);
    color.unwrap_or(Color::Palette(3))
}

/// What the screen shows: the lines in view, the status line and the help
/// panel.
#[derive(PartialEq)]
pub(crate) struct Frame {
    pub(crate) lines: Vec<Line>,
    pub(crate) status: Vec<Span>,
    /// Where the names of the path of views are on the status line.
    pub(crate) path: Vec<Named>,
    pub(crate) panel: Vec<Placed>,
    /// The warning line, drawn over the lines and under the panel.
    pub(crate) notice: Vec<Placed>,
    /// The cells round the panel's frame, where pictures darken.
    pub(crate) shade: Vec<Area>,
    /// The window's areas, which the lines are drawn in.
    pub(crate) areas: Areas,
    /// The rows of the contents bar, one for each row of its area; none
    /// when the window has no bar.
    pub(crate) bar: Vec<Line>,
}

impl Frame {
    /// The frame as a scene in `window`: its lines from the top, dimmed
    /// behind the panel; the status line on the row below the view's
    /// `rows` when it is not empty; the warning line; the panel's rows.
    pub(crate) fn scene(&self, rows: usize, window: (u16, u16)) -> Scene<'_> {
        let mut surfaces = Vec::new();
        if let Some(area) = self.areas.area(Role::Document) {
            surfaces.push(Surface::Lines {
                area,
                lines: &self.lines,
                dim: !self.panel.is_empty(),
            });
        }
        if let Some(area) = self.areas.area(Role::Contents) {
            surfaces.push(Surface::Lines {
                area,
                lines: &self.bar,
                dim: !self.panel.is_empty(),
            });
        }
        if !self.status.is_empty() {
            surfaces.push(Surface::Bar {
                row: rows + 1,
                column: 1,
                spans: &self.status,
            });
        }
        if !self.shade.is_empty() {
            surfaces.push(Surface::Shade { areas: &self.shade });
        }
        for placed in self.notice.iter().chain(&self.panel) {
            surfaces.push(Surface::Bar {
                row: placed.row,
                column: placed.column,
                spans: &placed.spans,
            });
        }
        Scene { window, surfaces }
    }
}

/// The lines to paint for `visible`: the rows of a split that the lines in
/// view are some of go on above and below them, as far as its image's
/// canvas has rows.
fn painted_range(lines: &[Line], visible: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let rows = (lines[visible.clone()].iter())
        .flat_map(|line| &line.spans)
        .filter_map(|span| span.embed.as_ref())
        .map(|embed| usize::from(embed.placement.size.rows))
        .max()
        .unwrap_or(0);
    visible.start.saturating_sub(rows)..(visible.end + rows).min(lines.len())
}

impl Layout<'_> {
    /// The window in cells: the status line's width, and the view's rows
    /// with the status line's.
    pub(crate) fn window(&self) -> (u16, u16) {
        let rows = self.view.rows() + usize::from(self.status.bar);
        (self.status.width, u16::try_from(rows).unwrap_or(u16::MAX))
    }

    /// What the screen shows in `state` at `now`.
    pub(crate) fn frame(&self, state: &State, now: Instant) -> Frame {
        let visible = self.view.visible();
        let fade = state.document().fade(now, self.rgb);
        let out = state.fade_out(now, self.rgb);
        let link = state.document().link_on_screen(self);
        let (status, path) = self.status.spans(
            self.view,
            state,
            (fade, out, state.document().count_fade(now, self.rgb)),
            // A selected picture is told by its name, not as a link.
            (
                link.as_ref().map(|at| &at.stop.kind),
                selected_frame(self.marks),
            ),
            now,
        );
        let panel = self.panel_rows(state);
        Frame {
            lines: {
                // The rows of a split above and below the screen are painted
                // too, so that a mark on the image's text row shows on its
                // rows in view.
                let wide = painted_range(self.lines, visible.clone());
                // A link is highlighted; the frame of an animation selected
                // is drawn below.
                let selected = link.as_ref().and_then(|at| match at.stop.kind {
                    Kind::Link { .. } => Some(at.stop.range.clone()),
                    Kind::Picture { .. } => None,
                });
                let mut painted =
                    state
                        .document()
                        .marked(self, wide.clone(), (now, fade), selected);
                // A selected animation has a frame round it.
                if let Some(Kind::Picture { key, .. }) = link.as_ref().map(|at| &at.stop.kind) {
                    let color = selected_frame(self.marks);
                    let shown = state.document().animation.shown_as(key);
                    animation::frame_selected(&mut painted, key, &shown, color);
                }
                painted.truncate(visible.end - wide.start);
                painted.drain(..visible.start - wide.start);
                // A line with text of a larger scale than the rows left
                // below the others is drawn smaller to fit them, not left
                // out. The view never starts inside a line, so there is no
                // such line at the top.
                let fitted = self.view.left_over().and_then(|(line, left)| {
                    let line = line..line + 1;
                    let [next] = &mut state.document().marked(self, line, (now, fade), None)[..]
                    else {
                        return None;
                    };
                    next.fitted(u8::try_from(left).unwrap_or(u8::MAX))
                });
                painted.extend(fitted);
                painted
            },
            status,
            path,
            shade: shade_round(&panel, self.view.rows(), usize::from(self.status.width)),
            panel,
            notice: self.status.notice(self.view, state, now, self.rgb),
            areas: Areas::of(self.window(), self.status.contents),
            bar: self.bar_rows(state),
        }
    }

    /// The rows of the contents bar for `state`: the headings of the
    /// document on top, with the one the view is in marked.
    fn bar_rows(&self, state: &State) -> Vec<Line> {
        let columns = usize::from(self.status.contents);
        let edition = state.document().edition.as_ref();
        let Some(edition) = edition.filter(|_| columns > 0) else {
            return Vec::new();
        };
        let contents = &edition.contents;
        let current = state.current_entry(contents, self.map, self.view);
        let size = (columns, self.view.rows());
        sidebar::rows(
            contents,
            (&state.folds, &edition.names),
            (current, state.selected_entry()),
            size,
            &self.status.rows,
        )
    }

    /// Where the frame of the panel laid out is in the window: the one
    /// place that drawing it and answering the mouse over it both ask.
    pub(crate) fn panel_shape(&self) -> Option<Shape> {
        let panel = self.panel?;
        let window = (usize::from(self.status.width), self.view.rows());
        Some(Shape::new(window, panel.width, panel.len()))
    }

    /// The panel over the view, in its frame, when it is open and laid out.
    fn panel_rows(&self, state: &State) -> Vec<Placed> {
        let (Some(open), Some(panel), Some(shape)) =
            (state.deck.panel(), self.panel, self.panel_shape())
        else {
            return Vec::new();
        };
        let status = self.status;
        let styles = (&status.panel, &status.scrollbar);
        framed(&panel.lines, open.scroll.top(), open.title, styles, &shape)
    }

    /// Whether something on the screen fades in `state` at `now`, so
    /// that the pager draws it a step at a time.
    pub(crate) fn fades(&self, state: &State, now: Instant) -> bool {
        state.document().jumped.is_some()
            || state.document().counted.is_some() && self.status.bar
            || self.status.bar
                && self.rgb.is_some()
                && (state.document().noticed)
                    .is_some_and(|at| now.saturating_duration_since(at) >= WARNING_STAY)
            || state.document().changed.is_some() && self.rgb.is_some()
            || self.status.bar
                && (state.fade_out(now, self.rgb))
                    .and_then(|out| self.status.styles.fading(out))
                    .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(row: usize, column: usize, text: &str) -> Placed {
        Placed {
            row,
            column,
            spans: vec![Span::new(text, Style::default())],
        }
    }

    fn frame(window: (u16, u16), status: &str, notice: Vec<Placed>, panel: Vec<Placed>) -> Frame {
        Frame {
            lines: vec![Line::new(vec![Span::new("text", Style::default())])],
            status: if status.is_empty() {
                Vec::new()
            } else {
                vec![Span::new(status, Style::default())]
            },
            path: Vec::new(),
            panel,
            notice,
            shade: Vec::new(),
            areas: Areas::of(window, 0),
            bar: Vec::new(),
        }
    }

    #[test]
    fn a_frame_is_its_lines_under_the_status_line_the_warning_line_and_the_panel() {
        let frame = frame(
            (20, 4),
            "s",
            vec![placed(3, 9, "n")],
            vec![placed(1, 2, "p1"), placed(2, 2, "p2")],
        );
        let scene = frame.scene(3, (20, 4));
        assert_eq!(scene.window, (20, 4));
        let Surface::Lines { lines, dim, .. } = &scene.surfaces[0] else {
            panic!("the lines come first");
        };
        assert_eq!((lines.len(), *dim), (1, true));
        let bars: Vec<_> = scene.surfaces[1..]
            .iter()
            .map(|surface| match surface {
                Surface::Bar { row, column, spans } => (*row, *column, spans[0].text.as_str()),
                Surface::Lines { .. } | Surface::Shade { .. } => panic!("one set of lines"),
            })
            .collect();
        assert_eq!(bars, [(4, 1, "s"), (3, 9, "n"), (1, 2, "p1"), (2, 2, "p2")]);
    }

    #[test]
    fn without_a_panel_nothing_is_dimmed_and_an_empty_status_line_is_no_bar() {
        let frame = frame((20, 4), "", Vec::new(), Vec::new());
        let scene = frame.scene(3, (20, 4));
        assert_eq!(scene.surfaces.len(), 1);
        assert!(matches!(
            scene.surfaces[0],
            Surface::Lines { dim: false, .. }
        ));
    }

    fn area(column: usize, row: usize, columns: usize, rows: usize) -> Area {
        Area {
            column,
            row,
            columns,
            rows,
        }
    }

    /// The frame of 24 × 4 cells at column 19 and row 4, as it is for
    /// two lines of text 20 wide in a window of 60 × 10.
    fn a_frame() -> Vec<Placed> {
        framed_with(&plain(2), 0, (60, 10), 20)
    }

    #[test]
    fn the_shade_is_the_cells_round_the_frame_in_the_rows_of_the_view() {
        let areas = shade_round(&a_frame(), 10, 60);
        assert_eq!(
            areas,
            [
                area(1, 1, 60, 3),
                area(1, 8, 60, 3),
                area(1, 4, 18, 4),
                area(43, 4, 18, 4),
            ]
        );
    }

    #[test]
    fn a_frame_as_tall_or_as_wide_as_the_window_leaves_no_shade_above_or_beside() {
        let tall = framed_with(&plain(30), 0, (60, 10), 20);
        let areas = shade_round(&tall, 10, 60);
        assert_eq!(areas, [area(1, 1, 17, 10), area(43, 1, 18, 10)]);
        // 16 columns: the frame is 15 wide, and a cell is left on the right.
        let narrow = framed_with(&plain(2), 0, (16, 10), 11);
        let areas = shade_round(&narrow, 10, 16);
        assert!(areas.iter().all(|a| a.column + a.columns <= 17));
        assert!(areas.contains(&area(16, 4, 1, 4)), "{areas:?}");
        assert_eq!(shade_round(&[], 10, 60), []);
    }

    #[test]
    fn a_frame_with_shade_has_a_shade_surface_over_the_lines_and_under_the_bars() {
        let mut open = frame((60, 4), "s", Vec::new(), a_frame());
        open.shade = shade_round(&open.panel, 3, 60);
        let scene = open.scene(3, (60, 4));
        assert!(matches!(
            scene.surfaces[0],
            Surface::Lines { area, .. } if area == Area::window((60, 4))
        ));
        assert!(matches!(scene.surfaces[1], Surface::Bar { row: 4, .. }));
        let at = (scene.surfaces.iter())
            .position(|surface| matches!(surface, Surface::Shade { .. }))
            .expect("a shade");
        assert!(matches!(scene.surfaces[at + 1], Surface::Bar { .. }));
        let shut = frame((60, 4), "s", Vec::new(), Vec::new());
        let closed = shut.scene(3, (60, 4));
        assert!(
            closed
                .surfaces
                .iter()
                .all(|surface| !matches!(surface, Surface::Shade { .. }))
        );
    }

    // The frame of a panel.
    fn plain(count: usize) -> Vec<Line> {
        (0..count)
            .map(|n| Line::new(vec![Span::new(&format!("line {n}"), Style::default())]))
            .collect()
    }

    fn rows(placed: &[Placed]) -> Vec<String> {
        placed
            .iter()
            .map(|row| crate::typeset::style::text(&row.spans))
            .collect()
    }

    fn track() -> Style {
        Style {
            fg: Some(Color::Rgb(200, 200, 200)),
            bg: Some(Color::Rgb(40, 40, 40)),
            ..Style::default()
        }
    }

    /// `lines` from `scroll`, as wide as the text `inner`, in `window`.
    fn framed_with(
        lines: &[Line],
        scroll: usize,
        window: (usize, usize),
        inner: usize,
    ) -> Vec<Placed> {
        let style = Style::default();
        let shape = Shape::new(window, inner, lines.len());
        framed(lines, scroll, "Help", (&style, &track()), &shape)
    }

    fn framed_in(lines: &[Line], scroll: usize, window: (usize, usize)) -> Vec<Placed> {
        framed_with(lines, scroll, window, panels::width(window.0, 48))
    }

    #[test]
    fn the_frame_has_rounded_corners_a_title_and_nothing_on_the_bottom_edge() {
        let placed = framed_in(&plain(2), 0, (60, 10));
        let rows = rows(&placed);
        assert!(rows[0].starts_with("╭─ Help ─"), "{rows:#?}");
        assert!(rows[0].ends_with('╮'), "{rows:#?}");
        let width = rows[0].chars().count();
        assert_eq!(rows[3], format!("╰{}╯", "─".repeat(width - 2)));
    }

    #[test]
    fn the_frame_is_as_wide_as_the_text_needs_and_in_the_middle() {
        let placed = framed_with(&plain(2), 0, (60, 10), 20);
        // The text, two spaces and two edges: 24 cells, in the middle.
        assert!(rows(&placed).iter().all(|row| row.chars().count() == 24));
        assert!(placed.iter().all(|row| row.column == 19), "{placed:#?}");
        assert_eq!(
            placed.iter().map(|row| row.row).collect::<Vec<_>>(),
            [4, 5, 6, 7]
        );
        let rows = rows(&placed);
        assert!(rows[1].starts_with("│ line 0"), "{rows:#?}");
        assert!(rows[1].ends_with(" │"), "{rows:#?}");
    }

    #[test]
    fn a_help_that_fits_shows_no_bar() {
        let placed = framed_with(&plain(6), 0, (60, 10), 20);
        let rows = rows(&placed);
        assert!(
            rows.iter().all(|row| row.chars().count() == 24),
            "{rows:#?}"
        );
        assert!(
            rows.iter().all(|row| !row.contains(['█', '▀', '▄'])),
            "{rows:#?}"
        );
    }

    /// The cells of the bar's column in the rows of the lines.
    fn bar(placed: &[Placed]) -> String {
        let rows = rows(placed);
        let inside = &rows[1..rows.len() - 1];
        inside
            .iter()
            .map(|row| row.chars().rev().nth(1).unwrap())
            .collect()
    }

    #[test]
    fn the_bar_is_in_a_column_of_its_own_by_the_right_edge() {
        let placed = framed_with(&plain(30), 0, (60, 10), 20);
        let rows = rows(&placed);
        // The text, a space, the bar's cell and the edge: 25 cells.
        assert!(
            rows.iter().all(|row| row.chars().count() == 25),
            "{rows:#?}"
        );
        assert!(rows[1].ends_with("█│"), "{rows:#?}");
        assert_eq!(bar(&placed), "██      ");
    }

    #[test]
    fn the_bar_moves_by_half_a_row_and_stops_at_the_end() {
        assert_eq!(bar(&framed_with(&plain(30), 1, (60, 10), 20)), "▄█▀     ");
        assert_eq!(bar(&framed_with(&plain(30), 11, (60, 10), 20)), "   ██   ");
        assert_eq!(bar(&framed_with(&plain(30), 22, (60, 10), 20)), "      ██");
    }

    #[test]
    fn the_bar_is_the_thumb_colour_on_the_track_and_the_track_runs_the_whole_height() {
        let placed = framed_with(&plain(30), 0, (60, 10), 20);
        for row in &placed[1..placed.len() - 1] {
            let cell = &row.spans[row.spans.len() - 2];
            assert_eq!(cell.style, track(), "{row:#?}");
        }
    }

    #[test]
    fn the_frame_shows_the_lines_from_the_scroll() {
        let placed = framed_in(&plain(20), 3, (60, 6));
        let rows = rows(&placed);
        assert_eq!(rows.len(), 6);
        assert!(rows[1].starts_with("│ line 3 "), "{rows:#?}");
        assert!(rows[4].starts_with("│ line 6 "), "{rows:#?}");
    }

    #[test]
    fn a_window_too_short_shows_the_top_of_the_frame() {
        let rows_of = |height| rows(&framed_in(&plain(5), 0, (60, height)));
        assert_eq!(rows_of(1).len(), 1);
        assert!(rows_of(1)[0].starts_with("╭─ Help"));
        let two = rows_of(2);
        assert!(two[0].starts_with("╭─ Help") && two[1].starts_with("│ line 0"));
        let three = rows_of(3);
        assert!(three[1].starts_with("│ line 0") && three[2].starts_with('╰'));
    }

    #[test]
    fn a_very_narrow_window_cuts_the_title_to_the_text() {
        // 16 columns: the text is 11 wide, and the title is longer.
        let style = Style::default();
        let lines = plain(2);
        let shape = Shape::new((16, 10), panels::width(16, 48), lines.len());
        let framed = framed(&lines, 0, "A long title", (&style, &track()), &shape);
        let rows = rows(&framed);
        assert!(
            rows.iter().all(|row| row.chars().count() == 15),
            "{rows:#?}"
        );
        assert!(rows[0].ends_with('╮'), "{rows:#?}");
        assert!(rows.last().unwrap().ends_with('╯'), "{rows:#?}");
    }

    #[test]
    fn a_narrow_window_cuts_the_frame_to_its_columns() {
        let placed = framed_in(&plain(2), 0, (10, 10));
        assert!(placed.iter().all(|row| wrap::width(&row.spans) <= 10));
        let placed = framed_in(&plain(2), 0, (40, 10));
        assert!(placed.iter().all(|row| wrap::width(&row.spans) == 39));
        assert!(placed.iter().all(|row| row.column == 1));
    }
}
