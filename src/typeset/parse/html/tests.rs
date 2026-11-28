use super::*;
use crate::typeset::model::Block;

mod browser;
mod elements;
mod props;
mod rows;
mod table;

#[test]
fn anchor_ids_are_the_id_and_the_name_of_a_tags() {
    assert_eq!(anchor_ids("<a id=\"x\">"), ["x"]);
    assert_eq!(anchor_ids("<a name='y'></a>"), ["y"]);
    assert_eq!(anchor_ids("<a id=\"x\" name=\"y\"></a>"), ["x", "y"]);
    assert_eq!(anchor_ids("<a id=\"x\" name=\"x\"></a>"), ["x"]);
    assert!(anchor_ids("<a href=\"u\">t</a>").is_empty());
    assert!(anchor_ids("<span id=\"x\"></span>").is_empty());
    assert!(anchor_ids("<a id=\"\"></a>").is_empty());
    assert_eq!(
        anchor_ids("<a name=\"a\"></a>\n<a id=\"b\"></a><a href=\"u\" id=\"c\">"),
        ["a", "b", "c"]
    );
    assert!(anchor_ids("no tags <not a tag").is_empty());
}

fn img(src: &str, alt: &str) -> Option<Img> {
    Some(Img {
        src: src.into(),
        alt: alt.into(),
        width: None,
        height: None,
    })
}

fn sized(width: Option<Length>, height: Option<Length>) -> Option<Img> {
    Some(Img {
        width,
        height,
        ..img("a.png", "")?
    })
}

/// The image of `src` in a link to `href`.
fn linked(src: &str, href: &str) -> Option<Inline> {
    Some(Inline::Link {
        url: href.into(),
        title: None,
        content: vec![image(img(src, ""))?],
    })
}

#[test]
fn reads_src_and_alt() {
    assert_eq!(
        inline_img(r#"<img src="a.png" alt="An image">"#),
        img("a.png", "An image")
    );
}

#[test]
fn reads_any_quoting_case_and_self_closing() {
    assert_eq!(inline_img("<IMG SRC='a.png' Alt=x />"), img("a.png", "x"));
    assert_eq!(inline_img("<img src=a.png alt=\"x\"/>"), img("a.png", "x"));
    assert_eq!(
        inline_img("<img\n  src=\"a.png\"\n  alt=\"two\nlines\">"),
        img("a.png", "two\nlines")
    );
    assert_eq!(
        inline_img(r#"<img alt="a > b" src="a.png">"#),
        img("a.png", "a > b")
    );
}

#[test]
fn an_attribute_without_a_value_is_empty() {
    assert_eq!(inline_img("<img hidden src=a.png alt>"), img("a.png", ""));
}

#[test]
fn the_first_of_repeated_attributes_counts() {
    assert_eq!(
        inline_img("<img src=a.png src=b.png alt=x>"),
        img("a.png", "x")
    );
}

#[test]
fn decodes_entities_in_values() {
    assert_eq!(
        inline_img(r#"<img src="a.png?x=1&amp;y=2" alt="&lt;Fish &amp; chips&gt;">"#),
        img("a.png?x=1&y=2", "<Fish & chips>")
    );
    assert_eq!(
        inline_img(r#"<img src=a.png alt="&quot;&apos;&nbsp;&#65;&#x42;&#X43;">"#),
        img("a.png", "\"'\u{a0}ABC")
    );
}

#[test]
fn keeps_entities_it_does_not_know() {
    assert_eq!(
        inline_img(r#"<img src=a.png alt="&foo; &amp &#; &#xZZ; &#0; a&b">"#),
        img("a.png", "&foo; &amp &#; &#xZZ; \u{fffd} a&b")
    );
}

#[test]
fn decodes_the_common_named_entities() {
    for (name, character) in [
        ("copy", '\u{a9}'),
        ("reg", '\u{ae}'),
        ("trade", '\u{2122}'),
        ("hellip", '\u{2026}'),
        ("mdash", '\u{2014}'),
        ("ndash", '\u{2013}'),
        ("lsquo", '\u{2018}'),
        ("rsquo", '\u{2019}'),
        ("ldquo", '\u{201c}'),
        ("rdquo", '\u{201d}'),
        ("sbquo", '\u{201a}'),
        ("bdquo", '\u{201e}'),
        ("laquo", '\u{ab}'),
        ("raquo", '\u{bb}'),
        ("lsaquo", '\u{2039}'),
        ("rsaquo", '\u{203a}'),
        ("bull", '\u{2022}'),
        ("middot", '\u{b7}'),
        ("times", '\u{d7}'),
        ("divide", '\u{f7}'),
        ("minus", '\u{2212}'),
        ("plusmn", '\u{b1}'),
        ("deg", '\u{b0}'),
        ("micro", '\u{b5}'),
        ("para", '\u{b6}'),
        ("sect", '\u{a7}'),
        ("dagger", '\u{2020}'),
        ("Dagger", '\u{2021}'),
        ("euro", '\u{20ac}'),
        ("pound", '\u{a3}'),
        ("yen", '\u{a5}'),
        ("cent", '\u{a2}'),
        ("larr", '\u{2190}'),
        ("rarr", '\u{2192}'),
        ("uarr", '\u{2191}'),
        ("darr", '\u{2193}'),
        ("harr", '\u{2194}'),
        ("ensp", '\u{2002}'),
        ("emsp", '\u{2003}'),
        ("thinsp", '\u{2009}'),
    ] {
        assert_eq!(entity(name), Some(character), "&{name};");
        assert_eq!(
            decode_entities(&format!("a&{name};b")),
            format!("a{character}b")
        );
    }
}

#[test]
fn named_entities_are_case_sensitive_and_need_their_semicolon() {
    for kept in ["&COPY;", "&Mdash;", "&dagger", "&mdash", "&foo;", "&;"] {
        assert_eq!(decode_entities(kept), kept);
    }
    assert_eq!(decode_entities("&amp;mdash;"), "&mdash;");
    assert_eq!(decode_entities("&mdash;&mdash;"), "\u{2014}\u{2014}");
}

#[test]
fn keeps_an_ampersand_far_from_a_semicolon() {
    let long = format!("&{};", "a".repeat(40));
    assert_eq!(
        inline_img(&format!("<img src=a.png alt=\"{long} &amp;\">")),
        img("a.png", &format!("{long} &"))
    );
}

#[test]
fn an_img_needs_a_src() {
    assert_eq!(inline_img("<img alt=x>"), None);
    assert_eq!(inline_img("<img src alt=x>"), None);
    assert_eq!(inline_img("<img src=\" \n\" alt=x>"), None);
}

#[test]
fn trims_whitespace_around_the_src() {
    assert_eq!(
        inline_img("<img src=\" a.png\n\" alt=\" x \">"),
        img("a.png", " x ")
    );
}

#[test]
fn inline_html_is_an_image_only_when_it_is_one_img_tag() {
    assert_eq!(inline_img("<span>"), None);
    assert_eq!(inline_img("</img>"), None);
    assert_eq!(inline_img("<imgx src=a.png>"), None);
    assert_eq!(inline_img("<img src=a.png"), None);
    assert_eq!(inline_img("<img src=a.png><img src=b.png>"), None);
}

#[test]
fn reads_width_and_height_in_css_pixels() {
    use Length::Pixels;
    assert_eq!(
        inline_img("<img src=a.png width=320 height=\"240\">"),
        sized(Some(Pixels(320)), Some(Pixels(240)))
    );
    assert_eq!(
        inline_img("<img src=a.png width=\" 100px\">"),
        sized(Some(Pixels(100)), None)
    );
    assert_eq!(
        inline_img("<img src=a.png height=\"80.6\">"),
        sized(None, Some(Pixels(81)))
    );
}

#[test]
fn reads_a_percentage() {
    assert_eq!(
        inline_img("<img src=a.png width=\"50%\" height=\"25.5 %\">"),
        sized(Some(Length::Percent(50)), Some(Length::Pixels(26)))
    );
    assert_eq!(
        inline_img("<img src=a.png width=\"33.4%\">"),
        sized(Some(Length::Percent(33)), None)
    );
}

#[test]
fn ignores_lengths_that_are_no_size() {
    for value in ["", "px", "-10", "0", "0.4", "0%", ".5", "auto"] {
        assert_eq!(
            inline_img(&format!(
                "<img src=a.png width=\"{value}\" height=\"{value}\">"
            )),
            sized(None, None),
            "{value:?}"
        );
    }
    assert_eq!(
        inline_img("<img src=a.png width=99999999999>"),
        sized(Some(Length::Pixels(u32::MAX)), None)
    );
}

/// The image that `source` shows, when it shows one and nothing more but
/// tags: an image, or a link round one, in a paragraph that may be centred.
fn shown_image(source: &str) -> Option<Inline> {
    fn shown(blocks: &[Block], into: &mut Vec<Inline>) -> bool {
        blocks.iter().all(|block| match block {
            Block::Html(_) => true,
            Block::Centered(inner) => shown(inner, into),
            Block::Paragraph(content) => {
                into.extend(
                    (content.iter())
                        .filter(|inline| !matches!(inline, Inline::Html(_)))
                        .cloned(),
                );
                true
            }
            _ => false,
        })
    }
    let mut inlines = Vec::new();
    if !shown(&crate::typeset::parse::parse(source).blocks, &mut inlines) {
        return None;
    }
    match inlines.as_slice() {
        [image @ Inline::Image(_)] => Some(image.clone()),
        [link @ Inline::Link { content, .. }]
            if matches!(content.as_slice(), [Inline::Image(_)]) =>
        {
            Some(link.clone())
        }
        _ => None,
    }
}

/// The image that `img` gives in a document.
fn image(img: Option<Img>) -> Option<Inline> {
    let img = img?;
    Some(Inline::Image(crate::typeset::model::Image {
        url: img.src,
        alt: match img.alt.is_empty() {
            true => Vec::new(),
            false => vec![Inline::Text(img.alt)],
        },
        width: img.width,
        height: img.height,
    }))
}

#[test]
fn a_block_of_one_img_is_an_image() {
    assert_eq!(
        shown_image("<img src=a.png alt=x>\n"),
        image(img("a.png", "x"))
    );
}

#[test]
fn a_block_may_wrap_its_img() {
    let wrapped = "<p align=\"center\">\n  <img src=\"a.png\" alt=\"x\">\n</p>\n";
    assert_eq!(shown_image(wrapped), image(img("a.png", "x")));
    let centred = "<CENTER><div><img src=a.png alt=x></div></CENTER>\n";
    assert_eq!(shown_image(centred), image(img("a.png", "x")));
    let commented = "<!-- logo -->\n<p><img src=a.png alt=x></p>\n";
    assert_eq!(shown_image(commented), image(img("a.png", "x")));
    // A `<p>` may leave out its end tag, which the block or an end tag
    // round it then ends.
    for html in [
        "<p align=\"center\">\n  <img src=a.png>\n",
        "<div><p><img src=a.png></div>\n",
    ] {
        assert_eq!(shown_image(html), image(img("a.png", "")), "{html:?}");
    }
}

#[test]
fn a_picture_shows_its_img() {
    let picture = "<picture>\n\
            \x20 <source media=\"(prefers-color-scheme: dark)\" srcset=\"dark.png\">\n\
            \x20 <img src=\"light.png\" alt=\"Logo\">\n\
            </picture>\n";
    assert_eq!(shown_image(picture), image(img("light.png", "Logo")));
    // Without its end tag it shows its tags as they are.
    let open = "<picture><source srcset=b.png><img src=a.png>\n";
    let blocks = crate::typeset::parse::parse(open).blocks;
    assert!(
        matches!(&blocks[..], [Block::Paragraph(content)] if content.len() == 3),
        "{blocks:?}"
    );
}

#[test]
fn reads_a_link_from_its_tags() {
    assert_eq!(
        link_start(r#"<a href="https://x/?a=1&amp;b=2">"#),
        Some("https://x/?a=1&b=2".into())
    );
    assert_eq!(link_start("<A HREF=x title=y>"), Some("x".into()));
    assert_eq!(link_start("<a>"), None);
    assert_eq!(link_start("<a href=x><b>"), None);
    assert_eq!(link_start("<abbr href=x>"), None);
    assert!(is_link_end("</a>"));
    assert!(is_link_end("</A >"));
    assert!(!is_link_end("<a>"));
    assert!(!is_link_end("</abbr>"));
}

#[test]
fn reads_a_line_break_in_any_spelling() {
    for br in ["<br>", "<br/>", "<br />", "<BR>", "<Br\n>", "<br class=x>"] {
        assert!(is_line_break(br), "{br:?}");
    }
    for other in ["<brx>", "<br", "<br><br>", "<b>", "</br>", "<!-- <br> -->"] {
        assert!(!is_line_break(other), "{other:?}");
    }
}

#[test]
fn reads_the_tags_of_a_kbd() {
    assert_eq!(phrase_start("<kbd>"), Some(Phrase::Kbd));
    assert_eq!(phrase_start("<KBD title=\"key\">"), Some(Phrase::Kbd));
    assert_eq!(phrase_end("</kbd>"), Some(Phrase::Kbd));
    assert_eq!(phrase_end("</Kbd >"), Some(Phrase::Kbd));
    for other in ["</kbd>", "<kbdx>", "<abbr>", "<kbd><kbd>", "<kbd"] {
        assert_eq!(phrase_start(other), None, "{other:?}");
    }
    for other in ["<kbd>", "</kbdx>", "</abbr>", "</kbd></kbd>"] {
        assert_eq!(phrase_end(other), None, "{other:?}");
    }
}

#[test]
fn reads_the_tags_of_a_sub_and_a_sup() {
    assert_eq!(phrase_start("<sub>"), Some(Phrase::Sub));
    assert_eq!(phrase_end("</SUB>"), Some(Phrase::Sub));
    assert_eq!(phrase_start("<sup class=x>"), Some(Phrase::Sup));
    assert_eq!(phrase_end("</sup >"), Some(Phrase::Sup));
    assert_eq!(phrase_start("<summary>"), None);
    assert_eq!(phrase_start("<supx>"), None);
}

#[test]
fn reads_the_tags_of_a_mark() {
    assert_eq!(phrase_start("<mark class=x>"), Some(Phrase::Mark));
    assert_eq!(phrase_end("</MARK>"), Some(Phrase::Mark));
    assert_eq!(phrase_start("<marker>"), None);
}

#[test]
fn a_block_may_link_its_img() {
    let linked_block = "<p align=center>\n  <a href=\"https://x\"><img src=a.png></a>\n</p>\n";
    assert_eq!(shown_image(linked_block), linked("a.png", "https://x"));
    assert_eq!(
        shown_image("<a><img src=a.png></a>\n"),
        image(img("a.png", ""))
    );
    // A link with nothing in it is no link: its tags stay.
    assert_eq!(
        shown_image("<a href=x></a><img src=a.png>\n"),
        image(img("a.png", ""))
    );
}

#[test]
fn a_block_with_more_than_an_img_is_not_an_image() {
    for html in [
        "<p>Text <img src=a.png></p>\n",
        "<p><img src=a.png><img src=b.png></p>\n",
        "<p><b><img src=a.png></b></p>\n",
        "<p><br><img src=a.png></p>\n",
        "<p>&nbsp;<img src=a.png></p>\n",
        "<div>\n</div>\n",
        "<!DOCTYPE html><img src=a.png>\n",
        "<!-- <img src=a.png> -->\n",
        "<img alt=x>\n",
    ] {
        assert_eq!(shown_image(html), None, "{html:?}");
    }
}

#[test]
fn a_block_cut_short_is_not_an_image() {
    for html in [
        "<img src=a.png>\n<p\n",
        "<img src=a.png>\n<!-- open\n",
        "<img src=\"a.png>\n",
    ] {
        assert_eq!(shown_image(html), None, "{html:?}");
    }
}

/// Odd input that the readers of HTML must meet without a panic.
fn odd_inputs() -> Vec<String> {
    let mut inputs: Vec<String> = [
        "",
        "<",
        "<h2",
        "<h2>",
        "</h2>",
        "<h2>x",
        "<h2 a=\"",
        "<!--",
        "<!-- x",
        "<pre",
        "<pre>",
        "&",
        "&;",
        "&#",
        "&#x;",
        "&#99999999999;",
        "<>",
        "</>",
        "</",
        "<h2></h2",
        "<h1><h2>a</h2></h1>",
        "\u{e9}<h2>\u{e9}</h2>\u{e9}",
        "<b",
        "<b>",
        "</b>",
        "<code",
        "<code>",
        "</code>",
        "&mdash",
        "&mdash;&",
    ]
    .map(str::to_owned)
    .into();
    inputs.push("&".repeat(100_000));
    inputs.push("&mdash".repeat(20_000));
    inputs.push("<h2>".repeat(5_000));
    inputs
}

#[test]
fn odd_input_does_not_panic() {
    for input in odd_inputs() {
        crate::typeset::parse::parse(&input);
        headings(&input);
        phrase_start(&input);
        phrase_end(&input);
        decode_entities(&input);
    }
    let code = |source| crate::typeset::parse::parse(source).blocks;
    assert!(matches!(&code("<pre")[..], [Block::Paragraph(_)]));
    assert!(matches!(&code("<pre>")[..], [Block::Html(_)]));
    assert!(matches!(
        &code("<pre>\u{e9}</pre>")[..],
        [Block::CodeBlock { code, .. }] if &**code == "\u{e9}\n"
    ));
    assert_eq!(decode_entities(&"&".repeat(1000)), "&".repeat(1000));
}

#[test]
fn an_a_start_tag_without_an_href_is_an_anchor_start() {
    for anchor in ["<a name=\"x\">", "<a id=x>", "<A>"] {
        assert!(is_anchor_start(anchor), "{anchor:?}");
    }
    for other in [
        "<a href=\"u\">",
        "<a name=\"x\" href=\"u\">",
        "</a>",
        "<b>",
        "<abbr>",
    ] {
        assert!(!is_anchor_start(other), "{other:?}");
    }
}

/// How the time to parse `make(n)` grows from `n` to eight times `n`
/// (`crate::testing::growth`).
fn growth(make: impl Fn(usize) -> String, n: usize) -> f64 {
    crate::testing::growth(
        |source| {
            crate::typeset::parse::parse(source);
        },
        make,
        n,
    )
}

#[test]
fn many_stray_end_tags_in_a_paragraph_are_joined_in_linear_time() {
    let alone = growth(|n| format!("text {}\n", "</b>".repeat(n)), 2_000);
    assert!(alone < 24.0, "only end tags: {alone}");
    // A start tag that never ends, far back, which each end tag of another
    // element must not search for.
    let after_start = growth(|n| format!("<b>text {}\n", "</i>".repeat(n)), 2_000);
    assert!(after_start < 24.0, "after a start tag: {after_start}");
    let in_links = growth(
        |n| format!("<a name=x>text {}\n", "</a><kbd>".repeat(n)),
        1_000,
    );
    assert!(in_links < 24.0, "start tags that never end: {in_links}");
}

/// How many elements are in each other, down the first of each.
fn depth(inlines: &[Inline]) -> usize {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Strong(inner) => 1 + depth(inner),
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn start_tags_one_in_another_join_only_to_a_depth_of_100() {
    let nested = |n: usize| {
        let source = format!("{}x{}\n", "<b>".repeat(n), "</b>".repeat(n));
        match crate::typeset::parse::parse(&source).blocks.as_slice() {
            [Block::Paragraph(content)] => depth(content),
            other => panic!("not one paragraph: {other:?}"),
        }
    };
    assert_eq!(nested(50), 50);
    assert_eq!(nested(100), 100);
    assert_eq!(nested(1_000), 100);
    // So deep that the stack of a thread would not hold it.
    assert_eq!(nested(20_000), 100);
}

/// Inlines as Markdown-like text, the elements of HTML in brackets.
fn show(inlines: &[Inline]) -> String {
    let wrap = |name: &str, inner: &[Inline]| format!("<{name}>{}</{name}>", show(inner));
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Text(text) => text.clone(),
            Inline::Html(tag) => format!("[{tag}]"),
            Inline::Strong(inner) => wrap("strong", inner),
            Inline::Emphasis(inner) => wrap("em", inner),
            Inline::Underline(inner) => wrap("u", inner),
            Inline::Kbd(inner) => wrap("kbd", inner),
            Inline::Code(code) => format!("<code:{code}>"),
            Inline::Anchor(id) => format!("<anchor:{id}>"),
            Inline::Image(image) => format!("<image:{}>", image.url),
            Inline::Link { url, content, .. } => format!("<link:{url}>{}</link>", show(content)),
            other => format!("{other:?}"),
        })
        .collect()
}

fn paragraph(source: &str) -> String {
    match crate::typeset::parse::parse_all(source).blocks.as_slice() {
        [Block::Paragraph(content)] => show(content),
        other => panic!("not one paragraph: {other:?}"),
    }
}

/// The pairing of start and end tags in text, pinned before the join was
/// rewritten to read each tag once.
#[test]
fn start_and_end_tags_in_text_pair_as_they_did() {
    for (source, expected) in [
        // The nearest start tag of its name, and what is open inside it is
        // given up.
        ("<b><i>x</b>y</i>\n", "<strong>[<i>]x</strong>y[</i>]"),
        (
            "<b>x<b>y</b>z</b>\n",
            "<strong>x<strong>y</strong>z</strong>",
        ),
        ("<b>x</i>y</b>\n", "<strong>x[</i>]y</strong>"),
        // An end tag of another element than the start tag before it.
        ("<b>x</strong>\n", "[<b>]x[</strong>]"),
        // A `<code>` is code only around text; the pair stays as it is and
        // is closed.
        (
            "<code><b>x</b></code>\n",
            "[<code>]<strong>x</strong>[</code>]",
        ),
        ("<code>a</code>b</code>\n", "<code:a>b[</code>]"),
        ("<code><code>a</code></code>\n", "[<code>]<code:a>[</code>]"),
        // A link holds something; an empty one stays as it is and is closed.
        ("<a href=x></a>y</a>\n", "[<a href=x>][</a>]y[</a>]"),
        (
            "<a href=x></a><a href=y>t</a>\n",
            "[<a href=x>][</a>]<link:y>t</link>",
        ),
        (
            "<a href=x><b>t</b></a>\n",
            "<link:x><strong>t</strong></link>",
        ),
        (
            "<a href=x><img src=a.png></a>\n",
            "<link:x><image:a.png></link>",
        ),
        // A place that an `<a>` without an `href` names; what it holds stays,
        // and the start tags in it are open still.
        ("<a name=p>x</a>\n", "<anchor:p>x"),
        ("<a name=p><u>x</a>y</u>\n", "<anchor:p><u>xy</u>"),
        ("<u><a id=q>x</a>y</u>\n", "<u><anchor:q>xy</u>"),
        ("</a><a name=p>x</a>\n", "[</a>]<anchor:p>x"),
        // Text elements nest.
        ("<kbd><kbd>x</kbd></kbd>\n", "<kbd><kbd>x</kbd></kbd>"),
    ] {
        assert_eq!(paragraph(source), expected, "{source:?}");
    }
}

/// How today's reading of the HTML in a document differs from a browser's,
/// on the documents of `browser::document`: the baseline that the switch
/// to the tree is measured by (roadmap 4.10.2, step 5 takes it to none).
/// How many documents `parse` reads otherwise than a browser, and why. The
/// reading before the tree of HTML (roadmap 4.10.2, step 4) differed on 347
/// of 600.
#[test]
fn parse_differs_from_a_browser_as_pinned() {
    insta::assert_snapshot!(browser::compare(browser::kiri));
}

/// The corpus has lists in it, so that no difference is not for want of them.
#[test]
fn the_corpus_of_the_comparison_has_lists() {
    let count = browser::with_lists();
    assert!(
        count >= 400,
        "{count} of {} documents have a list",
        browser::DOCUMENTS
    );
}

#[test]
fn the_comparison_maps_a_browsers_tree_onto_shapes() {
    use browser::Shape::{Centred, Details, Heading, List, Paragraph};
    let p = |text: &str| Paragraph(text.to_owned());
    // A plain `<div>` is transparent, a plain `<p>` a paragraph, and a `<p>`
    // ends at the start of a block.
    assert_eq!(
        browser::browser("<div>a <b>b</b><div>c</div>d<p>e<div>f</div></div>"),
        [p("a b"), p("c"), p("d"), p("e"), p("f")]
    );
    assert_eq!(
        browser::browser("<p align=center>x<br>y</p><center><h2>h</h2>t</center>"),
        [
            Centred(vec![p("x y")]),
            Centred(vec![Heading(2, "h".into()), p("t")])
        ]
    );
    // A centring element in one is flat, and a `<details>` has the summary
    // a browser gives it.
    assert_eq!(
        browser::browser("<div align=center><p align=center>x</p></div>"),
        [Centred(vec![p("x")])]
    );
    assert_eq!(
        browser::browser("<details><summary>S</summary>t</details><details>u</details>"),
        [
            Details("S".into(), vec![p("t")]),
            Details("Details".into(), vec![p("u")])
        ]
    );
    // A list is its items, loose or tight alike, and an `<ol>` has the number
    // a browser gives it.
    assert_eq!(
        browser::browser("<ul><li>a<ol start=7><li><p>b</p></ul>"),
        [List(
            None,
            vec![vec![p("a"), List(Some(7), vec![vec![p("b")]])]]
        )]
    );
}

#[test]
fn parse_agrees_with_a_browser_on_documents_that_need_no_repair() {
    for source in [
        "A paragraph.\n",
        "## A heading\n\ntext\n",
        "<p align=\"center\">\n  one <b>two</b><br>\n  three\n</p>\n",
        "<div align=\"center\">\n\nA paragraph in a div.\n\n</div>\n",
        "<h1 align=\"center\">Name</h1>\n\ntext\n",
        "<details>\n<summary>S</summary>\n\ntext\n\n</details>\n",
        "<center>\n<h2>H</h2>\n<p>text</p>\n</center>\n",
    ] {
        assert!(browser::agrees(source, browser::kiri), "{source:?}");
    }
}

/// Every kind of phrase has a stack of open start tags in the inline join:
/// `Phrase` and `Phrase::ALL` are made from one list, and `PHRASES` is the
/// length of `ALL`, so none is missing from the count. The test checks that
/// each is at its own place in `ALL`, and that `after`, which has an arm for
/// every variant with no wildcard, chains through all of them from the first.
#[test]
fn every_phrase_has_a_place_among_the_phrases_of_the_inline_join() {
    fn after(phrase: Phrase) -> Option<Phrase> {
        match phrase {
            Phrase::Kbd => Some(Phrase::Mark),
            Phrase::Mark => Some(Phrase::Sub),
            Phrase::Sub => Some(Phrase::Sup),
            Phrase::Sup => Some(Phrase::B),
            Phrase::B => Some(Phrase::Strong),
            Phrase::Strong => Some(Phrase::I),
            Phrase::I => Some(Phrase::Em),
            Phrase::Em => Some(Phrase::S),
            Phrase::S => Some(Phrase::Del),
            Phrase::Del => Some(Phrase::Code),
            Phrase::Code => Some(Phrase::U),
            Phrase::U => Some(Phrase::Span),
            Phrase::Span => Some(Phrase::Ins),
            Phrase::Ins => Some(Phrase::Strike),
            Phrase::Strike => Some(Phrase::Picture),
            Phrase::Picture => Some(Phrase::A),
            Phrase::A => None,
        }
    }
    assert_eq!(Phrase::ALL.len(), inline::PHRASES);
    for (at, phrase) in Phrase::ALL.iter().enumerate() {
        assert_eq!(*phrase as usize, at, "{phrase:?} is out of order");
    }
    let mut chain = Vec::new();
    let mut next = Phrase::ALL.first().copied();
    while let Some(phrase) = next {
        chain.push(phrase);
        next = after(phrase);
    }
    assert_eq!(
        chain,
        Phrase::ALL,
        "the chain of `after` visits every phrase"
    );
}
