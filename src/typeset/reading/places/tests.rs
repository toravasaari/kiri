use std::rc::Rc;

use unicode_segmentation::UnicodeSegmentation;

use super::*;
use crate::render::{FrontMatterStyle, Inputs};
use crate::typeset::caps::Capabilities;
use crate::typeset::driver::{Content, Stream, content};
use crate::typeset::model::{Block, Document};
use crate::typeset::parse::parse;
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page::{Shape, Wide};
use crate::typeset::reading::text::Paragraph;
use crate::typeset::text::DocumentText;

/// Inputs for reading mode, in a window `width` columns wide, with cells
/// of 10 × 20 pixels, the font the tests of reading mode use, and code
/// and tables wider than the area as `wide` says.
fn inputs(width: u16, wide: Wide, front_matter: FrontMatterStyle) -> Inputs {
    let caps = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(width)
    };
    Inputs {
        reading: Some(Reading {
            pictures: Default::default(),
            glyphs: Default::default(),
            fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide,
            snap: false,
            zoom: 100,
        }),
        front_matter,
        caps,
        ..Inputs::new(Capabilities::kitty(width))
    }
}

fn lay_out(source: &str, inputs: &Inputs) -> (Document, Stream) {
    let document = parse(source);
    let stream = content(Content::Document(&document), inputs);
    (document, stream)
}

/// The pages of `stream`, each once.
fn pages(stream: &Stream) -> Vec<Rc<Parts>> {
    let mut pages: Vec<Rc<Parts>> = Vec::new();
    for parts in stream
        .lines
        .iter()
        .filter_map(|line| line.image.as_ref()?.page.as_ref())
    {
        if !pages.iter().any(|page| Rc::ptr_eq(page, parts)) {
            pages.push(Rc::clone(parts));
        }
    }
    pages
}

/// The block in the table that what `at` says is on the page is in.
fn id_of(parts: &Parts, at: &ItemAt) -> BlockId {
    parts.block_of(at)
}

/// A text on a page: where it is, and its paragraph.
struct Text<'a> {
    block: BlockId,
    number: u32,
    paragraph: &'a Paragraph,
}

/// The texts on `parts`, those on the page and those in its bands.
fn texts(parts: &Parts) -> Vec<Text<'_>> {
    let items =
        (parts.page.items.iter()).chain(parts.page.bands.iter().flat_map(|band| &band.items));
    let mut found = Vec::new();
    for item in items {
        let (Shape::Text { paragraph, .. }, Some(at)) = (&item.shape, &item.at) else {
            continue;
        };
        found.push(Text {
            block: id_of(parts, at),
            number: at.text,
            paragraph,
        });
    }
    found
}

/// The fixtures and examples laid out in reading mode, at two widths,
/// both ways with code and tables wider than the area, and with front
/// matter shown in each way.
fn each_page(mut check: impl FnMut(&str, &Document, &Stream, FrontMatterStyle)) {
    for path in crate::testing::documents() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let source = std::fs::read_to_string(&path).unwrap();
        for width in [200, 60] {
            for wide in [Wide::Wrap, Wide::Scroll] {
                for style in [
                    FrontMatterStyle::Table,
                    FrontMatterStyle::Hide,
                    FrontMatterStyle::Code,
                ] {
                    let (document, stream) = lay_out(&source, &inputs(width, wide, style));
                    check(&format!("{name} at {width}"), &document, &stream, style);
                }
            }
        }
    }
}

/// The block of a row as a plain scan of every block says it: the block
/// the row's middle is in, else the block with no row's middle in it
/// whose own middle is in the row. What `row_blocks` does in one pass.
fn block_of_row_by_scanning(parts: &Parts, ids: &[Ids], row: u16) -> Option<usize> {
    let height = f32::from(parts.cell.1);
    let middle = (f32::from(row) * height + f32::from(row + 1) * height) / 2.0;
    let blocks = || parts.page.blocks.iter().zip(ids).enumerate();
    let by_middle = blocks().find(|(_, (placed, _))| placed.rows.contains(&middle));
    let own = || {
        blocks().find(|(_, (placed, _))| {
            let (first, end) = covered_rows(&placed.rows, height);
            end <= first
                && placed.rows.start < placed.rows.end
                && ((placed.rows.start + placed.rows.end) / 2.0 / height).floor() as u16 == row
        })
    };
    by_middle.or_else(own).map(|(index, _)| index)
}

/// The links over a row as a plain scan of every link of the page says
/// it: what `Parts::row_links` did before the index.
fn row_links_by_scanning(
    parts: &Parts,
    address: usize,
    band: Option<(u32, u32)>,
    row: u16,
) -> Vec<crate::typeset::reading::engine::RowLink> {
    use crate::typeset::reading::engine::RowLink;
    let (cell_width, cell_height) = (f32::from(parts.cell.0), f32::from(parts.cell.1));
    let middle = (f32::from(row) + 0.5) * cell_height;
    let shown = band.and_then(|(number, offset)| {
        let band = parts.page.bands.get(number as usize)?;
        Some((number as usize, band.x, band.visible, offset as f32))
    });
    let columns = match band {
        Some(_) => parts.columns,
        None => parts.columns_of(u32::from(row / crate::typeset::reading::engine::PART_ROWS)),
    };
    let mut claims: Vec<(u16, bool, usize)> = Vec::new();
    for (number, link) in parts.page.links().iter().enumerate() {
        if link.band != shown.map(|(band, ..)| band) {
            continue;
        }
        for piece in &link.pieces {
            if !(piece.y..piece.y + piece.height).contains(&middle) {
                continue;
            }
            let (mut left, mut right) = (piece.x, piece.x + piece.width);
            if let Some((_, x, visible, offset)) = shown {
                left = (x + left - offset).max(x);
                right = (x + right - offset).min(x + visible);
            }
            if right <= left {
                continue;
            }
            let first = (left / cell_width - 0.5).ceil().max(0.0) as u16;
            let last = (right / cell_width - 0.5).ceil().max(0.0) as u16;
            let over: Vec<u16> = (first..last).collect();
            if over.is_empty() {
                let at = ((left + right) / 2.0 / cell_width).floor() as u16;
                claims.push((at, false, number));
            }
            claims.extend(over.into_iter().map(|at| (at, true, number)));
        }
    }
    let mut owners: Vec<(u16, usize)> = Vec::new();
    claims.sort_by_key(|&(at, over, number)| (at, !over, number));
    for (at, _, number) in claims {
        if at < columns && owners.last().is_none_or(|&(last, _)| last != at) {
            owners.push((at, number));
        }
    }
    let mut found: Vec<RowLink> = Vec::new();
    for (at, number) in owners {
        match found.last_mut() {
            Some(last) if last.link.1 == number && last.columns.end == at => {
                last.columns.end = at + 1;
            }
            _ => found.push(RowLink {
                columns: at..at + 1,
                url: Rc::clone(&parts.page.links()[number].url),
                link: (address, number),
            }),
        }
    }
    found
}

/// Checks the first `limit` rows of `parts`, in the page and in each
/// band and one number past them, which is none, against the scan; how
/// many of the answers had links.
fn the_index_gives_what_the_scan_gives(parts: &Parts, name: &str, limit: u16) -> usize {
    // `row_links` uses the address only as a name.
    let address = parts as *const Parts as usize;
    let mut with_links = 0;
    let bands = parts.page.bands.len() as u32;
    let rows = parts.rows.min(limit);
    for row in 0..rows {
        let asked = [(None, row)]
            .into_iter()
            .chain((0..=bands).map(|band| (Some((band, 7)), row)));
        for (band, row) in asked {
            let found = parts.row_links(address, band, row);
            let scanned = row_links_by_scanning(parts, address, band, row);
            assert_eq!(found, scanned, "{name}: band {band:?}, row {row}");
            with_links += usize::from(!found.is_empty());
        }
    }
    with_links
}

#[test]
fn the_links_of_every_row_of_a_page_are_the_ones_a_scan_of_all_links_finds() {
    let mut with_links = 0;
    each_page(|name, _, stream, _| {
        for parts in pages(stream) {
            with_links += the_index_gives_what_the_scan_gives(&parts, name, 400);
        }
    });
    assert!(with_links > 100, "{with_links} rows with links");
}

#[test]
fn the_links_of_rows_of_17_pixels_are_the_ones_a_scan_finds_too() {
    let mut with_links = 0;
    for path in crate::testing::documents() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let source = std::fs::read_to_string(&path).unwrap();
        let mut inputs = inputs(100, Wide::Scroll, FrontMatterStyle::Table);
        inputs.caps.cell_pixels = Some((9, 17));
        let (_, stream) = lay_out(&source, &inputs);
        for parts in pages(&stream) {
            with_links += the_index_gives_what_the_scan_gives(&parts, &name, 400);
        }
    }
    assert!(with_links > 50, "{with_links} rows with links");
}

#[test]
fn the_links_in_a_scrolling_band_are_the_ones_a_scan_finds() {
    let words = "word ".repeat(120);
    let long = "x".repeat(900);
    let source = format!(
        "| a | b |\n|---|---|\n| {words}[one](https://e.example/1) {words}[two](https://e.example/2) | [{long}](https://e.example/3) |\n"
    );
    let mut in_bands = 0;
    let mut with_links = 0;
    for width in [30, 60, 100] {
        let (_, stream) = lay_out(
            &source,
            &inputs(width, Wide::Scroll, FrontMatterStyle::Table),
        );
        for parts in pages(&stream) {
            in_bands += parts
                .links()
                .iter()
                .filter(|link| link.band.is_some())
                .count();
            with_links += the_index_gives_what_the_scan_gives(&parts, "table", 400);
        }
    }
    assert!(in_bands > 0, "no link in a band");
    assert!(with_links > 0, "no row with links");
}

#[test]
fn a_row_has_few_candidate_pieces_however_many_links_the_page_has() {
    let source: String = (0..2000)
        .map(|number| format!("Line [{number}](https://e.example/{number}) of text.\n\n"))
        .collect();
    let (_, stream) = lay_out(&source, &inputs(80, Wide::Wrap, FrontMatterStyle::Table));
    let pages = pages(&stream);
    let mut candidates = 0;
    // A page is cut at its row budget, so 2000 links are on several
    // pages of some hundreds.
    for parts in &pages {
        assert!(parts.links().len() > 400, "{} links", parts.links().len());
        for row in 0..parts.rows {
            let here = parts.link_candidates(None, row).len();
            assert!(here <= 2, "{here} candidates on row {row}");
            candidates += here;
        }
    }
    assert!(candidates >= 2000, "{candidates} candidates");
}

#[test]
fn the_block_of_every_row_of_a_page_is_the_one_a_scan_of_all_blocks_finds() {
    let (mut rows_seen, mut between, mut small) = (0, 0, 0);
    each_page(|name, _, stream, _| {
        for parts in pages(stream) {
            let rows = stream
                .lines
                .iter()
                .filter_map(|line| line.image.as_ref()?.page.as_ref())
                .filter(|page| Rc::ptr_eq(page, &parts))
                .count() as u16;
            let found = row_blocks(&parts, &parts.ids, rows);
            assert_eq!(found.len(), usize::from(rows), "{name}");
            for row in 0..rows {
                let scanned = block_of_row_by_scanning(&parts, &parts.ids, row);
                assert_eq!(found[usize::from(row)], scanned, "{name}: row {row}");
                rows_seen += 1;
                between += usize::from(scanned.is_none());
                small += usize::from(scanned.is_some_and(|index| {
                    let placed = &parts.page.blocks[index].rows;
                    let (first, end) = covered_rows(placed, f32::from(parts.cell.1));
                    end <= first
                }));
            }
        }
    });
    assert!(rows_seen > 1000, "{rows_seen} rows");
    assert!(between > 100, "{between} rows between blocks");
    assert!(small > 0, "no block without a row of its own");
}

#[test]
fn what_a_text_on_a_page_shows_is_what_the_text_it_names_says() {
    let (mut exact, mut others, mut paragraphs) = (0, 0, 0);
    each_page(|name, document, stream, _| {
        let text = DocumentText::new([document]);
        for parts in pages(stream) {
            for shown in texts(&parts) {
                paragraphs += 1;
                let path = stream.blocks.path(shown.block);
                let pieces = text.texts(0, path);
                let Some(piece) = pieces.get(shown.number as usize) else {
                    panic!("{name}: {path:?} has no text {}", shown.number);
                };
                let on_page = shown.paragraph.text();
                for origin in shown.paragraph.from() {
                    let at = format!("{name}: {path:?} text {}: {origin:?}", shown.number);
                    assert!(origin.shown.end as usize <= on_page.len(), "{at}");
                    assert!(origin.doc.end as usize <= piece.text.len(), "{at}");
                    let shown = &on_page[origin.shown.start as usize..origin.shown.end as usize];
                    let doc = &piece.text[origin.doc.start as usize..origin.doc.end as usize];
                    if origin.exact {
                        exact += 1;
                        assert_eq!(shown, doc, "{at}");
                    } else {
                        others += 1;
                    }
                }
            }
        }
    });
    assert!(
        paragraphs > 1000 && exact > 1000,
        "{paragraphs} {exact} {others}"
    );
}

/// What a layout may leave out of the text of the document in reading
/// mode, as the cell layout's list does (`layout/inlines.rs`).
const LEFT_OUT: &[&str] = &[
    "the text of hidden front matter",
    "the keys and values of front matter shown as code",
];

#[test]
fn every_byte_of_a_text_on_a_page_is_shown_but_what_the_layout_names_it_leaves_out() {
    let mut left_out = [0usize; 2];
    let mut shown = 0;
    each_page(|name, document, stream, style| {
        let text = DocumentText::new([document]);
        let blocks = document.blocks();
        // For each text of each block: whether each byte is shown, and
        // whether the page shows it.
        let mut seen: Vec<(Vec<usize>, usize, Vec<bool>)> = Vec::new();
        for id in stream.blocks.ids() {
            let path = stream.blocks.path(id).to_vec();
            for (number, piece) in text.texts(0, &path).to_vec().iter().enumerate() {
                seen.push((path.clone(), number, vec![false; piece.text.len()]));
            }
        }
        // A layout that shows an origin of a text the model does not
        // have numbers its texts by itself, and the two will drift apart.
        let mut mark = |path: &[usize], number: usize, bytes: Range<u32>| {
            let Some(entry) = (seen.iter_mut()).find(|(at, n, ..)| at == path && *n == number)
            else {
                panic!("{name}: {path:?} shows text {number}, which the model does not have");
            };
            entry.2[bytes.start as usize..bytes.end as usize].fill(true);
        };
        for parts in pages(stream) {
            for text in texts(&parts) {
                let path = stream.blocks.path(text.block);
                for origin in text.paragraph.from() {
                    mark(path, text.number as usize, origin.doc.clone());
                }
                for number in 0..text.paragraph.pictures.len() {
                    if let Some(origin) = text.paragraph.picture_from(number) {
                        mark(path, text.number as usize, origin.doc.clone());
                    }
                }
            }
            for item in (parts.page.items.iter())
                .chain(parts.page.bands.iter().flat_map(|band| &band.items))
            {
                if let Some(at) = &item.at
                    && let Some(doc) = &at.picture
                {
                    let id = id_of(&parts, at);
                    mark(stream.blocks.path(id), at.text as usize, doc.clone());
                }
            }
        }
        // What the cell layout shows, in the blocks that reading mode
        // leaves to it.
        for line in &stream.lines {
            let Some(inner) = line.inner else { continue };
            let origins = (line.spans.iter()).flat_map(|span| span.origins.iter().cloned());
            let image = line.image.iter().filter_map(|row| row.origin.clone());
            for origin in origins.chain(image) {
                mark(stream.blocks.path(inner), origin.text as usize, origin.doc);
            }
        }
        for (path, number, flags) in seen {
            let piece = text.texts(0, &path).get(number).unwrap();
            let front_matter = blocks
                .iter()
                .any(|(at, block)| *at == path && matches!(block, Block::FrontMatter(_)));
            for (at, c) in piece.text.char_indices() {
                if flags[at..at + c.len_utf8()]
                    .iter()
                    .all(|shown_byte| *shown_byte)
                {
                    shown += 1;
                    continue;
                }
                let why = match (front_matter, style) {
                    (true, FrontMatterStyle::Hide) => 0,
                    (true, FrontMatterStyle::Code) => 1,
                    _ => panic!(
                        "{name}: {path:?} text {number}: {c:?} at {at} of {:?} is shown by nothing",
                        piece.text
                    ),
                };
                left_out[why] += 1;
            }
        }
    });
    for (count, what) in left_out.iter().zip(LEFT_OUT) {
        assert!(*count > 0, "nothing is left out as {what}");
    }
    assert!(shown > 20_000, "{shown}");
}

/// The paths of the innermost blocks of the rows of `source`, in order,
/// each once, and the path of each of their blocks.
fn inner_blocks(source: &str) -> Vec<Vec<usize>> {
    let (_, stream) = lay_out(source, &inputs(80, Wide::Wrap, FrontMatterStyle::Table));
    let mut found: Vec<Vec<usize>> = Vec::new();
    for line in &stream.lines {
        if let Some(inner) = line.inner {
            let path = stream.blocks.path(inner).to_vec();
            if !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn a_row_knows_the_innermost_block_it_is_in() {
    // A paragraph in a list, and a rule in the same item, which has no
    // text; the gaps between them are the list's.
    let found = inner_blocks("- a paragraph\n\n  ***\n\n  and another\n");
    assert_eq!(
        found,
        [vec![0, 0, 0], vec![0], vec![0, 0, 1], vec![0, 0, 2]]
    );
    // A table in a quote.
    let found = inner_blocks("> | a | b |\n> |---|---|\n> | 1 | 2 |\n\nafter\n");
    assert_eq!(found, [vec![0, 0], vec![1]]);
    // A heading in an alert, whose title is the alert's own, and a quote
    // in a quote.
    let found = inner_blocks("> [!NOTE]\n> ## A heading\n\n> > deep\n");
    assert_eq!(found, [vec![0], vec![0, 0], vec![1, 0, 0]]);
}

#[test]
fn the_table_has_the_blocks_of_a_page_in_the_documents_order_and_each_row_is_in_its_block() {
    each_page(|name, document, stream, _| {
        let known: Vec<Vec<usize>> = (document.blocks().into_iter())
            .map(|(path, _)| path)
            .collect();
        let table: Vec<&[usize]> = (stream.blocks.ids())
            .map(|id| stream.blocks.path(id))
            .collect();
        assert!(
            table.windows(2).all(|pair| pair[0] < pair[1]),
            "{name}: {table:?}"
        );
        assert!(
            table
                .iter()
                .all(|path| known.iter().any(|known| known == path)),
            "{name}"
        );
        for line in &stream.lines {
            match (line.block, line.inner) {
                (Some(block), Some(inner)) => {
                    let (outer, inner) = (stream.blocks.path(block), stream.blocks.path(inner));
                    assert!(inner.starts_with(outer), "{name}: {outer:?} {inner:?}");
                }
                (None, None) => {}
                other => panic!("{name}: a line with {other:?}"),
            }
        }
    });
}

/// The paragraphs of the page of `source`, as the texts of the block
/// at `path`, by their number: what they say.
fn said(source: &str, path: &[usize], inputs: &Inputs) -> Vec<(u32, String)> {
    let (_, stream) = lay_out(source, inputs);
    let mut found = Vec::new();
    for parts in pages(&stream) {
        for text in texts(&parts) {
            if stream.blocks.path(text.block) == path {
                found.push((text.number, text.paragraph.text().to_owned()));
            }
        }
    }
    found.sort();
    found
}

fn plain() -> Inputs {
    inputs(80, Wide::Wrap, FrontMatterStyle::Table)
}

/// The origins of the paragraph of text `number` of the block at
/// `path`, as (bytes shown, bytes of the document, exact).
fn origins_of(
    source: &str,
    path: &[usize],
    inputs: &Inputs,
) -> Vec<(Range<u32>, Range<u32>, bool)> {
    let (_, stream) = lay_out(source, inputs);
    let parts = pages(&stream);
    let shown = parts
        .iter()
        .flat_map(|parts| texts(parts))
        .find(|text| stream.blocks.path(text.block) == path);
    let text = shown.expect("a text of the block");
    (text.paragraph.from().iter())
        .map(|origin| (origin.shown.clone(), origin.doc.clone(), origin.exact))
        .collect()
}

#[test]
fn a_picture_in_a_paragraph_stands_in_the_place_of_its_alt_text() {
    let mut inputs = plain();
    let key = inputs.drawn("p.png");
    let image = crate::typeset::images::Source::Raster(::image::RgbaImage::new(20, 20));
    inputs.images.insert(key, Ok(image));
    let (_, stream) = lay_out("before ![the alt](p.png) after\n", &inputs);
    let parts = pages(&stream);
    let found = texts(&parts[0]);
    let [text] = &found[..] else {
        panic!("one text")
    };
    // The picture is a box where the alt text would be.
    assert_eq!(text.paragraph.text(), "before  after");
    let picture = text
        .paragraph
        .picture_from(0)
        .expect("the picture's origin");
    assert!(picture.picture);
    assert_eq!((picture.shown.clone(), picture.doc.clone()), (7..7, 7..14));
    let shown: Vec<_> = (text.paragraph.from().iter())
        .map(|origin| (origin.shown.clone(), origin.doc.clone()))
        .collect();
    assert_eq!(shown, [(0..7, 0..7), (7..13, 14..20)]);
    // Without the picture the alt text is shown, as the text it is.
    let alt = origins_of("before ![the alt](p.png) after\n", &[0], &plain());
    assert_eq!(alt, [(0..20, 0..20, true)]);
}

#[test]
fn a_formula_written_out_shows_all_of_its_source_as_a_whole() {
    let origins = origins_of("a $x^2$ b\n", &[0], &plain());
    assert_eq!(origins.len(), 3, "{origins:?}");
    assert_eq!((origins[0].1.clone(), origins[0].2), (0..2, true));
    assert_eq!((origins[1].1.clone(), origins[1].2), (2..5, false));
    assert_eq!((origins[2].1.clone(), origins[2].2), (5..7, true));
}

#[test]
fn the_cells_of_a_table_and_the_keys_and_values_of_front_matter_are_texts_by_number() {
    let table = said("| a | bb |\n|---|----|\n| c | d |\n", &[0], &plain());
    let words: Vec<(u32, &str)> = table.iter().map(|(n, text)| (*n, text.as_str())).collect();
    assert_eq!(words, [(0, "a"), (1, "bb"), (2, "c"), (3, "d")]);
    let front_matter = said(
        "---\nname: kiri\ntags: [a, b]\n---\n\nbody\n",
        &[0],
        &plain(),
    );
    let words: Vec<(u32, &str)> = front_matter
        .iter()
        .map(|(n, text)| (*n, text.as_str()))
        .collect();
    assert_eq!(words, [(0, "name"), (1, "kiri"), (2, "tags"), (3, "a, b")]);
}

#[test]
fn a_summary_shows_its_text_after_a_triangle_that_is_the_layouts() {
    let source = "<details><summary>The summary</summary>\n\ninside\n\n</details>\n";
    let found = said(source, &[0], &plain());
    assert_eq!(found, [(0, "▼ The summary".to_owned())]);
    let origins = origins_of(source, &[0], &plain());
    assert_eq!(origins, [(4..15, 0..11, true)]);
}

#[test]
fn code_shows_its_lines_and_its_tabs_as_wholes_in_a_band_and_wrapped() {
    let source = "```\nline one\n\tline two\n```\n";
    for wide in [Wide::Wrap, Wide::Scroll] {
        let origins = origins_of(source, &[0], &inputs(80, wide, FrontMatterStyle::Table));
        // Runs of the same bytes that go on from each other are one.
        let mut joined: Vec<(Range<u32>, Range<u32>, bool)> = Vec::new();
        for (shown, doc, exact) in origins {
            match joined.last_mut() {
                Some((last_shown, last_doc, true))
                    if exact && last_shown.end == shown.start && last_doc.end == doc.start =>
                {
                    last_shown.end = shown.end;
                    last_doc.end = doc.end;
                }
                _ => joined.push((shown, doc, exact)),
            }
        }
        // "line one" and its line break, a tab spread into four spaces,
        // and "line two".
        assert_eq!(
            joined,
            [
                (0..9, 0..9, true),
                (9..13, 9..10, false),
                (13..21, 10..18, true)
            ],
            "{wide:?}"
        );
    }
}

/// Inputs with the image `key` loaded, `width` by `height` pixels.
fn with_image(key: crate::typeset::images::ImageKey, (width, height): (u32, u32)) -> Inputs {
    let mut inputs = plain();
    let key = inputs.drawn(key);
    let image = crate::typeset::images::Source::Raster(::image::RgbaImage::new(width, height));
    inputs.images.insert(key, Ok(image));
    inputs
}

/// What the pictures on the page of `source` stand in the place of, as
/// the block they are in, the text and the bytes.
fn pictures_of(source: &str, inputs: &Inputs) -> Vec<(Vec<usize>, u32, Range<u32>)> {
    let (_, stream) = lay_out(source, inputs);
    let mut found = Vec::new();
    for parts in pages(&stream) {
        for item in &parts.page.items {
            let Some(at) = &item.at else { continue };
            let Some(doc) = &at.picture else { continue };
            let id = id_of(&parts, at);
            found.push((stream.blocks.path(id).to_vec(), at.text, doc.clone()));
        }
    }
    found
}

#[test]
fn a_lone_image_a_linked_one_a_diagram_and_a_formula_stand_in_the_place_of_their_text() {
    use crate::typeset::images::ImageKey;
    let image = with_image(ImageKey::from("p.png"), (40, 40));
    assert_eq!(
        pictures_of("![the alt](p.png)\n", &image),
        [(vec![0], 0, 0..7)]
    );
    assert_eq!(
        pictures_of("[![the alt](p.png)](https://example.com)\n", &image),
        [(vec![0], 0, 0..7)]
    );
    // In a list item, the path is the item's.
    assert_eq!(
        pictures_of("- one\n\n  ![the alt](p.png)\n", &image),
        [(vec![0, 0, 1], 0, 0..7)]
    );
    let code = "flowchart LR\n  a --> b\n";
    let diagram = with_image(ImageKey::rendered("mermaid", code), (40, 40));
    assert_eq!(
        pictures_of(&format!("```mermaid\n{code}```\n"), &diagram),
        [(vec![0], 0, 0..22)]
    );
    let formula = with_image(ImageKey::rendered("math", "x\n"), (40, 20));
    assert_eq!(pictures_of("$$x$$\n", &formula), [(vec![0], 0, 0..1)]);
}

#[test]
fn a_formula_drawn_in_a_line_stands_in_the_place_of_its_source() {
    use crate::typeset::diagrams::request::INLINE_FORMULA;
    use crate::typeset::images::ImageKey;
    let inputs = with_image(ImageKey::rendered(INLINE_FORMULA, "x^2\n"), (30, 20));
    let (_, stream) = lay_out("a $x^2$ b\n", &inputs);
    let parts = pages(&stream);
    let found = texts(&parts[0]);
    let [text] = &found[..] else {
        panic!("one text")
    };
    let picture = text
        .paragraph
        .picture_from(0)
        .expect("the picture's origin");
    assert!(picture.picture);
    assert_eq!(picture.doc, 2..5);
}

#[test]
fn what_is_under_a_glyph_of_a_text_on_a_page_is_the_place_it_shows() {
    let (mut glyphs, mut wholes) = (0, 0);
    each_page(|name, _, stream, _| {
        for parts in pages(stream) {
            for (item, laid) in parts.all_items() {
                let (
                    Some(at),
                    Shape::Text {
                        paragraph, x, y, ..
                    },
                ) = (&laid.at, &laid.shape)
                else {
                    continue;
                };
                let block = id_of(&parts, at);
                let (band_x, top, visible) = match item.band {
                    None => (0.0, 0.0, f32::MAX),
                    Some(band) => {
                        let band = &parts.page.bands[band];
                        (band.x, band.top, band.visible)
                    }
                };
                let lines = paragraph.lines();
                for origin in paragraph.from() {
                    let text =
                        &paragraph.text()[origin.shown.start as usize..origin.shown.end as usize];
                    for (offset, grapheme) in text.grapheme_indices(true) {
                        // A line break has no glyph to point at.
                        if grapheme.contains(['\n', '\r']) {
                            continue;
                        }
                        let byte = origin.shown.start as usize + offset;
                        let boxes = paragraph.boxes(byte..byte + grapheme.len());
                        let Some(part) = boxes.first() else { continue };
                        let line = &lines[part.line];
                        let middle = (
                            x + (part.left + part.right) / 2.0,
                            y + line.top + line.height / 2.0,
                        );
                        // A band shows what is within its width: scroll it to this.
                        let scrolled = (middle.0 - 5.0).max(0.0).floor();
                        let (px, scrolled) = match item.band {
                            None => (middle.0, 0),
                            Some(_) => (
                                band_x + (middle.0 - scrolled).min(visible - 1.0),
                                scrolled as u32,
                            ),
                        };
                        let expected = Found {
                            block,
                            text: Some((origin.text, origin.at(byte as u32).unwrap())),
                        };
                        assert_eq!(
                            parts.found_at(px, top + middle.1, scrolled),
                            Some(expected),
                            "{name}: {grapheme:?} at {byte} of {:?}",
                            paragraph.text()
                        );
                        match origin.exact {
                            true => glyphs += 1,
                            false => wholes += 1,
                        }
                    }
                }
            }
        }
    });
    assert!(glyphs > 2000 && wholes > 5, "{glyphs} {wholes}");
}

#[test]
fn the_rows_of_every_text_of_a_long_paragraph_are_found_in_time() {
    // One plain text of 20 000 lines, as a log file is. Asking for the
    // rows of each of its texts must not walk all its lines each time:
    // that took minutes for 100 000 lines. The bound is far above what
    // the answer takes, and far below the walk's.
    let source = format!(
        "```\n{}```\n",
        "2026-03-01 INFO request done\n".repeat(20_000)
    );
    let (_, stream) = lay_out(&source, &inputs(100, Wide::Scroll, FrontMatterStyle::Hide));
    let mut asked = 0;
    let started = std::time::Instant::now();
    for parts in pages(&stream) {
        for seen in parts.seen() {
            let Some(item) = seen.item else { continue };
            let shown = seen.origin.shown.clone();
            parts.rows_of(item, shown.start as usize..shown.end as usize);
            asked += 1;
        }
    }
    let took = started.elapsed();
    assert!(asked > 0);
    assert!(
        took < std::time::Duration::from_secs(5),
        "{asked} texts, {took:?}"
    );
}

/// What `whole_graphemes` answered when it scanned every grapheme of the
/// text: the answer to keep.
fn graphemes_by_scan(text: &str, bytes: &Range<usize>) -> Option<Range<usize>> {
    let starts: Vec<usize> = (text.grapheme_indices(true).map(|(at, _)| at))
        .chain([text.len()])
        .collect();
    let from = starts.iter().rev().find(|&&at| at <= bytes.start);
    let to = starts.iter().find(|&&at| at >= bytes.end);
    Some(*from?..*to?)
}

#[test]
fn the_whole_graphemes_of_a_range_are_those_a_scan_of_the_text_finds() {
    let text = "ab\u{2764}\u{fe0f}c\u{1f1fa}\u{1f1e6}\u{1f1fa}\u{1f1e6}e\u{301}\n\r\nzz";
    for start in 0..=text.len() + 1 {
        for end in start..=text.len() + 1 {
            let bytes = start..end;
            assert_eq!(
                whole_graphemes(text, &bytes),
                graphemes_by_scan(text, &bytes),
                "{bytes:?}"
            );
        }
    }
}

#[test]
fn the_whole_graphemes_of_a_range_in_a_long_text_are_found_in_time() {
    // The one-byte range of a newline at a cut, in 6 MB of text, asked
    // 100 times as a large file's map does for its cuts; a scan of the
    // text each time took minutes in a debug build.
    let text = "2026-03-01 INFO request done\n".repeat(200_000);
    let started = std::time::Instant::now();
    for number in 0..100 {
        let at = 29 * (number * 97 + 1) - 1;
        assert!(whole_graphemes(&text, &(at..at + 1)).is_some());
    }
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(2), "{took:?}");
}
