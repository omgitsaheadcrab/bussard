- A System 7 `LdCtrlCompareMem` pre-flight compares its `Size` octets, not the
  whole padded `InlineData`, so the Zennio 0701 programs pass it (#370).
