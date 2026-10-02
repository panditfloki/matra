import Foundation

/// Back off failed reads as well as successful ones. Frequent quota-store
/// publications must not repeatedly launch a missing or failing local reader.
struct UsageSpendRefreshPolicy {
    private var attempts: [String: Date] = [:]

    mutating func claim(providerID: String, force: Bool = false, now: Date = Date()) -> Bool {
        if !force, let previous = attempts[providerID], now.timeIntervalSince(previous) < 150 { return false }
        attempts[providerID] = now
        return true
    }
}

/// Explicit read-on-open/refresh, no new provider poller, dependencies, database,
/// auth access or prompt collection. All external commands use argv, offline.
enum UsageSpendReader {
    static func read(providerID: String, now: Date = Date(),
                     home: URL = FileManager.default.homeDirectoryForCurrentUser) throws -> UsageSpendReport {
        guard ["codex", "claude"].contains(providerID) else { throw SpendReaderError.unavailable }
        let paths = [home.appendingPathComponent(".local/bin/ccusage").path,
                     "/opt/homebrew/bin/ccusage", "/usr/local/bin/ccusage"]
        guard let binary = paths.first(where: { FileManager.default.isExecutableFile(atPath: $0) })
        else { throw SpendReaderError.unavailable }
        let command = nativeCommand(for: binary)
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .current
        let recentStart = calendar.date(byAdding: .day, value: -29, to: calendar.startOfDay(for: now)) ?? now
        let start = min(recentStart, calendar.dateInterval(of: .month, for: now)?.start ?? recentStart)
        let since = UsageSpendReport.dayKey(start, calendar: calendar).replacingOccurrences(of: "-", with: "")
        let projectsSince = UsageSpendReport.dayKey(recentStart, calendar: calendar).replacingOccurrences(of: "-", with: "")
        let until = UsageSpendReport.dayKey(now, calendar: calendar).replacingOccurrences(of: "-", with: "")
        let zone = calendar.timeZone.identifier
        var arguments = [providerID, "daily", "--json", "--offline", "--since", since, "--until", until, "--timezone", zone]
        if providerID == "claude" { arguments.append("--breakdown") }
        let data = try run(binary: command, arguments: arguments, home: home)
        // A Codex session report filtered by the same dates supplies project
        // totals and pricing evidence. Its directory is a date folder, never
        // a project name. Without it, unproven models stay unpriced.
        let sessions = providerID == "codex" ? try? run(binary: command,
            arguments: ["codex", "session", "--json", "--offline", "--since", projectsSince, "--until", until, "--timezone", zone], home: home) : nil
        var report = try UsageSpendReport.decodeDaily(data, providerID: providerID, measuredAt: now, timeZoneID: zone,
            evidence: sessions.map(UsageSpendReport.sessionEvidence) ?? [])
        // Same coverage rule for project rows: only proven-priced models count.
        let priced = Set(report.models.filter { $0.value.tokens > 0 && $0.value.pricedTokens == $0.value.tokens }.map(\.id))
        if let sessions, let projects = try? codexProjects(sessions, home: home, pricedModels: priced) {
            report = UsageSpendReport(providerID: report.providerID, measuredAt: report.measuredAt,
                timeZoneID: report.timeZoneID, days: report.days, projects: projects, pricingNote: report.pricingNote)
        }
        return report
    }

    /// Current ccusage's JS shim spawns a native child. Invoke that installed
    /// executable directly so the watchdog owns the process doing the work.
    static func nativeCommand(for wrapper: String) -> String {
        let package = URL(fileURLWithPath: wrapper).resolvingSymlinksInPath()
            .deletingLastPathComponent().deletingLastPathComponent()
        #if arch(arm64)
        let arch = "arm64"
        #else
        let arch = "x64"
        #endif
        let candidate = package.appendingPathComponent("node_modules/@ccusage/ccusage-darwin-\(arch)/bin/ccusage").path
        return FileManager.default.isExecutableFile(atPath: candidate) ? candidate : wrapper
    }

    static func run(binary: String, arguments: [String], home: URL) throws -> Data {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binary)
        process.arguments = arguments
        process.currentDirectoryURL = home
        var environment = ProcessInfo.processInfo.environment
        let extra = [home.appendingPathComponent(".local/bin").path, "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"]
        environment["PATH"] = (extra + (environment["PATH"] ?? "").split(separator: ":").map(String.init)).joined(separator: ":")
        environment["NO_COLOR"] = "1"
        process.environment = environment
        process.standardInput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        let output = Pipe()
        process.standardOutput = output
        try process.run()
        let timeout = DispatchWorkItem { if process.isRunning { process.terminate() } }
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 30, execute: timeout)
        defer { timeout.cancel() }
        var data = Data()
        while let chunk = try output.fileHandleForReading.read(upToCount: 65536), !chunk.isEmpty {
            data.append(chunk)
            if data.count > 32 * 1024 * 1024 {
                process.terminate()
                throw SpendReaderError.invalidReport
            }
        }
        process.waitUntilExit()
        guard process.terminationStatus == 0 else { throw SpendReaderError.failed }
        return data
    }

    static func codexProjects(_ data: Data, home: URL,
                              pricedModels: Set<String>? = nil) throws -> [UsageSpendReport.Breakdown] {
        guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let sessions = root["sessions"] as? [[String: Any]] else { throw SpendReaderError.invalidReport }
        var labels: [String: String] = [:]
        let fm = FileManager.default
        for folder in ["sessions", "archived_sessions"] {
            let root = home.appendingPathComponent(".codex/\(folder)")
            guard let files = fm.enumerator(at: root, includingPropertiesForKeys: [.isRegularFileKey], options: [.skipsHiddenFiles]) else { continue }
            for case let file as URL in files where file.pathExtension == "jsonl" {
                guard let handle = try? FileHandle(forReadingFrom: file) else { continue }
                defer { try? handle.close() }
                // Parse only the first metadata line, bounded to 4 MiB, never
                // retain prompt or transcript bodies in the analytics model.
                var first = Data()
                while first.count < 4 * 1024 * 1024 {
                    guard let chunk = try? handle.read(upToCount: 65536), !chunk.isEmpty else { break }
                    first.append(chunk)
                    if let index = first.firstIndex(of: 10) { first = first.prefix(upTo: index); break }
                }
                guard let row = try? JSONSerialization.jsonObject(with: first) as? [String: Any],
                      row["type"] as? String == "session_meta",
                      let p = row["payload"] as? [String: Any],
                      let id = (p["id"] ?? p["session_id"]) as? String,
                      let cwd = p["cwd"] as? String else { continue }
                labels[id.lowercased()] = cwd
            }
        }
        let uuid = try NSRegularExpression(pattern: "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        let rows = sessions.map { row -> UsageSpendReport.Breakdown in
            let key = (row["sessionId"] ?? row["sessionFile"]) as? String ?? ""
            let match = uuid.firstMatch(in: key, range: NSRange(key.startIndex..., in: key))
            let id = match.flatMap { Range($0.range, in: key) }.map { String(key[$0]).lowercased() }
            let label = id.flatMap { labels[$0] } ?? "Unattributed"
            let tokens = UsageSpendReport.integer(row["totalTokens"]) ?? 0
            let cost = UsageSpendReport.money(row["costUSD"])
            var value = UsageSpendReport.Value(tokens: tokens, costUSD: tokens > 0 && cost == 0 ? nil : cost)
            if let pricedModels, let map = row["models"] as? [String: [String: Any]] {
                let priced = min(tokens, map.filter { pricedModels.contains($0.key) }
                    .reduce(0) { $0 + (UsageSpendReport.integer($1.value["totalTokens"]) ?? 0) })
                value.pricedTokens = priced
                value.incompleteCost = priced < tokens
            }
            return .init(id: label, value: value)
        }
        return UsageSpendReport.group(rows)
    }
}
