#!/usr/bin/env python3
"""Keep docs/design.md in step with the GPUI app (issue #193).

Source text only, no build. Fails when:
  * a module in crates/tingskriver-app/src/lib.rs has no row in the doc,
  * a function returning `impl IntoElement` / `AnyElement` (other than
    `render`) or an `impl Render for X` type has no row,
  * the first column of a component table names something that is no longer
    in the app source,
  * AGENTS.md or CONTRIBUTING.md does not link docs/design.md, or the pull
    request template lacks the reuse field.

Run from anywhere: python3 scripts/check-design-doc.py
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "crates" / "tingskriver-app" / "src"
DOC = ROOT / "docs" / "design.md"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def component_section(doc: str) -> str:
    start = doc.index("\n## Components")
    end = doc.index("\n## Concepts")
    return doc[start:end]


def table_names(section: str) -> set:
    """Identifiers in the first column of the component tables."""
    names = set()
    for line in section.splitlines():
        m = re.match(r"\|\s*`([A-Za-z_][A-Za-z0-9_]*)`\s*\|", line)
        if m:
            names.add(m.group(1))
    return names


def code_components() -> set:
    found = set()
    lib = read(SRC / "lib.rs")
    found.update(re.findall(r"^\s*(?:pub\s+)?mod\s+([a-z_][a-z0-9_]*)\s*;", lib, re.M))
    for path in sorted(SRC.rglob("*.rs")):
        text = read(path)
        for m in re.finditer(r"\bfn\s+([a-z_][a-z0-9_]*)\b([^{;]*)\{", text):
            name, rest = m.groups()
            if name != "render" and re.search(r"->\s*(impl\s+IntoElement|AnyElement)\b", rest):
                found.add(name)
        found.update(re.findall(r"\bimpl\b(?:\s*<[^>]*>)?\s+(?:Render|RenderOnce)\s+for\s+([A-Za-z_][A-Za-z0-9_]*)", text))
    return found


def main() -> int:
    errors = []
    doc = read(DOC)
    section = component_section(doc)
    documented = table_names(section)
    in_code = code_components()
    sources = sorted(SRC.rglob("*.rs"))
    source = "\n".join(read(p) for p in sources)
    stems = {p.stem for p in sources}

    for name in sorted(in_code - documented):
        errors.append(f"docs/design.md: no row for `{name}` (add it to the component tables)")
    for name in sorted(documented - in_code):
        # Types and enums named in rows (Shell, Page, AppModel) are not found
        # by the scan above; they still have to be defined, not just mentioned.
        defined = re.search(
            rf"\b(?:fn|struct|enum|mod|type|trait|const|static)\s+{re.escape(name)}\b", source
        )
        if not defined and name not in stems:
            errors.append(f"docs/design.md: row `{name}` is not in crates/tingskriver-app/src any more")

    for rel in ("AGENTS.md", "CONTRIBUTING.md"):
        path = ROOT / rel
        if not path.exists():
            errors.append(f"{rel}: missing")
        elif "docs/design.md" not in read(path):
            errors.append(f"{rel}: does not link docs/design.md")
    template = ROOT / ".github" / "pull_request_template.md"
    if "## Components (GPUI app)" not in read(template):
        errors.append(".github/pull_request_template.md: no reused-or-new component field")

    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"design doc ok: {len(in_code)} components in code, {len(documented)} rows")
    return 0


if __name__ == "__main__":
    sys.exit(main())
