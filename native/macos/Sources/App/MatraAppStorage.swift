import SwiftUI

/// The app's four Boolean storage bindings use the same defaults boundary as
/// Preferences. A native @AppStorage without a store would bypass it and write
/// the host's real preferences during tests or sample UI review.
///
/// Production keeps Foundation persistence and its documented per-key KVO,
/// including changes from another process. Only the backing store changes in
/// tests/QA; the view's value, default and Binding stay the same.
@propertyWrapper
struct MatraAppStorage: DynamicProperty {
    @ObservedObject private var preference: MatraBoolPreference

    init(wrappedValue: Bool, _ key: String, store: MatraDefaults = .standard) {
        preference = MatraBoolPreference(key: key, fallback: wrappedValue, defaults: store)
    }

    var wrappedValue: Bool {
        get { preference.value }
        nonmutating set { preference.set(newValue) }
    }

    var projectedValue: Binding<Bool> {
        Binding(get: { preference.value }, set: { preference.set($0) })
    }
}

private final class MatraBoolPreference: NSObject, ObservableObject {
    @Published private(set) var value: Bool
    private let key: String
    private let fallback: Bool
    private let defaults: MatraDefaults
    private var notification: NSObjectProtocol?

    init(key: String, fallback: Bool, defaults: MatraDefaults) {
        self.key = key
        self.fallback = fallback
        self.defaults = defaults
        value = defaults.object(forKey: key) == nil ? fallback : defaults.bool(forKey: key)
        super.init()
        if let foundation = defaults.foundation {
            foundation.addObserver(self, forKeyPath: key, options: [], context: nil)
        } else {
            notification = NotificationCenter.default.addObserver(
                forName: MatraDefaults.didChangeNotification, object: nil, queue: nil
            ) { [weak self] _ in self?.reloadOnMain() }
        }
    }

    deinit {
        if let notification { NotificationCenter.default.removeObserver(notification) }
        defaults.foundation?.removeObserver(self, forKeyPath: key)
    }

    func set(_ value: Bool) { defaults.set(value, forKey: key) }

    override func observeValue(forKeyPath keyPath: String?, of object: Any?,
                               change: [NSKeyValueChangeKey: Any]?, context: UnsafeMutableRawPointer?) {
        guard keyPath == key, (object as AnyObject?) === defaults.foundation else {
            super.observeValue(forKeyPath: keyPath, of: object, change: change, context: context)
            return
        }
        reloadOnMain()
    }

    private func reloadOnMain() {
        if Thread.isMainThread { reload() }
        else { DispatchQueue.main.async { [weak self] in self?.reload() } }
    }

    private func reload() {
        let next = defaults.object(forKey: key) == nil ? fallback : defaults.bool(forKey: key)
        if next != value { value = next }
    }
}
