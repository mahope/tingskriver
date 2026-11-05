use super::*;

fn span(start_ms: u64, slut_ms: u64, nummer: usize) -> SpeakerSpan {
    SpeakerSpan {
        start_ms,
        slut_ms,
        nummer,
    }
}

#[test]
fn renumbered_ids_map_back_to_the_old_numbers() {
    let old = [span(0, 10_000, 1), span(10_000, 20_000, 2)];
    // The provider numbers the same two people the other way round and the
    // boundaries have moved a little.
    let new = [span(0, 9_500, 2), span(9_500, 20_000, 1)];
    let map = reconcile_speakers(&old, &new);
    assert_eq!(map[&2], 1);
    assert_eq!(map[&1], 2);
}

#[test]
fn overlap_below_threshold_gets_a_fresh_number() {
    let old = [span(0, 10_000, 1)];
    // Only 50 % of the new speaker's time falls on old speaker 1.
    let new = [span(5_000, 15_000, 1)];
    let map = reconcile_speakers(&old, &new);
    assert_ne!(map[&1], 1, "must not inherit the old number");
    assert_eq!(map[&1], 2);
}

#[test]
fn exactly_the_threshold_is_accepted() {
    let old = [span(0, 6_000, 3)];
    let new = [span(0, 10_000, 1)];
    assert_eq!(reconcile_speakers(&old, &new)[&1], 3);
}

#[test]
fn collisions_keep_the_best_match_and_the_other_gets_a_new_number() {
    let old = [span(0, 10_000, 1)];
    // Both new speakers mostly overlap old 1 (they are split halves of it).
    let new = [span(0, 6_500, 1), span(6_500, 10_000, 2)];
    let map = reconcile_speakers(&old, &new);
    assert_eq!(map[&1], 1);
    assert_eq!(map[&2], 2);
    assert_eq!(map.values().collect::<BTreeSet<_>>().len(), 2);
}

#[test]
fn fresh_numbers_never_reuse_an_old_number() {
    let old = [
        span(0, 1_000, 1),
        span(1_000, 2_000, 2),
        span(2_000, 3_000, 3),
    ];
    let new = [span(10_000, 11_000, 1)];
    assert_eq!(reconcile_speakers(&old, &new)[&1], 4);
}

#[test]
fn corrections_transfer_by_overlap() {
    let corrections = [span(0, 4_000, 7), span(4_000, 8_000, 9)];
    let segments = [
        (100, 3_900),
        (3_000, 5_000),
        (4_100, 7_900),
        (9_000, 9_500),
        (5, 5),
    ];
    assert_eq!(
        transfer_corrections(&corrections, &segments),
        vec![Some(7), None, Some(9), None, None]
    );
}

#[test]
fn normalization_keeps_danish_letters() {
    assert_eq!(normalize_letters("Blåbærgrød, på Ærø!"), "blåbærgrødpåærø");
    assert_eq!(normalize_letters("ÆØÅ 12 - ok"), "æøå12ok");
}

#[test]
fn truncation_rule_counts_danish_letters_and_punctuation_is_free() {
    let old = "æøå ".repeat(100); // 300 letters
    assert!(is_truncated(&old, "æøå"));
    // Lost 200 but kept 50 % or more: not truncated.
    let old = "å".repeat(400);
    assert!(!is_truncated(&old, &"å".repeat(200)));
    assert!(is_truncated(&old, &"å".repeat(199)));
    // Fewer than 200 lost: not truncated even if half is gone.
    assert!(!is_truncated(&"ø".repeat(300), &"ø".repeat(120)));
    // Punctuation and spacing do not count toward the length.
    assert!(!is_truncated(&old, &format!("{} ,.!", "å".repeat(300))));
    // An ASCII-only filter would see 0 letters in both and never trigger.
    assert!(is_truncated(&"ø".repeat(500), ""));
}

#[test]
fn reserved_names_and_corrections_are_not_reused() {
    let old = [span(0, 1_000, 1)];
    let new = [span(10_000, 11_000, 9)];
    // Number 2 and 3 carry a name or a correction but have no old span.
    assert_eq!(reconcile_speakers_reserving(&old, &new, &[2, 3])[&9], 4);
    assert_eq!(reconcile_speakers(&old, &new)[&9], 2);
}
