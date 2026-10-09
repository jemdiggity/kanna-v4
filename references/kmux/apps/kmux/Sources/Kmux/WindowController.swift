import AppKit
import WebKit
import KmuxCore
import KmuxMarkdown

/// One kmux window: draws the active tab's split tree.
@MainActor
final class WindowController: NSObject, NSWindowDelegate {
    static let divider: CGFloat = 1

    let id: String
    let window: NSWindow
    private let stage = StageView()
    /// Redraws from the model; called when the window resizes.
    var relayout: (() -> Void)? { didSet { stage.onLayout = relayout } }
    let tabBar = TabBar()
    var onCloseRequest: (() -> Void)?
    var onBecomeKey: (() -> Void)?
    /// The window moved or was resized.
    var onFrameChange: (() -> Void)?

    /// Named instances show their name in the title, to tell them apart.
    let instance: Instance

    init(id: String, instance: Instance, cascadeFrom previous: NSWindow?) {
        self.id = id
        self.instance = instance
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 600), styleMask: [.titled, .closable, .miniaturizable, .resizable],
                          backing: .buffered, defer: false)
        super.init()
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.title = Brand.current.displayName
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
        if let previous { window.setFrameTopLeftPoint(window.cascadeTopLeft(from: previous.frame.origin + NSPoint(x: 0, y: previous.frame.height))) } else { window.center() }
    }

    func render(_ model: Model, _ host: ContentHost) {
        guard let state = model.window(id), let tab = state.activeTab else { return }
        window.title = "\(tab.title) — \(id)" + (instance.isDefault ? "" : " · \(instance.name)")
        tabBar.show(model, state)
        let shown = Set(Model.paneIDs(state.zoomed.map { Node.pane($0) } ?? tab.root))
        for case let view as PaneView in stage.subviews where !shown.contains(view.id) { view.removeFromSuperview() }
        dividers.forEach { $0.removeFromSuperview() }
        dividers = []
        place(state.zoomed.map { Node.pane($0) } ?? tab.root, in: stage.bounds, host)
        dividers.forEach { stage.addSubview($0) }
        window.invalidateCursorRects(for: stage)
        for id in shown {
            guard let view = host.views[id], let pane = model.panes[id] else { continue }
            view.show(pane)
            view.focused = shown.count > 1 && id == state.focused && window.isKeyWindow
        }
        // Keyboard focus follows the model's focused pane, but only takes over
        // from another pane's view or from nothing: a tab being renamed keeps it.
        let responder = window.firstResponder
        if let focused = state.focused, let target = host.keyView(focused), responder !== target,
           responder == nil || responder === window || responder is TerminalSurfaceView || responder is FocusReportingWebView
           || responder is MarkdownTextView {
            window.makeFirstResponder(target)
        }
    }

    /// The handles over the dividers of the tab on screen, rebuilt by each render.
    private(set) var dividers: [DividerView] = []

    private func place(_ node: Node?, in rect: NSRect, _ host: ContentHost) {
        switch node {
        case nil: return
        case .pane(let id):
            guard let view = host.views[id] else { return }
            if view.superview !== stage { stage.addSubview(view) }
            view.frame = rect.integral
            view.autoresizingMask = []
        case .split(let split):
            let horizontal = split.axis == .row
            let total = (horizontal ? rect.width : rect.height) - Self.divider * CGFloat(split.kids.count - 1)
            var offset: CGFloat = 0
            var previous: NSRect?
            for (index, kid) in split.kids.enumerated() {
                let length = (total * kid.size).rounded()
                let frame = horizontal
                    ? NSRect(x: rect.minX + offset, y: rect.minY, width: length, height: rect.height)
                    : NSRect(x: rect.minX, y: rect.maxY - offset - length, width: rect.width, height: length)
                place(kid.node, in: frame, host)
                if let previous {
                    let line = horizontal
                        ? NSRect(x: previous.maxX, y: rect.minY, width: Self.divider, height: rect.height)
                        : NSRect(x: rect.minX, y: previous.minY - Self.divider, width: rect.width, height: Self.divider)
                    let handle = DividerView(split: split, index: index - 1, line: line, first: previous, second: frame)
                    handle.onChange = { [weak self] in self?.relayout?() }
                    dividers.append(handle)
                }
                previous = frame
                offset += length + Self.divider
            }
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

private func + (a: NSPoint, b: NSPoint) -> NSPoint { NSPoint(x: a.x + b.x, y: a.y + b.y) }
