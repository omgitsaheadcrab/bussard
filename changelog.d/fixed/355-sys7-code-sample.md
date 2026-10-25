- `bussard flash` no longer refuses a System 7 configuration flashed over
  another configuration of the same application (#355). The identity check
  sampled the group-object descriptor table as code, whose CONFIG octets
  follow the links, so the Theben Meteodata 140 S and 13 MDT 0705 programs
  read as "another application". It now samples only the segments streamed
  verbatim from the product; a product with none (the Meteodata) is judged by
  its load-state machines alone, and the pre-flight says so.
