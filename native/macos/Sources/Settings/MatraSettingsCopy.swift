import Foundation

/// Settings-only English copy. Translation keys, provider payloads, URLs and
/// stored identifiers are deliberately not rewritten.
enum MatraSettingsCopy {
    static let replacements: [String: String] = [
        "Notifications": "Alerts",
        "Custom Endpoints": "Custom sources",
        "Choose which providers the notch reads.": "Choose the AI tools that appear on your meter.",
        "See your usage on your phone.": "Check your usage from a paired phone.",
        "Peak and off-peak pricing for your DeepSeek spend.": "Set the rates used to estimate DeepSeek costs.",
        "Models running in Ollama on this Mac.": "Track your local Ollama models.",
        "Models loaded in LM Studio on this Mac.": "Track your local LM Studio models.",
        "OpenAI-compatible APIs, local runtimes and custom proxies.": "Connect a compatible API, local server or proxy.",
        "How the notch looks and where it sits.": "Make the meter fit your workspace.",
        "What Matra tells you, and when.": "Choose which changes deserve your attention.",
        "Startup, updates and everything else.": "Manage startup, updates and local files.",
        "Connected": "On your meter",
        "Not connected": "Available sources",
        "Notch": "Edge meter",
        "Show": "Visibility",
        "Always show": "Always visible",
        "Show on hover": "On hover",
        "Hide": "Hidden",
        "Edge": "Screen edge",
        "Recentre": "Centre meter",
        "Usage Limits": "Usage colours & alerts",
        "Colour transition": "Colour changes",
        "Watch limit": "Watch threshold",
        "Critical limit": "Critical threshold",
        "App": "App preferences",
        "Notch reads": "Limit window",
        "Model data": "Model group",
        "Where to notify": "Alert destination",
        "When a session ends": "Session activity",
        "When a limit is reached": "Usage limit reached",
        "When a limit resets": "Allowance renewed",
        "Threshold alerts": "Usage warnings",
        "Send a test": "Test alert",
        "In the notch": "In the meter",
        "Open the notch for a moment": "Briefly reveal the meter",
        "Show notification from notch": "Show a reset alert",
        "One provider always stays ticked, so the pill is never empty.": "Keep at least one source enabled so your meter has a reading to show.",
        "These have no ring to place. Switch one on and it joins the end of the list above.": "Enable a source to add it to your meter. Drag enabled sources to change their order.",
        "The figure under each ring. Turn it off for rings alone; the number is still a hover away in the card.": "Show usage below each ring. When hidden, the same figure remains in the detail card.",
        "The notch folds away while a full-screen app is frontmost, and returns when you leave it. Off keeps it in place over full-screen apps.": "Hide the meter during full-screen work. It returns when you leave the full-screen app.",
        "Hold ⌥ and drag the notch to slide it along its edge. Each edge remembers where you left it.": "Option + drag moves the meter along the screen edge. Mātrā remembers each edge's position.",
        "Long enough to read the session's name and reach for it.": "Choose how long the session alert stays visible.",
        "Displays a notification card from the side of the notch when a provider's session or weekly usage limit is reached.": "Show a card beside the meter when a session or weekly allowance runs out.",
        "Displays a notification card from the side of the notch when a provider's usage limit resets.": "Show a card beside the meter when a provider renews your allowance.",
        "A system notification the moment a provider's headline limit crosses 80%, and again at 100% \u{2014} once per crossing, and again only after the window rolls over. Mute one from the bell beside its row in Accounts.": "Warn at 80% and 100% of a provider's main allowance, once per level until reset. Use the bell in AI sources to mute that provider.",
        "Connect an assistant to get started": "Add an AI source to start measuring"
    ]

    static func text(_ value: String, locale: Locale = L10n.locale) -> String {
        guard locale.language.languageCode?.identifier == "en" else { return value }
        return (replacements[value] ?? value).replacingOccurrences(of: " \u{2014} ", with: ", ")
    }
}

extension L10n {
    static func settings(_ key: String.LocalizationValue, locale: Locale = locale) -> String {
        MatraSettingsCopy.text(t(key, locale: locale), locale: locale)
    }

    static func settingsBranded(_ key: String.LocalizationValue, locale: Locale = locale) -> String {
        MatraBrand.copy(settings(key, locale: locale))
    }
}
