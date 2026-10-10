import AppKit
import WebKit

@MainActor
public final class PageMarkupRelay: NSObject, WKScriptMessageHandler {
    public weak var target: PageMarkupView?

    public func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        target?.pageChanged()
    }
}

@MainActor
public final class PageMarkupView: NSView {
    public static let world = WKContentWorld.world(name: "kanna-markup")
    public static var scriptedCommand = false

    public static func install(on configuration: WKWebViewConfiguration) -> PageMarkupRelay {
        let relay = PageMarkupRelay()
        let controller = configuration.userContentController
        controller.addUserScript(WKUserScript(source: script, injectionTime: .atDocumentEnd, forMainFrameOnly: true, in: world))
        controller.add(relay, contentWorld: world, name: "kannaMarkup")
        return relay
    }

    public static func page(for url: URL?) -> String? {
        guard let url, url.isFileURL, ["html", "htm"].contains(url.pathExtension.lowercased()) else { return nil }
        var directory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &directory), !directory.boolValue else { return nil }
        return url.path
    }

    public let path: String
    public let webView: WKWebView
    public var agent: String? { didSet { agentChanged() } }
    public var onSend: ((String, String, @escaping (String?) -> Void) -> Void)?

    let bar = DocBar()
    let panel = ThreadPanel()
    let overlay = PageLayer()
    let store: DocStore
    var record: DocRecord
    var storeStamp: Date?
    var fileStamp: Date?
    var pageHashValue = ""
    var boxes: [String: NSRect] = [:]
    var active: String?
    var box: CommentBox?
    var pending: (spot: PageSpot, rect: NSRect)?
    var showThreads = true
    var forcedThreads = false
    private var poll: Timer?
    private var flags: Any?
    private var refreshing = 0
    private var hovering = 0

    public init(path: String, webView: WKWebView) {
        self.path = path
        self.webView = webView
        store = DocStore(file: path)
        record = store.read()
        storeStamp = store.stamp
        fileStamp = Self.modified(path)
        super.init(frame: .zero)
        addSubview(webView)
        addSubview(overlay)
        addSubview(bar)
        addSubview(panel)
        bar.path.stringValue = store.relativeName
        bar.threads.onPress = { [weak self] in self?.toggleThreads() }
        panel.emptyText = "No comments yet. Select text and press ⌘↵, ⌘-click a part of the page, or ⌘-drag a box."
        panel.onSelect = { [weak self] id in self?.reveal(id) }
        panel.onAct = { [weak self] thread in self?.setState(thread.id, open: !thread.isOpen) }
        panel.onReply = { [weak self] id, text in self?.reply(id, text) }
        overlay.owner = self
        agentChanged()
        poll = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        flags = NSEvent.addLocalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
            MainActor.assumeIsolated { self?.commandChanged(event.modifierFlags.contains(.command)) }
            return event
        }
        refresh()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    public func stop() {
        poll?.invalidate()
        poll = nil
        flags.map(NSEvent.removeMonitor)
        flags = nil
    }

    private static func modified(_ path: String) -> Date? {
        (try? FileManager.default.attributesOfItem(atPath: path))?[.modificationDate] as? Date
    }

    public override func layout() {
        super.layout()
        let barHeight: CGFloat = 34
        let side = threadsVisible ? ThreadPanel.width : 0
        bar.frame = NSRect(x: 0, y: bounds.height - barHeight, width: bounds.width, height: barHeight)
        webView.frame = NSRect(x: 0, y: 0, width: max(0, bounds.width - side), height: max(0, bounds.height - barHeight))
        overlay.frame = webView.frame
        panel.isHidden = side == 0
        if bar.threads.on != (side > 0) { bar.threads.on = side > 0 }
        panel.frame = NSRect(x: bounds.width - side, y: 0, width: side, height: max(0, bounds.height - barHeight))
        refresh()
    }

    var threadsVisible: Bool { showThreads && (bounds.width >= ThreadPanel.width * 2 || forcedThreads) }

    func toggleThreads() {
        if bounds.width < ThreadPanel.width * 2 {
            forcedThreads = !threadsVisible
            showThreads = forcedThreads
        } else {
            showThreads.toggle()
            forcedThreads = false
        }
        bar.threads.on = threadsVisible
        needsLayout = true
    }

    func agentChanged() {
        bar.agent.set(agent.map { "Agent: \($0)" } ?? "No agent: comments are saved, not sent")
        bar.agent.warn = agent == nil
        bar.needsLayout = true
        if let agent, record.agent != agent {
            try? store.update { $0.agent = agent }
            storeStamp = store.stamp
            record = store.read()
        }
    }

    private func tick() {
        let stamp = Self.modified(path)
        if stamp != fileStamp {
            fileStamp = stamp
            if box == nil { webView.reload() }
        }
        if store.stamp != storeStamp {
            storeStamp = store.stamp
            record = store.read()
            refresh()
        }
    }

    func pageChanged() { refresh() }

    public var threads: [DocThread] { record.threads }
    public var activeThread: String? { active }
    public var commentBoxThread: String? { box.map { $0.threadID ?? "new" } }
    public var markRects: [String: NSRect] {
        Dictionary(uniqueKeysWithValues: overlay.marks.compactMap { mark in mark.thread.map { ($0, overlay.convert(mark.frame, to: self)) } })
    }
    public var badges: [String] { overlay.marks.filter { $0.label != nil }.compactMap(\.thread) }
    public var pageHash: String { pageHashValue }

    public func rect(of selector: String) async -> NSRect? {
        Self.rect(await call("return kannaMarkup.rectOf(selector)", ["selector": selector])).map { overlay.convert($0, to: self) }
    }

    func call(_ body: String, _ arguments: [String: Any] = [:]) async -> Any? {
        try? await webView.callAsyncJavaScript(body, arguments: arguments, in: nil, contentWorld: Self.world)
    }

    static func rect(_ value: Any?) -> NSRect? {
        guard let r = value as? [String: Any], let x = r["x"] as? Double, let y = r["y"] as? Double, let w = r["w"] as? Double, let h = r["h"] as? Double else { return nil }
        return NSRect(x: x, y: y, width: w, height: h)
    }

    func refresh() {
        refreshing += 1
        let ticket = refreshing
        let items: [[String: Any]] = record.threads.compactMap { thread in
            thread.page.map { ["id": thread.id, "selector": $0.selector, "text": $0.text, "box": $0.box, "selection": $0.selection == true] }
        }
        Task { [weak self] in
            guard let self else { return }
            let answer = await call("return kannaMarkup.locate(items)", ["items": items]) as? [String: Any]
            guard ticket == refreshing else { return }
            pageHashValue = answer?["hash"] as? String ?? ""
            boxes = [:]
            for (id, value) in answer?["boxes"] as? [String: Any] ?? [:] {
                if let rect = Self.rect(value) { boxes[id] = rect }
            }
            drawMarks()
        }
    }

    func onPage(_ thread: DocThread) -> Bool { thread.page.map { $0.hash == pageHashValue } ?? false }

    func drawMarks() {
        overlay.marks.forEach { $0.removeFromSuperview() }
        overlay.marks = []
        let view = overlay.bounds
        for thread in record.threads where onPage(thread) {
            guard let rect = boxes[thread.id] else { continue }
            let kind: MarkBox.Kind = thread.id == active ? .active : thread.isOpen ? .open : .resolved
            let press = { [weak self] in
                guard let self else { return }
                active = thread.id
                drawMarks()
                panel.reveal(thread.id)
                openPop(thread.id)
            }
            guard rect.width * rect.height > view.width * view.height / 2 else {
                let mark = MarkBox(kind: kind)
                mark.thread = thread.id
                mark.frame = rect
                mark.onPress = press
                overlay.addSubview(mark, positioned: .below, relativeTo: nil)
                overlay.marks.append(mark)
                continue
            }
            let region = MarkBox(kind: .outline)
            region.frame = rect
            region.isHidden = thread.id != active
            let badge = MarkBox(kind: kind, label: thread.id)
            badge.thread = thread.id
            let size = badge.badgeSize
            badge.frame = NSRect(x: min(max(rect.minX, 0), view.width - size.width - 12) + 6, y: min(max(rect.minY, 0), view.height - size.height - 12) + 6, width: size.width, height: size.height)
            badge.onPress = press
            badge.onHover = { [weak self, weak region] inside in region?.isHidden = !inside && thread.id != self?.active }
            overlay.addSubview(region, positioned: .below, relativeTo: nil)
            overlay.addSubview(badge)
            overlay.marks.append(contentsOf: [region, badge])
        }
        panel.show(record.threads, active: active, notes: notes())
        bar.threads.set("Threads \(record.threads.filter(\.isOpen).count)")
        bar.needsLayout = true
    }

    func notes() -> [String: String] {
        var out: [String: String] = [:]
        for thread in record.threads where thread.isOpen {
            if thread.status == "unsent" {
                out[thread.id] = "not sent" + (thread.error.map { ": \($0)" } ?? "")
            } else if let page = thread.page, page.hash != pageHashValue {
                out[thread.id] = "on the page at \(page.hash.isEmpty ? "its start" : page.hash)"
            }
        }
        return out
    }

    func commandChanged(_ down: Bool) {
        if !down { overlay.hover = nil }
        overlay.window?.invalidateCursorRects(for: overlay)
    }

    func hover(at point: NSPoint) {
        hovering += 1
        let ticket = hovering
        Task { [weak self] in
            guard let self else { return }
            let hit = await call("return kannaMarkup.hit(x, y)", ["x": point.x, "y": point.y]) as? [String: Any]
            guard ticket == hovering, PageLayer.commandHeld else { return }
            overlay.hover = Self.rect(hit?["rect"])
        }
    }

    func picked(at start: NSPoint, drag: NSRect?) {
        hovering += 1
        overlay.hover = nil
        Task { [weak self] in
            guard let self else { return }
            var selector = "", text = "", rect = drag
            if drag == nil {
                guard let hit = await call("return kannaMarkup.hit(x, y)", ["x": start.x, "y": start.y]) as? [String: Any], let at = Self.rect(hit["rect"]) else {
                    overlay.draft = nil
                    return
                }
                selector = String((hit["selector"] as? String ?? "").prefix(200))
                text = String((hit["text"] as? String ?? "").prefix(120))
                rect = at
            }
            guard let rect else { return }
            let info = await call("return kannaMarkup.place(x, y, w, h)", ["x": rect.minX, "y": rect.minY, "w": rect.width, "h": rect.height]) as? [String: Any] ?? [:]
            let ints = { (key: String) in (info[key] as? [Any] ?? []).compactMap { ($0 as? NSNumber)?.intValue } }
            let spot = PageSpot(selector: selector, text: text, hash: info["hash"] as? String ?? "", box: ints("box"), viewport: ints("viewport"), scroll: ints("scroll"))
            openComposer(spot, rect: rect)
        }
    }

    public override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard event.keyCode == 36, event.modifierFlags.intersection([.command, .shift, .option, .control]) == .command,
              let responder = window?.firstResponder as? NSView, responder === webView || responder.isDescendant(of: webView) else {
            return super.performKeyEquivalent(with: event)
        }
        commentOnSelection()
        return true
    }

    func commentOnSelection() {
        Task { [weak self] in
            guard let self, let hit = await call("return kannaMarkup.selection()") as? [String: Any], let rect = Self.rect(hit["rect"]) else { return }
            let info = await call("return kannaMarkup.place(x, y, w, h)", ["x": rect.minX, "y": rect.minY, "w": rect.width, "h": rect.height]) as? [String: Any] ?? [:]
            let ints = { (key: String) in (info[key] as? [Any] ?? []).compactMap { ($0 as? NSNumber)?.intValue } }
            let spot = PageSpot(selector: String((hit["selector"] as? String ?? "").prefix(200)), text: String((hit["text"] as? String ?? "").prefix(120)),
                                hash: info["hash"] as? String ?? "", box: ints("box"), viewport: ints("viewport"), scroll: ints("scroll"), selection: true)
            openComposer(spot, rect: rect)
        }
    }

    func boxOrigin(below rect: NSRect, height: CGFloat) -> NSPoint {
        let x = max(8, min(rect.minX, overlay.bounds.width - CommentBox.width - 8))
        let y = min(rect.maxY + 6, max(8, overlay.bounds.height - height - 8))
        return NSPoint(x: x, y: y)
    }

    func openComposer(_ spot: PageSpot, rect: NSRect) {
        closeBox()
        pending = (spot, rect)
        overlay.draft = rect
        let box = CommentBox(composer: ())
        box.frame.origin = boxOrigin(below: rect, height: box.frame.height)
        (box.button("post") as? Chip)?.onPress = { [weak self] in self?.post() }
        box.field.onKey = { [weak self] event in
            if event.keyCode == 36, event.modifierFlags.contains(.command) { self?.post(); return true }
            if event.keyCode == 53 { self?.closeBox(); return true }
            return false
        }
        show(box)
    }

    func openPop(_ id: String) {
        guard let thread = record.threads.first(where: { $0.id == id }), let rect = boxes[id] else { return }
        closeBox()
        let box = CommentBox(thread: thread, note: notes()[id])
        box.frame.origin = boxOrigin(below: rect, height: box.frame.height)
        (box.button("act") as? ActButton)?.onPress = { [weak self] in
            self?.closeBox()
            self?.setState(id, open: !thread.isOpen)
        }
        box.field.onKey = { [weak self, weak box] event in
            if event.keyCode == 53 { self?.closeBox(); return true }
            guard event.keyCode == 36, !event.modifierFlags.contains(.shift), let box else { return false }
            let text = box.field.trimmed
            guard !text.isEmpty else { return true }
            self?.reply(id, text)
            self?.openPop(id)
            return true
        }
        show(box)
    }

    private func show(_ box: CommentBox) {
        self.box = box
        overlay.addSubview(box)
        window?.makeFirstResponder(box.field)
        DispatchQueue.main.async { [weak self, weak box] in
            guard let box, self?.box === box else { return }
            self?.window?.makeFirstResponder(box.field)
        }
    }

    func closeBox() {
        guard let box else { return }
        self.box = nil
        pending = nil
        overlay.draft = nil
        box.removeFromSuperview()
        window?.makeFirstResponder(webView)
    }

    func post() {
        guard let pending, let text = box.map({ DocStore.clean($0.field.trimmed) }), !text.isEmpty else { return }
        closeBox()
        Task { [weak self] in
            guard let self else { return }
            let png = await snapshot(pending.rect)
            let spot = pending.spot
            let source = (try? String(contentsOfFile: path, encoding: .utf8)) ?? ""
            let line = spot.text.isEmpty ? 1 : DocStore.line(of: spot.text, in: source, near: 1)
            let quote = DocStore.clean(spot.text.isEmpty ? (spot.selector.isEmpty ? "a box" : spot.selector) : spot.text)
            let id = try? store.update { record -> String in
                let id = "c\(record.next)"
                record.next += 1
                record.threads.append(DocThread(id: id, quote: quote, prefix: "", suffix: "", line: line, state: "open", status: "sent",
                                                messages: [DocMessage(who: "you", text: text, at: DocStore.now())], error: nil, page: spot))
                return id
            }
            guard let id else { return }
            if let png, let mark = try? store.saveMark(id, png: png) {
                try? store.update { record in
                    if let index = record.threads.firstIndex(where: { $0.id == id }) { record.threads[index].mark = mark }
                }
            }
            active = id
            reloadThreads()
            deliver(id, text)
        }
    }

    func snapshot(_ rect: NSRect) async -> Data? {
        guard let image = try? await webView.takeSnapshot(configuration: nil) else { return nil }
        let size = webView.bounds.size
        let scale = window?.backingScaleFactor ?? 2
        guard size.width > 0, size.height > 0,
              let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size.width * scale), pixelsHigh: Int(size.height * scale), bitsPerSample: 8,
                                         samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
        else { return nil }
        rep.size = size
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        image.draw(in: NSRect(origin: .zero, size: size))
        let line = max(3, CGFloat(rep.pixelsWide) / 400) / scale
        let flipped = NSRect(x: rect.minX, y: size.height - rect.maxY, width: rect.width, height: rect.height).insetBy(dx: line / 2, dy: line / 2)
        let path = NSBezierPath(rect: flipped)
        path.lineWidth = line
        NSColor(srgbRed: 1, green: 0.231, blue: 0.188, alpha: 1).setStroke()
        path.stroke()
        NSGraphicsContext.restoreGraphicsState()
        return rep.representation(using: .png, properties: [:])
    }

    func reply(_ id: String, _ text: String) {
        let text = DocStore.clean(text)
        try? store.update { record in
            guard let index = record.threads.firstIndex(where: { $0.id == id }) else { return }
            record.threads[index].messages.append(DocMessage(who: "you", text: text, at: DocStore.now()))
            record.threads[index].state = "open"
        }
        reloadThreads()
        deliver(id, text)
    }

    func setState(_ id: String, open: Bool) {
        try? store.update { record in
            guard let index = record.threads.firstIndex(where: { $0.id == id }) else { return }
            record.threads[index].state = open ? "open" : "resolved"
        }
        reloadThreads()
    }

    func reveal(_ id: String) {
        active = id
        closeBox()
        guard let page = record.threads.first(where: { $0.id == id })?.page else { return drawMarks() }
        Task { [weak self] in
            guard let self else { return }
            if page.hash != pageHashValue { _ = await call("kannaMarkup.go(hash)", ["hash": page.hash]) }
            _ = await call("kannaMarkup.show(box)", ["box": page.box])
            refresh()
        }
    }

    private func reloadThreads() {
        record = store.read()
        storeStamp = store.stamp
        refresh()
    }

    private func deliver(_ id: String, _ text: String) {
        guard var thread = record.threads.first(where: { $0.id == id }), DocStore.isThreadID(id) else { return }
        let source = (try? String(contentsOfFile: path, encoding: .utf8)) ?? ""
        if let page = thread.page, !page.text.isEmpty { thread.line = DocStore.line(of: page.text, in: source, near: thread.line) }
        let message = DocStore.clean(DocStore.message(thread, file: store.relativeName, text: text, mark: store.mark(id)))
        let finish: (String?) -> Void = { [weak self] error in
            guard let self else { return }
            try? store.update { record in
                guard let index = record.threads.firstIndex(where: { $0.id == id }) else { return }
                record.threads[index].status = error == nil ? "sent" : "unsent"
                record.threads[index].error = error
                record.threads[index].line = thread.line
            }
            reloadThreads()
        }
        guard let agent, let onSend else { return finish("no agent") }
        onSend(agent, message, finish)
    }

    static let script = #"""
    (() => {
      const zoom = () => window.visualViewport?.scale || 1;
      const origin = () => document.documentElement.getBoundingClientRect();
      const toView = r => { const s = zoom(); return { x: r.left * s, y: r.top * s, w: r.width * s, h: r.height * s }; };
      const toClient = (x, y) => { const s = zoom(); return [x / s, y / s]; };
      const flat = s => (s || "").replace(/\s+/g, " ").trim();
      const textOf = el => flat(el.innerText || el.getAttribute("aria-label") || el.getAttribute("alt")).slice(0, 120);
      function selectorOf(el) {
        const parts = [];
        for (let n = el; n && n.nodeType === 1 && n.localName !== "html"; n = n.parentElement) {
          if (n.id && /^[A-Za-z][\w-]*$/.test(n.id)) { parts.unshift("#" + n.id); break; }
          let part = n.localName;
          const same = n.parentElement ? [...n.parentElement.children].filter(c => c.localName === n.localName) : [];
          if (same.length > 1) part += `:nth-of-type(${same.indexOf(n) + 1})`;
          parts.unshift(part);
        }
        return parts.join(" > ");
      }
      function textRect(el, text) {
        const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT), nodes = [];
        let all = "";
        for (let n = walker.nextNode(); n; n = walker.nextNode()) { nodes.push([n, all.length]); all += n.data; }
        const words = text.replace(/…$/, "").split(/\s+/).filter(Boolean).map(w => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
        const match = words.length && new RegExp(words.join("\\s+")).exec(all);
        if (!match) return null;
        const at = off => { const [n, start] = nodes.findLast(([, s]) => s <= off); return [n, Math.min(off - start, n.data.length)]; };
        const range = document.createRange();
        range.setStart(...at(match.index));
        range.setEnd(...at(match.index + match[0].length));
        const r = range.getBoundingClientRect();
        return r.width || r.height ? toView(r) : null;
      }
      function find(item) {
        if (item.selector) {
          let el = null;
          try { el = document.querySelector(item.selector); } catch {}
          const inText = el && item.selection && item.text ? textRect(el, item.text) : null;
          if (inText) return inText;
          if (el && !item.selection && (!item.text || flat(el.innerText).startsWith(item.text.replace(/…$/, "")))) {
            const r = el.getBoundingClientRect();
            if (r.width || r.height) return toView(r);
          }
        }
        const [x, y, w, h] = item.box, o = origin();
        return toView({ left: x + o.left, top: y + o.top, width: w, height: h });
      }
      window.kannaMarkup = {
        hit(x, y) {
          const el = document.elementFromPoint(...toClient(x, y));
          if (!el || el === document.documentElement || el === document.body) return null;
          return { selector: selectorOf(el), text: textOf(el), rect: toView(el.getBoundingClientRect()) };
        },
        selection() {
          const s = getSelection();
          if (!s || s.isCollapsed || !s.rangeCount) return null;
          const text = flat(s.toString());
          if (!text) return null;
          const range = s.getRangeAt(0);
          let el = range.commonAncestorContainer;
          if (el.nodeType !== 1) el = el.parentElement;
          const out = { selector: selectorOf(el), text: text.slice(0, 120), rect: toView(range.getBoundingClientRect()) };
          s.removeAllRanges();
          return out;
        },
        locate(items) {
          return { hash: location.hash.slice(0, 100), boxes: Object.fromEntries(items.map(i => [i.id, find(i)])) };
        },
        place(x, y, w, h) {
          const [cx, cy] = toClient(x, y), s = zoom(), o = origin();
          return { hash: location.hash.slice(0, 100), box: [cx - o.left, cy - o.top, w / s, h / s].map(Math.round),
                   viewport: [innerWidth, innerHeight], scroll: [Math.round(-o.left), Math.round(-o.top)] };
        },
        go(hash) { if (location.hash !== hash) location.hash = hash; },
        show(box) {
          const top = box[1] + origin().top, height = innerHeight / zoom();
          if (top < 0 || top + box[3] > height) scrollTo(scrollX, Math.max(0, box[1] - height / 3));
        },
        rectOf(selector) { const el = document.querySelector(selector); return el ? toView(el.getBoundingClientRect()) : null; },
      };
      const changed = () => window.webkit?.messageHandlers?.kannaMarkup?.postMessage("changed");
      addEventListener("scroll", changed, { passive: true });
      addEventListener("resize", changed);
      addEventListener("hashchange", changed);
      addEventListener("load", changed);
      window.visualViewport?.addEventListener("resize", changed);
      window.visualViewport?.addEventListener("scroll", changed);
      changed();
    })();
    """#
}

@MainActor
final class PageLayer: FlippedView {
    weak var owner: PageMarkupView?
    var marks: [MarkBox] = []
    private var draftView: MarkBox?
    private var hoverView: MarkBox?

    static var commandHeld: Bool { NSEvent.modifierFlags.contains(.command) || PageMarkupView.scriptedCommand }

    var hover: NSRect? {
        didSet {
            hoverView?.removeFromSuperview()
            hoverView = nil
            guard let hover else { return }
            let view = MarkBox(kind: .hover)
            view.frame = hover
            addSubview(view)
            hoverView = view
        }
    }

    var draft: NSRect? {
        didSet {
            draftView?.removeFromSuperview()
            draftView = nil
            guard let draft else { return }
            let view = MarkBox(kind: .draft)
            view.frame = draft
            addSubview(view, positioned: .below, relativeTo: subviews.first { $0 is CommentBox })
            draftView = view
        }
    }

    init() {
        super.init(frame: .zero)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect, .cursorUpdate], owner: self))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func hitTest(_ point: NSPoint) -> NSView? {
        let hit = super.hitTest(point)
        if hit == nil || hit === self { return frame.contains(point) && Self.commandHeld ? self : nil }
        if hit is MarkBox, (hit as? MarkBox)?.kind == .hover || (hit as? MarkBox)?.kind == .draft { return Self.commandHeld ? self : nil }
        return hit
    }

    override func mouseMoved(with event: NSEvent) {
        guard Self.commandHeld || event.modifierFlags.contains(.command) else {
            hover = nil
            return
        }
        NSCursor.crosshair.set()
        owner?.hover(at: convert(event.locationInWindow, from: nil))
    }

    override func mouseExited(with event: NSEvent) { hover = nil }

    override func cursorUpdate(with event: NSEvent) {
        if Self.commandHeld { NSCursor.crosshair.set() } else { super.cursorUpdate(with: event) }
    }

    override func mouseDown(with event: NSEvent) {
        defer { PageMarkupView.scriptedCommand = false }
        guard event.modifierFlags.contains(.command) || Self.commandHeld, let window else { return }
        owner?.closeBox()
        hover = nil
        let start = convert(event.locationInWindow, from: nil)
        var drag: NSRect?
        while let next = window.nextEvent(matching: [.leftMouseDragged, .leftMouseUp]) {
            let at = convert(next.locationInWindow, from: nil)
            drag = abs(at.x - start.x) >= 6 || abs(at.y - start.y) >= 6
                ? NSRect(x: min(start.x, at.x), y: min(start.y, at.y), width: abs(at.x - start.x), height: abs(at.y - start.y)).intersection(bounds) : nil
            if next.type == .leftMouseUp { break }
            draft = drag
        }
        owner?.picked(at: start, drag: drag)
    }
}

@MainActor
final class MarkBox: FlippedView {
    enum Kind { case open, resolved, active, hover, draft, outline }
    let kind: Kind
    let label: String?
    var thread: String?
    var onPress: (() -> Void)?
    var onHover: ((Bool) -> Void)?

    init(kind: Kind, label: String? = nil) {
        self.kind = kind
        self.label = label
        super.init(frame: .zero)
        radius = 3
        paint()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    private var text: NSAttributedString? {
        label.map { NSAttributedString(string: $0, attributes: [.font: NSFont.systemFont(ofSize: 11.5, weight: .semibold), .foregroundColor: NSColor.white]) }
    }

    var badgeSize: NSSize {
        let width = ceil(text?.size().width ?? 0) + 14
        return NSSize(width: max(22, width), height: 22)
    }

    private func paint() {
        guard label == nil else { fill = nil; return }
        switch kind {
        case .open: fill = DocTheme.tint
        case .active, .draft: fill = DocTheme.tintOn
        case .resolved: fill = DocTheme.resolvedTint
        case .hover, .outline: fill = nil
        }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        paint()
    }

    override func draw(_ dirtyRect: NSRect) {
        let grey = NSColor(srgbRed: 0.5, green: 0.5, blue: 0.5, alpha: 0.7)
        if let text {
            let pill = NSBezierPath(roundedRect: bounds.insetBy(dx: 2, dy: 2), xRadius: bounds.height / 2 - 2, yRadius: bounds.height / 2 - 2)
            (kind == .resolved ? grey : DocTheme.accent).setFill()
            pill.fill()
            if kind == .active {
                let ring = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.75, dy: 0.75), xRadius: bounds.height / 2, yRadius: bounds.height / 2)
                ring.lineWidth = 1.5
                DocTheme.accent.setStroke()
                ring.stroke()
            }
            let size = text.size()
            text.draw(at: NSPoint(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2))
            return
        }
        super.draw(dirtyRect)
        let path = NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: radius, yRadius: radius)
        path.lineWidth = 2
        if kind == .hover || kind == .resolved { path.setLineDash([4, 3], count: 2, phase: 0) }
        (kind == .resolved ? grey : DocTheme.accent).setStroke()
        path.stroke()
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        kind == .outline ? nil : super.hitTest(point)
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        if onHover != nil { addTrackingArea(NSTrackingArea(rect: bounds, options: [.mouseEnteredAndExited, .activeAlways], owner: self)) }
    }

    override func mouseEntered(with event: NSEvent) { onHover?(true) }
    override func mouseExited(with event: NSEvent) { onHover?(false) }

    override func mouseDown(with event: NSEvent) {
        if let onPress { onPress() } else { super.mouseDown(with: event) }
    }

    override func resetCursorRects() {
        if onPress != nil { addCursorRect(bounds, cursor: .pointingHand) }
    }
}
