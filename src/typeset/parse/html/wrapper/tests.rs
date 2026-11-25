use super::*;

fn parts(html: &str) -> Option<Vec<Part>> {
    Some(centred(html)?.into_iter().map(|piece| piece.part).collect())
}

#[test]
fn centred_gives_the_pieces_and_the_byte_each_starts_at() {
    let html = "<div align=\"center\">\n<h2>Name</h2>\n<p>Tag</p>\nTail <b>x</b>\n</div>\n";
    let pieces = centred(html).expect("a wrapper");
    let at: Vec<_> = pieces.iter().map(|piece| &html[piece.at..][..6]).collect();
    assert_eq!(at, ["<h2>Na", "<p>Tag", "Tail <"]);
    assert!(matches!(&pieces[0].part, Part::Heading(2, _, _)));
    assert!(matches!(&pieces[1].part, Part::Paragraph(_)));
    assert!(matches!(&pieces[2].part, Part::Paragraph(content) if content.len() == 4));
}

#[test]
fn centred_reads_nothing_of_a_p_in_a_p_or_a_heading_in_a_p() {
    assert_eq!(parts("<p align=center><p>x</p></p>"), None);
    assert_eq!(parts("<p align=center><h1>x</h1></p>"), None);
    assert!(parts("<div align=center><p>x</p><h1>y</h1></div>").is_some());
}

#[test]
fn centred_needs_the_end_tag_and_nothing_after_it() {
    assert_eq!(parts("<div align=center>x"), None);
    assert_eq!(parts("<div align=center>x</div>y"), None);
    assert_eq!(
        parts("<div align=center>x</div><!-- c -->\n"),
        parts("<div align=center>x</div>")
    );
    assert_eq!(parts("<div align=center>x</p>"), None);
}

#[test]
fn centred_gives_an_image_in_wrapping_tags_as_an_image() {
    let pieces =
        parts("<div align=center><picture><source srcset=b.png><img src=a.png></picture></div>");
    assert!(matches!(pieces.as_deref(), Some([Part::Image(img)]) if img.src == "a.png"));
}

/// The marks of the events of `html`.
fn marks(html: &str) -> Option<Vec<Mark>> {
    Some(events(html)?.into_iter().map(|event| event.mark).collect())
}

fn named(name: &str) -> &'static Element {
    ELEMENTS
        .iter()
        .find(|element| element.name == name)
        .unwrap()
}

#[test]
fn the_events_of_a_block_are_its_tags_and_texts_without_comments_and_blanks() {
    let html = " <DIV Align=CENTER> <!-- c -->\ntext <span>x</span></P >\n";
    let found = events(html).unwrap();
    assert_eq!(
        found.iter().map(|event| &event.mark).collect::<Vec<_>>(),
        [
            &Mark::Start(named("div"), true),
            &Mark::Other,
            &Mark::Other,
            &Mark::Other,
            &Mark::Other,
            &Mark::End(named("p")),
        ]
    );
    assert_eq!(&html[found[0].span.clone()], "<DIV Align=CENTER>");
    assert_eq!(&html[found[5].span.clone()], "</P >");
    // A `<p>` or `<div>` that does not centre, a `<center>` and a
    // `<details>`, which never centres.
    assert_eq!(
        marks("<p><div align=right><center><details align=center>"),
        Some(vec![
            Mark::Start(named("p"), false),
            Mark::Start(named("div"), false),
            Mark::Start(named("center"), true),
            Mark::Start(named("details"), false),
        ])
    );
    assert_eq!(
        marks("<h1 align=center></span></summary>"),
        Some(vec![Mark::Other; 3])
    );
    assert_eq!(marks("<!-- c -->\n \n"), Some(Vec::new()));
    // HTML that `tokens` cannot read has none.
    assert_eq!(marks("<p align=\"center\" "), None);
    assert_eq!(marks("<details\n"), None);
}

#[test]
fn a_summary_has_its_content_up_to_its_end_tag() {
    let html = "<summary>The <kbd>S</kbd></summary>more";
    let found = marks(html).unwrap();
    let [Mark::Summary(Some((content, end))), ..] = found.as_slice() else {
        panic!("{found:?}");
    };
    assert_eq!(&html[*end..], "more");
    assert_eq!(
        summary_inlines(html, content.clone()).unwrap(),
        [
            Inline::Text("The ".into()),
            Inline::Html("<kbd>".into()),
            Inline::Text("S".into()),
            Inline::Html("</kbd>".into()),
        ]
    );
    assert_eq!(
        marks("<summary>S\n"),
        Some(vec![Mark::Summary(None), Mark::Other])
    );
}

#[test]
fn a_comment_between_words_of_a_summary_leaves_one_space() {
    let content = |html: &str| match marks(html).unwrap().first() {
        Some(Mark::Summary(Some((content, _)))) => summary_inlines(html, content.clone()).unwrap(),
        other => panic!("{other:?}"),
    };
    for html in [
        "<summary>a <!-- c --> b</summary>",
        "<summary>a <!-- c -->b</summary>",
        "<summary>a<!-- c --> b</summary>",
        "<summary>a <!-- c --> <!-- d --> b</summary>",
    ] {
        assert_eq!(content(html), [Inline::Text("a b".into())], "{html:?}");
    }
    assert_eq!(
        content("<summary>a<!-- c -->b</summary>"),
        [Inline::Text("ab".into())]
    );
    assert_eq!(
        content("<summary>A &mdash; B &copy;</summary>"),
        [Inline::Text("A \u{2014} B \u{a9}".into())]
    );
}
