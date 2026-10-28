- `flash` picks the restart form ETS uses from the product data (#452): only
  a System B application that declares `IsSecureEnabled` opens with the
  factory reset (erase code 7) and ends with the confirmed restart (erase
  code 1). Every other application, such as the KWL-KNX Connect (1.1.45) and
  the Steinel Control Pro (1.1.51), gets no master reset and ends with a bare
  `A_Restart`, as in ETS's captures. Before, every download with a filled
  segment got both.
