import AppKit

/// Foreground app windows temporarily need a Dock presence even when the
/// ambient app prefers only its menu item. Closing one must not hide another.
@MainActor
enum AppWindowPresentation {
    private static let windows = NSHashTable<NSWindow>.weakObjects()

    static func register(_ window: NSWindow) { windows.add(window) }

    static func policy(for presence: AppPresence, excluding closing: NSWindow? = nil)
        -> NSApplication.ActivationPolicy {
        windows.allObjects.contains { $0 !== closing && ($0.isVisible || $0.isMiniaturized) }
            ? .regular : presence.activationPolicy
    }

    static func restore(_ presence: AppPresence, excluding closing: NSWindow? = nil) {
        NSApp.setActivationPolicy(policy(for: presence, excluding: closing))
    }
}
