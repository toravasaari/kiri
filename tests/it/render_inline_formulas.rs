//! Formulas inside running text: drawn in the line's cells, and the
//! lines that grow for them.

use kiri::images::{ImageError, ImageKey, Images, Source};
use kiri::render::Inputs;
use kiri::typeset::caps::Capabilities;

use crate::common::{WIDTH, drawn, quadrants, readable, render, sent, with_images};

/// The key of the inline formula `x`, as the layout asks for it.
fn formula_key(source: &str) -> ImageKey {
    ImageKey::diagram(kiri::diagrams::INLINE_FORMULA, &format!("{source}\n"), 0, 0)
}

/// Images with the formula `x` as a picture `width` × `height` pixels and
/// the 16 × 16 `p.png`.
fn pictures(width: u32, height: u32) -> Images {
    let mut images = Images::new();
    images.insert(
        formula_key("x"),
        Ok(Source::Raster(quadrants(width, height))),
    );
    images.insert("p.png", Ok(Source::Raster(quadrants(16, 16))));
    images.insert("t.png", Ok(Source::Raster(quadrants(40, 90))));
    images
}

fn drawing(caps: &Capabilities, width: u32, height: u32) -> Inputs {
    with_images(caps, pictures(width, height))
}

/// What a terminal shows of `out`, a line each: the placeholders as
/// `⟦row n: k cells⟧`, without margin, escape sequences and graphics.
fn shown(out: &str) -> Vec<String> {
    let text = readable(out);
    let mut plain = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '␛' {
            plain.push(c);
        } else if let Some(kind) = chars.next() {
            let mut before = ' ';
            for c in chars.by_ref() {
                let done = match kind {
                    '[' => ('@'..='~').contains(&c),
                    _ => before == '␛' && c == '\\',
                };
                if done {
                    break;
                }
                before = c;
            }
        }
    }
    plain
        .lines()
        .map(|line| line.trim_start().to_owned())
        .collect()
}

#[test]
fn a_formula_that_fits_the_row_is_drawn_in_it() {
    // 40 × 18 pixels at 11/12 of a CSS pixel: 37 × 17, in a row of 20.
    let out = render("A $x$ b.\n", &drawing(&Capabilities::kitty(WIDTH), 40, 18));
    assert_eq!(shown(&out), ["A ⟦row 0: 4 cells⟧ b."], "{out:?}");
    assert_eq!(sent(&out).len(), 1);
}

#[test]
fn a_formula_a_little_too_high_is_drawn_smaller_in_the_row() {
    // 24 pixels high are 22: 20 / 22 is over 0.85, so the line stays a row.
    let out = render("A $x$ b.\n", &drawing(&Capabilities::kitty(WIDTH), 40, 24));
    assert_eq!(drawn(&out).len(), 1, "{out:?}");
    assert_eq!(shown(&out)[0], "A ⟦row 0: 4 cells⟧ b.", "{out:?}");
    let image = &sent(&out)[0];
    assert_eq!(image.keys["r"], "1");
    // Nothing of it is above the row's height.
    assert_eq!(image.keys["v"], "20");
}

#[test]
fn a_higher_formula_makes_its_line_two_rows_with_the_text_centred() {
    // 32 pixels high are 29: 20 / 29 is under 0.85.
    let out = render(
        "A $x$ b<sub>2</sub>.\n",
        &drawing(&Capabilities::kitty(WIDTH), 40, 32),
    );
    let rows = drawn(&out);
    assert_eq!(rows.len(), 2, "{out:?}");
    assert!(
        rows[0].contains("row 0") && rows[1].contains("row 1"),
        "{rows:?}"
    );
    // The text at half the scale 2, centred in the rows' height; the
    // subscript at half its own fraction.
    assert!(out.contains("s=2:n=1:d=2"), "{out:?}");
    assert!(out.contains("v=2"), "{out:?}");
    assert!(out.contains("s=2:n=1:d=3"), "{out:?}");
    let image = &sent(&out)[0];
    assert_eq!(
        (image.keys["c"].as_str(), image.keys["r"].as_str()),
        ("4", "2")
    );
}

#[test]
fn an_icon_in_a_two_row_line_covers_both_rows_with_its_bottom_on_the_text_cell() {
    let out = render(
        "A $x$ and ![i](p.png).\n",
        &drawing(&Capabilities::kitty(WIDTH), 40, 32),
    );
    let icon = (sent(&out).into_iter())
        .find(|image| image.keys["c"] == "2")
        .expect("the icon");
    assert_eq!(icon.keys["r"], "2");
    // The text cell is the lower half of the rows centred: 10 to 30.
    let width = icon.keys["s"].parse::<usize>().unwrap();
    let opaque: Vec<usize> = (icon.rgba.chunks(width * 4).enumerate())
        .filter(|(_, row)| row.chunks(4).any(|pixel| pixel[3] > 8))
        .map(|(y, _)| y)
        .collect();
    assert_eq!(
        (opaque.first(), opaque.last()),
        (Some(&14), Some(&29)),
        "{opaque:?}"
    );
}

#[test]
fn without_text_sizing_the_text_is_on_the_lower_row() {
    let caps = Capabilities {
        text_sizing: false,
        ..Capabilities::kitty(WIDTH)
    };
    let out = render("A $x$ b.\n", &drawing(&caps, 40, 32));
    // One line for each row: the text on the one the formula's baseline is
    // on, with the rows above and below it as high as the formula needs.
    let lines: Vec<String> = (shown(&out).into_iter())
        .map(|line| line.trim_end().to_owned())
        .collect();
    assert!(lines.len() >= 2, "{out:?}");
    assert_eq!(lines[0], "⟦row 0: 4 cells⟧", "{out:?}");
    assert_eq!(lines[1], "A ⟦row 1: 4 cells⟧ b.", "{out:?}");
    for (row, line) in lines.iter().enumerate().skip(2) {
        assert_eq!(line, &format!("⟦row {row}: 4 cells⟧"), "{out:?}");
    }
    assert!(!out.contains("]66;"), "{out:?}");
}

#[test]
fn a_full_stop_after_a_formula_stays_on_its_line() {
    let source = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk ll $x$.\n";
    let out = render(source, &drawing(&Capabilities::kitty(WIDTH), 40, 18));
    let line = shown(&out)
        .into_iter()
        .find(|line| line.contains('⟦'))
        .expect("the formula");
    assert!(line.ends_with("cells⟧."), "{line:?}");
}

#[test]
fn a_formula_that_is_not_drawn_shows_as_written() {
    let mut images = Images::new();
    images.insert(
        formula_key("x^2"),
        Err(ImageError::Corrupt("the formula".into())),
    );
    let out = render(
        "A $x^2$ b, $y$ c.\n",
        &with_images(&Capabilities::kitty(WIDTH), images),
    );
    assert!(drawn(&out).is_empty(), "{out:?}");
    assert!(shown(&out)[0].contains("A $x^2$ b, $y$ c."), "{out:?}");
}

/// kitty on a black background, where an alert has its tint.
fn tinted() -> Capabilities {
    Capabilities {
        background: Some(kiri::typeset::color::Color::Rgb(0, 0, 0)),
        dark_background: Some(true),
        ..Capabilities::kitty(WIDTH)
    }
}

/// The lines of `out` that hold placeholders.
fn rows_of_cells(out: &str) -> Vec<String> {
    (readable(out).lines())
        .filter(|line| line.contains('⟦'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_two_row_line_in_a_quote_has_the_bar_on_both_rows() {
    let out = render(
        "> A $x$ b.\n",
        &drawing(&Capabilities::kitty(WIDTH), 40, 32),
    );
    let rows = rows_of_cells(&out);
    assert_eq!(rows.len(), 2, "{out:?}");
    for row in &rows {
        assert!(row.contains('│'), "{row:?}");
        assert!(row.find('│') < row.find('⟦'), "{row:?}");
    }
}

#[test]
fn a_two_row_line_in_an_alert_has_the_bar_and_the_tint_on_both_rows_without_holes() {
    let source = "> [!NOTE]\n> A $x$ and ![i](p.png) b.\n";
    let out = render(source, &drawing(&tinted(), 40, 32));
    // The formula and the icon are on the same two rows.
    let rows = rows_of_cells(&out);
    assert_eq!(rows.len(), 2, "{out:?}");
    for row in &rows {
        let after_bar = &row[row.find('▎').expect("the bar on the row")..];
        // Every colour the row sets keeps the tint, as the cells of the
        // image and what is between them are on it.
        for set in after_bar.split("␛[0;").skip(1) {
            let colours = &set[..set.find('m').unwrap()];
            assert!(colours.contains("48;2;10;22;37"), "{colours:?} in {row:?}");
        }
        // And no reset until the end of the row.
        assert_eq!(after_bar.matches("␛[0m").count(), 1, "{row:?}");
    }
    // The cells are drawn over the tint, which the first of the rows set
    // before them.
    assert!(rows[1].find(";48;2;10;22;37").unwrap() < rows[1].find('⟦').unwrap());
}

#[test]
fn a_formula_in_a_heading_is_drawn_in_the_rows_of_its_text_and_the_heading_does_not_grow() {
    let out = render("# A $x$ b\n", &drawing(&Capabilities::kitty(WIDTH), 40, 18));
    let rows = rows_of_cells(&out);
    // A first-level heading is three rows high, with or without it.
    assert_eq!(rows.len(), 3, "{rows:?}");
    for (row, line) in rows.iter().enumerate() {
        assert!(line.contains(&format!("row {row}:")), "{line:?}");
    }
    assert!(
        rows[0].contains(";A ") && rows[0].contains("; b"),
        "{:?}",
        rows[0]
    );
    let image = &sent(&out)[0];
    assert_eq!(image.keys["r"], "3");
}

#[test]
fn new_cells_fit_a_formula_again() {
    let at = |cell| {
        let caps = Capabilities {
            cell_pixels: Some(cell),
            ..Capabilities::kitty(WIDTH)
        };
        let out = render("A $x$ b.\n", &drawing(&caps, 40, 18));
        let sent = sent(&out);
        let image = &sent[0];
        (image.keys["s"].clone(), image.keys["v"].clone())
    };
    // 37 × 17 at 20 pixels to the row, twice as many at 40, in cells of
    // 10 and 20 pixels: four columns on a canvas of the cells.
    let small = at((10, 20));
    let large = at((20, 40));
    assert_eq!(small, ("40".to_owned(), "20".to_owned()));
    assert_eq!(large, ("80".to_owned(), "40".to_owned()));
}

/// Like `shown`, but with the margin and the text of sized text kept: the
/// columns a terminal shows, at their width for normal text.
fn columns_shown(out: &str) -> Vec<String> {
    let text = readable(out);
    let mut plain = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '␛' {
            plain.push(c);
        } else if let Some(kind) = chars.next() {
            let mut before = ' ';
            let mut body = String::new();
            for c in chars.by_ref() {
                let done = match kind {
                    '[' => ('@'..='~').contains(&c),
                    _ => before == '␛' && c == '\\',
                };
                if done {
                    break;
                }
                body.push(c);
                before = c;
            }
            // Sized text: its text, from after the keys.
            if kind == ']' && body.starts_with("66;") {
                let text = body.rsplit_once(';').map_or("", |(_, text)| text);
                plain.push_str(text.trim_end_matches('␛'));
            }
        }
    }
    plain.lines().map(str::to_owned).collect()
}

#[test]
fn a_list_marker_is_on_the_text_row_of_a_split_line_and_the_rows_above_are_indented() {
    let out = render(
        "- Before ![tall](t.png) after.\n",
        &drawing(&Capabilities::kitty(WIDTH), 40, 18),
    );
    let lines: Vec<String> = shown(&out).into_iter().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 5, "{lines:?}");
    let marked: Vec<usize> = (lines.iter().enumerate())
        .filter(|(_, line)| line.contains('•'))
        .map(|(row, _)| row)
        .collect();
    assert_eq!(marked, [4], "{lines:?}");
    assert!(lines[4].starts_with("• Before"), "{lines:?}");
    // The image stands in one column on every row, after the indent.
    let raw: Vec<String> = columns_shown(&out)
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let at = |line: &String| line.chars().position(|c| c == '⟦').unwrap();
    assert!(raw.iter().all(|line| at(line) == at(&raw[0])), "{raw:?}");
    // Numbers go on the text row too.
    let out = render(
        "1. Before ![tall](t.png) after.\n",
        &drawing(&Capabilities::kitty(WIDTH), 40, 18),
    );
    let lines: Vec<String> = shown(&out).into_iter().filter(|l| !l.is_empty()).collect();
    assert!(lines[4].starts_with("1. Before"), "{lines:?}");
    assert!(!lines[0].contains("1."), "{lines:?}");
}

#[test]
fn a_list_marker_is_before_the_quote_bar_whatever_the_rows_of_the_line() {
    // 18 pixels high: one row; 32: two rows.
    let prefix = |height| {
        let out = render(
            "- > A $x$ b.\n",
            &drawing(&Capabilities::kitty(WIDTH), 40, height),
        );
        let first = columns_shown(&out)
            .into_iter()
            .find(|l| l.contains('⟦'))
            .unwrap();
        first[..first.find('A').unwrap()].to_owned()
    };
    assert_eq!(prefix(18), "  • │ ");
    assert_eq!(prefix(32), "  • │ ");
    // On the second row of the two, the bar goes on and the marker is blank.
    let out = render(
        "- > A $x$ b.\n",
        &drawing(&Capabilities::kitty(WIDTH), 40, 32),
    );
    let rows: Vec<String> = columns_shown(&out)
        .into_iter()
        .filter(|l| l.contains('⟦'))
        .collect();
    assert_eq!(rows.len(), 2);
    assert!(rows[1].starts_with("    │ "), "{rows:?}");
}
