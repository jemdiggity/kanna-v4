import AppKit
import GhosttyKit
import KmuxCore

/// Owns the model, the control socket and one WindowController per window,
/// and keeps the windows in step with the model.
@MainActor
final class AppController: NSObject, NSApplicationDelegate {
    private let core = Core()
    private var host: TerminalHost!
    private var server: SocketServer?
    private var controllers: [String: WindowController] = [:]

    func applicationDidFinishLaunching(_ notification: Notification) {
        do {
            host = TerminalHost(runtime: try GhosttyRuntime())
        } catch {
            fatal("kmux: \(error.localizedDescription)")
        }
        host.core = core
        host.onFocus = { [weak self] id in self?.focused(id) }
        host.onCloseRequest = { [weak self] id in self?.request(["cmd": "close", "args": ["pane": .string(id)]]) }
        core.host = host
        core.onChange = { [weak self] in self?.sync() }
        core.paneIsWide = { [weak self] id in
            guard let bounds = self?.host.views[id]?.bounds, bounds.height > 0 else { return true }
            return bounds.width >= bounds.height
        }
        core.extraCommands["debug.snapshot"] = { [weak self] args in try self?.snapshot(args) ?? [:] }
        core.extraCommands["debug.key"] = { [weak self] args in try self?.pressKey(args) ?? [:] }
        core.extraCommands["debug.menu"] = { _ in
            let items = (NSApp.mainMenu?.items ?? []).flatMap { top in
                (top.submenu?.items ?? []).filter { !$0.isSeparatorItem }.map { item -> JSON in
                    let mods = item.keyEquivalentModifierMask
                    let shortcut = (mods.contains(.control) ? "⌃" : "") + (mods.contains(.option) ? "⌥" : "") + (mods.contains(.shift) ? "⇧" : "")
                        + (mods.contains(.command) ? "⌘" : "") + item.keyEquivalent.replacingOccurrences(of: "\r", with: "↩")
                    return ["menu": .string(top.submenu?.title ?? ""), "item": .string(item.title), "shortcut": .string(item.keyEquivalent.isEmpty ? "" : shortcut)]
                }
            }
            return ["items": .array(items)]
        }
        host.runtime.onMuxAction = { [weak self] view, action in
            guard let self, let id = host.paneID(of: view) else { return false }
            return ghosttyAction(id, action)
        }

        let server = SocketServer { [weak self] request in await self?.core.handle(request) ?? nil }
        do {
            try server.start()
            self.server = server
        } catch let error as KmuxError {
            fatal("kmux: \(error.message)")
        } catch {
            fatal("kmux: \(error)")
        }
        installMenu()
        NSApp.activate()
        if ProcessInfo.processInfo.environment["KMUX_NO_INITIAL_WINDOW"] == nil { newWindow() }
    }

    func applicationWillTerminate(_ notification: Notification) { server?.stop() }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        if controllers.isEmpty { newWindow() }
        return true
    }

    // MARK: Model → windows

    private func sync() {
        let ids = Set(core.model.windows.map(\.id))
        for (id, controller) in controllers where !ids.contains(id) {
            controller.window.orderOut(nil)
            controllers[id] = nil
        }
        for state in core.model.windows {
            let controller = controllers[state.id] ?? makeController(state.id)
            controller.render(core.model, host)
        }
        if let key = core.model.key, let controller = controllers[key], !controller.window.isKeyWindow {
            controller.window.makeKeyAndOrderFront(nil)
        }
    }

    private func makeController(_ id: String) -> WindowController {
        let previous = controllers.values.map(\.window).max { $0.orderedIndex > $1.orderedIndex }
        let controller = WindowController(id: id, cascadeFrom: previous)
        controller.relayout = { [weak self, weak controller] in
            guard let self, let controller else { return }
            controller.render(core.model, host)
        }
        controller.tabBar.onSelect = { [weak self] tab in self?.request(["cmd": "focus", "args": ["tab": .string(tab)]]) }
        controller.tabBar.onClose = { [weak self] tab in self?.request(["cmd": "close", "args": ["tab": .string(tab)]]) }
        controller.tabBar.onNew = { [weak self] in self?.newTab(in: id) }
        controller.onCloseRequest = { [weak self] in self?.request(["cmd": "close", "args": ["window": .string(id)]]) }
        controller.onBecomeKey = { [weak self, weak controller] in
            guard let self, let controller else { return }
            if controller.window.isKeyWindow { core.model.key = id }
            controller.render(core.model, host)
        }
        controllers[id] = controller
        controller.window.makeKeyAndOrderFront(nil)
        return controller
    }

    private func focused(_ paneID: String) {
        guard let window = core.model.windows.first(where: { Model.paneIDs($0.activeTab?.root).contains(paneID) }) else { return }
        guard window.focused != paneID else { return }
        window.focused = paneID
        window.activeTab?.lastFocus = paneID
        controllers[window.id]?.render(core.model, host)
    }

    // MARK: Actions

    private func request(_ request: JSON) {
        Task { @MainActor in
            let reply = await core.handle(request)
            if reply["ok"] != true { NSLog("kmux: \(String(decoding: reply.encoded(), as: UTF8.self))") }
        }
    }

    // MARK: Shortcuts (docs/kmux-spec.md §5). Each acts on the key window.

    private var keyWindow: Window? { core.model.keyWindow }
    private var focusedPane: String? { keyWindow?.focused }

    @objc func splitRight() { split("right") }
    @objc func splitDown() { split("down") }
    private func split(_ direction: String) {
        var args: [String: JSON] = ["type": "term", "split": .string(direction)]
        if let window = keyWindow?.id { args["window"] = .string(window) }
        request(["cmd": "open", "args": .object(args)])
    }

    @objc func nextPane() { cyclePane(1) }
    @objc func previousPane() { cyclePane(-1) }
    private func cyclePane(_ step: Int) {
        guard let window = keyWindow, let pane = core.cyclePane(in: window, by: step) else { return }
        request(["cmd": "focus", "args": ["pane": .string(pane)]])
    }

    @objc func toggleZoom() {
        guard let pane = focusedPane else { return }
        request(["cmd": "zoom", "args": ["pane": .string(pane)]])
    }

    @objc func restartPane() {
        guard let pane = focusedPane else { return }
        request(["cmd": "restart", "args": ["pane": .string(pane)]])
    }

    @objc func closePane() {
        guard let pane = focusedPane else { return }
        request(["cmd": "close", "args": ["pane": .string(pane)]])
    }

    @objc func newTab() { newTab(in: keyWindow?.id) }
    private func newTab(in window: String?) {
        var args: [String: JSON] = ["type": "term", "tab": true]
        if let window { args["window"] = .string(window) }
        request(["cmd": "open", "args": .object(args)])
    }

    @objc func nextTab() { cycleTab(1) }
    @objc func previousTab() { cycleTab(-1) }
    private func cycleTab(_ step: Int) {
        guard let window = keyWindow, let tab = core.cycleTab(in: window, by: step) else { return }
        request(["cmd": "focus", "args": ["tab": .string(tab)]])
    }

    @objc func newWindow() { request(["cmd": "open", "args": ["type": "term", "window": "new"]]) }

    @objc func closeWindow() {
        guard let window = keyWindow?.id else { return }
        request(["cmd": "close", "args": ["window": .string(window)]])
    }

    @objc func nextWindow() { cycleWindow(1) }
    @objc func previousWindow() { cycleWindow(-1) }
    private func cycleWindow(_ step: Int) {
        let windows = core.model.windows
        guard !windows.isEmpty else { return }
        let at = windows.firstIndex { $0.id == core.model.key } ?? 0
        let next = windows[((at + step) % windows.count + windows.count) % windows.count]
        request(["cmd": "focus", "args": ["window": .string(next.id)]])
    }

    /// Mux actions from Ghostty keybindings that no menu item took.
    private func ghosttyAction(_ paneID: String, _ action: ghostty_action_s) -> Bool {
        let perform: (() -> Void)?
        switch action.tag {
        case GHOSTTY_ACTION_NEW_SPLIT:
            let direction = action.action.new_split
            perform = direction == GHOSTTY_SPLIT_DIRECTION_DOWN || direction == GHOSTTY_SPLIT_DIRECTION_UP ? splitDown : splitRight
        case GHOSTTY_ACTION_GOTO_SPLIT:
            let target = action.action.goto_split
            perform = target == GHOSTTY_GOTO_SPLIT_PREVIOUS || target == GHOSTTY_GOTO_SPLIT_LEFT || target == GHOSTTY_GOTO_SPLIT_UP ? previousPane : nextPane
        case GHOSTTY_ACTION_TOGGLE_SPLIT_ZOOM: perform = toggleZoom
        case GHOSTTY_ACTION_NEW_TAB: perform = newTab
        case GHOSTTY_ACTION_GOTO_TAB:
            let target = action.action.goto_tab
            perform = target == GHOSTTY_GOTO_TAB_PREVIOUS ? previousTab : target == GHOSTTY_GOTO_TAB_NEXT ? nextTab : nil
        case GHOSTTY_ACTION_NEW_WINDOW: perform = newWindow
        case GHOSTTY_ACTION_CLOSE_WINDOW: perform = closeWindow
        case GHOSTTY_ACTION_GOTO_WINDOW:
            perform = action.action.goto_window == GHOSTTY_GOTO_WINDOW_PREVIOUS ? previousWindow : nextWindow
        default: perform = nil
        }
        guard let perform else { return false }
        focused(paneID)
        perform()
        return true
    }

    private func snapshot(_ args: JSON) throws -> [String: JSON] {
        let window = try core.targetWindow(args["window"])
        guard let controller = controllers[window.id] else { throw KmuxError("not_found", "no window \"\(window.id)\"") }
        controller.window.displayIfNeeded()
        let panes = Model.paneIDs(window.activeTab?.root).compactMap { host.views[$0] }.filter { $0.window != nil }
        return try Snapshot.run(controller, panes: panes, path: args["path"]?.string, marker: args["marker"]?.string)
    }

    private func installMenu() {
        let runtime = host.runtime
        let main = NSMenu()
        func menu(_ title: String, _ items: [(String, Selector, String?, Shortcut?)?]) -> NSMenu {
            let item = NSMenuItem()
            let submenu = NSMenu(title: title)
            for entry in items {
                guard let (label, action, ghostty, fallback) = entry else {
                    submenu.addItem(.separator())
                    continue
                }
                let shortcut = ghostty.flatMap { runtime.shortcut(for: $0) } ?? fallback
                let menuItem = submenu.addItem(withTitle: label, action: action, keyEquivalent: shortcut?.key ?? "")
                menuItem.keyEquivalentModifierMask = shortcut?.modifiers ?? []
                menuItem.target = self
            }
            item.submenu = submenu
            main.addItem(item)
            return submenu
        }
        let app = menu("kmux", [])
        app.addItem(withTitle: "Quit kmux", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        _ = menu("Pane", [
            ("Split Right", #selector(splitRight), "new_split:right", .cmd("d")),
            ("Split Down", #selector(splitDown), "new_split:down", .shiftCmd("d")),
            nil,
            ("Next Pane", #selector(nextPane), "goto_split:next", .cmd("]")),
            ("Previous Pane", #selector(previousPane), "goto_split:previous", .cmd("[")),
            ("Zoom", #selector(toggleZoom), "toggle_split_zoom", .shiftCmd("\r")),
            nil,
            ("Restart", #selector(restartPane), nil, .cmd("r")),
            ("Close Pane", #selector(closePane), "close_surface", .cmd("w")),
        ])
        _ = menu("View", [
            ("New Tab", #selector(newTab as () -> Void), "new_tab", .cmd("t")),
            ("Next Tab", #selector(nextTab), "next_tab", .shiftCmd("]")),
            ("Previous Tab", #selector(previousTab), "previous_tab", .shiftCmd("[")),
        ])
        let windowMenu = menu("Window", [
            ("New Window", #selector(newWindow), "new_window", .cmd("n")),
            ("Close Window", #selector(closeWindow), "close_window", .shiftCmd("w")),
            nil,
            ("Next Window", #selector(nextWindow), nil, .cmd("`")),
            ("Previous Window", #selector(previousWindow), nil, .shiftCmd("`")),
            nil,
        ])
        NSApp.mainMenu = main
        NSApp.windowsMenu = windowMenu
    }

    /// `debug.key`: delivers a key press through AppKit's normal event path,
    /// as if typed into the key window (e.g. "cmd+shift+d").
    private func pressKey(_ args: JSON) throws -> [String: JSON] {
        guard let name = args["key"]?.string else { throw KmuxError("bad_request", "debug.key needs a key") }
        let window = core.model.key.flatMap { controllers[$0]?.window }
        guard let event = SyntheticKey.event(name, window: window) else { throw KmuxError("bad_request", "unknown key \"\(name)\"") }
        NSApp.sendEvent(event)
        return ["characters": .string(event.characters ?? ""), "charactersIgnoringModifiers": .string(event.charactersIgnoringModifiers ?? "")]
    }

    private func fatal(_ message: String) -> Never {
        FileHandle.standardError.write(Data((message + "\n").utf8))
        exit(1)
    }
}
