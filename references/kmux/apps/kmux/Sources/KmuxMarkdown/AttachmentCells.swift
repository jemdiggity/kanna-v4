import AppKit
import KmuxDiagram

/// A Mermaid diagram in the text, drawn as vectors from its laid-out scene:
/// sharp at any magnification. Too wide for the text, it shrinks to fit.
final class DiagramCell: NSTextAttachmentCell {
    // Read by TextKit's layout callbacks; set once, never changed.
    nonisolated(unsafe) let scene: DiagramScene
    nonisolated let diagramType: String

    init(scene: DiagramScene, type: String) {
        self.scene = scene
        self.diagramType = type
        super.init()
    }

    required init(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    nonisolated private func scale(width: CGFloat) -> CGFloat {
        guard scene.size.width > 0 else { return 1 }
        return min(1, max(0.1, width) / scene.size.width)
    }

    override func cellSize() -> NSSize { scene.size }

    override func cellFrame(for textContainer: NSTextContainer, proposedLineFragment lineFrag: NSRect,
                            glyphPosition position: NSPoint, characterIndex charIndex: Int) -> NSRect {
        let scale = scale(width: lineFrag.width - textContainer.lineFragmentPadding * 2)
        return NSRect(x: 0, y: 0, width: scene.size.width * scale, height: scene.size.height * scale)
    }

    override func cellBaselineOffset() -> NSPoint { .zero }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView?) {
        guard let context = NSGraphicsContext.current?.cgContext, scene.size.width > 0 else { return }
        let dark = MarkdownTheme.isDark(controlView?.effectiveAppearance ?? NSApp.effectiveAppearance)
        context.saveGState()
        context.translateBy(x: cellFrame.minX, y: cellFrame.minY)
        let scale = cellFrame.width / scene.size.width
        context.scaleBy(x: scale, y: scale)
        scene.draw(in: context, dark: dark)
        context.restoreGState()
    }

    override func wantsToTrackMouse() -> Bool { false }
}

/// A local image in the text, at its natural size, shrunk to fit the text width.
final class ImageCell: NSTextAttachmentCell {
    nonisolated let picture: NSImage

    init(image: NSImage) {
        picture = image
        super.init(imageCell: image)
    }

    required init(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func cellSize() -> NSSize { picture.size }

    override func cellFrame(for textContainer: NSTextContainer, proposedLineFragment lineFrag: NSRect,
                            glyphPosition position: NSPoint, characterIndex charIndex: Int) -> NSRect {
        let natural = cellSize()
        let width = max(1, lineFrag.width - textContainer.lineFragmentPadding * 2)
        let scale = natural.width > width ? width / natural.width : 1
        return NSRect(x: 0, y: 0, width: natural.width * scale, height: natural.height * scale)
    }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView?) {
        picture.draw(in: cellFrame, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: [.interpolation: NSImageInterpolation.high])
    }

    override func wantsToTrackMouse() -> Bool { false }
}
