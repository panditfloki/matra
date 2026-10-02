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
    /// Preview over the owner's real local logs (offline reader only). Quota
    /// polling and account discovery stay off, as in the sample preview.
    static let previewReadsLocalLogs = usesLocalLogPreview(environment: ProcessInfo.processInfo.environment)

    static func usesLocalLogPreview(environment: [String: String]) -> Bool {
        usesDesignPreview(environment: environment) && environment["MATRA_PREVIEW_DATA"] == "local"
    }
    /// Live preview: the normal live app (accounts, quotas, local logs) under
    /// the separate preview identity and its own storage, for judging
    /// production readiness. The installed app must be quit first. Updates,
    /// Phone Link and Claude token renewal stay off.
    static let isLivePreview = usesLivePreview(environment: ProcessInfo.processInfo.environment)

    static func usesLivePreview(environment: [String: String]) -> Bool {
        usesDesignPreview(environment: environment) && environment["MATRA_PREVIEW_DATA"] == "live"
    }
    /// The words every preview surface uses for what its readings are.
    static var previewDataLabel: String {
        isLivePreview ? "LIVE PREVIEW · Real accounts and quotas · Separate preview settings"
            : previewReadsLocalLogs ? "LOCAL-LOG PREVIEW · Real local logs · Quotas not polled"
            : "DESIGN PREVIEW · Sample data only · No accounts accessed"
    }
    static let isUnderTest: Bool =
        ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil
            || NSClassFromString("XCTestCase") != nil
}
