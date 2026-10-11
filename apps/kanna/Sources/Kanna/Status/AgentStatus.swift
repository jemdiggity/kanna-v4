import AppKit
import KannaStatus

extension AppController {
    func installAgentStatus() {
        daemon?.onStatus = { [weak self] pane, status in self?.statusChanged(pane, status) }
        daemon?.onFolder = { [weak self] pane, cwd in self?.folderChanged(pane, cwd) }
        daemon?.onBacklog = { [weak self] tasks in self?.backlogChanged(tasks) }
        NotificationCenter.default.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.statusSeen() }
        }
        guard debugCommands else { return }
        commands["debug.status"] = { [weak self] _ in
            guard let self else { return [:] }
            var panes: [String: JSON] = [:]
            for (pane, status) in agentStatuses.statuses {
                panes[pane] = ["status": .string(status), "unread": .bool(agentStatuses.unread.contains(pane))]
            }
            return ["panes": .object(panes), "looking": .array(lookingAt.sorted().map { .string($0) })]
        }
    }

    var lookingAt: Set<String> {
        guard NSApp.isActive || background, let window = keyWindow else { return [] }
        var shown = Set<String>()
        if let zoomed = window["zoomed"]?.string {
            shown.insert(zoomed)
        } else {
            shown.formUnion(WindowController.paneIDs(window["root"] ?? .null))
        }
        if let dock = window["dock"]?["pane"]?.string { shown.insert(dock) }
        return shown
    }

    func statusChanged(_ pane: String, _ status: String?) {
        agentStatuses.set(pane, status, looking: lookingAt.contains(pane))
        if isTask(pane) { refresh() }
    }

    func folderChanged(_ pane: String, _ cwd: String) {
        guard folders[pane] != cwd else { return }
        folders[pane] = cwd
        if isTask(pane) { refresh() }
    }

    func statusSeen() {
        agentStatuses.looked(at: lookingAt)
    }
}
