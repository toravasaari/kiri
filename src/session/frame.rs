//! What the screen shows of one layout of the document: its lines, the
//! view of them, the status line and the panels, as a frame to draw.

use std::time::Instant;

use crate::session::animation;
use crate::session::fade::{Fade, Rgb, WARNING_STAY};
use crate::session::files::Files;
use crate::session::help::{self, Placed};
use crate::session::search::Marks;
use crate::session::state::State;
use crate::session::view::View;
use crate::session::{status, warnings};
use crate::typeset::block::BlockInfo;
use crate::typeset::color::Color;
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

/// One layout of the document: its lines, how search matches in them are
/// marked, the view of them and the status line below it.
pub(crate) struct Layout<'a> {
    pub(crate) lines: &'a [Line],
    /// The blocks the lines come from.
    pub(crate) places: &'a [Option<BlockInfo>],
    pub(crate) marks: &'a Marks,
    /// The colours a fade mixes, where they are known.
    pub(crate) rgb: Option<&'a Rgb>,
    /// The cells of the text area, which a tint spans.
    pub(crate) columns: usize,
    pub(crate) status: &'a Status<'a>,
    pub(crate) view: &'a mut View,
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
    /// What the screen shows in `state` at `now`.
    pub(crate) fn frame(&self, state: &State, now: Instant) -> Frame {
        let visible = self.view.visible();
        let fade = state.fade(now, self.rgb);
        let out = state.fade_out(now, self.rgb);
        let link = (state.links).selected_on(self.lines, self.places, visible.clone());
        Frame {
            lines: {
                // The rows of a split above and below the screen are painted
                // too, so that a mark on the image's text row shows on its
                // rows in view.
                let wide = painted_range(self.lines, visible.clone());
                let mut painted = state.marked(
                    &self.lines[wide.clone()],
                    wide.start,
                    self.marks,
                    fade,
                    self.rgb.and_then(|rgb| state.tint(now, rgb, self.columns)),
                    link.as_ref(),
                );
                // A selected animation has a frame round it.
                if let Some(key) = link.as_ref().and_then(|found| found.image.as_ref()) {
                    let color = self.marks.current.bg.or(self.marks.current.fg);
                    let color = color.unwrap_or(Color::Palette(3));
                    animation::frame_selected(&mut painted, key, color);
                }
                painted.truncate(visible.end - wide.start);
                painted.drain(..visible.start - wide.start);
                painted
            },
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
