import Foundation
import os

/// Reads Higgsfield AI generation credits from the official CLI (`hf workspace list --json`)
/// or from Higgsfield's developer gateway API (`https://fnf-api-gw.higgsfield.ai/fnf/developer/v2alpha/account/workspaces`).
actor HiggsfieldProvider: UsageProvider {
    nonisolated let id = "higgsfield"
    nonisolated let displayName = "Higgsfield"
    nonisolated let glyph = ProviderGlyph.higgsfield

    private let session: URLSession
    private let archive: UsageArchive
    nonisolated private let sources: HiggsfieldCredentialSources
    private let now: @Sendable () -> Date
    private var retryNoEarlierThan: Date?
    /// Read by `account()` from outside the actor while a fetch may be writing.
    nonisolated private let lastKnown = LastKnown()

    init(session: URLSession = .shared,
         archive: UsageArchive = UsageArchive(),
         sources: HiggsfieldCredentialSources = HiggsfieldCredentialSources(),
         now: @escaping @Sendable () -> Date = { Date() }) {
        self.session = session
        self.archive = archive
        self.sources = sources
        self.now = now
        self.retryNoEarlierThan = archive.loadBackoffUntil(providerID: id)
    }

    nonisolated var isVisibleWhenAbsent: Bool {
        HiggsfieldCredentials.isPresent(sources)
    }

    nonisolated var signInRoute: SignInRoute {
        .command("hf auth login", name: "Higgsfield", install: URL(string: "https://github.com/higgsfield-ai/cli"))
    }

    nonisolated func account() -> ProviderAccount? {
        guard HiggsfieldCredentials.isPresent(sources) else { return nil }
        let known = lastKnown.value
        return ProviderAccount(
            label: known.workspace,
            plan: known.plan,
            source: sources.locateBinary() != nil ? L10n.t("Higgsfield CLI") : "Higgsfield",
            manageURL: URL(string: "https://higgsfield.ai/me/settings/subscription")
        )
    }

    nonisolated func signOut() async {
        sources.deleteSettingsToken()
    }

    nonisolated func forgetCachedCredential() {
        sources.forgetCached()
    }

    func fetchSnapshot() async throws -> ProviderSnapshot {
        if let retryNoEarlierThan, retryNoEarlierThan > now() {
            let delay = retryNoEarlierThan.timeIntervalSince(now())
            throw UsageProviderError.rateLimited(retryAfter: delay)
        }

        // 1. Try asking CLI first if installed
        var cliFailure: Error?
        if let binary = sources.locateBinary() {
            do {
                return snapshot(from: try HiggsfieldUsage.parse(try sources.runCLI(binary)))
            } catch UsageProviderError.rateLimited(let delay) {
                backOff(delay)
                cliFailure = UsageProviderError.rateLimited(retryAfter: delay)
            } catch {
                cliFailure = error
            }
        }

        // 2. Fall back to developer gateway HTTP API using saved credential.
        // With none saved, the CLI's own failure is the true answer: a timeout
        // there says nothing about being signed out.
        let credential: HiggsfieldCredentials.Credential
        do {
            credential = try HiggsfieldCredentials.load(sources)
        } catch {
            throw cliFailure ?? error
        }

        var request = URLRequest(url: URL(string: "https://fnf-api-gw.higgsfield.ai/fnf/developer/v2alpha/account/workspaces")!)
        let authHeader = credential.token.contains(":") ? "Key \(credential.token)" : "Bearer \(credential.token)"
        request.setValue(authHeader, forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.timeoutInterval = 15
        request.cachePolicy = .reloadIgnoringLocalCacheData

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch let error as URLError where error.code == .timedOut {
            throw UsageProviderError.timedOut
        } catch where error is CancellationError || (error as? URLError)?.code == .cancelled {
            throw CancellationError()
        } catch {
            throw UsageProviderError.apiError(L10n.t("Couldn't reach Higgsfield. Check your connection."))
        }

        let http = response as? HTTPURLResponse
        let status = http?.statusCode ?? 0
        if status == 401 {
            sources.forgetCached()
            throw UsageProviderError.needsAuth
        }
        if status == 429 {
            let delay = Self.retryDelay(http?.value(forHTTPHeaderField: "Retry-After"), now: now())
            backOff(delay)
            throw UsageProviderError.rateLimited(retryAfter: delay)
        }
        guard (200..<300).contains(status) else {
            throw UsageProviderError.badResponse(status: status)
        }

        return snapshot(from: try HiggsfieldUsage.parse(data))
    }

    private func snapshot(from result: HiggsfieldUsage.Result) -> ProviderSnapshot {
        lastKnown.value = (result.planName, result.workspaceName)
        if retryNoEarlierThan != nil {
            retryNoEarlierThan = nil
            archive.saveBackoffUntil(nil, providerID: id)
        }
        return ProviderSnapshot(
            id: id,
            displayName: displayName,
            glyph: glyph,
            fidelity: result.fidelity,
            status: .ok,
            windows: result.windows,
            headlineID: result.headlineID,
            plan: result.planName
        )
    }

    private func backOff(_ delay: TimeInterval) {
        retryNoEarlierThan = now().addingTimeInterval(delay)
        archive.saveBackoffUntil(retryNoEarlierThan, providerID: id)
    }

    /// `Retry-After` is either a number of seconds or an HTTP date; a hint
    /// shorter than a minute, or none, still waits the minute.
    static func retryDelay(_ header: String?, now: Date) -> TimeInterval {
        guard let header = header?.trimmingCharacters(in: .whitespaces), !header.isEmpty else { return 60 }
        if let seconds = Double(header), seconds.isFinite { return max(60, seconds) }
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.dateFormat = "EEE, dd MMM yyyy HH:mm:ss zzz"
        return max(60, formatter.date(from: header)?.timeIntervalSince(now) ?? 0)
    }

    private final class LastKnown: @unchecked Sendable {
        private let lock = NSLock()
        private var stored: (plan: String?, workspace: String?) = (nil, nil)

        var value: (plan: String?, workspace: String?) {
            get { lock.withLock { stored } }
            set { lock.withLock { stored = newValue } }
        }
    }
}
