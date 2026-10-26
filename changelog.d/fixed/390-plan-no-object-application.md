- `bussard plan`, `apply` and the MCP plan and apply no longer refuse a device
  without links when its application has no group objects (#390): the Jung
  IPS 300 SREG KNX IP interface's empty table set is its correct state. `plan`
  then reads the properties its property-only load procedure writes and says
  which differ from the model; `apply` names `flash` as the command that
  writes them. A program with group objects keeps the empty-table refusal.
