//! In-memory editing only. Positions count extended graphemes across span boundaries.
use super::{markdown, Block, BlockKind, Document, Span};
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub block: usize,
    pub grapheme: usize,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Position,
    pub focus: Position,
}
impl Selection {
    fn ordered(self) -> (Position, Position) {
        (self.anchor.min(self.focus), self.anchor.max(self.focus))
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Style {
    Bold,
    Italic,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct State {
    document: Document,
    selection: Selection,
    bold: bool,
    italic: bool,
}
/// Consecutive typing is grouped by word and pauses; other edits are separate steps.
/// History includes the selection and the style used for subsequent insertion.
/// The document always contains at least one (possibly empty) paragraph.
pub struct Editor {
    state: State,
    undo: Vec<State>,
    redo: Vec<State>,
    typing: Option<(Instant, Selection)>,
}
impl Editor {
    pub fn new(document: Document) -> Self {
        let mut document = document.canonical();
        if document.blocks.is_empty() {
            document.blocks.push(empty_block());
        }
        Self {
            state: State {
                document,
                selection: Selection::default(),
                bold: false,
                italic: false,
            },
            undo: Vec::new(),
            redo: Vec::new(),
            typing: None,
        }
    }
    pub fn document(&self) -> &Document {
        &self.state.document
    }
    pub fn selection(&self) -> Selection {
        self.state.selection
    }
    /// Invalid coordinates leave both the document and selection unchanged.
    pub fn set_selection(&mut self, selection: Selection) -> anyhow::Result<()> {
        for p in [selection.anchor, selection.focus] {
            let block = self
                .document()
                .blocks
                .get(p.block)
                .ok_or_else(|| anyhow::anyhow!("Invalid note block"))?;
            anyhow::ensure!(
                p.grapheme <= text(block).graphemes(true).count(),
                "Invalid note grapheme"
            );
        }
        self.typing = None;
        self.state.selection = selection;
        // Continue the style immediately before the focus, or the first span at start.
        let p = selection.focus;
        let b = &self.document().blocks[p.block];
        let byte = boundary(b, p.grapheme);
        let mut end = 0;
        let style = b.spans.iter().find(|s| {
            end += s.text.len();
            end >= byte && !s.text.is_empty()
        });
        let (bold, italic) = style.map(|s| (s.bold, s.italic)).unwrap_or_default();
        self.state.bold = bold;
        self.state.italic = italic;
        Ok(())
    }
    fn change(&mut self, edit: impl FnOnce(&mut Self)) {
        self.typing = None;
        let before = self.state.clone();
        edit(self);
        self.state.document = self.document().canonical();
        if before != self.state {
            self.undo.push(before);
            if self.undo.len() > 500 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }
    fn collapse(&mut self, block: usize, byte: usize) {
        let t = text(&self.document().blocks[block]);
        // Round forwards when insertion joins a combining mark or a ZWJ sequence.
        let grapheme = t
            .grapheme_indices(true)
            .take_while(|(i, _)| *i < byte)
            .count();
        let p = Position { block, grapheme };
        self.state.selection = Selection {
            anchor: p,
            focus: p,
        };
    }
    fn replace(&mut self, input: &str) {
        let (a, b) = self.selection().ordered();
        let first = &self.document().blocks[a.block];
        let start = boundary(first, a.grapheme);
        let mut left = slice(first, 0, start);
        let last = &self.document().blocks[b.block];
        let right = slice(last, boundary(last, b.grapheme), text(last).len());
        let mut blocks = vec![Block {
            spans: Vec::new(),
            ..first.clone()
        }];
        let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<_> = normalized.split('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                blocks.push(empty_block());
            }
            let block = blocks.last_mut().unwrap();
            if i == 0 {
                block.spans.append(&mut left);
            }
            block.spans.push(Span {
                text: (*line).into(),
                bold: self.state.bold,
                italic: self.state.italic,
            });
        }
        let last_byte = if lines.len() == 1 {
            start + lines[0].len()
        } else {
            lines.last().unwrap().len()
        };
        blocks.last_mut().unwrap().spans.extend(right);
        let cursor_block = a.block + blocks.len() - 1;
        self.state.document.blocks.splice(a.block..=b.block, blocks);
        self.collapse(cursor_block, last_byte);
    }
    /// Paste literal text. CRLF/CR/LF create paragraphs; markdown is never parsed.
    pub fn insert_plain_text(&mut self, input: &str) {
        self.change(|e| e.replace(input));
    }
    /// Insert typed text and recognize a shortcut only when it fills a paragraph.
    pub fn type_text(&mut self, input: &str) {
        let now = Instant::now();
        let word = input.unicode_words().next().is_some() && input.split_word_bounds().count() == 1;
        let join = word
            && self.typing.is_some_and(|(time, selection)| {
                now.duration_since(time) < Duration::from_millis(1000)
                    && selection == self.selection()
            });
        let prior_undo = if join { self.undo.pop() } else { None };
        self.change(|e| {
            e.replace(input);
            let p = e.selection().focus;
            let block = &mut e.state.document.blocks[p.block];
            if block.kind != BlockKind::Paragraph {
                return;
            }
            let t = text(block);
            if p.grapheme != t.graphemes(true).count() {
                return;
            }
            let kind = match t.as_str() {
                "# " => BlockKind::Heading(1),
                "## " => BlockKind::Heading(2),
                "### " => BlockKind::Heading(3),
                "- " => BlockKind::Bullet,
                "[] " => BlockKind::Checkbox(false),
                "1. " => BlockKind::Numbered,
                _ => return,
            };
            block.kind = kind;
            block.spans.clear();
            e.collapse(p.block, 0);
        });
        if let Some(before) = prior_undo {
            self.undo.pop();
            self.undo.push(before);
        }
        self.typing = word.then_some((now, self.selection()));
    }
    pub fn enter(&mut self) {
        self.change(|e| {
            e.replace("");
            let p = e.selection().focus;
            let block = &e.document().blocks[p.block];
            if p.grapheme == 0 && !text(block).is_empty() {
                e.state.document.blocks.insert(p.block, empty_block());
                e.collapse(p.block + 1, 0);
                return;
            }
            let list = matches!(
                block.kind,
                BlockKind::Bullet | BlockKind::Numbered | BlockKind::Checkbox(_)
            );
            if list && text(block).is_empty() {
                e.state.document.blocks[p.block].kind = BlockKind::Paragraph;
                return;
            }
            let kind = match block.kind {
                BlockKind::Checkbox(_) => BlockKind::Checkbox(false),
                _ if list => block.kind.clone(),
                _ => BlockKind::Paragraph,
            };
            e.replace("\n");
            let next = e.selection().focus.block;
            e.state.document.blocks[next].kind = kind;
        });
    }
    pub fn backspace(&mut self) {
        self.change(|e| {
            let (a, b) = e.selection().ordered();
            if a != b {
                e.replace("");
                return;
            }
            if a.grapheme > 0 {
                e.state.selection.anchor.grapheme -= 1;
                e.replace("");
            } else if e.document().blocks[a.block].kind != BlockKind::Paragraph {
                e.state.document.blocks[a.block].kind = BlockKind::Paragraph;
            } else if a.block > 0 {
                let previous = &e.document().blocks[a.block - 1];
                if text(previous).is_empty() && previous.created_offset_ms.is_none() {
                    let current = e.document().blocks[a.block].clone();
                    e.state.document.blocks[a.block - 1].kind = current.kind;
                    e.state.document.blocks[a.block - 1].created_offset_ms =
                        current.created_offset_ms;
                }
                let previous = &e.document().blocks[a.block - 1];
                e.state.selection.anchor = Position {
                    block: a.block - 1,
                    grapheme: text(previous).graphemes(true).count(),
                };
                e.replace("");
            }
        });
    }
    /// A range is switched off only when every selected span already has the style.
    /// An empty selection toggles the insertion style instead.
    pub fn toggle_style(&mut self, style: Style) {
        self.change(|e| {
            let (a, b) = e.selection().ordered();
            if a == b {
                match style {
                    Style::Bold => e.state.bold = !e.state.bold,
                    Style::Italic => e.state.italic = !e.state.italic,
                }
                return;
            }
            let selected = e.selected_blocks();
            let enabled = !selected.iter().any(|b| !b.spans.is_empty())
                || !selected.iter().flat_map(|b| &b.spans).all(|s| match style {
                    Style::Bold => s.bold,
                    Style::Italic => s.italic,
                });
            for i in a.block..=b.block {
                let block = &e.document().blocks[i];
                let start = if i == a.block {
                    boundary(block, a.grapheme)
                } else {
                    0
                };
                let end = if i == b.block {
                    boundary(block, b.grapheme)
                } else {
                    text(block).len()
                };
                let mut spans = slice(block, 0, start);
                let mut middle = slice(block, start, end);
                for s in &mut middle {
                    match style {
                        Style::Bold => s.bold = enabled,
                        Style::Italic => s.italic = enabled,
                    }
                }
                spans.extend(middle);
                spans.extend(slice(block, end, text(block).len()));
                e.state.document.blocks[i].spans = spans;
            }
            match style {
                Style::Bold => e.state.bold = enabled,
                Style::Italic => e.state.italic = enabled,
            }
        });
    }
    fn selected_blocks(&self) -> Vec<Block> {
        let (a, b) = self.selection().ordered();
        if a == b {
            return Vec::new();
        }
        (a.block..=b.block)
            .filter(|&i| i != b.block || b.grapheme != 0 || a.block == b.block)
            .map(|i| {
                let block = &self.document().blocks[i];
                let start = if i == a.block {
                    boundary(block, a.grapheme)
                } else {
                    0
                };
                let end = if i == b.block {
                    boundary(block, b.grapheme)
                } else {
                    text(block).len()
                };
                // Clipboard content must not carry meeting timestamps.
                Block {
                    kind: block.kind.clone(),
                    spans: slice(block, start, end),
                    created_offset_ms: None,
                }
            })
            .collect()
    }
    pub fn copy_markdown(&self) -> String {
        let mut copied = markdown::serialize(&Document {
            blocks: self.selected_blocks(),
        });
        let (a, b) = self.selection().ordered();
        if a.block != b.block && b.grapheme == 0 {
            copied.push('\n');
        }
        copied
    }
    pub fn undo(&mut self) -> bool {
        self.typing = None;
        if let Some(state) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.state, state));
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        self.typing = None;
        if let Some(state) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.state, state));
            true
        } else {
            false
        }
    }
}
fn empty_block() -> Block {
    Block {
        kind: BlockKind::Paragraph,
        spans: Vec::new(),
        created_offset_ms: None,
    }
}
fn text(block: &Block) -> String {
    block.spans.iter().map(|s| s.text.as_str()).collect()
}
fn boundary(block: &Block, grapheme: usize) -> usize {
    let t = text(block);
    t.grapheme_indices(true)
        .nth(grapheme)
        .map(|(i, _)| i)
        .unwrap_or(t.len())
}
/// Byte boundaries come from the whole block, so a grapheme can span styles.
fn slice(block: &Block, start: usize, end: usize) -> Vec<Span> {
    let mut offset = 0;
    let mut spans = Vec::new();
    for s in &block.spans {
        let from = start.saturating_sub(offset).min(s.text.len());
        let to = end.saturating_sub(offset).min(s.text.len());
        if from < to {
            spans.push(Span {
                text: s.text[from..to].into(),
                ..s.clone()
            });
        }
        offset += s.text.len();
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_typing_pause_and_other_operations_start_new_groups() {
        let mut e = Editor::new(Document::default());
        e.type_text("a");
        e.typing.as_mut().unwrap().0 = Instant::now() - Duration::from_secs(2);
        e.type_text("b");
        assert!(e.undo());
        assert_eq!(text(&e.document().blocks[0]), "a");
        e.type_text("c");
        e.insert_plain_text("d");
        e.type_text("e");
        assert!(e.undo());
        assert_eq!(text(&e.document().blocks[0]), "acd");
        assert!(e.undo());
        assert_eq!(text(&e.document().blocks[0]), "ac");
    }
}
