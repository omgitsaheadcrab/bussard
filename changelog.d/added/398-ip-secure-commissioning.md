- `bussard interface secure on|off <ia>` and the MCP tool `knx_interface_secure`
  switch KNXnet/IP Secure of a KNX IP interface without ETS (#398), in the
  sequence the ETS captures of 1.1.200 show: under the FDSK the security mode
  and the tool key, under the tool key the device authentication code, the
  password hashes, the tunnelling users and the secured service families, a
  master reset; `off` undoes it. Credentials come from and go to the key store
  only (missing ones generated and saved before the first write), every switch
  ends with a read-back after the restart, and `--dry-run --dump-images` feeds
  the offline oracle (`run=secure-on|secure-off` rows: identical against the
  ETS activation, 11 of 11 writes, and the decoded half of the deactivation).
  knx-sim models the interface's Secure properties, the tool-key switch, the
  gateway's switch to secure-only tunnelling at the master reset, and
  KNXnet/IP device management.
