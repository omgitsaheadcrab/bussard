- `status`, `describe` and `adopt` find the KNXnet/IP parameter object of an IP
  interface whose object-index walk does not list it, such as the Jung IPS 300
  SREG at mask 0021 (#391). They look in `PID_IO_LIST`, then probe object type
  11 by type and instance with one short extended-property read, which keeps
  the connection when the device leaves it unanswered. A device found without
  the object is recorded in its device facts, so later commands skip the probe.
