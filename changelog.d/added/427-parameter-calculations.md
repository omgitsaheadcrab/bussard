- Parameter calculations: values a program computes with a `<ParameterCalculation>`
  script (VBScript or JavaScript), such as an Ekinex EK-E72's fan block or a
  Jung LED dimmer's dimming curve, are now computed and written as ETS does.
  A script outside the supported subset refuses the device's download, naming
  the calculation, instead of writing a stale value (#427).
