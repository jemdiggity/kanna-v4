import AppKit
import KannaMarkdown

@MainActor
final class PaneSlot: NSView {
    var content: NSView? {
        didSet {
            oldValue?.removeFromSuperview()
            if let content {
                content.frame = bounds
                content.autoresizingMask = [.width, .height]
                addSubview(content)
            }
        }
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        paint()
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        paint()
    }

    private func paint() { layer?.backgroundColor = resolved(.windowBackgroundColor) }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }
}

@MainActor
final class Contents {
    private var slots: [String: PaneSlot] = [:]
    private(set) var states: [String: String] = [:]
    var onUpdate: ((String, [String: JSON]) -> Void)?
    var onRequest: ((String, [String: JSON]) -> Void)?
    var onCall: ((String, [String: JSON], @escaping (JSON) -> Void) -> Void)?
    var onFocus: ((String) -> Void)?
    var contextMenu: (() -> NSMenu?)?
    private var markdownSpots: [String: [String: (y: CGFloat, zoom: CGFloat)]] = [:]
    private var markdownZoom: [String: CGFloat] = [:]

    func slot(_ id: String) -> PaneSlot {
        if let slot = slots[id] { return slot }
        let slot = PaneSlot(frame: .zero)
        slots[id] = slot
        return slot
    }

    func shows(_ id: String) -> Bool { slots[id]?.content != nil }
    var ids: [String] { slots.keys.filter { slots[$0]?.content != nil }.sorted() }

    func web(_ id: String) -> WebPaneView? { slots[id]?.content as? WebPaneView }
    func ios(_ id: String) -> IosPaneView? { slots[id]?.content as? IosPaneView }
    func markdown(_ id: String) -> MarkdownPaneView? { slots[id]?.content as? MarkdownPaneView }
    func keyView(_ id: String) -> NSView? { web(id)?.webView ?? ios(id) ?? markdown(id)?.keyView }

    private func send(_ text: String, to agent: String, done: @escaping (String?) -> Void) {
        guard let onCall else { return done("not connected to the daemon") }
        onCall("send", ["pane": .string(agent), "text": .string("\u{1b}[200~" + text + "\u{1b}[201~")]) { answer in
            guard answer["ok"]?.bool == true else { return done(answer["error"]?["message"]?.string ?? "send failed") }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
                onCall("send", ["pane": .string(agent), "text": .string("")]) { _ in }
                done(nil)
            }
        }
    }

    private func report(_ id: String, _ fields: [String: JSON]) {
        if let state = fields["state"]?.string { states[id] = state }
        onUpdate?(id, fields)
    }

    func start(_ pane: JSON, restoring: Bool = false) {
        guard let id = pane["id"]?.string, let type = pane["type"]?.string, type != "term" else { return }
        stop(id)
        states[id] = "starting"
        let content: NSView
        var failure: String?
        switch type {
        case "md":
            let path = Self.absolute(pane["path"]?.string ?? "")
            var directory: ObjCBool = false
            if !FileManager.default.fileExists(atPath: path, isDirectory: &directory) || directory.boolValue {
                content = NSView()
                failure = directory.boolValue ? "\(path) is a folder, not a markdown file" : "No such file: \(path)"
            } else {
                let spot = markdownSpots[id]?[path]
                let markdown = MarkdownPaneView(path: path, zoom: spot?.zoom ?? markdownZoom[id] ?? pane["zoom"]?.number.map { CGFloat($0) } ?? 1, markup: pane["markup"]?.bool == true)
                markdown.startScrollY = spot?.y
                markdown.onHistory = { [weak self] back in self?.onRequest?("navigate", ["pane": .string(id), back ? "back" : "forward": true]) }
                markdown.onZoom = { [weak self, weak markdown] in
                    guard let markdown else { return }
                    self?.onUpdate?(id, ["zoom": .number(Double(markdown.zoom))])
                }
                markdown.onFocus = { [weak self] in self?.onFocus?(id) }
                markdown.agent = pane["agent"]?.string
                markdown.onSend = { [weak self] agent, text, done in self?.send(text, to: agent, done: done) }
                markdown.contextMenu = { [weak self] in self?.contextMenu?() }
                markdown.onOpenMarkdown = { [weak self] target in self?.onRequest?("navigate", ["pane": .string(id), "path": .string(target)]) }
                content = markdown
            }
        case "web":
            let web = WebPaneView(url: pane["url"]?.string ?? "about:blank", markup: pane["markup"]?.bool == true)
            web.onFocus = { [weak self] in self?.onFocus?(id) }
            web.webView.contextMenu = { [weak self] in self?.contextMenu?() }
            web.onNavigate = { [weak self] url in self?.onRequest?("navigate", ["pane": .string(id), "url": .string(url)]) }
            web.onURLChange = { [weak self] url in self?.onUpdate?(id, ["url": .string(url)]) }
            web.agent = pane["agent"]?.string
            web.onSend = { [weak self] agent, text, done in self?.send(text, to: agent, done: done) }
            content = web
        case "ios":
            let ios = IosPaneView()
            ios.onFocus = { [weak self] in self?.onFocus?(id) }
            ios.contextMenu = { [weak self] in self?.contextMenu?() }
            slot(id).content = ios
            startSimulator(id, app: pane["app"]?.string ?? "", device: pane["device"]?.string, restoring: restoring, in: ios)
            return
        default:
            return
        }
        slot(id).content = content
        DispatchQueue.main.async { [weak self] in
            guard self?.slots[id]?.content === content else { return }
            if let failure { self?.report(id, ["state": "failed", "error": .string(failure)]) } else { self?.report(id, ["state": "running"]) }
        }
    }

    private func startSimulator(_ id: String, app: String, device wanted: String?, restoring: Bool, in view: IosPaneView) {
        let appName = ((app as NSString).lastPathComponent as NSString).deletingPathExtension
        view.show(device: wanted ?? "iPhone", app: appName)
        view.show(status: "Finding \(wanted ?? "a device")…")
        Task { @MainActor [weak self] in
            let current = { self?.slots[id]?.content === view }
            do {
                let device = try await Simulator.resolve(wanted)
                guard current() else { return }
                if device.name != wanted { self?.onUpdate?(id, ["device": .string(device.name)]) }
                view.show(device: device.name, app: appName)
                view.show(status: "Booting \(device.name)…")
                try await Simulator.boot(device)
                guard current() else { return }
                view.show(status: "Launching \(appName)…")
                let name = restoring ? try await Simulator.reattach(app, on: device) : try await Simulator.launch(app, on: device)
                guard current() else { return }
                view.show(device: device.name, app: name)
                try view.attach(udid: device.udid)
                self?.report(id, ["state": "running"])
            } catch {
                guard current() else { return }
                view.show(status: nil)
                self?.report(id, ["state": "failed", "error": .string((error as? Simulator.Failure)?.message ?? error.localizedDescription)])
            }
        }
    }

    func stop(_ id: String) {
        guard let slot = slots[id], let content = slot.content else { return }
        (content as? WebPaneView)?.stop()
        (content as? IosPaneView)?.stop()
        if let markdown = content as? MarkdownPaneView {
            markdown.stop()
            markdownZoom[id] = markdown.zoom
            markdownSpots[id, default: [:]][markdown.path] = (markdown.scrollTop, markdown.zoom)
        }
        slot.content = nil
        states.removeValue(forKey: id)
    }

    func forget(_ id: String) {
        stop(id)
        slots.removeValue(forKey: id)?.removeFromSuperview()
        markdownSpots.removeValue(forKey: id)
        markdownZoom.removeValue(forKey: id)
    }

    static func absolute(_ path: String) -> String {
        let expanded = (path as NSString).expandingTildeInPath
        return expanded.hasPrefix("/") ? expanded : (NSHomeDirectory() as NSString).appendingPathComponent(expanded)
    }
}
