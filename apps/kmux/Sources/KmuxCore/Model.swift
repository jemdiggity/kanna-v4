import Foundation

public enum PaneType: String, Sendable { case term, web, ios }
public enum PaneState: String, Sendable { case starting, running, exited, failed }
public enum Axis: String, Sendable { case row, column }

public final class Pane {
    public let id: String
    public var name: String?
    public let type: PaneType
    public var state: PaneState = .starting
    public var exitCode: Int?
    public var error: String?
    public var command: String?
    public var cwd: String?

    init(id: String, name: String?, type: PaneType) {
        self.id = id
        self.name = name
        self.type = type
    }

    public var label: String { name.map { "\($0) (\(id))" } ?? id }
}

public final class Split {
    public var axis: Axis
    public var kids: [Child]

    init(axis: Axis, kids: [Child]) {
        self.axis = axis
        self.kids = kids
    }
}

public struct Child {
    public var node: Node
    public var size: Double
}

public enum Node {
    case pane(String)
    case split(Split)
}

public final class Tab {
    public let id: String
    public var title: String
    public var root: Node?
    public var lastFocus: String?

    init(id: String, title: String) {
        self.id = id
        self.title = title
    }
}

public final class Window {
    public let id: String
    public var tabs: [Tab] = []
    public var active: String?
    public var focused: String?
    public var zoomed: String?
    /// Tabs are named "Tab 1", "Tab 2", … in the order the window made them.
    var tabCount = 0

    init(id: String) { self.id = id }

    public var activeTab: Tab? { tabs.first { $0.id == active } ?? tabs.first }
}

struct Location {
    let window: Window
    let tab: Tab
    let parent: Split?
    let index: Int
}

/// Windows, tabs and split trees. Sizes are fractions of the parent split and
/// always add up to 1. Empty containers close: a tab without panes, then a
/// window without tabs. Mirrors reference/kmux/index.html.
public final class Model {
    public private(set) var panes: [String: Pane] = [:]
    public private(set) var paneOrder: [String] = []
    public private(set) var windows: [Window] = []
    public var key: String?
    private var counters = (pane: 0, tab: 0, window: 0)

    public init() {}

    public var keyWindow: Window? { windows.first { $0.id == key } }

    public func window(_ id: String) -> Window? { windows.first { $0.id == id } }

    public func pane(_ ref: String) -> Pane? {
        panes[ref] ?? paneOrder.lazy.compactMap { self.panes[$0] }.first { $0.name == ref }
    }

    public func tab(_ id: String) -> Tab? { windows.lazy.flatMap(\.tabs).first { $0.id == id } }

    func makeWindow() -> Window {
        counters.window += 1
        let window = Window(id: "w\(counters.window)")
        windows.append(window)
        key = window.id
        return window
    }

    @discardableResult
    func makeTab(in window: Window, activate: Bool = true) -> Tab {
        counters.tab += 1
        window.tabCount += 1
        let tab = Tab(id: "t\(counters.tab)", title: "Tab \(window.tabCount)")
        window.tabs.append(tab)
        if activate || window.active == nil { window.active = tab.id }
        return tab
    }

    func makePane(type: PaneType, name: String?) -> Pane {
        counters.pane += 1
        let pane = Pane(id: "p\(counters.pane)", name: name, type: type)
        panes[pane.id] = pane
        paneOrder.append(pane.id)
        return pane
    }

    func forget(_ pane: Pane) {
        panes[pane.id] = nil
        paneOrder.removeAll { $0 == pane.id }
    }

    // MARK: Trees

    public static func paneIDs(_ node: Node?) -> [String] {
        switch node {
        case nil: []
        case .pane(let id): [id]
        case .split(let split): split.kids.flatMap { paneIDs($0.node) }
        }
    }

    static func locate(_ node: Node?, _ id: String, parent: Split? = nil, index: Int = -1) -> (parent: Split?, index: Int)? {
        switch node {
        case nil: return nil
        case .pane(let paneID): return paneID == id ? (parent, index) : nil
        case .split(let split):
            for (i, kid) in split.kids.enumerated() {
                if let found = locate(kid.node, id, parent: split, index: i) { return found }
            }
            return nil
        }
    }

    func locate(_ id: String) -> Location? {
        for window in windows {
            for tab in window.tabs {
                if let found = Self.locate(tab.root, id) { return Location(window: window, tab: tab, parent: found.parent, index: found.index) }
            }
        }
        return nil
    }

    static func normalize(_ node: Node?) -> Node? {
        guard case .split(let split) = node else { return node }
        for i in split.kids.indices { split.kids[i].node = normalize(split.kids[i].node)! }
        return split.kids.count == 1 ? split.kids[0].node : node
    }

    /// Removes a pane from its tree; its siblings share the freed space.
    func detach(_ id: String) {
        guard let location = locate(id) else { return }
        if let parent = location.parent {
            let gone = parent.kids.remove(at: location.index).size
            let count = Double(parent.kids.count)
            for i in parent.kids.indices { parent.kids[i].size = gone < 1 ? parent.kids[i].size / (1 - gone) : 1 / count }
            location.tab.root = Self.normalize(location.tab.root)
        } else {
            location.tab.root = nil
        }
        if location.window.zoomed == id { location.window.zoomed = nil }
    }

    /// Puts `node` next to the target pane, taking `size` of the target's space.
    /// A parent split along the same axis takes it as a sibling.
    func insert(_ node: Node, beside targetID: String, in tab: Tab, axis: Axis, after: Bool, size: Double) {
        guard let found = Self.locate(tab.root, targetID) else { return }
        if let parent = found.parent, parent.axis == axis {
            let share = parent.kids[found.index].size
            parent.kids[found.index].size = share * (1 - size)
            parent.kids.insert(Child(node: node, size: share * size), at: found.index + (after ? 1 : 0))
            return
        }
        var kids = [Child(node: .pane(targetID), size: 1 - size), Child(node: node, size: size)]
        if !after { kids.reverse() }
        let split = Node.split(Split(axis: axis, kids: kids))
        if let parent = found.parent { parent.kids[found.index].node = split } else { tab.root = split }
    }

    /// Closes empty tabs and windows and repairs focus.
    func dropEmpty() {
        for window in windows {
            let at = max(0, window.tabs.firstIndex { $0.id == window.active } ?? 0)
            window.tabs.removeAll { $0.root == nil }
            if !window.tabs.contains(where: { $0.id == window.active }) {
                window.active = window.tabs.isEmpty ? nil : window.tabs[min(at, window.tabs.count - 1)].id
            }
            let ids = Self.paneIDs(window.activeTab?.root)
            if let focused = window.focused, ids.contains(focused) { continue }
            if let last = window.activeTab?.lastFocus, ids.contains(last) { window.focused = last } else { window.focused = ids.first }
            if let zoomed = window.zoomed, !ids.contains(zoomed) { window.zoomed = nil }
        }
        windows.removeAll { $0.tabs.isEmpty }
        if keyWindow == nil { key = windows.last?.id }
    }

    // MARK: Views for the protocol

    public func tree(_ node: Node?, size: Double? = nil) -> JSON {
        var out: [String: JSON]
        switch node {
        case nil: return .null
        case .pane(let id): out = ["pane": .string(panes[id]?.name ?? id)]
        case .split(let split): out = ["split": .string(split.axis.rawValue), "children": .array(split.kids.map { tree($0.node, size: $0.size) })]
        }
        if let size, size < 1 - 1e-6 { out["size"] = .string(Fraction.format(size)) }
        return .object(out)
    }

    public func summary(_ pane: Pane) -> JSON {
        var out: [String: JSON] = ["id": .string(pane.id), "name": pane.name.map(JSON.string) ?? .null, "type": .string(pane.type.rawValue), "state": .string(pane.state.rawValue)]
        if pane.type == .term {
            out["cmd"] = pane.command.map(JSON.string) ?? .null
            out["cwd"] = pane.cwd.map(JSON.string) ?? .null
        }
        if let code = pane.exitCode { out["exitCode"] = .number(Double(code)) }
        if let error = pane.error { out["error"] = .string(error) }
        return .object(out)
    }
}
