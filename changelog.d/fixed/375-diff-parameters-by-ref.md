- `diff` and `diff --checklist` compare parameters by ref, so one ref keyed
  differently on the two sides (`label@R`, `page-label@R`, `hidden@R`) is no
  change, or one change, instead of a set and an unset (#375). A `hidden[]`
  value of a ref the lock indexes is read under the visible key and written
  into the device file on the next save, and `knx_set_parameter` updates it
  instead of adding a second key for the ref.
