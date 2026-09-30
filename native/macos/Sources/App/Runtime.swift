import Foundation

/// Whether this process is an `xcodebuild test` host rather than a Matra
/// somebody launched.
///
/// The unit bundle is hosted by the app itself, so a test run *is* a running
/// Matra — and anything that would reach outside the process has to ask
/// first. Without the check every test run put a live request on the usage
/// endpoint, and every `NotchWindowController` a test constructed ordered a
/// real panel onto the developer's screen: a single `make test` put forty of
/// them up at once, over whatever was being worked on, for the half minute the
/// suite took.
enum Runtime {
    // Live startup was explicitly approved after design review. A normal reopen
    // must remain a working meter; sample data is always an explicit opt-in.
    static let isDesignPreview = usesDesignPreview(environment: ProcessInfo.processInfo.environment)

    static func usesDesignPreview(environment: [String: String]) -> Bool {
        // Explicit preview wins even if an old live-launch flag was inherited.
        environment["MATRA_DESIGN_PREVIEW"] == "1"
    }
    static let isUnderTest: Bool =
        ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil
            || NSClassFromString("XCTestCase") != nil
}
