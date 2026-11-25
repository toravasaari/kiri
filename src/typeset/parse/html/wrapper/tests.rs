use super::*;

fn parts(html: &str) -> Option<Vec<Part>> {
    Some(centred(html)?.into_iter().map(|piece| piece.part).collect())
}

#[test]
fn an_edge_is_the_start_or_the_end_tag_of_a_centring_element_and_nothing_else() {
    assert_eq!(
        edge("<p align=\"center\">\n"),
        Some(Edge::Start(Element::P))
    );
    assert_eq!(
        edge(" <DIV Align=CENTER> <!-- c -->\n"),
        Some(Edge::Start(Element::Div))
    );
    assert_eq!(edge("<center>"), Some(Edge::Start(Element::Center)));
    assert_eq!(edge("</p>\n"), Some(Edge::End(Element::P)));
    assert_eq!(edge("</DIV >"), Some(Edge::End(Element::Div)));
    assert_eq!(edge("</center>"), Some(Edge::End(Element::Center)));
    // A `<p>` or `<div>` that does not centre is no edge.
    assert_eq!(edge("<p>"), None);
    assert_eq!(edge("<div align=\"right\">"), None);
    for not in [
        "<h1 align=\"center\">",
        "<span>",
        "</span>",
        "<p align=\"center\">\ntext",
        "</p>\ntext",
        "<p align=\"center\"></p>",
        "<p align=\"center\" ",
        "",
        "text",
    ] {
        assert_eq!(edge(not), None, "{not:?}");
    }
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

#[test]
fn the_elements_a_block_starts_and_does_not_end_are_listed_in_order() {
    use Element::{Div, P};
    assert_eq!(open_elements("<div class=\"x\">hello\n"), vec![Div]);
    assert_eq!(open_elements("<p>\n"), vec![P]);
    assert_eq!(open_elements("<div align=\"center\">hello"), vec![Div]);
    assert_eq!(open_elements("<div><p>a"), vec![Div, P]);
    assert_eq!(open_elements("<div>a</div>"), Vec::new());
    assert_eq!(open_elements("<div><p>a</div>"), Vec::new());
    assert_eq!(open_elements("<div>a</div><p>b"), vec![P]);
    // An end tag that ends nothing in the block is left out.
    assert_eq!(open_elements("text</div><p>b"), vec![P]);
    assert_eq!(open_elements("<span>x</span>"), Vec::new());
}
