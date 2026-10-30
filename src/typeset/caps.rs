//! What the output terminal can do, as plain data: the renderer and the
//! pager take it as input, and `term` finds it out.

use crate::present::kitty::graphics;
use crate::typeset::color::Color;

/// Terminal features the renderer may use. Built once at start-up and passed
/// to the renderer, so rendering itself never talks to the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// Width in cells.
    pub width: u16,
    /// Height of the terminal window in cells; `None` when it is not known
    /// or the terminal reports no rows.
    pub height: Option<u16>,
    /// Colours and text attributes through escape sequences.
    pub styling: bool,
    /// Foreground and background colours, with `styling`. Off when the user
    /// sets `NO_COLOR`; text attributes, links and sizes stay.
    pub color: bool,
    /// 24-bit colour; without it, RGB colours are approximated.
    pub truecolor: bool,
    /// OSC 8 hyperlinks.
    pub hyperlinks: bool,
    /// kitty's text sizing protocol (OSC 66).
    pub text_sizing: bool,
    /// kitty's graphics protocol: the terminal answered its query.
    pub graphics: bool,
    /// The most rows and the most columns one image may cover, placed with
    /// the graphics protocol's Unicode placeholders, which is how kiri shows
    /// every image; for kitty, the number of placeholder diacritics. 0 when
    /// the terminal has no placeholders: not all that answer the graphics
    /// query do.
    pub max_image_cells: u16,
    /// Underline styles other than single (SGR `4:x`), which kitty
    /// introduced; without them every underline is single. Only the probe
    /// sets it, apart from `--mode kitty`.
    pub styled_underlines: bool,
    /// Size of one cell in pixels, when the terminal reports it: in the
    /// window size, or else in an answer to the probe.
    pub cell_pixels: Option<(u16, u16)>,
    /// Whether the background is dark; `None` when unknown.
    pub dark_background: Option<bool>,
    /// The foreground and background colours and the 16 palette colours
    /// as RGB, when the terminal tells them, for colours mixed from them.
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub palette: [Option<Color>; 16],
    /// Whether graphics sequences are to be wrapped in tmux's passthrough
    /// on their way out, as kiri runs inside tmux.
    pub tmux_passthrough: bool,
    /// Something unique to this run, mixed into every image id, where
    /// other programs share the terminal's images and may show the same
    /// image under kiri's id: under tmux, every pane of a kitty window
    /// shares them. `None` gives an image the same id on every run.
    pub image_run: Option<u64>,
}

impl Capabilities {
    /// No escape sequences at all, for pipes and files.
    pub fn plain(width: u16) -> Self {
        Self {
            width,
            height: None,
            styling: false,
            color: false,
            truecolor: false,
            hyperlinks: false,
            text_sizing: false,
            graphics: false,
            max_image_cells: 0,
            styled_underlines: false,
            cell_pixels: None,
            dark_background: None,
            foreground: None,
            background: None,
            palette: [None; 16],
            tmux_passthrough: false,
            image_run: None,
        }
    }

    /// Whether images are drawn rather than shown as alt text.
    pub fn draws_images(&self) -> bool {
        self.styling && self.graphics && self.max_image_cells > 0
    }

    /// Colours and attributes that any modern terminal understands.
    pub fn ansi(width: u16) -> Self {
        Self {
            styling: true,
            color: true,
            ..Self::plain(width)
        }
    }

    /// Everything kitty supports.
    pub fn kitty(width: u16) -> Self {
        Self {
            styling: true,
            color: true,
            truecolor: true,
            hyperlinks: true,
            text_sizing: true,
            graphics: true,
            max_image_cells: graphics::MAX_CELLS,
            styled_underlines: true,
            ..Self::plain(width)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_images_only_when_an_image_may_cover_a_cell() {
        // Images would be loaded and then not drawn.
        let caps = Capabilities {
            max_image_cells: 0,
            ..Capabilities::kitty(80)
        };
        assert!(!caps.draws_images());
        assert!(Capabilities::kitty(80).draws_images());
    }
}
