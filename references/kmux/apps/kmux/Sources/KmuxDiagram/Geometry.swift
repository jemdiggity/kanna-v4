import CoreGraphics
import Foundation

/// JSON helpers and the shapes all diagram types share.
extension Dictionary where Key == String, Value == Any {
    func double(_ key: String) -> CGFloat? { (self[key] as? NSNumber).map { CGFloat($0.doubleValue) } }
    func string(_ key: String) -> String? { self[key] as? String }
    func dict(_ key: String) -> [String: Any]? { self[key] as? [String: Any] }
    func list(_ key: String) -> [[String: Any]] { self[key] as? [[String: Any]] ?? [] }
    /// A layout box: merman gives centre x, y plus width and height.
    var box: CGRect? {
        guard let x = double("x"), let y = double("y"), let w = double("width"), let h = double("height") else { return nil }
        return CGRect(x: x - w / 2, y: y - h / 2, width: w, height: h)
    }
    var points: [CGPoint] { list("points").compactMap { p in p.double("x").flatMap { x in p.double("y").map { CGPoint(x: x, y: $0) } } } }
}

/// Moves a diagram so its bounds start at the margin and gives the scene size.
struct Frame {
    let offset: CGPoint
    let size: CGSize
    static let margin: CGFloat = 8

    init(layout: [String: Any]) {
        let b = layout.dict("bounds") ?? [:]
        let minX = b.double("min_x") ?? 0, minY = b.double("min_y") ?? 0
        let maxX = b.double("max_x") ?? 0, maxY = b.double("max_y") ?? 0
        offset = CGPoint(x: Self.margin - minX, y: Self.margin - minY)
        size = CGSize(width: maxX - minX + 2 * Self.margin, height: maxY - minY + 2 * Self.margin)
    }

    func rect(_ r: CGRect) -> CGRect { r.offsetBy(dx: offset.x, dy: offset.y) }
    func point(_ p: CGPoint) -> CGPoint { CGPoint(x: p.x + offset.x, y: p.y + offset.y) }
}

enum Paths {
    static func rect(_ r: CGRect, radius: CGFloat = 0) -> CGPath {
        radius > 0 ? CGPath(roundedRect: r, cornerWidth: min(radius, r.width / 2), cornerHeight: min(radius, r.height / 2), transform: nil) : CGPath(rect: r, transform: nil)
    }

    static func polygon(_ points: [CGPoint]) -> CGPath {
        let path = CGMutablePath()
        path.addLines(between: points)
        path.closeSubpath()
        return path
    }

    static func line(_ points: [CGPoint]) -> CGPath {
        let path = CGMutablePath()
        path.addLines(between: points)
        return path
    }

    /// d3's curveBasis, which Mermaid draws edges with: a B-spline through the
    /// layout's points, starting and ending exactly on the first and last.
    static func basis(_ p: [CGPoint]) -> CGPath {
        let path = CGMutablePath()
        guard let first = p.first else { return path }
        path.move(to: first)
        guard p.count > 2 else { p.dropFirst().forEach { path.addLine(to: $0) }; return path }
        func mix(_ a: CGPoint, _ b: CGPoint, _ c: CGPoint) -> CGPoint { CGPoint(x: (a.x + 4 * b.x + c.x) / 6, y: (a.y + 4 * b.y + c.y) / 6) }
        path.addLine(to: CGPoint(x: (5 * p[0].x + p[1].x) / 6, y: (5 * p[0].y + p[1].y) / 6))
        for i in 1..<(p.count - 1) {
            let a = p[i - 1], b = p[i], c = p[i + 1]
            path.addCurve(to: mix(a, b, c),
                          control1: CGPoint(x: (2 * a.x + b.x) / 3, y: (2 * a.y + b.y) / 3),
                          control2: CGPoint(x: (a.x + 2 * b.x) / 3, y: (a.y + 2 * b.y) / 3))
        }
        let a = p[p.count - 2], b = p[p.count - 1]
        path.addCurve(to: b, control1: CGPoint(x: (2 * a.x + b.x) / 3, y: (2 * a.y + b.y) / 3),
                      control2: CGPoint(x: (a.x + 2 * b.x) / 3, y: (a.y + 2 * b.y) / 3))
        return path
    }

    /// A polyline with its corners rounded: how Mermaid 12 draws ELK's
    /// right-angled edges.
    static func rounded(_ p: [CGPoint], radius: CGFloat = 5) -> CGPath {
        let path = CGMutablePath()
        guard let first = p.first else { return path }
        path.move(to: first)
        guard p.count > 2 else { p.dropFirst().forEach { path.addLine(to: $0) }; return path }
        for i in 1..<(p.count - 1) {
            let a = p[i - 1], b = p[i], c = p[i + 1]
            let r = min(radius, hypot(b.x - a.x, b.y - a.y) / 2, hypot(c.x - b.x, c.y - b.y) / 2)
            path.addArc(tangent1End: b, tangent2End: c, radius: r)
        }
        path.addLine(to: p[p.count - 1])
        return path
    }

    /// Pulls the last point back by `distance` (so a line stops at an arrowhead's base).
    static func shorten(_ points: [CGPoint], end distance: CGFloat) -> [CGPoint] {
        guard points.count >= 2, distance > 0 else { return points }
        var p = points
        let a = p[p.count - 2], b = p[p.count - 1]
        let length = hypot(b.x - a.x, b.y - a.y)
        guard length > distance else { return p }
        p[p.count - 1] = CGPoint(x: b.x - (b.x - a.x) / length * distance, y: b.y - (b.y - a.y) / length * distance)
        return p
    }

    enum Head { case none, filled, open, cross, circle }

    /// An arrowhead pointing from `from` to `tip`.
    static func head(_ kind: Head, tip: CGPoint, from: CGPoint, size: CGFloat = 10) -> (CGPath, filled: Bool)? {
        let angle = atan2(tip.y - from.y, tip.x - from.x)
        func at(_ back: CGFloat, _ side: CGFloat) -> CGPoint {
            CGPoint(x: tip.x - back * cos(angle) - side * sin(angle), y: tip.y - back * sin(angle) + side * cos(angle))
        }
        switch kind {
        case .none: return nil
        case .filled: return (polygon([tip, at(size, size / 2), at(size, -size / 2)]), true)
        case .open: return (line([at(size, size / 2), tip, at(size, -size / 2)]), false)
        case .cross:
            let c = at(size / 2, 0), r = size * 0.35
            let path = CGMutablePath()
            path.move(to: CGPoint(x: c.x - r, y: c.y - r)); path.addLine(to: CGPoint(x: c.x + r, y: c.y + r))
            path.move(to: CGPoint(x: c.x - r, y: c.y + r)); path.addLine(to: CGPoint(x: c.x + r, y: c.y - r))
            return (path, false)
        case .circle:
            let c = at(size * 0.4, 0), r = size * 0.4
            return (CGPath(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r), transform: nil), true)
        }
    }

    /// Mermaid's `style` / `classDef` CSS ("fill:#f96,stroke:#333") as colors.
    static func css(_ declarations: [String]) -> (fill: CGColor?, stroke: CGColor?, text: CGColor?, dash: Bool) {
        var fill: CGColor?, stroke: CGColor?, text: CGColor?, dash = false
        for declaration in declarations.flatMap({ $0.split(separator: ";").map(String.init) }) {
            let parts = declaration.split(separator: ":", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces).lowercased() }
            guard parts.count == 2 else { continue }
            switch parts[0] {
            case "fill": fill = color(parts[1])
            case "stroke": stroke = color(parts[1])
            case "color": text = color(parts[1])
            case "stroke-dasharray": dash = true
            default: break
            }
        }
        return (fill, stroke, text, dash)
    }

    static func color(_ css: String) -> CGColor? {
        var hex = css.trimmingCharacters(in: .whitespaces)
        if hex.hasPrefix("rgb") {
            let numbers = hex.components(separatedBy: CharacterSet(charactersIn: "0123456789.").inverted).compactMap(Double.init)
            guard numbers.count >= 3 else { return nil }
            return CGColor(srgbRed: numbers[0] / 255, green: numbers[1] / 255, blue: numbers[2] / 255, alpha: numbers.count > 3 ? numbers[3] : 1)
        }
        guard hex.hasPrefix("#") else { return nil }
        hex.removeFirst()
        if hex.count == 3 { hex = hex.map { "\($0)\($0)" }.joined() }
        guard hex.count == 6, let value = UInt32(hex, radix: 16) else { return nil }
        return CGColor(srgbRed: CGFloat(value >> 16 & 0xff) / 255, green: CGFloat(value >> 8 & 0xff) / 255, blue: CGFloat(value & 0xff) / 255, alpha: 1)
    }
}
