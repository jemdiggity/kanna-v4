import AppKit

/// Draws what sits behind markdown text: code block panels, the bar beside a
/// quote and horizontal rules (the `.kmuxBlock` attribute).
final class MarkdownLayoutManager: NSLayoutManager {
    override func drawBackground(forGlyphRange glyphsToShow: NSRange, at origin: NSPoint) {
        super.drawBackground(forGlyphRange: glyphsToShow, at: origin)
        guard let storage = textStorage, let container = textContainers.first, storage.length > 0 else { return }
        let shown = characterRange(forGlyphRange: glyphsToShow, actualGlyphRange: nil)
        let whole = NSRange(location: 0, length: storage.length)
        var drawn = Set<Int>()
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        var location = shown.location
        while location < NSMaxRange(shown) {
            var range = NSRange()
            let value = storage.attribute(.kmuxBlock, at: location, longestEffectiveRange: &range, in: whole)
            location = max(location + 1, NSMaxRange(range))
            guard let block = value as? BlockDecoration, drawn.insert(block.id).inserted else { continue }
            let used = usedRect(for: range)
            let left = origin.x + container.lineFragmentPadding + block.indent
            let right = origin.x + container.size.width - container.lineFragmentPadding
            switch block.kind {
            case .code:
                let pad: CGFloat = 8
                let panel = NSRect(x: left, y: origin.y + used.minY - pad, width: right - left, height: used.height + pad * 2)
                MarkdownTheme.panel.setFill()
                NSBezierPath(roundedRect: panel, xRadius: 6, yRadius: 6).fill()
            case .quote:
                MarkdownTheme.line.setFill()
                NSRect(x: left + 2, y: origin.y + used.minY, width: 3, height: used.height).fill()
            case .rule:
                MarkdownTheme.line.setFill()
                NSRect(x: left, y: origin.y + used.midY.rounded(), width: right - left, height: 1).fill()
            }
        }
    }

    /// The union of the used rects of the lines holding `range`.
    private func usedRect(for range: NSRange) -> NSRect {
        let glyphs = glyphRange(forCharacterRange: range, actualCharacterRange: nil)
        var rect = NSRect.null
        enumerateLineFragments(forGlyphRange: glyphs) { _, used, _, _, _ in rect = rect.union(used) }
        return rect.isNull ? .zero : rect
    }
}
