import AppKit

enum DocTheme {
    static let bar = color(dark: 0x202226, light: 0xf4f4f6)
    static let panel = color(dark: 0x1c1d22, light: 0xf7f7f9)
    static let card = color(dark: 0x23252b, light: 0xffffff)
    static let line = color(dark: 0x2c2e34, light: 0xdedfe3)
    static let edge = color(dark: 0x3a3d44, light: 0xc9cbd1)
    static let text = color(dark: 0xe6e6e6, light: 0x1d1f23)
    static let soft = color(dark: 0xc9ccd3, light: 0x3a3d44)
    static let dim = color(dark: 0x8b8f98, light: 0x7a7e87)
    static let accent = color(dark: 0x3478f6, light: 0x2f6fe4)
    static let ok = color(dark: 0x4cb782, light: 0x2f9a63)
    static let warn = color(dark: 0xd9a441, light: 0xb9801b)
    static let agent = color(dark: 0xd18ae6, light: 0x9a45c4)
    static let tint = color(dark: 0x3478f6, light: 0x2f6fe4, alpha: (0.2, 0.16))
    static let tintOn = color(dark: 0x3478f6, light: 0x2f6fe4, alpha: (0.42, 0.34))
    static let resolvedTint = NSColor(srgbRed: 0.5, green: 0.5, blue: 0.5, alpha: 0.16)

    static var sans: NSFont { NSFont.systemFont(ofSize: 13) }
    static var small: NSFont { NSFont.systemFont(ofSize: 12) }
    static var tiny: NSFont { NSFont.systemFont(ofSize: 11.5) }
    static var mono: NSFont { NSFont.monospacedSystemFont(ofSize: 11.5, weight: .regular) }

    private static func color(dark: UInt32, light: UInt32, alpha: (CGFloat, CGFloat) = (1, 1)) -> NSColor {
        NSColor(name: nil) { appearance in
            let isDark = MarkdownTheme.isDark(appearance)
            let value = isDark ? dark : light
            return NSColor(srgbRed: CGFloat((value >> 16) & 0xff) / 255, green: CGFloat((value >> 8) & 0xff) / 255,
                           blue: CGFloat(value & 0xff) / 255, alpha: isDark ? alpha.0 : alpha.1)
        }
    }

    static func paragraph(_ lineHeight: CGFloat, size: CGFloat) -> NSParagraphStyle {
        let style = NSMutableParagraphStyle()
        style.minimumLineHeight = (size * lineHeight).rounded()
        style.maximumLineHeight = (size * lineHeight).rounded()
        return style
    }
}

@MainActor
class FlippedView: NSView {
    override var isFlipped: Bool { true }
    var fill: NSColor? { didSet { needsDisplay = true } }
    var stroke: NSColor? { didSet { needsDisplay = true } }
    var radius: CGFloat = 0

    override func draw(_ dirtyRect: NSRect) {
        if let fill {
            fill.setFill()
            NSBezierPath(roundedRect: bounds, xRadius: radius, yRadius: radius).fill()
        }
        if let stroke {
            stroke.setStroke()
            NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: radius, yRadius: radius).stroke()
        }
    }
}

@MainActor
final class Chip: FlippedView {
    let label = NSTextField(labelWithString: "")
    var onPress: (() -> Void)?
    var on = false { didSet { paint() } }
    var warn = false { didSet { paint() } }

    init(_ text: String) {
        super.init(frame: .zero)
        radius = 5
        label.font = DocTheme.small
        label.lineBreakMode = .byTruncatingTail
        addSubview(label)
        set(text)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func set(_ text: String) {
        label.stringValue = text
        label.sizeToFit()
        frame.size = NSSize(width: ceil(label.frame.width) + 18, height: 22)
        label.frame.origin = NSPoint(x: 9, y: ((22 - label.frame.height) / 2).rounded())
        paint()
    }

    private func paint() {
        stroke = warn ? DocTheme.warn : on ? DocTheme.accent : DocTheme.edge
        label.textColor = warn ? DocTheme.warn : on ? DocTheme.text : DocTheme.soft
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        paint()
    }

    override func mouseDown(with event: NSEvent) {
        if let onPress { onPress() } else { super.mouseDown(with: event) }
    }
}

@MainActor
final class DocBar: FlippedView {
    let path = NSTextField(labelWithString: "")
    let agent = Chip("")
    let comment = Chip("Comment ⌘↵")
    let threads = Chip("Threads")

    init() {
        super.init(frame: .zero)
        fill = DocTheme.bar
        path.font = DocTheme.mono
        path.textColor = DocTheme.soft
        path.lineBreakMode = .byTruncatingTail
        for view in [path, agent, comment, threads] { addSubview(view) }
        comment.toolTip = "Select text, then ⌘↵. Or ⌘-click a word."
        threads.on = true
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func layout() {
        super.layout()
        var x = bounds.width - 10
        for chip in [threads, comment, agent] {
            x -= chip.frame.width
            chip.frame.origin = NSPoint(x: x, y: 6)
            x -= 10
        }
        path.sizeToFit()
        path.frame = NSRect(x: 10, y: ((bounds.height - path.frame.height) / 2).rounded(), width: max(0, min(path.frame.width, x - 10)), height: path.frame.height)
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        DocTheme.line.setFill()
        NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill()
    }
}

@MainActor
final class BoxField: NSTextView {
    var onKey: ((NSEvent) -> Bool)?
    var placeholder = ""

    convenience init(placeholder: String) {
        self.init(frame: NSRect(x: 0, y: 0, width: 100, height: 28))
        self.placeholder = placeholder
        font = DocTheme.sans
        textColor = DocTheme.text
        insertionPointColor = DocTheme.text
        drawsBackground = false
        isRichText = false
        allowsUndo = true
        textContainerInset = .zero
        textContainer?.lineFragmentPadding = 0
        isAutomaticQuoteSubstitutionEnabled = false
        isAutomaticDashSubstitutionEnabled = false
    }

    override func keyDown(with event: NSEvent) {
        if onKey?(event) == true { return }
        super.keyDown(with: event)
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if window?.firstResponder === self, event.modifierFlags.contains(.command), onKey?(event) == true { return true }
        return super.performKeyEquivalent(with: event)
    }

    override func didChangeText() {
        super.didChangeText()
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        if string.isEmpty {
            (placeholder as NSString).draw(at: NSPoint(x: 0, y: 0), withAttributes: [.font: font ?? DocTheme.sans, .foregroundColor: DocTheme.dim])
        }
    }

    var trimmed: String { string.trimmingCharacters(in: .whitespacesAndNewlines) }
}

@MainActor
func messageViews(_ thread: DocThread, width: CGFloat) -> [NSView] {
    thread.messages.map { message in
        let agent = message.who == "agent"
        let label = NSTextField(wrappingLabelWithString: "")
        label.attributedStringValue = NSAttributedString(string: message.text, attributes: [
            .font: DocTheme.sans, .foregroundColor: agent ? DocTheme.soft : DocTheme.text, .paragraphStyle: DocTheme.paragraph(1.45, size: 13),
        ])
        label.isSelectable = true
        let inset: CGFloat = agent ? 12 : 0
        let size = label.sizeThatFits(NSSize(width: width - inset, height: .greatestFiniteMagnitude))
        let holder = FlippedView(frame: NSRect(x: 0, y: 0, width: width, height: ceil(size.height)))
        label.frame = NSRect(x: inset, y: 0, width: width - inset, height: ceil(size.height))
        holder.addSubview(label)
        if agent {
            let bar = FlippedView(frame: NSRect(x: 0, y: 0, width: 2, height: ceil(size.height)))
            bar.fill = DocTheme.agent
            holder.addSubview(bar)
        }
        return holder
    }
}

@MainActor
func stack(_ views: [NSView], in host: NSView, x: CGFloat, y: CGFloat, gap: CGFloat) -> CGFloat {
    var y = y
    for (index, view) in views.enumerated() {
        if index > 0 { y += gap }
        view.frame.origin = NSPoint(x: x, y: y)
        host.addSubview(view)
        y += view.frame.height
    }
    return y
}

@MainActor
func noteLabel(_ text: String, width: CGFloat) -> NSTextField {
    let label = NSTextField(wrappingLabelWithString: text)
    label.font = DocTheme.tiny
    label.textColor = DocTheme.warn
    label.frame.size = NSSize(width: width, height: ceil(label.sizeThatFits(NSSize(width: width, height: .greatestFiniteMagnitude)).height))
    return label
}

@MainActor
final class ActButton: FlippedView {
    let label = NSTextField(labelWithString: "")
    var onPress: (() -> Void)?
    private var hover = false

    init(resolve: Bool) {
        super.init(frame: NSRect(x: 0, y: 0, width: 20, height: 20))
        radius = 4
        label.stringValue = resolve ? "✓" : "↺"
        label.font = DocTheme.small
        label.textColor = DocTheme.dim
        label.alignment = .center
        label.frame = NSRect(x: 0, y: 2, width: 20, height: 16)
        addSubview(label)
        toolTip = resolve ? "Resolve" : "Reopen"
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func mouseEntered(with event: NSEvent) { hover = true; paint() }
    override func mouseExited(with event: NSEvent) { hover = false; paint() }
    private func paint() {
        fill = hover ? DocTheme.panel : nil
        label.textColor = hover ? DocTheme.ok : DocTheme.dim
    }

    override func mouseDown(with event: NSEvent) { onPress?() }
}

@MainActor
final class CommentBox: FlippedView {
    static let width: CGFloat = 360
    let field: BoxField
    var threadID: String?

    init(composer: Void) {
        field = BoxField(placeholder: "Comment")
        super.init(frame: NSRect(x: 0, y: 0, width: Self.width, height: 100))
        decorate()
        field.frame = NSRect(x: 8, y: 8, width: Self.width - 16, height: 54)
        addSubview(field)
        let hint = NSTextField(labelWithString: "⌘↵ posts · Esc cancels")
        hint.font = DocTheme.tiny
        hint.textColor = DocTheme.dim
        hint.sizeToFit()
        let post = Chip("Post")
        post.fill = DocTheme.accent
        post.stroke = DocTheme.accent
        post.label.textColor = .white
        post.identifier = NSUserInterfaceItemIdentifier("post")
        post.frame.origin = NSPoint(x: Self.width - 8 - post.frame.width, y: 66)
        hint.frame.origin = NSPoint(x: post.frame.minX - 6 - hint.frame.width, y: 66 + ((22 - hint.frame.height) / 2).rounded())
        addSubview(hint)
        addSubview(post)
        frame.size.height = 96
    }

    init(thread: DocThread, note: String?) {
        field = BoxField(placeholder: "Reply")
        super.init(frame: NSRect(x: 0, y: 0, width: Self.width, height: 100))
        threadID = thread.id
        decorate()
        let width = Self.width - 12 - 32
        var y = stack(messageViews(thread, width: width), in: self, x: 12, y: 10, gap: 6)
        if let note {
            y = stack([noteLabel(note, width: width)], in: self, x: 12, y: y + 4, gap: 0)
        }
        let act = ActButton(resolve: thread.isOpen)
        act.identifier = NSUserInterfaceItemIdentifier("act")
        act.frame.origin = NSPoint(x: Self.width - 28, y: 8)
        addSubview(act)
        let rule = FlippedView(frame: NSRect(x: 12, y: y + 8, width: Self.width - 24, height: 1))
        rule.fill = DocTheme.line
        addSubview(rule)
        field.frame = NSRect(x: 12, y: y + 15, width: Self.width - 24, height: 18)
        addSubview(field)
        frame.size.height = y + 15 + 18 + 8
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    private func decorate() {
        fill = DocTheme.card
        stroke = DocTheme.accent
        radius = 8
        wantsLayer = true
        shadow = {
            let shadow = NSShadow()
            shadow.shadowColor = NSColor.black.withAlphaComponent(0.28)
            shadow.shadowBlurRadius = 14
            shadow.shadowOffset = NSSize(width: 0, height: -8)
            return shadow
        }()
    }

    func button(_ name: String) -> NSView? { subviews.first { $0.identifier?.rawValue == name } }
}

@MainActor
final class ThreadCard: FlippedView {
    let thread: DocThread
    let field = BoxField(placeholder: "Reply")
    let act: ActButton
    var onSelect: (() -> Void)?
    private let active: Bool
    private var hover = false

    init(thread: DocThread, active: Bool, note: String?, width: CGFloat) {
        self.thread = thread
        self.active = active
        act = ActButton(resolve: thread.isOpen)
        super.init(frame: NSRect(x: 0, y: 0, width: width, height: 10))
        radius = 6
        let inner = width - 10 - 30
        var y = stack(messageViews(thread, width: inner), in: self, x: 10, y: 8, gap: 6)
        if let note { y = stack([noteLabel(note, width: inner)], in: self, x: 10, y: y + 4, gap: 0) }
        if active {
            let rule = FlippedView(frame: NSRect(x: 10, y: y + 8, width: inner, height: 1))
            rule.fill = DocTheme.line
            addSubview(rule)
            field.font = NSFont.systemFont(ofSize: 12.5)
            field.frame = NSRect(x: 10, y: y + 14, width: inner, height: 18)
            addSubview(field)
            y += 14 + 18
        }
        act.frame.origin = NSPoint(x: width - 26, y: 6)
        addSubview(act)
        frame.size.height = y + 8
        alphaValue = thread.isOpen ? 1 : 0.55
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
        paint()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func mouseEntered(with event: NSEvent) { hover = true; paint() }
    override func mouseExited(with event: NSEvent) { hover = false; paint() }

    private func paint() {
        fill = hover || active ? DocTheme.card : nil
        act.isHidden = !(hover || active)
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        if active {
            DocTheme.accent.setFill()
            NSRect(x: 0, y: 0, width: 2, height: bounds.height).fill()
        }
    }

    override func mouseDown(with event: NSEvent) {
        if !active { onSelect?() }
    }
}

@MainActor
final class ThreadPanel: FlippedView {
    static let width: CGFloat = 280
    let scroll = NSScrollView()
    private let content = FlippedView()
    var onSelect: ((String) -> Void)?
    var onAct: ((DocThread) -> Void)?
    var onReply: ((String, String) -> Void)?
    var showResolved = false
    private(set) var cards: [ThreadCard] = []
    private var last: (threads: [DocThread], active: String?, notes: [String: String], showResolved: Bool, width: CGFloat)?

    init() {
        super.init(frame: .zero)
        fill = DocTheme.panel
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.documentView = content
        addSubview(scroll)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func layout() {
        super.layout()
        scroll.frame = NSRect(x: 1, y: 0, width: bounds.width - 1, height: bounds.height)
        if let last, last.width != bounds.width { show(last.threads, active: last.active, notes: last.notes) }
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        DocTheme.line.setFill()
        NSRect(x: 0, y: 0, width: 1, height: bounds.height).fill()
    }

    func show(_ threads: [DocThread], active: String?, notes: [String: String]) {
        let width = bounds.width - 1
        if let last, last.threads == threads, last.active == active, last.notes == notes, last.showResolved == showResolved, last.width == bounds.width { return }
        last = (threads, active, notes, showResolved, bounds.width)
        let drafts = Dictionary(cards.map { ($0.thread.id, $0.field.string) }, uniquingKeysWith: { a, _ in a })
        let focused = cards.first { window?.firstResponder === $0.field }?.thread.id
        content.subviews.forEach { $0.removeFromSuperview() }
        cards = []
        let open = threads.filter(\.isOpen), resolved = threads.filter { !$0.isOpen }
        var y: CGFloat = 10
        if !resolved.isEmpty {
            let link = NSTextField(labelWithString: "\(showResolved ? "Hide" : "Show") resolved (\(resolved.count))")
            link.font = DocTheme.tiny
            link.textColor = DocTheme.dim
            link.sizeToFit()
            let button = Clickable(frame: NSRect(x: width - 8 - link.frame.width, y: y, width: link.frame.width, height: link.frame.height))
            link.frame.origin = .zero
            button.addSubview(link)
            button.onPress = { [weak self] in
                guard let self else { return }
                showResolved.toggle()
                last = nil
                show(threads, active: active, notes: notes)
            }
            content.addSubview(button)
        }
        y += 16
        let shown = open + (showResolved ? resolved : [])
        if shown.isEmpty {
            let empty = NSTextField(wrappingLabelWithString: "No comments yet. Select text and press ⌘↵, or ⌘-click a word.")
            empty.font = DocTheme.small
            empty.textColor = DocTheme.dim
            empty.frame = NSRect(x: 10, y: y + 8, width: width - 20, height: ceil(empty.sizeThatFits(NSSize(width: width - 20, height: .greatestFiniteMagnitude)).height))
            content.addSubview(empty)
            y = empty.frame.maxY
        }
        for thread in shown {
            let card = ThreadCard(thread: thread, active: thread.id == active, note: notes[thread.id], width: width - 16)
            card.frame.origin = NSPoint(x: 8, y: y)
            card.onSelect = { [weak self] in self?.onSelect?(thread.id) }
            card.act.onPress = { [weak self] in self?.onAct?(thread) }
            card.field.string = drafts[thread.id] ?? ""
            card.field.onKey = { [weak self, weak card] event in
                guard let card, event.keyCode == 36, !event.modifierFlags.contains(.shift) else { return false }
                let text = card.field.trimmed
                if !text.isEmpty {
                    card.field.string = ""
                    self?.onReply?(thread.id, text)
                }
                return true
            }
            content.addSubview(card)
            cards.append(card)
            y += card.frame.height + 2
        }
        content.frame = NSRect(x: 0, y: 0, width: width, height: max(y + 10, scroll.contentSize.height))
        if let focused, let card = cards.first(where: { $0.thread.id == focused && $0.field.superview != nil }) {
            window?.makeFirstResponder(card.field)
            card.field.setSelectedRange(NSRange(location: (card.field.string as NSString).length, length: 0))
        }
    }

    func reveal(_ id: String) {
        guard let card = cards.first(where: { $0.thread.id == id }) else { return }
        content.scrollToVisible(card.frame)
    }
}

@MainActor
final class Clickable: FlippedView {
    var onPress: (() -> Void)?
    override func mouseDown(with event: NSEvent) { onPress?() }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .pointingHand) }
}
