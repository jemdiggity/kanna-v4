import AppKit
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

    @objc func newWindow() { request(["cmd": "open", "args": ["type": "term", "window": "new"]]) }

    @objc func closePane() {
        guard let focused = core.model.keyWindow?.focused else { return }
        request(["cmd": "close", "args": ["pane": .string(focused)]])
    }

    private func snapshot(_ args: JSON) throws -> [String: JSON] {
        let window = try core.targetWindow(args["window"])
        guard let controller = controllers[window.id] else { throw KmuxError("not_found", "no window \"\(window.id)\"") }
        controller.window.displayIfNeeded()
        let panes = Model.paneIDs(window.activeTab?.root).compactMap { host.views[$0] }.filter { $0.window != nil }
        return try Snapshot.run(controller, panes: panes, path: args["path"]?.string)
    }

    private func installMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        appItem.submenu = NSMenu(title: "kmux")
        appItem.submenu?.addItem(withTitle: "Quit kmux", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        main.addItem(appItem)

        let shell = NSMenuItem()
        shell.submenu = NSMenu(title: "Shell")
        shell.submenu?.addItem(withTitle: "New Window", action: #selector(newWindow), keyEquivalent: "n").target = self
        shell.submenu?.addItem(withTitle: "Close Pane", action: #selector(closePane), keyEquivalent: "w").target = self
        main.addItem(shell)
        NSApp.mainMenu = main
    }

    private func fatal(_ message: String) -> Never {
        FileHandle.standardError.write(Data((message + "\n").utf8))
        exit(1)
    }
}
