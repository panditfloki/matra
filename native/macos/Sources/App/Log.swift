import os

/// An agent app has no window to print into, so anything worth diagnosing has
/// to go somewhere you can read it:
///
///     log stream --predicate 'subsystem == "com.dydxfx.matra.mac"' --level debug
enum Log {
    static let usage = Logger(subsystem: MatraStorage.namespace, category: "usage")
    static let sessions = Logger(subsystem: MatraStorage.namespace, category: "sessions")
}
