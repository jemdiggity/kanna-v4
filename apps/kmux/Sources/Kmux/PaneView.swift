import AppKit
import KmuxCore

/// A pane on screen: its content, with no chrome except a close button on
/// hover, a focus outline, and a notice once it exits or fails.
@MainActor
final class PaneView: NSView {
    let id: String
    let content: NSView
    var onClose: (() -> Void)?
    private let notice = NSTextField(labelWithString: "")
    private let closeButton = NSButton()
    private let outline = NSView()
    private var hover: NSTrackingArea?

    init(id: String, content: NSView) {
        self.id = id
        self.content = content
        super.init(frame: .zero)
        addSubview(content)

        notice.font = .systemFont(ofSize: 13, weight: .medium)
        notice.textColor = .white
        notice.alignment = .center
        notice.wantsLayer = true
        notice.drawsBackground = true
        notice.backgroundColor = NSColor.black.withAlphaComponent(0.65)
        notice.isHidden = true
        addSubview(notice)

        outline.wantsLayer = true
        outline.layer?.borderWidth = 2
        outline.layer?.borderColor = NSColor.controlAccentColor.withAlphaComponent(0.8).cgColor
        outline.isHidden = true
        addSubview(outline)

        closeButton.image = NSImage(systemSymbolName: "xmark.circle.fill", accessibilityDescription: "Close pane")
        closeButton.isBordered = false
        closeButton.contentTintColor = .secondaryLabelColor
        closeButton.target = self
        closeButton.action = #selector(closeClicked)
        closeButton.isHidden = true
        addSubview(closeButton)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    var focused = false { didSet { outline.isHidden = !focused } }

    func show(_ pane: Pane) {
        switch pane.state {
        case .exited: notice.stringValue = "  exited\(pane.exitCode.map { " (\($0))" } ?? "")  "
        case .failed: notice.stringValue = "  failed: \(pane.error ?? "unknown error")  "
        default: notice.stringValue = ""
        }
        notice.isHidden = notice.stringValue.isEmpty
        needsLayout = true
    }

    override func layout() {
        super.layout()
        content.frame = bounds
        outline.frame = bounds
        notice.sizeToFit()
        notice.frame.origin = NSPoint(x: (bounds.width - notice.frame.width) / 2, y: 12)
        closeButton.frame = NSRect(x: bounds.width - 24, y: bounds.height - 24, width: 18, height: 18)
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let hover { removeTrackingArea(hover) }
        hover = NSTrackingArea(rect: bounds, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(hover!)
    }

    override func mouseEntered(with event: NSEvent) { closeButton.isHidden = false }
    override func mouseExited(with event: NSEvent) { closeButton.isHidden = true }

    @objc private func closeClicked() { onClose?() }
}
