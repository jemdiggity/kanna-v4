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
        var htmlLike = true
        var alignment: NSTextAlignment = .center
        /// Drawn at `size` regardless of what merman measured (kmux's own labels).
        var fixedSize = false
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
                bounds = bounds.union(label.rect)
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

    mutating func text(_ text: String, in rect: CGRect, size: CGFloat = 14, color: Paint = .role(.text), bold: Bool = false,
                       htmlLike: Bool = true, alignment: NSTextAlignment = .center, fixedSize: Bool = false) {
        guard !text.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        items.append(.text(Label(text: text, rect: rect, size: size, color: color, bold: bold, htmlLike: htmlLike, alignment: alignment, fixedSize: fixedSize)))
    }
}

enum Paint {
    case role(Role)
    case color(CGColor)
}

enum Role {
    case text, line, nodeFill, nodeStroke, clusterFill, clusterStroke, labelBackground, noteFill, noteStroke,
         actorFill, actorStroke, lifeline, frameStroke, frameLabelFill, activationFill, activationStroke, startFill, numberText, shadow
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
        }
        return hex(dark ? night : light, alpha)
    }

    func resolve(_ paint: Paint) -> CGColor {
        switch paint {
        case .role(let role): color(role)
        case .color(let color): color
        }
    }
}
