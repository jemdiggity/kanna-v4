import Foundation

/// Persistence (docs/kmux-spec.md §3.6): the layout as a versioned state,
/// ported from the reference model (reference/kmux/core.js: exportState,
/// importState). Panes keep their IDs. Restoring starts every pane again:
/// terminals as new terminals in their cwd with their command, web panes at
/// their URL, md panes on their file, iOS panes relaunching their app.
///
///     { kmux: "state", version: 1, key, counters: { pane, tab, window },
///       windows: [{ id, frame?, active, focused, zoomed, tabCount,
///                   tabs: [{ id, title, lastFocus, layout }] }],
///       panes: [{ id, name, type, …what it shows }] }
///
/// A layout is `{ pane: ID }` or `{ split: row|column, children: [{ …layout, size }] }`.
extension Core {
    public static let stateVersion = 1

    public func exportState() -> JSON {
        let model = self.model
        let counters = model.savedCounters
        let windows: [JSON] = model.windows.map { window in
            var out: [String: JSON] = [
                "id": .string(window.id), "active": Self.optional(window.active), "focused": Self.optional(window.focused),
                "zoomed": Self.optional(window.zoomed), "tabCount": .number(Double(window.tabCount)),
                "tabs": .array(window.tabs.map { tab in
                    ["id": .string(tab.id), "title": .string(tab.title), "lastFocus": Self.optional(tab.lastFocus), "layout": tab.root.map(Self.exportNode) ?? .null]
                }),
            ]
            if let frame = window.frame { out["frame"] = ["x": .number(frame.x), "y": .number(frame.y), "w": .number(frame.w), "h": .number(frame.h)] }
            return .object(out)
        }
        let panes: [JSON] = model.paneOrder.compactMap { model.panes[$0] }.map { pane in
            var out: [String: JSON] = ["id": .string(pane.id), "name": Self.optional(pane.name), "type": .string(pane.type.rawValue)]
            switch pane.type {
            case .term:
                out["cmd"] = Self.optional(pane.command)
                out["cwd"] = Self.optional(pane.cwd)
                out["session"] = Self.optional(pane.session)
            case .web:
                out["url"] = Self.optional(pane.url)
                out["history"] = .bool(pane.history != nil)
            case .md:
                out["path"] = Self.optional(pane.path)
                out["zoom"] = .number(pane.zoom ?? 1)
            case .ios:
                out["app"] = Self.optional(pane.app)
                out["device"] = Self.optional(pane.device)
            }
            return .object(out)
        }
        return [
            "kmux": "state", "version": .number(Double(Self.stateVersion)), "key": Self.optional(model.key),
            "counters": ["pane": .number(Double(counters.pane)), "tab": .number(Double(counters.tab)), "window": .number(Double(counters.window))],
            "windows": .array(windows), "panes": .array(panes),
        ]
    }

    private static func exportNode(_ node: Node) -> JSON {
        switch node {
        case .pane(let id): return ["pane": .string(id)]
        case .split(let split):
            return ["split": .string(split.axis.rawValue), "children": .array(split.kids.map { kid in
                guard case .object(var fields) = exportNode(kid.node) else { return .null }
                fields["size"] = .number(kid.size)
                return .object(fields)
            })]
        }
    }

    private static func optional(_ value: String?) -> JSON { value.map(JSON.string) ?? .null }

    /// Replaces everything with a saved state, as kmux does when it launches:
    /// the old panes stop, the restored ones start (without waiting for them).
    /// Throws `bad_request`, changing nothing, if the state can't be read.
    public func restoreState(_ state: JSON) throws {
        let (panes, windows, key, counters) = try Self.importState(state)
        for pane in model.paneOrder.compactMap({ model.panes[$0] }) { closePane(pane) }
        model.install(panes: panes, windows: windows, key: key, counters: counters)
        model.dropEmpty()
        onChange()
        for pane in panes where model.panes[pane.id] === pane { host?.start(pane) }
    }

    /// Checks a saved state and builds what it describes. Lenient where it can
    /// repair (dangling focus, empty tabs, sizes), strict about anything that
    /// would make the layout ambiguous.
    static func importState(_ state: JSON) throws -> (panes: [Pane], windows: [Window], key: String?, counters: (pane: Int, tab: Int, window: Int)) {
        func bad(_ message: String) -> KmuxError { KmuxError("bad_request", "bad state: \(message)") }
        func quoted(_ value: JSON?) -> String { String(decoding: (value ?? .null).encoded(), as: UTF8.self) }
        func number(_ id: String, _ prefix: String) -> Int? {
            guard id.hasPrefix(prefix), let n = Int(id.dropFirst(prefix.count)), n > 0, "\(prefix)\(n)" == id else { return nil }
            return n
        }
        func whole(_ value: JSON?) -> Int? {
            guard let n = value?.number, n.rounded() == n, n >= 0, n < 1e9 else { return nil }
            return Int(n)
        }
        guard case .object = state else { throw bad("not an object") }
        guard state["version"] == .number(Double(stateVersion)) else {
            throw KmuxError("bad_request", "unsupported state version \(quoted(state["version"])) (this kmux reads \(stateVersion))")
        }
        guard case .array(let savedPanes)? = state["panes"], case .array(let savedWindows)? = state["windows"] else { throw bad("needs panes and windows") }
        let saved = state["counters"] ?? [:]
        var counters = (pane: whole(saved["pane"]) ?? 0, tab: whole(saved["tab"]) ?? 0, window: whole(saved["window"]) ?? 0)

        var panes: [Pane] = [], byID: [String: Pane] = [:], names: Set<String> = []
        for item in savedPanes {
            guard case .object = item, let id = item["id"]?.string, let n = number(id, "p"), byID[id] == nil else { throw bad("pane \(quoted(item["id"]))") }
            guard let type = item["type"]?.string.flatMap(PaneType.init(rawValue:)) else { throw bad("pane \(id) has unknown type \(quoted(item["type"]))") }
            let name = item["name"]?.string
            if let name {
                guard !name.isEmpty, !names.contains(name) else { throw bad("pane name \(quoted(.string(name))) is used twice") }
                names.insert(name)
            }
            let needed: String? = switch type {
            case .web: "url"
            case .md: "path"
            case .ios: "app"
            case .term: nil
            }
            if let needed, item[needed]?.string?.isEmpty ?? true { throw bad("\(type.rawValue) pane \(id) has no \(needed)") }
            let pane = Pane(id: id, name: name, type: type)
            pane.command = item["cmd"]?.string
            pane.cwd = item["cwd"]?.string
            pane.session = item["session"]?.string
            pane.url = item["url"]?.string
            pane.path = item["path"]?.string
            pane.app = item["app"]?.string
            pane.device = item["device"]?.string
            if type == .md, let zoom = item["zoom"]?.number, zoom > 0 { pane.zoom = zoom }
            if (type == .web && item["history"] == true) || type == .md { pane.history = History() }
            if type != .term { pane.command = nil; pane.cwd = nil; pane.session = nil }
            panes.append(pane)
            byID[id] = pane
            counters.pane = max(counters.pane, n)
        }

        var placed: Set<String> = [], tabIDs: Set<String> = [], windowIDs: Set<String> = []
        func build(_ node: JSON) throws -> Node {
            guard case .object = node else { throw bad("malformed layout") }
            if let ref = node["pane"], ref != .null {
                guard let id = ref.string, byID[id] != nil else { throw bad("layout names unknown pane \(quoted(ref))") }
                guard !placed.contains(id) else { throw bad("pane \(id) is placed twice") }
                placed.insert(id)
                return .pane(id)
            }
            guard let axis = node["split"]?.string.flatMap(Axis.init(rawValue:)), case .array(let children)? = node["children"], !children.isEmpty else {
                throw bad("malformed layout")
            }
            let sizes = children.map { child -> Double in
                guard let size = child["size"]?.number, size > 0, size <= 1 else { return 1 / Double(children.count) }
                return size
            }
            let total = sizes.reduce(0, +)
            return .split(Split(axis: axis, kids: try zip(children, sizes).map { Child(node: try build($0), size: $1 / total) }))
        }

        var windows: [Window] = []
        for item in savedWindows {
            guard case .object = item, let id = item["id"]?.string, let n = number(id, "w"), !windowIDs.contains(id), case .array(let savedTabs)? = item["tabs"] else {
                throw bad("window \(quoted(item["id"]))")
            }
            windowIDs.insert(id)
            let window = Window(id: id)
            if let f = item["frame"], let x = f["x"]?.number, let y = f["y"]?.number, let w = f["w"]?.number, let h = f["h"]?.number,
               [x, y, w, h].allSatisfy(\.isFinite), w > 0, h > 0 {
                window.frame = Frame(x: x, y: y, w: w, h: h)
            }
            for entry in savedTabs {
                guard case .object = entry, let tabID = entry["id"]?.string, let tn = number(tabID, "t"), !tabIDs.contains(tabID) else {
                    throw bad("tab \(quoted(entry["id"]))")
                }
                tabIDs.insert(tabID)
                let root = try entry["layout"].flatMap { $0 == .null ? nil : Model.normalize(try build($0)) }
                let title = (entry["title"]?.string ?? "").trimmingCharacters(in: .whitespaces)
                let tab = Tab(id: tabID, title: title.isEmpty ? "Tab \(window.tabs.count + 1)" : title)
                tab.root = root
                let ids = Model.paneIDs(root)
                tab.lastFocus = entry["lastFocus"]?.string.flatMap { ids.contains($0) ? $0 : nil }
                window.tabs.append(tab)
                counters.tab = max(counters.tab, tn)
            }
            window.tabs.removeAll { $0.root == nil }
            let active = item["active"]?.string
            window.active = window.tabs.contains { $0.id == active } ? active : window.tabs.first?.id
            let shown = Model.paneIDs(window.activeTab?.root)
            let focused = item["focused"]?.string
            window.focused = focused.flatMap { shown.contains($0) ? $0 : nil } ?? window.activeTab?.lastFocus ?? shown.first
            window.zoomed = item["zoomed"]?.string.flatMap { shown.contains($0) ? $0 : nil }
            window.tabCount = max(whole(item["tabCount"]) ?? 0, window.tabs.count)
            windows.append(window)
            counters.window = max(counters.window, n)
        }
        // Panes no tab shows are not restored.
        panes.removeAll { !placed.contains($0.id) }
        windows.removeAll { $0.tabs.isEmpty }
        let key = state["key"]?.string.flatMap { key in windows.contains { $0.id == key } ? key : nil } ?? windows.last?.id
        return (panes, windows, key, counters)
    }
}
