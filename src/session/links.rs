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

use crate::session::animation::Animator;
use crate::session::edition::{Edition, Held};
use crate::session::reveal::Seen;
use crate::typeset::images::ImageKey;
use crate::typeset::locate::Hidden;
use crate::typeset::position::Pos;
use crate::typeset::targets::{Goes, Targets};

/// A place Tab stops at: a range of the text of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stop {
    pub range: Range<Pos>,
    pub kind: Kind,
}

/// What a stop is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A link, which goes to `url`, as `goes` tells, with the title it was
    /// written with.
    Link {
        url: String,
        title: Option<String>,
        goes: Goes,
    },
    /// A picture that plays, by its key and the name the status line tells.
    Picture { key: ImageKey, name: String },
}

/// The stops of `targets` in the order of the document: the links, with
/// the two rules above, and the pictures that are no link's and that
/// `pictures` says play, with their key and name. It is asked of the range of
/// each embed. `links` says whether a range has a link's cells on the lines
/// that show it: where it has not, as in text on a terminal that shows no
/// hyperlinks or for an image drawn as rows, a link there is no stop, and a
/// picture in it is not in a link.
pub(crate) fn stops(
    targets: &Targets,
    links: impl Fn(&Range<Pos>) -> bool,
    pictures: impl Fn(&Range<Pos>) -> Option<(ImageKey, String)>,
) -> Vec<Stop> {
    let mut found: Vec<Stop> = Vec::new();
    let linked = targets
        .links
        .iter()
        .filter(|link| !link.url.is_empty() && links(&link.range));
    for link in linked {
        if let Some(Stop {
            range,
            kind: Kind::Link { url, .. },
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
                title: link.title.clone(),
                goes: link.goes.clone(),
            },
        });
    }
    // A picture is in a link when its range is inside one of the links as
    // written, with an address or without.
    let written = &targets.links;
    let in_link = |range: &Range<Pos>| {
        let before = written.partition_point(|link| link.range.start <= range.start);
        before > 0 && range.end <= written[before - 1].range.end && links(range)
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

/// The stops of the document `edition` is of that the lines of `seen` show:
/// the links, unless `links` says the lines have none to select, and the
/// animations that `animator` knows and no link has.
pub(crate) fn on_layout(
    edition: &Edition,
    animator: &Animator,
    seen: Seen,
    links: bool,
) -> Vec<Placed> {
    let candidates = animator.keys();
    let pictures = |range: &Range<Pos>| {
        let key = seen.map.picture(seen.lines, range, &candidates)?;
        let name = animator.name_of(&key)?;
        Some((key, name.to_owned()))
    };
    // A link in a part that is not laid out has not been found to be without
    // cells, so it is a stop.
    let links =
        |range: &Range<Pos>| links && (seen.map.not_laid_out(range) || seen.map.holds_cells(range));
    placed(stops(&edition.targets, links, pictures), seen)
}

/// A stop and the lines that show it, from the first to the last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Placed {
    pub stop: Stop,
    /// The lines; `None` while the layout has not laid the stop out.
    pub lines: Option<Range<usize>>,
}

impl Placed {
    /// Whether any of its lines is among `visible`.
    pub(crate) fn is_on(&self, visible: &Range<usize>) -> bool {
        (self.lines.as_ref())
            .is_some_and(|lines| lines.start < visible.end && lines.end > visible.start)
    }
}

/// The stops among `stops` that the lines of `seen` show, with the lines,
/// and those it has not laid out, with none.
pub(crate) fn placed(stops: Vec<Stop>, seen: Seen) -> Vec<Placed> {
    (stops.into_iter())
        .filter_map(|stop| match seen.shown(&stop.range) {
            Ok((_, lines)) => (!lines.is_empty()).then_some(Placed {
                stop,
                lines: Some(lines),
            }),
            Err(Hidden::NotLaidOut) => Some(Placed { stop, lines: None }),
            Err(_) => None,
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
                let after = |n: &usize| {
                    all[*n]
                        .lines
                        .as_ref()
                        .is_none_or(|lines| lines.start >= visible.end)
                };
                let before = |n: &usize| {
                    all[*n]
                        .lines
                        .as_ref()
                        .is_some_and(|lines| lines.start < visible.start)
                };
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
            .or_else(|| {
                all.iter().find(|placed| {
                    (placed.lines.as_ref()).is_some_and(|lines| lines.contains(&line))
                })
            });
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
mod tests;
