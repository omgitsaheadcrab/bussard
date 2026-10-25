- knx-sim serves what the 24 unclassified sim-loop applications need from a
  real device: the properties and segment of a paired ABB/Busch-Jaeger PEI
  program (object 5 PID 16 and 13), a load completed without a TaskSegment
  when the vendor procedure has none (IRSC/Zennio 0701), and the factory
  memory a `CompareMem` checks. The baseline lists none of them (#346).
