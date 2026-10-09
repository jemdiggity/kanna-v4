// Parts follow kmux apps/kmux/Sources/Kmux/AppController.swift (at kmux.rev): --bg and
// `present`, the socket and signal handling, the Edit menu, and the debug
// commands' shapes. Kanna's app is otherwise its own.
import AppKit
import GhosttyKit

/// One window with one terminal pane, drawn by Ghostty in external-I/O mode,
/// and a private debug socket for tests.
@MainActor
final class AppController: NSObject, NSApplicationDelegate {
    private var runtime: GhosttyRuntime!
    private var terminals: ExternalIOBackend!
    private let commands = Commands()
    private var server: SocketServer?
    private var signalSources: [DispatchSourceSignal] = []
    /// Started with `--bg` (or `KANNA_BG=1`), e.g. by tests: don't take over the screen.
    private let background = CommandLine.arguments.contains("--bg") || ProcessInfo.processInfo.environment["KANNA_BG"] == "1"
    private var window: NSWindow!
    private let pane = Pane(id: "p1")
    private var terminal: TerminalSurfaceView!

    /// The debug socket: `$KANNA_APP_SOCKET`, else `kanna-app.sock` in
    /// `~/Library/Application Support/kanna` (beside, never on, `kanna.sock`).
    /// The daemon takes this role over in step 0.4.
    static var socketPath: String {
        if let path = ProcessInfo.processInfo.environment["KANNA_APP_SOCKET"], !path.isEmpty { return path }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("kanna/kanna-app.sock").path
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        do {
            runtime = try GhosttyRuntime()
        } catch {
            fatal("kanna: \(error.localizedDescription)")
        }
        terminals = ExternalIOBackend(runtime: runtime, link: EchoLink())
        installCommands()
        let server = SocketServer(path: Self.socketPath) { [weak self] request in await self?.commands.handle(request) ?? nil }
        do {
            try server.start()
            self.server = server
        } catch let error as KannaError {
            fatal("kanna: \(error.message)")
        } catch {
            fatal("kanna: \(error)")
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
    }

    func applicationWillTerminate(_ notification: Notification) {
        if let terminal { terminals.stop(terminal) }
        server?.stop()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    private func openWindow() {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 600), styleMask: [.titled, .closable, .miniaturizable, .resizable],
                          backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.title = "Kanna"
        let size = window.contentLayoutRect.size
        terminal = TerminalSurfaceView(frame: NSRect(origin: .zero, size: size))
        terminal.autoresizingMask = [.width, .height]
        window.contentView = terminal
        window.center()
        guard terminals.start(terminal, pane: pane) else { fatal("kanna: the terminal surface could not be created") }
        window.makeFirstResponder(terminal)
        present(window)
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
        // The app is up: who it is and where it listens.
        commands["debug.app"] = { [weak self] _ in
            ["app": "kanna", "pid": .number(Double(ProcessInfo.processInfo.processIdentifier)),
             "bundle": .string(Bundle.main.bundleIdentifier ?? ""), "socket": .string(Self.socketPath),
             "panes": .array(self.map { [.string($0.pane.id)] } ?? [])]
        }
        // Types `text` into the pane's window as key presses, through AppKit's
        // event path and the terminal's own key and text input handling.
        commands["debug.type"] = { [weak self] args in
            guard let self else { return [:] }
            let view = try self.terminal(args["pane"]?.string)
            guard let text = args["text"]?.string, let window = view.window else { throw KannaError("bad_request", "debug.type needs text") }
            let events = SyntheticKey.typing(text, window: window)
            for event in events { window.sendEvent(event) }
            return ["keys": .number(Double(events.count / 2))]
        }
        // `key`: one key press with modifiers, e.g. "ctrl+c", as kmux's debug.key.
        commands["debug.key"] = { [weak self] args in
            guard let self else { return [:] }
            let window = try self.terminal(args["pane"]?.string).window
            guard let name = args["key"]?.string else { throw KannaError("bad_request", "debug.key needs a key") }
            guard let event = SyntheticKey.event(name, window: window) else { throw KannaError("bad_request", "unknown key \"\(name)\"") }
            window?.sendEvent(event)
            return ["characters": .string(event.characters ?? ""), "charactersIgnoringModifiers": .string(event.charactersIgnoringModifiers ?? "")]
        }
        // The terminal's screen as text, one line per row.
        commands["debug.text"] = { [weak self] args in
            guard let self else { return [:] }
            return ["text": .string(try self.terminal(args["pane"]?.string).viewportText())]
        }
        // The window as composited, with per-pane ink and an optional marker
        // check (see Snapshot); `path` also saves it as a PNG.
        commands["debug.snapshot"] = { [weak self] args in
            guard let self, let window = self.window else { throw KannaError("not_found", "no window") }
            window.displayIfNeeded()
            return try Snapshot.run(window, id: "w1", panes: [(self.pane.id, self.terminal)], path: args["path"]?.string, marker: args["marker"]?.string)
        }
    }

    private func terminal(_ id: String?) throws -> TerminalSurfaceView {
        guard id == nil || id == pane.id else { throw KannaError("not_found", "no pane \"\(id ?? "")\"") }
        guard let terminal, terminal.surface != nil else { throw KannaError("not_found", "the terminal is not running") }
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
