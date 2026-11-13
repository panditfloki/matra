#!/bin/bash
# Fast dependency-free Rust + native Swift FFI contracts. All output stays under the supplied directory.
set -euo pipefail
script_dir=$(cd "$(dirname "$0")" && pwd -P)
core_dir=$(cd "$script_dir/../../matra-core" && pwd -P)
mac_dir=$(cd "$script_dir/.." && pwd -P)
output=${1:?Pass a contained test output directory}
mkdir -p "$output"
output=$(cd "$output" && pwd -P)
MATRA_RUSTC=${MATRA_RUSTC:-$(rustup which --toolchain 1.97.1 rustc)}
export MATRA_RUSTC
ARCHS=$(uname -m) "$script_dir/build-shared-core.sh" "$output/library"
mkdir -p "$output/tmp" "$output/module-cache"
export TMPDIR="$output/tmp"
"$MATRA_RUSTC" --edition=2021 --test "$core_dir/src/lib.rs" -o "$output/rust-contract"
"$output/rust-contract"
xcrun swiftc -D MATRA_CORE_STANDALONE_TEST -module-cache-path "$output/module-cache" \
    -import-objc-header "$core_dir/include/matra_core.h" \
    "$mac_dir/Sources/SharedCore/SharedCore.swift" "$mac_dir/Sources/Costs/Pricing.swift" \
    "$core_dir/tests/support/SwiftPricingAdapters.swift" "$core_dir/tests/swift-contract.swift" \
    -L "$output/library" -lmatra_core -o "$output/swift-contract"
"$output/swift-contract" "$core_dir/tests/fixtures"
