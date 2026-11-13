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
    let lines = kiri::typeset::driver::layout(&document, &inputs);
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
    kiri::typeset::driver::layout(&kiri::typeset::parse::parse(source), inputs)
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
    // The pager gives the scene only the lines that fit above the status row.
    let out = present(&scene(&lines[..4], 5, &bars), &inputs, Page::default());
    let status = out.find("\x1b[5;1H").expect("the status line is placed");
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

/// The bytes of each of `scenes` presented in turn through one presenter,
/// one encoder and one writer.
fn presented(scenes: &[Scene<'_>], inputs: &Inputs, page: Page) -> Vec<String> {
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&inputs.images, &inputs.caps, page, &mut encoder);
    let mut presenter = Presenter::default();
    scenes
        .iter()
        .map(|scene| {
            let mut out = Vec::new();
            presenter.present(scene, &mut writer, &mut out).unwrap();
            String::from_utf8(out).unwrap()
        })
        .collect()
}

/// Whether row `row` was erased in `out`.
fn erased(out: &str, row: usize) -> bool {
    out.contains(&format!("\x1b[{row};1H\x1b[2K"))
}

fn headed_inputs() -> Inputs {
    fixed_inputs(Capabilities::kitty(WIDTH))
}

#[test]
fn a_frame_like_the_one_before_writes_only_its_update() {
    let inputs = headed_inputs();
    let lines = laid(HEADED, &inputs);
    let scenes = [scene(&lines, 7, &[]), scene(&lines, 7, &[])];
    let outs = presented(&scenes, &inputs, Page::default());
    assert_eq!(outs[1], "\x1b[?2026h\x1b[?2026l");
}

#[test]
fn a_new_status_line_rewrites_its_row_alone() {
    let inputs = headed_inputs();
    let lines = laid(HEADED, &inputs);
    let (one, two) = ([(7, 1, bar("one"))], [(7, 1, bar("two"))]);
    let scenes = [scene(&lines, 7, &one), scene(&lines, 7, &two)];
    let outs = presented(&scenes, &inputs, Page::default());
    assert_eq!(
        outs[1],
        "\x1b[?2026h\x1b[7;1H\x1b[2K\x1b[7;1Htwo\x1b[?2026l"
    );
}

#[test]
fn a_changed_row_of_a_large_heading_rewrites_every_row_of_it() {
    let inputs = headed_inputs();
    let (before, after) = (laid(HEADED, &inputs), laid("# Title!\n\nOne\n", &inputs));
    let scenes = [scene(&before, 7, &[]), scene(&after, 7, &[])];
    let out = &presented(&scenes, &inputs, Page::default())[1];
    let text = out.find("\x1b]66;").expect("the heading is written");
    for row in 1..=3 {
        let at = out
            .find(&format!("\x1b[{row};1H\x1b[2K"))
            .unwrap_or_else(|| panic!("row {row}: {out:?}"));
        assert!(at < text, "{out:?}");
    }
    assert!(!out.contains("\x1b[5;1H"), "{out:?}");
    assert!(!out.contains("\x1b[2J"), "{out:?}");
}

#[test]
fn the_rows_a_large_heading_left_are_erased_with_those_it_takes() {
    let inputs = headed_inputs();
    let headed = laid(HEADED, &inputs);
    let mut moved = vec![Line::blank()];
    moved.extend(headed.iter().cloned());
    let scenes = [scene(&headed, 7, &[]), scene(&moved, 7, &[])];
    let out = &presented(&scenes, &inputs, Page::default())[1];
    let text = out.find("\x1b]66;").expect("the heading is written");
    for row in 1..=6 {
        let at = out
            .find(&format!("\x1b[{row};1H\x1b[2K"))
            .unwrap_or_else(|| panic!("row {row}: {out:?}"));
        assert!(at < text, "{out:?}");
    }
    assert!(!out[..text].ends_with("\x1b[2K"), "{out:?}");
}

#[test]
fn a_new_window_size_draws_the_frame_whole() {
    let inputs = headed_inputs();
    let lines = laid(HEADED, &inputs);
    let scenes = [scene(&lines, 7, &[]), scene(&lines, 8, &[])];
    let out = &presented(&scenes, &inputs, Page::default())[1];
    assert!(out.contains("\x1b[H\x1b[2J"), "{out:?}");
}

#[test]
fn a_new_page_background_draws_the_frame_whole() {
    let inputs = headed_inputs();
    let lines = laid(HEADED, &inputs);
    let mut encoder = Encoder::new();
    let mut presenter = Presenter::default();
    let mut out = Vec::new();
    let mut first = Writer::new(&inputs.images, &inputs.caps, Page::default(), &mut encoder);
    presenter
        .present(&scene(&lines, 7, &[]), &mut first, &mut out)
        .unwrap();
    let page = Page {
        background: Some(kiri::typeset::color::Color::Rgb(1, 2, 3)),
        foreground: None,
    };
    let mut second = Writer::new(&inputs.images, &inputs.caps, page, &mut encoder);
    let mut out = Vec::new();
    presenter
        .present(&scene(&lines, 7, &[]), &mut second, &mut out)
        .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("\x1b[0;48;2;1;2;3m\x1b[H\x1b[2J"), "{out:?}");
}

#[test]
fn a_new_cell_size_draws_the_frame_whole() {
    let caps = |cell| Capabilities {
        cell_pixels: Some(cell),
        ..Capabilities::kitty(WIDTH)
    };
    let (first_caps, second_caps) = (caps((10, 20)), caps((12, 20)));
    let inputs = fixed_inputs(first_caps.clone());
    let lines = laid(HEADED, &inputs);
    let mut encoder = Encoder::new();
    let mut presenter = Presenter::default();
    let mut out = Vec::new();
    let mut first = Writer::new(&inputs.images, &first_caps, Page::default(), &mut encoder);
    presenter
        .present(&scene(&lines, 7, &[]), &mut first, &mut out)
        .unwrap();
    let mut second = Writer::new(&inputs.images, &second_caps, Page::default(), &mut encoder);
    let mut out = Vec::new();
    presenter
        .present(&scene(&lines, 7, &[]), &mut second, &mut out)
        .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("\x1b[H\x1b[2J"), "{out:?}");
}

#[test]
fn after_forget_the_frame_is_drawn_whole() {
    let inputs = headed_inputs();
    let lines = laid(HEADED, &inputs);
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&inputs.images, &inputs.caps, Page::default(), &mut encoder);
    let mut presenter = Presenter::default();
    let mut out = Vec::new();
    presenter
        .present(&scene(&lines, 7, &[]), &mut writer, &mut out)
        .unwrap();
    presenter.forget();
    let mut out = Vec::new();
    presenter
        .present(&scene(&lines, 7, &[]), &mut writer, &mut out)
        .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("\x1b[H\x1b[2J"), "{out:?}");
}

/// A sink that takes `room` bytes and then fails.
struct Full {
    room: usize,
}

impl std::io::Write for Full {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.room == 0 {
            return Err(std::io::Error::other("the terminal is gone"));
        }
        let taken = buf.len().min(self.room);
        self.room -= taken;
        Ok(taken)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_frame_after_a_failed_write_is_drawn_whole() {
    let inputs = headed_inputs();
    let lines = laid(HEADED, &inputs);
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&inputs.images, &inputs.caps, Page::default(), &mut encoder);
    let mut presenter = Presenter::default();
    let mut out = Vec::new();
    presenter
        .present(&scene(&lines, 7, &[]), &mut writer, &mut out)
        .unwrap();
    let failed = presenter.present(&scene(&lines, 7, &[]), &mut writer, &mut Full { room: 4 });
    assert!(failed.is_err());
    let mut out = Vec::new();
    presenter
        .present(&scene(&lines, 7, &[]), &mut writer, &mut out)
        .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("\x1b[H\x1b[2J"), "{out:?}");
}

#[test]
fn an_image_whose_write_failed_is_sent_again_in_the_next_frame() {
    let caps = Capabilities::kitty(WIDTH);
    let inputs = with_images(&caps, one_image("p.png", quadrants(40, 40)));
    let lines = laid("![p](p.png)\n", &inputs);
    let whole = {
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&inputs.images, &inputs.caps, Page::default(), &mut encoder);
        let mut out = Vec::new();
        Presenter::default()
            .present(&scene(&lines, 6, &[]), &mut writer, &mut out)
            .unwrap();
        out.len()
    };
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&inputs.images, &inputs.caps, Page::default(), &mut encoder);
    let mut presenter = Presenter::default();
    // Fails in the middle of the image's data: after the frame was rendered,
    // which has the encoder note the image as sent.
    let failed = presenter.present(
        &scene(&lines, 6, &[]),
        &mut writer,
        &mut Full { room: whole / 2 },
    );
    assert!(failed.is_err());
    let mut out = Vec::new();
    presenter
        .present(&scene(&lines, 6, &[]), &mut writer, &mut out)
        .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("a=T"), "{out:?}");
}

#[test]
fn the_frame_after_a_loading_band_is_drawn_whole_too() {
    use kiri::typeset::style::{Band, Loading};

    let caps = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(WIDTH)
    };
    let inputs = fixed_inputs(caps);
    let banded = vec![Line {
        loading: Some(Loading {
            key: "b.png".into(),
            above: 0,
            band: Some(Band {
                columns: 4,
                indent: 0,
                low: true,
            }),
        }),
        ..Line::new(vec![Span::new("code", Style::default())])
    }];
    let plain = vec![Line::new(vec![Span::new("code", Style::default())])];
    let scenes = [
        scene(&banded, 3, &[]),
        scene(&banded, 3, &[]),
        scene(&plain, 3, &[]),
        scene(&plain, 3, &[]),
    ];
    let outs = presented(&scenes, &inputs, Page::default());
    for (n, out) in outs.iter().enumerate() {
        assert_eq!(out.contains("\x1b[2J"), n < 3, "frame {n}: {out:?}");
    }
}

#[test]
fn a_row_that_sends_its_image_is_written_though_it_looks_the_same() {
    let caps = Capabilities::kitty(WIDTH);
    let inputs = with_images(&caps, one_image("p.png", quadrants(40, 40)));
    let lines = laid("![p](p.png)\n", &inputs);
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&inputs.images, &inputs.caps, Page::default(), &mut encoder);
    let mut presenter = Presenter::default();
    let mut out = Vec::new();
    presenter
        .present(&scene(&lines, 6, &[]), &mut writer, &mut out)
        .unwrap();
    writer.lost();
    let mut out = Vec::new();
    presenter
        .present(&scene(&lines, 6, &[]), &mut writer, &mut out)
        .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("a=T"), "{out:?}");
    assert!(out.contains("\x1b[1;1H"), "{out:?}");
}

#[test]
fn the_frame_after_one_that_sent_an_image_leaves_its_rows_alone() {
    let caps = Capabilities::kitty(WIDTH);
    let inputs = with_images(&caps, one_image("p.png", quadrants(40, 40)));
    let lines = laid("![p](p.png)\n", &inputs);
    let scenes = [
        scene(&lines, 6, &[]),
        scene(&lines, 6, &[]),
        scene(&lines, 6, &[]),
    ];
    let outs = presented(&scenes, &inputs, Page::default());
    assert!(outs[0].contains("a=T"), "{:?}", outs[0]);
    assert!(!erased(&outs[1], 1), "{:?}", outs[1]);
    assert_eq!(outs[1], "\x1b[?2026h\x1b[?2026l");
    assert_eq!(outs[2], "\x1b[?2026h\x1b[?2026l");
}

#[test]
fn closing_a_bar_over_a_large_heading_rewrites_the_heading_whole() {
    let inputs = headed_inputs();
    let lines = laid(HEADED, &inputs);
    let bars = [(2, 3, bar("help"))];
    let scenes = [scene(&lines, 7, &bars), scene(&lines, 7, &[])];
    let out = &presented(&scenes, &inputs, Page::default())[1];
    for row in 1..=3 {
        assert!(erased(out, row), "row {row}: {out:?}");
        assert!(out.contains(&format!("\x1b[{row};1H")), "{out:?}");
    }
    assert!(!out.contains("\x1b[5;1H"), "{out:?}");
    insta::assert_snapshot!(visible(out));
}

#[test]
fn a_scroll_of_three_rows_rewrites_every_row_it_moved() {
    let inputs = headed_inputs();
    let lines = laid("One\n\nTwo\n\nThree\n\nFour\n\n# Big\n\nFive\n", &inputs);
    let mut encoder = Encoder::new();
    let writer = Writer::new(&inputs.images, &inputs.caps, Page::default(), &mut encoder);
    let heights = lines
        .iter()
        .map(|line| usize::from(writer.rows(line)))
        .collect();
    let mut view = View::new(heights, 7);
    let first = lines[view.visible()].to_vec();
    view.scroll(Motion::Down(3));
    let second = lines[view.visible()].to_vec();
    let scenes = [scene(&first, 7, &[]), scene(&second, 7, &[])];
    let out = &presented(&scenes, &inputs, Page::default())[1];
    assert!(!out.contains("\x1b[2J"), "{out:?}");
    // Row 6 is blank before and after, so it is not touched.
    for row in [1, 2, 3, 4, 5, 7] {
        assert!(erased(out, row), "row {row}: {out:?}");
    }
    assert!(!out.contains("\x1b[6;1H"), "{out:?}");
    assert!(!out.contains("Big"), "{out:?}");
    insta::assert_snapshot!(visible(out));
}
