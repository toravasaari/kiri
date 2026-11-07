//! The renditions of an embed: the ways it can be shown, which it offers in
//! the order it prefers them; which of them a terminal shows; and the choice
//! between them. Pure: the capabilities and where each rendition is are
//! given. How the chosen one is drawn is the layout's.

use crate::typeset::caps::Capabilities;
use crate::typeset::images::{ImageKey, Images, Placeholder, Source};

/// A way to show an embed that the layout lays out in its own way. An image
/// protocol is not one: which protocol carries a picture's bytes is the
/// terminal layer's choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenditionKind {
    /// The picture itself.
    Picture,
    /// The embed's alt text, as the paragraph's own text.
    AltText,
}

/// Whether a rendition can be laid out now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Readiness {
    /// It is there.
    Ready,
    /// It is not, but something stands in for it while it loads.
    StandIn,
    /// It is not there and nothing stands in for it.
    Missing,
}

/// What an image block offers, in the order it prefers them: its
/// picture, then its alt text. An offer is a kind alone until something
/// needs a size with it (G2.6.4).
pub(crate) const IMAGE_OFFERS: [RenditionKind; 2] =
    [RenditionKind::Picture, RenditionKind::AltText];

/// Whether a terminal with `caps` shows a rendition of this `kind`.
pub(crate) fn shows(kind: RenditionKind, caps: &Capabilities) -> bool {
    match kind {
        RenditionKind::Picture => caps.draws_images(),
        RenditionKind::AltText => true,
    }
}

/// The first of `offers`, in order, that the terminal shows and that is
/// ready or stood in for; `None` when there is no such offer.
pub(crate) fn resolve(
    offers: &[RenditionKind],
    caps: &Capabilities,
    readiness: impl Fn(RenditionKind) -> Readiness,
) -> Option<RenditionKind> {
    offers
        .iter()
        .copied()
        .find(|&kind| shows(kind, caps) && readiness(kind) != Readiness::Missing)
}

/// What there is to show of a picture: the one decision of what a picture
/// is, taken here for the layout to lay out and for [`picture_readiness`]
/// to name.
pub(crate) enum PictureState<'a> {
    /// The picture is loaded.
    Loaded(&'a Source),
    /// It is not, and the old picture, dimmed, stands in.
    Dimmed(&'a Source),
    /// It is not, and a box of this many rows stands in.
    Rows(u16),
    /// A failure or no size: there is nothing to show.
    Missing,
}

/// The state of the picture `key` among `images`, in the order the layout
/// looks: loaded wins whatever stands in; a dimmed old picture or a box of
/// rows stands in; anything else, a failure or no size, is missing.
pub(crate) fn picture_state<'a>(images: &'a Images, key: &ImageKey) -> PictureState<'a> {
    match (images.get(key), images.placeholder(key)) {
        (Some(Ok(source)), _) => PictureState::Loaded(source),
        (_, Some(Placeholder::Dimmed(old))) => PictureState::Dimmed(old),
        (_, Some(&Placeholder::Rows(rows))) => PictureState::Rows(rows),
        _ => PictureState::Missing,
    }
}

/// Where the picture `key` is among `images`: ready when loaded, stood in
/// for while something does, missing otherwise.
pub(crate) fn picture_readiness(images: &Images, key: &ImageKey) -> Readiness {
    match picture_state(images, key) {
        PictureState::Loaded(_) => Readiness::Ready,
        PictureState::Dimmed(_) | PictureState::Rows(_) => Readiness::StandIn,
        PictureState::Missing => Readiness::Missing,
    }
}

/// The stores of `picture_readiness`'s cases, each with its name and
/// readiness, for this module's tests and `image_block`'s.
#[cfg(test)]
pub(crate) fn picture_cases(key: &ImageKey) -> Vec<(&'static str, Images, Readiness)> {
    use crate::typeset::images::{ImageError, Source};
    let pixels = || Source::Raster(::image::RgbaImage::new(4, 3));
    let with = |loaded: Option<Result<Source, ImageError>>, waiting: Option<Placeholder>| {
        let mut images = Images::new();
        if let Some(loaded) = loaded {
            images.insert(key.clone(), loaded);
        }
        if let Some(waiting) = waiting {
            images.wait(key.clone(), waiting);
        }
        images
    };
    let (dimmed, failed) = (
        || Some(Placeholder::Dimmed(pixels())),
        || Some(Err(ImageError::Remote)),
    );
    vec![
        ("nothing", with(None, None), Readiness::Missing),
        ("loaded", with(Some(Ok(pixels())), None), Readiness::Ready),
        ("failed", with(failed(), None), Readiness::Missing),
        ("old one dimmed", with(None, dimmed()), Readiness::StandIn),
        (
            "box of rows",
            with(None, Some(Placeholder::Rows(3))),
            Readiness::StandIn,
        ),
        (
            "no size",
            with(None, Some(Placeholder::Unsized)),
            Readiness::Missing,
        ),
        (
            "loaded, loading again",
            with(Some(Ok(pixels())), dimmed()),
            Readiness::Ready,
        ),
        (
            "failed, loading again",
            with(failed(), dimmed()),
            Readiness::StandIn,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::caps::{Graphics, KittyGraphics, Playback};

    /// The terminals of the capability matrix, and whether each draws images.
    fn matrix() -> Vec<(&'static str, Capabilities, bool)> {
        let kitty_like = |playback, name: &str| match Graphics::kitty() {
            Graphics::Kitty(k) => Graphics::Kitty(KittyGraphics {
                playback,
                terminal: Some(name.to_owned()),
                ..k
            }),
            other => other,
        };
        let kitty = Capabilities::kitty(80);
        let tmux = Capabilities {
            text_sizing: false,
            hyperlinks: false,
            tmux_passthrough: true,
            graphics: kitty_like(Playback::None, "kitty(0.47.1)"),
            ..kitty.clone()
        };
        let ghostty = Capabilities {
            text_sizing: false,
            graphics: kitty_like(Playback::Redraw, "ghostty 1.1.3"),
            ..kitty.clone()
        };
        let no_cells = Capabilities {
            graphics: Graphics::kitty_cells(0),
            ..kitty.clone()
        };
        vec![
            ("plain", Capabilities::plain(80), false),
            ("ansi", Capabilities::ansi(80), false),
            ("kitty", kitty.clone(), true),
            (
                "kitty, no colour",
                Capabilities {
                    color: false,
                    ..kitty.clone()
                },
                true,
            ),
            ("kitty in tmux", tmux, true),
            ("ghostty", ghostty, true),
            ("graphics without placeholders", no_cells, false),
            (
                "kitty unstyled",
                Capabilities {
                    styling: false,
                    ..kitty
                },
                false,
            ),
        ]
    }

    #[test]
    fn the_picture_shows_where_the_terminal_draws_images_and_alt_text_everywhere() {
        for (name, caps, draws) in matrix() {
            assert_eq!(shows(RenditionKind::Picture, &caps), draws, "{name}");
            assert!(shows(RenditionKind::AltText, &caps), "{name}");
        }
    }

    #[test]
    fn an_image_block_shows_its_picture_where_drawn_and_there_or_stood_in_for() {
        for (name, caps, draws) in matrix() {
            for (picture, there) in [
                (Readiness::Ready, true),
                (Readiness::StandIn, true),
                (Readiness::Missing, false),
            ] {
                let chosen = resolve(&IMAGE_OFFERS, &caps, |kind| match kind {
                    RenditionKind::Picture => picture,
                    RenditionKind::AltText => Readiness::Ready,
                });
                let expected = match draws && there {
                    true => RenditionKind::Picture,
                    false => RenditionKind::AltText,
                };
                assert_eq!(chosen, Some(expected), "{name}, {picture:?}");
            }
        }
    }

    #[test]
    fn offers_are_taken_in_their_order() {
        let ([picture, alt], kitty) = (IMAGE_OFFERS, Capabilities::kitty(80));
        assert_eq!(
            resolve(&[alt, picture], &kitty, |_| Readiness::Ready),
            Some(alt)
        );
        assert_eq!(
            resolve(&[picture, alt], &kitty, |_| Readiness::Ready),
            Some(picture)
        );
    }

    #[test]
    fn nothing_is_chosen_when_no_offer_shows_or_is_there() {
        let [picture, _] = IMAGE_OFFERS;
        let kitty = Capabilities::kitty(80);
        assert_eq!(
            resolve(&[picture], &Capabilities::plain(80), |_| Readiness::Ready),
            None
        );
        assert_eq!(resolve(&[picture], &kitty, |_| Readiness::Missing), None);
        assert_eq!(resolve(&[], &kitty, |_| Readiness::Ready), None);
    }

    #[test]
    fn a_picture_is_ready_loaded_stood_in_for_while_it_loads_at_a_size_else_missing() {
        let key = ImageKey::from("p.png");
        for (name, images, expected) in picture_cases(&key) {
            assert_eq!(picture_readiness(&images, &key), expected, "{name}");
        }
    }
}
