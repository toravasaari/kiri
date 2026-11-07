//! The terminal's answers to the probe's queries, read without I/O.

use crate::present::kitty::query;
use crate::typeset::color::Color;

/// Start of an answer to `probe::palette`:
/// `ESC ] 4 ; index ; rgb:…`.
pub(crate) const PALETTE_ANSWER: &str = "\x1b]4;";

/// Start of the answer to `probe::VERSION`:
/// `ESC P > | name(version) ST`.
const VERSION_ANSWER: &str = "\x1bP>|";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    pub text_sizing: bool,
    /// The terminal answered kitty's graphics query.
    pub kitty_graphics: bool,
    /// Its device attributes list 4, sixel graphics: the first answer, or
    /// behind tmux both tmux's and the outer terminal's.
    pub sixel: bool,
    pub image_placeholders: bool,
    /// The terminal's name and version, as `version` reads them.
    pub terminal: Option<String>,
    pub dark_background: Option<bool>,
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub palette: [Option<Color>; 16],
    /// Width and height of a cell in pixels, when asked and answered.
    pub cell_pixels: Option<(u16, u16)>,
}

/// How many primary device attributes answers, `ESC [ ? digits and
/// semicolons c`, there are in `answers`: behind tmux, both tmux and the
/// terminal outside it answer.
pub(crate) fn device_attributes(answers: &[u8]) -> usize {
    device_attribute_answers(&String::from_utf8_lossy(answers)).len()
}

pub fn interpret(answers: &[u8]) -> Probe {
    let text = String::from_utf8_lossy(answers);
    // No query tells whether a terminal supports Unicode placeholders,
    // and some that answer the graphics query do not. A kitty from 0.28
    // on has them, which its version answer tells. Text sizing came to
    // kitty in 0.40, so kiri also takes a terminal with both protocols
    // to be a kitty that has placeholders. Other terminals with
    // placeholders get alt text for now.
    let text_sizing = query::text_sizing_answered(&cursor_columns(&text));
    let kitty_graphics = text.contains(query::GRAPHICS_ANSWER);
    let terminal = version(&text);
    let placeholders_by_version = terminal.is_some_and(query::placeholders_by_version);
    Probe {
        text_sizing,
        kitty_graphics,
        sixel: sixel(&text),
        image_placeholders: kitty_graphics && (text_sizing || placeholders_by_version),
        terminal: terminal.map(str::to_owned),
        dark_background: background_is_dark(&text),
        foreground: foreground(&text),
        background: background(&text),
        palette: palette_colors(&text),
        cell_pixels: query::cell_size(&text),
    }
}

/// The terminal's name and version from the answer to
/// `probe::VERSION`, ended by ST or BEL.
pub fn version(text: &str) -> Option<&str> {
    let start = text.find(VERSION_ANSWER)? + VERSION_ANSWER.len();
    let rest = &text[start..];
    let end = rest.find(['\x1b', '\x07'])?;
    Some(&rest[..end])
}

/// Whether `text`'s primary device attributes answers list attribute 4,
/// sixel graphics: the first answer, or when the version answer in `text`
/// starts with "tmux", the first two, tmux's and the one of the terminal
/// outside it, which must both list it.
fn sixel(text: &str) -> bool {
    let behind_tmux = version(text).is_some_and(|version| version.starts_with("tmux"));
    let needed = if behind_tmux { 2 } else { 1 };
    let answers = device_attribute_answers(text);
    answers.len() >= needed
        && answers[..needed]
            .iter()
            .all(|params| params.split(';').any(|param| param.parse::<u32>() == Ok(4)))
}

/// The parameters of each primary device attributes answer in `text`,
/// `ESC [ ? digits and semicolons c`, in order.
fn device_attribute_answers(text: &str) -> Vec<&str> {
    const START: &str = "\x1b[?";
    text.match_indices(START)
        .filter_map(|(start, _)| {
            let rest = &text[start + START.len()..];
            let end = rest.find(|c: char| !(c.is_ascii_digit() || c == ';'))?;
            rest[end..].starts_with('c').then(|| &rest[..end])
        })
        .collect()
}

/// Columns from cursor position reports: `ESC [ row ; column R`.
fn cursor_columns(text: &str) -> Vec<u32> {
    text.split("\x1b[")
        .filter_map(|part| {
            let report = &part[..part.find('R')?];
            let (row, column) = report.split_once(';')?;
            row.parse::<u32>().ok()?;
            column.parse().ok()
        })
        .collect()
}

/// The channels of `rgb:RRRR/GGGG/BBBB` at the start of `text`, with
/// one to four hex digits each, as fractions of their maximum.
fn channels(text: &str) -> Option<[f64; 3]> {
    let rest = text.strip_prefix("rgb:")?;
    let end = rest.find(|c: char| !(c.is_ascii_hexdigit() || c == '/'))?;
    let channels: Vec<f64> = rest[..end]
        .split('/')
        .map(|hex| {
            let max = 16f64.powi(hex.len() as i32) - 1.0;
            u32::from_str_radix(hex, 16)
                .ok()
                .map(|v| f64::from(v) / max)
        })
        .collect::<Option<_>>()?;
    channels.try_into().ok()
}

fn rgb([r, g, b]: [f64; 3]) -> Color {
    let byte = |channel: f64| (channel * 255.0).round() as u8;
    Color::Rgb(byte(r), byte(g), byte(b))
}

/// The channels of the answer to `probe::BACKGROUND`.
fn background_channels(text: &str) -> Option<[f64; 3]> {
    let start = text.find("\x1b]11;")? + "\x1b]11;".len();
    channels(&text[start..])
}

/// The colour in the answer to `probe::FOREGROUND`.
fn foreground(text: &str) -> Option<Color> {
    let start = text.find("\x1b]10;")? + "\x1b]10;".len();
    channels(&text[start..]).map(rgb)
}

fn background_is_dark(text: &str) -> Option<bool> {
    let [r, g, b] = background_channels(text)?;
    Some(0.2126 * r + 0.7152 * g + 0.0722 * b < 0.5)
}

fn background(text: &str) -> Option<Color> {
    background_channels(text).map(rgb)
}

/// The colours among the answers to `probe::palette`.
fn palette_colors(text: &str) -> [Option<Color>; 16] {
    let mut palette = [None; 16];
    for (start, _) in text.match_indices(PALETTE_ANSWER) {
        let rest = &text[start + PALETTE_ANSWER.len()..];
        let Some((index, color)) = rest.split_once(';') else {
            continue;
        };
        if let Ok(index) = index.parse::<usize>()
            && let Some(entry) = palette.get_mut(index)
        {
            *entry = channels(color).map(rgb);
        }
    }
    palette
}

#[cfg(test)]
mod tests {
    use super::*;

    const KITTY: &str = concat!(
        "\x1b[5;1R\x1b[5;3R\x1b[5;5R",
        "\x1b_Gi=31;OK\x1b\\",
        "\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\",
        "\x1b[?62;c",
    );

    #[test]
    fn reads_the_foreground_background_and_palette_colours() {
        // Four hex digits a channel ended by ST, two ended by BEL.
        let answers = concat!(
            "\x1b]10;rgb:d4/d4/d4\x07",
            "\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\",
            "\x1b]4;1;rgb:cccc/0000/0000\x1b\\",
            "\x1b]4;12;rgb:44/93/f8\x07",
            "\x1b]4;16;rgb:ff/ff/ff\x07",
            "\x1b[?62;c",
        );
        let probe = interpret(answers.as_bytes());
        assert_eq!(probe.foreground, Some(Color::Rgb(0xd4, 0xd4, 0xd4)));
        assert_eq!(probe.background, Some(Color::Rgb(0x1e, 0x1e, 0x2e)));
        let mut palette = [None; 16];
        palette[1] = Some(Color::Rgb(0xcc, 0, 0));
        palette[12] = Some(Color::Rgb(0x44, 0x93, 0xf8));
        // Entries beyond the 16 kiri asks for are left out.
        assert_eq!(probe.palette, palette);
    }

    #[test]
    fn malformed_colour_answers_are_left_out() {
        let answers = concat!(
            "\x1b]4;99;rgb:ff/ff/ff\x1b\\",
            "\x1b]4;x;rgb:ff/ff/ff\x1b\\",
            "\x1b]4;2;#ffffff\x1b\\",
            "\x1b]4;3;rgb:ff/ff\x1b\\",
            "\x1b]4;5;rgb:zz/00/00\x1b\\",
            "\x1b]4;6;rgb:12/34",
        );
        let probe = interpret(answers.as_bytes());
        assert_eq!(probe.palette, [None; 16]);
        // An answer cut short has no colour.
        let cut = "\x1b]11;rgb:1e1e/1e";
        assert_eq!(interpret(cut.as_bytes()).background, None);
    }

    #[test]
    fn reads_the_cell_size_among_the_answers() {
        let answers = KITTY.replace("\x1b[?62;c", "\x1b[6;19;9t\x1b[?62;c");
        assert_eq!(interpret(answers.as_bytes()).cell_pixels, Some((9, 19)));
        assert_eq!(interpret(KITTY.as_bytes()).cell_pixels, None);
    }

    #[test]
    fn recognises_kitty() {
        let probe = interpret(KITTY.as_bytes());
        assert_eq!(
            probe,
            Probe {
                text_sizing: true,
                kitty_graphics: true,
                sixel: false,
                image_placeholders: true,
                terminal: None,
                dark_background: Some(true),
                foreground: None,
                background: Some(Color::Rgb(0x1e, 0x1e, 0x2e)),
                palette: [None; 16],
                cell_pixels: None,
            }
        );
        assert_eq!(device_attributes(KITTY.as_bytes()), 1);
    }

    #[test]
    fn recognises_a_terminal_without_kitty_protocols() {
        let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b]11;rgb:ffff/ffff/ffff\x07\x1b[?1;2c";
        assert_eq!(
            interpret(answers.as_bytes()),
            Probe {
                text_sizing: false,
                kitty_graphics: false,
                sixel: false,
                image_placeholders: false,
                terminal: None,
                dark_background: Some(false),
                foreground: None,
                background: Some(Color::Rgb(0xff, 0xff, 0xff)),
                palette: [None; 16],
                cell_pixels: None,
            }
        );
    }

    #[test]
    fn graphics_alone_does_not_promise_placeholders() {
        // A terminal that answers the graphics query but has no text
        // sizing, as WezTerm and Konsole do: they show images but not
        // through Unicode placeholders.
        let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b_Gi=31;OK\x1b\\\x1b[?62;c";
        let probe = interpret(answers.as_bytes());
        assert!(probe.kitty_graphics);
        assert!(!probe.image_placeholders);
    }

    /// Answers from a kitty that names itself `version` in its
    /// XTVERSION answer, with or without text sizing.
    fn kitty_answers(version: &str, text_sizing: bool) -> String {
        let columns = if text_sizing {
            "\x1b[5;1R\x1b[5;3R\x1b[5;5R"
        } else {
            "\x1b[5;1R\x1b[5;1R\x1b[5;1R"
        };
        format!(
            "{columns}\x1b_Gi=31;OK\x1b\\\x1b]11;rgb:0000/0000/0000\x1b\\\
             \x1bP>|kitty({version})\x1b\\\x1b[?62;c"
        )
    }

    #[test]
    fn kitty_0_28_has_placeholders_without_text_sizing() {
        let probe = interpret(kitty_answers("0.28.0", false).as_bytes());
        assert!(!probe.text_sizing);
        assert!(probe.kitty_graphics);
        assert!(probe.image_placeholders);
    }

    #[test]
    fn kitty_0_27_has_no_placeholders() {
        let probe = interpret(kitty_answers("0.27.1", false).as_bytes());
        assert!(probe.kitty_graphics);
        assert!(!probe.image_placeholders);
    }

    #[test]
    fn kitty_0_47_1_has_placeholders_and_text_sizing() {
        let answers = kitty_answers("0.47.1", true);
        let probe = interpret(answers.as_bytes());
        assert!(probe.text_sizing);
        assert!(probe.image_placeholders);
        assert_eq!(device_attributes(answers.as_bytes()), 1);
    }

    #[test]
    fn a_kitty_version_without_graphics_has_no_placeholders() {
        let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1bP>|kitty(0.30.0)\x1b\\\x1b[?62;c";
        assert!(!interpret(answers.as_bytes()).image_placeholders);
    }

    #[test]
    fn an_unknown_terminal_version_promises_no_placeholders() {
        let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b_Gi=31;OK\x1b\\\
                       \x1bP>|WezTerm 20240203-110809-5046fc22\x1b\\\x1b[?62;c";
        let probe = interpret(answers.as_bytes());
        assert!(probe.kitty_graphics);
        assert!(!probe.image_placeholders);
    }

    #[test]
    fn reads_a_version_answer_ended_by_bel() {
        let answers = "\x1b_Gi=31;OK\x1b\\\x1bP>|kitty(0.28.0)\x07\x1b[?62;c";
        assert!(interpret(answers.as_bytes()).image_placeholders);
    }

    #[test]
    fn needs_both_width_and_scale_for_text_sizing() {
        let answers = "\x1b[5;1R\x1b[5;3R\x1b[5;3R\x1b[?62;c";
        assert!(!interpret(answers.as_bytes()).text_sizing);
    }

    #[test]
    fn waits_for_device_attributes() {
        assert_eq!(device_attributes(b"\x1b[5;1R\x1b[5;3R"), 0);
        assert_eq!(device_attributes(b"\x1b[?62;"), 0);
    }

    #[test]
    fn counts_the_device_attributes_of_tmux_and_kitty() {
        let tmux = "\x1bP>|tmux 3.8\x1b\\\x1b[?1;2;4c";
        assert_eq!(device_attributes(tmux.as_bytes()), 1);
        let both = format!("{tmux}\x1b_Gi=31;OK\x1b\\\x1b[?62;52;c");
        assert_eq!(device_attributes(both.as_bytes()), 2);
        assert_eq!(device_attributes(b"\x1b[?62;"), 0);
    }

    fn sixel_of(answers: &str) -> bool {
        interpret(answers.as_bytes()).sixel
    }

    #[test]
    fn sixel_is_attribute_4_of_the_device_attributes() {
        assert!(sixel_of("\x1b[?62;4;22c"));
        assert!(sixel_of("\x1b[?1;2;4c"));
        assert!(!sixel_of("\x1b[?62;c"));
        assert!(!sixel_of("\x1b[?62;22c"));
    }

    #[test]
    fn sixel_is_attribute_4_and_not_a_number_that_contains_it() {
        assert!(!sixel_of("\x1b[?64;14;24;42c"));
    }

    #[test]
    fn without_tmux_only_the_first_answer_counts() {
        assert!(!sixel_of("\x1b[?62;c\x1b[?1;2;4c"));
    }

    #[test]
    fn behind_tmux_sixel_needs_tmux_s_answer_and_the_outer_terminal_s_both() {
        let tmux = "\x1bP>|tmux 3.8\x1b\\";
        // Kitty outside tmux does not list 4.
        assert!(!sixel_of(&format!(
            "{tmux}\x1b[?1;2;4c\x1b_Gi=31;OK\x1b\\\x1b[?62;52;c"
        )));
        // A sixel terminal outside tmux.
        assert!(sixel_of(&format!("{tmux}\x1b[?1;2;4c\x1b[?65;4;22c")));
        // tmux itself without sixel.
        assert!(!sixel_of(&format!("{tmux}\x1b[?1;2c\x1b[?65;4;22c")));
        // No answer from outside, as when the query is skipped.
        assert!(!sixel_of(&format!("{tmux}\x1b[?1;2;4c")));
    }
}
