- A System 7 flash of a program with `DynamicTableManagement` (the Jung
  Schaltaktor `A-2088`, the Theben Meteodata `A-140C`) lays out its tables as
  ETS does (#383): the address-table segment is sized to the table, and the
  association table sits directly behind it, up to the end of the product's
  LSM 2 segment. `plan`, `apply`, the flash read-back and the MCP tools read
  and write the association table there when the device does not report its
  table reference.
