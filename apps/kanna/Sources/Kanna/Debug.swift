import AppKit
import GhosttyKit

extension AppController {
    func installCommands() {
        guard debugCommands else { return }
        TerminalDebug.install(commands, runtime: runtime) { [weak self] id in
            guard let self else { throw KannaError("app_gone", "Kanna.app is closing") }
            return try self.terminal(id)
        }
        commands["debug.app"] = { [weak self] args in
            guard let self else { return [:] }
            switch args["appearance"]?.string {
            case "dark": NSApp.appearance = NSAppearance(named: .darkAqua)
            case "light": NSApp.appearance = NSAppearance(named: .aqua)
            default: break
            }
            var reply: [String: JSON] = [
                "app": "kanna", "pid": .number(Double(ProcessInfo.processInfo.processIdentifier)),
                "bundle": .string(Bundle.main.bundleIdentifier ?? ""),
                "panes": .array(keyWindow.map { WindowController.paneIDs($0["root"] ?? .null).map { .string($0) } } ?? []),
                "windows": .array(controllers.keys.sorted().map { .string($0) }),
            ]
            if let daemon {
                reply["daemon"] = .string(daemon.socketPath)
                reply["attached"] = .bool(daemon.attached)
            } else {
                reply["socket"] = .string(Self.echoSocketPath)
            }
            return reply
        }
        commands["debug.type"] = { [weak self] args in
            guard let self else { return [:] }
            if args["focused"]?.bool == true {
                guard let text = args["text"]?.string, let window = state["key"]?.string.flatMap({ controllers[$0]?.window }) else {
                    throw KannaError("bad_request", "debug.type needs text and a key window")
                }
                let events = SyntheticKey.typing(text, window: window)
                for event in events { window.sendEvent(event) }
                return ["keys": .number(Double(events.count / 2)), "responder": .string(window.firstResponder.map { String(describing: type(of: $0)) } ?? "")]
            }
            let view = try terminal(args["pane"]?.string)
            guard let text = args["text"]?.string, let window = view.window else { throw KannaError("bad_request", "debug.type needs text") }
            if window.firstResponder !== view { window.makeFirstResponder(view) }
            let events = SyntheticKey.typing(text, window: window)
            for event in events { window.sendEvent(event) }
            return ["keys": .number(Double(events.count / 2))]
        }
        commands["debug.key"] = { [weak self] args in try self?.pressKey(args) ?? [:] }
        commands["debug.click"] = { [weak self] args in try self?.click(args) ?? [:] }
        commands["debug.drag"] = { [weak self] args in try await self?.debugDrag(args) ?? [:] }
        commands["debug.menu"] = { [weak self] _ in ["items": .array(self?.menuItems ?? [])] }
        commands["debug.stats"] = { _ in
            ["pid": .number(Double(ProcessInfo.processInfo.processIdentifier)), "footprint": .number(Double(memoryFootprint()))]
        }
        commands["debug.drop"] = { [weak self] args in
            guard let self else { return [:] }
            let view = try terminal(args["pane"]?.string)
            let pasteboard = NSPasteboard(name: NSPasteboard.Name("dev.kanna.debug-drop.\(UUID().uuidString)"))
            defer { pasteboard.releaseGlobally() }
            pasteboard.clearContents()
            if case .array(let list)? = args["files"] {
                pasteboard.writeObjects(list.compactMap { $0.string.map { NSURL(fileURLWithPath: $0) } })
            } else if let image = args["image"]?.string {
                guard let data = FileManager.default.contents(atPath: image) else { throw KannaError("bad_request", "no such image: \(image)") }
                pasteboard.setData(data, forType: image.lowercased().hasSuffix(".png") ? .png : .tiff)
            } else if let text = args["text"]?.string {
                pasteboard.setString(text, forType: .string)
            } else {
                throw KannaError("bad_request", "debug.drop needs files, image or text")
            }
            return ["dropped": .bool(view.drop(pasteboard))]
        }
        commands["debug.pinch"] = { [weak self] args in try await self?.pinch(args) ?? [:] }
        commands["debug.text"] = { [weak self] args in
            guard let self else { return [:] }
            let view = try terminal(args["pane"]?.string)
            var reply: [String: JSON] = ["text": .string(view.viewportText())]
            if let surface = view.surface {
                let size = ghostty_surface_size(surface)
                reply["columns"] = .number(Double(size.columns))
                reply["rows"] = .number(Double(size.rows))
            }
            return reply
        }
        commands["debug.snapshot"] = { [weak self] args in
            guard let self else { return [:] }
            let id = args["window"]?.string ?? state["key"]?.string ?? controllers.keys.sorted().first
            guard let id, let controller = controllers[id], let window = window(id) else { throw KannaError("not_found", "no window \"\(args["window"]?.string ?? "")\"") }
            controller.window.displayIfNeeded()
            let shown: [(id: String, view: NSView)] = WindowController.shownPanes(window).compactMap { pane in
                guard let view = panes[pane], view.window != nil, !view.isHiddenOrHasHiddenAncestor, view.superview != nil else { return nil }
                return (pane, view.terminal ?? view)
            }
            var reply = try Snapshot.run(controller.window, id: id, panes: shown, tabBar: controller.tabBar, path: args["path"]?.string, marker: args["marker"]?.string)
            if case .array(let list)? = reply["panes"] {
                reply["panes"] = .array(list.map { entry in
                    guard case .object(var fields) = entry, let pane = entry["id"]?.string, let message = panes[pane]?.message else { return entry }
                    fields["notice"] = .string(message)
                    return .object(fields)
                })
            }
            reply["tabs"] = window["tabs"].map { tabs in
                guard case .array(let list) = tabs else { return tabs }
                return .array(list.compactMap { $0["title"] })
            } ?? []
            return reply
        }
        commands["debug.web"] = { [weak self] args in
            guard let self, let id = args["pane"]?.string, let web = contents.web(id) else { throw KannaError("wrong_type", "not a web pane") }
            let text = try? await web.webView.evaluateJavaScript("document.body ? document.body.innerText : ''") as? String
            return ["url": web.webView.url.map { .string($0.absoluteString) } ?? .null, "text": .string(text ?? ""), "zoom": .number(Double(web.zoom))]
        }
        commands["debug.md"] = { [weak self] args in
            guard let self, let id = args["pane"]?.string, let markdown = contents.markdown(id) else { throw KannaError("wrong_type", "not a running md pane") }
            if let y = args["scroll"]?.number { markdown.scroll(toY: CGFloat(y)) }
            if case .array(let pick)? = args["select"], pick.count == 2, let at = pick[0].number, let length = pick[1].number {
                markdown.select(NSRange(location: Int(at), length: Int(length)))
            }
            if let id = args["mark"]?.string {
                guard let rect = markdown.markRect(id) else { throw KannaError("not_found", "no mark for \(id)") }
                return ["x": .number(Double(rect.midX)), "y": .number(Double(markdown.bounds.height - rect.midY))]
            }
            let threads: [JSON] = markdown.threads.map { thread in
                ["id": .string(thread.id), "quote": .string(thread.quote), "line": .number(Double(thread.line)), "state": .string(thread.state),
                 "status": thread.status.map { .string($0) } ?? .null, "error": thread.error.map { .string($0) } ?? .null,
                 "messages": .array(thread.messages.map { .string("\($0.who): \($0.text)") })]
            }
            let diagrams: [JSON] = markdown.drawnDiagrams.map { ["type": .string($0.type), "labels": .array($0.labels.map { .string($0) })] }
            return ["text": .string(markdown.text), "zoom": .number(Double(markdown.zoom)), "diagrams": .array(diagrams),
                    "render_ms": .number(markdown.renderMs), "parse_ms": .number(markdown.parseMs), "scroll": .number(Double(markdown.scrollTop)),
                    "layout_width": .number(Double(markdown.layoutWidth)), "threads": .array(threads),
                    "box": markdown.commentBoxThread.map { .string($0) } ?? .null, "box_text": markdown.commentBoxText.map { .string($0) } ?? .null,
                    "active": markdown.activeThread.map { .string($0) } ?? .null, "agent": markdown.agent.map { .string($0) } ?? .null]
        }
        commands["debug.ios"] = { [weak self] args in
            guard let self, let id = args["pane"]?.string, let screen = contents.ios(id)?.screen else { throw KannaError("wrong_type", "not a running ios pane") }
            if let x = args["x"]?.number, let y = args["y"]?.number {
                screen.touch(.leftMouseDown, at: CGPoint(x: x, y: y))
                try await Task.sleep(for: .milliseconds(80))
                screen.touch(.leftMouseUp, at: CGPoint(x: x, y: y))
            }
            if args["home"]?.bool == true { screen.pressHome() }
            return ["width": .number(screen.pixelSize.width), "height": .number(screen.pixelSize.height)]
        }
    }

    private func terminal(_ reference: String?) throws -> TerminalSurfaceView {
        let shown = keyWindow.map { WindowController.shownPanes($0) } ?? []
        let id = try reference.map(resolve) ?? focusedPane.flatMap { panes.terminal($0) != nil ? $0 : nil } ?? shown.first { panes.terminal($0) != nil }
        guard let id, let view = panes[id] else { throw KannaError("not_found", "no pane \"\(reference ?? "")\" on screen") }
        guard let terminal = view.terminal else { throw KannaError("wrong_type", "\(id) is not a terminal") }
        guard terminal.surface != nil else { throw KannaError("not_found", "\(id)'s terminal is not running") }
        return terminal
    }

    private func pressKey(_ args: JSON) throws -> [String: JSON] {
        guard let name = args["key"]?.string else { throw KannaError("bad_request", "debug.key needs a key") }
        var window = state["key"]?.string.flatMap { controllers[$0]?.window }
        if let reference = args["pane"]?.string {
            let view = try terminal(reference)
            window = view.window
            if window?.firstResponder !== view { window?.makeFirstResponder(view) }
        }
        guard let window else { throw KannaError("not_found", "no window") }
        guard let event = SyntheticKey.event(name, window: window) else { throw KannaError("bad_request", "unknown key \"\(name)\"") }
        if !event.modifierFlags.intersection([.command, .control, .option]).isEmpty, window.performKeyEquivalent(with: event)
            || Self.performResponderItem(event, in: window) || NSApp.mainMenu?.performKeyEquivalent(with: event) == true {
            return ["characters": .string(event.characters ?? ""), "charactersIgnoringModifiers": .string(event.charactersIgnoringModifiers ?? ""), "menu": true]
        }
        window.sendEvent(event)
        return ["characters": .string(event.characters ?? ""), "charactersIgnoringModifiers": .string(event.charactersIgnoringModifiers ?? "")]
    }

    private static func performResponderItem(_ event: NSEvent, in window: NSWindow) -> Bool {
        func find(_ menu: NSMenu?) -> NSMenuItem? {
            for item in menu?.items ?? [] {
                if let found = find(item.submenu) { return found }
                let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
                if item.target == nil, item.action != nil, !item.keyEquivalent.isEmpty, item.keyEquivalent == event.charactersIgnoringModifiers,
                   item.keyEquivalentModifierMask.intersection([.command, .control, .option, .shift]) == modifiers { return item }
            }
            return nil
        }
        guard let item = find(NSApp.mainMenu), let action = item.action, let responder = window.firstResponder else { return false }
        return responder.tryToPerform(action, with: item)
    }

    private func click(_ args: JSON) throws -> [String: JSON] {
        let window: NSWindow, point: NSPoint
        if let reference = args["pane"]?.string {
            let id = try resolve(reference)
            guard let view = panes[id], let paneWindow = view.window else { throw KannaError("not_found", "pane \"\(id)\" is not on screen") }
            var at = [view.bounds.midX, view.bounds.midY]
            if case .array(let list)? = args["at"] { at = list.compactMap(\.number).map { CGFloat($0) } }
            guard at.count == 2 else { throw KannaError("bad_request", "at is [x, y]") }
            window = paneWindow
            point = view.convert(NSPoint(x: at[0], y: view.bounds.height - at[1]), to: nil)
        } else {
            let id = args["tab"]?.string ?? ""
            guard let state = windowOf(tab: id), let controller = state["id"]?.string.flatMap({ controllers[$0] }),
                  let center = controller.tabBar.center(of: id) else { throw KannaError("not_found", "no tab \"\(id)\" on screen") }
            window = controller.window
            point = center
        }
        if let button = args["button"]?.number {
            let type: NSEvent.EventType = button == 1 ? .rightMouseDown : .otherMouseDown
            guard let cg = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                                              windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)?.cgEvent
            else { throw KannaError("bad_request", "can't make that mouse event") }
            cg.setIntegerValueField(.mouseEventButtonNumber, value: Int64(button))
            guard let down = NSEvent(cgEvent: cg) else { throw KannaError("bad_request", "can't make that mouse event") }
            if button == 1 {
                var view = window.contentView?.hitTest(window.contentView?.convert(point, from: nil) ?? point)
                var menu: NSMenu?
                while let current = view, menu == nil {
                    menu = (current as? TerminalSurfaceView)?.contextMenu?() ?? current.menu(for: down)
                    view = current.superview
                }
                let items = (menu?.items ?? []).filter { !$0.isSeparatorItem }.map { JSON.string($0.title) }
                return ["menu": .array(items)]
            }
            window.sendEvent(down)
            return [:]
        }
        for count in 1...max(1, Int(args["clicks"]?.number ?? 1)) {
            func event(_ type: NSEvent.EventType) -> NSEvent? {
                NSEvent.mouseEvent(with: type, location: point, modifierFlags: args["command"]?.bool == true ? [.command] : [],
                                   timestamp: ProcessInfo.processInfo.systemUptime,
                                   windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: count, pressure: 1)
            }
            guard let down = event(.leftMouseDown), let up = event(.leftMouseUp) else { continue }
            NSApp.postEvent(up, atStart: false)
            if window.isKeyWindow {
                NSApp.sendEvent(down)
            } else if let content = window.contentView, let view = content.hitTest(content.convert(point, from: nil)) {
                if view.acceptsFirstResponder { window.makeFirstResponder(view) }
                view.mouseDown(with: down)
            }
        }
        return [:]
    }

    private func pinch(_ args: JSON) async throws -> [String: JSON] {
        let id = try resolve(args["pane"]?.string ?? focusedPane ?? "")
        guard let view = panes.keyView(id), let window = view.window else { throw KannaError("wrong_type", "pane is not on screen") }
        if args["outline"]?.bool == true { panes[id]?.focused = true }
        var steps = [0.1, 0.1, 0.1, 0.1, 0.1]
        if case .array(let list)? = args["steps"] { steps = list.compactMap(\.number) }
        let area = view.visibleRect.intersection(view.bounds)
        let point = view.convert(NSPoint(x: area.midX, y: area.midY), to: nil)
        let height = NSScreen.screens.first?.frame.height ?? 0
        let gesture = CGEventType(rawValue: 29)!
        let gestureKind = CGEventField(rawValue: 110)!
        let magnification = CGEventField(rawValue: 113)!
        let phaseField = CGEventField(rawValue: 132)!
        let zoomGesture: Int64 = 8
        var zooms: [JSON] = [], renders: [JSON] = []
        let phases = [(1, 0.0)] + steps.map { (2, $0) } + [(4, 0.0)]
        for (phase, value) in phases {
            guard let cg = CGEvent(source: nil) else { continue }
            cg.type = gesture
            cg.location = CGPoint(x: point.x, y: height - point.y)
            cg.setIntegerValueField(gestureKind, value: zoomGesture)
            cg.setDoubleValueField(magnification, value: value)
            cg.setIntegerValueField(phaseField, value: Int64(phase))
            guard let event = NSEvent(cgEvent: cg) else { continue }
            window.sendEvent(event)
            try await Task.sleep(for: .milliseconds(40))
            if phase == 2, let markdown = contents.markdown(id) {
                zooms.append(.number(Double(markdown.zoom)))
                renders.append(.number(markdown.renderMs))
            } else if phase == 2, let web = contents.web(id) {
                zooms.append(.number(Double(web.zoom)))
            }
        }
        return ["zooms": .array(zooms), "render_ms": .array(renders)]
    }
}

private func memoryFootprint() -> UInt64 {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
    let result = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count) }
    }
    return result == KERN_SUCCESS ? info.phys_footprint : 0
}
