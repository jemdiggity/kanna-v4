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
}

/// Handles control-protocol requests (docs/kmux-spec.md §7) against the model.
@MainActor
public final class Core {
    public let model = Model()
    public var host: PaneHost?
    /// Called after every change so the UI can redraw.
    public var onChange: () -> Void = {}
    /// Whether a pane is wider than it is tall, for `split: auto`.
    public var paneIsWide: (String) -> Bool = { _ in true }
    public var startTimeout: Duration = .seconds(10)

    private var waiters: [String: [CheckedContinuation<Void, Never>]] = [:]

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

    private func settled(_ pane: Pane) async {
        guard pane.state == .starting else { return }
        let id = pane.id
        let timeout = Task { [startTimeout] in
            try? await Task.sleep(for: startTimeout)
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
            return ["mux": "kmux", "paneTypes": ["term"], "features": ["windows", "tabs", "fractionalSizing", "namedPanes", "lifecycle"]]
        case "open": return try await open(args)
        case "list": return list()
        case "close": return try close(args)
        default: throw KmuxError("bad_request", "unknown command \"\(cmd)\"")
        }
    }

    public func needPane(_ ref: String?) throws -> Pane {
        guard let ref else { throw KmuxError("bad_request", "missing pane") }
        guard let pane = model.pane(ref) else { throw KmuxError("not_found", "no pane \"\(ref)\"") }
        return pane
    }

    func targetWindow(_ ref: JSON?) throws -> Window {
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
        guard let type = PaneType(rawValue: typeName) else { throw KmuxError("bad_request", "unknown pane type \"\(typeName)\" (term, web or ios)") }
        guard type == .term else { throw KmuxError("bad_request", "\(type.rawValue) panes are not supported yet") }
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
        place(pane.id, in: window, split: split, size: size, newTab: args["tab"]?.bool ?? false)
        window.focused = pane.id
        window.activeTab?.lastFocus = pane.id
        model.key = window.id
        onChange()
        host?.start(pane)

        let reply = { [model] () -> [String: JSON] in ["pane": model.summary(pane), "window": .string(model.locate(pane.id)?.window.id ?? window.id)] }
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
                    "tabs": .array(window.tabs.map { tab in
                        ["id": .string(tab.id), "title": .string(model.title(of: tab)), "active": .bool(tab.id == window.active), "layout": model.tree(tab.root)]
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

    /// Removes a pane from the layout and stops it. Callers run `dropEmpty`.
    public func closePane(_ pane: Pane) {
        model.detach(pane.id)
        model.forget(pane)
        host?.stop(pane)
        waiters.removeValue(forKey: pane.id)?.forEach { $0.resume() }
    }

    private func fraction(_ value: JSON) throws -> Double {
        guard let result = Fraction.parse(value) else { throw KmuxError("bad_request", "\"\(value)\" is not a fraction") }
        return result
    }
}
