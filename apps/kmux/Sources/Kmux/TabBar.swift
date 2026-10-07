import AppKit
import KmuxCore

/// The row of tabs under the title bar: each tab's title with a close
/// button, the active one underlined, and + for a new tab.
@MainActor
final class TabBar: NSView {
    static let height: CGFloat = 28

    var onSelect: ((String) -> Void)?
    var onClose: ((String) -> Void)?
    var onNew: (() -> Void)?
    private var shown: [(id: String, title: String, active: Bool)] = []

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(_ model: Model, _ window: Window) {
        let tabs = window.tabs.map { (id: $0.id, title: model.title(of: $0), active: $0.id == window.active) }
        guard tabs.map(\.id) != shown.map(\.id) || tabs.map(\.title) != shown.map(\.title) || tabs.map(\.active) != shown.map(\.active) else { return }
        shown = tabs
        subviews.forEach { $0.removeFromSuperview() }
        var x: CGFloat = 8
        for tab in tabs {
            let item = TabItem(id: tab.id, title: tab.title, active: tab.active)
            item.onSelect = { [weak self] in self?.onSelect?(tab.id) }
            item.onClose = { [weak self] in self?.onClose?(tab.id) }
            item.frame.origin = NSPoint(x: x, y: 0)
            addSubview(item)
            x += item.frame.width + 2
        }
        let plus = NSButton(title: "+", target: self, action: #selector(newTab))
        plus.isBordered = false
        plus.font = .systemFont(ofSize: 15)
        plus.contentTintColor = .secondaryLabelColor
        plus.frame = NSRect(x: x + 4, y: 3, width: 22, height: 22)
        plus.toolTip = "New tab"
        addSubview(plus)
    }

    @objc private func newTab() { onNew?() }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.separatorColor.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: 1).fill()
    }
}

@MainActor
private final class TabItem: NSView {
    var onSelect: (() -> Void)?
    var onClose: (() -> Void)?
    private let active: Bool

    init(id: String, title: String, active: Bool) {
        self.active = active
        let label = NSTextField(labelWithString: title)
        label.font = .systemFont(ofSize: 12, weight: active ? .semibold : .regular)
        label.textColor = active ? .labelColor : .secondaryLabelColor
        label.lineBreakMode = .byTruncatingTail
        label.sizeToFit()
        let width = min(max(label.frame.width, 40), 220)
        super.init(frame: NSRect(x: 0, y: 0, width: width + 34, height: TabBar.height))
        label.frame = NSRect(x: 10, y: (TabBar.height - label.frame.height) / 2, width: width, height: label.frame.height)
        addSubview(label)
        let close = NSButton(title: "×", target: self, action: #selector(closeClicked))
        close.isBordered = false
        close.font = .systemFont(ofSize: 13)
        close.contentTintColor = .tertiaryLabelColor
        close.frame = NSRect(x: width + 14, y: (TabBar.height - 18) / 2, width: 16, height: 18)
        close.toolTip = "Close tab"
        addSubview(close)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func draw(_ dirtyRect: NSRect) {
        guard active else { return }
        NSColor.controlAccentColor.setFill()
        NSRect(x: 6, y: 1, width: bounds.width - 12, height: 2).fill()
    }

    override func mouseDown(with event: NSEvent) { onSelect?() }

    @objc private func closeClicked() { onClose?() }
}
