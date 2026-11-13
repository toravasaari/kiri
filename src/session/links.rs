//! What Tab stops at: the links of the document, and the pictures that
//! play, as ranges of its text in the order of the document.
//!
//! The model lists the links as they are written (`typeset::targets`), and
//! this keeps what a terminal makes of them: a link without an address is no
//! stop, and two links with one address that touch (`[a](u)**[b](u)**`) are
//! one, as the terminal shows them as one hyperlink. A picture that is in a
//! link is not a stop of its own: the link is. Where a stop is shown is the
//! layout's answer (`Seen::shown`), so a stop that no line shows is not one
//! in that layout, and the selection is a range, which is the same stop in
//! either layout and, with `Held`, after a reload.

use std::ops::Range;

use crate::session::edition::{Edition, Held};
use crate::session::reveal::Seen;
use crate::typeset::images::ImageKey;
use crate::typeset::position::Pos;
use crate::typeset::targets::Targets;

/// A place Tab stops at: a range of the text of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stop {
    pub range: Range<Pos>,
    pub kind: Kind,
}

/// What a stop is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A link, which goes to `url`.
    Link { url: String },
    /// A picture that plays, by its key and the name the status line tells.
    Picture { key: ImageKey, name: String },
}

/// The stops of `targets` in the order of the document: the links, with
/// the two rules above, and the pictures that are no link's and that
/// `pictures` says play, with their key and name. It is asked of the range of
/// each embed.
pub(crate) fn stops(
    targets: &Targets,
    pictures: impl Fn(&Range<Pos>) -> Option<(ImageKey, String)>,
) -> Vec<Stop> {
    let mut found: Vec<Stop> = Vec::new();
    for link in targets.links.iter().filter(|link| !link.url.is_empty()) {
        if let Some(Stop {
            range,
            kind: Kind::Link { url },
        }) = found.last_mut()
            && *url == link.url
            && range.end == link.range.start
        {
            range.end = link.range.end.clone();
            continue;
        }
        found.push(Stop {
            range: link.range.clone(),
            kind: Kind::Link {
                url: link.url.clone(),
            },
        });
    }
    // A picture is in a link when its range is inside one of the links as
    // written, with an address or without.
    let links = &targets.links;
    let in_link = |range: &Range<Pos>| {
        let before = links.partition_point(|link| link.range.start <= range.start);
        before > 0 && range.end <= links[before - 1].range.end
    };
    for embed in &targets.embeds {
        if in_link(&embed.range) {
            continue;
        }
        if let Some((key, name)) = pictures(&embed.range) {
            found.push(Stop {
                range: embed.range.clone(),
                kind: Kind::Picture { key, name },
            });
        }
    }
    found.sort_by(|a, b| a.range.start.cmp(&b.range.start));
    found
}

/// A stop and the lines that show it, from the first to the last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Placed {
    pub stop: Stop,
    pub lines: Range<usize>,
}

impl Placed {
    /// Whether any of its lines is among `visible`.
    pub(crate) fn is_on(&self, visible: &Range<usize>) -> bool {
        self.lines.start < visible.end && self.lines.end > visible.start
    }
}

/// The stops among `stops` that the lines of `seen` show, with the lines. A
/// link that only the words of a heading drawn as an image show is not one:
/// there is nothing to follow.
pub(crate) fn placed(stops: Vec<Stop>, seen: Seen) -> Vec<Placed> {
    (stops.into_iter())
        .filter_map(|stop| {
            let link = matches!(stop.kind, Kind::Link { .. });
            if link && !seen.map.holds_cells(&stop.range) {
                return None;
            }
            let (_, lines) = seen.shown(&stop.range).ok()?;
            (!lines.is_empty()).then_some(Placed { stop, lines })
        })
        .collect()
}

/// Which stop is selected.
#[derive(Debug, Default)]
pub(crate) struct Links {
    /// The range of the stop, in the version it was selected in.
    pub(crate) selected: Option<Held<Range<Pos>>>,
}

impl Links {
    /// Selects the next stop in the whole document, or the one before it when
    /// `back`, from the selected one, and round at the ends; with none
    /// selected, the first one on the lines `visible`, or the last when
    /// `back`, and when none is there, the first after them, or the last
    /// before them. Whether that changed the selection.
    pub fn select_next(
        &mut self,
        edition: &Edition,
        all: &[Placed],
        visible: Range<usize>,
        back: bool,
    ) -> bool {
        if all.is_empty() {
            return false;
        }
        let at = self.position(edition, all);
        let next = match (at, back) {
            (Some(at), false) => (at + 1) % all.len(),
            (Some(at), true) => (at + all.len() - 1) % all.len(),
            (None, back) => {
                let on: Vec<usize> = (0..all.len()).filter(|n| all[*n].is_on(&visible)).collect();
                let after = |n: &usize| all[*n].lines.start >= visible.end;
                let before = |n: &usize| all[*n].lines.start < visible.start;
                match back {
                    false => (on.first().copied())
                        .or_else(|| (0..all.len()).find(after))
                        .unwrap_or(0),
                    true => (on.last().copied())
                        .or_else(|| (0..all.len()).rev().find(before))
                        .unwrap_or(all.len() - 1),
                }
            }
        };
        let changed = at != Some(next);
        self.selected = Some(Held::new(edition, all[next].stop.range.clone()));
        changed
    }

    /// Selects the picture `key`, or else the stop that has the line `line`
    /// among its own; whether something was.
    pub fn select_picture(
        &mut self,
        edition: &Edition,
        all: &[Placed],
        (line, key): (usize, &ImageKey),
    ) -> bool {
        let chosen = (all.iter())
            .find(|placed| matches!(&placed.stop.kind, Kind::Picture { key: at, .. } if at == key))
            .or_else(|| all.iter().find(|placed| placed.lines.contains(&line)));
        let Some(placed) = chosen else { return false };
        self.selected = Some(Held::new(edition, placed.stop.range.clone()));
        true
    }

    /// Selects no stop.
    pub fn clear(&mut self) -> bool {
        self.selected.take().is_some()
    }

    /// The index among `all` of the selected stop: the one with the range
    /// selected, or else, as after a reload moved the range, the one that
    /// holds its start.
    fn position(&self, edition: &Edition, all: &[Placed]) -> Option<usize> {
        let range = self.selected.as_ref()?.get(edition)?;
        (all.iter().position(|placed| placed.stop.range == *range)).or_else(|| {
            let holds = |placed: &Placed| {
                placed.stop.range.start <= range.start && range.start < placed.stop.range.end
            };
            all.iter().position(holds)
        })
    }

    /// The selected stop among `all`, when it is one.
    pub fn selected<'a>(&self, edition: &Edition, all: &'a [Placed]) -> Option<&'a Placed> {
        self.position(edition, all).map(|at| &all[at])
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::render::Inputs;
    use crate::session::sources::Session;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::layout::{Content, Stream, content};
    use crate::typeset::locate::Map;
    use crate::typeset::model::Document;
    use crate::typeset::parse::parse;
    use crate::typeset::reading::Fonts;
    use crate::typeset::reading::engine::Reading;
    use crate::typeset::reading::page::Wide;
    use crate::typeset::targets::targets;

    fn no_pictures(_: &Range<Pos>) -> Option<(ImageKey, String)> {
        None
    }

    fn urls(found: &[Stop]) -> Vec<&str> {
        (found.iter())
            .map(|stop| match &stop.kind {
                Kind::Link { url } => url.as_str(),
                Kind::Picture { name, .. } => name.as_str(),
            })
            .collect()
    }

    #[test]
    fn a_link_without_an_address_is_no_stop() {
        let found = stops(&targets([&parse("[a]() [b](u)\n")]), no_pictures);
        assert_eq!(urls(&found), ["u"]);
    }

    #[test]
    fn links_of_one_address_that_touch_are_one_stop_whatever_their_styles() {
        let document = parse("[a](u)**[b](u)** [c](u)\n");
        let all = targets(&[document][..]);
        let found = stops(&all, no_pictures);
        assert_eq!(urls(&found), ["u", "u"]);
        // The first is `a` and `b` together.
        assert_eq!(found[0].range.start, all.links[0].range.start);
        assert_eq!(found[0].range.end, all.links[1].range.end);
        assert_eq!(found[1].range, all.links[2].range);
        // Another address does not join, and nor does a gap.
        let found = stops(&targets([&parse("[a](u)[b](v) [c](v)\n")]), no_pictures);
        assert_eq!(urls(&found), ["u", "v", "v"]);
    }

    #[test]
    fn links_of_one_address_in_two_cells_are_two_stops() {
        let document = parse("| [a](u) | [b](u) |\n|---|---|\n");
        let found = stops(&targets([&document]), no_pictures);
        assert_eq!(urls(&found), ["u", "u"]);
    }

    #[test]
    fn a_linked_picture_is_the_link_s_stop_and_one_that_plays_outside_a_link_is_its_own() {
        let document = parse("see [![a](a.gif) b](u) and ![c](c.gif) and ![d](d.png)\n");
        let all = targets([&document]);
        let key = |name: &str| ImageKey::from(name);
        // Every embed is an animation but the last.
        let plays = |range: &Range<Pos>| {
            let embed = all.embeds.iter().find(|embed| embed.range == *range)?;
            let crate::typeset::targets::EmbedKind::Image { url } = &embed.kind else {
                return None;
            };
            (url != "d.png").then(|| (key(url), format!("name of {url}")))
        };
        let found = stops(&all, plays);
        assert_eq!(urls(&found), ["u", "name of c.gif"]);
        assert!(matches!(&found[1].kind, Kind::Picture { key: at, .. } if *at == key("c.gif")));
    }

    #[test]
    fn the_stops_are_in_the_order_of_the_document() {
        let document = parse("[a](ua)\n\n- [b](ub)\n\n> [c](uc)\n\n| [d](ud) |\n|---|\n");
        let found = stops(&targets([&document]), no_pictures);
        assert_eq!(urls(&found), ["ua", "ub", "uc", "ud"]);
        assert!(
            found
                .windows(2)
                .all(|pair| pair[0].range.end <= pair[1].range.start)
        );
    }

    /// `document` laid out as text, or in reading mode, with the stops the
    /// layout shows and the edition they are of.
    fn shown(document: &Document, reading: bool, columns: u16) -> (Vec<Placed>, Edition, usize) {
        let reading = reading.then(|| Reading {
            pictures: Default::default(),
            fonts: Rc::new(Fonts::fixture()),
            wide: Wide::Wrap,
        });
        let inputs = Inputs {
            reading,
            ..Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(columns)
            })
        };
        let Stream { lines, blocks } = content(Content::Document(document), &inputs);
        let map = Map::new(&lines, &blocks);
        let seen = Seen {
            map: &map,
            lines: &lines,
            blocks: &blocks,
        };
        let edition = Edition::new(Content::Document(document), &Session::new(["a.md"]), None);
        let all = placed(stops(&edition.targets, no_pictures), seen);
        (all, edition, lines.len())
    }

    fn selected_url(links: &Links, edition: &Edition, all: &[Placed]) -> Option<String> {
        links.selected(edition, all).map(|at| match &at.stop.kind {
            Kind::Link { url } => url.clone(),
            Kind::Picture { name, .. } => name.clone(),
        })
    }

    const DOCUMENT: &str = "A [one](u1) and [two](u2) on a line.\n\nBetween.\n\n- a [three](u3)\n\n| h |\n|---|\n| [four](u4) |\n";

    #[test]
    fn tab_selects_the_next_stop_and_shift_tab_the_one_before_and_goes_round() {
        for reading in [false, true] {
            let (all, edition, count) = shown(&parse(DOCUMENT), reading, 80);
            let mut links = Links::default();
            assert_eq!(selected_url(&links, &edition, &all), None);
            // From none: the first, or backwards the last; and on round.
            assert!(links.select_next(&edition, &all, 0..count, false));
            assert_eq!(selected_url(&links, &edition, &all).as_deref(), Some("u1"));
            for expected in ["u2", "u3", "u4", "u1"] {
                assert!(links.select_next(&edition, &all, 0..count, false));
                let found = selected_url(&links, &edition, &all);
                assert_eq!(found.as_deref(), Some(expected), "reading {reading}");
            }
            assert!(links.select_next(&edition, &all, 0..count, true));
            assert_eq!(selected_url(&links, &edition, &all).as_deref(), Some("u4"));
            assert!(links.clear());
            assert!(links.select_next(&edition, &all, 0..count, true));
            assert_eq!(selected_url(&links, &edition, &all).as_deref(), Some("u4"));
        }
    }

    #[test]
    fn with_none_selected_tab_takes_the_first_stop_on_the_screen_or_else_the_first_after_it() {
        let (all, edition, count) = shown(&parse(DOCUMENT), false, 80);
        let line_of = |url: &str| {
            let at = all
                .iter()
                .find(|at| matches!(&at.stop.kind, Kind::Link { url: u } if u == url));
            at.unwrap().lines.start
        };
        // The window shows the line of `u3` only: Tab takes it, and
        // backwards too.
        let window = line_of("u3")..line_of("u3") + 1;
        for back in [false, true] {
            let mut links = Links::default();
            links.select_next(&edition, &all, window.clone(), back);
            assert_eq!(selected_url(&links, &edition, &all).as_deref(), Some("u3"));
        }
        // With none on the screen, the first after it, and backwards the
        // last before it.
        let between = line_of("u2") + 1..line_of("u3");
        let mut links = Links::default();
        links.select_next(&edition, &all, between.clone(), false);
        assert_eq!(selected_url(&links, &edition, &all).as_deref(), Some("u3"));
        let mut links = Links::default();
        links.select_next(&edition, &all, between, true);
        assert_eq!(selected_url(&links, &edition, &all).as_deref(), Some("u2"));
        // Past the last, round to the first.
        let mut links = Links::default();
        links.select_next(&edition, &all, count..count, false);
        assert_eq!(selected_url(&links, &edition, &all).as_deref(), Some("u1"));
    }

    #[test]
    fn a_selected_stop_is_found_again_in_another_layout_of_the_same_document() {
        let document = parse(DOCUMENT);
        let (wide, edition, count) = shown(&document, false, 80);
        let mut links = Links::default();
        links.select_next(&edition, &wide, 0..count, false);
        links.select_next(&edition, &wide, 0..count, false);
        assert_eq!(selected_url(&links, &edition, &wide).as_deref(), Some("u2"));
        // The same range is the same stop in a narrow window and in reading
        // mode, where it is on other lines.
        for (reading, columns) in [(false, 20), (true, 80)] {
            let (again, _, _) = shown(&document, reading, columns);
            let found = selected_url(&links, &edition, &again);
            assert_eq!(found.as_deref(), Some("u2"), "{reading} {columns}");
        }
    }

    /// The addresses of the stops each fixture and example has in each
    /// layout, in the order Tab visits them: what `identify` found before
    /// the stops came from the model.
    #[test]
    fn the_stops_tab_visits_in_every_fixture_and_example_are_pinned() {
        let mut out = String::new();
        for path in crate::testing::documents() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let document = parse(&std::fs::read_to_string(&path).unwrap());
            for (layout, reading) in [("text", false), ("reading", true)] {
                let (all, _, _) = shown(&document, reading, 100);
                let urls: Vec<String> = (all.iter())
                    .map(|at| match &at.stop.kind {
                        Kind::Link { url } => url.clone(),
                        Kind::Picture { name, .. } => name.clone(),
                    })
                    .collect();
                if !urls.is_empty() {
                    out += &format!("{name} ({layout}): {urls:?}\n");
                }
            }
        }
        insta::assert_snapshot!(out);
    }
}
