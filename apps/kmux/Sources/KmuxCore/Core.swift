import Foundation

public struct KmuxError: Error, Equatable {
    public let code: String
    public let message: String

    public init(_ code: String, _ message: String) {
        self.code = code
        self.message = message
    }
}

/// Starts and stops what is inside a pane. The app supplies Ghostty and web
/// views; tests supply a fake. Hosts report progress through `Core.update`.
@MainActor
public protocol PaneHost: AnyObject {
    func start(_ pane: Pane)
    func stop(_ pane: Pane)
    /// Types `text` into a running terminal pane and presses Return.
    func send(_ pane: Pane, text: String)
}

/// Handles control-protocol requests (docs/kmux-spec.md §7) against the model.
@MainActor
public final class Core {
    public let model = Model()
    public var host: PaneHost?
    /// Which kmux this is; `capabilities` reports its name.
    public var instance = Instance(name: Instance.defaultName)
    /// Called after every change so the UI can redraw.
    public var onChange: () -> Void = {}
    /// Whether a pane is wider than it is tall, for `split: auto`.
    public var paneIsWide: (String) -> Bool = { _ in true }
    public var startTimeout: Duration = .seconds(10)
    /// iOS panes may boot a simulator first, which takes much longer.
    public var iosStartTimeout: Duration = .seconds(180)
    /// Commands the app adds, such as `debug.snapshot`.
    public var extraCommands: [String: @MainActor (JSON) async throws -> [String: JSON]] = [:]

    private var waiters: [String: [CheckedContinuation<Void, Never>]] = [:]
    /// The last state `debug.saveState` made.
    private var savedState: JSON?

    public init() {}

    // MARK: Lifecycle reports from hosts

    public func update(_ paneID: String, state: PaneState, exitCode: Int? = nil, error: String? = nil) {
        guard let pane = model.panes[paneID] else { return }
        pane.state = state
        pane.exitCode = exitCode
        pane.error = error
        if state != .starting { waiters.removeValue(forKey: paneID)?.forEach { $0.resume() } }
        onChange()
    }

    func settled(_ pane: Pane) async {
        guard pane.state == .starting else { return }
        let id = pane.id
        let startTimeout = pane.type == .ios ? iosStartTimeout : startTimeout
        let timeout = Task { [startTimeout] in
            guard (try? await Task.sleep(for: startTimeout)) != nil, self.model.panes[id]?.state == .starting else { return }
            self.update(id, state: .failed, error: "did not start within \(startTimeout)")
        }
        await withCheckedContinuation { waiters[id, default: []].append($0) }
        timeout.cancel()
    }

    // MARK: Requests

    public func handle(_ request: JSON) async -> JSON {
        let id = request["id"] ?? .null
        do {
            guard let cmd = request["cmd"]?.string else { throw KmuxError("bad_request", "missing cmd") }
            var reply = try await run(cmd, request["args"] ?? [:])
            reply["id"] = id
            reply["ok"] = true
            return .object(reply)
        } catch let error as KmuxError {
            return ["id": id, "ok": false, "error": ["code": .string(error.code), "message": .string(error.message)]]
        } catch {
            return ["id": id, "ok": false, "error": ["code": "bad_request", "message": .string("\(error)")]]
        }
    }

    private func run(_ cmd: String, _ args: JSON) async throws -> [String: JSON] {
        switch cmd {
        case "capabilities":
            return ["mux": "kmux", "instance": .string(instance.name), "paneTypes": ["term", "web", "ios", "md"], "commands": .array(Self.commands.sorted().map(JSON.string)), "features": ["windows", "tabs", "fractionalSizing", "namedPanes", "zoom", "lifecycle"]]
        case "open": return try await open(args)
        case "list": return list()
        case "close": return try close(args)
        case "focus": return try focus(args)
        case "zoom": return try zoom(args)
        case "restart": return try await restart(args)
        case "rename-tab": return try renameTab(args)
        case "move": return try move(args)
        case "move-tab": return try moveTab(args)
        case "resize": return try resize(args)
        case "arrange": return try arrange(args)
        case "send": return try send(args)
        case "navigate": return try await navigate(args)
        // Persistence, for tests: the state kmux would save, and restoring one
        // as a launch would (without `state`, the last one saved).
        case "debug.saveState":
            savedState = exportState()
            return ["state": savedState!]
        case "debug.restoreState":
            guard let state = args["state"] ?? savedState else { throw KmuxError("bad_request", "no saved state to restore") }
            try restoreState(state)
            return ["windows": .array(model.windows.map { .string($0.id) }), "panes": .array(model.paneOrder.map(JSON.string))]
        default:
            guard let extra = extraCommands[cmd] else { throw KmuxError("bad_request", "unknown command \"\(cmd)\"") }
            return try await extra(args)
        }
    }

    /// Commands for tests that this core handles but `capabilities` doesn't list.
    public static let debugCommands: Set<String> = ["debug.saveState", "debug.restoreState"]

    /// The commands this core handles, besides `extraCommands`.
    public static let commands: Set<String> = [
        "capabilities", "open", "list", "close", "focus", "zoom", "restart", "rename-tab",
        "move", "move-tab", "resize", "arrange", "send", "navigate",
    ]

    public func needPane(_ ref: String?) throws -> Pane {
        guard let ref else { throw KmuxError("bad_request", "missing pane") }
        guard let pane = model.pane(ref) else { throw KmuxError("not_found", "no pane \"\(ref)\"") }
        return pane
    }

    public func targetWindow(_ ref: JSON?) throws -> Window {
        switch ref?.string {
        case nil: return model.keyWindow ?? model.makeWindow()
        case "new": return model.makeWindow()
        case let id?:
            guard let window = model.window(id) else { throw KmuxError("not_found", "no window \"\(id)\"") }
            return window
        }
    }

    private func open(_ args: JSON) async throws -> [String: JSON] {
        let typeName = args["type"]?.string ?? ""
        guard let type = PaneType(rawValue: typeName) else { throw KmuxError("bad_request", "unknown pane type \"\(typeName)\" (term, web, ios or md)") }
        if type == .ios, args["app"]?.string?.isEmpty ?? true { throw KmuxError("bad_request", "ios panes need an app") }
        if args["app"] != nil || args["device"] != nil, type != .ios { throw KmuxError("bad_request", "app and device are only for ios panes") }
        if type == .web, args["url"]?.string?.isEmpty ?? true { throw KmuxError("bad_request", "web panes need a url") }
        if args["history"] != nil, type != .web { throw KmuxError("bad_request", "history is only for web panes") }
        if type == .md, args["path"]?.string?.isEmpty ?? true { throw KmuxError("bad_request", "markdown panes need a path") }
        if args["path"] != nil, type != .md { throw KmuxError("bad_request", "path is only for markdown panes") }
        let name = args["name"]?.string
        if let name, model.pane(name) != nil { throw KmuxError("name_taken", "a pane named \"\(name)\" already exists") }
        let split = args["split"]?.string ?? "auto"
        guard ["right", "down", "auto"].contains(split) else { throw KmuxError("bad_request", "split must be right, down or auto") }
        let size = try args["size"].map { try fraction($0) } ?? 0.5
        guard size > 0, size < 1 else { throw KmuxError("bad_request", "size must be a fraction between 0 and 1") }

        let window = try targetWindow(args["window"])
        let pane = model.makePane(type: type, name: name)
        pane.command = args["cmd"]?.string
        pane.cwd = args["cwd"]?.string
        pane.url = args["url"]?.string.map(Self.normalizeURL)
        pane.app = args["app"]?.string
        pane.device = args["device"]?.string
        pane.path = args["path"]?.string
        if (type == .web && args["history"] == true) || type == .md { pane.history = History() }
        place(pane.id, in: window, split: split, size: size, newTab: args["tab"]?.bool ?? false)
        window.focused = pane.id
        window.activeTab?.lastFocus = pane.id
        model.key = window.id
        onChange()
        host?.start(pane)

        let reply = { [model] () -> [String: JSON] in
            let location = model.locate(pane.id)
            return ["pane": model.summary(pane), "window": .string(location?.window.id ?? window.id), "tab": location.map { .string($0.tab.id) } ?? nil]
        }
        if args["wait"]?.bool == false { return reply() }
        await settled(pane)
        if pane.state == .failed { throw KmuxError("start_failed", pane.error ?? "\(pane.label) failed to start") }
        return reply()
    }

    func place(_ paneID: String, in window: Window, split: String, size: Double, newTab: Bool) {
        let tab = newTab || window.tabs.isEmpty ? model.makeTab(in: window) : window.activeTab!
        window.zoomed = nil
        guard tab.root != nil else {
            tab.root = .pane(paneID)
            return
        }
        let ids = Model.paneIDs(tab.root)
        let target = window.focused.flatMap { ids.contains($0) ? $0 : nil } ?? ids.last!
        let axis: Axis = switch split {
        case "right": .row
        case "down": .column
        default: paneIsWide(target) ? .row : .column
        }
        model.insert(.pane(paneID), beside: target, in: tab, axis: axis, after: true, size: size)
    }

    private func list() -> [String: JSON] {
        [
            "windows": .array(model.windows.map { window in
                [
                    "id": .string(window.id), "key": .bool(window.id == model.key),
                    "focused": window.focused.map { .string(model.panes[$0]?.name ?? $0) } ?? nil,
                    "zoomed": window.zoomed.map { .string(model.panes[$0]?.name ?? $0) } ?? nil,
                    "tabs": .array(window.tabs.map { tab in
                        ["id": .string(tab.id), "title": .string(tab.title), "active": .bool(tab.id == window.active), "layout": model.tree(tab.root)]
                    }),
                ]
            }),
            "panes": .array(model.paneOrder.compactMap { model.panes[$0] }.map(model.summary)),
        ]
    }

    private func close(_ args: JSON) throws -> [String: JSON] {
        let victims: [Pane]
        if let ref = args["pane"]?.string {
            victims = [try needPane(ref)]
        } else if let ref = args["tab"]?.string {
            guard let tab = model.tab(ref) else { throw KmuxError("not_found", "no tab \"\(ref)\"") }
            victims = Model.paneIDs(tab.root).compactMap { model.panes[$0] }
        } else if args["window"] != nil {
            let window = try targetWindow(args["window"])
            victims = window.tabs.flatMap { Model.paneIDs($0.root) }.compactMap { model.panes[$0] }
        } else {
            throw KmuxError("bad_request", "close needs a pane, tab or window")
        }
        for pane in victims { closePane(pane) }
        model.dropEmpty()
        onChange()
        return ["closed": .array(victims.map { .string($0.id) })]
    }

    private func focus(_ args: JSON) throws -> [String: JSON] {
        if let ref = args["pane"]?.string {
            focusPane(try needPane(ref))
        } else if let ref = args["tab"]?.string {
            guard let tab = model.tab(ref), let window = model.windows.first(where: { $0.tabs.contains { $0 === tab } }) else {
                throw KmuxError("not_found", "no tab \"\(ref)\"")
            }
            let ids = Model.paneIDs(tab.root)
            window.active = tab.id
            window.zoomed = nil
            window.focused = tab.lastFocus.flatMap { ids.contains($0) ? $0 : nil } ?? ids.first
            model.key = window.id
        } else if args["window"] != nil {
            model.key = try targetWindow(args["window"]).id
        } else {
            throw KmuxError("bad_request", "focus needs a pane, tab or window")
        }
        onChange()
        return ["window": model.key.map(JSON.string) ?? nil]
    }

    func focusPane(_ pane: Pane) {
        guard let location = model.locate(pane.id) else { return }
        let window = location.window
        window.active = location.tab.id
        if let zoomed = window.zoomed, zoomed != pane.id { window.zoomed = nil }
        window.focused = pane.id
        location.tab.lastFocus = pane.id
        model.key = window.id
    }

    private func renameTab(_ args: JSON) throws -> [String: JSON] {
        let ref = args["tab"]?.string ?? ""
        guard let tab = model.tab(ref) else { throw KmuxError("not_found", "no tab \"\(ref)\"") }
        let title = (args["title"]?.string ?? "").trimmingCharacters(in: .whitespaces)
        guard !title.isEmpty else { throw KmuxError("bad_request", "a tab title cannot be empty") }
        tab.title = title
        onChange()
        return ["tab": .string(tab.id), "title": .string(title)]
    }

    private func zoom(_ args: JSON) throws -> [String: JSON] {
        let pane = try needPane(args["pane"]?.string)
        guard let window = model.locate(pane.id)?.window else { throw KmuxError("not_found", "no pane \"\(pane.id)\"") }
        let was = window.zoomed == pane.id
        focusPane(pane)
        window.zoomed = was ? nil : pane.id
        onChange()
        return ["zoomed": .bool(window.zoomed != nil)]
    }

    private func restart(_ args: JSON) async throws -> [String: JSON] {
        let pane = try needPane(args["pane"]?.string)
        host?.stop(pane)
        pane.state = .starting
        pane.exitCode = nil
        pane.error = nil
        onChange()
        host?.start(pane)
        await settled(pane)
        if pane.state == .failed { throw KmuxError("start_failed", pane.error ?? "\(pane.label) failed to start") }
        return ["pane": model.summary(pane)]
    }

    /// The pane `step` places after the focused one in reading order, wrapping.
    public func cyclePane(in window: Window, by step: Int) -> String? {
        let ids = Model.paneIDs(window.activeTab?.root)
        guard !ids.isEmpty else { return nil }
        let at = window.focused.flatMap { ids.firstIndex(of: $0) } ?? 0
        return ids[((at + step) % ids.count + ids.count) % ids.count]
    }

    /// The tab `step` places after the active one, wrapping.
    public func cycleTab(in window: Window, by step: Int) -> String? {
        guard !window.tabs.isEmpty else { return nil }
        let at = window.tabs.firstIndex { $0.id == window.active } ?? 0
        return window.tabs[((at + step) % window.tabs.count + window.tabs.count) % window.tabs.count].id
    }

    /// Removes a pane from the layout and stops it. Callers run `dropEmpty`.
    public func closePane(_ pane: Pane) {
        model.detach(pane.id)
        model.forget(pane)
        host?.stop(pane)
        waiters.removeValue(forKey: pane.id)?.forEach { $0.resume() }
    }

    func fraction(_ value: JSON) throws -> Double {
        guard let result = Fraction.parse(value) else { throw KmuxError("bad_request", "\"\(value)\" is not a fraction") }
        return result
    }
}
