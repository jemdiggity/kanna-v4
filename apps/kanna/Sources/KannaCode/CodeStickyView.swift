import AppKit

@MainActor
final class CodeStickyView: NSView {
    struct Row {
        let line: Int
        let text: NSAttributedString
    }

    var rows: [Row] = [] { didSet { needsDisplay = true } }
    var gutterWidth: CGFloat = 0
    var textX: CGFloat = 0
    var lineHeight: CGFloat = 16
    var font = CodeTheme.font(size: CodeTheme.fontSize)
    var onPick: ((Int) -> Void)?

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        guard !rows.isEmpty else { return }
        CodeTheme.background.setFill()
        bounds.fill()
        for (index, row) in rows.enumerated() {
            let y = CGFloat(index) * lineHeight
            let label = String(row.line) as NSString
            let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: CodeTheme.number]
            let size = label.size(withAttributes: attributes)
            label.draw(at: NSPoint(x: gutterWidth - CodeTheme.gutterPadding - size.width, y: y + (lineHeight - size.height) / 2), withAttributes: attributes)
            NSGraphicsContext.saveGraphicsState()
            NSRect(x: gutterWidth, y: y, width: bounds.width - gutterWidth, height: lineHeight).clip()
            row.text.draw(at: NSPoint(x: textX, y: y))
            NSGraphicsContext.restoreGraphicsState()
        }
        CodeTheme.line.setFill()
        NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill()
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let index = Int(point.y / lineHeight)
        if rows.indices.contains(index) { onPick?(rows[index].line) }
    }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .pointingHand)
    }
}
