#!/usr/bin/env bash
#
# The flash-then-plan loop against knx-sim over the product catalog (#346).
#
# For every application of the product corpus that bussard flashes and knx-sim
# simulates (masks 07B0, 0705, 0701, and a property-only 0021 procedure,
# issue #390): flash the vendor defaults and N seeded
# generated configurations to a fresh simulated device, each followed by a
# plan that must be a no-op; then apply a partial change (parameters, one link
# added, one removed), plan, flash again, plan; then flash the first
# configuration over the last one, plan. The driver is the corpus-gated test
# crates/bussard-cli/tests/it/sim_catalog_loop.rs; knx-sim runs as a separate
# process on a loopback port, never linked into bussard.
#
# Usage:
#   BUSSARD_PRODUCT_CORPUS=$PWD/tests-support/product-corpus \
#     scripts/sim-catalog-loop.sh [--filter A-140C,A-A0ED] [--configs N]
#                                 [--seed S] [--shard k/n] [--update-baseline]
#
#   --filter           comma-separated substrings of an application id or
#                      archive name (default: every application)
#   --configs N        generated configurations per application (default 2)
#   --seed S           generator seed (default 346)
#   --shard k/n        run the k-th of n runner shards (0-based), for CI
#   --update-baseline  rewrite tests-support/product-corpus/sim-loop-baseline.json
#                      from this run's failures; review and commit the diff
#   --subset           only the curated applications below (the CI job); an
#                      application whose archive is not in the corpus is
#                      reported as skipped, not failed
#
# Reports land in target/sim-loop/report (one JSON per shard); the summary at
# the end lists every failing application with its first failing step.
# Without a corpus the script stops: vendor data is never committed, and the
# script never downloads it (tests-support/product-corpus/fetch.sh does). An
# archive missing from the corpus, or one that cannot be read, skips its
# applications and does not fail the run.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# The curated subset the required CI job runs (--subset): one line per
# application, "<order number>|<application id filter>" (the order number is
# what fetch.sh takes to put the archive in the corpus). The full
# corpus runs nightly (.github/workflows/sim-catalog-nightly.yml). Add a line
# to grow the subset; keep the job inside its 15-minute budget (ABB took
# 111 s, the Jung F50 41 s, the Meteodata 23 s locally).
SUBSET=(
  "2CDG 110 283 R0011|A-A0ED-10"       # ABB i-bus (159 MB archive, 07B0, big object 5)
  "52921ST|A-D142-21"                  # Jung F50 push-button (07B0, the house's)
  "1409207|A-140C-14-A512"             # Theben Meteodata 140 S (0701)
  "SCN-P360D3.03|A-0072-44-49AB"       # MDT 0705
  "SCN-P360D3.03|A-0076-44-685A"       # MDT 0705
  "IPS300SREG|A-0016-10-EBD7"          # Jung IPS 300 SREG (0021, property-only, the house's)
)

update_baseline=0
subset=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --filter)  export BUSSARD_SIM_LOOP_FILTER="$2"; shift 2 ;;
    --configs) export BUSSARD_SIM_LOOP_CONFIGS="$2"; shift 2 ;;
    --seed)    export BUSSARD_SIM_LOOP_SEED="$2"; shift 2 ;;
    --shard)   export BUSSARD_SIM_LOOP_SHARD="$2"; shift 2 ;;
    --update-baseline) update_baseline=1; shift ;;
    --subset)  subset=1; shift ;;
    -h|--help) sed -n '2,35p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1 (see --help)" >&2; exit 2 ;;
  esac
done

if [[ $subset -eq 1 ]]; then
  filter=""
  for line in "${SUBSET[@]}"; do filter="${filter:+$filter,}${line#*|}"; done
  export BUSSARD_SIM_LOOP_FILTER="$filter"
  # Read by the summary below, to name the subset applications that did not
  # run because their archive is not in the corpus.
  SIM_LOOP_SUBSET="$(printf '%s\n' "${SUBSET[@]}")"
  export SIM_LOOP_SUBSET
fi

if [[ -z "${BUSSARD_PRODUCT_CORPUS:-}" ]]; then
  echo "BUSSARD_PRODUCT_CORPUS is unset: point it at the product corpus (docs/testing.md)." >&2
  exit 2
fi

# Hermetic: never read the repository's .env, never reach a real gateway. The
# driver passes an explicit 127.0.0.1 gateway to every bussard command.
export BUSSARD_NO_DOTENV=1
unset BUSSARD_ALLOW_REAL_GATEWAY

# knx-sim is its own workspace with its own target/; build it first, then the
# bussard side (one cargo command at a time).
# Release: the sim parses the application's archive at every start (once per
# configuration), and a debug build takes 24.5 s on the 159 MB ABB i-bus
# archive where a release build takes 2.6 s. It builds in about 20 s, in
# knx-sim's own target directory.
cargo build --quiet --release --manifest-path "$REPO/knx-sim/Cargo.toml" --bin serve
export BUSSARD_KNX_SIM_BIN="$REPO/knx-sim/target/release/serve"

OUT="$REPO/target/sim-loop"
export BUSSARD_SIM_LOOP_REPORT="${BUSSARD_SIM_LOOP_REPORT:-$OUT/report}"
export BUSSARD_SIM_LOOP_CACHE="${BUSSARD_SIM_LOOP_CACHE:-$OUT/product-cache}"
rm -rf "$BUSSARD_SIM_LOOP_REPORT"
mkdir -p "$BUSSARD_SIM_LOOP_REPORT" "$BUSSARD_SIM_LOOP_CACHE"

started=$(date +%s)
status=0
( cd "$REPO" && cargo nextest run -p bussard-cli --profile corpus --no-fail-fast \
    --status-level fail --final-status-level fail \
    -E 'test(/^sim_catalog_loop::test_sim_catalog_loop_shard/)' ) || status=$?
elapsed=$(( $(date +%s) - started ))

python3 - "$BUSSARD_SIM_LOOP_REPORT" "$REPO/tests-support/product-corpus/sim-loop-baseline.json" \
  "$update_baseline" "$elapsed" <<'PY'
import glob, json, os, sys

report_dir, baseline_path, update, elapsed = sys.argv[1], sys.argv[2], sys.argv[3] == "1", sys.argv[4]
rows = []
for path in sorted(glob.glob(os.path.join(report_dir, "*.json"))):
    with open(path) as f:
        rows.extend(json.load(f))
rows.sort(key=lambda r: r["application"])
count = lambda o: sum(1 for r in rows if r["outcome"] == o)
print(f"\nsim-loop: {len(rows)} applications in {elapsed}s: {count('pass')} pass, "
      f"{count('fail')} fail ({sum(1 for r in rows if r['outcome'] == 'fail' and r['known'])} known), "
      f"{count('skip')} skip")
for r in rows:
    if r["outcome"] == "fail":
        mark = "known" if r["known"] else "NEW"
        detail = (r.get("detail") or "").strip().splitlines()
        print(f"  {mark:5} {r['application']} ({r['mask']}, {r['configurations']} cfg, "
              f"{r['seconds']:.0f}s): {r['step']}")
        for line in detail[:4]:
            print(f"        {line[:200]}")
skips = {}
for r in rows:
    if r["outcome"] == "skip":
        reason = (r.get("detail") or "").split(":")[0]
        skips[reason] = skips.get(reason, 0) + 1
for reason, n in sorted(skips.items(), key=lambda kv: -kv[1]):
    print(f"  skip  {n:4} x {reason}")
# --subset: an application with no row at all was never found, because its
# archive is not in the corpus (not fetched, or not readable). It is skipped,
# not failed. A sharded run only sees its own share, so it does not judge.
if not os.environ.get("BUSSARD_SIM_LOOP_SHARD"):
    for line in os.environ.get("SIM_LOOP_SUBSET", "").splitlines():
        order, _, app = line.partition("|")
        if app and not any(app in r["application"] for r in rows):
            print(f"  skip  {app} ({order}): archive not in corpus")
timed = [r for r in rows if r["outcome"] != "skip"]
if timed:
    total = sum(r["seconds"] for r in timed)
    print(f"  time  {total:.0f}s over {len(timed)} applications run, "
          f"{total / len(timed):.1f}s each; slowest: "
          + ", ".join(f"{r['application']} {r['seconds']:.0f}s"
                      for r in sorted(timed, key=lambda r: -r["seconds"])[:3]))
if update:
    try:
        with open(baseline_path) as f:
            baseline = json.load(f)
    except FileNotFoundError:
        baseline = {}
    ran = {r["application"] for r in rows}
    known = {k: v for k, v in baseline.get("known_failures", {}).items() if k not in ran}
    for r in rows:
        if r["outcome"] == "fail":
            first = ((r.get("detail") or "").strip().splitlines() or [""])[0]
            entry = {"step": r["step"], "reason": first[:300]}
            old = baseline.get("known_failures", {}).get(r["application"], {})
            if old.get("step") == r["step"] and "issue" in old:
                entry["issue"] = old["issue"]
            known[r["application"]] = entry
    baseline["known_failures"] = dict(sorted(known.items()))
    with open(baseline_path, "w") as f:
        json.dump(baseline, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"  wrote {baseline_path} ({len(known)} known failures)")
PY

if [[ $update_baseline -eq 1 ]]; then
  exit 0
fi
exit $status
