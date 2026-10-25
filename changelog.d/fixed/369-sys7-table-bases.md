- `plan`, `apply` and the read-back after `flash` read a System 7 device's
  link tables where its product puts them when the device does not answer
  `PID_TABLE_REFERENCE`, not at the fixed `0x4000` / `0x4201`. Products with
  the association table at `0x41FF` (Jung, Siemens), `0x41D5` or `0x4041`
  read back as missing every link, and `apply` would have written there (#369).
