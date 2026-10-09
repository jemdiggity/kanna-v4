// Parts follow kmux apps/kmux/Sources/Kmux/AppController.swift (at kmux.rev): --bg and
// `present`, the socket and signal handling, the Edit menu, and the debug
// commands' shapes. Kanna's app is otherwise its own.
import AppKit
import GhosttyKit

/// One window showing the panes of Kanna's daemon (its first window's
/// active tab, for M0), drawn by Ghostty in external-I/O mode. The daemon
/// passes `debug.*` requests to it. With `KANNA_ECHO=1` it runs alone
/// instead: one terminal that echoes typing, and a private debug socket.
@MainActor
final class AppController: NSObject, NSApplicationDelegate {
    private var runtime: GhosttyRuntime!
    private var terminals: ExternalIOBackend!
    private let commands = Commands()
    private var daemon: DaemonLink?
    private var server: SocketServer?
    private var signalSources: [DispatchSourceSignal] = []
    /// Started with `--bg` (or `KANNA_BG=1`), e.g. by tests: don't take over the screen.
    private let background = CommandLine.arguments.contains("--bg") || ProcessInfo.processInfo.environment["KANNA_BG"] == "1"
    private let echo = ProcessInfo.processInfo.environment["KANNA_ECHO"] == "1"
    private let debugCommands = ProcessInfo.processInfo.environment["KANNA_DEBUG"] == "1"
    private var window: NSWindow!
    private var layout: LayoutView!
    private var focused: String?

    /// The daemon's socket: `$KANNA_SOCKET`, else the development daemon's,
    /// `kanna-dev/kanna.sock` in Application Support (never `kanna/kanna.sock`,
    /// the Kanna.app you use).
    static var daemonSocketPath: String {
        if let path = ProcessInfo.processInfo.environment["KANNA_SOCKET"], !path.isEmpty { return path }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("kanna-dev/kanna.sock").path
    }

    static var appSocketPath: String {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("kanna/kanna.sock").path
    }

    static func isAppSocket(_ path: String) -> Bool {
        (path as NSString).standardizingPath.lowercased() == (appSocketPath as NSString).standardizingPath.lowercased()
    }

    /// Echo mode's debug socket: `$KANNA_APP_SOCKET`, else `kanna-app.sock`
    /// beside the development daemon's.
    static var echoSocketPath: String {
        if let path = ProcessInfo.processInfo.environment["KANNA_APP_SOCKET"], !path.isEmpty { return path }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("kanna-dev/kanna-app.sock").path
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        do {
            runtime = try GhosttyRuntime()
        } catch {
            fatal("kanna: \(error.localizedDescription)")
        }
        installCommands()
        if echo {
            terminals = ExternalIOBackend(runtime: runtime, link: EchoLink())
            let server = SocketServer(path: Self.echoSocketPath) { [weak self] request in await self?.commands.handle(request) ?? nil }
            do {
                try server.start()
                self.server = server
            } catch let error as KannaError {
                fatal("kanna: \(error.message)")
            } catch {
                fatal("kanna: \(error)")
            }
        } else {
            if Self.isAppSocket(Self.daemonSocketPath) { fatal("kanna: \(Self.daemonSocketPath) belongs to Kanna.app; use another KANNA_SOCKET") }
            let daemon = DaemonLink(socketPath: Self.daemonSocketPath, commands: commands)
            daemon.onLayout = { [weak self] in self?.mirror($0) }
            terminals = ExternalIOBackend(runtime: runtime, link: daemon)
            self.daemon = daemon
        }
        // Quit cleanly (removing the socket) when killed, not only on ⌘Q.
        for signal in [SIGTERM, SIGINT, SIGHUP] {
            Darwin.signal(signal, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: signal, queue: .main)
            source.setEventHandler { NSApp.terminate(nil) }
            source.resume()
            signalSources.append(source)
        }
        installMenu()
        if !background { NSApp.activate() }
        openWindow()
        if echo { mirror(["root": ["pane": "p1", "type": "term"], "focused": "p1"]) }
        daemon?.start()
    }

    func applicationWillTerminate(_ notification: Notification) {
        for case let terminal as TerminalSurfaceView in layout?.panes.values.map({ $0 }) ?? [] { terminals.stop(terminal) }
        daemon?.stop()
        server?.stop()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    private func openWindow() {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 600), styleMask: [.titled, .closable, .miniaturizable, .resizable],
                          backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.title = "Kanna"
        layout = LayoutView(frame: NSRect(origin: .zero, size: window.contentLayoutRect.size))
        layout.autoresizingMask = [.width, .height]
        layout.make = { id, type in type == "term" ? TerminalSurfaceView(frame: .zero) : PlaceholderView(pane: id, type: type) }
        layout.added = { [weak self] id, view in
            guard let self, let terminal = view as? TerminalSurfaceView else { return }
            if !self.terminals.start(terminal, pane: Pane(id: id)) {
                FileHandle.standardError.write(Data("kanna: the terminal surface for \(id) could not be created\n".utf8))
            }
        }
        layout.removed = { [weak self] _, view in
            if let terminal = view as? TerminalSurfaceView { self?.terminals.stop(terminal) }
        }
        window.contentView = layout
        window.center()
        present(window)
    }

    /// Shows the layout the daemon sent: `root`, the tree, and `focused`.
    private func mirror(_ args: JSON) {
        layout.show(args["root"] ?? .null)
        let focus = args["focused"]?.string
        if focus != focused, let focus, let view = layout.panes[focus] as? TerminalSurfaceView {
            window.makeFirstResponder(view)
        }
        focused = focus
    }

    /// Brings a window forward, except in a `--bg` Kanna that isn't the app in
    /// use: there the window goes just behind the front window of the app
    /// that is, so tests driving Kanna don't interrupt the user.
    private func present(_ window: NSWindow) {
        if NSApp.isActive || !background {
            return window.makeKeyAndOrderFront(nil)
        }
        window.makeKey()
        let pid = NSWorkspace.shared.frontmostApplication?.processIdentifier
        let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
        let front = windows.first { ($0[kCGWindowOwnerPID as String] as? pid_t) == pid && ($0[kCGWindowLayer as String] as? Int) == 0 }
        if let number = front?[kCGWindowNumber as String] as? Int, pid != ProcessInfo.processInfo.processIdentifier {
            window.order(.below, relativeTo: number)
        } else {
            window.orderFront(nil)
        }
    }

    // MARK: Debug commands

    private func installCommands() {
        guard debugCommands else { return }
        // The app is up: who it is, what it's attached to, what it shows.
        commands["debug.app"] = { [weak self] _ in
            var reply: [String: JSON] = [
                "app": "kanna", "pid": .number(Double(ProcessInfo.processInfo.processIdentifier)),
                "bundle": .string(Bundle.main.bundleIdentifier ?? ""), "panes": .array(self?.layout?.order.map { .string($0) } ?? []),
            ]
            if let daemon = self?.daemon {
                reply["daemon"] = .string(daemon.socketPath)
                reply["attached"] = .bool(daemon.attached)
            } else {
                reply["socket"] = .string(Self.echoSocketPath)
            }
            return reply
        }
        // Types `text` into a pane (focusing it) as key presses, through
        // AppKit's event path and the terminal's own key and text input handling.
        commands["debug.type"] = { [weak self] args in
            guard let self else { return [:] }
            let view = try self.terminal(args["pane"]?.string)
            guard let text = args["text"]?.string, let window = view.window else { throw KannaError("bad_request", "debug.type needs text") }
            if window.firstResponder !== view { window.makeFirstResponder(view) }
            let events = SyntheticKey.typing(text, window: window)
            for event in events { window.sendEvent(event) }
            return ["keys": .number(Double(events.count / 2))]
        }
        // `key`: one key press with modifiers, e.g. "ctrl+c", as kmux's debug.key.
        commands["debug.key"] = { [weak self] args in
            guard let self else { return [:] }
            let view = try self.terminal(args["pane"]?.string)
            guard let name = args["key"]?.string else { throw KannaError("bad_request", "debug.key needs a key") }
            guard let event = SyntheticKey.event(name, window: view.window) else { throw KannaError("bad_request", "unknown key \"\(name)\"") }
            if view.window?.firstResponder !== view { view.window?.makeFirstResponder(view) }
            view.window?.sendEvent(event)
            return ["characters": .string(event.characters ?? ""), "charactersIgnoringModifiers": .string(event.charactersIgnoringModifiers ?? "")]
        }
        // The terminal's screen as the app draws it, one line per row.
        commands["debug.text"] = { [weak self] args in
            guard let self else { return [:] }
            return ["text": .string(try self.terminal(args["pane"]?.string).viewportText())]
        }
        // The window as composited, with per-pane ink and an optional marker
        // check (see Snapshot); `path` also saves it as a PNG.
        commands["debug.snapshot"] = { [weak self] args in
            guard let self, let window = self.window, let layout = self.layout else { throw KannaError("not_found", "no window") }
            window.displayIfNeeded()
            let panes = layout.order.compactMap { id in layout.panes[id].map { (id: id, view: $0) } }
            return try Snapshot.run(window, id: "w1", panes: panes, path: args["path"]?.string, marker: args["marker"]?.string)
        }
    }

    /// A pane's terminal view: by ID, else the focused one, else the first.
    private func terminal(_ id: String?) throws -> TerminalSurfaceView {
        let id = id ?? focused.flatMap { layout.panes[$0] is TerminalSurfaceView ? $0 : nil } ?? layout.order.first { layout.panes[$0] is TerminalSurfaceView }
        guard let id, let view = layout.panes[id] else { throw KannaError("not_found", "no pane \"\(id ?? "")\" in this window") }
        guard let terminal = view as? TerminalSurfaceView else { throw KannaError("wrong_type", "\(id) is not a terminal") }
        guard terminal.surface != nil else { throw KannaError("not_found", "\(id)'s terminal is not running") }
        return terminal
    }

    // MARK: Menu

    private func installMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        let appMenu = NSMenu(title: "Kanna")
        appMenu.addItem(withTitle: "Quit Kanna", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        main.addItem(appItem)
        // Sent to whatever has focus: the terminal runs Ghostty's copy and paste.
        let editItem = NSMenuItem()
        let edit = NSMenu(title: "Edit")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Select All", action: #selector(NSResponder.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = edit
        main.addItem(editItem)
        NSApp.mainMenu = main
    }

    private func fatal(_ message: String) -> Never {
        FileHandle.standardError.write(Data((message + "\n").utf8))
        exit(1)
    }
}
