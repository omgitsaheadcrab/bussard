- A System 7 flash now matches ETS on the ABB 6119/20 (`A-A002`, mask 0701)
  (#386): a program that declares `ParameterByteOrder="LittleEndian"` gets its
  multi-octet integer parameters least significant octet first, and the
  group-object descriptor table is taken from the program's
  `<ComObjectTable CodeSegment Offset>` declaration, so its CONFIG octets get
  the project's flags (C cleared on unlinked objects) even when the
  descriptors point into controller RAM the plan does not allocate.
