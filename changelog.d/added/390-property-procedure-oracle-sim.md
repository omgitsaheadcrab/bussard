- The offline oracle and knx-sim follow the property load procedures (#390):
  `knxtrace ops` names the extended property services (`prop-write
  type11.1/PID205`, `function-prop`), the KNXnet/IP Secure keys (PIDs 91 to
  93 of object type 11) are redacted like the Data Secure ones, and a
  property-only row lists ETS's KNX IP interface commissioning (objects 11 and
  17) without counting it and says when ETS did not run the load procedure.
  knx-sim serves a property-only device on mask 0021, the compares a
  procedure runs on objects other than the device object, writes by object
  type through the extended services, and the label objects a System 7
  procedure writes; the sim catalog loop runs the 0021 program. A
  type-addressed write that the device leaves unanswered falls back to the
  object index from `PID_IO_LIST`. Masks 0020, 0021 and 0025 are now labelled
  System 2 (BCU2), not System 1.
