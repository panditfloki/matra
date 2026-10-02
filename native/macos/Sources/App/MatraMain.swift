import SwiftUI

@main
struct MatraMain: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        // The notch is the UI; the panel is put up by the delegate. This scene
        // exists only because `App` needs one.
        Settings { EmptyView() }
            .commands {
                CommandGroup(replacing: .appSettings) {
                    Button("Settings…") { appDelegate.openSettings() }
                        .keyboardShortcut(",", modifiers: .command)
                }
                CommandGroup(after: .appSettings) {
                    Button("Open Dashboard") { appDelegate.openDashboard() }
                        .keyboardShortcut("1", modifiers: .command)
                    Button("Open Usage Panel") { appDelegate.openUsagePanel() }
                        .keyboardShortcut("2", modifiers: .command)
                }
            }
    }
}
