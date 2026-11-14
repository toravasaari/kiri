//! The layouts the loop keeps, by the sheet of the deck they are of: the
//! document's, which a reuse carries from one round to the next, and the
//! panels', which are laid out at their own width while the deck lists
//! them.

use std::collections::HashMap;

use super::{Driver, Planned, Terminal, cell_size, loading, shown_content};
use crate::images::{Images, Placeholder};
use crate::render::{Headings, Inputs};
use crate::session::commands::Topic;
use crate::session::deck::Sheet;
use crate::session::panels::{self, Panel, help};
use crate::typeset::caps::{Capabilities, Graphics};
use crate::typeset::driver::{self, Content, Stream};
use crate::typeset::locate::Map;
use crate::typeset::style::{Line, text};

/// A layout that is still good: the caps it is for, its lines and, once
/// it was shown, their map, which stays good with them.
struct Kept {
    caps: Capabilities,
    stream: Stream,
    map: Option<Map>,
}

/// A panel's layout that is still good, for the caps it is for.
struct Aside {
    caps: Capabilities,
    panel: Panel,
}

/// The layouts the loop keeps: the document's, and the panels' by their
/// sheet.
#[derive(Default)]
pub(super) struct Layouts {
    base: Option<Kept>,
    panels: HashMap<Sheet, Aside>,
}

impl Layouts {
    /// The layouts at the start: the document laid out beforehand for
    /// `caps`, if it was.
    pub(super) fn starting(laid_out: Option<Stream>, caps: &Capabilities) -> Self {
        let mut layouts = Self::default();
        if let Some(stream) = laid_out {
            layouts.keep_base(caps.clone(), stream, None);
        }
        layouts
    }

    /// The lines of the document for a window with `rows` for them: the
    /// layout kept when it is still good, with its map if it has one, and
    /// otherwise a new one of `content`.
    pub(super) fn base(
        &mut self,
        content: Content,
        inputs: &Inputs,
        rows: usize,
    ) -> (Stream, Option<Map>) {
        let (mut stream, map) = match self.base.take() {
            Some(kept) if kept.caps == inputs.caps => (kept.stream, kept.map),
            _ => (driver::content(content, inputs), None),
        };
        // The text sizing protocol cannot draw part of a line, so a line
        // taller than the window is drawn at normal size.
        for line in &mut stream.lines {
            if usize::from(crate::present::output::rows(line, &inputs.caps)) > rows {
                line.to_normal_size();
            }
        }
        (stream, map)
    }

    /// Keeps the document's layout, with its map, for the next round.
    pub(super) fn keep_base(&mut self, caps: Capabilities, stream: Stream, map: Option<Map>) {
        self.base = Some(Kept { caps, stream, map });
    }

    /// Forgets the document's layout: images it waits for began to load.
    pub(super) fn drop_base(&mut self) {
        self.base = None;
    }

    /// Forgets the panels' layouts, which a new theme, background or
    /// reading mode makes out of date.
    pub(super) fn drop_panels(&mut self) {
        self.panels.clear();
    }

    /// Makes the layouts of `sheets` ready: the panels the deck no longer
    /// lists are let go, and a panel that has none, or whose caps changed,
    /// is laid out at its width.
    pub(super) fn ready(&mut self, sheets: impl Iterator<Item = Sheet>, inputs: &Inputs) {
        let listed: Vec<Sheet> = sheets.collect();
        self.panels.retain(|sheet, _| listed.contains(sheet));
        for sheet in listed {
            if (self.panels.get(&sheet)).is_some_and(|aside| aside.caps == inputs.caps) {
                continue;
            }
            let width = panels::width(usize::from(inputs.caps.width));
            let width = u16::try_from(width).unwrap_or(u16::MAX);
            let panel = match sheet {
                Sheet::Base => continue,
                Sheet::Help => {
                    let document = help::document();
                    let inputs = panel_inputs(inputs, width);
                    let lines = driver::content(Content::Document(&document), &inputs).lines;
                    Panel {
                        topics: topics(&lines),
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
/// line of text that is the heading and nothing else (the lines of a list
/// have a bullet and the keys in them).
fn topics(lines: &[Line]) -> Vec<(Topic, usize)> {
    let at = |heading: &str| {
        lines
            .iter()
            .position(|line| text(&line.spans).trim() == heading)
    };
    let found = help::TOPICS
        .iter()
        .map(|&(topic, heading)| (topic, at(heading)));
    found
        .map(|(topic, line)| (topic, line.unwrap_or(0)))
        .collect()
}

/// The inputs for a panel `width` cells wide: text only, with no scaled
/// headings, heading images or pictures, and no reading mode; the theme
/// and the other looks as the document's.
fn panel_inputs(inputs: &Inputs, width: u16) -> Inputs {
    let caps = Capabilities {
        width,
        text_sizing: false,
        graphics: Graphics::None,
        ..inputs.caps.clone()
    };
    Inputs {
        images: Images::default(),
        caps,
        theme: inputs.theme.clone(),
        front_matter: inputs.front_matter,
        headings: Headings::Plain,
        table_rows: inputs.table_rows,
        render_errors: inputs.render_errors,
        reading: None,
        diagram_panel: false,
    }
}

impl<T: Terminal> Driver<'_, '_, '_, T> {
    /// Plans the images the shown content wants when the mode or the cells
    /// are not those it was planned for, and gives those that start loading
    /// something to stand in for them.
    pub(super) fn plan(&mut self) {
        let inputs = &mut *self.tools.inputs;
        let source = &mut self.tools.source;
        if source.planned_for != Some((inputs.reading.is_some(), cell_size(&inputs.caps))) {
            let view = self.session.base();
            let sources = self.session.sources_of(view.id());
            let planned = Planned {
                content: shown_content(self.content, &self.reloaded),
                first: view.document(0),
                paths: sources.iter().map(|source| source.path()).collect(),
            };
            source.plan(planned, inputs);
        }
        let keys = source.planned.iter().map(|job| &job.key);
        for key in loading::starting(keys, &inputs.images) {
            inputs.images.wait(key, Placeholder::Unsized);
            self.layouts.drop_base();
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

    fn help_lines(layouts: &Layouts) -> &[Line] {
        let panel = layouts.panel(Sheet::Help);
        &panel.expect("the help is laid out").lines
    }

    #[test]
    fn panel_inputs_lay_out_plain_text() {
        // A terminal with text sizing, pictures and reading mode.
        let mut inputs = kitty(100);
        inputs.caps.text_sizing = true;
        inputs.reading = Some(Reading {
            pictures: Default::default(),
            fonts: Rc::new(Fonts::fixture()),
            wide: Wide::Scroll,
        });
        let mut layouts = Layouts::default();
        layouts.ready([Sheet::Base, Sheet::Help].into_iter(), &inputs);
        let lines = help_lines(&layouts);
        assert!(!lines.is_empty());
        for line in lines {
            let spans = [&line.lead[..], &line.spans[..], &line.trail[..]].concat();
            assert!(crate::typeset::wrap::width(&spans) <= 48, "{line:?}");
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
        layouts.ready([Sheet::Help].into_iter(), &kitty(80));
        let panel = layouts.panel(Sheet::Help).unwrap();
        let mut at = 0;
        for (topic, heading) in help::TOPICS {
            let line = panel.line_of(topic);
            assert_eq!(text(&panel.lines[line].spans).trim(), heading);
            assert!(line >= at, "{heading}: the topics come in order");
            at = line;
        }
        assert_eq!(panel.line_of(Topic::Moving), 0);
    }

    #[test]
    fn the_panel_is_as_wide_as_the_window_allows() {
        let mut layouts = Layouts::default();
        layouts.ready([Sheet::Help].into_iter(), &kitty(30));
        for line in help_lines(&layouts) {
            assert!(crate::typeset::wrap::width(&line.spans) <= 26, "{line:?}");
        }
    }

    #[test]
    fn a_sheet_the_deck_no_longer_lists_is_dropped() {
        let inputs = kitty(80);
        let mut layouts = Layouts::default();
        layouts.ready([Sheet::Base, Sheet::Help].into_iter(), &inputs);
        assert!(layouts.panel(Sheet::Help).is_some());
        layouts.ready([Sheet::Base].into_iter(), &inputs);
        assert!(layouts.panel(Sheet::Help).is_none());
    }

    #[test]
    fn the_document_s_layout_stays_when_the_panels_go() {
        let inputs = kitty(80);
        let mut layouts = Layouts::starting(Some(Stream::default()), &inputs.caps);
        layouts.ready([Sheet::Base, Sheet::Help].into_iter(), &inputs);
        layouts.drop_panels();
        assert!(layouts.panel(Sheet::Help).is_none());
        assert!(layouts.base.is_some());
        layouts.ready([Sheet::Base].into_iter(), &inputs);
        assert!(layouts.base.is_some());
    }

    #[test]
    fn a_change_of_caps_lays_the_help_out_anew() {
        let mut layouts = Layouts::default();
        layouts.ready([Sheet::Help].into_iter(), &kitty(80));
        let wide = help_lines(&layouts)[0].spans.clone();
        // The same caps keep the layout; another width makes a new one.
        layouts.ready([Sheet::Help].into_iter(), &kitty(80));
        assert_eq!(help_lines(&layouts)[0].spans, wide);
        layouts.ready([Sheet::Help].into_iter(), &kitty(20));
        let rows = |lines: &[Line]| {
            let width = |line: &Line| crate::typeset::wrap::width(&line.spans);
            lines.iter().map(width).max().unwrap_or(0)
        };
        assert!(rows(help_lines(&layouts)) <= 16);
    }
}
