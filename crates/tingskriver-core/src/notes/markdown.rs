use super::{Block, BlockKind, Document, Span};

fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '<' => out.push_str("\\l"),
            '\\' | '*' | '#' | '-' | '.' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}
pub fn serialize(doc: &Document) -> String {
    doc.canonical()
        .blocks
        .iter()
        .map(|b| {
            let prefix = match b.kind {
                BlockKind::Paragraph => "".into(),
                BlockKind::Heading(n) => format!("{} ", "#".repeat(n as usize)),
                BlockKind::Bullet => "- ".into(),
                BlockKind::Numbered => "1. ".into(),
                BlockKind::Checkbox(false) => "- [ ] ".into(),
                BlockKind::Checkbox(true) => "- [x] ".into(),
            };
            let text = b
                .spans
                .iter()
                .map(|s| {
                    let mark = "*".repeat(usize::from(s.bold) * 2 + usize::from(s.italic));
                    format!("{mark}{}{mark}", escape(&s.text))
                })
                .collect::<Vec<_>>()
                .join("<!--s-->");
            let offset = b
                .created_offset_ms
                .map(|v| format!("<!--offset:{v}--> "))
                .unwrap_or_default();
            format!(
                "{offset}{prefix}{}",
                if text.is_empty() {
                    "<!--empty-->"
                } else {
                    &text
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn parse(input: &str) -> anyhow::Result<Document> {
    if input.is_empty() {
        return Ok(Document::default());
    }
    let mut blocks = Vec::new();
    for mut line in input.split('\n') {
        let mut offset = None;
        if let Some(rest) = line.strip_prefix("<!--offset:") {
            let (value, rest) = rest
                .split_once("--> ")
                .ok_or_else(|| anyhow::anyhow!("Invalid note offset marker"))?;
            offset = Some(
                value
                    .parse::<u64>()
                    .map_err(|_| anyhow::anyhow!("Invalid note offset"))?,
            );
            line = rest;
        }
        let mut kind = BlockKind::Paragraph;
        for (prefix, candidate) in [
            ("### ", BlockKind::Heading(3)),
            ("## ", BlockKind::Heading(2)),
            ("# ", BlockKind::Heading(1)),
            ("- [x] ", BlockKind::Checkbox(true)),
            ("- [ ] ", BlockKind::Checkbox(false)),
            ("- ", BlockKind::Bullet),
            ("1. ", BlockKind::Numbered),
        ] {
            if let Some(rest) = line.strip_prefix(prefix) {
                kind = candidate;
                line = rest;
                break;
            }
        }
        if line == "<!--empty-->" {
            line = "";
        }
        let mut spans = Vec::new();
        for part in line.split("<!--s-->") {
            let mut text = String::new();
            let mut bold = false;
            let mut italic = false;
            let mut chars = part.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    if let Some(c) = chars.next() {
                        text.push(match c {
                            'n' => '\n',
                            'r' => '\r',
                            'l' => '<',
                            c => c,
                        });
                    } else {
                        text.push('\\');
                    }
                } else if c == '*' {
                    if !text.is_empty() {
                        spans.push(Span {
                            text: std::mem::take(&mut text),
                            bold,
                            italic,
                        });
                    }
                    let mut count = 1;
                    while chars.peek() == Some(&'*') {
                        chars.next();
                        count += 1;
                    }
                    if count & 2 != 0 {
                        bold = !bold;
                    }
                    if count & 1 != 0 {
                        italic = !italic;
                    }
                } else {
                    text.push(c);
                }
            }
            spans.push(Span { text, bold, italic });
        }
        blocks.push(Block {
            kind,
            spans,
            created_offset_ms: offset,
        });
    }
    Ok(Document { blocks }.canonical())
}
