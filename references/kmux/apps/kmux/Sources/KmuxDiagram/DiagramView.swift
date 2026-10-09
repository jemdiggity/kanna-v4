import AppKit

extension DiagramScene {
    /// Draws the scene into `context`, whose user space is diagram points
    /// with y down. Returns how many labels had to shrink to fit (0 normally).
    @discardableResult
    public func draw(in context: CGContext, dark: Bool) -> Int {
        let palette = Palette(dark: dark)
        var shrunk = 0
        for item in items {
            switch item {
            case .shape(let shape):
                context.saveGState()
                if shape.shadow {
                    // y is down in the diagram but up in Core Graphics' shadow offset.
                    context.setShadow(offset: CGSize(width: 2, height: -2), blur: 3, color: palette.color(.shadow))
                }
                context.addPath(shape.path)
                if let fill = shape.fill {
                    context.setFillColor(palette.resolve(fill))
                    if shape.stroke != nil {
                        context.drawPath(using: .fill)
                        context.setShadow(offset: .zero, blur: 0, color: nil)
                        context.addPath(shape.path)
                    } else { context.fillPath() }
                }
                if let stroke = shape.stroke {
                    context.setStrokeColor(palette.resolve(stroke))
                    context.setLineWidth(shape.lineWidth)
                    context.setLineDash(phase: 0, lengths: shape.dash)
                    context.setLineJoin(.round)
                    context.strokePath()
                }
                context.restoreGState()
            case .text(let label):
                if LabelText.draw(label.text, in: label.rect, size: label.size, color: palette.resolve(label.color), bold: label.bold,
                                  htmlLike: label.htmlLike, alignment: label.alignment, context: context) {
                    shrunk += 1
                }
            }
        }
        return shrunk
    }

    /// The scene as PNG data, at `scale` pixels per point, on a plain page background.
    public func png(scale: CGFloat = 2, dark: Bool = false) -> Data? {
        let width = Int(ceil(size.width * scale)), height = Int(ceil(size.height * scale))
        guard width > 0, height > 0, let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                                                             space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                                             bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        context.setFillColor(dark ? CGColor(gray: 0.1, alpha: 1) : CGColor(gray: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        context.translateBy(x: 0, y: CGFloat(height))
        context.scaleBy(x: scale, y: -scale)
        draw(in: context, dark: dark)
        guard let image = context.makeImage() else { return nil }
        return NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
    }
}

/// A Mermaid diagram drawn natively. Set `source`; the view sizes itself to
/// the diagram times `zoom` and redraws as vectors, so it stays sharp at any
/// zoom. Invalid or unsupported diagrams show their message and the source.
public final class DiagramView: NSView {
    public var source = "" { didSet { if source != oldValue { relayout() } } }
    public var zoom: CGFloat = 1 { didSet { if zoom != oldValue { invalidateIntrinsicContentSize(); needsDisplay = true } } }
    public private(set) var layoutResult: Result<DiagramLayout, DiagramError>?

    public override var isFlipped: Bool { true }

    public override init(frame: NSRect) {
        super.init(frame: frame)
        layerContentsRedrawPolicy = .onSetNeedsDisplay
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    private func relayout() {
        do { layoutResult = .success(try Diagram.layout(source: source)) } catch let error as DiagramError { layoutResult = .failure(error) } catch {
            layoutResult = .failure(DiagramError(message: "\(error)"))
        }
        invalidateIntrinsicContentSize()
        needsDisplay = true
    }

    /// The diagram's size at this zoom, or room for the message.
    public override var intrinsicContentSize: NSSize {
        if case .success(let layout) = layoutResult, let scene = layout.scene {
            return NSSize(width: scene.size.width * zoom, height: scene.size.height * zoom)
        }
        return NSSize(width: NSView.noIntrinsicMetric, height: messageHeight * zoom)
    }

    private var message: String? {
        switch layoutResult {
        case .failure(let error): "Diagram error: \(error.message)"
        case .success(let layout) where layout.scene == nil: "Unsupported diagram type: \(layout.type)"
        default: nil
        }
    }

    private var messageHeight: CGFloat {
        28 + CGFloat(source.components(separatedBy: "\n").count) * 16
    }

    public override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        let dark = effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        context.saveGState()
        context.scaleBy(x: zoom, y: zoom)
        if let message {
            let palette = Palette(dark: dark)
            LabelText.draw(message, in: CGRect(x: 8, y: 4, width: bounds.width / zoom - 16, height: 20), size: 13,
                           color: CGColor(srgbRed: 0.82, green: 0.14, blue: 0.18, alpha: 1), htmlLike: false, alignment: .left, context: context)
            var y: CGFloat = 28
            for line in source.components(separatedBy: "\n") {
                let font = LabelText.font(size: 12, monospaced: true)
                let attributed = NSAttributedString(string: line, attributes: [.font: font, NSAttributedString.Key(kCTForegroundColorAttributeName as String): palette.color(.text)])
                context.textMatrix = CGAffineTransform(scaleX: 1, y: -1)
                context.textPosition = CGPoint(x: 8, y: y + 12)
                CTLineDraw(CTLineCreateWithAttributedString(attributed), context)
                y += 16
            }
        } else if case .success(let layout) = layoutResult, let scene = layout.scene {
            scene.draw(in: context, dark: dark)
        }
        context.restoreGState()
    }

    public override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }
}
