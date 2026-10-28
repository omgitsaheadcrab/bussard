- `bussard restart <ADDRESS>` restarts one device (nothing erased) behind the
  usual confirmation and real-gateway gate, in the form ETS's *Restart device*
  uses for it (#435, #452): the confirmed restart (master reset, erase code 1)
  for a secure-capable System B application, verified only by its
  `A_Restart_Response` with error code 0, else a bare `A_Restart`, reported as
  sent and not verified. Either way it then waits until the device answers
  again with the same mask. `--json` carries `form` and `confirmed`.
