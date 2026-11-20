//! How the contents bar looks in colours: its cells with their styles, as
//! snapshots, for the default theme with and without the focus, a theme
//! with keys of its own and a terminal without colours.

use super::*;
use crate::typeset::caps::Capabilities;
use crate::typeset::style::Style;
use crate::typeset::theme::Theme;

const SOURCE: &str = "# Notes\n\nIntro text.\n\n## Setup\n\nSetup text.\n\n### Details\n\nMore.\n\n## Usage\n\nUsage text.\n";

/// A kitty terminal that tells its colours, as kitty does.
fn colored() -> Capabilities {
    let mut caps = Capabilities::kitty(120);
    caps.background = Some(Color::Rgb(0x1e, 0x1e, 0x2e));
    caps.foreground = Some(Color::Rgb(0xcd, 0xd6, 0xf4));
    caps.palette[3] = Some(Color::Rgb(0xf9, 0xe2, 0xaf));
    caps.palette[8] = Some(Color::Rgb(0x58, 0x5b, 0x70));
    caps
}

/// The screens of a run of `SOURCE` in a window of 120 columns in `caps`,
/// with `theme`, and the terminal giving `keys`.
fn ran_in(caps: Capabilities, theme: Theme, keys: &[&[u8]]) -> Fake {
    let document = crate::typeset::parse::parse(SOURCE);
    let mut terminal = Fake::new(120, 12, keys);
    let mut inputs = Inputs::new(caps);
    inputs.theme = theme;
    run_document(
        &document,
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    terminal
}

/// A colour as `#rrggbb`, or the palette's number.
fn colour(color: Option<Color>) -> String {
    match color {
        None => "-".to_owned(),
        Some(Color::Rgb(r, g, b)) => format!("#{r:02x}{g:02x}{b:02x}"),
        Some(Color::Palette(n)) => format!("p{n}"),
    }
}

/// What a style does to its cells: the colours, `-` for the terminal's own,
/// and the letters of bold (b), dim (d), italic (i) and reverse (r).
fn describe(style: &Style) -> String {
    let marks: String = [
        (style.bold, 'b'),
        (style.dim, 'd'),
        (style.italic, 'i'),
        (style.reverse, 'r'),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, mark)| *mark)
    .collect();
    format!("{} on {} {marks}", colour(style.fg), colour(style.bg))
}

/// The first 30 columns of each row of the bar of `seen` that is not the
/// status line, each run of cells with one style as `‹fg on bg marks›text`.
fn styled(seen: &Seen) -> String {
    let mut out = Vec::new();
    for row in &seen.rows[..seen.rows.len() - 1] {
        let mut line = String::new();
        let mut last: Option<String> = None;
        for cell in row.cells.iter().take(30) {
            let now = describe(&cell.style);
            if last.as_ref() != Some(&now) {
                line.push_str(&format!("‹{now}›"));
                last = Some(now);
            }
            line.push_str(&cell.text);
        }
        out.push(line);
    }
    out.join("\n")
}

#[test]
fn the_default_theme_draws_a_coloured_bar() {
    let terminal = ran_in(colored(), Theme::default(), &[b"q"]);
    insta::assert_snapshot!(styled(&terminal.seen()[0]));
}

#[test]
fn the_default_theme_with_the_focus_in_the_bar() {
    let terminal = ran_in(colored(), Theme::default(), &[b"t", b"j", b"q"]);
    insta::assert_snapshot!(styled(terminal.seen().last().unwrap()));
}

#[test]
fn a_theme_with_keys_of_its_own_draws_them() {
    let theme = crate::typeset::theme::parse(
        "[contents]\nbackground = \"#101820\"\ntitle = \"#ffd479\"\ntext = \"#9fb4c7\"\n\
         glyph = \"#5c6f80\"\ncurrent = \"#ff7a59\"\ncurrent_background = \"#2a2320\"\n\
         selected = \"#59c2ff\"\nborder = \"#33414f\"\n",
    )
    .unwrap();
    let terminal = ran_in(colored(), theme, &[b"t", b"j", b"q"]);
    insta::assert_snapshot!(styled(terminal.seen().last().unwrap()));
}

#[test]
fn without_colours_the_bar_is_plain_with_the_current_row_in_reverse_video() {
    let mut caps = Capabilities::kitty(120);
    caps.color = false;
    let terminal = ran_in(caps, Theme::default().without_colors(), &[b"q"]);
    insta::assert_snapshot!(styled(&terminal.seen()[0]));
}

#[test]
fn without_colours_the_selection_is_in_reverse_video_with_the_focus() {
    let mut caps = Capabilities::kitty(120);
    caps.color = false;
    let theme = Theme::default().without_colors();
    let terminal = ran_in(caps, theme, &[b"t", b"j", b"q"]);
    insta::assert_snapshot!(styled(terminal.seen().last().unwrap()));
}
