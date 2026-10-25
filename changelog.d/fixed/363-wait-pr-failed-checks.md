- `scripts/dev/wait-pr.sh` reports a failed check within a minute instead of
  waiting out its deadline: `gh pr checks` exits 8 once a check has failed, which
  the "no checks yet" loop took for "no checks".
