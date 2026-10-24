- The KNX Data Secure invariants are validation codes (#348): W037 (a device
  on a secured group address is not activated), W039 (a Secure sender has no
  sequence number in the lock), W041 (the group flag and the object flags
  disagree; with W029 it names `bussard groups secure <ga> on|off` as the
  fix), W040 (a keyring export, or Home Assistant's keyring, is older than
  the key store's last change, recorded in `bussard.keys.meta.toml`), E038
  (a secured write to a tunnel-only group address whose devices admit no
  tunnelling address is refused while `admit_tunnel_senders` is off) and
  W042 (`bussard import` or `bussard keys import` secures a group address
  the model ran plain again, naming the devices that need an apply). Each
  code has a mutation test on a fixture project, and a corpus-gated test
  (`BUSSARD_ETS_PROJECTS`) checks that a clean ETS import validates without
  errors or warnings.
