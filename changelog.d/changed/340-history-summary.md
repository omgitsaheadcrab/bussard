- `bussard history`, `history --json` and MCP `knx_history` read each
  snapshot's one-line summary from its manifest instead of loading two models
  per snapshot (#340). The summary is computed when the snapshot is taken; a
  snapshot from before this change gets it computed once on the first listing
  and written back.
