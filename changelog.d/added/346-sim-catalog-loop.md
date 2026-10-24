- `scripts/sim-catalog-loop.sh` and the corpus-gated `sim-catalog-loop` CI job
  flash every corpus application bussard and knx-sim support to a simulated
  device, with its defaults and seeded generated configurations, and require
  every plan after a flash, a partial `apply` and a re-flash to be a no-op
  (#346). knx-sim now keeps every segment a System 7 load-state machine
  allocates, accepts an in-place partial download of a loaded one, serves the
  device-object values a program's `LdCtrlCompareProp` expects, accepts a
  `PID_MCB_TABLE` seeding write while loading, accepts the property writes of a
  program's `LdCtrlWriteProp` steps, and places a relative segment that would
  cover a table's base above 0xFFFF.
