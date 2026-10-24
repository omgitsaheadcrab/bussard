# Changelog fragments

A pull request does not edit `CHANGELOG.md`. It adds one file here, under the
directory of its section, holding the bullet(s) exactly as they will appear in
the changelog:

```
changelog.d/fixed/326-ref-values.md
```

```markdown
- The plan no longer calls a value "ignored" that the flash writes (#326).
  Continuation lines are indented two spaces, as in `CHANGELOG.md`.
```

Name the file after the issue (or PR) and a short slug. The sections are the
Keep a Changelog headings of the Unreleased section:

| directory | heading |
|---|---|
| `breaking` | Breaking changes |
| `added` | Added |
| `changed` | Changed |
| `deprecated` | Deprecated |
| `removed` | Removed |
| `fixed` | Fixed |
| `security` | Security |
| `limitations` | Known limitations |

`scripts/changelog.py check` validates the fragments and runs in CI.
`scripts/changelog.py render` prints the Unreleased section with every
fragment folded in. `scripts/changelog.py collect` writes that into
`CHANGELOG.md` and deletes the fragments; run it before a release, or whenever
the section should read as one text again.

Why: with a dozen PRs in flight, every one of them appended to the same lines
of `CHANGELOG.md`, and each pair conflicted. A rebase and a fresh CI run per
conflict was the single largest cost in the merge pipeline.
