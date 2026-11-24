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
    assert!(matches!(&pieces[0].part, Part::Heading(2, _)));
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
