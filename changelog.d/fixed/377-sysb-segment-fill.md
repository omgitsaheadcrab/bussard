- A System B relative segment that carries no `<Data>` now reads `0xFF` in
  every octet no parameter writes, as ETS writes it, instead of `0x00` (#377).
  The flash, the dry-run images and the read-back compare all see the same
  image; the Jung IP interface additional function (`A-0024-22`) is now
  byte-identical to its ETS download.
