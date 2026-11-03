//! A lighter background over the lines of the change auto-follow went
//! to, so that the reader sees why the view moved.

use crate::session::fade::Rgb;
use crate::typeset::style::{Line, Span, Style};
use crate::typeset::wrap;

/// `line` with its background mixed `amount` of the way toward the text
/// colour, across the `width` cells of the text area. The background a span
/// has of its own, such as a code block's, is the one mixed; a span in
/// reverse video is left as it is. `line` as it is where the colours are
/// not known as RGB.
pub fn tint(line: &Line, rgb: &Rgb, amount: f32, width: usize) -> Line {
    let Some(page) = rgb.tinted(None, amount) else {
        return line.clone();
    };
    let spans = |spans: &[Span]| -> Vec<Span> {
        spans
            .iter()
            .map(|span| Span {
                style: tinted(&span.style, rgb, amount),
                ..span.clone()
            })
            .collect()
    };
    let mut line = Line {
        lead: spans(&line.lead),
        spans: spans(&line.spans),
        trail: spans(&line.trail),
        ..line.clone()
    };
    // The cells the line fills: what is repeated on every row, the text,
    // as scaled, an image row and the trail.
    let image = line.image.as_ref().map_or(0, |row| row.columns.len());
    let used =
        wrap::width(&line.lead) + wrap::width(&line.spans) + image + wrap::width(&line.trail);
    if used < width {
        let rest = Style {
            bg: Some(page),
            ..Style::default()
        };
        line.trail.push(Span::new(&" ".repeat(width - used), rest));
    }
    line
}

/// `style` with its background, or the page's where it has none, mixed
/// toward the text colour.
fn tinted(style: &Style, rgb: &Rgb, amount: f32) -> Style {
    if style.reverse {
        return style.clone();
    }
    match rgb.tinted(style.bg, amount) {
        Some(bg) => Style {
            bg: Some(bg),
            ..style.clone()
        },
        None => style.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::color::Color;
    use crate::typeset::size::TextSize;
    use crate::typeset::theme::Theme;

    fn rgb() -> Rgb {
        let mut caps = Capabilities::kitty(80);
        caps.foreground = Some(Color::Rgb(200, 200, 200));
        caps.background = Some(Color::Rgb(0, 0, 0));
        Rgb::new(&Theme::default(), &caps).unwrap()
    }

    #[test]
    fn a_line_is_tinted_across_the_text_area_with_its_scaled_text_counted() {
        let scaled = Style {
            size: TextSize::new(2).unwrap(),
            ..Style::default()
        };
        let line = Line::new(vec![Span::new("Title", scaled)]);
        let tinted = tint(&line, &rgb(), 0.15, 40);
        let grey = Some(Color::Rgb(30, 30, 30));
        assert_eq!(tinted.spans[0].style.bg, grey);
        // Five cells at scale 2 take ten, and the trail the other thirty.
        assert_eq!(tinted.trail.len(), 1);
        assert_eq!(tinted.trail[0].text, " ".repeat(30));
        assert_eq!(tinted.trail[0].style.bg, grey);
    }

    #[test]
    fn an_embed_is_kept_and_its_background_mixed() {
        let line = Line::new(vec![
            Span::new("a", Style::default()),
            Span::image("alt", 3, Style::default()),
        ]);
        let tinted = tint(&line, &rgb(), 0.15, 40);
        let grey = Some(Color::Rgb(30, 30, 30));
        assert!(tinted.spans[1].embed.is_some());
        assert_eq!(tinted.spans[1].text, "alt");
        assert_eq!(tinted.spans[1].style.bg, grey);
        // Its columns count, not its text's.
        assert_eq!(tinted.trail[0].text, " ".repeat(36));
    }

    #[test]
    fn a_background_of_its_own_is_mixed_and_reverse_video_is_left() {
        let own = Style {
            bg: Some(Color::Rgb(100, 100, 100)),
            ..Style::default()
        };
        let reverse = Style {
            reverse: true,
            ..Style::default()
        };
        let line = Line::new(vec![Span::new("a", own), Span::new("b", reverse.clone())]);
        let tinted = tint(&line, &rgb(), 0.5, 2);
        assert_eq!(tinted.spans[0].style.bg, Some(Color::Rgb(150, 150, 150)));
        assert_eq!(tinted.spans[1].style, reverse);
        assert!(tinted.trail.is_empty());
    }

    #[test]
    fn without_known_colours_the_line_is_as_it_was() {
        let caps = Capabilities::kitty(80);
        let unknown = Rgb::new(&Theme::default(), &caps).unwrap();
        let line = Line::new(vec![Span::new("a", Style::default())]);
        assert_eq!(tint(&line, &unknown, 0.15, 10), line);
    }
}
