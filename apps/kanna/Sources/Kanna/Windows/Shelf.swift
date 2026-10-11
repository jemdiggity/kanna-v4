import AppKit

struct ShelfTask: Equatable {
    let id: String
    let title: String
    let parkedAt: Double?
    let status: String?
    let detail: String

    var meta: String { status == "needs-you" ? "needs you" : "parked " + Self.ago(parkedAt) }

    static func ago(_ at: Double?) -> String {
        guard let at else { return "now" }
        let minutes = Int(max(0, Date().timeIntervalSince1970 - at) / 60)
        return minutes < 1 ? "now" : minutes < 60 ? "\(minutes)m ago" : minutes < 60 * 24 ? "\(minutes / 60)h ago" : "\(minutes / 60 / 24)d ago"
    }

    static func color(_ status: String?) -> NSColor {
        switch status {
        case "working": return .controlAccentColor
        case "needs-you": return .systemOrange
        case "idle": return .systemGreen
        default: return .tertiaryLabelColor
        }
    }
}

@MainActor
final class ShelfStrip: NSView {
    static let height: CGFloat = 30
    var onPull: ((String) -> Void)?
    private var shown: [ShelfTask] = []

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        paint()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        paint()
    }

    private func paint() { layer?.backgroundColor = resolved(.windowBackgroundColor) }

    var items: [(task: String, center: NSPoint)] {
        subviews.compactMap { $0 as? ShelfItem }.map { ($0.task.id, $0.convert(NSPoint(x: 20, y: $0.bounds.midY), to: nil)) }
    }

    func show(_ tasks: [ShelfTask]) {
        guard tasks != shown else { return }
        shown = tasks
        subviews.forEach { $0.removeFromSuperview() }
        var x: CGFloat = 8
        for (index, task) in tasks.enumerated() {
            let item = ShelfItem(task: task, key: index < 3 ? "⌘\(index + 1)" : nil)
            item.onPress = { [weak self] in self?.onPull?(task.id) }
            item.frame.origin = NSPoint(x: x, y: (Self.height - item.frame.height) / 2)
            addSubview(item)
            x += item.frame.width + 4
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.separatorColor.setFill()
        NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill()
    }
}

@MainActor
private final class ShelfItem: NSView {
    let task: ShelfTask
    var onPress: (() -> Void)?
    private let title: NSTextField
    private let extra: NSTextField
    private var hover = false

    init(task: ShelfTask, key: String?) {
        self.task = task
        title = NSTextField(labelWithString: task.title)
        extra = NSTextField(labelWithString: "· \(task.meta)" + (key.map { "  \($0)" } ?? ""))
        for label in [title, extra] {
            label.font = .systemFont(ofSize: 12)
            label.lineBreakMode = .byTruncatingTail
            label.sizeToFit()
        }
        title.textColor = task.status == "needs-you" ? .systemOrange : .secondaryLabelColor
        extra.textColor = .tertiaryLabelColor
        let width = min(title.frame.width, 260)
        super.init(frame: NSRect(x: 0, y: 0, width: 8 + 8 + 7 + width + 8, height: 22))
        title.frame = NSRect(x: 23, y: (22 - title.frame.height) / 2, width: width, height: title.frame.height)
        extra.frame = NSRect(x: title.frame.maxX + 6, y: title.frame.minY, width: extra.frame.width, height: extra.frame.height)
        extra.isHidden = true
        addSubview(title)
        addSubview(extra)
        toolTip = task.meta
        wantsLayer = true
        layer?.cornerRadius = 6
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func draw(_ dirtyRect: NSRect) {
        ShelfTask.color(task.status).setFill()
        NSBezierPath(ovalIn: NSRect(x: 9, y: bounds.midY - 4, width: 8, height: 8)).fill()
    }

    override func mouseEntered(with event: NSEvent) { setHover(true) }
    override func mouseExited(with event: NSEvent) { setHover(false) }

    private func setHover(_ on: Bool) {
        hover = on
        extra.isHidden = !on
        title.textColor = on ? .labelColor : task.status == "needs-you" ? .systemOrange : .secondaryLabelColor
        frame.size.width = (on ? extra.frame.maxX : title.frame.maxX) + 8
        layer?.backgroundColor = on ? resolved(.quaternaryLabelColor) : nil
        superview?.needsLayout = true
        (superview as? ShelfStrip)?.relayout()
    }

    override func mouseDown(with event: NSEvent) { onPress?() }
}

extension ShelfStrip {
    fileprivate func relayout() {
        var x: CGFloat = 8
        for item in subviews {
            item.frame.origin.x = x
            x += item.frame.width + 4
        }
    }
}

@MainActor
final class ShelfPage: NSView {
    static let cheers = [
        "Clear desk, clear mind. Pick what's next.",
        "Nice work. What's next?",
        "One thing at a time, and you're doing great.",
        "Fresh start. Pull a task when you're ready.",
        "Good progress today. Keep it going.",
        "Breathe. Then pick the next one.",
    ]
    var onPull: ((String) -> Void)?
    private var shown: (tasks: [ShelfTask], project: String, cheer: String)?

    override var isFlipped: Bool { true }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        paint()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        paint()
    }

    private func paint() { layer?.backgroundColor = resolved(.textBackgroundColor) }

    var rows: [(task: String, center: NSPoint)] {
        subviews.compactMap { $0 as? ShelfRow }.map { ($0.task.id, $0.convert(NSPoint(x: 20, y: $0.bounds.midY), to: nil)) }
    }

    func show(_ tasks: [ShelfTask], project: String, cheer: String) {
        if let shown, shown.tasks == tasks, shown.project == project, shown.cheer == cheer { return }
        shown = (tasks, project, cheer)
        subviews.forEach { $0.removeFromSuperview() }
        let left: CGFloat = 48, width = min(760, bounds.width - left * 2)
        var y: CGFloat = 40
        let line = NSTextField(labelWithString: cheer)
        line.font = .systemFont(ofSize: 13)
        line.textColor = .secondaryLabelColor
        line.sizeToFit()
        line.frame.origin = NSPoint(x: left, y: y)
        addSubview(line)
        y += line.frame.height + 22
        let head = NSTextField(labelWithString: project)
        head.font = .systemFont(ofSize: 12, weight: .semibold)
        head.sizeToFit()
        head.frame.origin = NSPoint(x: left, y: y)
        addSubview(head)
        let count = NSTextField(labelWithString: "\(tasks.count) parked")
        count.font = .systemFont(ofSize: 12)
        count.textColor = .secondaryLabelColor
        count.sizeToFit()
        count.frame.origin = NSPoint(x: left + width - count.frame.width, y: y)
        addSubview(count)
        y += head.frame.height + 8
        if tasks.isEmpty {
            let none = NSTextField(labelWithString: "No tasks. Start one with kanna open --dock --cmd claude.")
            none.font = .systemFont(ofSize: 12)
            none.textColor = .secondaryLabelColor
            none.sizeToFit()
            none.frame.origin = NSPoint(x: left, y: y + 6)
            addSubview(none)
        }
        for (index, task) in tasks.enumerated() {
            let row = ShelfRow(task: task, key: index < 3 ? "⌘\(index + 1)" : nil, width: width + 20)
            row.onPress = { [weak self] in self?.onPull?(task.id) }
            row.frame.origin = NSPoint(x: left - 10, y: y)
            addSubview(row)
            y += row.frame.height
        }
    }

    override func resizeSubviews(withOldSize oldSize: NSSize) {
        super.resizeSubviews(withOldSize: oldSize)
        guard let shown, oldSize.width != bounds.width else { return }
        self.shown = nil
        show(shown.tasks, project: shown.project, cheer: shown.cheer)
    }
}

@MainActor
private final class ShelfRow: NSView {
    let task: ShelfTask
    var onPress: (() -> Void)?
    private let go: NSTextField

    override var isFlipped: Bool { true }

    init(task: ShelfTask, key: String?, width: CGFloat) {
        self.task = task
        go = NSTextField(labelWithString: (key.map { "\($0) · " } ?? "") + "Pull →")
        super.init(frame: NSRect(x: 0, y: 0, width: width, height: 52))
        wantsLayer = true
        layer?.cornerRadius = 8
        let name = NSTextField(labelWithString: task.title)
        name.font = .systemFont(ofSize: 13, weight: .semibold)
        name.lineBreakMode = .byTruncatingTail
        name.frame = NSRect(x: 30, y: 8, width: width - 140, height: 18)
        let status = task.status.map { $0 == "needs-you" ? "needs you" : $0 }
        let sub = NSTextField(labelWithString: [status, task.detail.isEmpty ? nil : task.detail, "parked " + ShelfTask.ago(task.parkedAt)].compactMap { $0 }.joined(separator: " · "))
        sub.font = .systemFont(ofSize: 12)
        sub.textColor = .secondaryLabelColor
        sub.lineBreakMode = .byTruncatingTail
        sub.frame = NSRect(x: 30, y: 27, width: width - 140, height: 17)
        go.font = .systemFont(ofSize: 12)
        go.textColor = .controlAccentColor
        go.sizeToFit()
        go.frame.origin = NSPoint(x: width - go.frame.width - 10, y: (52 - go.frame.height) / 2)
        go.isHidden = true
        [name, sub, go].forEach(addSubview)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func draw(_ dirtyRect: NSRect) {
        ShelfTask.color(task.status).setFill()
        NSBezierPath(ovalIn: NSRect(x: 10, y: bounds.midY - 4, width: 8, height: 8)).fill()
    }

    override func mouseEntered(with event: NSEvent) {
        go.isHidden = false
        layer?.backgroundColor = resolved(.quaternaryLabelColor)
    }

    override func mouseExited(with event: NSEvent) {
        go.isHidden = true
        layer?.backgroundColor = nil
    }

    override func mouseDown(with event: NSEvent) { onPress?() }
}
