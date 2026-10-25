- Regenerating `.bussard/models/` writes only the models of the applications the
  lock's devices pin, parsing each archive once for them (#356). The first command
  in a one-device model of the ABB i-bus archive (407 programs, 159 MB) took 223 s
  and wrote 407 models; it now takes 6 s and writes one. `import` still writes every
  program of the archives it pins.
