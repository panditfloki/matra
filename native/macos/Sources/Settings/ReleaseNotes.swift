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
            headline: "The menu bar, with the detail beside it.",
            changes: [
                .init(title: "Menu bar tabs", detail: "Overview plus a tab for each enabled provider, switched from the reading already on screen."),
                .init(title: "Detail beside the popover", detail: "Hover or pin a provider to open its detail in a panel next to the popover. The popover keeps its width."),
                .init(title: "Brand colours", detail: "Off by default; colours provider marks in the menu bar, notch and app; quota colours keep their meaning. Quota colour turns red at 90% used."),
                .init(title: "Local usage and spend", detail: "Claude and Codex: local token counts and USD API-price estimates, read from an installed ccusage. Not your subscription bill. Normal launch reads your enabled providers. Unknown readings stay neutral."),
                .init(title: "Updates during the beta", detail: "Updates are installed manually from GitHub during the beta. A signed feed for automatic installation is not published. Your preferences stay in place.")
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
