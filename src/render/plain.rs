//! Plain text backend: no escape sequences, works in any terminal.

use std::io::{self, Write};

use crate::model::{Block, Document, Inline};

pub fn render(document: &Document, out: &mut impl Write) -> io::Result<()> {
    for (index, block) in document.blocks.iter().enumerate() {
        if index > 0 {
            writeln!(out)?;
        }
        match block {
            Block::Heading { level, content } => {
                let marker = "#".repeat(usize::from(*level));
                writeln!(out, "{marker} {}", text(content))?;
            }
            Block::Paragraph(content) => writeln!(out, "{}", text(content))?,
        }
    }
    Ok(())
}

fn text(content: &[Inline]) -> String {
    content
        .iter()
        .map(|inline| match inline {
            Inline::Text(text) => text.as_str(),
        })
        .collect()
}
