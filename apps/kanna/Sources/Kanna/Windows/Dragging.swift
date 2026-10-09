import AppKit
import KannaLayoutC

@MainActor func screenPoint(_ event: NSEvent) -> NSPoint {
    event.window.map { $0.convertPoint(toScreen: event.locationInWindow) } ?? event.locationInWindow
}

@MainActor var scriptedDrag: [NSPoint]?

@MainActor func trackMouse(_ moved: (NSPoint) -> Void) -> NSPoint {
    if let points = scriptedDrag, let end = points.last {
        scriptedDrag = nil
        points.dropLast().forEach(moved)
        return end
    }
    while let event = NSApp.nextEvent(matching: [.leftMouseDragged, .leftMouseUp], until: .distantFuture, inMode: .eventTracking, dequeue: true) {
        let point = screenPoint(event)
        if event.type == .leftMouseUp { return point }
        moved(point)
    }
    return NSEvent.mouseLocation
}

@MainActor
final class DropHint {
    private let panel: NSPanel
    private let box = NSView()
    private let label = NSTextField(labelWithString: "")

    static var level = NSWindow.Level.floating
    static var quiet = false

    init() {
        panel = NSPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.ignoresMouseEvents = true
        panel.level = Self.level
        box.wantsLayer = true
        box.layer?.backgroundColor = NSColor.controlAccentColor.withAlphaComponent(0.22).cgColor
        box.layer?.borderColor = NSColor.controlAccentColor.cgColor
        box.layer?.borderWidth = 2
        label.font = .monospacedSystemFont(ofSize: 12, weight: .semibold)
        label.textColor = .controlAccentColor
        label.alignment = .center
        box.addSubview(label)
        panel.contentView = box
    }

    var windowNumber: Int { panel.windowNumber }

    func show(_ rect: NSRect, text: String = "", rounded: Bool = false) {
        guard !Self.quiet else { return }
        panel.setFrame(rect, display: false)
        box.layer?.cornerRadius = rounded ? 10 : 2
        label.stringValue = text
        label.sizeToFit()
        label.frame.origin = NSPoint(x: (rect.width - label.frame.width) / 2, y: (rect.height - label.frame.height) / 2)
        panel.orderFront(nil)
    }

    func hide() { panel.orderOut(nil) }
}

@MainActor
final class DividerView: NSView {
    static let grab: CGFloat = 3
    static let nice: [Double] = [1 / 4, 1 / 3, 1 / 2, 2 / 3, 3 / 4]
    static let minimum: CGFloat = 80

    let tab: String
    let path: [Int]
    let index: Int
    let row: Bool
    let after: String?
    private let pair: Double
    private let start: CGFloat
    private let span: CGFloat
    var onShare: ((Double) -> Void)?

    init(tab: String, path: [Int], index: Int, row: Bool, sizes: (Double, Double), after: String?, line: NSRect, first: NSRect, second: NSRect) {
        self.tab = tab
        self.path = path
        self.index = index
        self.row = row
        self.after = after
        pair = sizes.0 + sizes.1
        start = row ? first.minX : first.maxY
        span = row ? second.maxX - first.minX : first.maxY - second.minY
        super.init(frame: row ? line.insetBy(dx: -Self.grab, dy: 0) : line.insetBy(dx: 0, dy: -Self.grab))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: row ? .resizeLeftRight : .resizeUpDown)
    }

    override func mouseDown(with event: NSEvent) {
        guard let stage = superview else { return }
        drag(in: stage)
    }

    func position(_ fraction: CGFloat) -> NSPoint {
        row ? NSPoint(x: start + span * fraction, y: frame.midY) : NSPoint(x: frame.midX, y: start - span * fraction)
    }

    func share(at position: NSPoint) -> Double {
        let low = min(Self.minimum, span / 2)
        let offset = min(max(row ? position.x - start : start - position.y, low), span - low)
        let share = pair * Double(offset / span)
        return Self.nice.first { abs(share - $0) < 0.015 && $0 < pair } ?? share
    }

    func drag(in stage: NSView) {
        let tip = NSTextField(labelWithString: "")
        tip.font = .monospacedSystemFont(ofSize: 11, weight: .medium)
        tip.textColor = .white
        tip.drawsBackground = true
        tip.backgroundColor = NSColor.black.withAlphaComponent(0.75)
        stage.addSubview(tip)
        let move = { [self] (point: NSPoint) in
            guard let window = stage.window else { return }
            let local = stage.convert(window.convertPoint(fromScreen: point), from: nil)
            let share = share(at: local)
            onShare?(share)
            tip.stringValue = " \(Self.format(share)) \(row ? "|" : "/") \(Self.format(pair - share)) "
            tip.sizeToFit()
            tip.frame.origin = NSPoint(x: local.x + 12, y: local.y - 12 - tip.frame.height)
            stage.addSubview(tip)
        }
        move(trackMouse(move))
        tip.removeFromSuperview()
    }

    static func format(_ share: Double) -> String {
        guard let text = kanna_layout_fraction(share) else { return String(format: "%.2f", share) }
        defer { kanna_layout_free_string(text) }
        return String(cString: text)
    }
}

@MainActor
final class PaneGrip: NSView {
    static let size = NSSize(width: 34, height: 14)
    var onDrag: ((NSEvent) -> Void)?
    private var hovered = false { didSet { needsDisplay = true } }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        toolTip = "Drag to move this pane"
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        let inside = window.map { bounds.contains(convert($0.mouseLocationOutsideOfEventStream, from: nil)) } ?? false
        var options: NSTrackingArea.Options = [.mouseEnteredAndExited, .activeAlways, .inVisibleRect]
        if inside { options.insert(.assumeInside) }
        addTrackingArea(NSTrackingArea(rect: bounds, options: options, owner: self))
        hovered = inside
    }

    override func mouseEntered(with event: NSEvent) { hovered = true }
    override func mouseExited(with event: NSEvent) { hovered = false }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .openHand) }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) {
        NSCursor.closedHand.push()
        defer { NSCursor.pop() }
        onDrag?(event)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard hovered else { return }
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0, dy: -6).offsetBy(dx: 0, dy: 6), xRadius: 6, yRadius: 6)
        NSColor.controlAccentColor.setFill()
        shape.fill()
        NSColor.white.setFill()
        for i in -1...1 {
            NSBezierPath(ovalIn: NSRect(x: bounds.midX + CGFloat(i) * 6 - 1.5, y: bounds.midY - 1, width: 3, height: 3)).fill()
        }
    }
}
