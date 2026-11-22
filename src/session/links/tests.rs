use std::rc::Rc;

use super::*;
use crate::render::Inputs;
use crate::session::animation::{Pacing, clips};
use crate::session::sources::Session;
use crate::typeset::caps::Capabilities;
use crate::typeset::driver::{Content, Stream, content};
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
            Kind::Link { url, .. } => url.as_str(),
            Kind::Picture { name, .. } => name.as_str(),
        })
        .collect()
}

#[test]
fn a_link_without_an_address_is_no_stop() {
    let found = stops(&targets([&parse("[a]() [b](u)\n")]), |_| true, no_pictures);
    assert_eq!(urls(&found), ["u"]);
}

#[test]
fn links_of_one_address_that_touch_are_one_stop_whatever_their_styles() {
    let document = parse("[a](u)**[b](u)** [c](u)\n");
    let all = targets(&[document][..]);
    let found = stops(&all, |_| true, no_pictures);
    assert_eq!(urls(&found), ["u", "u"]);
    // The first is `a` and `b` together.
    assert_eq!(found[0].range.start, all.links[0].range.start);
    assert_eq!(found[0].range.end, all.links[1].range.end);
    assert_eq!(found[1].range, all.links[2].range);
    // Another address does not join, and nor does a gap.
    let found = stops(
        &targets([&parse("[a](u)[b](v) [c](v)\n")]),
        |_| true,
        no_pictures,
    );
    assert_eq!(urls(&found), ["u", "v", "v"]);
}

#[test]
fn links_of_one_address_that_touch_keep_the_first_title() {
    let title = |source: &str| match &stops(&targets([&parse(source)]), |_| true, no_pictures)[..] {
        [
            Stop {
                kind: Kind::Link { title, .. },
                ..
            },
        ] => title.clone(),
        other => panic!("not one stop: {other:?}"),
    };
    assert_eq!(
        title("[a](u \"one\")**[b](u \"two\")**\n").as_deref(),
        Some("one")
    );
    // The first one's, also when it has none.
    assert_eq!(title("[a](u)**[b](u \"two\")**\n"), None);
}

#[test]
fn links_of_one_address_in_two_cells_are_two_stops() {
    let document = parse("| [a](u) | [b](u) |\n|---|---|\n");
    let found = stops(&targets([&document]), |_| true, no_pictures);
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
    let found = stops(&all, |_| true, plays);
    assert_eq!(urls(&found), ["u", "name of c.gif"]);
    assert!(matches!(&found[1].kind, Kind::Picture { key: at, .. } if *at == key("c.gif")));
}

#[test]
fn the_stops_are_in_the_order_of_the_document() {
    let document = parse("[a](ua)\n\n- [b](ub)\n\n> [c](uc)\n\n| [d](ud) |\n|---|\n");
    let found = stops(&targets([&document]), |_| true, no_pictures);
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
    shown_on(document, reading, columns, (false, true))
}

/// Loads the images `document` names that the fixtures have, by the name
/// of the file, animated.
fn load_fixture_images(document: &Document, inputs: &mut Inputs) {
    for embed in &targets([document]).embeds {
        let crate::typeset::targets::EmbedKind::Image { url } = &embed.kind else {
            continue;
        };
        let name = url.rsplit('/').next().unwrap();
        let path = crate::load::tests::fixtures().join("images").join(name);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let fonts = &crate::images::Fonts::none();
        let source =
            crate::typeset::images::decode::decode_named_as(&bytes, None, 10_000_000, fonts, true);
        inputs.images.insert(url.as_str(), source);
    }
}

/// As `shown`, with the fixtures' images loaded and their animations known
/// when `loaded`, on a terminal that shows hyperlinks or not.
fn shown_on(
    document: &Document,
    reading: bool,
    columns: u16,
    (loaded, hyperlinks): (bool, bool),
) -> (Vec<Placed>, Edition, usize) {
    let reading = reading.then(|| Reading {
        pictures: Default::default(),
        glyphs: Default::default(),
        fonts: Rc::new(Fonts::fixture()),
        wide: Wide::Wrap,
        snap: false,
        zoom: 100,
    });
    let links = hyperlinks || reading.is_some();
    let mut inputs = Inputs {
        reading,
        ..Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            hyperlinks,
            ..Capabilities::kitty(columns)
        })
    };
    if loaded {
        load_fixture_images(document, &mut inputs);
    }
    let Stream { lines, blocks } = content(Content::Document(document), &inputs);
    let map = Map::new(&lines, &blocks);
    let seen = Seen {
        map: &map,
        lines: &lines,
    };
    let mut animator = Animator::new(Pacing::new(0));
    if loaded {
        animator.know(clips(&lines, &inputs.images));
    }
    let session = Session::new(["a.md"]);
    let edition = Edition::new(
        Content::Document(document),
        &session,
        session.base().id(),
        None,
    );
    let all = on_layout(&edition, &animator, seen, links);
    (all, edition, lines.len())
}

fn selected_url(links: &Links, edition: &Edition, all: &[Placed]) -> Option<String> {
    links.selected(edition, all).map(|at| match &at.stop.kind {
        Kind::Link { url, .. } => url.clone(),
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
            .find(|at| matches!(&at.stop.kind, Kind::Link { url: u, .. } if u == url));
        at.unwrap().lines.clone().unwrap().start
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
/// layout, in the order Tab visits them: with no image loaded, with the
/// fixtures' images loaded and the animations among the stops, and on a
/// terminal without hyperlinks. What `identify` found before the stops
/// came from the model.
#[test]
fn the_stops_tab_visits_in_every_fixture_and_example_are_pinned() {
    let mut out = String::new();
    let mut documents: Vec<(String, String)> = Vec::new();
    for path in crate::testing::documents() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        documents.push((name, std::fs::read_to_string(&path).unwrap()));
    }
    // No fixture has an animation in a link, and the cell layout draws one
    // alone on its line as image rows that have no link on them.
    let block = "[![a](animated.gif)](https://example.com/x)\n\n[![b](jiggy.gif)]()\n\n\
                     after [text](https://example.com/y)\n";
    documents.push(("linked block animation".to_owned(), block.to_owned()));
    for (name, source) in documents {
        let document = parse(&source);
        for (pass, options) in [
            ("", (false, true)),
            (", images", (true, true)),
            (", no hyperlinks", (true, false)),
        ] {
            for (layout, reading) in [("text", false), ("reading", true)] {
                let (all, _, _) = shown_on(&document, reading, 100, options);
                let urls: Vec<String> = (all.iter())
                    .map(|at| match &at.stop.kind {
                        Kind::Link { url, .. } => url.clone(),
                        Kind::Picture { name, .. } => name.clone(),
                    })
                    .collect();
                if !urls.is_empty() {
                    out += &format!("{name} ({layout}{pass}): {urls:?}\n");
                }
            }
        }
    }
    insta::assert_snapshot!(out);
}
