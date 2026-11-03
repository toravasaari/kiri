//! What the output terminal can do, as plain data: the renderer and the
//! pager take it as input, and `term` finds it out.

use crate::present::kitty::graphics;
use crate::typeset::color::Color;

/// The protocol a terminal shows images with. Sixel and others join here
/// as variants, each with what is true of that protocol alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Graphics {
    /// No images.
    None,
    /// kitty's graphics protocol: the terminal answered its query.
    Kitty(KittyGraphics),
}

/// Who plays an animated image, so that it moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Playback {
    /// The terminal: it is sent every frame once, and told to start and
    /// stop.
    Terminal,
    /// kiri: the terminal shows an image it is sent, and kiri sends the
    /// next frame in its time, so only while the pager runs.
    Redraw,
    /// No one: the first frame.
    #[default]
    None,
}

/// What is known of a terminal that answers kitty's graphics protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KittyGraphics {
    /// The most rows and the most columns one image may cover, placed with
    /// the protocol's Unicode placeholders, which is how kiri shows every
    /// image; the number of placeholder diacritics. 0 when the terminal has
    /// no placeholders: not all that answer the graphics query do.
    pub max_image_cells: u16,
    /// Who plays an animated image: kitty does itself, from the versions
    /// kiri shows images in; Ghostty shows the first frame, so kiri sends
    /// it the frames one after the other; kiri does not count on either
    /// behind tmux.
    pub playback: Playback,
    /// The terminal's name and version as it told them, such as
    /// `kitty(0.47.1)` or `ghostty 1.1.3`; `None` when it did not.
    pub terminal: Option<String>,
}

impl Graphics {
    /// kitty's protocol as kitty has it, with every placeholder.
    pub fn kitty() -> Self {
        Self::Kitty(KittyGraphics {
            max_image_cells: graphics::MAX_CELLS,
            playback: Playback::Terminal,
            terminal: None,
        })
    }

    /// kitty's protocol with an image limit of `max_image_cells`.
    pub fn kitty_cells(max_image_cells: u16) -> Self {
        match Self::kitty() {
            Self::Kitty(kitty) => Self::Kitty(KittyGraphics {
                max_image_cells,
                ..kitty
            }),
            other => other,
        }
    }

    /// The protocol's name, for people.
    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Kitty(_) => "kitty",
        }
    }
}

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
    /// How the terminal shows images: the protocol it answered to.
    pub graphics: Graphics,
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
            graphics: Graphics::None,
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
        self.styling && self.max_image_cells() > 0
    }

    /// The most rows and the most columns one image may cover; 0 when the
    /// terminal draws no images. See [`KittyGraphics::max_image_cells`].
    pub fn max_image_cells(&self) -> u16 {
        match &self.graphics {
            Graphics::None => 0,
            Graphics::Kitty(kitty) => kitty.max_image_cells,
        }
    }

    /// Whether the terminal plays an animated image on its own once it has
    /// all the frames and is told to start.
    pub fn plays_animations(&self) -> bool {
        self.playback() == Playback::Terminal
    }

    /// Who plays an animated image on this terminal.
    pub fn playback(&self) -> Playback {
        match &self.graphics {
            Graphics::Kitty(kitty) if self.draws_images() => kitty.playback,
            _ => Playback::None,
        }
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
            graphics: Graphics::kitty(),
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
            graphics: Graphics::Kitty(KittyGraphics {
                max_image_cells: 0,
                playback: Playback::Terminal,
                terminal: None,
            }),
            ..Capabilities::kitty(80)
        };
        assert!(!caps.draws_images());
        assert!(Capabilities::kitty(80).draws_images());
    }
}
