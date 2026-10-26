- A System 7 load procedure whose `<choose>` branch allocates an absolute
  segment in parts now streams each allocation the part of the image it holds
  (#413). The MDT central operation unit `A-0054` with `Switching times in the
  device` = 1 allocates `0x47E4` as 899, 2966 and 578 octets of its 5406;
  bussard streamed the whole image into the first allocation and retried until
  the timeout. The octets no allocation holds are the device's and are never
  written. Where the configuration places a parameter in them, the plan
  refuses, as it does for a System B `<choose>` that writes a segment in part
  (#402).
