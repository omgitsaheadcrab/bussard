# Agent brief

The standing instructions for an implementation agent on bussard. A
coordinator pastes a pointer to this file into every agent prompt, followed
by the task itself (issue number, worktree path, and any investigation report
to start from). Keep the task part short; everything that does not change
between tasks belongs here.

## Where you work

- Only in the worktree the prompt names, never in the main checkout
  (`/Users/tmbo/projects/bussard`) and never in another agent's worktree.
- Never write under `knx/`, `captures/` or `shared-with-windows/`. They are
  the owner's installation, in use by other processes.
- Never contact a real gateway. `bussard.yaml` defaults to the owner's house,
  so every bus test uses the `bussard-testkit` mocks on `127.0.0.1:0`, and
  every CLI invocation in a test passes an explicit loopback `--gateway`.

## Commands

Each crate's integration tests are one `it` binary since #329, so filter by
test name, not by binary:

```
cargo nextest run -p <crate> -E 'test(/<pattern>/)'    # while iterating
cargo nextest run -p <crate> --profile ci                # once, before the PR
cargo clippy -p <crate> --all-targets -- -D warnings
cargo fmt --all
cargo test -p <crate> --doc                              # only if you touched doc examples
cargo deny check                                         # only if you touched dependencies
```

Crate-scoped only. CI runs the workspace gate; do not duplicate it locally.
Filter output before it reaches your transcript (aim for under 50 lines per
command). Keep `[profile.dev] debug = "line-tables-only"`.

## Time budget

- Stop any single cargo command that runs longer than 15 minutes and report
  it instead of waiting. nextest's slow-timeout already kills a hung test.
- When the coordinator sends "status?", answer in three lines: the step you
  are on, what is left, and the longest-running command so far.
- If the brief turns out to be wrong about the code (a file moved, a helper
  has a different signature), say so in the report and continue with what is
  there; do not stop to ask.

## Code rules

CLAUDE.md applies in full. The ones agents most often miss:

- No `.unwrap()` or `.expect()` anywhere but a binary's `main`; tests return
  `Result` and use `?`.
- Doc comments on every public item; no wildcard imports outside test modules.
- No new dependency without a one-line justification in the PR body, and
  never a copyleft or git dependency.
- Changelog: never edit `CHANGELOG.md`. Add a fragment under
  `changelog.d/<section>/<issue>-<slug>.md` (see `changelog.d/README.md`).
- Unit tests in `#[cfg(test)]` at the bottom of the file, integration tests in
  the crate's `tests/it/`. Name them `test_<function>_<scenario>`.

## Commit and PR

1. Commit with the `Co-Authored-By` trailer your own system sets. Do not
   change it to another model's line.
2. `gh pr create` with a title in the imperative and a body that states what
   changed, why, how it was tested, and `Closes #<issue>`.
3. `gh pr merge --auto --squash`, then stop. Do not watch CI, do not rebase
   onto a moving main, do not touch other branches. If the push or the
   auto-merge command is blocked, say so in the report; the coordinator runs
   it.

## Final report

At most 150 words, in this shape:

```
PR: <url>, auto-merge on|off (<why if off>)
Gates: fmt ok, clippy ok, nextest <crate>: N passed (<profile>), doc-tests ok|skipped
Deviations from the brief: <one line each, or "none">
Open points: <one line each, or "none">
Files: <paths, one per line>
```

No narration of what you tried first, no restating the issue, no praise of
the design. A behaviour change the coordinator must know about is a
deviation; a thing you chose not to do is an open point.
