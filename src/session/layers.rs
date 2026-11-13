//! The highlights over the document as one list of ranges: what each
//! means and how it stacks, so that a new highlight is a new entry here and
//! not a branch in the pager.
//!
//! The state engine decides what is highlighted where, as a
//! [`Highlight`] (`typeset/layers.rs`): a range of the document, what it
//! means (a change to tint, a match, the selected link) and a `z`. How a
//! meaning is painted is each layout's: the engines paint the list, the text
//! layout in colours and reverse video and reading mode with boxes and a
//! frame. The list goes from the bottom up by `z`: the tint 0, the
//! selection 1, a match 2 and the current match 3, so that a match in a
//! selected link stays a match.

use std::ops::Range;

use crate::session::fade::{FADE, Fade};
use crate::session::search::Search;
use crate::typeset::layers::{Highlight, Paint};
use crate::typeset::locate::Map;
use crate::typeset::position::Pos;

/// What is highlighted over the document.
pub struct Highlights<'a> {
    /// The change auto-follow went to and how much of the text colour its
    /// tint mixes into the background.
    pub tint: Option<(Range<Pos>, f32)>,
    /// The search whose matches are shown, if it is shown.
    pub search: Option<&'a Search>,
    /// The fade of the current match.
    pub fade: Option<Fade<'a>>,
    /// The range of the selected link.
    pub selected: Option<Range<Pos>>,
}

/// The highlights of `highlights` that may be on the lines `window` of the
/// layout `map` is of, from the bottom up: the tint, the matches and the
/// selected link. The matches that are off the window are left out.
pub fn stack(highlights: &Highlights, map: &Map, window: Range<usize>) -> Vec<Highlight> {
    let mut layers = Vec::new();
    if let Some((range, amount)) = &highlights.tint {
        layers.push(Highlight {
            range: range.clone(),
            paint: Paint::Tint { amount: *amount },
            z: 0,
        });
    }
    if let Some(selected) = &highlights.selected {
        layers.push(Highlight {
            range: selected.clone(),
            paint: Paint::Selected,
            z: 1,
        });
    }
    if let Some(search) = highlights.search {
        let shown = map.span_of(window);
        let fade = highlights.fade.and_then(|fade| fade.share(FADE));
        for (range, current) in search.ranges() {
            let near = shown
                .as_ref()
                .is_some_and(|shown| range.end > shown.start && range.start <= shown.end);
            if !near {
                continue;
            }
            layers.push(Highlight {
                range: range.clone(),
                paint: Paint::Match {
                    current,
                    fade: fade.filter(|_| current),
                },
                z: if current { 3 } else { 2 },
            });
        }
    }
    layers
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::render::Inputs;
    use crate::session::settings::SearchCase;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::layout::{Content, Stream, content};
    use crate::typeset::parse::parse;
    use crate::typeset::text::DocumentText;

    /// A search for `term` in 30 paragraphs laid out as text, with the map of
    /// the lines.
    fn searched(term: &str) -> (Search, Stream, Map) {
        let body: String = (0..30).map(|n| format!("word{n} kiri\n\n")).collect();
        let document = parse(&body);
        let stream = content(
            Content::Document(&document),
            &Inputs::new(Capabilities::kitty(40)),
        );
        let map = Map::new(&stream.lines, &stream.blocks);
        let text = DocumentText::new([&document]);
        let search = Search::new((&map, &stream.lines), &text, term, SearchCase::Smart, 4);
        (search, stream, map)
    }

    fn highlights(search: &Search) -> Highlights<'_> {
        Highlights {
            tint: None,
            search: Some(search),
            fade: None,
            selected: None,
        }
    }

    #[test]
    fn the_layers_go_from_the_bottom_up_by_z() {
        let (search, stream, map) = searched("kiri");
        let range = search.current().unwrap().range.clone();
        let all = Highlights {
            tint: Some((range.clone(), 0.15)),
            selected: Some(range),
            ..highlights(&search)
        };
        let layers = stack(&all, &map, 0..stream.lines.len());
        assert!(matches!(layers[0].paint, Paint::Tint { .. }) && layers[0].z == 0);
        assert!(matches!(layers[1].paint, Paint::Selected) && layers[1].z == 1);
        let zs: Vec<(bool, u8)> = (layers[2..].iter())
            .map(|layer| match layer.paint {
                Paint::Match { current, .. } => (current, layer.z),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(zs.len(), 30);
        // Only the current match is on top of the others.
        assert_eq!(
            zs.iter().filter(|(current, z)| *current && *z == 3).count(),
            1
        );
        assert_eq!(
            zs.iter()
                .filter(|(current, z)| !*current && *z == 2)
                .count(),
            29
        );
    }

    #[test]
    fn the_matches_off_the_window_are_left_out() {
        let (search, _, map) = searched("kiri");
        // Each paragraph is a line and a blank one.
        let layers = stack(&highlights(&search), &map, 10..14);
        assert!(!layers.is_empty() && layers.len() < 8, "{}", layers.len());
        assert!(stack(&highlights(&search), &map, 0..0).is_empty());
    }

    #[test]
    fn only_the_current_match_fades_and_a_fade_that_is_over_is_none() {
        let (search, stream, map) = searched("kiri");
        let fade = |millis| Fade {
            elapsed: Duration::from_millis(millis),
            rgb: None,
        };
        let fades = |millis| -> Vec<Option<f32>> {
            let all = Highlights {
                fade: Some(fade(millis)),
                ..highlights(&search)
            };
            let layers = stack(&all, &map, 0..stream.lines.len());
            (layers.iter())
                .filter_map(|layer| match layer.paint {
                    Paint::Match {
                        current: true,
                        fade,
                    } => Some(fade),
                    Paint::Match { fade, .. } => {
                        assert_eq!(fade, None);
                        None
                    }
                    _ => None,
                })
                .collect()
        };
        assert_eq!(fades(375), [Some(0.5)]);
        assert_eq!(fades(750), [None]);
    }
}
