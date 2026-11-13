//! What the screen shows of one layout of the document: its lines, the
//! view of them, the status line and the panels, as a frame to draw.

use std::cell::OnceCell;
use std::time::Instant;

use crate::present::scene::{Scene, Surface};
use crate::session::animation;
use crate::session::fade::{Fade, WARNING_STAY};
use crate::session::files::Files;
use crate::session::help::{self, Placed};
use crate::session::links::Kind;
use crate::session::reveal::Seen;
use crate::session::state::State;
use crate::session::view::View;
use crate::session::{status, warnings};
use crate::typeset::block::Blocks;
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
    pub(crate) styles: status::Styles,
    /// The window's columns.
    pub(crate) width: u16,
    /// Whether the window has a row for the status line besides the
    /// document's.
    pub(crate) bar: bool,
    /// The help panel's style, and that of its notes.
    pub(crate) panel: Style,
    pub(crate) muted: Style,
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
                let count = state.warnings.len() + state.notes.len();
                let file = state.file(&self.files, view.top());
                let name = file.map_or(self.name, |file| self.files.name(file));
                let name = self.loading.as_deref().unwrap_or(name);
                let place = (file.filter(|_| self.files.several()))
                    .map(|file| (file + 1, self.files.count()));
                let position = view.position();
                let tags = status::Tags {
                    follow: state.follow.mode,
                    reading: self.reading,
                    warnings: (count > 0).then(|| status::Warned {
                        count,
                        flash: counted.map(|fade| self.styles.number(fade)),
                    }),
                    pill: state.animation.pill_drawn(now),
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
                        Some(Kind::Link { url }) => Some(status::Selected {
                            name: url.clone(),
                            style: self.styles.link.clone(),
                            url: true,
                        }),
                        None => None,
                    },
                };
                status::spans(name, place, position, tags, found, width, &self.styles)
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
        let spans = warnings::spans(&state.all_warnings(), columns, &red, &dim);
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
    pub(crate) fn panel(&self, view: &View, open: bool) -> Vec<Placed> {
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

/// What a layout shows, as a search looks into it: the document or files
/// laid out, and the table of the blocks that the lines come from.
#[derive(Clone, Copy)]
pub(crate) struct Shown<'a> {
    pub(crate) content: Content<'a>,
    pub(crate) blocks: &'a Blocks,
    /// Whether the lines have links to select: the pages of reading mode do,
    /// and text does where the terminal shows hyperlinks. Where it does not,
    /// the text shows the address after the link, and there is nothing to
    /// select.
    pub(crate) links: bool,
}

impl<'a> Shown<'a> {
    /// `content` laid out as the table `blocks` says, with links to select
    /// or not.
    pub(crate) fn new(content: Content<'a>, blocks: &'a Blocks, links: bool) -> Self {
        Self {
            content,
            blocks,
            links,
        }
    }
}

/// One layout of the document: its lines, how search matches in them are
/// marked, the view of them and the status line below it.
pub(crate) struct Layout<'a> {
    pub(crate) lines: &'a [Line],
    /// What the lines show, and the blocks they come from.
    pub(crate) shown: Shown<'a>,
    pub(crate) marks: &'a Marks,
    /// The colours a fade mixes, where they are known.
    pub(crate) rgb: Option<&'a Rgb>,
    /// The cells of the text area, which a tint spans.
    pub(crate) columns: usize,
    pub(crate) status: &'a Status<'a>,
    pub(crate) view: &'a mut View,
    /// Where the places of the lines are, made by the first to ask.
    pub(crate) map: OnceCell<Map>,
}

impl<'a> Layout<'a> {
    /// The layout of `lines`, which `shown` says what they show; `marks`,
    /// the colours a fade mixes and the cells of the text area are `looks`;
    /// `status` below it and `view` of it.
    pub(crate) fn new(
        lines: &'a [Line],
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
            map: OnceCell::new(),
        }
    }

    /// Where the places of the lines are: made once, when first asked.
    pub(crate) fn map(&self) -> &Map {
        map_of(&self.map, self.lines, self.shown.blocks)
    }

    /// What the lines say of where a range is: the map, the lines and the
    /// table of their blocks.
    pub(crate) fn seen(&self) -> Seen<'_> {
        Seen {
            map: self.map(),
            lines: self.lines,
            blocks: self.shown.blocks,
        }
    }
}

/// The map of `lines`, made in `cell` when it is not there yet.
pub(crate) fn map_of<'m>(cell: &'m OnceCell<Map>, lines: &[Line], blocks: &Blocks) -> &'m Map {
    cell.get_or_init(|| Map::new(lines, blocks))
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
    pub(crate) panel: Vec<Placed>,
    /// The warning line, drawn over the lines and under the panel.
    pub(crate) notice: Vec<Placed>,
}

impl Frame {
    /// The frame as a scene in `window`: its lines from the top, dimmed
    /// behind the panel; the status line on the row below the view's
    /// `rows` when it is not empty; the warning line; the panel's rows.
    pub(crate) fn scene(&self, rows: usize, window: (u16, u16)) -> Scene<'_> {
        let mut surfaces = vec![Surface::Lines {
            lines: &self.lines,
            dim: !self.panel.is_empty(),
        }];
        if !self.status.is_empty() {
            surfaces.push(Surface::Bar {
                row: rows + 1,
                column: 1,
                spans: &self.status,
            });
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
        let fade = state.fade(now, self.rgb);
        let out = state.fade_out(now, self.rgb);
        let link = state.link_on_screen(self);
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
                let mut painted = state.marked(self, wide.clone(), (now, fade), selected);
                // A selected animation has a frame round it.
                if let Some(Kind::Picture { key, .. }) = link.as_ref().map(|at| &at.stop.kind) {
                    let color = selected_frame(self.marks);
                    let shown = state.animation.shown_as(key);
                    animation::frame_selected(&mut painted, key, &shown, color);
                }
                painted.truncate(visible.end - wide.start);
                painted.drain(..visible.start - wide.start);
                painted
            },
            status: self.status.spans(
                self.view,
                state,
                (fade, out, state.count_fade(now, self.rgb)),
                // A selected picture is told by its name, not as a link.
                (
                    link.as_ref().map(|at| &at.stop.kind),
                    selected_frame(self.marks),
                ),
                now,
            ),
            panel: self.status.panel(self.view, state.help),
            notice: self.status.notice(self.view, state, now, self.rgb),
        }
    }

    /// Whether something on the screen fades in `state` at `now`, so
    /// that the pager draws it a step at a time.
    pub(crate) fn fades(&self, state: &State, now: Instant) -> bool {
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

    fn frame(status: &str, notice: Vec<Placed>, panel: Vec<Placed>) -> Frame {
        Frame {
            lines: vec![Line::new(vec![Span::new("text", Style::default())])],
            status: if status.is_empty() {
                Vec::new()
            } else {
                vec![Span::new(status, Style::default())]
            },
            panel,
            notice,
        }
    }

    #[test]
    fn a_frame_is_its_lines_under_the_status_line_the_warning_line_and_the_panel() {
        let frame = frame(
            "s",
            vec![placed(3, 9, "n")],
            vec![placed(1, 2, "p1"), placed(2, 2, "p2")],
        );
        let scene = frame.scene(3, (20, 4));
        assert_eq!(scene.window, (20, 4));
        let Surface::Lines { lines, dim } = &scene.surfaces[0] else {
            panic!("the lines come first");
        };
        assert_eq!((lines.len(), *dim), (1, true));
        let bars: Vec<_> = scene.surfaces[1..]
            .iter()
            .map(|surface| match surface {
                Surface::Bar { row, column, spans } => (*row, *column, spans[0].text.as_str()),
                Surface::Lines { .. } => panic!("one set of lines"),
            })
            .collect();
        assert_eq!(bars, [(4, 1, "s"), (3, 9, "n"), (1, 2, "p1"), (2, 2, "p2")]);
    }

    #[test]
    fn without_a_panel_nothing_is_dimmed_and_an_empty_status_line_is_no_bar() {
        let frame = frame("", Vec::new(), Vec::new());
        let scene = frame.scene(3, (20, 4));
        assert_eq!(scene.surfaces.len(), 1);
        assert!(matches!(
            scene.surfaces[0],
            Surface::Lines { dim: false, .. }
        ));
    }
}
