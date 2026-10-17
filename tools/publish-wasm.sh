#!/bin/sh
# Publishes this commit's programs, describe modules, schemas and views as the GitHub release
# wasm-<commit>, unpacked and as the founding bundle `ducktape init` reads.
#   tools/publish-wasm.sh <artifact>...   (run by `make wasm-publish`)
set -eu
fail() { echo "wasm-publish: $*" >&2; exit 1; }
host=$(rustc -vV | sed -n 's/^host: //p')
[ "$host" = x86_64-unknown-linux-gnu ] || fail "the reference host is x86_64-unknown-linux-gnu, this is $host"
[ "$WASM_PROFILE" = release ] || fail "a release is the release profile's bytes, not WASM_PROFILE=$WASM_PROFILE's"
[ -z "$(git status --porcelain --untracked-files=no)" ] || fail "the checkout has uncommitted changes"
commit=$(git rev-parse HEAD)
[ -n "$(git branch --remotes --contains "$commit")" ] || fail "$commit is on no branch of origin"
tag="wasm-$commit"
! gh release view "$tag" >/dev/null 2>&1 || fail "$tag is published already"
"${MAKE:-make}" wasm-modules
out="$BUILD_TARGET/publish"
rm -rf "$out" && mkdir -p "$out"
(cd "$WASM_OUT" && sha256sum "$@" > "$out/SHA256SUMS" && tar czf "$out/programs-wasm.tar.gz" "$@" -C "$out" SHA256SUMS)
"${CARGO:-cargo}" run --locked --release -p founding -- "$WASM_OUT" "$out/founding"
# every file the bundle wrote into its fresh directory, which is every file the founding names
# and founding.toml (the founding crate's bundle test holds the two equal)
(cd "$out/founding" && set -- * && sha256sum "$@" > SHA256SUMS)
tar czf "$out/founding.tar.gz" -C "$out" founding
gh release create "$tag" "$out/programs-wasm.tar.gz" "$out/founding.tar.gz" --target "$commit" --latest=false \
  --title "wasm $(git rev-parse --short HEAD)" \
  --notes "Every program, describe module, schema and view at $commit, built by \`make wasm-publish\` on x86_64-unknown-linux-gnu (wasm-opt 132). programs-wasm.tar.gz holds them unpacked; founding.tar.gz holds a founding: founding.toml and every file it names (each program with its describe module, its schema and any view, and each program's params). Each archive carries its SHA256SUMS."
echo "published $tag"
