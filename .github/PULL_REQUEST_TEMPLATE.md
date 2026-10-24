## What and why

<!-- One or two sentences. Link the issue: Closes #NNN -->

## Checklist

See "Device Writes" in `CLAUDE.md`. Strike out items that do not apply.

- [ ] Touches a device write? Reuses the path in `bussard-service` / `bussard-download` (name it), or says here why a new one is needed. CLI and MCP call the same function.
- [ ] Oracle row(s) in `95-offline-oracle.sh` named with their coverage figure, or a capture plan entry added (#347).
- [ ] The write ends with an independent read-back compared against the model (#349).
- [ ] Model-side Dynamic logic calls the shared evaluator (#345), no local copy.
- [ ] Tests run crate-scoped (`cargo nextest run -p <crate>`); CI covers the rest.
- [ ] Changelog fragment added under `changelog.d/`.
