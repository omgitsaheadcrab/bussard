- `bussard device`, `knx_show_device`, the parameter key resolver, the
  `set_parameter` refusal and the plan's "ignored" notes now take page and
  channel activity, parameter and object placement, values and conditions
  from the one Dynamic evaluator the flash uses, with the same values (#345).
  The view no longer marks a page active because the lock indexes a
  parameter on it. Internal parameters (`Access="None"`) are no longer
  offered as keys.
