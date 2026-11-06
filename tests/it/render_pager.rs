//! Pager frames: which lines are drawn where, as escape sequences.

use crate::common::{
    WIDTH, drawn, fixed_inputs, one_image, quadrants, readable, visible, with_images,
};
use kiri::images::Images;
use kiri::present::encoder::Encoder;
use kiri::present::kitty::graphics;
use kiri::present::output::{Page, Writer};
use kiri::present::presenter::Presenter;
use kiri::present::scene::{Scene, Surface};
use kiri::render::Inputs;
use kiri::session::view::{Motion, View};
use kiri::typeset::caps::Capabilities;
use kiri::typeset::style::{Line, Span, Style};

/// The frame the presenter writes for `source` in a window of `rows`
/// rows, after `motions`.
fn frame(source: &str, caps: Capabilities, page: Page, rows: usize, motions: &[Motion]) -> String {
    let document = kiri::typeset::parse::parse(source);
    let inputs = fixed_inputs(caps);
    let lines = kiri::typeset::layout::layout(&document, &inputs);
    let images = Images::new();
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, &inputs.caps, page, &mut encoder);
    let heights = lines
        .iter()
        .map(|line| usize::from(writer.rows(line)))
        .collect();
    let mut view = View::new(heights, rows);
    for &motion in motions {
        view.scroll(motion);
    }
    let shown = &lines[view.visible()];
    let scene = Scene {
        window: (WIDTH, rows as u16 + 1),
        surfaces: vec![Surface::Lines {
            lines: shown,
            dim: false,
        }],
    };
    let mut out = Vec::new();
    Presenter::default()
        .present(&scene, &mut writer, &mut out)
        .unwrap();
    String::from_utf8(out).unwrap()
}

/// The bytes the presenter writes for `scene`, with a new encoder and
/// writer.
fn present(scene: &Scene<'_>, inputs: &Inputs, page: Page) -> String {
    let images = &inputs.images;
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(images, &inputs.caps, page, &mut encoder);
    let mut out = Vec::new();
    Presenter::default()
        .present(scene, &mut writer, &mut out)
        .unwrap();
    String::from_utf8(out).unwrap()
}

/// A bar of `text` in the default style.
fn bar(text: &str) -> Vec<Span> {
    vec![Span::new(text, Style::default())]
}

/// A scene of `lines` in a window of `rows` rows with the bars
/// `(row, column, spans)` over them.
fn scene<'a>(lines: &'a [Line], rows: u16, bars: &'a [(usize, usize, Vec<Span>)]) -> Scene<'a> {
    let mut surfaces = vec![Surface::Lines { lines, dim: false }];
    surfaces.extend(bars.iter().map(|(row, column, spans)| Surface::Bar {
        row: *row,
        column: *column,
        spans,
    }));
    Scene {
        window: (WIDTH, rows),
        surfaces,
    }
}

/// `source` laid out with `inputs`.
fn laid(source: &str, inputs: &Inputs) -> Vec<Line> {
    kiri::typeset::layout::layout(&kiri::typeset::parse::parse(source), inputs)
}

/// Title on rows 1 to 3, One on row 5; the window has 7 rows.
const HEADED: &str = "# Title\n\nOne\n";

const DOCUMENT: &str = "# Title\n\nOne\n\nTwo\n\nThree\n\nFour\n";

#[test]
fn a_frame_places_each_row_and_clears_the_rest() {
    let out = frame(
        DOCUMENT,
        Capabilities::kitty(WIDTH),
        Page::default(),
        6,
        &[],
    );
    // One synchronized update.
    assert!(out.starts_with("\x1b[?2026h"), "{out:?}");
    assert!(out.ends_with("\x1b[?2026l"), "{out:?}");
    assert!(!out.contains('\n'), "{out:?}");
    insta::assert_snapshot!(visible(&out));
}

#[test]
fn a_heading_scrolled_past_is_not_drawn() {
    let out = frame(
        DOCUMENT,
        Capabilities::kitty(WIDTH),
        Page::default(),
        6,
        &[Motion::Down(1)],
    );
    assert!(!out.contains("Title"), "{out:?}");
    assert!(out.contains("\x1b[1;1H"), "{out:?}");
}

#[test]
fn the_screen_is_cleared_in_the_page_colour() {
    let page = Page {
        background: Some(kiri::typeset::color::Color::Rgb(1, 2, 3)),
        foreground: None,
    };
    let out = frame(DOCUMENT, Capabilities::kitty(WIDTH), page, 6, &[]);
    let clear = out.find("\x1b[2J").expect("the screen is cleared");
    let colour = out.find("48;2;1;2;3").expect("the page colour is set");
    assert!(colour < clear, "{out:?}");
    insta::assert_snapshot!(visible(&out));
}

fn headed(bars: &[(usize, usize, Vec<Span>)], page: Page) -> String {
    let inputs = fixed_inputs(Capabilities::kitty(WIDTH));
    let lines = laid(HEADED, &inputs);
    present(&scene(&lines, 7, bars), &inputs, page)
}

#[test]
fn bars_come_after_every_line_in_the_order_of_the_scene() {
    let out = headed(
        &[
            (7, 1, bar("status")),
            (6, 30, bar("notice")),
            (4, 5, bar("panel")),
        ],
        Page::default(),
    );
    let at = |word: &str| out.find(word).unwrap_or_else(|| panic!("{word}: {out:?}"));
    assert!(at("One") < at("status"), "{out:?}");
    assert!(at("status") < at("notice"), "{out:?}");
    assert!(at("notice") < at("panel"), "{out:?}");
}

#[test]
fn a_bar_below_the_top_row_of_a_large_heading_erases_the_cells_it_covers_first() {
    let out = headed(
        &[
            (2, 3, bar("help")),
            (1, 20, bar("top")),
            (5, 3, bar("one row")),
        ],
        Page::default(),
    );
    assert!(out.contains("\x1b[2;3H\x1b[4Xhelp"), "{out:?}");
    assert!(out.contains("\x1b[1;20Htop"), "{out:?}");
    assert!(out.contains("\x1b[5;3Hone row"), "{out:?}");
    insta::assert_snapshot!(visible(&out));
}

#[test]
fn the_cells_a_bar_erases_take_the_page_s_background() {
    let page = Page {
        background: Some(kiri::typeset::color::Color::Rgb(1, 2, 3)),
        foreground: None,
    };
    let out = headed(&[(2, 3, bar("help"))], page);
    assert!(
        out.contains("\x1b[2;3H\x1b[0;48;2;1;2;3m\x1b[4X\x1b[0mhelp"),
        "{out:?}"
    );
}

#[test]
fn a_bar_outside_the_window_is_not_written() {
    let out = headed(
        &[(0, 1, bar("above")), (8, 1, bar("below"))],
        Page::default(),
    );
    assert!(!out.contains("above") && !out.contains("below"), "{out:?}");
}

#[test]
fn the_status_line_shows_no_part_of_an_image_that_goes_on_below_the_view() {
    let caps = Capabilities::kitty(WIDTH);
    let inputs = with_images(&caps, one_image("p.png", quadrants(40, 200)));
    let lines = laid("![p](p.png)\n", &inputs);
    let bars = [(5, 1, bar("status"))];
    let out = present(&scene(&lines, 5, &bars), &inputs, Page::default());
    let status = out.rfind("\x1b[5;1H").expect("the status line is placed");
    assert!(!out[status..].contains(graphics::PLACEHOLDER), "{out:?}");
    assert!(!drawn(&out).is_empty(), "{out:?}");
    insta::assert_snapshot!(readable(&out));
}

#[test]
fn a_heading_a_scroll_of_three_rows_would_cut_is_left_out() {
    let source = "One\n\nTwo\n\nThree\n\nFour\n\n# Big\n\nFive\n";
    let out = frame(
        source,
        Capabilities::kitty(WIDTH),
        Page::default(),
        7,
        &[Motion::Down(3)],
    );
    assert!(!out.contains("Big"), "{out:?}");
    assert!(!out.contains("\x1b[7;1H"), "{out:?}");
    insta::assert_snapshot!(visible(&out));
}
