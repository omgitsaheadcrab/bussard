- A hidden value ETS stored for a parameter the current configuration does not
  reach (the Meteodata's C4 temperature refs while C4 is a brightness sensor)
  is no longer reported as an ignored override, listed in the device view or
  offered as a key; the flash still writes it (#380). Whether such a value
  counts as a visible setting is now decided by the Dynamic evaluation, not by
  the lock's parameter index.
