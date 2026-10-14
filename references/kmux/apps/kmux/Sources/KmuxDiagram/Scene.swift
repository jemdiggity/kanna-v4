import AppKit

/// What a diagram draws: shapes and labels in diagram coordinates (top-left
/// origin, y down, in points at zoom 1). Colors are roles, resolved against
/// the light or dark palette when drawn.
public struct DiagramScene {
    public var size: CGSize
    public var items: [Item] = []

    public enum Item {
        case shape(Shape)
        case text(Label)
    }

    public struct Shape {
        var path: CGPath
        var fill: Paint?
        var stroke: Paint?
        var lineWidth: CGFloat = 1
        var dash: [CGFloat] = []
        var shadow = false
    }

    public struct Label {
        public var text: String
        public var rect: CGRect
        var size: CGFloat = 16
        var color: Paint = .role(.text)
        var bold = false
        var italic = false
        var htmlLike = true
        var alignment: NSTextAlignment = .center
        /// Drawn at `size` regardless of what merman measured (kmux's own labels).
        var fixedSize = false
        /// Turned this many radians (clockwise, as y is down) about the rect's centre.
        var angle: CGFloat = 0

        /// The rect as drawn, turned by `angle`.
        var drawnBounds: CGRect {
            guard angle != 0 else { return rect }
            let turn = CGAffineTransform(translationX: rect.midX, y: rect.midY).rotated(by: angle).translatedBy(x: -rect.midX, y: -rect.midY)
            return rect.applying(turn)
        }
    }

    /// Moves everything so it starts `margin` from the top left and sizes the
    /// scene to what is drawn. merman's bounds come from its own layout pass and
    /// can miss boxes widened by Core Text measurements, cropping the diagram.
    mutating func fit(margin: CGFloat = 8) {
        var bounds = CGRect.null
        for item in items {
            switch item {
            case .shape(let shape):
                let box = shape.path.boundingBoxOfPath
                guard !box.isNull, box.width.isFinite, box.height.isFinite else { continue }
                bounds = bounds.union(box.insetBy(dx: -shape.lineWidth - (shape.shadow ? 4 : 0), dy: -shape.lineWidth - (shape.shadow ? 4 : 0)))
            case .text(let label):
                bounds = bounds.union(label.drawnBounds)
            }
        }
        guard !bounds.isNull else { return }
        var move = CGAffineTransform(translationX: margin - bounds.minX, y: margin - bounds.minY)
        items = items.map { item in
            switch item {
            case .shape(var shape):
                shape.path = shape.path.copy(using: &move) ?? shape.path
                return .shape(shape)
            case .text(var label):
                label.rect = label.rect.applying(move)
                return .text(label)
            }
        }
        size = CGSize(width: ceil(bounds.width + 2 * margin), height: ceil(bounds.height + 2 * margin))
    }

    /// Every label's text, for tests and search.
    public var texts: [String] { items.compactMap { if case .text(let label) = $0 { label.text } else { nil } } }

    mutating func shape(_ path: CGPath, fill: Paint? = nil, stroke: Paint? = nil, lineWidth: CGFloat = 1, dash: [CGFloat] = [], shadow: Bool = false) {
        items.append(.shape(Shape(path: path, fill: fill, stroke: stroke, lineWidth: lineWidth, dash: dash, shadow: shadow)))
    }

    mutating func text(_ text: String, in rect: CGRect, size: CGFloat = 14, color: Paint = .role(.text), bold: Bool = false, italic: Bool = false,
                       htmlLike: Bool = true, alignment: NSTextAlignment = .center, fixedSize: Bool = false, angle: CGFloat = 0) {
        guard !text.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        items.append(.text(Label(text: text, rect: rect, size: size, color: color, bold: bold, italic: italic, htmlLike: htmlLike, alignment: alignment,
                                 fixedSize: fixedSize, angle: angle)))
    }
}

enum Paint {
    case role(Role)
    case color(CGColor)
}

enum Role {
    case text, line, nodeFill, nodeStroke, clusterFill, clusterStroke, labelBackground, noteFill, noteStroke,
         actorFill, actorStroke, lifeline, frameStroke, frameLabelFill, activationFill, activationStroke, startFill, numberText, shadow
    /// Class diagrams: Mermaid 12 gives each class one of 12 colours, in declaration order.
    case classFill(Int), classStroke(Int)
    /// Git graphs: each branch's colour (8, repeating), its label's text, and its highlight commits.
    case branch(Int), branchText(Int), branchHighlight(Int)
    case branchLine, commitLabelFill, commitInner, tagFill, tagStroke
}

/// Mermaid 12's default look ("neo" with the "redux-color" theme), and a
/// dark counterpart.
struct Palette {
    let dark: Bool

    func color(_ role: Role) -> CGColor {
        func hex(_ value: UInt32, _ alpha: CGFloat = 1) -> CGColor {
            CGColor(srgbRed: CGFloat(value >> 16 & 0xff) / 255, green: CGFloat(value >> 8 & 0xff) / 255, blue: CGFloat(value & 0xff) / 255, alpha: alpha)
        }
        let light: UInt32, night: UInt32
        var alpha: CGFloat = 1
        switch role {
        case .text: (light, night) = (0x1f1d2b, 0xe6e4f0)
        case .line: (light, night) = (0x000000, 0xd4d2e0)
        case .nodeFill, .actorFill, .frameLabelFill: (light, night) = (0xffffff, 0x1e1d26)
        case .nodeStroke, .actorStroke: (light, night) = (0x28253d, 0xc9c6e0)
        case .clusterFill: (light, night) = (0xf9f9fb, 0x26252e)
        case .clusterStroke: (light, night) = (0xbdbccc, 0x5a5870)
        case .labelBackground: (light, night) = (0xcccccc, 0x4a4858); alpha = 0.9
        case .noteFill: (light, night) = (0xfff5ad, 0x4a4520)
        case .noteStroke: (light, night) = (0xfacc15, 0xa08a1a)
        case .lifeline: (light, night) = (0x28253d, 0x8a8898)
        case .frameStroke: (light, night) = (0x28253d, 0xa9a6c0)
        case .activationFill: (light, night) = (0xcccccc, 0x45434f)
        case .activationStroke: (light, night) = (0x28253d, 0xa9a6c0)
        case .startFill: (light, night) = (0x28253d, 0xd4d2e0)
        case .numberText: (light, night) = (0xffffff, 0x1e1d26)
        case .shadow: (light, night) = (0x000000, 0x000000); alpha = dark ? 0.5 : 0.18
        case .classStroke(let i): (light, night) = (Self.classColors[i % 12].stroke, Self.classColors[i % 12].stroke)
        case .classFill(let i):
            // Dark: a sixth of the stroke colour over the dark node fill.
            let stroke = Self.classColors[i % 12].stroke
            (light, night) = (Self.classColors[i % 12].fill, Self.mix(stroke, over: 0x1e1d26, 0.16))
        case .branch(let i): (light, night) = (Self.branchColors[i % 8].light, Self.branchColors[i % 8].dark)
        case .branchText(let i): (light, night) = ([0, 3].contains(i % 8) ? 0xffffff : 0x000000, [0, 3].contains(i % 8) ? 0xffffff : 0x000000)
        case .branchHighlight(let i): (light, night) = (Self.branchColors[i % 8].highlight, Self.branchColors[i % 8].darkHighlight)
        case .branchLine: (light, night) = (0x333333, 0x8a8898)
        case .commitLabelFill: (light, night) = (0xffffde, 0x3a3820); alpha = dark ? 0.7 : 0.5
        case .commitInner: (light, night) = (0xececff, 0x1e1d26)
        case .tagFill: (light, night) = (0xececff, 0x2e2d45)
        case .tagStroke: (light, night) = (0xc8c8f0, 0x6a68a0)
        }
        return hex(dark ? night : light, alpha)
    }

    /// Mermaid 12's class colours (borderColorArray, bkgColorArray).
    static let classColors: [(stroke: UInt32, fill: UInt32)] = [
        (0xe879f9, 0xfdf4ff), (0x2dd4bf, 0xf0fdfa), (0xfb923c, 0xfff7ed), (0x22d3ee, 0xecfeff), (0x4ade80, 0xf0fdf4), (0xa78bfa, 0xf5f3ff),
        (0xf87171, 0xfef2f2), (0xfacc15, 0xfefce8), (0x818cf8, 0xeef2ff), (0xa3e635, 0xf7fee7), (0x38bdf8, 0xf0f9ff), (0xfb7185, 0xfff1f2),
    ]

    /// Mermaid 12's git branch colours (git0-7 and their inverses for highlights), with lighter dark counterparts.
    static let branchColors: [(light: UInt32, dark: UInt32, highlight: UInt32, darkHighlight: UInt32)] = [
        (0x0000ec, 0x5555ff, 0x131300, 0xd8d860), (0xdede00, 0xd4d400, 0x0000a1, 0x6060ff), (0x9dec00, 0x9dec00, 0x310093, 0x9a60ff),
        (0x0076ec, 0x3399ff, 0x934900, 0xff9a40), (0x00ecec, 0x00d4d4, 0x930000, 0xff6060), (0x00ec76, 0x00d46a, 0x930049, 0xff60a0),
        (0xec00ec, 0xe040e0, 0x009300, 0x40d040), (0xec0000, 0xf04040, 0x009393, 0x40d0d0),
    ]

    static func mix(_ top: UInt32, over bottom: UInt32, _ amount: Double) -> UInt32 {
        [16, 8, 0].reduce(0) { value, shift in
            let a = Double(top >> UInt32(shift) & 0xff), b = Double(bottom >> UInt32(shift) & 0xff)
            return value | UInt32((a * amount + b * (1 - amount)).rounded()) << UInt32(shift)
        }
    }

    func resolve(_ paint: Paint) -> CGColor {
        switch paint {
        case .role(let role): color(role)
        case .color(let color): color
        }
    }
}
