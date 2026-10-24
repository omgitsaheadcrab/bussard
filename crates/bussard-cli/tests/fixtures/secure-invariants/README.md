# secure-invariants

A small, consistent KNX Data Secure model for the mutation tests of
`tests/it/validate_invariants_cli.rs` (issue #348). Everything is invented.

- `1/2/3` is secured. The Secure-activated `1.1.10` sends on it (object 21)
  and listens to it (object 22). The synthetic keyring
  `knx-sim/examples/secure/synthetic.knxkeys` holds its group key and the tool
  key of `1.1.10`, and assigns `1/2/3` to the tunnelling address `1.1.200`.
- `1/2/4` is plain. The push button `1.1.20` (no Data Secure) sends on it and
  `1.1.10` listens to it (object 23).

With that keyring imported as the key store, `bussard validate` reports no
error and no warning. Each test copies the directory, breaks one invariant the
way a user would, and asserts the code that names it.
