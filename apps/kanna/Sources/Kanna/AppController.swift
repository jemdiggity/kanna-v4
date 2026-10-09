import AppKit
import GhosttyKit
import KannaMarkdown

@MainActor
final class AppController: NSObject, NSApplicationDelegate {
    private var runtime: GhosttyRuntime!
    private var terminals: ExternalIOBackend!
    private let commands = Commands()
    private var daemon: DaemonLink?
    private var server: SocketServer?
    private var signalSources: [DispatchSourceSignal] = []
    private let background = CommandLine.arguments.contains("--bg") || ProcessInfo.processInfo.environment["KANNA_BG"] == "1"
    private let echo = ProcessInfo.processInfo.environment["KANNA_ECHO"] == "1"
    private let debugCommands = ProcessInfo.processInfo.environment["KANNA_DEBUG"] == "1"
    private var window: NSWindow!
    private var layout: LayoutView!
    private var focused: String?
    private var wantsFirstTerminal = ProcessInfo.processInfo.environment["KANNA_NO_INITIAL_WINDOW"] == nil
    private var appLayout: AppLayout?
    private var saveTimer: Timer?
    private let contents = Contents()

    static var daemonSocketPath: String {
        if let path = ProcessInfo.processInfo.environment["KANNA_SOCKET"], !path.isEmpty { return path }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("kanna-dev/kanna.sock").path
    }

    static var statePath: String? {
        let environment = ProcessInfo.processInfo.environment
        if environment["KANNA_NO_STATE"] == "1" { return nil }
        let folder = environment["KANNA_STATE_DIR"].flatMap { $0.isEmpty ? nil : $0 } ?? (daemonSocketPath as NSString).deletingLastPathComponent
        return (folder as NSString).appendingPathComponent("layout.json")
    }

    static var fresh: Bool {
        CommandLine.arguments.contains("--fresh") || ProcessInfo.processInfo.environment["KANNA_FRESH"] == "1"
    }

    static var appSocketPath: String {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("kanna/kanna.sock").path
    }

    static func isAppSocket(_ path: String) -> Bool {
        (path as NSString).standardizingPath.lowercased() == (appSocketPath as NSString).standardizingPath.lowercased()
    }

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
            let daemon = DaemonLink(socketPath: Self.daemonSocketPath, commands: commands, starter: DaemonStarter(socketPath: Self.daemonSocketPath))
            appLayout = AppLayout(statePath: Self.statePath, fresh: Self.fresh)
            daemon.onAttach = { [weak self] in self?.attached($0) }
            daemon.onLayout = { [weak self] in self?.layoutRequest($0) ?? ["ok": false, "error": ["code": "app_gone", "message": "Kanna.app is closing"]] }
            daemon.helloPanes = { [weak self] in self?.helloPanes() ?? [] }
            daemon.onStart = { [weak self] pane in self?.startContent(pane) }
            daemon.onStop = { [weak self] pane in self?.contents.stop(pane) }
            daemon.onPane = { [weak self] pane in self?.learn(pane) }
            contents.onUpdate = { [weak daemon] pane, fields in daemon?.update(pane, fields) }
            contents.onRequest = { [weak daemon] cmd, args in daemon?.request(cmd, args) }
            contents.onFocus = { [weak self] pane in self?.userFocused(pane) }
            terminals = ExternalIOBackend(runtime: runtime, link: daemon)
            self.daemon = daemon
        }
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
        saveTimer?.invalidate()
        appLayout?.save()
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
        layout.make = { [weak self] id, type in
            guard type == "term" else { return self?.contents.slot(id) ?? PlaceholderView(pane: id, type: type) }
            let view = TerminalSurfaceView(frame: .zero)
            view.focused = { self?.userFocused(id) }
            return view
        }
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

    private func attached(_ panes: JSON) {
        guard let appLayout else { return }
        let reply = appLayout.attach(panes)
        if let problem = reply["problem"]?.string { FileHandle.standardError.write(Data("kanna: \(problem)\n".utf8)) }
        if reply["ok"]?.bool != true {
            FileHandle.standardError.write(Data("kanna: the daemon's panes could not be placed: \(reply["error"]?["message"]?.string ?? "")\n".utf8))
        }
        if case .array(let list) = panes {
            for pane in list where pane["type"]?.string != "term" {
                guard let id = pane["id"]?.string else { continue }
                if !contents.shows(id) { contents.start(pane, restoring: true) }
            }
        }
        layoutChanged()
        if wantsFirstTerminal {
            wantsFirstTerminal = false
            openTerminalIfEmpty()
        }
    }

    private var isEmpty: Bool {
        guard let appLayout, case .array(let panes)? = appLayout.call(["call": "export"])["panes"] else { return false }
        return panes.isEmpty
    }

    private func openTerminalIfEmpty() {
        guard isEmpty, daemon?.attached == true else { return }
        daemon?.request("open", ["type": "term"])
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        openTerminalIfEmpty()
        return true
    }

    private func helloPanes() -> JSON {
        guard let appLayout, case .array(let panes)? = appLayout.call(["call": "hello"])["panes"] else { return [] }
        return .array(panes.map { pane in
            guard case .object(var fields) = pane, let id = pane["id"]?.string else { return pane }
            fields["state"] = .string(contents.shows(id) ? contents.states[id] ?? "starting" : "starting")
            return .object(fields)
        })
    }

    private func startContent(_ pane: JSON) {
        learn(pane)
        contents.start(pane)
        layoutChanged()
    }

    private func learn(_ pane: JSON) {
        _ = appLayout?.call(["call": "pane", "pane": pane])
        scheduleSave()
    }

    private func pruneContents() {
        guard let appLayout, case .array(let panes)? = appLayout.call(["call": "export"])["panes"] else { return }
        let known = Set(panes.compactMap { $0["id"]?.string })
        for id in contents.ids where !known.contains(id) { contents.forget(id) }
    }

    private func layoutRequest(_ request: JSON) -> JSON {
        guard let appLayout else { return ["ok": false, "error": ["code": "no_layout", "message": "this app keeps no layout"]] }
        let reply = appLayout.serve(request)
        layoutChanged()
        return reply
    }

    private func userFocused(_ pane: String) {
        guard let appLayout, pane != focused else { return }
        focused = pane
        _ = appLayout.serve(["op": "command", "cmd": "focus", "args": ["pane": .string(pane)]])
        scheduleSave()
    }

    private func layoutChanged() {
        guard let appLayout else { return }
        mirror(appLayout.view)
        pruneContents()
        scheduleSave()
    }

    private func scheduleSave() {
        guard appLayout != nil, saveTimer?.isValid != true else { return }
        saveTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.appLayout?.save() }
        }
    }

    private func mirror(_ args: JSON) {
        layout.show(args["root"] ?? .null)
        let focus = args["focused"]?.string
        if focus != focused, let focus, let view = (layout.panes[focus] as? TerminalSurfaceView).map({ $0 as NSView }) ?? contents.keyView(focus) {
            window.makeFirstResponder(view)
        }
        focused = focus
    }

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

    private func installCommands() {
        guard debugCommands else { return }
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
        commands["debug.type"] = { [weak self] args in
            guard let self else { return [:] }
            let view = try self.terminal(args["pane"]?.string)
            guard let text = args["text"]?.string, let window = view.window else { throw KannaError("bad_request", "debug.type needs text") }
            if window.firstResponder !== view { window.makeFirstResponder(view) }
            let events = SyntheticKey.typing(text, window: window)
            for event in events { window.sendEvent(event) }
            return ["keys": .number(Double(events.count / 2))]
        }
        commands["debug.key"] = { [weak self] args in
            guard let self else { return [:] }
            let view = try self.terminal(args["pane"]?.string)
            guard let name = args["key"]?.string else { throw KannaError("bad_request", "debug.key needs a key") }
            guard let event = SyntheticKey.event(name, window: view.window) else { throw KannaError("bad_request", "unknown key \"\(name)\"") }
            if view.window?.firstResponder !== view { view.window?.makeFirstResponder(view) }
            view.window?.sendEvent(event)
            return ["characters": .string(event.characters ?? ""), "charactersIgnoringModifiers": .string(event.charactersIgnoringModifiers ?? "")]
        }
        commands["debug.text"] = { [weak self] args in
            guard let self else { return [:] }
            return ["text": .string(try self.terminal(args["pane"]?.string).viewportText())]
        }
        commands["debug.snapshot"] = { [weak self] args in
            guard let self, let window = self.window, let layout = self.layout else { throw KannaError("not_found", "no window") }
            window.displayIfNeeded()
            let panes = layout.order.compactMap { id in layout.panes[id].map { (id: id, view: $0) } }
            return try Snapshot.run(window, id: "w1", panes: panes, path: args["path"]?.string, marker: args["marker"]?.string)
        }
        commands["debug.web"] = { [weak self] args in
            guard let self, let id = args["pane"]?.string, let web = self.contents.web(id) else { throw KannaError("wrong_type", "not a web pane") }
            let text = try? await web.webView.evaluateJavaScript("document.body ? document.body.innerText : ''") as? String
            return ["url": web.webView.url.map { .string($0.absoluteString) } ?? .null, "text": .string(text ?? "")]
        }
        commands["debug.md"] = { [weak self] args in
            guard let self, let id = args["pane"]?.string, let markdown = self.contents.markdown(id) else { throw KannaError("wrong_type", "not a running md pane") }
            if let y = args["scroll"]?.number { markdown.scroll(toY: CGFloat(y)) }
            let diagrams: [JSON] = markdown.drawnDiagrams.map { ["type": .string($0.type), "labels": .array($0.labels.map { .string($0) })] }
            return ["text": .string(markdown.text), "zoom": .number(Double(markdown.zoom)), "diagrams": .array(diagrams),
                    "render_ms": .number(markdown.renderMs), "parse_ms": .number(markdown.parseMs), "scroll": .number(Double(markdown.scrollTop)),
                    "layout_width": .number(Double(markdown.layoutWidth))]
        }
        commands["debug.ios"] = { [weak self] args in
            guard let self, let id = args["pane"]?.string, let screen = self.contents.ios(id)?.screen else { throw KannaError("wrong_type", "not a running ios pane") }
            if let x = args["x"]?.number, let y = args["y"]?.number {
                screen.touch(.leftMouseDown, at: CGPoint(x: x, y: y))
                try await Task.sleep(for: .milliseconds(80))
                screen.touch(.leftMouseUp, at: CGPoint(x: x, y: y))
            }
            if args["home"]?.bool == true { screen.pressHome() }
            return ["width": .number(screen.pixelSize.width), "height": .number(screen.pixelSize.height)]
        }
    }

    @objc func openURL() {
        guard let focused else { return }
        contents.web(focused)?.editURL()
    }

    @objc func goBack() { history(back: true) }
    @objc func goForward() { history(back: false) }
    private func history(back: Bool) {
        guard let focused, contents.web(focused) != nil || contents.markdown(focused) != nil else { return }
        daemon?.request("navigate", ["pane": .string(focused), back ? "back" : "forward": true])
    }

    @objc func zoomIn() { contents.markdown(focused ?? "")?.zoom(by: 1) }
    @objc func zoomOut() { contents.markdown(focused ?? "")?.zoom(by: -1) }
    @objc func actualSize() { contents.markdown(focused ?? "")?.zoom(by: 0) }
    @objc func pressHome() { contents.ios(focused ?? "")?.screen?.pressHome() }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        let pane = focused ?? ""
        switch item.action {
        case #selector(openURL): return contents.web(pane) != nil
        case #selector(goBack), #selector(goForward): return contents.web(pane) != nil || contents.markdown(pane) != nil
        case #selector(zoomIn), #selector(zoomOut), #selector(actualSize): return contents.markdown(pane) != nil
        case #selector(pressHome): return contents.ios(pane)?.screen != nil
        default: return true
        }
    }

    private func terminal(_ id: String?) throws -> TerminalSurfaceView {
        let id = id ?? focused.flatMap { layout.panes[$0] is TerminalSurfaceView ? $0 : nil } ?? layout.order.first { layout.panes[$0] is TerminalSurfaceView }
        guard let id, let view = layout.panes[id] else { throw KannaError("not_found", "no pane \"\(id ?? "")\" in this window") }
        guard let terminal = view as? TerminalSurfaceView else { throw KannaError("wrong_type", "\(id) is not a terminal") }
        guard terminal.surface != nil else { throw KannaError("not_found", "\(id)'s terminal is not running") }
        return terminal
    }

    private func installMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        let appMenu = NSMenu(title: "Kanna")
        appMenu.addItem(withTitle: "Quit Kanna", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        main.addItem(appItem)
        let editItem = NSMenuItem()
        let edit = NSMenu(title: "Edit")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Select All", action: #selector(NSResponder.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = edit
        main.addItem(editItem)
        let viewItem = NSMenuItem()
        let view = NSMenu(title: "View")
        for (title, action, key, modifiers) in [
            ("Open URL…", #selector(openURL), "l", NSEvent.ModifierFlags.command), ("Back", #selector(goBack), "[", .command),
            ("Forward", #selector(goForward), "]", .command), ("Zoom In", #selector(zoomIn), "+", .command),
            ("Zoom Out", #selector(zoomOut), "-", .command), ("Actual Size", #selector(actualSize), "0", .command),
            ("Home", #selector(pressHome), "h", [.command, .shift]),
        ] {
            let item = view.addItem(withTitle: title, action: action, keyEquivalent: key)
            item.keyEquivalentModifierMask = modifiers
            item.target = self
        }
        viewItem.submenu = view
        main.addItem(viewItem)
        let shellItem = NSMenuItem()
        let shell = NSMenu(title: "Shell")
        for (title, action, key, modifiers) in [
            ("Split Right", #selector(splitRight), "d", NSEvent.ModifierFlags.command), ("Split Down", #selector(splitDown), "d", [.command, .shift]),
            ("Close Pane", #selector(closePane), "w", .command),
        ] {
            let item = shell.addItem(withTitle: title, action: action, keyEquivalent: key)
            item.keyEquivalentModifierMask = modifiers
            item.target = self
        }
        shellItem.submenu = shell
        main.insertItem(shellItem, at: 1)
        NSApp.mainMenu = main
    }

    @objc private func splitRight() { split("right") }
    @objc private func splitDown() { split("down") }

    private func split(_ direction: String) {
        guard daemon?.attached == true else { return }
        if isEmpty {
            daemon?.request("open", ["type": "term"])
            return
        }
        daemon?.request("open", ["type": "term", "split": .string(direction)])
    }

    @objc private func closePane() {
        guard let focused, daemon?.attached == true else { return }
        daemon?.request("close", ["pane": .string(focused)])
    }

    private func fatal(_ message: String) -> Never {
        FileHandle.standardError.write(Data((message + "\n").utf8))
        exit(1)
    }
}
