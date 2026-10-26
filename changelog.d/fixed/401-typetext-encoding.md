- A text parameter is written in the code page its application program
  declares in `<Options TextParameterEncoding>` (#401): MDT's ISO 8859-15
  labels such as the eight-character `Scènario` now fit their eight-octet
  field instead of being refused as nine UTF-8 bytes. A character the code
  page cannot represent, or a text longer than the field, is refused; a
  program that declares no encoding takes ASCII text only. `validate` measures
  text the same way, and the product model records the encoding
  (`model_format: 7`, regenerated on the next command).
