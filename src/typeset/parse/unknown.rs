//! Markdown elements that the parser leaves out of the document.

use comrak::Node;

use crate::typeset::diagnostic::{Code, Diagnostic};

/// The kinds of Markdown element that the parser found and has no place
/// for in the document model, each with the line it first starts on.
#[derive(Default)]
pub(super) struct Unknown {
    found: Vec<(String, usize)>,
}

impl Unknown {
    /// Notes that `node` is left out of the document.
    pub(super) fn note(&mut self, node: Node<'_>) {
        let name = name(&format!("{:?}", node.data().value));
        if self.found.iter().all(|(found, _)| *found != name) {
            self.found.push((name, node.data().sourcepos.start.line));
        }
    }

    /// One warning for each kind noted, in the order they were found; its
    /// line is `by` lines below where comrak counted.
    pub(super) fn into_warnings(self, by: usize) -> Vec<Diagnostic> {
        (self.found.into_iter())
            .map(|(name, line)| Diagnostic {
                line: Some(line + by),
                ..Diagnostic::new(
                    Code::Markdown,
                    format!("Markdown element not shown: {name}"),
                )
            })
            .collect()
    }
}

/// The words of the name of a comrak node, as its `debug` text starts
/// with it: `FootnoteDefinition(..)` is "footnote definition".
fn name(debug: &str) -> String {
    let variant = debug.split(|c: char| !c.is_alphanumeric()).next();
    let mut words = String::new();
    for c in variant.unwrap_or_default().chars() {
        if c.is_uppercase() && !words.is_empty() {
            words.push(' ');
        }
        words.extend(c.to_lowercase());
    }
    words
}

#[cfg(test)]
mod tests {
    use comrak::Options;

    use super::super::{options, parse_with_options, parse_with_warnings};
    use crate::typeset::diagnostic::{Code, Diagnostic};

    /// The options kiri parses with, and in addition the Markdown that
    /// kiri has no place for yet: footnotes and description lists.
    fn wider() -> Options<'static> {
        let mut options = options();
        options.extension.footnotes = true;
        options.extension.description_lists = true;
        options
    }

    /// The warnings for `source` read with `wider` options, as the lines
    /// that standard error would tell.
    fn told(source: &str) -> Vec<String> {
        let (_, warnings) = parse_with_options(source, true, &wider());
        warnings.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_footnote_is_warned_about_once_with_the_line_of_its_first_part() {
        let source = "A[^1] and B[^2].\n\n[^1]: One.\n\n[^2]: Two.\n";
        assert_eq!(
            told(source),
            [
                "Markdown element not shown: footnote reference (line 1)",
                "Markdown element not shown: footnote definition (line 3)",
            ]
        );
    }

    #[test]
    fn a_dropped_element_inside_a_list_a_quote_or_a_table_is_found_too() {
        let in_list = "- item\n\n  Term\n  : Detail\n";
        assert_eq!(
            told(in_list),
            ["Markdown element not shown: description list (line 3)"]
        );
        let in_quote = "> Term\n> : Detail\n";
        assert_eq!(
            told(in_quote),
            ["Markdown element not shown: description list (line 1)"]
        );
        let in_table = "| a |\n|---|\n| b[^1] |\n\n[^1]: Note.\n";
        assert_eq!(
            told(in_table)[0],
            "Markdown element not shown: footnote reference (line 3)"
        );
    }

    #[test]
    fn the_line_counts_from_the_start_of_the_file_below_front_matter() {
        let source = "---\ntitle: x\n---\n\nText[^1].\n\n[^1]: Note.\n";
        assert_eq!(
            told(source),
            [
                "Markdown element not shown: footnote reference (line 5)",
                "Markdown element not shown: footnote definition (line 7)",
            ]
        );
    }

    #[test]
    fn the_pager_line_gives_the_line_in_front_and_not_in_the_text() {
        let (_, warnings) = parse_with_options("a[^1]\n\n[^1]: Note.\n", true, &wider());
        let warning: &Diagnostic = &warnings[0];
        assert_eq!(warning.code, Code::Markdown);
        assert_eq!(warning.line, Some(1));
        assert_eq!(
            warning.pager_message(),
            "Markdown element not shown: footnote reference"
        );
    }

    #[test]
    fn the_syntax_kiri_reads_today_leaves_nothing_out() {
        let source = "# H\n\n[^1]: not a footnote here\n\nTerm\n: not a list\n\n- [x] done\n\n$x$ and :tada:\n";
        assert!(parse_with_warnings(source, true).1.is_empty());
    }
}
