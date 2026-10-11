import Foundation

public struct AgentStatuses: Equatable, Sendable {
    public private(set) var statuses: [String: String] = [:]
    public private(set) var unread: Set<String> = []
    public private(set) var since: [String: Double] = [:]

    public init() {}

    public mutating func set(_ pane: String, _ status: String?, looking: Bool, at now: Double = Date().timeIntervalSince1970) {
        let before = statuses[pane]
        statuses[pane] = status
        if status != before { since[pane] = status == nil ? nil : now }
        if status != "idle" {
            unread.remove(pane)
        } else if before == "working" && !looking {
            unread.insert(pane)
        }
    }

    public mutating func looked(at panes: Set<String>) {
        unread.subtract(panes)
    }

    public mutating func keep(_ panes: Set<String>) {
        statuses = statuses.filter { panes.contains($0.key) }
        unread.formIntersection(panes)
        since = since.filter { panes.contains($0.key) }
    }

    public func waitingLongest(among panes: [String]) -> String? {
        panes.filter { statuses[$0] == "needs-you" }.min { since[$0] ?? 0 < since[$1] ?? 0 }
    }
}
