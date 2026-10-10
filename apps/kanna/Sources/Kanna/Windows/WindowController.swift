import AppKit
import KannaMarkdown

@MainActor
final class WindowController: NSObject, NSWindowDelegate {
    static let divider: CGFloat = 1

    let id: String
    let window: NSWindow
    private let stage = StageView()
    private let content = StageView()
    private let dockStage = NSView()
    private(set) var dockDivider: DockDividerView?
    var relayout: (() -> Void)? { didSet { content.onLayout = relayout } }
    let tabBar = TabBar()
    var onCloseRequest: (() -> Void)?
    var onBecomeKey: (() -> Void)?
    var onFrameChange: (() -> Void)?
    var onDivider: ((DividerView, Double) -> Void)?
    var onDockWidth: ((String, Double) -> Void)?
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
        let size = window.contentLayoutRect.size
        content.frame = NSRect(origin: .zero, size: size)
        content.wantsLayer = true
        tabBar.frame = NSRect(x: 0, y: size.height - TabBar.height, width: size.width, height: TabBar.height)
        stage.frame = NSRect(x: 0, y: 0, width: size.width, height: size.height - TabBar.height)
        content.addSubview(stage)
        content.addSubview(tabBar)
        content.addSubview(dockStage)
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
        let dock = state["dock"].flatMap { $0["pane"]?.string == nil ? nil : $0 }
        let docked = dock?["pane"]?.string
        let ids = Self.paneIDs(root)
        shown = Self.shownPanes(state)
        arrange(dock, zoomed: docked != nil && state["zoomed"]?.string == docked)
        let visible = Set(ids)
        for case let view as PaneView in stage.subviews where !visible.contains(view.id) { view.removeFromSuperview() }
        for case let view as PaneView in dockStage.subviews where view.id != docked { view.removeFromSuperview() }
        dividers.forEach { $0.removeFromSuperview() }
        dividers = []
        place(root, path: [], in: stage.bounds, panes)
        dividers.forEach { stage.addSubview($0) }
        if let dock, let docked, let view = panes.view(docked, type: dock["type"]?.string ?? "term") {
            if view.superview !== dockStage { dockStage.addSubview(view) }
            view.frame = dockStage.bounds
            view.autoresizingMask = []
        }
        window.invalidateCursorRects(for: content)
        let focusedID = state["focused"]?.string
        let visibleIDs = ids + (docked.map { [$0] } ?? [])
        let many = state["zoomed"]?.string == nil && shown.count > 1
        for id in visibleIDs {
            guard let view = panes[id] else { continue }
            panes.started(id)
            view.focused = many && id == focusedID && window.isKeyWindow
        }
        let responder = window.firstResponder
        if let focusedID, let target = panes.keyView(focusedID), responder !== target,
           responder == nil || responder === window || responder is TerminalSurfaceView || responder is FocusReportingWebView
           || responder is MarkdownTextView || responder is IosPaneView {
            window.makeFirstResponder(target)
        }
    }

    private func arrange(_ dock: JSON?, zoomed: Bool) {
        let bounds = content.bounds
        var side = bounds
        dockDivider?.removeFromSuperview()
        dockDivider = nil
        if let dock, let pane = dock["pane"]?.string {
            if zoomed {
                dockStage.frame = bounds
                side = .zero
            } else {
                let total = bounds.width - Self.divider
                let low = min(DividerView.minimum, total / 2)
                let width = min(max((total * CGFloat(dock["width"]?.number ?? 0.4)).rounded(), low), total - low)
                let left = dock["edge"]?.string != "right"
                dockStage.frame = NSRect(x: left ? 0 : bounds.width - width, y: 0, width: width, height: bounds.height)
                side = NSRect(x: left ? width + Self.divider : 0, y: 0, width: total - width, height: bounds.height)
                let line = NSRect(x: left ? width : side.maxX, y: 0, width: Self.divider, height: bounds.height)
                let handle = DockDividerView(pane: pane, left: left, line: line, span: total, extent: bounds.width)
                handle.onWidth = { [weak self] width in self?.onDockWidth?(pane, width) }
                content.addSubview(handle)
                dockDivider = handle
            }
        }
        dockStage.isHidden = dock == nil
        tabBar.isHidden = side.isEmpty
        stage.isHidden = side.isEmpty
        tabBar.frame = NSRect(x: side.minX, y: side.maxY - TabBar.height, width: side.width, height: TabBar.height)
        stage.frame = NSRect(x: side.minX, y: side.minY, width: side.width, height: max(0, side.height - TabBar.height))
    }

    private(set) var dividers: [DividerView] = []

    static func shownPanes(_ window: JSON) -> [String] {
        let tab = window["tab"]?.string
        var ids: [String] = []
        if case .array(let tabs)? = window["tabs"], case .array(let list)? = tabs.first(where: { $0["id"]?.string == tab })?["panes"] {
            ids = list.compactMap(\.string)
        }
        guard let dock = window["dock"], let pane = dock["pane"]?.string else { return ids }
        return dock["edge"]?.string == "right" ? ids + [pane] : [pane] + ids
    }

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
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        paint()
    }
    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        paint()
    }
    private func paint() { layer?.backgroundColor = resolved(.separatorColor) }
    override func layout() {
        super.layout()
        onLayout?()
    }
}
