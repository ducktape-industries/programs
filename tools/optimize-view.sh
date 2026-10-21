#!/bin/sh
# Keep the manifest section and real imports/exports; optimize code only.
set -eu
optimizer=${WASM_OPT:-wasm-opt}
here=$(dirname "$0")
. "$here/wasm-features.sh"
case "$("$optimizer" --version)" in
    "wasm-opt version 132 "*) ;;
    *) echo "View builds require Binaryen wasm-opt 132" >&2; exit 1 ;;
esac
# Cargo can report Fresh after this script replaces its output. Reuse that
# optimized result instead of optimizing an already optimized program again.
if [ -f "$1.optimized" ] && cmp -s "$1" "$1.optimized"; then
    if [ "$1.optimized" -nt "$0" ] && [ "$1.optimized" -nt "$here/wasm-features.sh" ]; then
        exit 0
    fi
else
    cp "$1" "$1.unoptimized"
fi
# The bytes cargo produced, kept beside the optimized artifact for a size
# comparison (`wasm-tools objdump`); names are stripped in both, so
# `make wasm-why` builds `--profile why` for twiggy instead.
# shellcheck disable=SC2086 # one argument per feature in the shared list
preserved_cargo_input_for_script_or_feature_reoptimization=$1.unoptimized
"$optimizer" "$preserved_cargo_input_for_script_or_feature_reoptimization" -Oz --flatten --rereloop -Oz --converge \
    $wasm_features -o "$1.optimized"
cp "$1.optimized" "$1"
