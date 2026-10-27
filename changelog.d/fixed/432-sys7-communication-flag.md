- `apply` on a System 7 device switches on the communication flag of an
  object that gains its first link, and off for one that loses its last (#432).
  Before, the device ignored a new link on an object whose flag ETS had left
  off. `plan` lists each switch, and without product data names the new links
  whose flag it could not check.
