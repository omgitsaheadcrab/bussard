- A System B load procedure now runs only the `<choose>` branches the device's
  parameters take, as the System 7 path already did, instead of every branch
  (#402). The MDT 07B0 programs that pick their hardware variant by parameter
  (`A-047A`, `A-04C0`, `A-018C`) planned one PID 78 or PID 162 compare per
  variant and could not pass the second; they now plan the chosen variant's
  compare. Nested `<choose>` blocks are decided recursively with the Dynamic
  evaluator's branch rules, and a self-closed `<when …/>` keeps its test.
- A `<choose>` whose taken branches write only part of a segment (the MDT
  JTA-01 `A-00A2` with its keep-data parameter set) is refused at plan time:
  ETS keeps the device's octets there, which neither the MCB check nor the
  read-back can verify (#402).
