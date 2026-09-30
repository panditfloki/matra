import Foundation

struct ReleaseNote: Equatable {
    let version: String
    let headline: String
    let changes: [Change]

    struct Change: Equatable {
        let title: String
        let detail: String
        init(title: String, detail: String = "") {
            self.title = title
            self.detail = detail
        }
    }
}

/// Product release notes are bundled so they remain readable offline.
enum ReleaseNotes {
    static var all: [ReleaseNote] {
        [ReleaseNote(
            version: "1.8.5",
            headline: "Mātrā, shaped for your workspace.",
            changes: [
                .init(title: "Three signature surfaces", detail: "Choose Liquid Glass, Dark Glass or Solid Dark. Light and System remain available. Glass ring centres share the surrounding material."),
                .init(title: "Your accent, clear status colours", detail: "App controls follow your accent. Usage stays green below Watch, amber at Watch, orange at Critical and red from 90%."),
                .init(title: "Your sources, live", detail: "Normal launch reads your enabled providers. Unknown readings stay neutral, with independent current and weekly limits."),
                .init(title: "Settings in Mātrā's own words", detail: "AI sources, Display, Alerts and App & data explain each choice directly."),
                .init(title: "Updates with your choice", detail: "Enable automatic installation for verified Mac releases, or choose when to install each update. Your preferences stay in place."),
                .init(title: "One product identity", detail: "Mātrā branding, DYDXFX links and product licence information are consistent throughout the app.")
            ])]
    }

    static func note(for version: String) -> ReleaseNote? {
        all.first { $0.version == version }
    }

    static func unseen(in version: String, lastSeen: String?,
                       notes: [ReleaseNote] = ReleaseNotes.all) -> ReleaseNote? {
        guard lastSeen != version else { return nil }
        return notes.first { $0.version == version }
    }
}
