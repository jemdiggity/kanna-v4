import Foundation

/// Commands that rearrange panes and tabs, and act on pane contents. Ported
/// from the reference model's core (reference/kmux/core.js).
extension Core {
    static func normalizeURL(_ url: String) -> String {
        url.range(of: "^[a-z][a-z0-9+.-]*://", options: [.regularExpression, .caseInsensitive]) == nil ? "http://" + url : url
    }

    func needTab(_ ref: String?) throws -> Tab {
        guard let ref, let tab = model.tab(ref) else { throw KmuxError("not_found", "no tab \"\(ref ?? "")\"") }
        return tab
    }

    func move(_ args: JSON) throws -> [String: JSON] {
        let pane = try needPane(args["pane"]?.string)
        guard let from = model.locate(pane.id) else { throw KmuxError("not_found", "no pane \"\(pane.id)\"") }
        if let ref = args["to"]?.string {
            let target = try needPane(ref)
            let side = args["side"]?.string ?? "swap"
            guard ["left", "right", "top", "bottom", "swap"].contains(side) else {
                throw KmuxError("bad_request", "side must be left, right, top, bottom or swap")
            }
            if target.id == pane.id { return ["window": .string(from.window.id)] }
            if side == "swap" {
                model.swap(pane.id, target.id)
                if from.window.focused == pane.id { from.window.focused = target.id }
            } else {
                model.detach(pane.id)
                guard let tab = model.locate(target.id)?.tab else { throw KmuxError("not_found", "no pane \"\(ref)\"") }
                model.insert(.pane(pane.id), beside: target.id, in: tab, axis: side == "left" || side == "right" ? .row : .column,
                             after: side == "right" || side == "bottom", size: 0.5)
            }
        } else {
            let destination: Tab
            if let ref = args["tab"]?.string, ref != "new" {
                destination = try needTab(ref)
            } else if args["tab"]?.string == "new" {
                destination = model.makeTab(in: args["window"] != nil ? try targetWindow(args["window"]) : from.window)
            } else if args["window"] != nil {
                let window = try targetWindow(args["window"])
                destination = window.activeTab ?? model.makeTab(in: window)
            } else {
                throw KmuxError("bad_request", "move needs to + side, tab, or window")
            }
            if destination === from.tab { return ["window": .string(from.window.id)] }
            model.detach(pane.id)
            model.append(pane.id, to: destination)
        }
        focusPane(pane)
        model.dropEmpty()
        onChange()
        guard let location = model.locate(pane.id) else { return [:] }
        return ["window": .string(location.window.id), "tab": .string(location.tab.id), "layout": model.tree(location.tab.root)]
    }

    func moveTab(_ args: JSON) throws -> [String: JSON] {
        let tab = try needTab(args["tab"]?.string)
        guard let source = model.window(of: tab) else { throw KmuxError("not_found", "no tab \"\(tab.id)\"") }
        let destination = try targetWindow(args["window"] ?? .string(source.id))
        var index: Int?
        if let value = args["index"] {
            guard let number = value.number, number >= 0, number.rounded() == number else {
                throw KmuxError("bad_request", "index must be a whole number ≥ 0")
            }
            index = Int(number)
        }
        model.moveTab(tab, to: destination, index: index)
        destination.active = tab.id
        let ids = Model.paneIDs(tab.root)
        destination.focused = tab.lastFocus.flatMap { ids.contains($0) ? $0 : nil } ?? ids.first
        destination.zoomed = nil
        model.key = destination.id
        model.dropEmpty()
        onChange()
        return ["window": .string(destination.id), "tabs": .array(destination.tabs.map { .string($0.id) })]
    }

    func resize(_ args: JSON) throws -> [String: JSON] {
        let pane = try needPane(args["pane"]?.string)
        let size = try fraction(args["size"] ?? .null)
        guard size > 0, size < 1 else { throw KmuxError("bad_request", "size must be a fraction between 0 and 1") }
        guard let location = model.locate(pane.id) else { throw KmuxError("not_found", "no pane \"\(pane.id)\"") }
        guard let parent = location.parent else {
            throw KmuxError("bad_request", "\(pane.label) fills its tab, so there is nothing to resize against")
        }
        let old = parent.kids[location.index].size
        for i in parent.kids.indices where i != location.index { parent.kids[i].size *= (1 - size) / (1 - old) }
        parent.kids[location.index].size = size
        onChange()
        return ["layout": model.tree(location.tab.root)]
    }

    func arrange(_ args: JSON) throws -> [String: JSON] {
        var used: [String] = []
        func build(_ node: JSON) throws -> Node {
            if let ref = node["pane"]?.string {
                let pane = try needPane(ref)
                if used.contains(pane.id) { throw KmuxError("layout_invalid", "\"\(ref)\" appears more than once") }
                used.append(pane.id)
                return .pane(pane.id)
            }
            guard let axis = node["split"]?.string.flatMap(Axis.init(rawValue:)), case .array(let children) = node["children"] ?? nil, !children.isEmpty else {
                throw KmuxError("layout_invalid", "malformed layout tree")
            }
            let sizes = try children.map { child -> Double? in
                guard let value = child["size"], value != .null else { return nil }
                let size = try fraction(value)
                guard size > 0, size <= 1 else { throw KmuxError("layout_invalid", "sizes must be fractions between 0 and 1") }
                return size
            }
            let given = sizes.compactMap { $0 }.reduce(0, +)
            let unsized = sizes.filter { $0 == nil }.count
            if given > 1 + 1e-9 { throw KmuxError("layout_invalid", "sizes in a \(axis.rawValue) add up to \(Fraction.format(given)), more than 1") }
            if unsized > 0, given >= 1 - 1e-9 { throw KmuxError("layout_invalid", "no space left for the unsized panes in a \(axis.rawValue)") }
            let fill = unsized > 0 ? (1 - given) / Double(unsized) : 0
            let scale = unsized > 0 ? 1 : 1 / given
            return .split(Split(axis: axis, kids: try zip(children, sizes).map { Child(node: try build($0), size: ($1 ?? fill) * scale) }))
        }
        let root = Model.normalize(try build(args["layout"] ?? .null))!
        let window = try targetWindow(args["window"])
        let tab = window.activeTab ?? model.makeTab(in: window)
        for id in used { model.detach(id) }
        let leftovers = Model.paneIDs(tab.root)
        tab.root = root
        if !leftovers.isEmpty {
            let rest = model.makeTab(in: window, activate: false)
            rest.title = "unarranged"
            rest.root = Model.normalize(.split(Split(axis: .row, kids: leftovers.map { Child(node: .pane($0), size: 1 / Double(leftovers.count)) })))
        }
        window.zoomed = nil
        window.active = tab.id
        if !used.contains(window.focused ?? "") { window.focused = used.first }
        model.key = window.id
        model.dropEmpty()
        onChange()
        return ["window": .string(window.id), "layout": model.tree(tab.root)]
    }

    func send(_ args: JSON) throws -> [String: JSON] {
        let pane = try needPane(args["pane"]?.string)
        guard pane.type == .term else { throw KmuxError("wrong_type", "\(pane.label) is a \(pane.type.rawValue) pane; send only works on term panes") }
        guard pane.state == .running else { throw KmuxError("bad_request", "\(pane.label) is \(pane.state.rawValue)") }
        host?.send(pane, text: args["text"]?.string ?? "")
        return [:]
    }

    /// The pane's page is now `url` (asked for, or a link the page followed).
    func moved(_ pane: Pane, to url: String) {
        func same(_ a: String, _ b: String) -> Bool { a == b || a + "/" == b || a == b + "/" }
        if pane.history != nil, let current = pane.url, !same(current, url) {
            pane.history?.back.append(current)
            pane.history?.forward = []
        }
        pane.url = url
    }

    /// A web page followed a link or redirect on its own.
    public func pageMoved(_ id: String, to url: String) {
        guard let pane = model.panes[id] else { return }
        moved(pane, to: url)
    }

    /// A link's target relative to the file it is in: docs/a.md + ../b.md → b.md.
    static func resolve(_ link: String, from file: String) -> String {
        if link.hasPrefix("/") { return link }
        var parts = file.split(separator: "/", omittingEmptySubsequences: false).dropLast().map(String.init)
        for part in link.split(separator: "/").map(String.init) {
            if part == ".." { if !parts.isEmpty { parts.removeLast() } } else if part != "." && !part.isEmpty { parts.append(part) }
        }
        return parts.joined(separator: "/")
    }

    func navigate(_ args: JSON) async throws -> [String: JSON] {
        let pane = try needPane(args["pane"]?.string)
        if pane.type == .md {
            var history = pane.history ?? History()
            let current = pane.path ?? ""
            if let step = args["back"] == true ? "back" : args["forward"] == true ? "forward" : nil {
                guard let path = step == "back" ? history.back.popLast() : history.forward.popLast() else {
                    throw KmuxError("bad_request", "nothing to go \(step) to")
                }
                if step == "back" { history.forward.append(current) } else { history.back.append(current) }
                pane.path = path
            } else {
                guard let path = args["path"]?.string, !path.isEmpty else { throw KmuxError("bad_request", "missing path") }
                let target = Self.resolve(path, from: current)
                if target != current {
                    history.back.append(current)
                    history.forward = []
                }
                pane.path = target
            }
            pane.history = history
            host?.stop(pane)
            pane.state = .starting
            pane.error = nil
            onChange()
            host?.start(pane)
            await settled(pane)
            return ["pane": model.summary(pane)]
        }
        guard pane.type == .web else { throw KmuxError("wrong_type", "\(pane.label) is a \(pane.type.rawValue) pane; navigate only works on web and markdown panes") }
        if let step = args["back"] == true ? "back" : args["forward"] == true ? "forward" : nil {
            guard var history = pane.history else { throw KmuxError("bad_request", "\(pane.label) keeps no history; open it with history: true") }
            guard let url = step == "back" ? history.back.popLast() : history.forward.popLast() else {
                throw KmuxError("bad_request", "nothing to go \(step) to")
            }
            if let current = pane.url {
                if step == "back" { history.forward.append(current) } else { history.back.append(current) }
            }
            pane.history = history
            pane.url = url
        } else {
            guard let url = args["url"]?.string, !url.isEmpty else { throw KmuxError("bad_request", "missing url") }
            moved(pane, to: Self.normalizeURL(url))
        }
        host?.stop(pane)
        pane.state = .starting
        pane.error = nil
        onChange()
        host?.start(pane)
        await settled(pane)
        return ["pane": model.summary(pane)]
    }
}
