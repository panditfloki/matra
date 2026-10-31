import Foundation

/// Installs and removes the opt-in OpenCode bridge. Nothing here runs on its
/// own: `install` and `uninstall` are only reached from an explicit Settings
/// action, and tests always pass a temporary directory.
///
/// One owned file per app identity, so a preview build can never replace the
/// installed app's bridge or send its activity to the other app's folder.
enum OpenCodePluginInstaller {
    enum Failure: Error {
        case missingResource
        /// Something already sits at the target and cannot be read, so it
        /// cannot be backed up. It is left alone.
        case unreadableExisting
        /// The bundled plugin lacks the support-folder line, so a preview copy
        /// would write into the installed app's folder.
        case unrecognizedTemplate
    }

    /// In the plugin's source. A file carrying it is this app's to remove.
    static let ownershipMarker = "matra-opencode-bridge"
    private static let supportFolderLine = #""Matra" /* matra:support-folder */"#
    private static let previewSupportFolderLine = #""Matra Preview" /* matra:support-folder */"#

    static var pluginsDirectory: URL {
        let configured = ProcessInfo.processInfo.environment["XDG_CONFIG_HOME"]?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let root = configured.flatMap { $0.hasPrefix("/") ? URL(fileURLWithPath: $0) : nil }
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".config")
        return root.appendingPathComponent("opencode/plugins")
    }

    static func pluginStem(isPreview: Bool = MatraStorage.isPreviewIdentity) -> String {
        isPreview ? "matra-preview" : "matra"
    }

    /// The bytes installed for an identity. The installed app's copy is the
    /// bundled file unchanged; a preview's differs only in its support folder.
    static func rendered(template: Data, isPreview: Bool) throws -> Data {
        guard isPreview else { return template }
        guard let text = String(data: template, encoding: .utf8), text.contains(supportFolderLine) else {
            throw Failure.unrecognizedTemplate
        }
        return Data(text.replacingOccurrences(of: supportFolderLine, with: previewSupportFolderLine).utf8)
    }

    /// Only called by the explicit Settings button. Preserve an earlier or
    /// user-edited file outside the loader's .js/.ts extension set.
    static func install(directory: URL = pluginsDirectory, source: URL? = nil,
                        isPreview: Bool = MatraStorage.isPreviewIdentity) throws {
        guard let source = source ?? Bundle.main.url(forResource: "matra-opencode", withExtension: "mjs")
        else { throw Failure.missingResource }
        let data = try rendered(template: try Data(contentsOf: source), isPreview: isPreview)
        let stem = pluginStem(isPreview: isPreview)
        let target = directory.appendingPathComponent("\(stem).js")
        let fileManager = FileManager.default
        try fileManager.createDirectory(at: directory, withIntermediateDirectories: true)
        if fileManager.fileExists(atPath: target.path) {
            guard let existing = try? Data(contentsOf: target) else { throw Failure.unreadableExisting }
            if existing == data { return }
            let backup = directory.appendingPathComponent("\(stem).\(UUID().uuidString).backup")
            try existing.write(to: backup, options: .atomic)
        }
        try data.write(to: target, options: .atomic)
    }

    /// Undoes `install`: removes the bridge only if it is still this app's, and
    /// puts back the newest file it displaced that was not an older bridge.
    /// Returns whether anything was removed. A target the user has since
    /// replaced with their own file stays exactly as it is.
    @discardableResult
    static func uninstall(directory: URL = pluginsDirectory,
                          isPreview: Bool = MatraStorage.isPreviewIdentity) throws -> Bool {
        let stem = pluginStem(isPreview: isPreview)
        let target = directory.appendingPathComponent("\(stem).js")
        guard let installed = try? Data(contentsOf: target), isOwned(installed) else { return false }
        let fileManager = FileManager.default
        try fileManager.removeItem(at: target)

        let backups = ((try? fileManager.contentsOfDirectory(
            at: directory, includingPropertiesForKeys: [.contentModificationDateKey]
        )) ?? []).filter {
            $0.lastPathComponent.hasPrefix("\(stem).") && $0.pathExtension == "backup"
        }
        let displaced = backups
            .filter { (try? Data(contentsOf: $0)).map { !isOwned($0) } ?? false }
            .max { modifiedAt($0) < modifiedAt($1) }
        if let displaced { try fileManager.moveItem(at: displaced, to: target) }
        return true
    }

    private static func isOwned(_ data: Data) -> Bool {
        data.range(of: Data(ownershipMarker.utf8)) != nil
    }

    private static func modifiedAt(_ url: URL) -> Date {
        (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate ?? .distantPast
    }
}
