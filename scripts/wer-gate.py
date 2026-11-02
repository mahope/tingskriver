#!/usr/bin/env python3
"""Compare micro-averaged WER. Exit 0 pass, 1 regression, 2 invalid/missing input."""
import argparse
from bisect import bisect_right
import json
from pathlib import Path
import re
import sys


class InvalidReport(ValueError):
    pass


def require(condition):
    if not condition:
        raise InvalidReport("Ugyldig eller usammenlignelig WER-rapport")


# Generated from Rust char::is_alphanumeric; Python str.isalnum uses a
# different Unicode property. The contract test checks every scalar value.
ALPHANUMERIC_RANGES = json.loads(
    Path(__file__).with_name("wer-unicode-alphanumeric.json").read_text(encoding="utf-8")
)
ALPHANUMERIC_STARTS = [start for start, _ in ALPHANUMERIC_RANGES]


def is_alphanumeric(character):
    codepoint = ord(character)
    index = bisect_right(ALPHANUMERIC_STARTS, codepoint) - 1
    return index >= 0 and codepoint <= ALPHANUMERIC_RANGES[index][1]


def words(text):
    return "".join(c if is_alphanumeric(c) else " " for c in text.lower()).split()


def validate(report):
    require(isinstance(report, dict))
    require(report.get("schema_version") == 1)
    require(report.get("normalization") == "unicode-alphanumeric-lower-v1")
    require(report.get("dataset_kind") in ("audio", "fixture-text"))
    for key in ("engine", "date", "git_sha"):
        require(isinstance(report.get(key), str) and bool(report[key].strip()))
    require(re.fullmatch(r"[0-9a-f]{40,64}", report["git_sha"]) is not None)
    require(isinstance(report.get("clips"), list) and bool(report["clips"]))
    totals = dict.fromkeys(("reference_words", "substitutions", "insertions", "deletions"), 0)
    clips = {}
    for clip in report["clips"]:
        require(isinstance(clip, dict))
        clip_id = clip.get("id")
        require(isinstance(clip_id, str) and re.fullmatch(r"[A-Za-z0-9_-]+", clip_id) is not None)
        require(clip_id not in clips)
        require(isinstance(clip.get("reference"), str) and isinstance(clip.get("hypothesis"), str))
        counts = clip.get("score")
        require(isinstance(counts, dict) and set(counts) == set(totals))
        for key, value in counts.items():
            require(type(value) is int and value >= 0)
            totals[key] += value
        n = counts["reference_words"]
        require(n == len(words(clip["reference"])) and n > 0)
        require(counts["substitutions"] + counts["deletions"] <= n)
        require(n - counts["deletions"] + counts["insertions"] == len(words(clip["hypothesis"])))
        check_wer(clip, counts)
        clips[clip_id] = clip
    require(report.get("total") == totals)
    check_wer(report, totals)
    return clips


def errors(counts):
    return sum(counts[k] for k in ("substitutions", "insertions", "deletions"))


def check_wer(row, counts):
    value = row.get("wer")
    require(type(value) in (int, float))
    require(abs(value - errors(counts) / counts["reference_words"]) < 1e-12)


def read_report(path):
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        validate(data)
        return data
    except (OSError, ValueError, TypeError, KeyError, OverflowError):
        # JSON exceptions may include transcript text; never print them.
        raise InvalidReport("Kan ikke læse en gyldig WER-rapport") from None


def compare(baseline, current):
    before = validate(baseline)
    after = validate(current)
    for key in ("schema_version", "normalization", "dataset_kind", "engine"):
        require(baseline[key] == current[key])
    require(before.keys() == after.keys())
    for clip_id in before:
        require(before[clip_id]["reference"] == after[clip_id]["reference"])
    b, c = baseline["total"], current["total"]
    # Exact integer arithmetic: precisely 1.0 percentage point passes.
    regression = (errors(c) * b["reference_words"] - errors(b) * c["reference_words"]) * 100 > c["reference_words"] * b["reference_words"]
    lines = ["# WER-sammenligning", "", f"Motor: `{current['engine'].replace('`', '')}`", "",
             "| Klip | Baseline | Aktuel | Ændring (pp) |", "|---|---:|---:|---:|"]
    for clip_id in sorted(before):
        old, new = before[clip_id]["wer"], after[clip_id]["wer"]
        lines.append(f"| {clip_id} | {old * 100:.3f}% | {new * 100:.3f}% | {(new - old) * 100:+.3f} |")
    old, new = baseline["wer"], current["wer"]
    lines.extend([f"| **Samlet** | **{old * 100:.3f}%** | **{new * 100:.3f}%** | **{(new - old) * 100:+.3f}** |", "",
                  "FEJL: forværring over 1,0 procentpoint." if regression else "BESTÅET: forværring højst 1,0 procentpoint.", ""])
    return regression, "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rapport", required=True, type=Path)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--markdown", type=Path)
    parser.add_argument("--skriv-baseline", action="store_true")
    args = parser.parse_args(argv)
    try:
        current = read_report(args.rapport)
        if args.skriv_baseline:
            args.baseline.write_text(json.dumps(current, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            print("Baseline skrevet. Kontrollér motor, modeller og testsæt før commit.")
            return 0
        if not args.baseline.exists():
            print("Baseline mangler. Kør --skriv-baseline på referencemaskinen.", file=sys.stderr)
            return 2
        regression, markdown = compare(read_report(args.baseline), current)
        if args.markdown:
            args.markdown.write_text(markdown, encoding="utf-8")
        print(markdown)
        return 1 if regression else 0
    except (InvalidReport, OSError):
        print("WER-gate: ugyldige filer, forskellige motorer, klip eller referencer (exit 2).", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
