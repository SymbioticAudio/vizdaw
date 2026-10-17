import Foundation
import Observation

/// The default list filter and the optional columns: app preferences
/// (Plan 4c Ruling 19). Without `defaults` they live in memory only.
@Observable
@MainActor
public final class ListPreferences {
    public static let key = "VizDAW.list"
    @ObservationIgnored private let defaults: UserDefaults?

    private struct Stored: Codable {
        var defaultFilter: ListFilter
        var columns: ListColumns
    }

    public var defaultFilter: ListFilter { didSet { save() } }
    public var columns: ListColumns { didSet { save() } }

    public init(defaults: UserDefaults? = nil) {
        self.defaults = defaults
        let s = defaults?.data(forKey: Self.key).flatMap { try? JSONDecoder().decode(Stored.self, from: $0) }
        defaultFilter = s?.defaultFilter ?? ListFilter()
        columns = s?.columns ?? ListColumns()
    }

    private func save() {
        guard let defaults,
            let data = try? JSONEncoder().encode(Stored(defaultFilter: defaultFilter, columns: columns))
        else { return }
        defaults.set(data, forKey: Self.key)
    }
}
