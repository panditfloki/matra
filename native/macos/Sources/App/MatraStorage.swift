import Foundation

/// Only storage owned by this app. Vendor session locations remain unchanged.
enum MatraStorage {
    static let namespace = "com.dydxfx.matra.mac"
    static let keychainAccount = "matra"

    static func service(_ name: String) -> String { "\(namespace).\(name)" }

    static func supportRoot(in applicationSupport: URL) -> URL {
        applicationSupport.appendingPathComponent("Matra", isDirectory: true)
    }

    /// Called on an explicit Open folder action, never during view rendering.
    static func prepareSupportRoot(in applicationSupport: URL,
                                   fileManager: FileManager = .default) throws -> URL {
        let folder = supportRoot(in: applicationSupport)
        try fileManager.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }
}
