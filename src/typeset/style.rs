//! Styled text: the unit between layout and output.

use std::ops::Range;
use std::rc::Rc;

use crate::typeset::reading::engine::RowLink;
use crate::typeset::reading::page::Overlay;

use crate::typeset::block::BlockId;
use crate::typeset::color::{Color, Underline};
use crate::typeset::images::ImageKey;
use crate::typeset::images::size::{Fit, Placement};
use crate::typeset::reading::engine::Parts;
use crate::typeset::size::TextSize;
use crate::typeset::theme::Theme;

/// How the wrapper treats spaces in a span, much like CSS `white-space`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Spaces {
    /// Runs collapse to one, and the line can break at any of them.
    #[default]
    Collapse,
    /// Kept as written; the line can break at any of them.
    Keep,
    /// Kept as written; the line breaks at them only when the text they
    /// join is longer than a line.
    KeepTogether,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: Underline,
    /// The underline's own colour; `None` draws it in the text colour.
    pub underline_color: Option<Color>,
    pub strike: bool,
    /// Foreground and background swapped.
    pub reverse: bool,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    /// Target of an OSC 8 hyperlink.
    pub link: Option<Rc<str>>,
    pub spaces: Spaces,
    /// The size the text is drawn at.
    pub size: TextSize,
}

impl Style {
    pub fn fg(color: Option<Color>) -> Self {
        Self {
            fg: color,
            ..Self::default()
        }
    }

    /// The same style without the hyperlink, the wrapping and the size, for
    /// comparing attributes only.
    pub fn attributes(&self) -> Self {
        Self {
            link: None,
            spaces: Spaces::Collapse,
            size: TextSize::NORMAL,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    /// An image drawn in the cells of the span, in place of its text.
    pub embed: Option<Rc<Embed>>,
    /// What of the document's text the span's bytes show; none for text
    /// that the layout makes up.
    pub origins: Origins,
}

/// What of the document's text some bytes of a span show: the bytes `shown`
/// of the span's text show the bytes `doc` of text number `text` of the
/// block that the line's `inner` names.
///
/// When the two ranges are `exact` (as long as each other and shown as they
/// are), byte `i` of one is byte `i` of the other. Otherwise the piece is a
/// whole: what the layout shows in its place, such as a tab spread into
/// spaces or a superscript's own characters, which is the whole of `doc`
/// wherever it is cut. `picture` says that the bytes only stand in the place
/// of the text, as the alt text of an image that is drawn does, and show
/// nothing of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub shown: Range<u32>,
    pub text: u32,
    pub doc: Range<u32>,
    pub exact: bool,
    pub picture: bool,
}

impl Origin {
    /// Bytes that show the bytes `doc` as they are. `shown` is set where the
    /// origin is given to a span.
    pub fn exact(text: u32, doc: Range<u32>) -> Self {
        Self {
            shown: 0..0,
            text,
            doc,
            exact: true,
            picture: false,
        }
    }

    /// Bytes that show the whole of `doc` in another form.
    pub fn whole(text: u32, doc: Range<u32>) -> Self {
        Self {
            exact: false,
            ..Self::exact(text, doc)
        }
    }

    /// Bytes that stand in the place of `doc`, shown as a picture.
    pub fn picture(text: u32, doc: Range<u32>) -> Self {
        Self {
            picture: true,
            ..Self::whole(text, doc)
        }
    }

    /// The part of the origin that `bytes` of the shown text cover, shown
    /// from 0, if any.
    fn part(&self, bytes: &Range<u32>) -> Option<Self> {
        let start = self.shown.start.max(bytes.start);
        let end = self.shown.end.min(bytes.end);
        if start >= end {
            return None;
        }
        let doc = match self.exact {
            true => {
                self.doc.start + (start - self.shown.start)
                    ..self.doc.start + (end - self.shown.start)
            }
            false => self.doc.clone(),
        };
        Some(Self {
            shown: start - bytes.start..end - bytes.start,
            doc,
            ..self.clone()
        })
    }

    /// The document byte that the shown byte `byte` of the span shows, if
    /// it is in this origin: the same offset into `doc` when exact,
    /// otherwise the start of `doc`.
    pub fn at(&self, byte: u32) -> Option<u32> {
        self.shown.contains(&byte).then(|| match self.exact {
            true => self.doc.start + (byte - self.shown.start),
            false => self.doc.start,
        })
    }
}

/// The origins of a span, in the order of its bytes. Two spans that draw
/// alike are equal whatever they show of the document, as for a `Model`'s
/// source map.
#[derive(Debug, Clone, Default)]
pub struct Origins(Vec<Origin>);

impl PartialEq for Origins {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Origins {}

impl Origins {
    pub fn iter(&self) -> std::slice::Iter<'_, Origin> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The origin of the shown byte `byte`, if the text shows any of the
    /// document there.
    pub fn of(&self, byte: usize) -> Option<&Origin> {
        let byte = u32::try_from(byte).ok()?;
        self.0.iter().find(|origin| origin.shown.contains(&byte))
    }

    /// Adds `origin`, which comes after the others, and joins it to the
    /// last one when it goes on from it as the same bytes of the same text.
    fn push(&mut self, origin: Origin) {
        if let Some(last) = self.0.last_mut()
            && last.exact
            && origin.exact
            && !(last.picture || origin.picture)
            && last.text == origin.text
            && last.shown.end == origin.shown.start
            && last.doc.end == origin.doc.start
        {
            last.shown.end = origin.shown.end;
            last.doc.end = origin.doc.end;
            return;
        }
        self.0.push(origin);
    }
}

impl<'a> IntoIterator for &'a Origins {
    type Item = &'a Origin;
    type IntoIter = std::slice::Iter<'a, Origin>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// An image drawn in the cells of a span, as an inline image or formula is
/// in running text: as wide as `placement.size.columns`, whatever the
/// span's text says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Embed {
    pub placement: Rc<Placement>,
    /// The image row on the line's first row; the line's next rows show the
    /// next ones.
    pub row: u16,
    /// How many of the image's rows the line shows: all of the line's rows,
    /// or 1 on each line of a split.
    pub rows: u16,
    /// How the image was fitted into the line, which fits it again when the
    /// line is shaped anew: its opaque rows, baseline and rows.
    pub fit: Fit,
    /// Whether the image is a formula, whose baseline lies on the text's,
    /// and not a picture, whose opaque bottom lies on the bottom of the
    /// text's cell unless it is tall.
    pub formula: bool,
    /// The rows of the canvas that the image itself is on, as far as its
    /// opaque rows reach; the canvas covers the line's rows, which are
    /// more.
    pub ink_rows: std::ops::Range<u16>,
}

impl Embed {
    /// An embed of `placement` showing `rows` rows of it from `row`, whose
    /// image is wholly opaque and has its baseline at its bottom.
    pub fn new(placement: Rc<Placement>, row: u16, rows: u16) -> Self {
        let size = placement.size;
        let fit = Fit {
            width: size.width,
            height: size.height,
            opaque: 0..size.height,
            columns: size.columns,
            rows: size.rows,
            baseline: size.height,
        };
        Self {
            placement,
            row,
            rows,
            fit,
            formula: false,
            ink_rows: 0..size.rows,
        }
    }
}

impl Span {
    /// Control characters are replaced, so a document cannot smuggle escape
    /// sequences to the terminal. Newlines stay: they mark hard line breaks
    /// for the wrapper, which consumes them.
    pub fn new(text: &str, style: Style) -> Self {
        let text = text
            .chars()
            .map(|c| match c {
                '\n' => '\n',
                '\t' => ' ',
                c if c.is_control() => char::REPLACEMENT_CHARACTER,
                c => c,
            })
            .collect();
        Self {
            text,
            style,
            embed: None,
            origins: Origins::default(),
        }
    }

    /// The span, as showing `origin` of the document: all of its text,
    /// which `origin` says what of the document it is.
    #[must_use]
    pub fn showing(mut self, mut origin: Origin) -> Self {
        let len = u32::try_from(self.text.len()).unwrap_or(u32::MAX);
        origin.shown = 0..len;
        // Text that was changed in the making of the span is not as long as
        // what it shows.
        origin.exact &= usize::try_from(origin.doc.end - origin.doc.start) == Ok(self.text.len());
        self.origins = Origins::default();
        if len > 0 {
            self.origins.push(origin);
        }
        self
    }

    /// The bytes `bytes` of the span, with what they show of the document.
    pub fn part(&self, bytes: Range<usize>) -> Span {
        let range = u32::try_from(bytes.start).unwrap_or(u32::MAX)
            ..u32::try_from(bytes.end).unwrap_or(u32::MAX);
        let mut origins = Origins::default();
        for origin in self.origins.iter().filter_map(|origin| origin.part(&range)) {
            origins.push(origin);
        }
        Span {
            text: self.text[bytes].to_owned(),
            style: self.style.clone(),
            embed: self.embed.clone(),
            origins,
        }
    }

    /// Adds `other`'s text, and what it shows of the document, after this
    /// span's.
    fn join(&mut self, other: Span) {
        let at = u32::try_from(self.text.len()).unwrap_or(u32::MAX);
        self.text.push_str(&other.text);
        for mut origin in other.origins.0 {
            origin.shown = origin.shown.start + at..origin.shown.end + at;
            self.origins.push(origin);
        }
    }

    /// A span that is the cells of the image `embed`; `text` is what a
    /// search finds in it, sanitised as `new` does.
    pub fn embed(text: &str, style: Style, embed: Embed) -> Self {
        Self {
            embed: Some(Rc::new(embed)),
            ..Self::new(text, style)
        }
    }
}

#[cfg(test)]
impl Span {
    /// A span that is the cells of an image `columns` wide and one row
    /// high, with `text` for a search to find, in `style`.
    pub(crate) fn image(text: &str, columns: u16, style: Style) -> Span {
        use crate::typeset::images::size::Shows;
        let size = crate::typeset::images::size::Size {
            width: u32::from(columns) * 10,
            height: 20,
            columns,
            rows: 1,
        };
        let placement = Placement::block(ImageKey::from("e.png"), size, Shows::Image);
        let embed = Embed::new(Rc::new(placement), 0, 1);
        Span::embed(text, style, embed)
    }
}

/// One row of an image shown as a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRow {
    pub placement: Rc<Placement>,
    /// Counted from 0 at the top of the image.
    pub row: u16,
    /// The background of the block it lies in, such as an alert's tint,
    /// which shows through where the image is transparent; `None` for the
    /// page's.
    pub background: Option<Color>,
    /// The columns of the image the row shows: all of them, or a part,
    /// as a line of a heading too long for one line shows its words.
    pub columns: Range<u16>,
    /// The text the line of the image shows, such as a heading's words,
    /// which a search finds on its first row; `None` for every other
    /// image.
    pub text: Option<String>,
    /// What the row shows of the document, such as the alt text of an image
    /// or the words of a heading, as a picture; `shown` means nothing here.
    /// `None` for an image that shows none of it, and for a row of a page.
    pub origin: Option<Origin>,
    /// The page of reading mode that the image is a part of, which draws
    /// it; `None` for an image from `Images`.
    pub page: Option<Rc<Parts>>,
    /// What is drawn over the part of the page, such as a search's marks,
    /// when the key says so.
    pub overlay: Option<Rc<Overlay>>,
    /// The hyperlinks over the cells of the row, as the page has them.
    pub links: Vec<RowLink>,
}

/// What a line says about a block's image that is loading: which image,
/// how far above the line it is shown, and the band drawn over the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loading {
    /// The image loading.
    pub key: ImageKey,
    /// How many lines above this one show the image, or what stands in
    /// for it: 0 when this line is the only one.
    pub above: usize,
    /// The band that tells it, which the line draws over itself so that it
    /// takes no row of its own.
    pub band: Option<Band>,
}

/// How the band of a loading image is drawn over a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Band {
    /// How many cells wide it is.
    pub columns: u16,
    /// How many cells from the start of the line it starts.
    pub indent: usize,
    /// Over the bottom of the line, as on the last row of a block with no
    /// blank line after it; otherwise a little below the line's top, as on
    /// the blank line after a block.
    pub low: bool,
}

/// One output line. Text of a size other than the normal one is drawn with
/// kitty's text sizing protocol, and the line occupies as many terminal rows
/// as the largest scale of its spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Normal-size cells repeated at the start of every row the line
    /// occupies, so an indent keeps its colour beside scaled text.
    pub lead: Vec<Span>,
    /// Which spans of `lead` are for the first row only, such as a list's
    /// marker, and are blank below it; spans past its end are for every
    /// row.
    pub lead_once: Vec<bool>,
    /// For the first line of a split (an image or formula taller than its
    /// text's row, one line for each row): how many lines below it the
    /// text row is, where a list's marker goes.
    pub marker_row: u16,
    pub spans: Vec<Span>,
    /// A row of an image after the spans, which then hold only what goes
    /// before it, such as a quote bar or a list marker.
    pub image: Option<ImageRow>,
    /// Normal-size cells after the text on every row the line occupies, so
    /// a background reaches the edge when the scaled text cannot.
    pub trail: Vec<Span>,
    /// The block the line comes from, or none for the blank line between
    /// two blocks.
    pub block: Option<BlockId>,
    /// The innermost block that the line shows the text of: a paragraph
    /// in a list in a quote, where `block` is the quote. The blocks of the
    /// same table as `block`'s.
    pub inner: Option<BlockId>,
    /// The image of a block above that is loading, and the band drawn over
    /// the line to tell it.
    pub loading: Option<Loading>,
}

impl Line {
    pub fn new(spans: Vec<Span>) -> Self {
        Self {
            lead: Vec::new(),
            lead_once: Vec::new(),
            marker_row: 0,
            spans,
            image: None,
            trail: Vec::new(),
            block: None,
            inner: None,
            loading: None,
        }
    }

    pub fn blank() -> Self {
        Self::new(Vec::new())
    }

    /// A line of one image row.
    pub fn image(placement: Rc<Placement>, row: u16) -> Self {
        let columns = 0..placement.size.columns;
        Self {
            image: Some(ImageRow {
                placement,
                row,
                background: None,
                columns,
                text: None,
                origin: None,
                page: None,
                overlay: None,
                links: Vec::new(),
            }),
            ..Self::blank()
        }
    }

    /// Nothing to show: no text and no image.
    pub fn is_blank(&self) -> bool {
        self.spans.is_empty() && self.image.is_none()
    }

    pub fn prepend(&mut self, span: Span) {
        self.spans.insert(0, span);
    }

    /// Puts `span` at the start of every row of the line: in `lead` when
    /// the line has more than one row, as a quote's bar or a list's indent
    /// repeats beside it, and otherwise as `prepend` does.
    pub fn indent(&mut self, span: Span) {
        match self.rows() > 1 {
            true => self.lead_insert(span, false),
            false => self.spans.insert(0, span),
        }
    }

    /// Puts `span`, a list's marker, at the start of the first row of the
    /// line, before what `indent` put there: in `lead` when the line has
    /// more than one row, where the rows below show it blank, and otherwise
    /// as `prepend` does.
    pub fn marker(&mut self, span: Span) {
        match self.rows() > 1 {
            true => self.lead_insert(span, true),
            false => self.spans.insert(0, span),
        }
    }

    /// Puts `span` first in `lead`, for the first row only if `once`.
    pub fn lead_insert(&mut self, span: Span, once: bool) {
        // The flags are kept only where one is set.
        if once || !self.lead_once.is_empty() {
            self.lead_once.resize(self.lead.len(), false);
            self.lead_once.insert(0, once);
        }
        self.lead.insert(0, span);
    }

    /// The rows the line occupies: as many as the largest scale of its
    /// text spans or the most rows of its embeds, and at least one.
    pub fn rows(&self) -> u8 {
        (self.spans.iter())
            .map(|span| match &span.embed {
                Some(embed) => u8::try_from(embed.rows).unwrap_or(u8::MAX),
                None => span.style.size.scale(),
            })
            .fold(1, u8::max)
    }

    /// Draws the whole line at the normal size.
    pub fn to_normal_size(&mut self) {
        for span in &mut self.spans {
            span.style.size = TextSize::NORMAL;
        }
    }
}

/// Text in `style` played down: in `theme`'s muted colour, or dimmed when
/// the theme has none.
pub fn muted(theme: &Theme, style: Style) -> Style {
    muted_in(theme.muted, style)
}

/// Text in `style` played down: in `muted`, or dimmed when there is none.
pub fn muted_in(muted: Option<Color>, style: Style) -> Style {
    match muted {
        Some(color) => Style {
            fg: Some(color),
            ..style
        },
        None => Style { dim: true, ..style },
    }
}

/// The text of `spans` one after another.
pub fn text(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// The `Rc` for a link to `url` that comes right after `spans`. When the
/// last span is a link to `url` too, as in `[a](u)**[b](u)**`, it is that
/// span's `Rc`: the pager joins a link across lines by its `Rc`
/// (`text_links`), so the two stay one link where a line breaks between
/// them, as the terminal shows them as one. Otherwise it is a new `Rc`.
pub fn link_to(spans: &[Span], url: &str) -> Rc<str> {
    let before = spans.last().and_then(|span| span.style.link.clone());
    before
        .filter(|before| **before == *url)
        .unwrap_or_else(|| Rc::from(url))
}

/// Appends text that the layout makes up, merging it into the last span
/// when the style matches. It shows nothing of the document.
pub fn push(spans: &mut Vec<Span>, text: &str, style: &Style) {
    if text.is_empty() {
        return;
    }
    push_span(spans, Span::new(text, style.clone()));
}

/// Appends `text`, which shows what `origin` says of the document, merging
/// it into the last span when the style matches.
pub fn push_at(spans: &mut Vec<Span>, text: &str, style: &Style, origin: Origin) {
    if text.is_empty() {
        return;
    }
    push_span(spans, Span::new(text, style.clone()).showing(origin));
}

/// Appends `span`, merging it into the last span when it is text in the same
/// style, and moving what it shows of the document along.
pub fn push_span(spans: &mut Vec<Span>, span: Span) {
    match spans.last_mut() {
        Some(last) if last.embed.is_none() && span.embed.is_none() && last.style == span.style => {
            last.join(span);
        }
        _ => spans.push(span),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::images::size::{Shows, Size};

    /// A span of an image of `rows` rows that a line shows `shown` rows of,
    /// from its row `row`.
    fn image_span(row: u16, shown: u16, rows: u16) -> Span {
        let size = Size {
            width: 10,
            height: 20 * u32::from(rows),
            columns: 1,
            rows,
        };
        let placement = Placement::block(ImageKey::from("e.png"), size, Shows::Image);
        let embed = Embed::new(Rc::new(placement), row, shown);
        Span::embed("", Style::default(), embed)
    }

    #[test]
    fn an_indent_repeats_on_every_row_of_a_line_of_several_and_prepends_otherwise() {
        let bar = || Span::new("│ ", Style::default());
        let mut one = Line::new(vec![Span::new("a", Style::default())]);
        one.indent(bar());
        assert_eq!((one.lead.len(), one.spans.len()), (0, 2));
        let mut two = Line::new(vec![Span::new("a", Style::default()), image_span(0, 2, 2)]);
        two.indent(bar());
        assert_eq!((two.lead.len(), two.spans.len()), (1, 2));
        two.indent(Span::new("> ", Style::default()));
        assert_eq!(two.lead[0].text, "> ");
        // A marker goes where `prepend` puts it, on the first row only.
        two.prepend(Span::new("- ", Style::default()));
        assert_eq!(two.spans[0].text, "- ");
    }

    #[test]
    fn a_marker_on_a_line_of_several_rows_is_first_row_only_and_stays_before_the_bar() {
        let span = |text: &str| Span::new(text, Style::default());
        let mut line = Line::new(vec![span("a"), image_span(0, 2, 2)]);
        line.indent(span("│ "));
        line.marker(span("• "));
        // A margin put before them all shifts nothing.
        line.lead_insert(span("  "), false);
        let lead: Vec<&str> = line.lead.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(lead, ["  ", "• ", "│ "]);
        assert_eq!(line.lead_once, [false, true, false]);
        // On a line of one row it is in the spans, as `prepend` has it.
        let mut one = Line::new(vec![span("a")]);
        one.marker(span("• "));
        assert_eq!((one.lead.len(), one.spans[0].text.as_str()), (0, "• "));
    }

    #[test]
    fn a_line_has_as_many_rows_as_its_tallest_text_or_embed() {
        assert_eq!(Line::new(vec![Span::new("a", Style::default())]).rows(), 1);
        // One row of a taller image on a line of a split.
        assert_eq!(Line::new(vec![image_span(2, 1, 4)]).rows(), 1);
        // A line of two rows shows two of its image's.
        let line = Line::new(vec![Span::new("a", Style::default()), image_span(0, 2, 2)]);
        assert_eq!(line.rows(), 2);
        let scaled = Style {
            size: TextSize::new(3).unwrap(),
            ..Style::default()
        };
        assert_eq!(
            Line::new(vec![Span::new("a", scaled), image_span(0, 2, 2)]).rows(),
            3
        );
    }

    /// The bytes of `spans` that show the document, as pairs of the shown
    /// byte and the document byte it shows.
    fn shown(spans: &[Span]) -> Vec<(usize, u32)> {
        let mut at = 0;
        let mut shown = Vec::new();
        for span in spans {
            for byte in 0..span.text.len() {
                if let Some(doc) = span.origins.of(byte).and_then(|o| o.at(byte as u32)) {
                    shown.push((at + byte, doc));
                }
            }
            at += span.text.len();
        }
        shown
    }

    #[test]
    fn text_pushed_with_its_origin_merges_and_moves_the_origins_along() {
        let style = Style::default();
        let mut spans = Vec::new();
        push_at(&mut spans, "ab", &style, Origin::exact(0, 4..6));
        push(&mut spans, "-", &style);
        push_at(&mut spans, "cd", &style, Origin::exact(1, 0..2));
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "ab-cd");
        let origins: Vec<_> = spans[0]
            .origins
            .iter()
            .map(|o| (o.shown.clone(), o.text, o.doc.clone()))
            .collect();
        assert_eq!(origins, [(0..2, 0, 4..6), (3..5, 1, 0..2)]);
        // What the layout makes up has none.
        assert_eq!(shown(&spans), [(0, 4), (1, 5), (3, 0), (4, 1)]);
        let mut made = Vec::new();
        push(&mut made, "x", &style);
        assert!(made[0].origins.is_empty());
    }

    #[test]
    fn text_that_goes_on_from_the_text_before_it_is_one_origin() {
        let style = Style::default();
        let mut spans = Vec::new();
        push_at(&mut spans, "ab", &style, Origin::exact(0, 4..6));
        push_at(&mut spans, "cd", &style, Origin::exact(0, 6..8));
        assert_eq!(spans[0].origins.iter().count(), 1);
        assert_eq!(shown(&spans), [(0, 4), (1, 5), (2, 6), (3, 7)]);
    }

    #[test]
    fn a_part_of_a_span_has_the_part_of_what_it_shows() {
        let mut spans = Vec::new();
        push_at(
            &mut spans,
            "abcd",
            &Style::default(),
            Origin::exact(2, 10..14),
        );
        push_at(
            &mut spans,
            "[img]",
            &Style::default(),
            Origin::whole(3, 0..9),
        );
        let part = spans[0].part(2..7);
        assert_eq!(part.text, "cd[im");
        // "cd" are the bytes 12 and 13, and the whole stays the whole.
        assert_eq!(
            shown(std::slice::from_ref(&part)),
            [(0, 12), (1, 13), (2, 0), (3, 0), (4, 0)]
        );
        let doc: Vec<_> = part
            .origins
            .iter()
            .map(|o| (o.text, o.doc.clone()))
            .collect();
        assert_eq!(doc, [(2, 12..14), (3, 0..9)]);
    }

    #[test]
    fn a_whole_stays_whole_when_cut_to_the_length_of_its_text() {
        let mut spans = Vec::new();
        push_at(
            &mut spans,
            "    ",
            &Style::default(),
            Origin::whole(0, 5..6),
        );
        let right = spans[0].part(1..3);
        assert_eq!(shown(&[right]), [(0, 5), (1, 5)]);
        let mut spans = Vec::new();
        push_at(
            &mut spans,
            "abcdef",
            &Style::default(),
            Origin::whole(0, 5..8),
        );
        let cut = spans[0].part(0..3);
        let origin = cut.origins.iter().next().expect("an origin");
        assert_eq!((origin.doc.clone(), origin.exact), (5..8, false));
        assert_eq!(origin.at(2), Some(5));
    }

    #[test]
    fn a_control_character_makes_what_a_span_shows_a_whole() {
        let mut spans = Vec::new();
        // The character is replaced by one of three bytes.
        push_at(
            &mut spans,
            "a\u{1b}b",
            &Style::default(),
            Origin::exact(0, 0..3),
        );
        assert_eq!(spans[0].text, "a\u{fffd}b");
        let origin = spans[0].origins.iter().next().expect("an origin");
        assert!(!origin.exact);
        assert_eq!(origin.doc, 0..3);
        // A tab is a space of the same length, and stays exact.
        let mut tab = Vec::new();
        push_at(&mut tab, "a\tb", &Style::default(), Origin::exact(0, 0..3));
        assert!(tab[0].origins.iter().next().expect("an origin").exact);
    }

    #[test]
    fn two_spans_that_draw_alike_are_equal_whatever_they_show_of_the_document() {
        let mut shown = Vec::new();
        push_at(&mut shown, "ab", &Style::default(), Origin::exact(0, 4..6));
        assert_eq!(shown, vec![Span::new("ab", Style::default())]);
    }
}
