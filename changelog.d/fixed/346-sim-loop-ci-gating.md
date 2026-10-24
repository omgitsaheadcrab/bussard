- The `sim-catalog-loop` CI job no longer downloads vendor archives (#346
  follow-up): it runs only when `BUSSARD_PRODUCT_CORPUS` names a corpus, like
  the other corpus-gated jobs, and is skipped otherwise. A subset application
  whose archive is not in the corpus is reported as skipped, not failed.
