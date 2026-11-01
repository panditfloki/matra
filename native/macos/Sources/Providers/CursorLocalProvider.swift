import AppKit
import Foundation

/// Reads Cursor usage as the account already signed in on this Mac.
///
/// This replaced a WebView sign-in, and the reason is worth keeping: signing
/// into cursor.com inside the app created a second, empty account, so the notch
/// faithfully reported zero usage for someone who was not the user. Borrowing
/// the editor's own session — or, when that is missing, the `cursor-agent`
/// login — removes the question. There is only ever one account, the one
/// actually being used.
actor CursorLocalProvider: UsageProvider {
    nonisolated let id = "cursor"
    nonisolated let displayName = "Cursor"
    nonisolated let glyph = ProviderGlyph.cursor

    private let endpoint = URL(string: "https://cursor.com/api/usage-summary")!
    private let session: URLSession
    private let loadCredentials: @Sendable () throws -> CursorCredentials
    nonisolated private let loadAccount: @Sendable () -> ProviderAccount?
    nonisolated private let askForAccess: @Sendable () -> Void
    private let forgetCredentials: @Sendable () -> Void

    init(session: URLSession = .shared,
         loadCredentials: @escaping @Sendable () throws -> CursorCredentials = { try CursorCredentials.load() },
         loadAccount: @escaping @Sendable () -> ProviderAccount? = { CursorCredentials.account() },
         askForAccess: @escaping @Sendable () -> Void = {
             CursorSafeStorage.askAgain()
             CursorAgentKeychain.askAgain()
         },
         forgetCredentials: @escaping @Sendable () -> Void = { CursorCredentials.forgetCachedAgent() }) {
        self.session = session
        self.loadCredentials = loadCredentials
        self.loadAccount = loadAccount
        self.askForAccess = askForAccess
        self.forgetCredentials = forgetCredentials
    }

    nonisolated var signInRoute: SignInRoute {
        // Bundle-id lookup, not a hard-coded /Applications path: Cursor can
        // live in ~/Applications, and a miss here would hide the Open button
        // from someone who does have the editor.
        let installed = NSWorkspace.shared.urlForApplication(
            withBundleIdentifier: CursorCredentials.bundleID
        ) != nil
        return CursorCredentials.signInRoute(editorInstalled: installed)
    }

    nonisolated func account() -> ProviderAccount? { loadAccount() }

    /// UsageStore calls this only for the explicit "Allow access…" action.
    nonisolated func forgetCachedCredential() { askForAccess() }

    func fetchSnapshot() async throws -> ProviderSnapshot {
        // Re-read every time: the editor rotates this, and holding a stale copy
        // would mean signing ourselves out for no reason. The agent token is
        // cached inside `CursorAgentKeychain` so the keychain is not.
        let credentials = try loadCredentials()
        guard credentials.isUsableForRequest else { throw UsageProviderError.needsAuth }

        var request = URLRequest(url: endpoint)
        request.setValue(credentials.sessionCookie, forHTTPHeaderField: "Cookie")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.timeoutInterval = 15

        let (data, response) = try await session.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0

        if status == 401 || status == 403 {
            // A rejected token is the one signal the copy in hand is wrong
            // despite not having expired — signing into a different CLI
            // account replaces the keychain item. Drop it so the next read
            // checks again without prompting. A rejected session says nothing
            // about the editor's AES key: retain that key until its Keychain
            // item changes or the person explicitly asks for access again.
            forgetCredentials()
            throw UsageProviderError.needsAuth
        }
        guard (200..<300).contains(status) else {
            throw UsageProviderError.badResponse(status: status)
        }

        let body = String(data: data, encoding: .utf8) ?? ""
        let windows = try CursorUsage.windows(fromJSON: body)
        return ProviderSnapshot(
            id: id,
            displayName: displayName,
            glyph: glyph,
            fidelity: .official,
            status: .ok,
            windows: windows,
            headlineID: CursorUsage.headlineID(in: windows),
            plan: CursorUsage.plan(fromJSON: body) ?? account()?.plan?.nonEmptyPlan
        )
    }
}
