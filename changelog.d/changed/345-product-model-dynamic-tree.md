- Product models are format 5 (#345): each one carries its program's Dynamic
  tree (`dynamic`, `module_dynamics`), its unions, every parameter ref with its
  value and `Access="None"` mark, and the parameters' `base_value` and integer
  width, so the Dynamic evaluator runs over the model as it runs over the XML.
  Models of an older format regenerate from their archive at the next command;
  the device view is unchanged for now.
