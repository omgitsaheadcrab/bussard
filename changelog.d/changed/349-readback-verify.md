- Verified means read back and compared, for every write (#349). `apply` (CLI
  and the MCP job) ends with a fresh read of the tables, the written
  parameters and, on a secured write, the security object (the PID 53 entry
  count, PID 54 and PID 61), compared with the model; anything left is
  `verified: false` with `readback.leftovers` naming each item, and the CLI
  exits non-zero. A flash's read-back now also compares the links, both table
  sizes and the group-object count, and runs after an unverified flash too,
  so the failure names what differs. `bussard keys export` reads the written
  file back, decrypts it, compares its keys with the store and prints the
  group addresses it carries and the ones it omits. One routine,
  `bussard_service::verify`, serves every surface.
