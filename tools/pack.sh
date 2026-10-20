#!/bin/sh
# Packs a program crate kept outside this repository as a founding packs its own programs, and
# prints its code id. Run it from a checkout of the program's repository.
#   tools/pack.sh [--view VIEW_CRATE [--notifier]] PROGRAM_CRATE OUT
set -eu
usage() { echo "usage: tools/pack.sh [--view VIEW_CRATE [--notifier]] PROGRAM_CRATE OUT" >&2; exit 2; }
view_crate=
notifier=
if [ "${1:-}" = --view ]; then
    [ $# -ge 2 ] || usage
    view_crate=$2
    shift 2
    if [ "${1:-}" = --notifier ]; then
        notifier=yes
        shift
    fi
fi
[ $# -eq 2 ] || usage
program_crate=$1
out=$2
case $out in /*) ;; *) out=$(pwd)/$out ;; esac
CARGO=${CARGO:-cargo}
sdk=$(cd "$(dirname "$0")/.." && pwd)

package_and_root() {
    "$CARGO" metadata --locked --no-deps --format-version 1 --manifest-path "$1/Cargo.toml" | python3 -c '
import json, os, sys
metadata = json.load(sys.stdin)
manifest = os.path.realpath(sys.argv[1])
names = [p["name"] for p in metadata["packages"] if os.path.realpath(p["manifest_path"]) == manifest]
if not names:
    sys.exit(sys.argv[1] + " is no package of its workspace")
print(names[0])
print(metadata["workspace_root"])' "$1/Cargo.toml"
}

described=$(package_and_root "$program_crate")
program=$(printf '%s\n' "$described" | sed -n 1p)
root=$(printf '%s\n' "$described" | sed -n 2p)
stem=$(printf '%s' "$program" | tr - _)
target=${CARGO_TARGET_DIR:-$root/target}/pack/$program
mkdir -p "$target"
target=$(cd "$target" && pwd)
release=wasm32-unknown-unknown/release
sysroot=$(rustc --print sysroot)
rustc_commit=$(rustc -vV | sed -n 's/^commit-hash: //p')
cargo_home=${CARGO_HOME:-$HOME/.cargo}
wasm_rustflags="--remap-path-prefix=$root=/build --remap-path-prefix=$sdk=/sdk --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$sysroot=/rustc --remap-path-prefix=$sysroot/lib/rustlib/src/rust=/rustc/$rustc_commit"

wasm() {
    (cd "$root" && RUSTFLAGS="$wasm_rustflags" "$CARGO" "$@")
}

wasm build --locked --target wasm32-unknown-unknown --release --target-dir "$target/program" -p "$program" --features program
wasm build --locked --target wasm32-unknown-unknown --release --target-dir "$target/describe" -p "$program" --features describe
python3 "$sdk/tools/check-describe-abi.py" "$target/describe/$release/$stem.wasm"
(cd "$root" && "$CARGO" run --locked -q --target-dir "$target/native" -p "$program" --example interface -- "$target/interface")
set -- --code "$target/program/$release/$stem.wasm" --describe "$target/describe/$release/$stem.wasm" \
    --schema "$target/interface/interface.schema" --tools "$target/interface/interface.tools"

if [ -n "$view_crate" ]; then
    described=$(package_and_root "$view_crate")
    view=$(printf '%s\n' "$described" | sed -n 1p)
    view_root=$(printf '%s\n' "$described" | sed -n 2p)
    if [ "$view_root" != "$root" ]; then
        echo "pack.sh: $view is not in $program's workspace ($root)" >&2
        exit 2
    fi
    view_stem=$(printf '%s' "$view" | tr - _)
    wasm build --locked --target wasm32-unknown-unknown --release --target-dir "$target/view" -p "$view"
    cp "$target/view/$release/$view_stem.wasm" "$target/$view_stem.wasm"
    sh "$sdk/tools/view-gate.sh" "$view" "$target/$view_stem.wasm"
    set -- "$@" --view "$target/$view_stem.wasm"
    if [ -n "$notifier" ]; then
        notify_stack=$(sed -n 's/^NOTIFY_STACK = //p' "$sdk/Makefile")
        if [ -z "$notify_stack" ]; then
            echo "pack.sh: $sdk/Makefile sets no NOTIFY_STACK" >&2
            exit 2
        fi
        wasm rustc --locked --target wasm32-unknown-unknown --profile notify --target-dir "$target/notify" \
            -p "$view" --features notifier --lib --crate-type cdylib -- -C link-arg=-zstack-size="$notify_stack"
        cp "$target/notify/wasm32-unknown-unknown/notify/$view_stem.wasm" "$target/$view_stem.notify.wasm"
        sh "$sdk/tools/view-gate.sh" "$view.notify" "$target/$view_stem.notify.wasm" notifier
        set -- "$@" --notify "$target/$view_stem.notify.wasm"
    fi
fi

source="none: REPO names no forge repository"
if [ -n "${REPO:-}" ]; then
    source="none: the checkout has changes git sees, or is no git checkout"
    if changes=$(git -C "$root" status --porcelain 2>/dev/null); then
        if [ -z "$changes" ]; then
            head=$(git -C "$root" rev-parse HEAD)
            set -- "$@" --commit "$head" --repo "$REPO"
            source="$REPO $head"
        fi
    fi
fi
echo "source: $source"
"$CARGO" run --locked -q --release --manifest-path "$sdk/Cargo.toml" --target-dir "$target/founding" \
    -p founding --bin pack -- "$@" "$out"
