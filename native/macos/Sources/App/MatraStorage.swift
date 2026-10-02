import Foundation

/// Only storage owned by this app. Vendor session locations remain unchanged.
enum MatraStorage {
    static let installedBundleID = "com.dydxfx.matra.mac"
    static let previewBundleID = "com.dydxfx.matra.mac.preview"
    /// A preview build never reads or writes the installed app's own keychain
    /// items or support folder. Vendor credential locations are not affected.
    static let isPreviewIdentity = Bundle.main.bundleIdentifier == previewBundleID
    static let namespace = isPreviewIdentity ? previewBundleID : installedBundleID
    static let keychainAccount = "matra"

    static func service(_ name: String) -> String { "\(namespace).\(name)" }

    static func supportRoot(in applicationSupport: URL) -> URL {
        applicationSupport.appendingPathComponent(isPreviewIdentity ? "Matra Preview" : "Matra", isDirectory: true)
    }

    /// Called on an explicit Open folder action, never during view rendering.
    static func prepareSupportRoot(in applicationSupport: URL,
                                   fileManager: FileManager = .default) throws -> URL {
        let folder = supportRoot(in: applicationSupport)
        try fileManager.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }
}
