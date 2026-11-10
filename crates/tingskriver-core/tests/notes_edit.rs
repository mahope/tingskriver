use tingskriver_core::notes::{
    edit::{Editor, Position, Selection, Style},
    markdown, BlockKind, Document,
};

fn pos(block: usize, grapheme: usize) -> Position {
    Position { block, grapheme }
}
fn select(e: &mut Editor, a: Position, b: Position) {
    e.set_selection(Selection {
        anchor: a,
        focus: b,
    })
    .unwrap();
}
fn check(e: &Editor) {
    assert_eq!(
        markdown::parse(&markdown::serialize(e.document())).unwrap(),
        *e.document()
    );
    let s = e.selection();
    let mut other = Editor::new(e.document().clone());
    other.set_selection(s).unwrap();
}
#[test]
fn shortcuts_lists_and_history_round_trip() {
    for (prefix, kind) in [
        ("# ", BlockKind::Heading(1)),
        ("- ", BlockKind::Bullet),
        ("[] ", BlockKind::Checkbox(false)),
        ("1. ", BlockKind::Numbered),
    ] {
        let mut e = Editor::new(Document::default());
        for c in prefix.chars() {
            e.type_text(&c.to_string());
            check(&e);
        }
        assert_eq!(e.document().blocks[0].kind, kind);
        e.insert_plain_text("æøå");
        check(&e);
        e.enter();
        check(&e);
        assert_eq!(
            e.document().blocks[1].kind,
            if kind == BlockKind::Heading(1) {
                BlockKind::Paragraph
            } else {
                kind.clone()
            }
        );
        e.enter();
        check(&e);
        assert_eq!(
            e.document().blocks.last().unwrap().kind,
            BlockKind::Paragraph
        );
        let after = e.document().clone();
        assert!(e.undo());
        check(&e);
        assert!(e.redo());
        check(&e);
        assert_eq!(e.document(), &after);
    }
}
#[test]
fn list_backspace_unwraps_then_merges_and_split_preserves_styles() {
    let mut e = Editor::new(markdown::parse("first\n- **æøå**").unwrap());
    select(&mut e, pos(1, 1), pos(1, 1));
    e.enter();
    check(&e);
    assert_eq!(e.document().blocks[2].spans[0].text, "øå");
    assert!(e.document().blocks[2].spans[0].bold);
    e.backspace();
    check(&e);
    assert_eq!(e.document().blocks[2].kind, BlockKind::Paragraph);
    e.backspace();
    check(&e);
    assert_eq!(e.document().blocks[1].spans[0].text, "æøå");
    select(&mut e, pos(1, 0), pos(1, 0));
    e.backspace();
    check(&e);
    assert_eq!(e.document().blocks[1].kind, BlockKind::Paragraph);
    e.backspace();
    check(&e);
    assert_eq!(e.document().blocks.len(), 1);
    assert_eq!(e.selection().focus, pos(0, 5));
}
#[test]
fn formatting_copy_and_cross_block_replacement() {
    let mut e = Editor::new(markdown::parse("æøå\n- xyz").unwrap());
    select(&mut e, pos(1, 2), pos(0, 1));
    e.toggle_style(Style::Bold);
    check(&e);
    e.toggle_style(Style::Italic);
    check(&e);
    let copied = markdown::parse(&e.copy_markdown()).unwrap();
    assert_eq!(copied.blocks[0].spans[0].text, "øå");
    assert_eq!(copied.blocks[1].spans[0].text, "xy");
    assert!(copied
        .blocks
        .iter()
        .all(|b| b.spans.iter().all(|s| s.bold && s.italic)));
    e.toggle_style(Style::Bold);
    check(&e);
    assert!(!e.document().blocks[0].spans[1].bold);
    e.insert_plain_text("**plain**\r\n# literal");
    check(&e);
    assert_eq!(e.document().blocks.len(), 2);
    assert_eq!(e.document().blocks[1].kind, BlockKind::Paragraph);
    assert_eq!(e.document().blocks[1].spans[0].text, "# literalz");
    assert!(e.undo());
    check(&e);
    assert!(e.redo());
    check(&e);
    assert!(e.undo());
    e.insert_plain_text("new");
    check(&e);
    assert!(!e.redo());
}
#[test]
fn grapheme_boundaries_include_combining_marks_emoji_and_span_boundaries() {
    let mut e = Editor::new(Document::default());
    e.insert_plain_text("æøa\u{30a}👨‍👩‍👧‍👦");
    check(&e);
    assert_eq!(e.selection().focus, pos(0, 4));
    e.backspace();
    check(&e);
    assert_eq!(e.selection().focus, pos(0, 3));
    e.backspace();
    check(&e);
    assert_eq!(e.document().blocks[0].spans[0].text, "æø");
    e.toggle_style(Style::Bold);
    check(&e);
    e.insert_plain_text("a");
    check(&e);
    e.toggle_style(Style::Bold);
    e.insert_plain_text("\u{30a}");
    check(&e);
    assert_eq!(e.selection().focus, pos(0, 3));
    e.backspace();
    check(&e);
    assert_eq!(e.document().blocks[0].spans[0].text, "æø");
    let before = e.selection();
    assert!(e
        .set_selection(Selection {
            anchor: pos(99, 0),
            focus: pos(0, 0)
        })
        .is_err());
    assert!(e
        .set_selection(Selection {
            anchor: pos(0, 3),
            focus: pos(0, 0)
        })
        .is_err());
    assert_eq!(e.selection(), before);
}

#[test]
fn history_restores_selection_styles_and_offsets() {
    let mut e = Editor::new(markdown::parse("<!--offset:42--> - [x] **æøå**").unwrap());
    let original = e.document().clone();
    select(&mut e, pos(0, 1), pos(0, 2));
    let selection = e.selection();
    e.insert_plain_text("x");
    check(&e);
    assert_eq!(e.document().blocks[0].created_offset_ms, Some(42));
    assert!(e.undo());
    check(&e);
    assert_eq!(e.document(), &original);
    assert_eq!(e.selection(), selection);
    assert!(e.redo());
    check(&e);
    select(&mut e, pos(0, 3), pos(0, 3));
    e.enter();
    check(&e);
    assert_eq!(e.document().blocks[1].kind, BlockKind::Checkbox(false));
    select(&mut e, pos(0, 0), pos(1, 0));
    let copied = markdown::parse(&e.copy_markdown()).unwrap();
    assert!(copied.blocks.iter().all(|b| b.created_offset_ms.is_none()));
}

proptest::proptest! {
    #[test]
    fn operation_sequences_preserve_round_trip_and_valid_cursor(operations in proptest::collection::vec((0u8..10, ".{0,12}"), 0..60)) {
        let mut e = Editor::new(Document::default());
        for (op, text) in operations {
            match op {
                0 => e.type_text(&text),
                1 => e.insert_plain_text(&text),
                2 => e.enter(),
                3 => e.backspace(),
                4 => e.toggle_style(Style::Bold),
                5 => e.toggle_style(Style::Italic),
                6 => { e.undo(); },
                7 => { e.redo(); },
                8 => { let end = e.selection().focus; select(&mut e, end, pos(0, 0)); },
                _ => { let p = e.selection().focus; select(&mut e, p, p); },
            }
            check(&e);
            markdown::parse(&e.copy_markdown()).unwrap();
        }
    }
}

#[test]
fn toggles_remove_both_styles_and_plain_paste_does_not_trigger_shortcuts() {
    let mut e = Editor::new(Document::default());
    e.backspace();
    check(&e);
    assert!(!e.undo());
    e.insert_plain_text("# ");
    check(&e);
    assert_eq!(e.document().blocks[0].kind, BlockKind::Paragraph);
    select(&mut e, pos(0, 0), pos(0, 2));
    e.toggle_style(Style::Bold);
    check(&e);
    e.toggle_style(Style::Italic);
    check(&e);
    assert!(e.document().blocks[0].spans[0].bold);
    assert!(e.document().blocks[0].spans[0].italic);
    e.toggle_style(Style::Bold);
    check(&e);
    e.toggle_style(Style::Italic);
    check(&e);
    assert!(!e.document().blocks[0].spans[0].bold);
    assert!(!e.document().blocks[0].spans[0].italic);
    e.backspace();
    check(&e);
    assert!(e.document().blocks[0].spans.is_empty());
    e.toggle_style(Style::Italic);
    check(&e);
    e.insert_plain_text("æ");
    check(&e);
    assert!(e.document().blocks[0].spans[0].italic);
    e.toggle_style(Style::Italic);
    check(&e);
    e.insert_plain_text("ø");
    check(&e);
    assert!(!e.document().blocks[0].spans[1].italic);
}
