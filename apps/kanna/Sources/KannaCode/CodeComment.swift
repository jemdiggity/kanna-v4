import AppKit

public struct CodeComment: Equatable, Sendable {
    public let first: Int
    public let last: Int
    public let text: String

    public init(first: Int, last: Int, text: String) {
        self.first = first
        self.last = max(last, first)
        self.text = text
    }

    func contains(_ line: Int) -> Bool { line >= first && line <= last }
}

@MainActor
final class CodeCommentBox: NSView {
    static let widest: CGFloat = 620
    static let padding = NSSize(width: 11, height: 7)
    static let margin: CGFloat = 6

    private let heading = NSTextField(labelWithString: "")
    private let body = NSTextField(wrappingLabelWithString: "")

    init(comment: CodeComment) {
        super.init(frame: .zero)
        wantsLayer = true
        heading.font = .systemFont(ofSize: 11)
        heading.stringValue = comment.last > comment.first ? "lines \(comment.first)–\(comment.last)" : "line \(comment.first)"
        body.font = .systemFont(ofSize: 12.5)
        body.stringValue = comment.text
        body.isSelectable = true
        for label in [heading, body] {
            label.translatesAutoresizingMaskIntoConstraints = true
            addSubview(label)
        }
        paint()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }

    var text: String { body.stringValue }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        paint()
    }

    private func paint() {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            layer?.backgroundColor = CodeTheme.card.cgColor
            layer?.borderColor = CodeTheme.line.cgColor
        }
        layer?.borderWidth = 1
        layer?.cornerRadius = 6
        heading.textColor = CodeTheme.muted
        body.textColor = CodeTheme.text
        needsDisplay = true
    }

    func height(width: CGFloat) -> CGFloat {
        let inner = width - Self.padding.width * 2 - 3
        let top = heading.intrinsicContentSize.height
        body.preferredMaxLayoutWidth = inner
        let text = body.attributedStringValue.boundingRect(with: NSSize(width: inner, height: .greatestFiniteMagnitude), options: [.usesLineFragmentOrigin, .usesFontLeading])
        return ceil(Self.padding.height * 2 + top + 2 + text.height)
    }

    func place(at origin: NSPoint, width: CGFloat) {
        let height = height(width: width)
        frame = NSRect(x: origin.x, y: origin.y, width: width, height: height)
        let inner = width - Self.padding.width * 2 - 3
        let top = heading.intrinsicContentSize.height
        heading.frame = NSRect(x: Self.padding.width + 3, y: Self.padding.height, width: inner, height: top)
        body.frame = NSRect(x: Self.padding.width + 3, y: Self.padding.height + top + 2, width: inner, height: height - Self.padding.height * 2 - top - 2)
    }

    override func draw(_ dirtyRect: NSRect) {
        CodeTheme.tintEdge.setFill()
        NSBezierPath(roundedRect: NSRect(x: 0, y: 0, width: 3, height: bounds.height), xRadius: 1.5, yRadius: 1.5).fill()
    }
}
