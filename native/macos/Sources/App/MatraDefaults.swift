import Foundation
import CoreFoundation

/// The only entrance to app-owned preferences. Production uses Foundation
/// unchanged. XCTest and explicitly opted-in QA never create a UserDefaults
/// object: named suites alone are not isolated from the user's cfprefsd daemon.
///
/// This is a façade, not a UserDefaults subclass (which Apple discourages).
final class MatraDefaults: @unchecked Sendable, Equatable {
    static let argumentDomain = UserDefaults.argumentDomain
    static let globalDomain = UserDefaults.globalDomain
    static let registrationDomain = UserDefaults.registrationDomain
    static let didChangeNotification = Notification.Name("MatraInMemoryDefaultsDidChange")
    static let standardDomain = Bundle.main.bundleIdentifier ?? MatraStorage.namespace

    static let standard: MatraDefaults = {
        if Runtime.usesInMemoryDefaults { return MatraDefaults(inMemorySuite: nil) }
        return MatraDefaults(foundation: .standard)
    }()

    /// Only the SwiftUI storage adapter may use this for Foundation KVO.
    /// Always nil in tests/QA, including when their named suites are reopened.
    let foundation: UserDefaults?
    private let memory: MatraMemoryDefaults?
    private let domain: String
    var isInMemory: Bool { memory != nil }

    static func == (lhs: MatraDefaults, rhs: MatraDefaults) -> Bool { lhs === rhs }

    private init(foundation: UserDefaults) {
        self.foundation = foundation
        memory = nil
        domain = Self.standardDomain
    }

    init?(suiteName: String?) {
        domain = suiteName ?? Self.standardDomain
        if Runtime.usesInMemoryDefaults {
            foundation = nil
            memory = .shared
        } else {
            guard let defaults = UserDefaults(suiteName: suiteName) else { return nil }
            foundation = defaults
            memory = nil
        }
    }

    /// Explicit memory construction also makes standalone Foundation fixtures
    /// safe before XCTest has been loaded. Separate databases isolate tests of
    /// global/registration precedence without clearing another fixture's state.
    init(inMemorySuite: String?, database: MatraMemoryDefaults = .shared) {
        foundation = nil
        memory = database
        domain = inMemorySuite ?? Self.standardDomain
    }

    func object(forKey key: String) -> Any? {
        if let foundation { return foundation.object(forKey: key) }
        return memory!.read { $0.merged(domain: domain)[key] }
    }

    func set(_ value: Any?, forKey key: String) {
        if let foundation { foundation.set(value, forKey: key); return }
        if let url = value as? URL { set(url, forKey: key); return }
        let copy = value.map(MatraMemoryDefaults.copyValue)
        memory!.change { $0.persistent[domain, default: [:]][key] = copy }
    }

    func removeObject(forKey key: String) {
        if let foundation { foundation.removeObject(forKey: key); return }
        memory!.change { $0.persistent[domain]?[key] = nil }
    }

    func string(forKey key: String) -> String? {
        if let foundation { return foundation.string(forKey: key) }
        let value = object(forKey: key)
        return (value as? String) ?? (value as? NSNumber)?.stringValue
    }

    func bool(forKey key: String) -> Bool {
        if let foundation { return foundation.bool(forKey: key) }
        let value = object(forKey: key)
        return (value as? NSNumber)?.boolValue ?? (value as? NSString)?.boolValue ?? false
    }

    func integer(forKey key: String) -> Int {
        if let foundation { return foundation.integer(forKey: key) }
        let value = object(forKey: key)
        return (value as? NSNumber)?.intValue ?? (value as? NSString)?.integerValue ?? 0
    }

    func double(forKey key: String) -> Double {
        if let foundation { return foundation.double(forKey: key) }
        let value = object(forKey: key)
        if let number = value as? NSNumber {
            return CFGetTypeID(number) == CFBooleanGetTypeID() ? 0 : number.doubleValue
        }
        return (value as? NSString)?.doubleValue ?? 0
    }

    func float(forKey key: String) -> Float {
        if let foundation { return foundation.float(forKey: key) }
        let value = object(forKey: key)
        if let number = value as? NSNumber {
            return CFGetTypeID(number) == CFBooleanGetTypeID() ? 0 : number.floatValue
        }
        return (value as? NSString)?.floatValue ?? 0
    }

    func array(forKey key: String) -> [Any]? {
        if let foundation { return foundation.array(forKey: key) }
        return object(forKey: key) as? [Any]
    }

    func stringArray(forKey key: String) -> [String]? {
        if let foundation { return foundation.stringArray(forKey: key) }
        return object(forKey: key) as? [String]
    }

    func dictionary(forKey key: String) -> [String: Any]? {
        if let foundation { return foundation.dictionary(forKey: key) }
        return object(forKey: key) as? [String: Any]
    }

    func data(forKey key: String) -> Data? {
        if let foundation { return foundation.data(forKey: key) }
        return object(forKey: key) as? Data
    }

    func url(forKey key: String) -> URL? {
        if let foundation { return foundation.url(forKey: key) }
        if let path = object(forKey: key) as? String {
            return URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        }
        guard let data = object(forKey: key) as? Data else { return nil }
        return (try? NSKeyedUnarchiver.unarchivedObject(ofClass: NSURL.self, from: data)) as URL?
    }

    func set(_ value: Int, forKey key: String) {
        if let foundation { foundation.set(value, forKey: key); return }
        set(NSNumber(value: value) as Any, forKey: key)
    }

    func set(_ value: Double, forKey key: String) {
        if let foundation { foundation.set(value, forKey: key); return }
        set(NSNumber(value: value) as Any, forKey: key)
    }

    func set(_ value: Float, forKey key: String) {
        if let foundation { foundation.set(value, forKey: key); return }
        set(NSNumber(value: value) as Any, forKey: key)
    }

    func set(_ value: Bool, forKey key: String) {
        if let foundation { foundation.set(value, forKey: key); return }
        set(NSNumber(value: value) as Any, forKey: key)
    }

    func set(_ url: URL?, forKey key: String) {
        if let foundation { foundation.set(url, forKey: key); return }
        guard let url else { removeObject(forKey: key); return }
        if url.isFileURL {
            set((url.path as NSString).abbreviatingWithTildeInPath as Any, forKey: key)
        } else {
            // Foundation stores non-file URLs as an archive, not as a path.
            let data = try! NSKeyedArchiver.archivedData(withRootObject: url as NSURL,
                                                       requiringSecureCoding: true)
            set(data as Any, forKey: key)
        }
    }

    func register(defaults values: [String: Any]) {
        if let foundation { foundation.register(defaults: values); return }
        let values = MatraMemoryDefaults.copyDictionary(values)
        memory!.change { $0.volatile[Self.registrationDomain, default: [:]].merge(values) { _, new in new } }
    }

    func persistentDomain(forName name: String) -> [String: Any]? {
        if let foundation { return foundation.persistentDomain(forName: name) }
        return memory!.read { $0.persistent[name] }
    }

    func setPersistentDomain(_ values: [String: Any], forName name: String) {
        if let foundation { foundation.setPersistentDomain(values, forName: name); return }
        let values = MatraMemoryDefaults.copyDictionary(values)
        memory!.change { $0.persistent[name] = values }
    }

    func removePersistentDomain(forName name: String) {
        if let foundation { foundation.removePersistentDomain(forName: name); return }
        memory!.change { $0.persistent[name] = nil }
    }

    var volatileDomainNames: [String] {
        if let foundation { return foundation.volatileDomainNames }
        return memory!.read { $0.volatile.keys.sorted() }
    }

    func volatileDomain(forName name: String) -> [String: Any] {
        if let foundation { return foundation.volatileDomain(forName: name) }
        return memory!.read { $0.volatile[name] ?? [:] }
    }

    func setVolatileDomain(_ values: [String: Any], forName name: String) {
        if let foundation { foundation.setVolatileDomain(values, forName: name); return }
        let values = MatraMemoryDefaults.copyDictionary(values)
        memory!.change { $0.volatile[name] = values }
    }

    func removeVolatileDomain(forName name: String) {
        if let foundation { foundation.removeVolatileDomain(forName: name); return }
        memory!.change { $0.volatile[name] = nil }
    }

    func addSuite(named name: String) {
        if let foundation { foundation.addSuite(named: name); return }
        memory!.change {
            $0.suites[domain, default: []].removeAll { $0 == name }
            $0.suites[domain, default: []].insert(name, at: 0)
        }
    }

    func removeSuite(named name: String) {
        if let foundation { foundation.removeSuite(named: name); return }
        memory!.change { $0.suites[domain]?.removeAll { $0 == name } }
    }

    func dictionaryRepresentation() -> [String: Any] {
        if let foundation { return foundation.dictionaryRepresentation() }
        return memory!.read { $0.merged(domain: domain) }
    }

    @discardableResult func synchronize() -> Bool { foundation?.synchronize() ?? true }

    func objectIsForced(forKey key: String) -> Bool {
        foundation?.objectIsForced(forKey: key) ?? false
    }

    func objectIsForced(forKey key: String, inDomain name: String) -> Bool {
        foundation?.objectIsForced(forKey: key, inDomain: name) ?? false
    }
}

/// In-process analogue of the public defaults domains. No file, preferences
/// daemon, account, network or managed/global user preferences are consulted.
/// Suite names alias the same persistent dictionary, including domain-based
/// migration writes made through the standard object.
final class MatraMemoryDefaults: @unchecked Sendable {
    static let shared = MatraMemoryDefaults(arguments: ProcessInfo.processInfo.arguments)
    private let lock = NSLock()
    fileprivate var persistent: [String: [String: Any]] = [:]
    fileprivate var volatile: [String: [String: Any]]
    fileprivate var suites: [String: [String]] = [:]

    init(arguments: [String] = []) {
        var values: [String: Any] = [:]
        var index = 1
        while index + 1 < arguments.count {
            let key = arguments[index]
            if key.hasPrefix("-"), !key.hasPrefix("--"), key.count > 1 {
                let value = arguments[index + 1]
                values[String(key.dropFirst())] = (try? PropertyListSerialization.propertyList(
                    from: Data(value.utf8), options: [], format: nil)) ?? value
                index += 2
            } else { index += 1 }
        }
        volatile = [MatraDefaults.argumentDomain: values, MatraDefaults.registrationDomain: [:]]
    }

    fileprivate func read<T>(_ body: (MatraMemoryDefaults) -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return body(self)
    }

    fileprivate func change(_ body: (MatraMemoryDefaults) -> Void) {
        lock.lock()
        body(self)
        lock.unlock()
        // Notify after unlocking: observers may immediately read the new value.
        NotificationCenter.default.post(name: MatraDefaults.didChangeNotification, object: self)
    }

    fileprivate func merged(domain: String) -> [String: Any] {
        let search = [volatile[MatraDefaults.argumentDomain] ?? [:], persistent[domain] ?? [:]]
            + (suites[domain] ?? []).map { persistent[$0] ?? [:] }
            + [persistent[MatraDefaults.globalDomain] ?? [:], volatile[MatraDefaults.registrationDomain] ?? [:]]
        return search.reversed().reduce(into: [:]) { $0.merge($1) { _, earlier in earlier } }
    }

    /// Property-list round trips detach mutable collections/data supplied by a
    /// caller, just as Foundation's immutable values do. Never serialize to disk.
    fileprivate static func copyValue(_ value: Any) -> Any {
        copyDictionary(["value": value])["value"]!
    }

    fileprivate static func copyDictionary(_ values: [String: Any]) -> [String: Any] {
        guard PropertyListSerialization.propertyList(values, isValidFor: .binary),
              let data = try? PropertyListSerialization.data(fromPropertyList: values, format: .binary, options: 0),
              let copy = try? PropertyListSerialization.propertyList(from: data, options: [], format: nil) as? [String: Any]
        else { preconditionFailure("Defaults require property-list values") }
        return copy
    }
}
