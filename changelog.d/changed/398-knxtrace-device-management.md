- knxtrace decodes KNXnet/IP device management connections (#398): the cEMI
  `T_Data_Connected` / `T_Data_Individual` frames ETS sends there and the local
  `M_PropRead` / `M_PropWrite` / `M_FuncProp*` / `M_Reset` services, filed under
  the interface that answered the description, so `ops`, `diff` and `image`
  record a device management write like a bus-level one. The IP Secure
  function properties (`PID_SECURED_SERVICE_FAMILIES`, `PID_SECURITY_MODE`)
  and the type 11 and 17 PIDs are named. `--keyring` is repeatable (the
  exports before and after a key rotation), credential writes (tool key,
  device authentication code, password hashes) are compared with the keyring
  by name only, and each KNXnet/IP Secure session is listed with its verified
  device authentication code and connection type: a secure session itself
  cannot be decrypted from a capture (ephemeral X25519 key). The offline
  oracle rows take `props=VERDICT` to gate the property-write parity of a
  property-only plan.
