#!/bin/sh
# `make scaffold-check`: `make new-program` and `make new-view` still write a
# program and a view that build, gate and test. In a throwaway worktree of this
# checkout (HEAD plus its tracked edits), removed on any exit, it scaffolds
# both and runs what their author runs: `make dev` over the pair (program
# wasm, view wasm and its ABI gate, native tests) and the program's describe
# build and ABI check. It shares $CARGO_TARGET_DIR (default: this checkout's
# target/), and the worktree sits at a fixed path under it so a second run
# is warm. Unlocked: the scaffold adds workspace members to Cargo.lock.
set -eu
name=scaffold-check
repo=$(git rev-parse --show-toplevel)
target=${CARGO_TARGET_DIR:-$repo/target}
mkdir -p "$target"
# absolute: the make runs below run from the worktree
CARGO_TARGET_DIR=$(cd "$target" && pwd)
export CARGO_TARGET_DIR
wt=$CARGO_TARGET_DIR/scaffold-check/worktree

# a worktree a killed run left behind
git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
trap 'git -C "$repo" worktree remove --force "$wt"' EXIT
trap 'exit 1' INT TERM
# `stash create` commits the tracked edits (nothing: no commit) without
# touching the stash list or the tree.
edits=$(git -C "$repo" stash create)
git -C "$repo" worktree add --detach "$wt" "${edits:-HEAD}"
cd "$wt"

make new-program NAME=$name
make new-view NAME=$name-view
lists=$(printf "lists:\n\t@echo \$(PROGRAMS) / \$(VIEWS)\n" | make -s --no-print-directory -f Makefile -f - lists)
case " $lists " in *" $name "*" / "*" $name-view "*) ;; *) echo "the Makefile's lists miss $name or $name-view: $lists" >&2; exit 1 ;; esac
modules=$(${CARGO:-cargo} run -q -p founding --bin modules -- "$name")
test "$modules" = "VIEWS=$name-view NOTIFIERS=" || { echo "founding packs $name with $modules, not its view" >&2; exit 1; }
make dev P=$name V=$name-view
make wasm-describes PROGRAMS=$name
echo "scaffold-check: $name and $name-view build, gate and test"
