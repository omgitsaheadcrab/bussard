- `plan` and `apply` write a KNX IP interface's `[knxnet]` table (#391): the
  friendly name, the tunnel addresses, the static address set and the IP
  assignment become property writes on its KNXnet/IP parameter object (object
  type 11, by type as ETS writes the IPS 300 SREG), each read back and the
  object read again before `apply` says verified. The tunnel count changes
  only as far as the device's description of PID 53 allows. A change of the IP
  settings or the tunnel addresses (PID 53) of the interface bussard tunnels
  through is refused unless
  `apply --allow-self-reconfigure`. Over MCP, `knx_set_interface` edits the
  table with validation (E043 to E046, W047) and `knx_plan_device` /
  `knx_apply_device` write it through the same service functions. knx-sim's
  simulated interface takes the writes and needs no product file.
