import XCTest
@testable import Matra

/// Grok Bot, Higgsfield, oMLX and the OpenCode bridge, on fake caches, payloads
/// and process output only. Nothing here reads a real profile or keychain,
/// reaches a network, runs a CLI or installs into a real OpenCode config.
final class MatraProviderUpgradesTests: XCTestCase {
    private func temporaryDirectory() throws -> URL {
        let url = MatraTestFiles.temporaryDirectory
            .appendingPathComponent("MatraProviderUpgrades.\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func isolatedArchive() -> UsageArchive {
        let name = "MatraProviderUpgrades.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        addTeardownBlock { defaults.removePersistentDomain(forName: name) }
        return UsageArchive(defaults: defaults)
    }

    // MARK: - Grok Bot

    private func grokCache(percent: String = "40", readAtMs: String? = nil, expiresAtMs: String? = nil,
                           includedLimit: String = "true") -> Data {
        let now = Int(Date().timeIntervalSince1970 * 1000)
        return Data("""
        {"schemaVersion":2,"value":{"kind":"present","reading":{"usage":{"percentUsed":\(percent),\
        "nextResetMs":\(now + 86_400_000),"hasNonZeroIncludedLimit":\(includedLimit)},\
        "readAtMs":\(readAtMs ?? String(now))}\(expiresAtMs.map { #","expiresAtMs":\#($0)"# } ?? "")}}
        """.utf8)
    }

    private func writeGrok(_ data: Data, slot: String, in directory: URL) throws {
        try data.write(to: directory.appendingPathComponent(GrokBotCache.blobFilename(
            forKey: "sand.client.slice.account.\(slot).weekly-usage.cache")))
    }

    private func selectGrokSlot(_ slot: String, in directory: URL) throws {
        try Data(#"{"schemaVersion":1,"value":"\#(slot)"}"#.utf8).write(to: directory.appendingPathComponent(
            GrokBotCache.blobFilename(forKey: GrokBotCache.accountSlotKey)))
    }

    func testASelectedGrokAccountWithADamagedCacheNeverBorrowsAnotherAccountsWeek() throws {
        let directory = try temporaryDirectory()
        try writeGrok(Data("{broken".utf8), slot: "grok|user_active", in: directory)
        try writeGrok(grokCache(percent: "90"), slot: "grok|user_other", in: directory)
        try selectGrokSlot("grok|user_active", in: directory)
        XCTAssertThrowsError(try GrokBotCache.load(persistenceDir: directory)) { error in
            guard case UsageProviderError.badResponse = error else { return XCTFail("\(error)") }
        }
    }

    func testASelectedGrokAccountWithNoCacheIsNothingMeteredNotItsNeighbour() throws {
        let directory = try temporaryDirectory()
        try writeGrok(grokCache(percent: "90"), slot: "grok|user_other", in: directory)
        try selectGrokSlot("grok|user_active", in: directory)
        XCTAssertThrowsError(try GrokBotCache.load(persistenceDir: directory)) { error in
            guard case UsageProviderError.nothingMetered = error else { return XCTFail("\(error)") }
        }
        XCTAssertNil(GrokBotCache.account(persistenceDir: directory))
    }

    func testWithoutASelectedSlotADamagedFreshestCacheFallsBackToARealReading() throws {
        let directory = try temporaryDirectory()
        try writeGrok(grokCache(percent: "25"), slot: "grok|user_old", in: directory)
        let old = directory.appendingPathComponent(GrokBotCache.blobFilename(
            forKey: "sand.client.slice.account.grok|user_old.weekly-usage.cache"))
        try FileManager.default.setAttributes([.modificationDate: Date().addingTimeInterval(-86400)],
                                              ofItemAtPath: old.path)
        try writeGrok(Data("{broken".utf8), slot: "grok|user_new", in: directory)
        XCTAssertEqual(try GrokBotCache.load(persistenceDir: directory).percentUsed, 25, accuracy: 0.001)
    }

    func testAGrokCacheNamingNoExpiryStillGoesStaleADayAfterItsReading() async throws {
        let directory = try temporaryDirectory()
        let twoDaysAgo = Int(Date().addingTimeInterval(-2 * 86400).timeIntervalSince1970 * 1000)
        try writeGrok(grokCache(readAtMs: String(twoDaysAgo)), slot: "grok|user_a", in: directory)
        let snapshot = try await GrokBotProvider(persistenceDir: directory).fetchSnapshot()
        guard case .stale = snapshot.status else { return XCTFail("\(snapshot.status)") }

        let recent = try GrokBotCache.reading(from: grokCache(), fallbackDate: .distantPast)
        XCTAssertFalse(recent.isExpired)
    }

    func testAnExplicitGrokExpiryMarksTheReadingStale() async throws {
        let directory = try temporaryDirectory()
        let past = Int(Date().addingTimeInterval(-60).timeIntervalSince1970 * 1000)
        try writeGrok(grokCache(expiresAtMs: String(past)), slot: "grok|user_a", in: directory)
        let snapshot = try await GrokBotProvider(persistenceDir: directory).fetchSnapshot()
        guard case .stale = snapshot.status else { return XCTFail("\(snapshot.status)") }
    }

    func testGrokAbsenceIsNeverAFreshZero() {
        for body in [grokCache(percent: "true"), grokCache(percent: "null"), grokCache(percent: #""12""#),
                     grokCache(percent: "0", includedLimit: "false")] {
            XCTAssertThrowsError(try GrokBotCache.reading(from: body, fallbackDate: Date())) { error in
                guard case UsageProviderError.nothingMetered = error else { return XCTFail("\(error)") }
            }
        }
        XCTAssertEqual(try GrokBotCache.reading(from: grokCache(percent: "0"), fallbackDate: Date()).percentUsed, 0,
                       "a real zero on an account with an allowance is still a reading")
    }

    // MARK: - Higgsfield usage

    private func assertNothingMetered(_ json: String, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertThrowsError(try HiggsfieldUsage.parse(json), file: file, line: line) { error in
            guard case UsageProviderError.nothingMetered = error else {
                return XCTFail("\(error)", file: file, line: line)
            }
        }
    }

    func testHiggsfieldNeverGuessesAWorkspaceOrBorrowsANeighboursBalance() throws {
        assertNothingMetered(#"[{"name":"A","credits":10},{"name":"B","credits":20}]"#)
        assertNothingMetered(#"[{"name":"A","credits":10},{"name":"B","is_selected":true}]"#)
        let result = try HiggsfieldUsage.parse(#"[{"name":"A","credits":10,"is_selected":false},{"name":"B","credits":20,"is_selected":true}]"#)
        XCTAssertEqual(result.workspaceName, "B")
        XCTAssertEqual(result.windows.first?.remaining, 20)
    }

    func testHiggsfieldRejectsNumbersItCannotCountRatherThanCrashing() {
        assertNothingMetered(#"{"credits":1e300,"plan_type":"ultra"}"#)
        assertNothingMetered(#"{"credits":true,"plan_type":"ultra"}"#)
        assertNothingMetered(#"{"credits":null,"plan_type":"ultra"}"#)
    }

    func testHiggsfieldUnknownAllowancesStayCountsAndExplicitOnesWin() throws {
        for json in [#"{"credits":42,"plan_type":"mystery_plan"}"#, #"{"credits":42,"plan_type":"free"}"#,
                     #"{"credits":42}"#, #"{"credits":42,"allowance":0,"plan_type":"ultra"}"#] {
            let result = try HiggsfieldUsage.parse(json)
            let window = try XCTUnwrap(result.windows.first, json)
            XCTAssertNil(window.usedFraction, json)
            XCTAssertEqual(window.remaining, 42, json)
            XCTAssertEqual(result.fidelity, .official, json)
        }
        let explicit = try HiggsfieldUsage.parse(#"{"credits":50,"credit_limit":200,"plan_type":"ultra"}"#)
        XCTAssertEqual(explicit.windows.first?.usedFraction ?? -1, 0.75, accuracy: 0.001)
        XCTAssertEqual(explicit.fidelity, .official)
        let table = try HiggsfieldUsage.parse(#"{"credits":1500,"plan_type":"ultra"}"#)
        XCTAssertEqual(table.windows.first?.usedFraction ?? -1, 0.5, accuracy: 0.001)
        XCTAssertEqual(table.fidelity, .derived, "a percentage from our own plan table is not the provider's")
    }

    func testHiggsfieldOverdraftAndPlanNamesReadCleanly() throws {
        let result = try HiggsfieldUsage.parse(#"{"credits":-5,"plan_type":"pay_as_you_go"}"#)
        XCTAssertEqual(result.windows.first?.remaining, 0)
        XCTAssertEqual(result.planName, "Pay As You Go")
    }

    // MARK: - Higgsfield identity and process

    func testHiggsfieldOwnsItsKeychainNameInEachAppIdentity() {
        XCTAssertEqual(HiggsfieldCredentials.keychainService, MatraStorage.service("higgsfield-api-key"))
        XCTAssertTrue(HiggsfieldCredentials.keychainService.hasPrefix(MatraStorage.namespace + "."))
        XCTAssertEqual(HiggsfieldCredentials.keychainAccount, MatraStorage.keychainAccount)
        XCTAssertNotEqual(MatraStorage.installedBundleID, MatraStorage.previewBundleID)
    }

    func testTheCLINeverInheritsHuggingFaceSecretsAndCanFindItsRuntime() {
        let env = HiggsfieldCLI.childEnvironment(
            binary: URL(fileURLWithPath: "/fake/prefix/bin/higgsfield"),
            base: ["HF_TOKEN": "x", "HF_HOME": "y", "HUGGING_FACE_HUB_TOKEN": "z",
                   "HIGGSFIELD_API_KEY": "own", "HOME": "/fake/home", "PATH": "/usr/bin:/bin"]
        )
        XCTAssertNil(env["HF_TOKEN"])
        XCTAssertNil(env["HF_HOME"])
        XCTAssertNil(env["HUGGING_FACE_HUB_TOKEN"])
        XCTAssertEqual(env["HIGGSFIELD_API_KEY"], "own")
        XCTAssertEqual(env["HOME"], "/fake/home")
        XCTAssertEqual(env["PATH"]?.split(separator: ":").first, "/fake/prefix/bin")
        XCTAssertTrue(env["PATH"]?.contains("/usr/bin") == true)
    }

    func testCLIFailuresAreClassifiedFromEitherStreamsText() {
        guard case .needsAuth = HiggsfieldCLI.failure(status: 1, output: "Please run hf auth login") else {
            return XCTFail("expected needsAuth")
        }
        guard case .rateLimited = HiggsfieldCLI.failure(status: 1, output: "HTTP 429 Too Many Requests") else {
            return XCTFail("expected rateLimited")
        }
        guard case .apiError = HiggsfieldCLI.failure(status: 3, output: "boom") else {
            return XCTFail("expected apiError")
        }
    }

    func testRetryAfterIsHonouredAboveTheMinimum() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        XCTAssertEqual(HiggsfieldProvider.retryDelay("300", now: now), 300)
        XCTAssertEqual(HiggsfieldProvider.retryDelay("5", now: now), 60)
        XCTAssertEqual(HiggsfieldProvider.retryDelay(nil, now: now), 60)
        XCTAssertEqual(HiggsfieldProvider.retryDelay("soon", now: now), 60)
    }

    // MARK: - Higgsfield provider

    private func fakeSources(token: String? = nil,
                             cli: ((URL) throws -> String)? = nil) -> HiggsfieldCredentialSources {
        var sources = HiggsfieldCredentialSources()
        sources.environment = [:]
        sources.credentialsURL = URL(fileURLWithPath: "/nonexistent/\(UUID().uuidString).json")
        sources.settingsToken = { token }
        sources.settingsPresent = { token != nil }
        sources.deleteSettingsToken = {}
        sources.forgetCached = {}
        sources.locateBinary = { cli == nil ? nil : URL(fileURLWithPath: "/fake/higgsfield") }
        sources.runCLI = cli ?? { _ in throw UsageProviderError.apiError("unused") }
        return sources
    }

    private func stubbedSession() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [UpgradesStubProtocol.self]
        return URLSession(configuration: configuration)
    }

    func testFakeCLIOutputBecomesASnapshotCarryingThePlan() async throws {
        let provider = HiggsfieldProvider(
            session: stubbedSession(), archive: isolatedArchive(),
            sources: fakeSources(cli: { _ in #"[{"name":"Studio","plan_type":"ultra","credits":1500,"is_selected":true}]"# }))
        let snapshot = try await provider.fetchSnapshot()
        XCTAssertEqual(snapshot.plan, "Ultra")
        XCTAssertEqual(snapshot.fidelity, .derived)
        XCTAssertEqual(provider.account()?.label, "Studio")
    }

    func testACLITimeoutWithNoSavedKeyIsATimeoutNotASignOut() async {
        let provider = HiggsfieldProvider(
            session: stubbedSession(), archive: isolatedArchive(),
            sources: fakeSources(cli: { _ in throw UsageProviderError.timedOut }))
        do {
            _ = try await provider.fetchSnapshot()
            XCTFail("expected a failure")
        } catch UsageProviderError.timedOut {
        } catch {
            XCTFail("\(error)")
        }
    }

    func testAGatewayRateLimitIsHonouredAndNoFurtherRequestIsSpent() async throws {
        var requests = 0
        UpgradesStubProtocol.handler = { _ in
            requests += 1
            return (429, ["Retry-After": "300"], Data())
        }
        defer { UpgradesStubProtocol.handler = nil }
        let provider = HiggsfieldProvider(session: stubbedSession(), archive: isolatedArchive(),
                                          sources: fakeSources(token: "fake-token"))
        for _ in 0..<2 {
            do {
                _ = try await provider.fetchSnapshot()
                XCTFail("expected rateLimited")
            } catch UsageProviderError.rateLimited(let delay) {
                XCTAssertGreaterThan(delay, 60)
            }
        }
        XCTAssertEqual(requests, 1, "the back-off answers the second call without a request")
    }

    func testAGatewayReadingClearsAPersistedBackOff() async throws {
        let archive = isolatedArchive()
        UpgradesStubProtocol.handler = { _ in (200, [:], Data(#"{"credits":42}"#.utf8)) }
        defer { UpgradesStubProtocol.handler = nil }
        let start = Date()
        archive.saveBackoffUntil(start.addingTimeInterval(30), providerID: "higgsfield")
        let later = start.addingTimeInterval(31)
        let provider = HiggsfieldProvider(session: stubbedSession(), archive: archive,
                                          sources: fakeSources(token: "fake-token"), now: { later })
        let snapshot = try await provider.fetchSnapshot()
        XCTAssertEqual(snapshot.windows.first?.remaining, 42)
        XCTAssertNil(archive.loadBackoffUntil(providerID: "higgsfield"))
    }

    // MARK: - oMLX

    func testAnOMLXEntryOfUnknownLoadedStateIsNotANoModelsClaim() {
        XCTAssertThrowsError(try OMLXUsage.parse(Data(#"{"models":[{"id":"a"}]}"#.utf8))) { error in
            XCTAssertEqual(error as? OMLXError, .invalidResponse)
        }
        XCTAssertEqual(try OMLXUsage.parse(Data(#"{"models":[{"id":"a","loaded":false}]}"#.utf8)).models.count, 0)
    }

    func testAnOMLXSizeOfZeroIsUnknownRatherThanZeroBytes() throws {
        let reading = try OMLXUsage.parse(Data(#"{"models":[{"id":"a","loaded":true,"actual_size":0,"estimated_size":0}]}"#.utf8))
        XCTAssertNil(reading.models.first?.memoryBytes)
    }

    @MainActor
    func testOMLXOnlyEverReadsItsStatusListing() async throws {
        var seen: [URLRequest] = []
        UpgradesStubProtocol.handler = { request in
            seen.append(request)
            return (200, [:], Data(#"{"models":[]}"#.utf8))
        }
        defer { UpgradesStubProtocol.handler = nil }
        let provider = OMLXLocalProvider(endpoint: URL(string: OMLXEndpoint.defaultAddress)!,
                                         session: stubbedSession(), key: { nil })
        _ = try await provider.fetchSnapshot()
        XCTAssertEqual(seen.count, 1)
        XCTAssertEqual(seen.first?.httpMethod, "GET")
        XCTAssertEqual(seen.first?.url?.path, "/v1/models/status")
    }

    @MainActor
    func testTheBorrowedOMLXKeyIsNeverSentOffThisMac() async {
        var requests = 0
        UpgradesStubProtocol.handler = { _ in
            requests += 1
            return (200, [:], Data(#"{"models":[]}"#.utf8))
        }
        defer { UpgradesStubProtocol.handler = nil }
        let provider = OMLXLocalProvider(endpoint: URL(string: "http://192.0.2.1:8000")!,
                                         session: stubbedSession(), key: { "fake-key" })
        do {
            _ = try await provider.fetchSnapshot()
            XCTFail("expected invalidEndpoint")
        } catch {
            XCTAssertEqual(error as? OMLXError, .invalidEndpoint)
        }
        XCTAssertEqual(requests, 0)
    }

    // MARK: - OpenCode bridge

    private func bridgeTemplate(in directory: URL) throws -> URL {
        let url = directory.appendingPathComponent("template.mjs")
        try Data("""
        // \(OpenCodePluginInstaller.ownershipMarker)
        const SUPPORT_FOLDER = "Matra" /* matra:support-folder */
        """.utf8).write(to: url)
        return url
    }

    func testTheActivityFolderFollowsThisAppsIdentity() {
        let path = OpenCodeActivityMonitor.directory.path
        XCTAssertTrue(path.hasSuffix(MatraStorage.isPreviewIdentity ? "/Matra Preview/OpenCode" : "/Matra/OpenCode"), path)
    }

    func testAPreviewBridgeDiffersOnlyInItsFolderAndTheInstalledCopyIsUnchanged() throws {
        let template = Data(#"const SUPPORT_FOLDER = "Matra" /* matra:support-folder */"#.utf8)
        XCTAssertEqual(try OpenCodePluginInstaller.rendered(template: template, isPreview: false), template)
        XCTAssertEqual(try OpenCodePluginInstaller.rendered(template: template, isPreview: true),
                       Data(#"const SUPPORT_FOLDER = "Matra Preview" /* matra:support-folder */"#.utf8))
        XCTAssertThrowsError(try OpenCodePluginInstaller.rendered(template: Data("no line".utf8), isPreview: true),
                             "a preview must not fall back to the installed app's folder")
    }

    func testTheBundledBridgeCarriesTheMarkerAndThePreviewSeam() throws {
        let url = try XCTUnwrap(Bundle.main.url(forResource: "matra-opencode", withExtension: "mjs"))
        XCTAssertTrue(try String(contentsOf: url, encoding: .utf8).contains(OpenCodePluginInstaller.ownershipMarker))
        XCTAssertNoThrow(try OpenCodePluginInstaller.rendered(template: Data(contentsOf: url), isPreview: true))
    }

    func testPreviewAndInstalledBridgesOccupySeparateFiles() throws {
        let root = try temporaryDirectory()
        let plugins = root.appendingPathComponent("plugins")
        let source = try bridgeTemplate(in: root)
        try OpenCodePluginInstaller.install(directory: plugins, source: source, isPreview: false)
        try OpenCodePluginInstaller.install(directory: plugins, source: source, isPreview: true)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: plugins.path).sorted(),
                       ["matra-preview.js", "matra.js"])
        XCTAssertTrue(try OpenCodePluginInstaller.uninstall(directory: plugins, isPreview: true))
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: plugins.path), ["matra.js"],
                       "removing one identity's bridge leaves the other's")
    }

    func testInstallLeavesAnUnrelatedPluginAloneAndUninstallIsReversible() throws {
        let root = try temporaryDirectory()
        let plugins = root.appendingPathComponent("plugins")
        try FileManager.default.createDirectory(at: plugins, withIntermediateDirectories: true)
        let unrelated = Data("// someone else's plugin".utf8)
        try unrelated.write(to: plugins.appendingPathComponent("other.js"))
        let own = Data("// my own matra.js".utf8)
        try own.write(to: plugins.appendingPathComponent("matra.js"))

        let source = try bridgeTemplate(in: root)
        try OpenCodePluginInstaller.install(directory: plugins, source: source, isPreview: false)
        XCTAssertEqual(try Data(contentsOf: plugins.appendingPathComponent("other.js")), unrelated)

        XCTAssertTrue(try OpenCodePluginInstaller.uninstall(directory: plugins, isPreview: false))
        XCTAssertEqual(try Data(contentsOf: plugins.appendingPathComponent("matra.js")), own,
                       "the file the bridge displaced comes back")
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: plugins.path).sorted(),
                       ["matra.js", "other.js"])
    }

    func testUninstallNeverRemovesAFileThatIsNotTheBridge() throws {
        let plugins = try temporaryDirectory()
        let own = Data("// my own matra.js".utf8)
        try own.write(to: plugins.appendingPathComponent("matra.js"))
        XCTAssertFalse(try OpenCodePluginInstaller.uninstall(directory: plugins, isPreview: false))
        XCTAssertEqual(try Data(contentsOf: plugins.appendingPathComponent("matra.js")), own)
    }

    func testUninstallDoesNotRestoreAnOlderBridgeAsIfItWereTheUsers() throws {
        let root = try temporaryDirectory()
        let plugins = root.appendingPathComponent("plugins")
        try FileManager.default.createDirectory(at: plugins, withIntermediateDirectories: true)
        try Data("// \(OpenCodePluginInstaller.ownershipMarker) old version".utf8)
            .write(to: plugins.appendingPathComponent("matra.js"))
        try OpenCodePluginInstaller.install(directory: plugins, source: try bridgeTemplate(in: root), isPreview: false)
        XCTAssertTrue(try OpenCodePluginInstaller.uninstall(directory: plugins, isPreview: false))
        XCTAssertFalse(FileManager.default.fileExists(atPath: plugins.appendingPathComponent("matra.js").path))
    }
}

private final class UpgradesStubProtocol: URLProtocol {
    nonisolated(unsafe) static var handler: ((URLRequest) -> (Int, [String: String], Data))?

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let handler = Self.handler, let url = request.url else {
            client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse))
            return
        }
        let (status, headers, data) = handler(request)
        let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: headers)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}
