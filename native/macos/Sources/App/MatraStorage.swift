import Foundation

/// App-owned state and services follow the bundle identity, never an environment
/// flag. A development build can coexist with the installed product.
struct MatraChannel: Equatable {
    let namespace: String
    let supportFolder: String
    let pluginStem: String
    let relayPort: Int
    let phonePort: Int
    let isProduction: Bool

    init(bundleID: String?) {
        switch bundleID {
        case MatraStorage.installedBundleID:
            namespace = MatraStorage.installedBundleID
            supportFolder = "Matra"; pluginStem = "matra"
            relayPort = 11435; phonePort = 8788; isProduction = true
        case MatraStorage.previewBundleID:
            namespace = MatraStorage.previewBundleID
            supportFolder = "Matra Preview"; pluginStem = "matra-preview"
            relayPort = 11437; phonePort = 8790; isProduction = false
        case MatraStorage.developmentBundleID:
            namespace = MatraStorage.developmentBundleID
            supportFolder = "Matra Development"; pluginStem = "matra-development"
            relayPort = 11436; phonePort = 8789; isProduction = false
        default:
            // Unknown identities must never inherit the installed product's state.
            namespace = "com.dydxfx.matra.mac.unidentified"
            supportFolder = "Matra Unidentified"; pluginStem = "matra-unidentified"
            relayPort = 0; phonePort = 0; isProduction = false
        }
    }

    /// Where this identity's Ollama speed relay listens.
    var relayAddress: String { "http://127.0.0.1:\(relayPort)" }

    func allowsUpdates(underTest: Bool, sample: Bool) -> Bool {
        isProduction && !underTest && !sample
    }
}

enum MatraStorage {
    static let installedBundleID = "com.dydxfx.matra.mac"
    static let previewBundleID = "com.dydxfx.matra.mac.preview"
    static let developmentBundleID = "com.dydxfx.matra.mac.development"
    static let channel = MatraChannel(bundleID: Bundle.main.bundleIdentifier)
    static let isPreviewIdentity = Bundle.main.bundleIdentifier == previewBundleID
    static let isDevelopmentIdentity = Bundle.main.bundleIdentifier == developmentBundleID
    static let namespace = channel.namespace
    static let keychainAccount = "matra"

    static func service(_ name: String) -> String { "\(namespace).\(name)" }

    static func supportRoot(in applicationSupport: URL) -> URL {
        applicationSupport.appendingPathComponent(channel.supportFolder, isDirectory: true)
    }

    /// Called on an explicit Open folder action, never during view rendering.
    static func prepareSupportRoot(in applicationSupport: URL,
                                   fileManager: FileManager = .default) throws -> URL {
        let folder = supportRoot(in: applicationSupport)
        try fileManager.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }
}
