//! Line diff and three-way merge for proposals. No dependencies: notes and
//! summaries are short, so a plain LCS is fast enough, with a size cap.

/// Above this many table cells the middle of the text is treated as replaced
/// instead of matched, so a huge input can never stall the UI thread.
const MAX_LCS_CELLS: usize = 4_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    Equal,
    Delete,
    Insert,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
    /// 1-based line number in the old text (not for inserted lines).
    pub old_line: Option<usize>,
    /// 1-based line number in the new text (not for deleted lines).
    pub new_line: Option<usize>,
}

/// Splits text into lines. Empty text has no lines (not one empty line).
pub fn lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').map(str::to_string).collect()
    }
}

pub fn join(lines: &[String]) -> String {
    lines.join("\n")
}

/// Matching line pairs `(index in a, index in b)` of a longest common
/// subsequence, in increasing order.
pub fn matches(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let mut prefix = 0;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < a.len() - prefix
        && suffix < b.len() - prefix
        && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let mut out: Vec<(usize, usize)> = (0..prefix).map(|i| (i, i)).collect();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    if !am.is_empty() && !bm.is_empty() && (am.len() + 1) * (bm.len() + 1) <= MAX_LCS_CELLS {
        let w = bm.len() + 1;
        let mut table = vec![0u32; (am.len() + 1) * w];
        for i in (0..am.len()).rev() {
            for j in (0..bm.len()).rev() {
                table[i * w + j] = if am[i] == bm[j] {
                    table[(i + 1) * w + j + 1] + 1
                } else {
                    table[(i + 1) * w + j].max(table[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < am.len() && j < bm.len() {
            if am[i] == bm[j] {
                out.push((prefix + i, prefix + j));
                i += 1;
                j += 1;
            } else if table[(i + 1) * w + j] >= table[i * w + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    out.extend((0..suffix).map(|k| (a.len() - suffix + k, b.len() - suffix + k)));
    out
}

pub fn diff_lines(a: &[String], b: &[String]) -> Vec<DiffLine> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    let push_until = |out: &mut Vec<DiffLine>, i: &mut usize, j: &mut usize, ei, ej| {
        while *i < ei {
            out.push(DiffLine {
                kind: DiffKind::Delete,
                text: a[*i].clone(),
                old_line: Some(*i + 1),
                new_line: None,
            });
            *i += 1;
        }
        while *j < ej {
            out.push(DiffLine {
                kind: DiffKind::Insert,
                text: b[*j].clone(),
                old_line: None,
                new_line: Some(*j + 1),
            });
            *j += 1;
        }
    };
    for (mi, mj) in matches(a, b) {
        push_until(&mut out, &mut i, &mut j, mi, mj);
        out.push(DiffLine {
            kind: DiffKind::Equal,
            text: a[mi].clone(),
            old_line: Some(mi + 1),
            new_line: Some(mj + 1),
        });
        i = mi + 1;
        j = mj + 1;
    }
    push_until(&mut out, &mut i, &mut j, a.len(), b.len());
    out
}

/// Unified diff text (`@@ -a,b +c,d @@` hunks with `context` lines around
/// each change). Empty when the texts are equal.
pub fn unified(a: &[String], b: &[String], context: usize) -> String {
    let all = diff_lines(a, b);
    let changed: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind != DiffKind::Equal)
        .map(|(i, _)| i)
        .collect();
    if changed.is_empty() {
        return String::new();
    }
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for i in changed {
        let (s, e) = (i.saturating_sub(context), (i + context + 1).min(all.len()));
        match ranges.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => ranges.push((s, e)),
        }
    }
    let mut out = String::from("--- base\n+++ proposed\n");
    for (s, e) in ranges {
        let old_before = all[..s].iter().filter(|l| l.old_line.is_some()).count();
        let new_before = all[..s].iter().filter(|l| l.new_line.is_some()).count();
        let old_n = all[s..e].iter().filter(|l| l.old_line.is_some()).count();
        let new_n = all[s..e].iter().filter(|l| l.new_line.is_some()).count();
        let start = |before: usize, n: usize| if n == 0 { before } else { before + 1 };
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            start(old_before, old_n),
            old_n,
            start(new_before, new_n),
            new_n
        ));
        for l in &all[s..e] {
            out.push(match l.kind {
                DiffKind::Equal => ' ',
                DiffKind::Delete => '-',
                DiffKind::Insert => '+',
            });
            out.push_str(&l.text);
            out.push('\n');
        }
    }
    out
}

/// Three-way merge: applies the changes `base -> theirs` onto `ours`.
/// `None` when both sides changed the same region differently.
pub fn merge3(base: &[String], ours: &[String], theirs: &[String]) -> Option<Vec<String>> {
    let mut to_ours = vec![None; base.len()];
    for (b, o) in matches(base, ours) {
        to_ours[b] = Some(o);
    }
    let mut to_theirs = vec![None; base.len()];
    for (b, t) in matches(base, theirs) {
        to_theirs[b] = Some(t);
    }
    let mut out = Vec::new();
    let (mut pb, mut po, mut pt) = (0, 0, 0);
    let syncs = (0..base.len())
        .filter_map(|b| Some((b, to_ours[b]?, to_theirs[b]?)))
        .chain(std::iter::once((base.len(), ours.len(), theirs.len())));
    for (b, o, t) in syncs {
        let (bc, oc, tc) = (&base[pb..b], &ours[po..o], &theirs[pt..t]);
        if oc == bc {
            out.extend_from_slice(tc);
        } else if tc == bc || oc == tc {
            out.extend_from_slice(oc);
        } else {
            return None;
        }
        if b < base.len() {
            out.push(base[b].clone());
        }
        (pb, po, pt) = (b + 1, o + 1, t + 1);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(s: &str) -> Vec<String> {
        lines(s)
    }

    #[test]
    fn empty_text_has_no_lines() {
        assert!(l("").is_empty());
        assert_eq!(l("a\n\nb").len(), 3);
    }

    #[test]
    fn equal_texts_have_empty_unified_diff() {
        assert_eq!(unified(&l("a\nb"), &l("a\nb"), 3), "");
    }

    #[test]
    fn unified_marks_changes_with_hunk_header() {
        let d = unified(&l("a\nb\nc"), &l("a\nB\nc"), 1);
        assert_eq!(
            d,
            "--- base\n+++ proposed\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n"
        );
    }

    #[test]
    fn pure_insert_into_empty_text() {
        let d = unified(&[], &l("x"), 3);
        assert_eq!(d, "--- base\n+++ proposed\n@@ -0,0 +1,1 @@\n+x\n");
    }

    #[test]
    fn far_apart_changes_make_two_hunks() {
        let a: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        let mut b = a.clone();
        b[1] = "x".into();
        b[18] = "y".into();
        assert_eq!(unified(&a, &b, 1).matches("@@ -").count(), 2);
    }

    #[test]
    fn merge_keeps_both_sides_when_changes_do_not_overlap() {
        let base = l("a\nb\nc\nd");
        let ours = l("a\nb\nc\nd\nuser");
        let theirs = l("a\nB\nc\nd");
        assert_eq!(
            merge3(&base, &ours, &theirs).unwrap(),
            l("a\nB\nc\nd\nuser")
        );
    }

    #[test]
    fn merge_reports_conflict_on_same_region() {
        assert!(merge3(&l("a\nb"), &l("a\nx"), &l("a\ny")).is_none());
    }

    #[test]
    fn merge_with_unchanged_ours_takes_theirs() {
        assert_eq!(merge3(&l("a\nb"), &l("a\nb"), &l("z")).unwrap(), l("z"));
    }

    #[test]
    fn matches_are_monotonic_and_equal() {
        let a = l("a\nb\nc\nd\ne");
        let b = l("b\nx\nd\ne\nf");
        let m = matches(&a, &b);
        assert_eq!(m, vec![(1, 0), (3, 2), (4, 3)]);
    }
}
