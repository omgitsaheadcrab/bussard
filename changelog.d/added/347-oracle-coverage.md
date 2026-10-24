- The offline oracle reports coverage (#347): of the octets bussard places, the
  share the ETS capture holds, per segment and per 256-octet page
  (`knxtrace imgdiff`, a coverage column in the `95-offline-oracle.sh`
  summary). A matching row below 50 %, or with a page that has placed octets
  and none compared, is "not comparable (thin)", never "identical".
  `flash --dry-run --dump-images` records the placed octets of each image in
  `plan.json`. Rows take a recorded `coverage` and `expect=`/`scope=` options,
  `--check` gates on them, and the corpus-gated `offline_oracle` test runs it
  (`BUSSARD_ORACLE_ROWS`). The script now plans the bussard side with the
  model's key store (`--keyring` only decodes the capture) and offers
  `--no-admit`.
