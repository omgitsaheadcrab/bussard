- Product models are format 6: they no longer carry `blocks`, `parameter_when`
  and `ref_values`, and a parameter's `default` is its own value (a ref's value
  sits on the ref). The ETS pages are derived from the Dynamic tree when the
  model loads, with the same titles; models regenerate from their archive on
  the next command. The plan no longer calls a stored value "ignored" when the
  flash writes it as the source of an `<Assign>` (#345).
