#!/usr/bin/env python3
"""Changelog fragments for bussard.

A pull request never edits CHANGELOG.md: every PR would add lines at the same
spot of the Unreleased section and conflict with every other PR in flight.
Instead a PR adds one file under changelog.d/<section>/ holding the bullet(s)
exactly as they will appear in the changelog:

    changelog.d/fixed/326-ref-values.md
    - The plan no longer calls a value "ignored" that the flash writes (#326).
      Continuation lines are indented two spaces, as in CHANGELOG.md.

Sections, directory name -> heading: breaking, added, changed, deprecated,
removed, fixed, security, limitations.

    scripts/changelog.py check     validate every fragment (CI runs this)
    scripts/changelog.py render    print the Unreleased section with the
                                   fragments folded in; CHANGELOG.md untouched
    scripts/changelog.py collect   fold the fragments into CHANGELOG.md and
                                   delete them (before a release, or whenever
                                   the section should read as one text again)
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CHANGELOG = ROOT / "CHANGELOG.md"
FRAGMENTS = ROOT / "changelog.d"
SECTIONS = [
    ("breaking", "Breaking changes"),
    ("added", "Added"),
    ("changed", "Changed"),
    ("deprecated", "Deprecated"),
    ("removed", "Removed"),
    ("fixed", "Fixed"),
    ("security", "Security"),
    ("limitations", "Known limitations"),
]
DIR_TO_HEADING = dict(SECTIONS)
ORDER = [heading for _, heading in SECTIONS]
SKIP = {"README.md", ".gitkeep"}

Fragment = "tuple[Path, str]"


def load_fragments() -> "tuple[dict[str, list[Fragment]], list[str]]":
    """Fragments per heading in file-name order, and the problems found."""
    found: "dict[str, list[Fragment]]" = {}
    problems: list[str] = []
    if not FRAGMENTS.is_dir():
        return found, problems
    for path in sorted(FRAGMENTS.rglob("*")):
        if path.is_dir() or path.name in SKIP:
            continue
        rel = path.relative_to(FRAGMENTS)
        if len(rel.parts) != 2:
            problems.append(f"{rel}: fragments live in changelog.d/<section>/<name>.md")
            continue
        section = rel.parts[0]
        if section not in DIR_TO_HEADING:
            known = ", ".join(DIR_TO_HEADING)
            problems.append(f"{rel}: unknown section '{section}' (one of: {known})")
            continue
        if path.suffix != ".md":
            problems.append(f"{rel}: not a .md file")
            continue
        text = path.read_text(encoding="utf-8").strip("\n")
        lines = text.splitlines()
        if not lines or not lines[0].startswith("- "):
            problems.append(f"{rel}: must start with a markdown bullet ('- ')")
            continue
        bad = [
            n
            for n, line in enumerate(lines, 1)
            if line and not (line.startswith("- ") or line.startswith("  "))
        ]
        if bad:
            problems.append(f"{rel}: line {bad[0]} is neither a bullet nor indented two spaces")
            continue
        found.setdefault(DIR_TO_HEADING[section], []).append((path, text))
    return found, problems


def strip_blank(lines: list[str]) -> list[str]:
    """The lines without leading and trailing blank lines."""
    start, end = 0, len(lines)
    while start < end and not lines[start].strip():
        start += 1
    while end > start and not lines[end - 1].strip():
        end -= 1
    return lines[start:end]


def fold(text: str, frags: dict[str, list[Fragment]]) -> "tuple[str, str]":
    """CHANGELOG text with the fragments folded into Unreleased, and that section alone."""
    lines = text.split("\n")
    start = next((i for i, l in enumerate(lines) if l.startswith("## [Unreleased]")), None)
    if start is None:
        sys.exit("CHANGELOG.md has no '## [Unreleased]' section")
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")), len(lines))

    intro: list[str] = []
    sections: list[tuple[str, list[str]]] = []
    for line in lines[start:end]:
        if line.startswith("### "):
            sections.append((line[4:].strip(), []))
        elif sections:
            sections[-1][1].append(line)
        else:
            intro.append(line)

    for heading in ORDER:
        items = frags.get(heading)
        if not items:
            continue
        body = "\n".join(t for _, t in items).split("\n")
        existing = next((s for s in sections if s[0] == heading), None)
        if existing is not None:
            existing[1][:] = strip_blank(existing[1]) + body
            continue
        rank = ORDER.index(heading)
        pos = next(
            (i for i, (h, _) in enumerate(sections) if h in ORDER and ORDER.index(h) > rank),
            len(sections),
        )
        sections.insert(pos, (heading, body))

    out = strip_blank(intro) + [""]
    for heading, body in sections:
        out += [f"### {heading}", ""] + strip_blank(body) + [""]
    lines[start:end] = out
    return "\n".join(lines), "\n".join(out)


def main(argv: list[str]) -> int:
    if len(argv) != 2 or argv[1] not in {"check", "render", "collect"}:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    frags, problems = load_fragments()
    if problems:
        for p in problems:
            print(f"changelog.d: {p}", file=sys.stderr)
        return 1
    count = sum(len(v) for v in frags.values())
    if argv[1] == "check":
        print(f"changelog.d: {count} fragment(s) ok")
        return 0
    text = CHANGELOG.read_text(encoding="utf-8")
    folded, unreleased = fold(text, frags)
    if argv[1] == "render":
        print(unreleased)
        return 0
    CHANGELOG.write_text(folded, encoding="utf-8")
    for items in frags.values():
        for path, _ in items:
            path.unlink()
    for d in FRAGMENTS.iterdir():
        if d.is_dir() and not any(d.iterdir()):
            d.rmdir()
    print(f"changelog.d: folded {count} fragment(s) into CHANGELOG.md")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
