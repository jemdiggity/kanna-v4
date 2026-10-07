import AppKit
import KmuxCore

/// One kmux window: draws the active tab's split tree.
@MainActor
final class WindowController: NSObject, NSWindowDelegate {
    static let divider: CGFloat = 1

    let id: String
    let window: NSWindow
    private let stage = StageView()
    /// Redraws from the model; called when the window resizes.
    var relayout: (() -> Void)? { didSet { stage.onLayout = relayout } }
    var onCloseRequest: (() -> Void)?
    var onBecomeKey: (() -> Void)?

    init(id: String, cascadeFrom previous: NSWindow?) {
        self.id = id
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 600), styleMask: [.titled, .closable, .miniaturizable, .resizable],
                          backing: .buffered, defer: false)
        super.init()
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.title = "kmux"
        window.delegate = self
        stage.wantsLayer = true
        stage.layer?.backgroundColor = NSColor.separatorColor.cgColor
        window.contentView = stage
        if let previous { window.setFrameTopLeftPoint(window.cascadeTopLeft(from: previous.frame.origin + NSPoint(x: 0, y: previous.frame.height))) } else { window.center() }
    }

    func render(_ model: Model, _ host: TerminalHost) {
        guard let state = model.window(id), let tab = state.activeTab else { return }
        window.title = model.title(of: tab)
        let shown = Set(Model.paneIDs(state.zoomed.map { Node.pane($0) } ?? tab.root))
        for case let view as PaneView in stage.subviews where !shown.contains(view.id) { view.removeFromSuperview() }
        place(state.zoomed.map { Node.pane($0) } ?? tab.root, in: stage.bounds, host)
        for id in shown {
            guard let view = host.views[id], let pane = model.panes[id] else { continue }
            view.show(pane)
            view.focused = shown.count > 1 && id == state.focused && window.isKeyWindow
        }
        if let focused = state.focused, let terminal = host.terminal(focused), window.firstResponder !== terminal {
            window.makeFirstResponder(terminal)
        }
    }

    private func place(_ node: Node?, in rect: NSRect, _ host: TerminalHost) {
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
            for kid in split.kids {
                let length = (total * kid.size).rounded()
                let frame = horizontal
                    ? NSRect(x: rect.minX + offset, y: rect.minY, width: length, height: rect.height)
                    : NSRect(x: rect.minX, y: rect.maxY - offset - length, width: rect.width, height: length)
                place(kid.node, in: frame, host)
                offset += length + Self.divider
            }
        }
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        onCloseRequest?()
        return false
    }

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
