use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub blocks: Vec<Block>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockKind {
    Paragraph,
    Heading(u8),
    Bullet,
    Numbered,
    Checkbox(bool),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
    pub created_offset_ms: Option<u64>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}
impl Document {
    pub fn canonical(&self) -> Self {
        Self {
            blocks: self
                .blocks
                .iter()
                .map(|block| {
                    let mut b = block.clone();
                    if let BlockKind::Heading(level) = &mut b.kind {
                        *level = (*level).clamp(1, 3);
                    }
                    b.spans.clear();
                    for span in &block.spans {
                        if span.text.is_empty() {
                            continue;
                        }
                        if let Some(last) = b.spans.last_mut() {
                            if last.bold == span.bold && last.italic == span.italic {
                                last.text.push_str(&span.text);
                                continue;
                            }
                        }
                        b.spans.push(span.clone());
                    }
                    b
                })
                .collect(),
        }
    }
    /// Inclusive time window, preserving document order. Untimed blocks are excluded.
    pub fn around(&self, t_ms: u64, window_ms: u64) -> Vec<Block> {
        self.blocks
            .iter()
            .filter(|b| {
                b.created_offset_ms
                    .is_some_and(|t| t.abs_diff(t_ms) <= window_ms)
            })
            .cloned()
            .collect()
    }
}
