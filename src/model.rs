//! Document model shared by all input formats and backends.
//!
//! The model says what the document means, not how a terminal draws it.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading { level: u8, content: Vec<Inline> },
    Paragraph(Vec<Inline>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text(String),
}
