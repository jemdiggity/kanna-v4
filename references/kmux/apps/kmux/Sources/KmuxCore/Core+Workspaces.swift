import Foundation

/// Workspaces (docs/kmux-spec.md §3.8): named groups of windows, shown or
/// hidden together. Ported from the reference model's core
/// (reference/kmux/core.js: `workspace` and `window`).
extension Core {
    func needWorkspace(_ ref: String?) throws -> Workspace {
        guard let ref, !ref.isEmpty else { throw KmuxError("bad_request", "missing workspace") }
        guard let workspace = model.workspace(ref) else { throw KmuxError("not_found", "no workspace \"\(ref)\"") }
        return workspace
    }

    func workspace(_ args: JSON) throws -> [String: JSON] {
        let action = args["action"]?.string ?? ""
        if action == "new" {
            let name = args["name"].flatMap { $0 == .null ? nil : ($0.string ?? "").trimmingCharacters(in: .whitespaces) }
            if name == "" { throw KmuxError("bad_request", "a workspace name cannot be empty") }
            if let name, model.workspace(name) != nil || name.range(of: "^(ws-|s)[1-9][0-9]*$", options: .regularExpression) != nil {
                throw KmuxError("name_taken", "a workspace named \"\(name)\" already exists")
            }
            let workspace = model.makeWorkspace(name: name)
            onChange()
            return ["workspace": model.workspaceView(workspace), "windows": []]
        }
        guard ["hide", "show", "close"].contains(action) else {
            throw KmuxError("bad_request", "unknown workspace action \"\(action)\" (new, hide, show or close)")
        }
        let workspace = try needWorkspace(args["workspace"]?.string)
        let windows = model.windows.filter { $0.workspace == workspace.id }
        if action == "close" {
            let victims = windows.flatMap { window in window.tabs.flatMap { Model.paneIDs($0.root) } + [window.dock?.pane].compactMap { $0 } }.compactMap { model.panes[$0] }
            for pane in victims { closePane(pane) }
            model.forget(workspace)
            model.dropEmpty()
            onChange()
            return ["workspace": .string(workspace.id), "closed": .array(victims.map { .string($0.id) })]
        }
        workspace.hidden = action == "hide"
        if !workspace.hidden, let last = windows.last { model.raise(last) }
        model.dropEmpty()
        onChange()
        return ["workspace": model.workspaceView(workspace), "windows": .array(windows.map { .string($0.id) })]
    }

    func windowCommand(_ args: JSON) throws -> [String: JSON] {
        guard let ref = args["window"]?.string else { throw KmuxError("bad_request", "window needs a window") }
        guard let window = model.window(ref) else { throw KmuxError("not_found", "no window \"\(ref)\"") }
        let was = model.workspace(of: window).map(model.workspaceView) ?? .null
        let before = model.isShown(window)
        switch args["action"]?.string {
        case "assign": window.workspace = try needWorkspace(args["workspace"]?.string).id
        case "free": window.workspace = nil
        case let action: throw KmuxError("bad_request", "unknown window action \"\(action ?? "")\" (assign or free)")
        }
        if model.isShown(window), !before { model.raise(window) }
        model.dropEmpty()
        onChange()
        var out: [String: JSON] = ["window": .string(window.id), "was": was, "shown": .bool(model.isShown(window))]
        if let workspace = model.workspace(of: window) { out["workspace"] = model.workspaceView(workspace) }
        return out
    }
}
