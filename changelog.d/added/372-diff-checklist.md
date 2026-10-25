- `bussard diff <A> <B> --checklist` prints a per-device Markdown checklist of
  the changes to redo by hand in ETS: group addresses first, then one section
  per device with its parameters on their ETS pages (an enabling parameter
  before the one it reveals, old values as ETS shows them), its objects and
  links and its security, then plain notes for what ETS cannot take by hand.
  `--checklist --json` emits the same structure. Link sentences of `diff`,
  `status` and the MCP tools now carry the device address (#372).
