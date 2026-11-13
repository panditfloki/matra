import Foundation
import Darwin

// This executable is intentionally not an app/XCTest host. Its runner denies
// writes outside a disposable directory and access to both cfprefsd services.
guard Runtime.usesInMemoryDefaults else { fatalError("Run with MATRA_IN_MEMORY_DEFAULTS=1") }
var assertions = 0
func check(_ value: @autoclosure () -> Bool, _ message: String) {
    precondition(value(), message)
    assertions += 1
}

let environment = ProcessInfo.processInfo.environment
guard let deniedPath = environment["MATRA_ISOLATION_DENIED_PATH"] else { fatalError("Missing sandbox canary") }
check(!FileManager.default.fileExists(atPath: deniedPath), "Canary must not already exist")
do {
    try Data("must be denied".utf8).write(to: URL(fileURLWithPath: deniedPath))
    fatalError("Sandbox did not deny a write outside its disposable directory")
} catch { assertions += 1 }
for name in ["com.apple.cfprefsd.agent", "com.apple.cfprefsd.daemon"] {
    var port: mach_port_t = 0
    let result = name.withCString { bootstrap_look_up(bootstrap_port, $0, &port) }
    check(result != KERN_SUCCESS, "Sandbox must deny preferences daemon access")
}

check(!Runtime.usesInMemoryDefaults(environment: [:], underTest: false), "Normal runtime keeps Foundation")
check(Runtime.usesInMemoryDefaults(environment: [:], underTest: true), "XCTest is inert")
check(Runtime.usesInMemoryDefaults(environment: ["MATRA_IN_MEMORY_DEFAULTS": "1"], underTest: false), "QA opt-in")
check(!Runtime.usesInMemoryDefaults(environment: ["MATRA_IN_MEMORY_DEFAULTS": "0"], underTest: false), "Exact opt-in")
check(MatraDefaults.standard.isInMemory && MatraDefaults.standard.foundation == nil, "Standard is memory")
let name = "MatraDefaultsIsolation.\(UUID().uuidString)"
let first = MatraDefaults(suiteName: name)!
let again = MatraDefaults(suiteName: name)!
check(first.isInMemory && first.foundation == nil, "Named suite is memory")
first.set(42, forKey: "integer")
check(again.integer(forKey: "integer") == 42, "Named suite sharing")
MatraDefaults.standard.setPersistentDomain(["migrated": "yes"], forName: name)
check(again.string(forKey: "migrated") == "yes" && again.object(forKey: "integer") == nil, "Domain writes replace and alias")
MatraDefaults.standard.set(true, forKey: "standard-alias")
check(MatraDefaults(suiteName: nil)!.bool(forKey: "standard-alias"), "Nil suite aliases standard")
check(MatraDefaults(suiteName: MatraDefaults.standardDomain)!.bool(forKey: "standard-alias"), "Explicit standard domain aliases")
MatraDefaults.standard.removePersistentDomain(forName: name)
check(again.persistentDomain(forName: name) == nil && first.object(forKey: "migrated") == nil, "Domain reset reaches open instances")
check(MatraDefaults.standard.bool(forKey: "standard-alias"), "Reset does not clear another domain")

let database = MatraMemoryDefaults(arguments: ["fixture", "-argument", "YES"])
let defaults = MatraDefaults(inMemorySuite: "fixture", database: database)
let other = MatraDefaults(inMemorySuite: "other", database: database)
check(defaults.bool(forKey: "argument"), "Argument domain parsed")
defaults.register(defaults: ["choice": "registered", "kept": 8])
defaults.register(defaults: ["choice": "replacement"])
check(defaults.string(forKey: "choice") == "replacement" && defaults.integer(forKey: "kept") == 8, "Registration merges")
check(other.string(forKey: "choice") == "replacement", "Process registration shared")
check(defaults.persistentDomain(forName: "fixture") == nil, "Registration is not persistent")
defaults.setPersistentDomain(["choice": "global"], forName: MatraDefaults.globalDomain)
check(defaults.string(forKey: "choice") == "global", "Global precedes registration")
other.set("suite", forKey: "choice")
defaults.addSuite(named: "other")
check(defaults.string(forKey: "choice") == "suite", "Added suite precedes global")
defaults.set("local", forKey: "choice")
check(defaults.string(forKey: "choice") == "local", "Current domain precedes added suites")
defaults.setVolatileDomain(["choice": "argument"], forName: MatraDefaults.argumentDomain)
check(defaults.string(forKey: "choice") == "argument", "Arguments precede persistent settings")
check(defaults.dictionaryRepresentation()["choice"] as? String == "argument", "Dictionary uses the same precedence")
defaults.removeVolatileDomain(forName: MatraDefaults.argumentDomain)
defaults.removeObject(forKey: "choice")
defaults.removeSuite(named: "other")
check(defaults.string(forKey: "choice") == "global", "Removing a suite restores fallback")
defaults.removePersistentDomain(forName: MatraDefaults.globalDomain)
check(defaults.string(forKey: "choice") == "replacement", "Registration survives persistent reset")
defaults.setVolatileDomain(["unsearched": true], forName: "custom")
check(defaults.object(forKey: "unsearched") == nil, "Custom volatile domain is not implicitly searched")
check(defaults.volatileDomainNames.contains("custom"), "Volatile names")
defaults.removeVolatileDomain(forName: "custom")
check(defaults.volatileDomain(forName: "custom").isEmpty, "Volatile reset")

let date = Date(timeIntervalSince1970: 123456)
let bytes = Data([0, 1, 255])
defaults.set(true, forKey: "bool")
defaults.set(-42, forKey: "integer")
defaults.set(2.5, forKey: "double")
defaults.set(Float(1.25), forKey: "float")
defaults.set(date, forKey: "date")
defaults.set(bytes, forKey: "data")
defaults.set(["a", "b"], forKey: "array")
defaults.set(["a": "b"], forKey: "dictionary")
check(defaults.object(forKey: "bool") as? Bool == true, "Bool object type")
check(defaults.integer(forKey: "integer") == -42 && defaults.string(forKey: "integer") == "-42", "Integer and numeric string")
check(defaults.double(forKey: "double") == 2.5 && defaults.float(forKey: "float") == 1.25, "Floating point types")
check(defaults.object(forKey: "date") as? Date == date && defaults.data(forKey: "data") == bytes, "Date and data")
check(defaults.stringArray(forKey: "array") == ["a", "b"], "String array")
check(defaults.dictionary(forKey: "dictionary") as? [String: String] == ["a": "b"], "Dictionary")
defaults.set("YES", forKey: "stringBool")
defaults.set("3.5", forKey: "stringNumber")
check(defaults.bool(forKey: "stringBool") && defaults.double(forKey: "stringNumber") == 3.5, "Typed string conversion")
check(defaults.integer(forKey: "missing") == 0 && !defaults.bool(forKey: "missing"), "Missing scalar defaults")
check(defaults.data(forKey: "integer") == nil && defaults.stringArray(forKey: "integer") == nil, "Mismatched types")
let mutable = NSMutableArray(array: ["original"])
defaults.set(mutable, forKey: "mutable")
mutable.add("changed")
check(defaults.stringArray(forKey: "mutable") == ["original"], "Input collections copied")
let fileURL = URL(fileURLWithPath: deniedPath)
let remoteURL = URL(string: "https://example.invalid/fixture?q=1")!
defaults.set(fileURL, forKey: "fileURL")
defaults.set(remoteURL, forKey: "remoteURL")
check(defaults.url(forKey: "fileURL") == fileURL && defaults.url(forKey: "remoteURL") == remoteURL, "URL round trips without I/O")
defaults.set(nil, forKey: "data")
check(defaults.object(forKey: "data") == nil, "Nil removes a value")
check(defaults.synchronize() && !defaults.objectIsForced(forKey: "anything"), "No daemon sync or managed preferences")

var notifications = 0
let token = NotificationCenter.default.addObserver(forName: MatraDefaults.didChangeNotification, object: nil, queue: nil) { _ in notifications += 1 }
defaults.set(true, forKey: "notified")
defaults.removeObject(forKey: "notified")
NotificationCenter.default.removeObserver(token)
check(notifications == 2, "Changes are observable")
DispatchQueue.concurrentPerform(iterations: 100) { index in
    defaults.set(index, forKey: "concurrent-\(index)")
    precondition(defaults.integer(forKey: "concurrent-\(index)") == index)
}
check((0..<100).allSatisfy { defaults.integer(forKey: "concurrent-\($0)") == $0 }, "Concurrent access keeps all writes")
print("Defaults isolation: \(assertions) assertions passed; Foundation-only process; cfprefsd access and out-of-directory writes denied")
