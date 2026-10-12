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

struct BacklogTask: Equatable {
    let id: String
    let name: String
    let message: String?
    let from: String?
    let madeAt: Double?

    var sub: String {
        let made = "made " + ShelfTask.ago(madeAt).replacingOccurrences(of: "now", with: "just now")
        return [message, from.map { "from \($0)" }, made].compactMap { $0 }.joined(separator: " · ")
    }
}

struct DeskPage: Equatable {
    var tasks: [ShelfTask] = []
    var backlog: [BacklogTask] = []
    var running = 0
    var project = ""
    var cheer = ""
    var note = ""

    static let limit = 3

    var full: Bool { running >= Self.limit }
}

@MainActor
final class ShelfStrip: NSView {
    static let height: CGFloat = 30
    var onPull: ((String) -> Void)?
    private var shown: [ShelfTask] = []
    private var waiting = 0
    private let count = NSTextField(labelWithString: "")

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        count.font = .systemFont(ofSize: 12)
        count.textColor = .tertiaryLabelColor
        count.autoresizingMask = [.minXMargin]
        paint()
    }

    var waitingText: String { count.isHidden ? "" : count.stringValue }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        paint()
    }

    private func paint() { layer?.backgroundColor = resolved(.windowBackgroundColor) }

    var items: [(task: String, center: NSPoint)] {
        subviews.compactMap { $0 as? ShelfItem }.map { ($0.task.id, $0.convert(NSPoint(x: 20, y: $0.bounds.midY), to: nil)) }
    }

    func show(_ tasks: [ShelfTask], waiting: Int) {
        guard tasks != shown || waiting != self.waiting else { return }
        shown = tasks
        self.waiting = waiting
        subviews.forEach { $0.removeFromSuperview() }
        count.stringValue = "\(waiting) in backlog"
        count.isHidden = waiting == 0
        count.sizeToFit()
        count.frame.origin = NSPoint(x: bounds.width - count.frame.width - 12, y: (Self.height - count.frame.height) / 2)
        addSubview(count)
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
        for item in subviews where item is ShelfItem {
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
    var onStart: ((String) -> Void)?
    var onBlank: (() -> Void)?
    private var shown: DeskPage?
    private let blank = NSButton(title: "Start a task  ⌘N", target: nil, action: nil)
    private let scroller = NSScrollView()
    private let document = FlippedView()

    override var isFlipped: Bool { true }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        scroller.frame = bounds
        scroller.autoresizingMask = [.width, .height]
        scroller.drawsBackground = false
        scroller.automaticallyAdjustsContentInsets = false
        scroller.hasVerticalScroller = true
        scroller.hasHorizontalScroller = false
        scroller.scrollerStyle = .overlay
        scroller.autohidesScrollers = true
        scroller.verticalScrollElasticity = .allowed
        scroller.horizontalScrollElasticity = .none
        scroller.documentView = document
        addSubview(scroller)
        blank.bezelStyle = .rounded
        blank.controlSize = .regular
        blank.target = self
        blank.action = #selector(startBlank)
        paint()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        paint()
    }

    private func paint() { layer?.backgroundColor = resolved(.textBackgroundColor) }

    @objc private func startBlank() { onBlank?() }

    var rows: [(task: String, center: NSPoint)] {
        document.subviews.compactMap { $0 as? ShelfRow }.map { ($0.task.id, $0.convert(NSPoint(x: 20, y: $0.bounds.midY), to: nil)) }
    }

    var backlogRows: [(task: String, center: NSPoint)] {
        document.subviews.compactMap { $0 as? BacklogRow }.map { ($0.task.id, $0.convert(NSPoint(x: 20, y: $0.bounds.midY), to: nil)) }
    }

    var blankCenter: NSPoint? { blank.superview == nil || !blank.isEnabled ? nil : blank.convert(NSPoint(x: blank.bounds.midX, y: blank.bounds.midY), to: nil) }

    var visibleBacklog: [String] {
        let seen = document.visibleRect
        return document.subviews.compactMap { $0 as? BacklogRow }.filter { seen.contains($0.frame) }.map(\.task.id)
    }

    var scrollTop: CGFloat { scroller.contentView.bounds.minY }
    var contentHeight: CGFloat { document.frame.height }
    var visibleHeight: CGFloat { scroller.contentView.bounds.height }

    func scroll(toY y: CGFloat) {
        scroller.contentView.scroll(to: NSPoint(x: 0, y: min(max(0, y), max(0, document.frame.height - scroller.contentView.bounds.height))))
        scroller.reflectScrolledClipView(scroller.contentView)
    }

    var note: String { document.subviews.compactMap { $0 as? NSTextField }.first { $0.identifier?.rawValue == "note" }?.stringValue ?? "" }

    private func label(_ text: String, size: CGFloat = 12, weight: NSFont.Weight = .regular, color: NSColor = .labelColor) -> NSTextField {
        let field = NSTextField(labelWithString: text)
        field.font = .systemFont(ofSize: size, weight: weight)
        field.textColor = color
        field.sizeToFit()
        document.addSubview(field)
        return field
    }

    func show(_ page: DeskPage) {
        if page == shown { return }
        shown = page
        lay(page, top: scrollTop)
    }

    private func lay(_ page: DeskPage, top: CGFloat) {
        document.subviews.forEach { $0.removeFromSuperview() }
        let left: CGFloat = 48, width = min(760, scroller.contentSize.width - left * 2)
        var y: CGFloat = 40
        let line = label(page.cheer, size: 13, color: .secondaryLabelColor)
        line.frame.origin = NSPoint(x: left, y: y)
        y += line.frame.height + 22
        y = header("Shelf", detail: page.project, count: "\(page.running) of \(DeskPage.limit) tasks", y: y, left: left, width: width)
        if page.tasks.isEmpty {
            let none = label("Nothing on the Shelf.", color: .secondaryLabelColor)
            none.frame.origin = NSPoint(x: left, y: y + 4)
            y += none.frame.height + 12
        }
        for (index, task) in page.tasks.enumerated() {
            let row = ShelfRow(task: task, key: index < 3 ? "⌘\(index + 1)" : nil, width: width + 20)
            row.onPress = { [weak self] in self?.onPull?(task.id) }
            row.frame.origin = NSPoint(x: left - 10, y: y)
            document.addSubview(row)
            y += row.frame.height
        }
        blank.isEnabled = !page.full
        blank.toolTip = page.full ? "Three tasks already run: finish one first" : "A new agent in a worktree of its own; tell it what to do and it names the task"
        blank.sizeToFit()
        blank.frame.origin = NSPoint(x: left - 6, y: y + 4)
        document.addSubview(blank)
        y += blank.frame.height + 28
        y = header("Backlog", detail: nil, count: "\(page.backlog.count) waiting", y: y, left: left, width: width)
        if page.backlog.isEmpty {
            let none = label("Empty. Ask the planning agent for tasks.", color: .secondaryLabelColor)
            none.frame.origin = NSPoint(x: left, y: y + 4)
            y += none.frame.height + 12
        }
        for task in page.backlog {
            let row = BacklogRow(task: task, full: page.full, width: width + 20)
            row.onPress = { [weak self] in self?.onStart?(task.id) }
            row.frame.origin = NSPoint(x: left - 10, y: y)
            document.addSubview(row)
            y += row.frame.height
        }
        let note = label(page.note, color: .systemOrange)
        note.identifier = NSUserInterfaceItemIdentifier("note")
        note.frame.origin = NSPoint(x: left, y: y + 8)
        document.frame = NSRect(x: 0, y: 0, width: scroller.contentSize.width, height: y + 8 + note.frame.height + 40)
        scroll(toY: top)
    }

    private func header(_ title: String, detail: String?, count: String, y: CGFloat, left: CGFloat, width: CGFloat) -> CGFloat {
        let head = label(title, weight: .semibold)
        head.frame.origin = NSPoint(x: left, y: y)
        if let detail {
            let more = label("· " + detail, color: .secondaryLabelColor)
            more.frame.origin = NSPoint(x: head.frame.maxX + 4, y: y)
        }
        let tally = label(count, color: .secondaryLabelColor)
        tally.frame.origin = NSPoint(x: left + width - tally.frame.width, y: y)
        return y + head.frame.height + 8
    }

    override func resizeSubviews(withOldSize oldSize: NSSize) {
        let top = scrollTop
        super.resizeSubviews(withOldSize: oldSize)
        guard let shown else { return }
        if oldSize.width != bounds.width { lay(shown, top: top) } else { scroll(toY: top) }
    }
}

private final class FlippedView: NSView {
    override var isFlipped: Bool { true }
}

@MainActor
private final class BacklogRow: NSView {
    let task: BacklogTask
    var onPress: (() -> Void)?
    private let go: NSTextField
    private let full: Bool

    override var isFlipped: Bool { true }

    init(task: BacklogTask, full: Bool, width: CGFloat) {
        self.task = task
        self.full = full
        go = NSTextField(labelWithString: full ? "Shelf full" : "Move to Shelf →")
        super.init(frame: NSRect(x: 0, y: 0, width: width, height: 52))
        wantsLayer = true
        layer?.cornerRadius = 8
        let name = NSTextField(labelWithString: task.name)
        name.font = .systemFont(ofSize: 13, weight: .medium)
        name.lineBreakMode = .byTruncatingTail
        name.frame = NSRect(x: 30, y: 8, width: width - 160, height: 18)
        let sub = NSTextField(labelWithString: task.sub)
        sub.font = .systemFont(ofSize: 12)
        sub.textColor = .secondaryLabelColor
        sub.lineBreakMode = .byTruncatingTail
        sub.frame = NSRect(x: 30, y: 27, width: width - 160, height: 17)
        go.font = .systemFont(ofSize: 12)
        go.textColor = .tertiaryLabelColor
        go.sizeToFit()
        go.frame.origin = NSPoint(x: width - go.frame.width - 10, y: (52 - go.frame.height) / 2)
        [name, sub, go].forEach(addSubview)
        toolTip = task.message
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.tertiaryLabelColor.setStroke()
        let ring = NSBezierPath(ovalIn: NSRect(x: 10.5, y: bounds.midY - 4, width: 8, height: 8))
        ring.lineWidth = 1.4
        ring.stroke()
    }

    override func mouseEntered(with event: NSEvent) {
        layer?.backgroundColor = resolved(.quaternaryLabelColor)
        if !full { go.textColor = .controlAccentColor }
    }

    override func mouseExited(with event: NSEvent) {
        layer?.backgroundColor = nil
        go.textColor = .tertiaryLabelColor
    }

    override func mouseDown(with event: NSEvent) { onPress?() }
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
