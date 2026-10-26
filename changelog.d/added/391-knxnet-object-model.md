- KNX IP interfaces carry their KNXnet/IP settings as a `[knxnet]` table in the
  device file (IP assignment, static address, netmask and gateway, friendly name,
  tunnel addresses), imported from the project's `IPConfig` and
  `AdditionalAddresses` and written by `adopt` from the device (#391).
  `bussard status <ADDRESS>` reads a device live and shows an interface's IP
  address, assignment, MAC address, tunnels and state; `describe` and
  `knx_show_device` show them too, the read values are kept with the device
  facts, `diff` describes table changes in sentences, and `validate` checks the
  table (E043 to E046) and its tunnels against the key store (W047). Writing the
  settings to the device follows separately.
