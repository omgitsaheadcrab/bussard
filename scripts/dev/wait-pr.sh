#!/usr/bin/env bash
# Wait for a pull request to land, event-driven instead of on a timer.
#
#   scripts/dev/wait-pr.sh <pr-number> [max-minutes]
#
# Run it as a background command; the harness wakes the session when it
# exits, so nothing polls on a cron. It watches the PR's checks until they
# finish, then waits for auto-merge to land the squash.
#
# Exit codes:
#   0  merged
#   1  a check failed (the failing checks are printed)
#   2  the PR is closed, conflicting, or has no auto-merge: it needs a hand
#   3  no checks appeared, or the merge did not land, within max-minutes
set -euo pipefail

pr=${1:?usage: wait-pr.sh <pr-number> [max-minutes]}
max=${2:-60}
deadline=$((SECONDS + max * 60))

state() {
  gh pr view "$pr" --json state,mergeStateStatus,autoMergeRequest \
    --jq '"\(.state) \(.mergeStateStatus) \(.autoMergeRequest != null)"'
}

# A freshly pushed head has no checks for a few seconds; `gh pr checks` would
# exit with "no checks reported" instead of waiting. Once checks exist it exits
# 0 (all green) or 8 (some pending or failed); anything else means none yet.
checks_present() {
  gh pr checks "$pr" >/dev/null 2>&1
  local rc=$?
  [ "$rc" -eq 0 ] || [ "$rc" -eq 8 ]
}
until checks_present; do
  read -r st _ _ < <(state)
  [ "$st" = MERGED ] && { echo "PR #$pr merged"; exit 0; }
  [ "$st" = CLOSED ] && { echo "PR #$pr closed without merging"; exit 2; }
  [ $SECONDS -ge $deadline ] && { echo "PR #$pr: no checks after $max min"; exit 3; }
  sleep 15
done

if ! gh pr checks "$pr" --watch --fail-fast --interval 20 >/dev/null 2>&1; then
  read -r st _ _ < <(state)
  [ "$st" = MERGED ] && { echo "PR #$pr merged"; exit 0; }
  echo "PR #$pr: checks failed"
  gh pr checks "$pr" 2>/dev/null | grep -Ev '\b(pass|skipping)\b' || true
  exit 1
fi

# Checks are green; auto-merge lands the squash within a minute or so.
while :; do
  read -r st mstate auto < <(state)
  case $st in
    MERGED) echo "PR #$pr merged"; exit 0 ;;
    CLOSED) echo "PR #$pr closed without merging"; exit 2 ;;
  esac
  if [ "$auto" != true ] || [ "$mstate" = DIRTY ]; then
    echo "PR #$pr: $mstate, auto-merge=$auto: needs a hand (rebase, or arm auto-merge)"
    exit 2
  fi
  [ $SECONDS -ge $deadline ] && { echo "PR #$pr: checks green but not merged after $max min ($mstate)"; exit 3; }
  sleep 20
done
