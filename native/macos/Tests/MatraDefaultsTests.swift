import XCTest
@testable import Matra

final class MatraDefaultsTests: XCTestCase {
    private func stores() -> (MatraDefaults, MatraDefaults) {
        let database = MatraMemoryDefaults()
        return (MatraDefaults(inMemorySuite: "first", database: database),
                MatraDefaults(inMemorySuite: "second", database: database))
    }

    func testRuntimeUsesMemoryOnlyForTestsOrExplicitQA() {
        XCTAssertTrue(Runtime.usesInMemoryDefaults(environment: [:], underTest: true))
        XCTAssertTrue(Runtime.usesInMemoryDefaults(environment: ["MATRA_IN_MEMORY_DEFAULTS": "1"], underTest: false))
        XCTAssertFalse(Runtime.usesInMemoryDefaults(environment: [:], underTest: false))
        XCTAssertFalse(Runtime.usesInMemoryDefaults(environment: ["MATRA_IN_MEMORY_DEFAULTS": "0"], underTest: false))
        XCTAssertTrue(MatraDefaults.standard.isInMemory)
        XCTAssertNil(MatraDefaults.standard.foundation)
    }

    func testNamedSuiteReopensAndStandardDomainWritesAliasIt() throws {
        let name = "MatraDefaultsTests.\(UUID().uuidString)"
        let first = try XCTUnwrap(MatraDefaults(suiteName: name))
        let reopened = try XCTUnwrap(MatraDefaults(suiteName: name))
        XCTAssertTrue(first.isInMemory)
        XCTAssertNil(first.foundation)
        defer { MatraDefaults.standard.removePersistentDomain(forName: name) }
        first.set("first", forKey: "value")
        XCTAssertEqual(reopened.string(forKey: "value"), "first")
        MatraDefaults.standard.setPersistentDomain(["value": "replacement"], forName: name)
        XCTAssertEqual(first.string(forKey: "value"), "replacement")
        MatraDefaults.standard.removePersistentDomain(forName: name)
        XCTAssertNil(first.object(forKey: "value"))
        XCTAssertNil(reopened.persistentDomain(forName: name))
    }

    func testNilAndExplicitStandardSuiteAliasStandard() throws {
        let key = "MatraDefaultsTests.\(UUID().uuidString)"
        defer { MatraDefaults.standard.removeObject(forKey: key) }
        MatraDefaults.standard.set(17, forKey: key)
        XCTAssertEqual(try XCTUnwrap(MatraDefaults(suiteName: nil)).integer(forKey: key), 17)
        XCTAssertEqual(try XCTUnwrap(MatraDefaults(suiteName: MatraDefaults.standardDomain)).integer(forKey: key), 17)
        XCTAssertEqual(MatraDefaults.standard.persistentDomain(forName: MatraDefaults.standardDomain)?[key] as? Int, 17)
    }

    func testPersistentMigrationReadsOnlyNamedDomainAndReplacesRatherThanMerges() {
        let (first, second) = stores()
        first.register(defaults: ["registered-only": true])
        first.setPersistentDomain(["choice": "old", "removed": true], forName: "old")
        first.setPersistentDomain(["choice": "migrated"], forName: "old")
        let old = second.persistentDomain(forName: "old")!
        XCTAssertNil(old["registered-only"])
        XCTAssertNil(old["removed"])
        for (key, value) in old { second.set(value, forKey: key) }
        XCTAssertEqual(second.string(forKey: "choice"), "migrated")
        first.removePersistentDomain(forName: "old")
        XCTAssertEqual(second.string(forKey: "choice"), "migrated")
    }

    func testRegistrationIsSharedMergedAndSurvivesDomainRemoval() {
        let (first, second) = stores()
        first.register(defaults: ["default": "one", "keep": 2])
        second.register(defaults: ["default": "two"])
        XCTAssertEqual(first.string(forKey: "default"), "two")
        XCTAssertEqual(second.integer(forKey: "keep"), 2)
        XCTAssertNil(first.persistentDomain(forName: "first"))
        first.set("saved", forKey: "default")
        XCTAssertEqual(first.string(forKey: "default"), "saved")
        first.removePersistentDomain(forName: "first")
        XCTAssertEqual(first.string(forKey: "default"), "two")
        XCTAssertEqual(first.volatileDomain(forName: MatraDefaults.registrationDomain)["keep"] as? Int, 2)
    }

    func testArgumentCurrentSuiteGlobalAndRegistrationPrecedence() {
        let (first, second) = stores()
        first.register(defaults: ["choice": "registration"])
        first.setPersistentDomain(["choice": "global"], forName: MatraDefaults.globalDomain)
        second.set("suite", forKey: "choice")
        first.addSuite(named: "second")
        XCTAssertEqual(first.string(forKey: "choice"), "suite")
        first.set("current", forKey: "choice")
        first.setVolatileDomain(["choice": "argument"], forName: MatraDefaults.argumentDomain)
        XCTAssertEqual(first.string(forKey: "choice"), "argument")
        XCTAssertEqual(first.dictionaryRepresentation()["choice"] as? String, "argument")
        first.removeVolatileDomain(forName: MatraDefaults.argumentDomain)
        XCTAssertEqual(first.string(forKey: "choice"), "current")
        first.removeObject(forKey: "choice")
        first.removeSuite(named: "second")
        XCTAssertEqual(first.string(forKey: "choice"), "global")
        first.removePersistentDomain(forName: MatraDefaults.globalDomain)
        XCTAssertEqual(first.string(forKey: "choice"), "registration")
        first.setVolatileDomain(["ignored": true], forName: "custom")
        XCTAssertNil(first.object(forKey: "ignored"))
        XCTAssertTrue(first.volatileDomainNames.contains("custom"))
    }

    func testPropertyListTypesAndConversions() {
        let (defaults, _) = stores()
        let date = Date(timeIntervalSince1970: 12345)
        let data = Data([0, 128, 255])
        defaults.set(true, forKey: "bool")
        defaults.set(42, forKey: "int")
        defaults.set(3.5, forKey: "double")
        defaults.set(Float(1.25), forKey: "float")
        defaults.set(date, forKey: "date")
        defaults.set(data, forKey: "data")
        defaults.set(["a", "b"], forKey: "array")
        defaults.set(["a": "b"], forKey: "dict")
        XCTAssertEqual(defaults.object(forKey: "bool") as? Bool, true)
        XCTAssertEqual(defaults.object(forKey: "int") as? Int, 42)
        XCTAssertEqual(defaults.string(forKey: "int"), "42")
        XCTAssertEqual(defaults.double(forKey: "double"), 3.5)
        XCTAssertEqual(defaults.float(forKey: "float"), 1.25)
        XCTAssertEqual(defaults.object(forKey: "date") as? Date, date)
        XCTAssertEqual(defaults.data(forKey: "data"), data)
        XCTAssertEqual(defaults.stringArray(forKey: "array"), ["a", "b"])
        XCTAssertEqual(defaults.dictionary(forKey: "dict") as? [String: String], ["a": "b"])
        defaults.set("YES", forKey: "stringBool")
        defaults.set("-7.5", forKey: "stringNumber")
        XCTAssertTrue(defaults.bool(forKey: "stringBool"))
        XCTAssertEqual(defaults.double(forKey: "stringNumber"), -7.5)
        XCTAssertNil(defaults.data(forKey: "int"))
        XCTAssertEqual(defaults.integer(forKey: "missing"), 0)
        XCTAssertFalse(defaults.bool(forKey: "missing"))
        defaults.set(nil, forKey: "data")
        XCTAssertNil(defaults.object(forKey: "data"))
    }

    func testCollectionsAreCopiedAndURLsRoundTripWithoutFilesystemAccess() {
        let (defaults, _) = stores()
        let value = NSMutableArray(array: ["original"])
        defaults.set(value, forKey: "array")
        value.add("changed")
        XCTAssertEqual(defaults.stringArray(forKey: "array"), ["original"])
        let url = URL(string: "https://example.invalid/preference")!
        defaults.set(url, forKey: "url")
        XCTAssertEqual(defaults.url(forKey: "url"), url)
        XCTAssertTrue(defaults.synchronize())
        XCTAssertFalse(defaults.objectIsForced(forKey: "array"))
    }

    func testChangeNotificationCanReadWithoutDeadlock() {
        let (defaults, _) = stores()
        let observed = expectation(description: "changed value")
        let token = NotificationCenter.default.addObserver(
            forName: MatraDefaults.didChangeNotification, object: nil, queue: nil
        ) { _ in
            if defaults.integer(forKey: "value") == 42 { observed.fulfill() }
        }
        defer { NotificationCenter.default.removeObserver(token) }
        defaults.set(42, forKey: "value")
        wait(for: [observed], timeout: 1)
    }
}

@MainActor
final class MatraAppStorageTests: XCTestCase {
    func testBindingSharesPreferencesUpdatesAndResetRestoresDeclaredDefault() {
        let database = MatraMemoryDefaults()
        let defaults = MatraDefaults(inMemorySuite: "bindings", database: database)
        let reopened = MatraDefaults(inMemorySuite: "bindings", database: database)
        let first = MatraAppStorage(wrappedValue: false, "choice", store: defaults)
        let second = MatraAppStorage(wrappedValue: false, "choice", store: reopened)
        XCTAssertFalse(first.wrappedValue)
        first.projectedValue.wrappedValue = true
        XCTAssertTrue(reopened.bool(forKey: "choice"))
        XCTAssertTrue(second.wrappedValue)
        reopened.removePersistentDomain(forName: "bindings")
        XCTAssertFalse(first.wrappedValue)
        XCTAssertFalse(second.wrappedValue)
    }

    func testMissingTrueDefaultDoesNotWriteUntilUserChangesIt() {
        let defaults = MatraDefaults(inMemorySuite: "binding", database: MatraMemoryDefaults())
        let binding = MatraAppStorage(wrappedValue: true, "accountsExpanded", store: defaults)
        XCTAssertTrue(binding.wrappedValue)
        XCTAssertNil(defaults.persistentDomain(forName: "binding"))
        binding.wrappedValue = false
        XCTAssertEqual(defaults.object(forKey: "accountsExpanded") as? Bool, false)
        defaults.removeObject(forKey: "accountsExpanded")
        XCTAssertTrue(binding.wrappedValue)
    }
}
