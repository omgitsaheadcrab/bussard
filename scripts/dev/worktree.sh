#!/usr/bin/env bash
# A fresh worktree off origin/main for one agent, warmed in the background.
#
#   scripts/dev/worktree.sh <name> <branch>
#
# Creates ../bussard-wt/<name> on a new branch <branch> from origin/main and
# starts `cargo check --workspace --all-targets` followed by
# `cargo build --workspace` in it, detached, logging to .warm.log. With
# sccache as the rustc wrapper the dependency crates come from the cache, so
# the warm-up is mostly the workspace crates themselves. An agent that starts
# a cargo command before the warm-up ends waits on the target lock; that is
# expected, not a hang.
#
# Prints the worktree path.
set -euo pipefail

name=${1:?usage: worktree.sh <name> <branch>}
branch=${2:?usage: worktree.sh <name> <branch>}
root=$(git rev-parse --show-toplevel)
dir=$(cd "$root/.." && pwd)/bussard-wt/$name

git -C "$root" fetch -q origin
git -C "$root" worktree add -q -b "$branch" "$dir" origin/main
(
  cd "$dir"
  nohup sh -c 'cargo check --workspace --all-targets && cargo build --workspace' \
    >"$dir/.warm.log" 2>&1 </dev/null &
)
echo "$dir"
