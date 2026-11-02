use super::{Block, BlockKind, Document, Span};
use serde_json::Value;

/// Flatten nested legacy blocks in reading order; report unsupported block types.
pub fn convert(json: &str) -> anyhow::Result<(Document, usize)> {
    let value: Value =
        serde_json::from_str(json).map_err(|_| anyhow::anyhow!("Invalid legacy notes JSON"))?;
    let blocks = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Legacy notes must be an array"))?;
    let mut doc = Document::default();
    let mut unsupported = 0;
    visit(blocks, &mut doc, &mut unsupported)?;
    Ok((doc.canonical(), unsupported))
}
fn inline(value: &Value, spans: &mut Vec<Span>) {
    match value {
        Value::String(text) => spans.push(Span {
            text: text.clone(),
            ..Span::default()
        }),
        Value::Array(values) => {
            for value in values {
                inline(value, spans);
            }
        }
        Value::Object(obj) => {
            if let Some(text) = obj.get("text").and_then(Value::as_str) {
                spans.push(Span {
                    text: text.into(),
                    bold: value["styles"]["bold"].as_bool().unwrap_or(false),
                    italic: value["styles"]["italic"].as_bool().unwrap_or(false),
                });
            }
            for (key, child) in obj {
                if !matches!(
                    key.as_str(),
                    "text" | "styles" | "type" | "id" | "url" | "href"
                ) {
                    if matches!(key.as_str(), "rows" | "cells") {
                        if let Some(items) = child.as_array() {
                            for (index, item) in items.iter().enumerate() {
                                if index > 0 {
                                    spans.push(Span {
                                        text: " ".into(),
                                        ..Span::default()
                                    });
                                }
                                inline(item, spans);
                            }
                        } else {
                            inline(child, spans);
                        }
                    } else {
                        inline(child, spans);
                    }
                }
            }
        }
        _ => {}
    }
}
fn visit(values: &[Value], doc: &mut Document, unsupported: &mut usize) -> anyhow::Result<()> {
    for value in values {
        let kind = match value["type"].as_str() {
            Some("paragraph") => BlockKind::Paragraph,
            Some("heading") => {
                BlockKind::Heading(value["props"]["level"].as_u64().unwrap_or(1).clamp(1, 3) as u8)
            }
            Some("bulletListItem") => BlockKind::Bullet,
            Some("numberedListItem") => BlockKind::Numbered,
            Some("checkListItem") => {
                BlockKind::Checkbox(value["props"]["checked"].as_bool().unwrap_or(false))
            }
            Some(_) => {
                *unsupported += 1;
                BlockKind::Paragraph
            }
            None => anyhow::bail!("Legacy block has no type"),
        };
        let mut spans = Vec::new();
        inline(&value["content"], &mut spans);
        inline(&value["props"]["caption"], &mut spans);
        inline(&value["props"]["name"], &mut spans);
        doc.blocks.push(Block {
            kind,
            spans,
            created_offset_ms: None,
        });
        if let Some(children) = value["children"].as_array() {
            visit(children, doc, unsupported)?;
        }
    }
    Ok(())
}
