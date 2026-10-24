- The Dynamic evaluator (`evaluate_dynamic`) and its tree types moved from
  `bussard-ets` into `bussard-model` behind a `DynamicSource` trait, which the
  parsed product XML implements (#345). `bussard_ets::dynamic` re-exports it,
  so download, flash and plan behave exactly as before: the parameter images
  of every fixture application are byte-identical. The evaluation also lists
  every reached channel and parameter block, and a new value-free `RefIndex`
  records where each parameter and object ref sits and behind which `<when>`
  gates; nothing reads either yet.
