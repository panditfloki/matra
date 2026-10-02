import Combine
import Foundation

/// A read-only projection, not a second provider store or polling schedule.
@MainActor
final class DashboardModel: ObservableObject {
    @Published private(set) var snapshots: [ProviderSnapshot]
    @Published private(set) var refreshing: Set<String> = []
    @Published private(set) var selectedID: String?
    @Published private(set) var spendReports: [String: UsageSpendReport] = [:]
    @Published private(set) var analyticsLoading: Set<String> = []
    @Published private(set) var analyticsErrors: [String: String] = [:]
    /// Where readings come from. Both preview kinds skip UsageStore entirely.
    enum DataSource: Equatable {
        case live
        case samples
        /// The owner's real local logs through the offline reader. No
        /// accounts, credentials or quota polling.
        case localLogs
    }
    let dataSource: DataSource
    /// The live app path, run as the separate preview build.
    private(set) var isLivePreview = false
    var isPreview: Bool { dataSource != .live }
    var readsLocalLogs: Bool { dataSource == .localLogs }
    /// Quota or local-analytics refresh exists on this surface.
    var canRefresh: Bool { dataSource != .samples }
    var previewTitle: String? {
        switch dataSource {
        case .live: return isLivePreview ? "Live preview" : nil
        case .samples: return "Sample preview"
        case .localLogs: return "Local-log preview"
        }
    }
    private var subscriptions = Set<AnyCancellable>()
    private let requestRefresh: (String?) -> Void
    private let readReport: @Sendable (String) throws -> UsageSpendReport
    private var analyticsRequested = false
    private var analyticsRefreshPolicy = UsageSpendRefreshPolicy()

    init(store: UsageStore, isLivePreview: Bool = false) {
        snapshots = store.notchSnapshots
        dataSource = .live
        self.isLivePreview = isLivePreview
        readReport = { try UsageSpendReader.read(providerID: $0) }
        requestRefresh = { [weak store] id in
            if let id { store?.refresh(providerID: id) }
            else { store?.refreshNow() }
        }
        store.$notchSnapshots.sink { [weak self] in self?.replaceSnapshots($0) }
            .store(in: &subscriptions)
        store.$refreshing.sink { [weak self] in self?.refreshing = $0 }
            .store(in: &subscriptions)
    }

    /// Isolated visual QA: no UsageStore, credentials, network or timers.
    init(samples: [ProviderSnapshot]) {
        snapshots = samples
        dataSource = .samples
        requestRefresh = { _ in }
        readReport = { _ in throw SpendReaderError.unavailable }
        spendReports = UsageSpendSamples.reports()
    }

    /// Real local analytics, nothing else: reads only on open or refresh,
    /// never from a store publication, so nothing polls in the background.
    init(localLogs snapshots: [ProviderSnapshot],
         reader: @escaping @Sendable (String) throws -> UsageSpendReport = { try UsageSpendReader.read(providerID: $0) }) {
        self.snapshots = snapshots
        dataSource = .localLogs
        requestRefresh = { _ in }
        readReport = reader
    }

    var selectedSnapshot: ProviderSnapshot? {
        snapshots.first { $0.id == selectedID }
    }

    var isRefreshingSelection: Bool {
        if let snapshot = selectedSnapshot { return refreshing.contains(snapshot.providerID) }
        return !refreshing.isEmpty
    }

    func select(_ id: String?) {
        selectedID = snapshots.contains { $0.id == id } ? id : nil
    }

    func replaceSnapshots(_ values: [ProviderSnapshot]) {
        snapshots = values
        // Never display a disconnected account's cached details.
        if let selectedID, !values.contains(where: { $0.id == selectedID }) {
            self.selectedID = nil
        }
        if analyticsRequested { loadAnalytics() }
    }

    func refresh() { refresh(providerID: selectedSnapshot?.providerID) }

    func refresh(providerID: String?) {
        requestRefresh(providerID)
        loadAnalytics(force: true)
    }

    func report(for snapshot: ProviderSnapshot) -> UsageSpendReport? {
        spendReports[Self.analyticsID(snapshot)]
    }

    static func analyticsID(_ snapshot: ProviderSnapshot) -> String {
        snapshot.providerID.split(separator: ":").first.map(String.init) ?? snapshot.providerID
    }

    /// All app surfaces share these reports and in-flight requests. Native
    /// provider quotas remain exclusively owned by UsageStore.
    func loadAnalytics(force: Bool = false) {
        guard dataSource != .samples else { return }
        analyticsRequested = true
        let ids = Set(snapshots.map(Self.analyticsID)).intersection(["codex", "claude"])
        for id in ids {
            guard !analyticsLoading.contains(id),
                  analyticsRefreshPolicy.claim(providerID: id, force: force) else { continue }
            analyticsLoading.insert(id)
            Task { [weak self] in
                let read = readReport
                let result = await Task.detached(priority: .utility) {
                    Result { try read(id) }
                }.value
                guard let self else { return }
                analyticsLoading.remove(id)
                // A local report may cover several logins. Keep it device-
                // scoped and never borrow it as a disconnected account reading.
                switch result {
                case .success(let report): spendReports[id] = report; analyticsErrors[id] = nil
                case .failure(let error):
                    analyticsErrors[id] = error.localizedDescription
                    if var previous = spendReports[id] {
                        previous.staleReason = error.localizedDescription
                        spendReports[id] = previous
                    }
                }
            }
        }
    }
}
