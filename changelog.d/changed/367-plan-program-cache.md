- `bussard plan` on a large program is faster (#367). The parameter read-back
  evaluates the application's Dynamic section once per set of values and
  shares it between the read-back plan, the decode, the group-object check and
  the octet attribution; a pinned archive is hashed once per process (keyed by
  path, size and mtime); the parsed-product cache is decoded from memory with
  the JSON parser optimized in debug builds; and a plan with nothing to write
  skips the octet attribution. On a one-device model of the ABB
  `M-0002_A-A0ED-10-9B4E` program against knx-sim, a debug `plan` with
  nothing to write drops from about 6.0 s to 2.4 s with the 159 MB vendor
  archive. `plan --timing` shows the new `parameter read` phase. Nothing
  written to a device changes. Old `.bussard/products/` entries are no longer read and are
  pruned as usual.
