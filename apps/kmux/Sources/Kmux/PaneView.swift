import AppKit
import KmuxCore

/// A pane on screen: its content, with no chrome except a focus outline and
/// a notice once it exits or fails.
@MainActor
final class PaneView: NSView {
    let id: String
    let content: NSView
    private let notice = NSTextField(labelWithString: "")
    private let outline = NSView()

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
    }
}
