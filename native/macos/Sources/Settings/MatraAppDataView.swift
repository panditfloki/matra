import AppKit
import SwiftUI

/// Mirrors the Windows settings hierarchy, with truthful macOS behaviour.
struct MatraAppDataView: View {
    @ObservedObject var preferences: Preferences
    @ObservedObject var updater: Updater
    @State private var folderError: String?

    static let tagline = "AI usage, in proportion."
    static let formula = "Used / limit × 100 = usage %"
    static let startupHelp = "Launch this Mātrā when you sign in to your Mac. Your saved notch visibility applies. Disable this to launch it yourself."
    static let automaticInstallHelp = "Install verified Mātrā updates and restart the app automatically. When off, you choose when each update is installed."
    static let updateUnavailable = "Automatic updates are not configured for this local Mac build."
    static let updateHelp = "Mātrā checks its Mac release feed at startup and daily. Updates are verified with Mātrā's signing key before installation. Your preferences stay in place."
    static let folderHelp = "Open Mātrā’s local support files and custom provider icons in Finder. App preferences are managed separately by macOS; provider logins stay in their original tools or Keychain."

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                HStack(spacing: 18) {
                    Image("MatraLogo")
                        .resizable()
                        .scaledToFit()
                        .frame(width: 60, height: 60)
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 8) {
                        Text(MatraBrand.name)
                            .font(.system(size: 28, weight: .semibold))
                        Text(Self.tagline).font(.system(size: 15))
                        Text(Self.formula)
                            .font(.system(size: 12, design: .monospaced))
                            .foregroundStyle(Palette.textSecondary)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 10)

                card {
                    row {
                        Text("Start Mātrā at login")
                        Spacer()
                        Toggle("Start Mātrā at login", isOn: $preferences.launchAtLogin)
                            .labelsHidden()
                            .toggleStyle(.switch)
                            .tint(preferences.accentColor.color)
                            .disabled(Runtime.isDesignPreview)
                    }
                    rule
                    help(Self.startupHelp)
                    if let problem = preferences.launchAtLoginProblem {
                        help(problem)
                    }
                    rule
                    row {
                        Text("Install updates automatically")
                        Spacer()
                        Toggle("Install updates automatically", isOn: $updater.automaticallyInstallsUpdates)
                            .labelsHidden()
                            .toggleStyle(.switch)
                            .tint(preferences.accentColor.color)
                            .disabled(Runtime.isDesignPreview)
                    }
                    help(Self.automaticInstallHelp)
                    rule
                    row {
                        Text("Version")
                        Spacer()
                        Text(updater.currentVersion)
                            .foregroundStyle(Palette.textSecondary)
                            .textSelection(.enabled)
                    }
                    rule
                    VStack(alignment: .leading, spacing: 12) {
                        HStack {
                            Text("Updates")
                            Spacer()
                            Button("Check for updates") { updater.checkNow() }
                                .disabled(!Updater.isEnabled)
                        }
                        Text(Updater.isEnabled
                             ? (updater.outcome.message ?? "Check for a new Mātrā release.")
                             : Self.updateUnavailable)
                            .font(.system(size: 12))
                            .foregroundStyle(Palette.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                        Link("View releases on GitHub", destination: MatraBrand.repository.appendingPathComponent("releases"))
                            .font(.system(size: 12, weight: .medium))
                    }
                    .padding(16)
                    rule
                    help(Self.updateHelp)
                }

                card {
                    row {
                        Text("Data folder")
                        Spacer()
                        Button("Open folder", action: openDataFolder)
                    }
                    rule
                    help(Self.folderHelp)
                    if let folderError {
                        help(folderError)
                            .accessibilityLabel("Data folder error: \(folderError)")
                    }
                }

                card {
                    VStack(alignment: .leading, spacing: 8) {
                        Link("dydxfx.com", destination: MatraBrand.website)
                            .fontWeight(.semibold)
                        HStack(spacing: 4) {
                            Text("Developed by").foregroundStyle(Palette.textSecondary)
                            Link("Pandit Floki · X", destination: MatraBrand.author)
                        }
                        Link("GitHub", destination: MatraBrand.repository)
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(preferences.accentColor.color)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(16)
                }
            }
            .font(.system(size: 13))
            .foregroundStyle(Palette.textPrimary)
            .tint(preferences.accentColor.color)
            .padding(24)
        }
        .scrollBounceBehavior(.basedOnSize)
    }

    private var rule: some View { MatraBrand.border.frame(height: 1) }

    private func card<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 0, content: content)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(MatraBrand.card.opacity(0.72))
            .clipShape(RoundedRectangle(cornerRadius: 14))
            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(MatraBrand.border))
    }

    private func row<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        HStack(spacing: 12, content: content).padding(16)
    }

    private func help(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(Palette.textSecondary)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(16)
    }

    private func openDataFolder() {
        do {
            let support = try FileManager.default.url(for: .applicationSupportDirectory,
                in: .userDomainMask, appropriateFor: nil, create: true)
            let folder = try MatraStorage.prepareSupportRoot(in: support)
            folderError = NSWorkspace.shared.open(folder) ? nil : "Finder could not open the Mātrā data folder. Try again."
        } catch {
            folderError = "Could not open the Mātrā data folder: \(error.localizedDescription)"
        }
    }
}
