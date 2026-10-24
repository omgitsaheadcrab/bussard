- The MCP server runs the source-address probe once per tunnel instead of on
  every `knx_plan_device`, `knx_apply_device` and `knx_restore_parameters`
  call (#341). The second plan on one connection no longer waits the 600 ms
  probe window; a reconnect or a re-established tunnel probes again, and an
  occupied address still refuses every call. CLI commands open a fresh tunnel
  each time and still probe once per command.
