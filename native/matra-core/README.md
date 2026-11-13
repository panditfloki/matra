# Shared business core

`matra-core` is the dependency-free runtime used by the native Swift Mac model and the Rust Windows/Linux adapters. It builds as an `rlib` and a `staticlib`. This is executable business logic, not generated reference data.

The shared rules cover exact model identity, cache-aware API list-price estimates in INR, token allocation weights, known/unknown cost aggregation and Claude account headline/display ordering. Unknown prices, missing cache prices for used lanes, missing exchange rates and non-finite/overflowing amounts stay unknown. Zero tokens and explicitly free prices remain known zero. No prefix pricing is allowed. Account-suffixed window IDs cannot become another account's headline.

Production call sites:

- Mac: `Pricing.swift` (`Pricer` and `CostEstimator`), `CostAccounting.swift` (`CostValue`), `CostStore.swift` (SQL weight constants), `ClaudeOAuthProvider.swift` (`UsageResponse`). `SharedCore.swift` only translates values through `include/matra_core.h`.
- Windows/Linux: `costs.rs` (`PriceTable`, `Usage`, `Tally`), `usage.rs` (Claude display order), `main.rs` (`ring_window` Claude headline).

Provider transports, credentials, storage, source precedence, account aggregation, calendar boundaries, subscription allocation and UI remain platform adapters. These have different existing semantics and were not collapsed into one implementation. In particular, Mac's first matching stored price and Rust's explicit last-write/custom-price override remain their existing adapter policies. The Mac screens and native presentation are unchanged.

## Build and test

Cargo builds the local workspace dependency automatically. No third-party dependency was added. The native Mac Xcode target runs `macos/Scripts/build-shared-core.sh` before linking `libmatra_core.a`; `ARCHS` selects the Rust target(s). `MATRA_RUSTC` may name a contained compiler, otherwise the script locates the already-installed pinned Rust 1.97.1. It never downloads or installs tools. Intel or universal builds require that compiler's matching `x86_64-apple-darwin` standard library already be available. Output and temporary files stay in `DERIVED_FILE_DIR/matra-core`.

Run the portable Rust contracts with `cargo test -p matra-core --locked --offline`. On Mac, `native/macos/Scripts/test-shared-core.sh <contained-output-directory>` also links the actual Swift wrapper to the Rust static library and runs the same 33 TSV vectors plus 48 frozen pre-extraction native-Mac arithmetic cases. The fixture data is synthetic and requires no profiles, keychain or network. `SharedCoreTests` in the native app tests the actual Mac adapters. Rust `costs` tests include the same production-pricer vectors.

The C ABI is versioned (`matra_core_abi_version() == 1`), uses fixed-width structs and borrowed UTF-8 buffers, and never transfers heap ownership. The header and struct layouts are exercised by the Swift executable tests. Native device acceptance remains separate from these tests.
