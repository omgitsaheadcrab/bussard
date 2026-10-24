- Each product model is parsed once per process and cached as JSON under
  `.bussard/model-cache/`, kept in sync with its YAML by length and mtime
  (#339). On the reference house a debug `validate` drops from 16.3 s to
  about 2.3 s once the cache is warm, `status` from 11.2 s to about 2 s. The
  capability lookup uses the same cache; `.bussard/lookup/` is no longer
  written and can be deleted.
