// What the Swift side of the app can see of the C side.
//
// Vendored Zstandard decoding and the platform-independent Rust business rules.
// Both expose a narrow C ABI; native SwiftUI/AppKit presentation stays native.
#import "Vendor/zstd/MatraZstd.h"
#import "../../matra-core/include/matra_core.h"
