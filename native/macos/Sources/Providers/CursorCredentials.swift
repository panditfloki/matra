import Foundation
import CommonCrypto
import Security
import SQLite3

/// The session Cursor keeps for itself — the editor's SQLite store first,
/// then the `cursor-agent` login if that store is missing or signed out.
///
/// Matra only ever reads it, the same bargain as Claude Code's keychain
/// token: the owning tool mints and refreshes it, we borrow the current value.
/// The editor database is opened read-only and never `immutable`, so a running
/// editor is not blocked and a rotated token is not served from a stale
/// checkpoint. The agent token lives in the login keychain; that read is
/// cached, because every data fetch can raise a prompt.
///
/// The existing `/api/usage-summary` integration sends the historical cookie
/// `WorkosCursorSessionToken={accountID}::{accessToken}`. Storage protection
/// does not by itself establish a different HTTP authentication scheme.
/// The editor stores those two halves in SQLite; the CLI stores the JWT under
/// `cursor-access-token` and the account id in `~/.cursor/cli-config.json`.
struct CursorCredentials {
    let accountID: String
    let accessToken: String
    /// The web API wants the pair as one cookie.
    var sessionCookie: String { "WorkosCursorSessionToken=\(accountID)::\(accessToken)" }
    var isUsableForRequest: Bool {
        Self.isSafeCookieComponent(accountID) && Self.isSafeCookieComponent(accessToken)
            && !CursorSafeStorage.isWrapped(accessToken)
    }

    /// RFC 6265 cookie-octet, including the `|` and `:` in Cursor's account
    /// ids. Reject controls and delimiters instead of letting a malformed
    /// credential add another cookie or HTTP header.
    static func isSafeCookieComponent(_ value: String) -> Bool {
        !value.isEmpty && value.utf8.allSatisfy {
            $0 >= 0x21 && $0 <= 0x7e && ![0x22, 0x2c, 0x3b, 0x5c].contains($0)
        }
    }

    static var storeURL: URL {
        URL(fileURLWithPath: NSHomeDirectory())
            .appendingPathComponent("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
    }

    /// Written by `cursor-agent login`. Non-secret: email and the WorkOS
    /// subject that belongs in the cookie. The JWT itself is not here.
    static var agentConfigURL: URL {
        URL(fileURLWithPath: NSHomeDirectory())
            .appendingPathComponent(".cursor/cli-config.json")
    }

    /// Minted by ToDesktop, who build Cursor — stable across updates, but not
    /// across Cursor leaving ToDesktop or rebranding. Kept here beside the store
    /// path so the two facts about a Cursor installation change together: the
    /// activity monitor and the sign-in route both read this one.
    static let bundleID = "com.todesktop.230313mzl4w4u92"

    /// Identity follows the source that would supply the credential. Cached
    /// editor metadata alone must not conceal an active agent session.
    /// Neither this path nor the settings row decrypts a token.
    static func account() -> ProviderAccount? {
        account(editorStore: storeURL, agentConfig: agentConfigURL,
                agentTokenPresent: CursorAgentKeychain.isPresent)
    }

    static func account(editorStore: URL, agentConfig: URL,
                        agentTokenPresent: Bool) -> ProviderAccount? {
        do {
            if let editor = try editorAuthInfo(from: editorStore) { return account(forEditor: editor) }
            return agentTokenPresent ? agentAccount(from: agentConfig) : nil
        } catch {
            // An unreadable editor is not evidence that another account owns
            // the current session. Do not display the CLI identity either.
            return nil
        }
    }

    /// Identity, read from the same store as the session. Non-secret: the email
    /// and plan the editor caches for its own UI.
    static func account(from url: URL) -> ProviderAccount? {
        guard let editor = try? editorAuthInfo(from: url) else { return nil }
        return account(forEditor: editor)
    }

    private static func account(forEditor editor: EditorAuthInfo) -> ProviderAccount {
        return ProviderAccount(
            label: editor.email,
            plan: editor.plan,
            source: "Cursor",
            manageURL: URL(string: "https://cursor.com/dashboard")
        )
    }

    static func agentAccount(from url: URL) -> ProviderAccount? {
        guard let info = agentAuthInfo(from: url),
              info.email != nil || info.authId != nil
        else { return nil }
        return ProviderAccount(
            label: info.email,
            plan: nil,
            source: "cursor-agent",
            manageURL: URL(string: "https://cursor.com/dashboard")
        )
    }

    /// Editor first. The CLI session is only reached when the editor has
    /// nothing to borrow — otherwise a laptop with both would flip between
    /// accounts depending on which file we happened to read.
    static func load() throws -> CursorCredentials {
        try load(editorStore: storeURL, agentConfig: agentConfigURL,
                 agentToken: { try CursorAgentKeychain.load() })
    }

    /// Editor store only. Tests, and the combined loader above, pin the path
    /// so a missing editor cannot silently become a live keychain read.
    static func load(from url: URL, now: Date = Date(),
                     keyReader: () throws -> Data = { try CursorSafeStorage.loadKey() }) throws -> CursorCredentials {
        guard let editor = try editorAuthInfo(from: url) else { throw UsageProviderError.needsAuth }
        return try session(fromEditor: editor, now: now, keyReader: keyReader)
    }

    private static func session(fromEditor editor: EditorAuthInfo, now: Date,
                                keyReader: () throws -> Data) throws -> CursorCredentials {
        let token = try CursorSafeStorage.decode(editor.token, keyReader: keyReader)
        if agentTokenIsExpired(token, now: now) { throw UsageProviderError.credentialExpired }
        // Prefer the editor's cached WorkOS id when present. Recent Cursor
        // builds (Auth0 / enterprise in particular) often omit
        // `stripeMembershipAuthId` even while signed in — the JWT `sub` is the
        // same value the cookie needs, so fall back to it rather than telling
        // the user to sign in again.
        let account = editor.accountID ?? subject(fromJWT: token)
        guard let account, isSafeCookieComponent(account) else { throw UsageProviderError.needsAuth }

        return CursorCredentials(accountID: account, accessToken: token)
    }

    /// Same precedence as `load()`, with the agent token injected so the
    /// fallback can be tested without a keychain.
    static func load(editorStore: URL, agentToken: String?, agentConfig: URL,
                     now: Date = Date(),
                     keyReader: () throws -> Data = { try CursorSafeStorage.loadKey() }) throws -> CursorCredentials {
        try load(editorStore: editorStore, agentConfig: agentConfig, now: now,
                 keyReader: keyReader, agentToken: {
            guard let agentToken, !agentToken.isEmpty else { throw UsageProviderError.needsAuth }
            return agentToken
        })
    }

    /// The absence of an editor token is the only fallback condition. A
    /// protected, malformed, expired or refused editor session still belongs
    /// to the editor's account; none is permission to select another login.
    static func load(editorStore: URL, agentConfig: URL, now: Date = Date(),
                     keyReader: () throws -> Data = { try CursorSafeStorage.loadKey() },
                     agentToken: () throws -> String) throws -> CursorCredentials {
        if let editor = try editorAuthInfo(from: editorStore) {
            return try session(fromEditor: editor, now: now, keyReader: keyReader)
        }
        return try session(fromAgentToken: agentToken(), configURL: agentConfig, now: now)
    }

    /// Open the editor when it is installed; otherwise name the CLI command.
    /// Offering "Open Cursor" on a machine that has never had the app is a
    /// button that does nothing — worse than no button.
    static func signInRoute(editorInstalled: Bool) -> SignInRoute {
        if editorInstalled {
            return .openApp(bundleID: bundleID, name: "Cursor")
        }
        return .guidance(
            "Run `cursor-agent login` once — the notch reads that session. "
            + "The Cursor editor works the same way, if you have it."
        )
    }

    static func forgetCachedAgent() { CursorAgentKeychain.forgetCached() }

    /// The cookie's left half. `authId` is the WorkOS subject the editor
    /// itself stores as `stripeMembershipAuthId`. Numeric `userId` also works
    /// against the same endpoint, and the JWT `sub` is the last resort when
    /// someone has a token but no config file yet.
    static func agentAccountID(token: String, configURL: URL) -> String? {
        if let info = agentAuthInfo(from: configURL) {
            if let authId = info.authId { return authId }
            if let userId = info.userId { return userId }
        }
        guard let sub = claims(inJWT: token)?["sub"] as? String, !sub.isEmpty else { return nil }
        return sub
    }

    static func session(fromAgentToken token: String, configURL: URL,
                        now: Date = Date()) throws -> CursorCredentials {
        let trimmed = token.trimmingCharacters(in: .whitespacesAndNewlines)
        guard isSafeCookieComponent(trimmed), !CursorSafeStorage.isWrapped(trimmed) else {
            throw UsageProviderError.needsAuth
        }
        if agentTokenIsExpired(trimmed, now: now) {
            throw UsageProviderError.credentialExpired
        }
        guard let accountID = agentAccountID(token: trimmed, configURL: configURL),
              isSafeCookieComponent(accountID)
        else { throw UsageProviderError.needsAuth }
        return CursorCredentials(accountID: accountID, accessToken: trimmed)
    }

    static func agentTokenIsExpired(_ token: String, now: Date = Date()) -> Bool {
        // JWT `exp` is an integer second. JSONSerialization hands it back as
        // NSNumber; `as? Double` is the wrong ask and would miss a live expiry.
        guard let exp = (claims(inJWT: token)?["exp"] as? NSNumber)?.doubleValue else { return false }
        return exp <= now.timeIntervalSince1970
    }

    /// Claims supply the subject and a local expiry hint. The server validates
    /// the token; this is only so an aged-out JWT can keep the last reading
    /// instead of looking like a sign-out.
    static func claims(inJWT token: String) -> [String: Any]? {
        let parts = token.split(separator: ".")
        guard parts.count >= 2 else { return nil }

        var payload = String(parts[1])
            .replacingOccurrences(of: "-", with: "+")
            .replacingOccurrences(of: "_", with: "/")
        payload += String(repeating: "=", count: (4 - payload.count % 4) % 4)

        guard let data = Data(base64Encoded: payload) else { return nil }
        return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    }

    private static func agentAuthInfo(from url: URL) -> AgentAuthInfo? {
        guard let data = try? Data(contentsOf: url),
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let info = root["authInfo"] as? [String: Any]
        else { return nil }

        let userId: String?
        if let number = info["userId"] as? NSNumber {
            // JSON numbers arrive as NSNumber. `as? Int` works for small
            // values and fails for the same object once it is too wide, so
            // take the string form and keep one path.
            userId = number.stringValue
        } else if let text = info["userId"] as? String, !text.isEmpty {
            userId = text
        } else {
            userId = nil
        }

        func nonempty(_ key: String) -> String? {
            guard let value = info[key] as? String, !value.isEmpty else { return nil }
            return value
        }

        return AgentAuthInfo(email: nonempty("email"), authId: nonempty("authId"), userId: userId)
    }

    /// `sub` claim from an unsigned JWT payload — Cursor's access token is a
    /// standard three-part JWT whose subject is the WorkOS / Auth0 user id.
    /// Reuses the decoder the agent-token path already has for `exp`, rather
    /// than a second copy of the same base64 dance.
    static func subject(fromJWT token: String) -> String? {
        guard let sub = claims(inJWT: token)?["sub"] as? String, !sub.isEmpty else { return nil }
        return sub
    }

    private static func value(forKey key: String, in db: OpaquePointer) throws -> String? {
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, "SELECT value FROM ItemTable WHERE key = ?", -1, &statement, nil) == SQLITE_OK else {
            throw UsageProviderError.credentialExpired
        }
        defer { sqlite3_finalize(statement) }
        guard sqlite3_bind_text(statement, 1, key, -1,
                                unsafeBitCast(-1, to: sqlite3_destructor_type.self)) == SQLITE_OK else {
            throw UsageProviderError.credentialExpired
        }
        let result = sqlite3_step(statement)
        if result == SQLITE_DONE { return nil }
        guard result == SQLITE_ROW else { throw UsageProviderError.credentialExpired }
        let type = sqlite3_column_type(statement, 0)
        if type == SQLITE_NULL { return nil }
        guard type == SQLITE_TEXT, let bytes = sqlite3_column_text(statement, 0),
              let value = String(data: Data(bytes: bytes, count: Int(sqlite3_column_bytes(statement, 0))),
                                 encoding: .utf8),
              sqlite3_step(statement) == SQLITE_DONE else {
            throw UsageProviderError.credentialExpired
        }
        return value
    }

    private static func editorAuthInfo(from url: URL) throws -> EditorAuthInfo? {
        do {
            _ = try FileManager.default.attributesOfItem(atPath: url.path)
        } catch let error as NSError {
            if error.domain == NSCocoaErrorDomain,
               [NSFileNoSuchFileError, NSFileReadNoSuchFileError].contains(error.code) { return nil }
            throw UsageProviderError.credentialExpired
        }
        guard let db = SQLiteStore.open(url) else { throw UsageProviderError.credentialExpired }
        defer { sqlite3_close(db) }
        // A read-only WAL-aware connection sees the editor's current rotation.
        guard let token = try value(forKey: "cursorAuth/accessToken", in: db),
              !token.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
        func nonempty(_ key: String) throws -> String? {
            try value(forKey: key, in: db).flatMap { $0.isEmpty ? nil : $0 }
        }
        return EditorAuthInfo(token: token,
                              accountID: try nonempty("cursorAuth/stripeMembershipAuthId"),
                              email: try nonempty("cursorAuth/cachedEmail"),
                              plan: try nonempty("cursorAuth/stripeMembershipType"))
    }

    private struct EditorAuthInfo {
        let token: String
        let accountID: String?
        let email: String?
        let plan: String?
    }

    private struct AgentAuthInfo {
        let email: String?
        let authId: String?
        let userId: String?
    }
}

/// Cursor's editor wraps Electron safeStorage bytes before putting them in
/// SQLite. The macOS v10 payload is AES-128-CBC with PKCS#7 padding, using
/// Chromium's PBKDF2-HMAC-SHA1 parameters. Only the derived key is held in
/// memory; neither it nor the decrypted session is saved by Matra.
enum CursorSafeStorage {
    static let service = "Cursor Safe Storage"
    static let account = "Cursor Key"
    private static let prefix = "cursor-safe-storage:v1:"

    private static let keychain = CursorKeychainReader<Data>(
        refusalKey: service,
        refusals: Runtime.isUnderTest ? nil : .standard,
        itemModifiedAt: { KeychainItem.modifiedAt(service: service, account: account) },
        // Check the item's cheap metadata even while a key is held, so a
        // safe-storage password rotation invalidates the in-memory copy.
        isExpired: { _ in true },
        reader: { interactive in
            guard let winner = KeychainItem.newest(service: service, account: account) else {
                throw UsageProviderError.needsAuth
            }
            let (status, data) = KeychainSecret.read(
                query: [
                    kSecClass: kSecClassGenericPassword,
                    kSecValuePersistentRef: winner.persistentRef,
                    kSecReturnData: true,
                    kSecMatchLimit: kSecMatchLimitOne
                ],
                interactive: interactive,
                rescue: nil
            )
            guard status == errSecSuccess, let data, !data.isEmpty else {
                throw CursorKeychainReader<Data>.error(for: status)
            }
            return try deriveKey(password: data)
        }
    )

    static func loadKey() throws -> Data { try keychain.load() }
    static func forgetCached() { keychain.forgetCached() }
    static func askAgain() { keychain.askAgain() }

    /// Reserve the whole namespace, including unknown versions and casing.
    /// A future wrapper must never become an HTTP cookie by accident.
    static func isWrapped(_ token: String) -> Bool {
        token.trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased().hasPrefix("cursor-safe-storage")
    }

    static func decode(_ stored: String,
                       keyReader: () throws -> Data = { try loadKey() }) throws -> String {
        guard isWrapped(stored) else {
            guard CursorCredentials.isSafeCookieComponent(stored) else { throw UsageProviderError.needsAuth }
            return stored
        }
        guard stored.hasPrefix(prefix) else { throw UsageProviderError.needsAuth }
        let encoded = String(stored.dropFirst(prefix.count))
        guard let payload = Data(base64Encoded: encoded),
              payload.base64EncodedString() == encoded,
              payload.starts(with: [0x76, 0x31, 0x30]) else {
            throw UsageProviderError.needsAuth
        }
        let ciphertext = Data(payload.dropFirst(3))
        guard !ciphertext.isEmpty, ciphertext.count % kCCBlockSizeAES128 == 0 else {
            throw UsageProviderError.needsAuth
        }
        let key = try keyReader()
        guard key.count == kCCKeySizeAES128 else { throw UsageProviderError.needsAuth }
        let iv = Data(repeating: 0x20, count: kCCBlockSizeAES128)
        let capacity = ciphertext.count + kCCBlockSizeAES128
        var plaintext = Data(count: capacity)
        var written = 0
        let status = plaintext.withUnsafeMutableBytes { output in
            key.withUnsafeBytes { keyBytes in
                iv.withUnsafeBytes { ivBytes in
                    ciphertext.withUnsafeBytes { input in
                        CCCrypt(CCOperation(kCCDecrypt), CCAlgorithm(kCCAlgorithmAES),
                                CCOptions(kCCOptionPKCS7Padding),
                                keyBytes.baseAddress, key.count, ivBytes.baseAddress,
                                input.baseAddress, ciphertext.count,
                                output.baseAddress, capacity, &written)
                    }
                }
            }
        }
        guard status == kCCSuccess else { throw UsageProviderError.needsAuth }
        plaintext.count = written
        guard let token = String(data: plaintext, encoding: .utf8),
              CursorCredentials.isSafeCookieComponent(token),
              !isWrapped(token) else { throw UsageProviderError.needsAuth }
        return token
    }

    static func deriveKey(password: Data) throws -> Data {
        guard !password.isEmpty else { throw UsageProviderError.needsAuth }
        let salt = Data("saltysalt".utf8)
        var key = Data(count: kCCKeySizeAES128)
        let status = key.withUnsafeMutableBytes { output in
            password.withUnsafeBytes { passwordBytes in
                salt.withUnsafeBytes { saltBytes in
                    CCKeyDerivationPBKDF(CCPBKDFAlgorithm(kCCPBKDF2),
                                        passwordBytes.bindMemory(to: Int8.self).baseAddress, password.count,
                                        saltBytes.bindMemory(to: UInt8.self).baseAddress, salt.count,
                                        CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA1), 1003,
                                        output.bindMemory(to: UInt8.self).baseAddress, kCCKeySizeAES128)
                }
            }
        }
        guard status == kCCSuccess else { throw UsageProviderError.needsAuth }
        return key
    }
}

/// Shared permission, refusal and cache policy for Cursor's two separate
/// Keychain items. Initializers take injected readers and no preferences by
/// default, so tests need neither a Keychain nor the installed app's settings.
final class CursorKeychainReader<Secret>: @unchecked Sendable {
    private let cache: CredentialCache<Secret>
    private let prompt: PromptPermission
    private let refusal: KeychainRefusal
    private let itemModifiedAt: () -> Date?
    private let reader: (_ interactive: Bool) throws -> Secret

    init(refusalKey: String,
         now: @escaping () -> Date = Date.init,
         refusals: UserDefaults? = nil,
         itemModifiedAt: @escaping () -> Date? = { nil },
         isExpired: @escaping (Secret) -> Bool,
         reader: @escaping (_ interactive: Bool) throws -> Secret) {
        self.prompt = PromptPermission(now: now)
        self.refusal = KeychainRefusal(key: refusalKey, defaults: refusals)
        self.itemModifiedAt = itemModifiedAt
        self.reader = reader
        self.cache = CredentialCache<Secret>(
            now: now,
            isPermanentFailure: { if case UsageProviderError.accessDenied = $0 { return true }; return false },
            isExpired: isExpired
        )
    }

    var isRefused: Bool { refusal.isRefused && !prompt.isOwed }

    func load() throws -> Secret {
        if isRefused { throw UsageProviderError.accessDenied }
        return try cache.value(itemModifiedAt: itemModifiedAt, reload: { [self] in
            let interactive = prompt.take()
            do {
                let secret = try reader(interactive)
                if interactive { refusal.set(false) }
                return secret
            } catch UsageProviderError.accessDenied where interactive {
                refusal.set(true)
                throw UsageProviderError.accessDenied
            }
        })
    }

    var held: Secret? { cache.held }
    func forgetCached() { cache.forget() }
    func askAgain() {
        prompt.grant()
        cache.forget()
    }

    static func error(for status: OSStatus) -> UsageProviderError {
        if ClaudeCredentials.wasTransient(status) { return .credentialExpired }
        return ClaudeCredentials.wasRefused(status) ? .accessDenied : .needsAuth
    }
}

/// The JWT `cursor-agent login` files in the login keychain.
///
/// Held until the item moves, for the reason spelled out in `CredentialCache`:
/// every data read can prompt, and an unchanged item cannot produce a
/// different token. Refreshing is left to the CLI.
enum CursorAgentKeychain {
    static let service = "cursor-access-token"
    static let account = "cursor-user"

    private static let keychain = CursorKeychainReader<String>(
        refusalKey: service,
        refusals: Runtime.isUnderTest ? nil : .standard,
        itemModifiedAt: { KeychainItem.modifiedAt(service: service, account: account) },
        isExpired: { CursorCredentials.agentTokenIsExpired($0) },
        reader: { try read(interactive: $0) }
    )

    static func load() throws -> String {
        try keychain.load()
    }

    static func forgetCached() { keychain.forgetCached() }
    static func askAgain() { keychain.askAgain() }
    static var isPresent: Bool {
        keychain.held != nil || KeychainItem.newest(service: service, account: account) != nil
    }

    /// Newest item under this service and account, then a targeted data read.
    /// Same two-step as Claude Code: attributes are free, the secret is not,
    /// and `kSecMatchLimitOne` has no ordering if a rotation left duplicates.
    static func read(interactive: Bool = false) throws -> String {
        guard let winner = KeychainItem.newest(service: service, account: account) else {
            Log.usage.error("cursor-agent keychain read failed: no item under \(service, privacy: .public)")
            throw UsageProviderError.needsAuth
        }

        let (status, data) = KeychainSecret.read(
            query: [
                kSecClass: kSecClassGenericPassword,
                kSecValuePersistentRef: winner.persistentRef,
                kSecReturnData: true,
                kSecMatchLimit: kSecMatchLimitOne
            ],
            interactive: interactive,
            rescue: nil
        )

        guard status == errSecSuccess, let data else {
            Log.usage.error("cursor-agent keychain read failed: OSStatus \(status)")
            throw CursorKeychainReader<String>.error(for: status)
        }

        guard let token = String(data: data, encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines),
              !token.isEmpty
        else { throw UsageProviderError.needsAuth }
        return token
    }
}
