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
    var onRename: ((String, String) -> Void)?
    var onEditEnded: (() -> Void)?
    private var shown: [(id: String, title: String, active: Bool)] = []

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(_ model: Model, _ window: Window) {
        let tabs = window.tabs.map { (id: $0.id, title: $0.title, active: $0.id == window.active) }
        guard tabs.map(\.id) != shown.map(\.id) || tabs.map(\.title) != shown.map(\.title) || tabs.map(\.active) != shown.map(\.active) else { return }
        shown = tabs
        subviews.forEach { $0.removeFromSuperview() }
        var x: CGFloat = 8
        for tab in tabs {
            let item = TabItem(id: tab.id, title: tab.title, active: tab.active)
            item.onSelect = { [weak self] in self?.onSelect?(tab.id) }
            item.onClose = { [weak self] in self?.onClose?(tab.id) }
            item.onRename = { [weak self] title in self?.onRename?(tab.id, title) }
            item.onEditEnded = { [weak self] in self?.onEditEnded?() }
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

    /// Where a tab is drawn, in window coordinates (for debug.click).
    func center(of id: String) -> NSPoint? {
        guard let item = subviews.compactMap({ $0 as? TabItem }).first(where: { $0.id == id }) else { return nil }
        return item.convert(NSPoint(x: item.bounds.midX - 8, y: item.bounds.midY), to: nil)
    }

    @objc private func newTab() { onNew?() }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.separatorColor.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: 1).fill()
    }
}

@MainActor
private final class TabItem: NSView, NSTextFieldDelegate {
    let id: String
    var onSelect: (() -> Void)?
    var onClose: (() -> Void)?
    var onRename: ((String) -> Void)?
    var onEditEnded: (() -> Void)?
    private let active: Bool
    private let label: NSTextField
    private var editor: NSTextField?
    private var cancelled = false

    init(id: String, title: String, active: Bool) {
        self.id = id
        self.active = active
        label = NSTextField(labelWithString: title)
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

    override func mouseDown(with event: NSEvent) {
        if event.clickCount == 2 { startEditing() } else { onSelect?() }
    }

    // Renaming: double-click; Enter or clicking away saves, Escape cancels.
    private func startEditing() {
        guard editor == nil else { return }
        let field = NSTextField(string: label.stringValue)
        field.font = label.font
        field.frame = label.frame.insetBy(dx: -3, dy: -2)
        field.frame.size.width = max(field.frame.width, 90)
        field.focusRingType = .exterior
        field.delegate = self
        cancelled = false
        label.isHidden = true
        addSubview(field)
        editor = field
        window?.makeFirstResponder(field)
        field.currentEditor()?.selectAll(nil)
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard selector == #selector(NSResponder.cancelOperation(_:)) else { return false }
        cancelled = true
        window?.makeFirstResponder(nil)
        return true
    }

    func controlTextDidEndEditing(_ notification: Notification) {
        guard let field = editor else { return }
        editor = nil
        field.removeFromSuperview()
        label.isHidden = false
        let title = field.stringValue.trimmingCharacters(in: .whitespaces)
        if !cancelled, !title.isEmpty, title != label.stringValue { onRename?(title) }
        // Hand the keyboard back to the terminal.
        window?.makeFirstResponder(nil)
        onEditEnded?()
    }

    @objc private func closeClicked() { onClose?() }
}
