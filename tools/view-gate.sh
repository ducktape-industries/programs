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
usage() { echo "usage: tools/view-gate.sh [--fast] <view> <artifact> [notifier]" >&2; exit 2; }
if [ "${1:-}" = --fast ]; then fast=yes; shift; fi
case $# in 2 | 3) ;; *) usage ;; esac
case ${3:-view} in view | notifier) ;; *) usage ;; esac
view=$1
artifact=$2
kind=${3:-view}
here=$(dirname "$0")
getrandom_internal_c_export=__getrandom_v03_custom
if wasm-tools print --skeleton "$artifact" | grep -Fq "(export \"$getrandom_internal_c_export\" "; then
    . "$here/wasm-features.sh"
    # shellcheck disable=SC2086 # one argument per feature in the shared list
    "${WASM_OPT:-wasm-opt}" "$artifact" $wasm_features --remove-exports \
        --pass-arg=remove-exports@"$getrandom_internal_c_export" -o "$artifact.private"
    mv "$artifact.private" "$artifact"
fi
if [ -z "$fast" ]; then
    WASM_OPT="${WASM_OPT:-wasm-opt}" "$here/optimize-view.sh" "$artifact"
fi
python3 "$here/check-view-abi.py" --"$kind" "$artifact"
echo "$view  $(wc -c < "$artifact")"
