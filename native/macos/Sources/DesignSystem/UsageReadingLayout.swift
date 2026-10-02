import SwiftUI

/// The notch keeps its measured tracks. Larger reading surfaces opt into
/// filling the available width without changing the quota or severity rules.
private struct ExpandsUsageRowsKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    var expandsUsageRows: Bool {
        get { self[ExpandsUsageRowsKey.self] }
        set { self[ExpandsUsageRowsKey.self] = newValue }
    }
}
