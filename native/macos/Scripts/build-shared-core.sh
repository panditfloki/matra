#!/bin/bash
# Build the dependency-free Rust core for the native Mac target. Never installs a toolchain.
set -euo pipefail
script_dir=$(cd "$(dirname "$0")" && pwd -P)
core_dir=$(cd "$script_dir/../../matra-core" && pwd -P)
output=${1:-${DERIVED_FILE_DIR:?Set DERIVED_FILE_DIR or pass a contained output directory}/matra-core}
mkdir -p "$output/tmp"
output=$(cd "$output" && pwd -P)
export TMPDIR="$output/tmp"
required=1.97.1
if [ -n "${MATRA_RUSTC:-}" ]; then
    compiler=$MATRA_RUSTC
elif command -v rustup >/dev/null 2>&1; then
    compiler=$(rustup which --toolchain "$required" rustc)
else
    compiler=$(command -v rustc)
fi
actual=$("$compiler" --version)
case "$actual" in
    "rustc $required "*) ;;
    *) echo "Shared core requires existing Rust $required; found $actual. No toolchain was installed." >&2; exit 1 ;;
esac
architectures=${ARCHS:-$(uname -m)}
libraries=()
for arch in $architectures; do
    case "$arch" in
        arm64) target=aarch64-apple-darwin ;;
        x86_64) target=x86_64-apple-darwin ;;
        *) echo "Unsupported Mac architecture: $arch" >&2; exit 1 ;;
    esac
    mkdir -p "$output/$arch"
    MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-15.0} "$compiler" \
        --crate-name matra_core --crate-type staticlib --edition=2021 \
        --target "$target" -C opt-level=2 -C panic=abort \
        "$core_dir/src/lib.rs" -o "$output/$arch/libmatra_core.a"
    libraries+=("$output/$arch/libmatra_core.a")
done
if [ "${#libraries[@]}" -eq 1 ]; then
    cp "${libraries[0]}" "$output/libmatra_core.a"
else
    xcrun lipo -create "${libraries[@]}" -output "$output/libmatra_core.a"
fi
printf 'Shared core ABI 1: %s (%s)\n' "$output/libmatra_core.a" "$actual"
