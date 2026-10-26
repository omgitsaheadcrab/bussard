- The sim loop's configuration generator leaves a parameter at its vendor
  default when a `<choose>` of the load procedure tested on it holds a device
  compare (`LdCtrlCompareProp`, `LdCtrlCompareRelMem`, `LdCtrlCompareMem`):
  such a parameter states which hardware the project expects, and flipping it
  made the simulated device refuse the download like real hardware (#407).
