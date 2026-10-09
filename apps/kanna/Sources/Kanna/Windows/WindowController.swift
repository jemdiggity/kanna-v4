import AppKit
import KannaMarkdown

@MainActor
final class WindowController: NSObject, NSWindowDelegate {
    static let divider: CGFloat = 1

    let id: String
    let window: NSWindow
    private let stage = StageView()
    var relayout: (() -> Void)? { didSet { stage.onLayout = relayout } }
    let tabBar = TabBar()
    var onCloseRequest: (() -> Void)?
    var onBecomeKey: (() -> Void)?
    var onFrameChange: (() -> Void)?
    var onDivider: ((DividerView, Double) -> Void)?
    private(set) var shown: [String] = []
    private(set) var tab: String?

    init(id: String, cascadeFrom previous: NSWindow?) {
        self.id = id
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 600), styleMask: [.titled, .closable, .miniaturizable, .resizable],
                          backing: .buffered, defer: false)
        super.init()
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.title = "Kanna"
        window.delegate = self
        stage.wantsLayer = true
        stage.layer?.backgroundColor = NSColor.separatorColor.cgColor
        let size = window.contentLayoutRect.size
        let content = NSView(frame: NSRect(origin: .zero, size: size))
        content.wantsLayer = true
        tabBar.frame = NSRect(x: 0, y: size.height - TabBar.height, width: size.width, height: TabBar.height)
        tabBar.autoresizingMask = [.width, .minYMargin]
        stage.frame = NSRect(x: 0, y: 0, width: size.width, height: size.height - TabBar.height)
        stage.autoresizingMask = [.width, .height]
        content.addSubview(stage)
        content.addSubview(tabBar)
        window.contentView = content
        if let previous {
            window.setFrameTopLeftPoint(window.cascadeTopLeft(from: NSPoint(x: previous.frame.minX, y: previous.frame.maxY)))
        } else {
            window.center()
        }
    }

    func render(_ state: JSON, _ panes: PaneViews) {
        tab = state["tab"]?.string
        let instance = AppController.instanceName
        window.title = "\(state["title"]?.string ?? "Kanna") — \(id)" + (instance == "default" ? "" : " · \(instance)")
        tabBar.show(state)
        let root = state["root"] ?? .null
        let ids = Self.paneIDs(root)
        shown = ids
        let visible = Set(ids)
        for case let view as PaneView in stage.subviews where !visible.contains(view.id) { view.removeFromSuperview() }
        dividers.forEach { $0.removeFromSuperview() }
        dividers = []
        place(root, path: [], in: stage.bounds, panes)
        dividers.forEach { stage.addSubview($0) }
        window.invalidateCursorRects(for: stage)
        let focusedID = state["focused"]?.string
        for id in ids {
            guard let view = panes[id] else { continue }
            panes.started(id)
            view.focused = ids.count > 1 && id == focusedID && window.isKeyWindow
        }
        let responder = window.firstResponder
        if let focusedID, let target = panes.keyView(focusedID), responder !== target,
           responder == nil || responder === window || responder is TerminalSurfaceView || responder is FocusReportingWebView
           || responder is MarkdownTextView || responder is IosPaneView {
            window.makeFirstResponder(target)
        }
    }

    private(set) var dividers: [DividerView] = []

    static func paneIDs(_ node: JSON) -> [String] {
        if let id = node["pane"]?.string { return [id] }
        guard case .array(let children)? = node["children"] else { return [] }
        return children.flatMap(paneIDs)
    }

    private func place(_ node: JSON, path: [Int], in rect: NSRect, _ panes: PaneViews) {
        if let id = node["pane"]?.string {
            guard let view = panes.view(id, type: node["type"]?.string ?? "term") else { return }
            if view.superview !== stage { stage.addSubview(view, positioned: .below, relativeTo: nil) }
            view.frame = rect.integral
            view.autoresizingMask = []
            return
        }
        guard case .array(let children)? = node["children"], !children.isEmpty else { return }
        let horizontal = node["split"]?.string == "row"
        let total = (horizontal ? rect.width : rect.height) - Self.divider * CGFloat(children.count - 1)
        var offset: CGFloat = 0
        var previous: (frame: NSRect, size: Double, last: String?)?
        for (index, child) in children.enumerated() {
            let size = child["size"]?.number ?? 1 / Double(children.count)
            let length = (total * CGFloat(size)).rounded()
            let frame = horizontal
                ? NSRect(x: rect.minX + offset, y: rect.minY, width: length, height: rect.height)
                : NSRect(x: rect.minX, y: rect.maxY - offset - length, width: rect.width, height: length)
            place(child, path: path + [index], in: frame, panes)
            if let previous, let tab {
                let line = horizontal
                    ? NSRect(x: previous.frame.maxX, y: rect.minY, width: Self.divider, height: rect.height)
                    : NSRect(x: rect.minX, y: previous.frame.minY - Self.divider, width: rect.width, height: Self.divider)
                let handle = DividerView(tab: tab, path: path, index: index - 1, row: horizontal, sizes: (previous.size, size), after: previous.last,
                                         line: line, first: previous.frame, second: frame)
                handle.onShare = { [weak self, weak handle] share in
                    guard let self, let handle else { return }
                    onDivider?(handle, share)
                }
                dividers.append(handle)
            }
            previous = (frame, size, child["pane"]?.string)
            offset += length + Self.divider
        }
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        onCloseRequest?()
        return false
    }

    func windowDidMove(_ notification: Notification) { onFrameChange?() }
    func windowDidResize(_ notification: Notification) { onFrameChange?() }
    func windowDidBecomeKey(_ notification: Notification) { onBecomeKey?() }
    func windowDidResignKey(_ notification: Notification) { onBecomeKey?() }
}

private final class StageView: NSView {
    var onLayout: (() -> Void)?
    override func layout() {
        super.layout()
        onLayout?()
    }
}
