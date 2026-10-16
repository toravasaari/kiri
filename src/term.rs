//! What the output terminal can do.

/// Terminal features the renderer may use. Built once at start-up and passed
/// to the renderer, so rendering itself never talks to the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// Width in cells.
    pub width: u16,
    /// Colours and text attributes through escape sequences.
    pub styling: bool,
    /// 24-bit colour; without it, RGB colours are approximated.
    pub truecolor: bool,
    /// OSC 8 hyperlinks.
    pub hyperlinks: bool,
    /// kitty's text sizing protocol (OSC 66).
    pub text_sizing: bool,
    /// kitty's graphics protocol.
    pub graphics: bool,
    /// Size of one cell in pixels, when the terminal reports it.
    pub cell_pixels: Option<(u16, u16)>,
    /// Whether the background is dark; `None` when unknown.
    pub dark_background: Option<bool>,
}

impl Capabilities {
    /// No escape sequences at all, for pipes and files.
    pub fn plain(width: u16) -> Self {
        Self {
            width,
            styling: false,
            truecolor: false,
            hyperlinks: false,
            text_sizing: false,
            graphics: false,
            cell_pixels: None,
            dark_background: None,
        }
    }

    /// Colours and attributes that any modern terminal understands.
    pub fn ansi(width: u16) -> Self {
        Self {
            styling: true,
            ..Self::plain(width)
        }
    }

    /// Everything kitty supports.
    pub fn kitty(width: u16) -> Self {
        Self {
            styling: true,
            truecolor: true,
            hyperlinks: true,
            text_sizing: true,
            graphics: true,
            ..Self::plain(width)
        }
    }
}
