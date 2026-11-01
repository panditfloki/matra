import SQLite3
import XCTest
@testable import Matra

/// Pinned to a response recorded from a live free account. `/api/usage-summary`
/// is not a documented API, so this is what fails first if it changes.
final class CursorUsageTests: XCTestCase {
    /// Verbatim, from the account the editor is signed into.
    private let recorded = """
    {"billingCycleStart":"2026-08-24T03:32:15.933Z",
     "billingCycleEnd":"2026-09-24T03:32:15.933Z",
     "membershipType":"free","limitType":"user","isUnlimited":false,
     "autoModelSelectedDisplayMessage":"You've used 10% of your included total usage",
     "namedModelSelectedDisplayMessage":"You've used 19% of your included API usage",
     "individualUsage":{
       "plan":{"enabled":true,"used":0,"limit":0,"remaining":0,
               "breakdown":{"included":0,"bonus":19,"total":19},
               "autoPercentUsed":0,"apiPercentUsed":19,"totalPercentUsed":9.5},
       "onDemand":{"enabled":false,"used":0,"limit":null,"remaining":null}},
     "teamUsage":{}}
    """

    private func windows(_ json: String) throws -> [LimitWindow] {
        try CursorUsage.windows(fromJSON: json)
    }

    func testReadsTheAutoBar() throws {
        let w = try windows(recorded)
        XCTAssertEqual(w[0].id, "auto")
        XCTAssertEqual(w[0].label, CursorUsage.modelsLabel)
        XCTAssertEqual(w[0].usedFraction ?? -1, 0, accuracy: 0.0001)
        XCTAssertEqual(w[0].duration, 31 * 86400)
        XCTAssertEqual(CursorUsage.headlineID(in: w), "auto")
    }

    func testMonthlyPaceUsesActualBillingDates() throws {
        for days in [28, 29, 30, 31] {
            let start = Date(timeIntervalSince1970: 1_800_000_000)
            let end = start.addingTimeInterval(Double(days) * 86400)
            let iso = ISO8601DateFormatter()
            let result = try windows("""
            {"billingCycleStart":"\(iso.string(from: start))",
             "billingCycleEnd":"\(iso.string(from: end))",
             "individualUsage":{"plan":{"autoPercentUsed":80}}}
            """)
            let window = try XCTUnwrap(result.first)
            let halfway = start.addingTimeInterval(Double(days) * 43200)
            XCTAssertEqual(window.duration, Double(days) * 86400)
            XCTAssertEqual(try XCTUnwrap(window.usagePace(now: halfway)).percentagePoints, 30,
                           accuracy: 0.00001)
        }
    }

    func testApiUsageIsReportedSeparately() throws {
        let w = try windows(recorded)
        XCTAssertEqual(w.map(\.id), ["auto", "api"])
        XCTAssertEqual(w[1].usedFraction ?? -1, 0.19, accuracy: 0.0001)
    }

    /// It is only worth a row when it has actually been touched.
    func testZeroApiUsageIsOmitted() throws {
        let json = """
        {"individualUsage":{"plan":{"autoPercentUsed":4,"apiPercentUsed":0}}}
        """
        XCTAssertEqual(try windows(json).map(\.id), ["auto"])
    }

    /// The reset is Cursor's own `billingCycleEnd` — Sep 24 here, which is what
    /// the dashboard prints, and is *not* a month after `billingCycleStart`.
    func testResetComesFromTheBillingCycleEnd() throws {
        let reset = try XCTUnwrap(windows(recorded)[0].resetsAt)
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        XCTAssertEqual(calendar.component(.month, from: reset), 9)
        XCTAssertEqual(calendar.component(.day, from: reset), 24)
    }

    func testOnDemandOnlyCountsWhenSwitchedOnWithACeiling() throws {
        let on = """
        {"individualUsage":{"plan":{"autoPercentUsed":5},
                            "onDemand":{"enabled":true,"used":3,"limit":50}}}
        """
        XCTAssertEqual(try windows(on).map(\.id), ["auto", "on_demand"])

        let off = """
        {"individualUsage":{"plan":{"autoPercentUsed":5},
                            "onDemand":{"enabled":false,"used":0,"limit":null}}}
        """
        XCTAssertEqual(try windows(off).map(\.id), ["auto"])
    }

    func testNothingMeteredRatherThanAFalseZero() {
        let json = #"{"membershipType":"free","individualUsage":{"plan":{}}}"#
        XCTAssertThrowsError(try windows(json)) { error in
            guard case UsageProviderError.nothingMetered = error else {
                return XCTFail("expected nothingMetered, got \(error)")
            }
        }
    }

    func testRejectsRubbish() {
        XCTAssertThrowsError(try windows("not json"))
    }

    /// The blended total is not a dashboard row. A Pro+ account at 19.87%
    /// Cursor Models / 100% API used to print 25% because the ring followed
    /// `totalPercentUsed`.
    func testTheBlendIsNotARow() throws {
        let json = """
        {"individualUsage":{"plan":{
          "autoPercentUsed":19.870833333333334,
          "apiPercentUsed":100,
          "totalPercentUsed":25.123586744639375}}}
        """
        let w = try windows(json)
        XCTAssertEqual(w.map(\.id), ["auto", "api"])
        XCTAssertEqual(w[0].label, CursorUsage.modelsLabel)
        XCTAssertEqual(CursorUsage.headlineID(in: w), "auto")
        XCTAssertEqual(w[0].summary, "20% Used · 80% left")
        let snap = ProviderSnapshot(
            id: "cursor", displayName: "Cursor", glyph: .cursor,
            fidelity: .official, status: .ok, windows: w,
            headlineID: CursorUsage.headlineID(in: w)
        )
        XCTAssertEqual(snap.headlineText, "20%")
    }
}

/// Borrowing the editor's session is what stops the notch reporting a different
/// account's usage — signing into cursor.com separately made a second, empty one.
/// The CLI session is the same cookie, reached only when the editor has none.
final class CursorCredentialsTests: XCTestCase {
    func testCookieIsTheAccountAndTokenPair() {
        let credentials = CursorCredentials(accountID: "google-oauth2|user_ABC", accessToken: "tok")
        XCTAssertEqual(credentials.sessionCookie, "WorkosCursorSessionToken=google-oauth2|user_ABC::tok")
    }

    func testAMissingStoreMeansSignedOutRatherThanAnError() {
        let missing = URL(fileURLWithPath: "/tmp/definitely-not-here-\(UUID().uuidString).vscdb")
        XCTAssertThrowsError(try CursorCredentials.load(from: missing)) { error in
            guard case UsageProviderError.needsAuth = error else {
                return XCTFail("expected needsAuth, got \(error)")
            }
        }
    }

    /// The editor path must not become a live keychain read. A missing store
    /// with no agent token is still signed out, even on a machine that has
    /// `cursor-agent` installed.
    func testAMissingEditorAndNoAgentTokenIsSignedOut() {
        let missing = URL(fileURLWithPath: "/tmp/definitely-not-here-\(UUID().uuidString).vscdb")
        let config = URL(fileURLWithPath: "/tmp/definitely-not-here-\(UUID().uuidString).json")
        XCTAssertThrowsError(
            try CursorCredentials.load(editorStore: missing, agentToken: nil, agentConfig: config)
        ) { error in
            guard case UsageProviderError.needsAuth = error else {
                return XCTFail("expected needsAuth, got \(error)")
            }
        }
    }

    func testAgentCookieUsesAuthIdFromCliConfig() throws {
        let token = Self.jwt(sub: "auth0|user_JWT", exp: Date().timeIntervalSince1970 + 3600)
        let config = try writeAgentConfig(authId: "auth0|user_CLI", userId: 42, email: "cli@example.com")
        defer { try? FileManager.default.removeItem(at: config) }

        let credentials = try CursorCredentials.session(fromAgentToken: token, configURL: config)
        XCTAssertEqual(credentials.accountID, "auth0|user_CLI")
        XCTAssertEqual(credentials.sessionCookie,
                       "WorkosCursorSessionToken=auth0|user_CLI::\(token)")
    }

    /// Numeric userId is accepted by the same endpoint, and is what remains
    /// when an older config has no authId.
    func testAgentAccountIDFallsBackToUserIdThenJWTSub() throws {
        let token = Self.jwt(sub: "auth0|user_JWT", exp: Date().timeIntervalSince1970 + 3600)

        let withUser = try writeAgentConfig(authId: nil, userId: 99, email: "cli@example.com")
        defer { try? FileManager.default.removeItem(at: withUser) }
        XCTAssertEqual(CursorCredentials.agentAccountID(token: token, configURL: withUser), "99")

        let missing = URL(fileURLWithPath: "/tmp/definitely-not-here-\(UUID().uuidString).json")
        XCTAssertEqual(CursorCredentials.agentAccountID(token: token, configURL: missing),
                       "auth0|user_JWT")
    }

    func testExpiredAgentTokenKeepsTheLastReading() {
        let token = Self.jwt(sub: "auth0|user_JWT", exp: 1)
        let missing = URL(fileURLWithPath: "/tmp/definitely-not-here-\(UUID().uuidString).json")
        XCTAssertThrowsError(
            try CursorCredentials.session(fromAgentToken: token, configURL: missing)
        ) { error in
            guard case UsageProviderError.credentialExpired = error else {
                return XCTFail("expected credentialExpired, got \(error)")
            }
        }
    }

    func testAgentAccountReadsEmailFromCliConfig() throws {
        let config = try writeAgentConfig(authId: "auth0|user_CLI", userId: 1, email: "cli@example.com")
        defer { try? FileManager.default.removeItem(at: config) }

        let account = try XCTUnwrap(CursorCredentials.agentAccount(from: config))
        XCTAssertEqual(account.label, "cli@example.com")
        XCTAssertEqual(account.source, "cursor-agent")
    }

    /// A signed-in editor wins so a laptop with both tools cannot flip
    /// accounts depending on which file we happened to read.
    func testTheEditorSessionIsPreferredOverTheAgent() throws {
        let store = try writeEditorStore(account: "auth0|user_EDITOR", token: "editor-tok")
        defer { try? FileManager.default.removeItem(at: store) }
        let token = Self.jwt(sub: "auth0|user_JWT", exp: Date().timeIntervalSince1970 + 3600)
        let config = try writeAgentConfig(authId: "auth0|user_CLI", userId: 1, email: "cli@example.com")
        defer { try? FileManager.default.removeItem(at: config) }

        let credentials = try CursorCredentials.load(editorStore: store, agentToken: token,
                                                     agentConfig: config)
        XCTAssertEqual(credentials.sessionCookie,
                       "WorkosCursorSessionToken=auth0|user_EDITOR::editor-tok")
    }

    func testAMissingEditorFallsThroughToTheAgent() throws {
        let missing = URL(fileURLWithPath: "/tmp/definitely-not-here-\(UUID().uuidString).vscdb")
        let token = Self.jwt(sub: "auth0|user_JWT", exp: Date().timeIntervalSince1970 + 3600)
        let config = try writeAgentConfig(authId: "auth0|user_CLI", userId: 1, email: "cli@example.com")
        defer { try? FileManager.default.removeItem(at: config) }

        let credentials = try CursorCredentials.load(editorStore: missing, agentToken: token,
                                                     agentConfig: config)
        XCTAssertEqual(credentials.accountID, "auth0|user_CLI")
        XCTAssertEqual(credentials.accessToken, token)
    }

    func testSignInOpensTheEditorWhenItIsInstalled() {
        guard case .openApp(let bundleID, let name) =
                CursorCredentials.signInRoute(editorInstalled: true) else {
            return XCTFail("expected openApp")
        }
        XCTAssertEqual(bundleID, CursorCredentials.bundleID)
        XCTAssertEqual(name, "Cursor")
    }

    func testSignInNamesTheCLIWhenTheEditorIsMissing() {
        guard case .guidance(let text) =
                CursorCredentials.signInRoute(editorInstalled: false) else {
            return XCTFail("expected guidance")
        }
        XCTAssertTrue(text.contains("cursor-agent login"), text)
    }

    private func writeAgentConfig(authId: String?, userId: Int, email: String) throws -> URL {
        let url = MatraTestFiles.temporaryDirectory
            .appendingPathComponent("cursor-cli-\(UUID().uuidString).json")
        var authInfo: [String: Any] = ["email": email, "userId": userId]
        if let authId { authInfo["authId"] = authId }
        try JSONSerialization.data(withJSONObject: ["authInfo": authInfo])
            .write(to: url)
        return url
    }

    private func writeEditorStore(account: String, token: String) throws -> URL {
        let url = MatraTestFiles.temporaryDirectory
            .appendingPathComponent("cursor-\(UUID().uuidString).vscdb")
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(url.path, &db), SQLITE_OK)
        sqlite3_exec(db, "CREATE TABLE ItemTable (key TEXT, value TEXT);", nil, nil, nil)
        sqlite3_exec(db, "INSERT INTO ItemTable VALUES ('cursorAuth/accessToken', '\(token)');",
                     nil, nil, nil)
        sqlite3_exec(db, "INSERT INTO ItemTable VALUES ('cursorAuth/stripeMembershipAuthId', '\(account)');",
                     nil, nil, nil)
        sqlite3_close(db)
        return url
    }

    /// Unsigned, for tests only. The server never sees these; they exist so
    /// expiry and `sub` can be pinned without a live keychain item.
    private static func jwt(sub: String, exp: TimeInterval) -> String {
        func encode(_ object: [String: Any]) -> String {
            let data = try! JSONSerialization.data(withJSONObject: object)
            return data.base64EncodedString()
                .replacingOccurrences(of: "+", with: "-")
                .replacingOccurrences(of: "/", with: "_")
                .trimmingCharacters(in: CharacterSet(charactersIn: "="))
        }
        return encode(["alg": "none", "typ": "JWT"])
            + "." + encode(["sub": sub, "exp": exp])
            + ".sig"
    }

    func testSubjectIsReadFromTheAccessTokenJWT() {
        let token = Self.jwt(sub: "auth0|user_ABC", exp: Date().timeIntervalSince1970 + 3600)
        XCTAssertEqual(CursorCredentials.subject(fromJWT: token), "auth0|user_ABC")
    }
}

/// Confirmed against a real account switched mid-cycle: Cursor reports 0% and
/// means it. An earlier version suppressed this as "no allowance to be a
/// percentage of", which hid a correct reading — Cursor's own response says
/// "You've used 0% of your included total usage" in the same payload.
final class CursorZeroUsageTests: XCTestCase {
    /// Verbatim from a live free account, just after signing into a new one.
    private let freePlan = """
    {"billingCycleEnd":"2026-09-20T01:35:15.142Z","membershipType":"free","isUnlimited":false,
     "autoModelSelectedDisplayMessage":"You've used 0% of your included total usage",
     "individualUsage":{"plan":{"enabled":true,"used":0,"limit":0,"remaining":0,
       "breakdown":{"included":0,"bonus":0,"total":0},
       "autoPercentUsed":0,"apiPercentUsed":0,"totalPercentUsed":0},
      "onDemand":{"enabled":false,"used":0,"limit":null,"remaining":null}}}
    """

    func testZeroPercentIsShownRatherThanSuppressed() throws {
        let windows = try CursorUsage.windows(fromJSON: freePlan)
        XCTAssertEqual(windows.first?.id, "auto")
        XCTAssertEqual(windows.first?.usedFraction, 0)
    }

    func testItStillReadsARealPercentage() throws {
        let used = freePlan.replacingOccurrences(of: "\"autoPercentUsed\":0",
                                                 with: "\"autoPercentUsed\":34")
        let windows = try CursorUsage.windows(fromJSON: used)
        XCTAssertEqual(windows.first?.usedFraction ?? 0, 0.34, accuracy: 0.0001)
    }
}


/// Enterprise / team plans omit `plan.totalPercentUsed` and meter a hard
/// `overall` ceiling instead. Verbatim shape from a live enterprise account.
final class CursorEnterpriseUsageTests: XCTestCase {
    private let recorded = """
    {"billingCycleStart":"2026-09-01T00:00:00.000Z",
     "billingCycleEnd":"2026-10-01T00:00:00.000Z",
     "membershipType":"enterprise","limitType":"team","isUnlimited":false,
     "autoModelSelectedDisplayMessage":"You've used 0% of your included total usage",
     "namedModelSelectedDisplayMessage":"You've used 0% of your included API usage",
     "individualUsage":{
       "overall":{"enabled":true,"used":6907,"limit":45000,"remaining":38093}},
     "teamUsage":{
       "onDemand":{"enabled":true,"used":0,"limit":1000000,"remaining":1000000}}}
    """

    func testOverallCeilingIsTheIncludedWindow() throws {
        let windows = try CursorUsage.windows(fromJSON: recorded)
        XCTAssertEqual(windows.map(\.id), ["included"])
        XCTAssertEqual(windows[0].label, "Included usage")
        XCTAssertEqual(windows[0].usedFraction ?? -1, 6907.0 / 45000.0, accuracy: 0.0001)
    }

    func testUnusedTeamOnDemandIsOmitted() throws {
        let windows = try CursorUsage.windows(fromJSON: recorded)
        XCTAssertFalse(windows.contains { $0.id == "team_on_demand" })
    }

    func testTeamOnDemandAppearsOnceTouched() throws {
        let touched = recorded.replacingOccurrences(
            of: #""onDemand":{"enabled":true,"used":0,"limit":1000000,"remaining":1000000}"#,
            with: #""onDemand":{"enabled":true,"used":250000,"limit":1000000,"remaining":750000}"#)
        let windows = try CursorUsage.windows(fromJSON: touched)
        XCTAssertEqual(windows.map(\.id), ["included", "team_on_demand"])
        XCTAssertEqual(windows[1].usedFraction ?? -1, 0.25, accuracy: 0.0001)
    }

    /// `headline` resolves the declared id by exact match and shows nothing
    /// if it does not appear — the enterprise shape has no "auto" or "api"
    /// window at all, so the headline has to name "included" or the ring
    /// reads as unmetered despite `windows` holding a real reading.
    func testTheHeadlineResolvesToTheEnterpriseCeiling() throws {
        let windows = try CursorUsage.windows(fromJSON: recorded)
        XCTAssertEqual(CursorUsage.headlineID(in: windows), "included")
    }
}

/// Fixed values produced independently with Python hashlib PBKDF2 and
/// OpenSSL AES-128-CBC. They contain only invented credentials.
private enum CursorProtectedFixtures {
    static let password = Data("matra-synthetic-password".utf8)
    static let key = Data([0xb1, 0xa1, 0x74, 0x8d, 0x17, 0xe9, 0xc6, 0xf1,
                           0xb1, 0x2c, 0x5e, 0x7c, 0x91, 0x99, 0xf4, 0xbf])
    static let plaintext = "synthetic-token"
    static let wrapped = "cursor-safe-storage:v1:djEw8XMfUKD6qXuz8A0AEOD5Jw=="
    static let jwt = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJhdXRoMHxzeW50aGV0aWMtZWRpdG9yIiwiZXhwIjo0MTAyNDQ0ODAwfQ.sig"
    static let wrappedJWT = "cursor-safe-storage:v1:djEwaxzrrphpEqxqP7Yc+aaqPvEfSq3InJv0Vr9bhLBEmJKyzSaEsA2RMzxf6VoI+i4McSQltNvDr7RZpujRO7E4yOxln3xTubdDgtY+/apNazOCIVA9y+qDuhM7hI8m7Ims"
    static let expiredJWT = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJhdXRoMHxzeW50aGV0aWMtZWRpdG9yIiwiZXhwIjoxfQ.sig"
    static let wrappedExpiredJWT = "cursor-safe-storage:v1:djEwaxzrrphpEqxqP7Yc+aaqPvEfSq3InJv0Vr9bhLBEmJKyzSaEsA2RMzxf6VoI+i4McSQltNvDr7RZpujRO7E4yO+/K9fwyNmW1eKmRKzTRAc="
    static let now = Date(timeIntervalSince1970: 1_800_000_000)
}

final class CursorSafeStorageTests: XCTestCase {
    func testPlaintextIsUnchangedAndDoesNotReadAKey() throws {
        let plaintext = "legacy-token-preserved-exactly"
        XCTAssertEqual(try CursorSafeStorage.decode(plaintext, keyReader: {
            XCTFail("a plaintext token has no Keychain dependency")
            throw UsageProviderError.needsAuth
        }), plaintext)
    }

    func testPBKDF2MatchesTheIndependentVector() throws {
        XCTAssertEqual(try CursorSafeStorage.deriveKey(password: CursorProtectedFixtures.password),
                       CursorProtectedFixtures.key)
    }

    func testDecryptsTheIndependentSafeStorageVector() throws {
        XCTAssertEqual(try CursorSafeStorage.decode(CursorProtectedFixtures.wrapped, keyReader: {
            CursorProtectedFixtures.key
        }), CursorProtectedFixtures.plaintext)
    }

    func testUnsupportedAndMalformedWrappersFailBeforeReadingAKey() {
        let invalid = [
            "cursor-safe-storage:v2:djEw8XMfUKD6qXuz8A0AEOD5Jw==",
            "CURSOR-SAFE-STORAGE:v1:djEw8XMfUKD6qXuz8A0AEOD5Jw==",
            " cursor-safe-storage:v1:djEw8XMfUKD6qXuz8A0AEOD5Jw==",
            "cursor-safe-storage:v1:",
            "cursor-safe-storage:v1:not base64!",
            "cursor-safe-storage:v1:djEw8XMfUKD6qXuz8A0AEOD5Jw==\n",
            "cursor-safe-storage:v1:" + Data("v11".utf8).base64EncodedString(),
            "cursor-safe-storage:v1:" + Data("v10".utf8).base64EncodedString(),
            "cursor-safe-storage:v1:" + Data("v10bad-block".utf8).base64EncodedString()
        ]
        for token in invalid {
            XCTAssertThrowsError(try CursorSafeStorage.decode(token, keyReader: {
                XCTFail("invalid envelopes must not consult the Keychain")
                return CursorProtectedFixtures.key
            }), token) { error in
                guard case UsageProviderError.needsAuth = error else { return XCTFail("\(error)") }
            }
        }
    }

    func testWrongLengthKeyAndInvalidPaddingFailClosed() {
        XCTAssertThrowsError(try CursorSafeStorage.decode(CursorProtectedFixtures.wrapped, keyReader: {
            Data(repeating: 0, count: 15)
        }))
        XCTAssertThrowsError(try CursorSafeStorage.decode(CursorProtectedFixtures.wrapped, keyReader: {
            Data(repeating: 0, count: 16)
        }))
    }

    func testKeyRefusalIsPreservedAsAccessDenied() {
        XCTAssertThrowsError(try CursorSafeStorage.decode(CursorProtectedFixtures.wrapped, keyReader: {
            throw UsageProviderError.accessDenied
        })) { error in
            guard case UsageProviderError.accessDenied = error else { return XCTFail("\(error)") }
        }
    }

    func testPlaintextHeaderControlsAndCookieDelimitersFailClosed() {
        for token in ["token\r\nInjected:yes", "token\0", "token;another=cookie", "token,other", "token\\other"] {
            XCTAssertThrowsError(try CursorSafeStorage.decode(token, keyReader: {
                XCTFail("invalid plaintext must not consult the Keychain")
                return CursorProtectedFixtures.key
            }))
        }
    }

    func testAgentCannotSendAProtectedWrapperAsItsToken() {
        let missing = MatraTestFiles.temporaryDirectory.appendingPathComponent("missing-\(UUID()).json")
        XCTAssertThrowsError(try CursorCredentials.session(
            fromAgentToken: CursorProtectedFixtures.wrapped, configURL: missing
        ))
    }
}

final class CursorProtectedCredentialsTests: XCTestCase {
    private var directory: URL!

    override func setUpWithError() throws {
        directory = MatraTestFiles.temporaryDirectory.appendingPathComponent("cursor-protected-\(UUID())")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try FileManager.default.removeItem(at: directory)
    }

    func testEncryptedEditorIsDecodedBeforeSubjectAndCookieAreRead() throws {
        let store = try editor(token: CursorProtectedFixtures.wrappedJWT, account: nil, email: "editor@example.invalid")
        let credentials = try CursorCredentials.load(
            editorStore: store, agentConfig: try agent(), now: CursorProtectedFixtures.now,
            keyReader: { CursorProtectedFixtures.key }, agentToken: {
                XCTFail("the editor must win over the agent")
                return "agent-token"
            }
        )
        XCTAssertEqual(credentials.accessToken, CursorProtectedFixtures.jwt)
        XCTAssertEqual(credentials.accountID, "auth0|synthetic-editor")
        XCTAssertEqual(credentials.sessionCookie,
                       "WorkosCursorSessionToken=auth0|synthetic-editor::\(CursorProtectedFixtures.jwt)")
    }

    func testRefusedEncryptedEditorCannotFallThroughToAgent() throws {
        let store = try editor(token: CursorProtectedFixtures.wrappedJWT, account: "editor", email: nil)
        let config = try agent()
        XCTAssertThrowsError(try CursorCredentials.load(
            editorStore: store, agentConfig: config,
            keyReader: { throw UsageProviderError.accessDenied }, agentToken: {
                XCTFail("a refused editor session is not permission to change accounts")
                return "agent-token"
            }
        )) { error in
            guard case UsageProviderError.accessDenied = error else { return XCTFail("\(error)") }
        }
    }

    func testMalformedEditorCannotFallThroughToAgent() throws {
        for token in ["cursor-safe-storage:v2:anything", "cursor-safe-storage:v1:invalid"] {
            let store = try editor(token: token, account: "editor", email: nil)
            XCTAssertThrowsError(try CursorCredentials.load(
                editorStore: store, agentConfig: try agent(),
                keyReader: { XCTFail("bad wrappers must not need a key"); return CursorProtectedFixtures.key },
                agentToken: { XCTFail("bad wrappers must not select the agent"); return "agent-token" }
            ))
        }
    }

    func testPlaintextAndEncryptedEditorExpiryMatchAgentExpiry() throws {
        for token in [CursorProtectedFixtures.expiredJWT, CursorProtectedFixtures.wrappedExpiredJWT] {
            let store = try editor(token: token, account: "editor", email: nil)
            XCTAssertThrowsError(try CursorCredentials.load(
                editorStore: store, agentConfig: try agent(), now: CursorProtectedFixtures.now,
                keyReader: { CursorProtectedFixtures.key }, agentToken: {
                    XCTFail("an expired editor session still selects the editor's account")
                    return CursorProtectedFixtures.jwt
                }
            )) { error in
                guard case UsageProviderError.credentialExpired = error else { return XCTFail("\(error)") }
            }
        }
    }

    func testStaleEditorEmailDoesNotSelectTheEditorWithoutItsToken() throws {
        let store = try editor(token: nil, account: "stale-editor", email: "stale@example.invalid")
        let config = try agent()
        let credentials = try CursorCredentials.load(editorStore: store, agentToken: CursorProtectedFixtures.jwt,
                                                     agentConfig: config, now: CursorProtectedFixtures.now)
        let account = CursorCredentials.account(editorStore: store, agentConfig: config, agentTokenPresent: true)
        XCTAssertEqual(credentials.accountID, "auth0|synthetic-agent")
        XCTAssertEqual(account?.source, "cursor-agent")
        XCTAssertEqual(account?.label, "agent@example.invalid")
        XCTAssertNil(CursorCredentials.account(editorStore: store, agentConfig: config, agentTokenPresent: false))
    }

    func testEncryptedEditorIdentityDoesNotRequireDecryptionOrAnEmail() throws {
        let config = try agent()
        let withoutEmail = try editor(token: CursorProtectedFixtures.wrappedJWT, account: nil, email: nil)
        let account = CursorCredentials.account(editorStore: withoutEmail, agentConfig: config,
                                                 agentTokenPresent: true)
        XCTAssertEqual(account?.source, "Cursor")
        XCTAssertNil(account?.label)

        let withEmail = try editor(token: CursorProtectedFixtures.wrappedJWT, account: nil,
                                   email: "editor@example.invalid")
        XCTAssertEqual(CursorCredentials.account(editorStore: withEmail, agentConfig: config,
                                                  agentTokenPresent: true)?.label,
                       "editor@example.invalid")
    }

    func testCorruptOrUnexpectedEditorStoreDoesNotSelectAgentCredentialsOrIdentity() throws {
        let corrupt = directory.appendingPathComponent("corrupt.vscdb")
        try Data("not a SQLite database".utf8).write(to: corrupt)
        let unexpected = directory.appendingPathComponent("no-item-table.vscdb")
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(unexpected.path, &db), SQLITE_OK)
        XCTAssertEqual(sqlite3_exec(db, "CREATE TABLE unrelated (value TEXT)", nil, nil, nil), SQLITE_OK)
        sqlite3_close(db)
        let config = try agent()
        for store in [corrupt, unexpected] {
            XCTAssertThrowsError(try CursorCredentials.load(
                editorStore: store, agentConfig: config,
                keyReader: { XCTFail("unreadable stores must not need the Keychain"); return CursorProtectedFixtures.key },
                agentToken: { XCTFail("unreadable stores must not select another account"); return CursorProtectedFixtures.jwt }
            ))
            XCTAssertNil(CursorCredentials.account(editorStore: store, agentConfig: config,
                                                    agentTokenPresent: true))
        }
    }

    func testAnEmptyEditorTokenAllowsTheAgentFallback() throws {
        let store = try editor(token: "", account: "old-editor", email: "old@example.invalid")
        let config = try agent()
        let credentials = try CursorCredentials.load(
            editorStore: store, agentConfig: config, now: CursorProtectedFixtures.now,
            keyReader: { XCTFail("an empty token has nothing to decrypt"); return CursorProtectedFixtures.key },
            agentToken: { CursorProtectedFixtures.jwt }
        )
        XCTAssertEqual(credentials.accountID, "auth0|synthetic-agent")
        XCTAssertEqual(CursorCredentials.account(editorStore: store, agentConfig: config,
                                                  agentTokenPresent: true)?.source, "cursor-agent")
    }

    private func editor(token: String?, account: String?, email: String?) throws -> URL {
        let url = directory.appendingPathComponent("editor-\(UUID()).vscdb")
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(url.path, &db), SQLITE_OK)
        defer { sqlite3_close(db) }
        XCTAssertEqual(sqlite3_exec(db, "CREATE TABLE ItemTable (key TEXT, value TEXT)", nil, nil, nil), SQLITE_OK)
        let rows: [(String, String?)] = [
            ("cursorAuth/accessToken", token), ("cursorAuth/stripeMembershipAuthId", account),
            ("cursorAuth/cachedEmail", email), ("cursorAuth/stripeMembershipType", "pro")
        ]
        for (key, value) in rows {
            guard let value else { continue }
            var statement: OpaquePointer?
            XCTAssertEqual(sqlite3_prepare_v2(db, "INSERT INTO ItemTable VALUES (?, ?)", -1, &statement, nil), SQLITE_OK)
            let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)
            sqlite3_bind_text(statement, 1, key, -1, transient)
            sqlite3_bind_text(statement, 2, value, -1, transient)
            XCTAssertEqual(sqlite3_step(statement), SQLITE_DONE)
            sqlite3_finalize(statement)
        }
        return url
    }

    private func agent() throws -> URL {
        let url = directory.appendingPathComponent("agent-\(UUID()).json")
        try JSONSerialization.data(withJSONObject: ["authInfo": [
            "authId": "auth0|synthetic-agent", "email": "agent@example.invalid", "userId": 99
        ]]).write(to: url)
        return url
    }
}

/// This test double persists a refusal across reader instances entirely in
/// memory. It never writes a system preferences domain.
private final class CursorRefusalDefaults: UserDefaults, @unchecked Sendable {
    private var flags: [String: Bool] = [:]
    private let flagLock = NSLock()

    override func bool(forKey defaultName: String) -> Bool {
        flagLock.lock(); defer { flagLock.unlock() }
        return flags[defaultName] ?? false
    }

    override func set(_ value: Any?, forKey defaultName: String) {
        flagLock.lock(); defer { flagLock.unlock() }
        flags[defaultName] = value as? Bool
    }

    override func set(_ value: Bool, forKey defaultName: String) {
        flagLock.lock(); defer { flagLock.unlock() }
        flags[defaultName] = value
    }

    override func removeObject(forKey defaultName: String) {
        flagLock.lock(); defer { flagLock.unlock() }
        flags.removeValue(forKey: defaultName)
    }
}

final class CursorKeychainPermissionTests: XCTestCase {
    private final class Reads {
        var modes: [Bool] = []
        var deny = false
        var now = Date(timeIntervalSince1970: 1_000)
        var stamp = Date(timeIntervalSince1970: 1_000)
        var key = CursorProtectedFixtures.key
    }

    private func keychain(_ reads: Reads, refusals: UserDefaults? = nil) -> CursorKeychainReader<Data> {
        CursorKeychainReader(refusalKey: "cursor-test", now: { reads.now }, refusals: refusals,
                             itemModifiedAt: { reads.stamp }, isExpired: { _ in true }, reader: { interactive in
            reads.modes.append(interactive)
            if reads.deny { throw UsageProviderError.accessDenied }
            return reads.key
        })
    }

    func testBackgroundReadAndSilentCacheInvalidationNeverPrompt() throws {
        let reads = Reads(), keychain = keychain(reads)
        _ = try keychain.load()
        _ = try keychain.load()
        keychain.forgetCached()
        _ = try keychain.load()
        XCTAssertEqual(reads.modes, [false, false])
    }

    func testExplicitAccessGrantsExactlyOneInteractiveRead() throws {
        let reads = Reads(), keychain = keychain(reads)
        keychain.askAgain()
        _ = try keychain.load()
        keychain.forgetCached()
        _ = try keychain.load()
        XCTAssertEqual(reads.modes, [true, false])
    }

    func testExplicitDenyStopsReadsAcrossRotationAndRelaunchUntilAskedAgain() throws {
        let defaults = CursorRefusalDefaults()
        let reads = Reads(), first = keychain(reads, refusals: defaults)
        reads.deny = true
        first.askAgain()
        XCTAssertThrowsError(try first.load())
        XCTAssertTrue(first.isRefused)
        reads.deny = false
        reads.stamp = reads.stamp.addingTimeInterval(1)
        let relaunched = keychain(reads, refusals: defaults)
        relaunched.forgetCached()
        XCTAssertThrowsError(try relaunched.load())
        XCTAssertEqual(reads.modes, [true])
        relaunched.askAgain()
        XCTAssertEqual(try relaunched.load(), CursorProtectedFixtures.key)
        XCTAssertFalse(relaunched.isRefused)
        relaunched.forgetCached()
        _ = try relaunched.load()
        XCTAssertEqual(reads.modes, [true, true, false])
    }

    func testBackgroundRefusalDoesNotRecordAnExplicitDeny() {
        let reads = Reads(), keychain = keychain(reads)
        reads.deny = true
        XCTAssertThrowsError(try keychain.load())
        XCTAssertFalse(keychain.isRefused)
    }

    func testAnUnspentInteractivePermissionExpires() throws {
        let reads = Reads(), keychain = keychain(reads)
        keychain.askAgain()
        reads.now = reads.now.addingTimeInterval(PromptPermission.window + 1)
        _ = try keychain.load()
        XCTAssertEqual(reads.modes, [false])
    }

    func testCachedSafeStorageKeyIsReusedUntilItsItemRotates() throws {
        let reads = Reads(), keychain = keychain(reads)
        XCTAssertEqual(try keychain.load(), CursorProtectedFixtures.key)
        reads.key = Data(repeating: 0x55, count: 16)
        XCTAssertEqual(try keychain.load(), CursorProtectedFixtures.key)
        reads.stamp = reads.stamp.addingTimeInterval(1)
        XCTAssertEqual(try keychain.load(), reads.key)
        XCTAssertEqual(reads.modes, [false, false])
    }
}

private final class CursorProviderCalls: @unchecked Sendable {
    private let lock = NSLock()
    private var accessCount = 0
    private var forgottenCount = 0
    func ask() { lock.lock(); defer { lock.unlock() }; accessCount += 1 }
    func forget() { lock.lock(); defer { lock.unlock() }; forgottenCount += 1 }
    var counts: (access: Int, forgotten: Int) {
        lock.lock(); defer { lock.unlock() }
        return (accessCount, forgottenCount)
    }
}

final class CursorLocalProviderProtectedTests: XCTestCase {
    private var session: URLSession!

    override func setUp() {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [CursorProtectedEndpoint.self]
        config.httpCookieStorage = nil
        session = URLSession(configuration: config)
        CursorProtectedEndpoint.reset(status: 200)
    }

    override func tearDown() {
        session.invalidateAndCancel()
    }

    func testOnlyDecodedEditorTokenReachesTheExistingCookieEndpoint() async throws {
        let token = try CursorSafeStorage.decode(CursorProtectedFixtures.wrappedJWT,
                                                 keyReader: { CursorProtectedFixtures.key })
        let provider = CursorLocalProvider(session: session, loadCredentials: {
            CursorCredentials(accountID: "auth0|synthetic-editor", accessToken: token)
        }, loadAccount: { nil }, askForAccess: {}, forgetCredentials: {})
        _ = try await provider.fetchSnapshot()
        let request = try XCTUnwrap(CursorProtectedEndpoint.requests.last)
        XCTAssertEqual(request.url?.absoluteString, "https://cursor.com/api/usage-summary")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Cookie"),
                       "WorkosCursorSessionToken=auth0|synthetic-editor::\(CursorProtectedFixtures.jwt)")
        XCTAssertNil(request.value(forHTTPHeaderField: "Authorization"))
    }

    func testRefusedCredentialDoesNotSendAnyHTTPRequest() async {
        let provider = CursorLocalProvider(session: session, loadCredentials: {
            throw UsageProviderError.accessDenied
        }, loadAccount: { nil }, askForAccess: {}, forgetCredentials: {})
        do {
            _ = try await provider.fetchSnapshot()
            XCTFail("expected accessDenied")
        } catch UsageProviderError.accessDenied {
        } catch { XCTFail("\(error)") }
        XCTAssertTrue(CursorProtectedEndpoint.requests.isEmpty)
    }

    func testAProtectedWrapperCannotBecomeAnHTTPRequestEvenFromAnInjectedLoader() async {
        let provider = CursorLocalProvider(session: session, loadCredentials: {
            CursorCredentials(accountID: "account", accessToken: CursorProtectedFixtures.wrapped)
        }, loadAccount: { nil }, askForAccess: {}, forgetCredentials: {})
        do {
            _ = try await provider.fetchSnapshot()
            XCTFail("expected needsAuth")
        } catch UsageProviderError.needsAuth {
        } catch { XCTFail("\(error)") }
        XCTAssertTrue(CursorProtectedEndpoint.requests.isEmpty)
    }

    func testServerRejectionForgetsSilentlyAndNeverGrantsPromptPermission() async {
        let calls = CursorProviderCalls()
        let provider = CursorLocalProvider(session: session, loadCredentials: {
            CursorCredentials(accountID: "account", accessToken: "synthetic-token")
        }, loadAccount: { nil }, askForAccess: { calls.ask() }, forgetCredentials: { calls.forget() })
        for status in [401, 403] {
            CursorProtectedEndpoint.reset(status: status)
            do {
                _ = try await provider.fetchSnapshot()
                XCTFail("expected needsAuth")
            } catch UsageProviderError.needsAuth {
            } catch { XCTFail("\(error)") }
        }
        XCTAssertEqual(calls.counts.forgotten, 2)
        XCTAssertEqual(calls.counts.access, 0)
        provider.forgetCachedCredential()
        XCTAssertEqual(calls.counts.access, 1)
    }

    func testHTTPRejectionDoesNotRequireAnotherReadOfAnUnchangedSafeStorageKey() async throws {
        let reads = CursorProviderCalls()
        let keychain = CursorKeychainReader<Data>(
            refusalKey: "synthetic-http-key", itemModifiedAt: { CursorProtectedFixtures.now },
            isExpired: { _ in true }, reader: { interactive in
                XCTAssertTrue(interactive, "a second read after Allow once would be refused")
                reads.ask()
                return CursorProtectedFixtures.key
            }
        )
        keychain.askAgain()
        _ = try keychain.load()
        let provider = CursorLocalProvider(session: session, loadCredentials: {
            let token = try CursorSafeStorage.decode(CursorProtectedFixtures.wrappedJWT,
                                                     keyReader: { try keychain.load() })
            return CursorCredentials(accountID: "auth0|synthetic-editor", accessToken: token)
        }, loadAccount: { nil }, askForAccess: {})
        CursorProtectedEndpoint.reset(status: 401)
        do {
            _ = try await provider.fetchSnapshot()
            XCTFail("expected needsAuth")
        } catch UsageProviderError.needsAuth {
        }
        CursorProtectedEndpoint.reset(status: 200)
        _ = try await provider.fetchSnapshot()
        XCTAssertEqual(reads.counts.access, 1, "HTTP rejection must not discard the safe-storage key")
    }
}

private final class CursorProtectedEndpoint: URLProtocol {
    private static let lock = NSLock()
    private static var status = 200
    private static var recorded: [URLRequest] = []
    static var requests: [URLRequest] {
        lock.lock(); defer { lock.unlock() }
        return recorded
    }

    static func reset(status: Int) {
        lock.lock(); defer { lock.unlock() }
        self.status = status
        recorded = []
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock()
        Self.recorded.append(request)
        let status = Self.status
        Self.lock.unlock()
        let response = HTTPURLResponse(url: request.url!, statusCode: status,
                                       httpVersion: "HTTP/1.1", headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(#"{"membershipType":"pro","individualUsage":{"plan":{"autoPercentUsed":19}}}"#.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
