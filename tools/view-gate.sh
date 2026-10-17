#!/bin/sh
# One view's (or notifier's) gate over the artifact cargo built: optimize
# (keeping the pre-wasm-opt bytes beside it), check the bare-WASM ABI, print
# the size. `--fast` skips wasm-opt, for a fast build (`WASM_PROFILE=fast`,
# the Makefile's), whose bytes never ship. Only that flag skips it, never the
# environment: every other caller (programs-lab's views) gets the bytes that
# ship.
#   tools/view-gate.sh [--fast] <view> <artifact> [notifier]
set -eu
fast=
if [ "$1" = --fast ]; then fast=yes; shift; fi
view=$1
artifact=$2
kind=${3:-view}
here=$(dirname "$0")
if [ -z "$fast" ]; then
    WASM_OPT="${WASM_OPT:-wasm-opt}" "$here/optimize-view.sh" "$artifact"
fi
python3 "$here/check-view-abi.py" --"$kind" "$artifact"
echo "$view  $(wc -c < "$artifact")"
