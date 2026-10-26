- The offline oracle compares PID 54 (the security individual address table)
  by sender address and reports differing sequence numbers as a note
  (`equal (sequence numbers differ: ETS …, bussard …)`), and compares PID 53
  (the group key table) per entry, naming the group whose key differs or is
  missing (`0/0/6: key differs`). Both sides hash each key with a salt drawn
  per run, so no output holds a key or a hash of a bare key (#378).
