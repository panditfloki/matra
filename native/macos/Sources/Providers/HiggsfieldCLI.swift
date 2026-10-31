import Foundation

/// Integrates with the official `@higgsfield/cli` (`hf` / `higgsfield`) binary.
struct HiggsfieldCLI: Sendable {
    static let timeout: TimeInterval = 10

    private static let systemPaths = [
        "/opt/homebrew/bin/hf",
        "/usr/local/bin/hf",
        "/opt/homebrew/bin/higgsfield",
        "/usr/local/bin/higgsfield"
    ]

    static func locateBinary(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        fileManager: FileManager = .default
    ) -> URL? {
        if let override = environment["HIGGSFIELD_CLI_PATH"]?.trimmingCharacters(in: .whitespacesAndNewlines),
           !override.isEmpty {
            let expanded = (override as NSString).expandingTildeInPath
            if isRunnable(expanded, fileManager: fileManager) {
                return URL(fileURLWithPath: expanded)
            }
            return nil
        }

        let home = environment["HOME"].map { URL(fileURLWithPath: $0) }
            ?? fileManager.homeDirectoryForCurrentUser

        let candidates = [
            home.appendingPathComponent(".local/bin/hf"),
            home.appendingPathComponent(".local/bin/higgsfield"),
            home.appendingPathComponent(".nvm/current/bin/hf"),
            home.appendingPathComponent(".nvm/current/bin/higgsfield")
        ] + systemPaths.map { URL(fileURLWithPath: $0) }

        if let found = candidates.first(where: { isHiggsfield($0, fileManager: fileManager) }) {
            return found
        }

        return searchPATH(environment: environment, fileManager: fileManager)
    }

    /// `hf` is also the name of Hugging Face's CLI, so a binary by that name
    /// counts only when it resolves into Higgsfield's own package. A
    /// `higgsfield` binary is unambiguous.
    static func isHiggsfield(_ url: URL, fileManager: FileManager = .default) -> Bool {
        guard isRunnable(url.path, fileManager: fileManager) else { return false }
        if url.lastPathComponent == "higgsfield" { return true }
        return url.resolvingSymlinksInPath().path.lowercased().contains("higgsfield")
    }

    private static func isRunnable(_ path: String, fileManager: FileManager) -> Bool {
        var isDir: ObjCBool = false
        return fileManager.fileExists(atPath: path, isDirectory: &isDir)
            && !isDir.boolValue
            && fileManager.isExecutableFile(atPath: path)
    }

    private static func searchPATH(
        environment: [String: String],
        fileManager: FileManager
    ) -> URL? {
        guard let path = environment["PATH"] else { return nil }
        for dir in path.split(separator: ":").map(String.init) {
            guard dir.hasPrefix("/") else { continue }
            for name in ["hf", "higgsfield"] {
                let candidate = URL(fileURLWithPath: (dir as NSString).appendingPathComponent(name))
                if isHiggsfield(candidate, fileManager: fileManager) {
                    return candidate
                }
            }
        }
        return nil
    }

    /// What the CLI runs with. A menu-bar app inherits launchd's short PATH,
    /// where an npm-installed `hf` cannot find `node`. Hugging Face's own
    /// variables stay behind: their tools share the `HF_` prefix, and a
    /// Hugging Face secret must not reach Higgsfield's binary.
    static func childEnvironment(binary: URL,
                                 base: [String: String] = ProcessInfo.processInfo.environment) -> [String: String] {
        var env = base.filter { key, _ in
            let name = key.uppercased()
            return !name.hasPrefix("HF_") && !name.hasPrefix("HUGGING_FACE_") && !name.hasPrefix("HUGGINGFACE_")
        }
        env["TERM"] = "dumb"
        var path = [binary.deletingLastPathComponent().path, "/opt/homebrew/bin", "/usr/local/bin"]
        path += (base["PATH"] ?? "/usr/bin:/bin:/usr/sbin:/sbin").split(separator: ":").map(String.init)
        var seen = Set<String>()
        env["PATH"] = path.filter { seen.insert($0).inserted }.joined(separator: ":")
        return env
    }

    /// Why a failed run failed, from what it printed on either stream. The text
    /// is only classified, never shown or kept.
    static func failure(status: Int32, output: String) -> UsageProviderError {
        if output.localizedCaseInsensitiveContains("rate limit") ||
           output.localizedCaseInsensitiveContains("too many requests") ||
           output.contains("429") {
            return .rateLimited(retryAfter: 60)
        }
        if output.localizedCaseInsensitiveContains("login") ||
           output.localizedCaseInsensitiveContains("auth") ||
           output.localizedCaseInsensitiveContains("unauthorized") {
            return .needsAuth
        }
        return .apiError(L10n.t("Higgsfield CLI exited with status \(status)"))
    }

    static func run(binary: URL, timeout: TimeInterval = timeout) throws -> String {
        let process = Process()
        process.executableURL = binary
        process.arguments = ["workspace", "list", "--json"]
        process.environment = childEnvironment(binary: binary)
        process.currentDirectoryURL = FileManager.default.temporaryDirectory
        process.standardInput = FileHandle.nullDevice

        let output = Pipe()
        process.standardOutput = output
        let errors = Pipe()
        process.standardError = errors

        let stdout = output.fileHandleForReading
        let stderr = errors.fileHandleForReading
        let collectedOut = Slot(Data())
        let collectedErr = Slot(Data())

        let exited = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in exited.signal() }

        try process.run()
        let pid = process.processIdentifier
        let grouped = setpgid(pid, pid) == 0
        let deadline = DispatchTime.now() + timeout

        // Each stream is read to its end, off this thread. Stopping at the exit
        // notification instead would drop whatever was still in the pipe, and a
        // full stderr nobody reads would stall the process.
        let drained = DispatchGroup()
        for (handle, slot) in [(stdout, collectedOut), (stderr, collectedErr)] {
            drained.enter()
            DispatchQueue.global(qos: .utility).async {
                slot.append(handle.readDataToEndOfFile())
                drained.leave()
            }
        }

        guard exited.wait(timeout: deadline) == .success, drained.wait(timeout: deadline) == .success else {
            if grouped { _ = killpg(pid, SIGKILL) }
            _ = kill(pid, SIGKILL)
            throw UsageProviderError.timedOut
        }

        let status = process.terminationStatus
        guard status == 0 else {
            let printed = String(decoding: collectedOut.value + collectedErr.value, as: UTF8.self)
            throw failure(status: status, output: printed)
        }

        guard let text = String(data: collectedOut.value, encoding: .utf8), !text.isEmpty else {
            throw UsageProviderError.badResponse(status: 0)
        }
        return text
    }

    private final class Slot: @unchecked Sendable {
        private let lock = NSLock()
        private var stored: Data

        init(_ value: Data) { stored = value }

        func append(_ chunk: Data) {
            lock.lock()
            stored.append(chunk)
            lock.unlock()
        }

        var value: Data {
            lock.lock()
            defer { lock.unlock() }
            return stored
        }
    }
}
